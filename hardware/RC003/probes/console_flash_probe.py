"""Measure the console-window flash when the RC003 helper is started by Task Scheduler.

Why Task Scheduler: that is how the product starts sayall-helper.exe -- both on
app startup (auto trigger, when the switch is already on) and right after the
user switches 全按键支持 on. A console-subsystem helper gets a console window
(shown by the console host; the helper can only hide it right after startup),
which is the reported "one-frame black window" flash. A GUI-subsystem helper
never allocates a console at all, so there is nothing to flash.

Detectors (two, independent):
  * Fast window sampler: a background thread enumerates visible top-level
    windows continuously (~1 ms cadence) and records every window handle that
    *becomes* visible during the run, with class/pid/title, first-seen offset
    and visible span (last pass in which it was still visible).
  * Process sampling: conhost.exe process ids are diffed across the run, so
    "a console was allocated at all" is detectable on its own.

Per run the probe starts the throwaway task with `schtasks /run` (the exact
trigger the app uses; no elevation involved here) and reports:
  - newly visible windows during the run (console-class ones are the flash)
  - new conhost.exe process ids
  - whether sayall-helper.exe was actually observed running

Baseline (console-subsystem exe): a new console-class window and/or a new
conhost appears. Fixed (GUI-subsystem exe): both stay empty while the helper is
still observed running -- that is the pass criterion.

Usage:
  python console_flash_probe.py --exe <helper.exe> --out <evidence.json> [--runs 2]
                               [--args "--selftest --hide-window"] [--seconds 8]

Note: the probe task is registered WITHOUT /rl highest (this session has no
admin rights). Console allocation is independent of the integrity level, so the
flash mechanism is faithfully reproduced; the elevated product path differs only
in the token.
"""

import argparse
import ctypes
import hashlib
import json
import os
import subprocess
import sys
import threading
import time
from ctypes import wintypes

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

EnumWindowsProc = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

user32.EnumWindows.argtypes = [EnumWindowsProc, wintypes.LPARAM]
user32.EnumWindows.restype = wintypes.BOOL
user32.IsWindowVisible.argtypes = [wintypes.HWND]
user32.IsWindowVisible.restype = wintypes.BOOL
user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
user32.GetWindowThreadProcessId.restype = wintypes.DWORD
user32.GetClassNameW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
user32.GetClassNameW.restype = ctypes.c_int
user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
user32.GetWindowTextW.restype = ctypes.c_int

kernel32.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
kernel32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
kernel32.Process32FirstW.argtypes = [wintypes.HANDLE, ctypes.c_void_p]
kernel32.Process32FirstW.restype = wintypes.BOOL
kernel32.Process32NextW.argtypes = [wintypes.HANDLE, ctypes.c_void_p]
kernel32.Process32NextW.restype = wintypes.BOOL
kernel32.CloseHandle.argtypes = [wintypes.HANDLE]

TH32CS_SNAPPROCESS = 0x00000002
INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value

CONSOLE_CLASSES = ("ConsoleWindowClass", "CASCADIA_HOSTING_WINDOW_CLASS", "PseudoConsoleWindow")

_sampler_stop = threading.Event()
_records_lock = threading.Lock()
_records = {}   # hwnd -> record dict


class PROCESSENTRY32W(ctypes.Structure):
    _fields_ = [
        ("dwSize", wintypes.DWORD),
        ("cntUsage", wintypes.DWORD),
        ("th32ProcessID", wintypes.DWORD),
        ("th32DefaultHeapID", ctypes.c_void_p),
        ("th32ModuleID", wintypes.DWORD),
        ("cntThreads", wintypes.DWORD),
        ("th32ParentProcessID", wintypes.DWORD),
        ("pcPriClassBase", ctypes.c_long),
        ("dwFlags", wintypes.DWORD),
        ("szExeFile", ctypes.c_wchar * 260),
    ]


def process_snapshot():
    """pid -> exe name (lowercase) for all processes."""
    snap = kernel32.CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
    if not snap or snap == INVALID_HANDLE_VALUE:
        return {}
    out = {}
    entry = PROCESSENTRY32W()
    entry.dwSize = ctypes.sizeof(PROCESSENTRY32W)
    ok = kernel32.Process32FirstW(snap, ctypes.byref(entry))
    while ok:
        out[entry.th32ProcessID] = entry.szExeFile.lower()
        ok = kernel32.Process32NextW(snap, ctypes.byref(entry))
    kernel32.CloseHandle(snap)
    return out


def process_snapshot_full():
    """pid -> (exe name lowercase, parent pid) for all processes."""
    snap = kernel32.CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
    if not snap or snap == INVALID_HANDLE_VALUE:
        return {}
    out = {}
    entry = PROCESSENTRY32W()
    entry.dwSize = ctypes.sizeof(PROCESSENTRY32W)
    ok = kernel32.Process32FirstW(snap, ctypes.byref(entry))
    while ok:
        out[entry.th32ProcessID] = (entry.szExeFile.lower(), entry.th32ParentProcessID)
        ok = kernel32.Process32NextW(snap, ctypes.byref(entry))
    kernel32.CloseHandle(snap)
    return out


def window_info(hwnd):
    pid = wintypes.DWORD(0)
    user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    cls = ctypes.create_unicode_buffer(64)
    user32.GetClassNameW(hwnd, cls, 64)
    title = ctypes.create_unicode_buffer(256)
    user32.GetWindowTextW(hwnd, title, 256)
    return {"hwnd": int(hwnd or 0), "pid": int(pid.value),
            "class": cls.value, "title": title.value[:120]}


def visible_top_level_windows():
    """[(hwnd, pid, class, title)] for every currently visible top-level window."""
    found = []

    def cb(hwnd, _lparam):
        if not user32.IsWindowVisible(hwnd):
            return True
        info = window_info(hwnd)
        found.append((info["hwnd"], info["pid"], info["class"], info["title"]))
        return True

    user32.EnumWindows(EnumWindowsProc(cb), 0)
    return found


def start_window_sampler():
    """Track visibility transitions for every top-level window, continuously."""

    def loop():
        prev = set()
        while not _sampler_stop.is_set():
            now = time.perf_counter()
            cur = {}
            for hwnd, pid, cls, title in visible_top_level_windows():
                cur[hwnd] = (pid, cls, title)
            with _records_lock:
                for hwnd, (pid, cls, title) in cur.items():
                    rec = _records.get(hwnd)
                    if rec is None:
                        _records[hwnd] = {"t_first": now, "t_last": now, "hwnd": hwnd,
                                          "pid": pid, "class": cls, "title": title}
                    else:
                        rec["t_last"] = now
            prev = set(cur)
            time.sleep(0.001)

    t = threading.Thread(target=loop, name="window-sampler", daemon=True)
    t.start()
    return t


def reset_sampler_records():
    with _records_lock:
        _records.clear()


def sampler_records():
    with _records_lock:
        return [dict(r) for r in _records.values()]


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def register_task(task, exe, args):
    action = '"%s" %s' % (exe, args)
    proc = subprocess.run(
        ["schtasks", "/create", "/tn", task, "/tr", action,
         "/sc", "once", "/st", "00:00", "/f"],
        capture_output=True, text=True, encoding="gbk", errors="replace")
    return proc.returncode == 0, (proc.stdout or "") + (proc.stderr or "")


def delete_task(task):
    subprocess.run(["schtasks", "/delete", "/tn", task, "/f"],
                   capture_output=True, text=True, encoding="gbk", errors="replace")


def ancestry_chain(pid, new_procs, ps_before, ps_after):
    """Walk parent links (new-process map first, then before/after snapshots)."""
    chain = []
    seen = set()
    cur = pid
    while cur and cur not in seen:
        seen.add(cur)
        chain.append(cur)
        parent = 0
        if cur in new_procs:
            parent = new_procs[cur]["parent_pid"]
        elif cur in ps_after:
            parent = ps_after[cur][1]
        elif cur in ps_before:
            parent = ps_before[cur][1]
        cur = parent if parent else 0
    return chain


def run_once(task, seconds, target_name):
    before_ps = process_snapshot_full()
    before_conhost = {p for p, (n, _pp) in before_ps.items() if n == "conhost.exe"}
    pre_visible = {w[0] for w in visible_top_level_windows()}

    reset_sampler_records()
    t0 = time.perf_counter()
    proc = subprocess.run(["schtasks", "/run", "/tn", task],
                          capture_output=True, text=True, encoding="gbk", errors="replace")

    new_conhost = set()
    new_procs = {}
    helper_pid = 0
    helper_start_ms = None
    helper_end_ms = None
    helper_seen = False
    helper_gone_at = None
    tick = 0
    while time.perf_counter() - t0 < seconds:
        now = time.perf_counter() - t0
        if tick % 20 == 0:
            ps = process_snapshot_full()
            for p, (n, ppid) in ps.items():
                if p not in before_ps and p not in new_procs:
                    # 首次见到即记录（不能用最后一次覆盖：t_ms 要表示"何时出现"）。
                    new_procs[p] = {"pid": p, "name": n, "parent_pid": ppid,
                                    "t_ms": round(now * 1000)}
                    if n == target_name and helper_pid == 0:
                        # 只认**本次新拉起**的目标进程：机器上可能本来就有同名
                        # 进程在跑（用户 App 会话的助手），按"存在"判会认错对象。
                        helper_pid = p
                        helper_start_ms = round(now * 1000)
                if n == "conhost.exe" and p not in before_conhost:
                    new_conhost.add(p)
            alive = helper_pid != 0 and helper_pid in ps
            if alive:
                helper_seen = True
                helper_gone_at = None
            elif helper_seen:
                helper_gone_at = helper_gone_at or now
                helper_end_ms = round(now * 1000)
                if helper_gone_at and now - helper_gone_at > 0.4 and now > 1.0:
                    break
        tick += 1
        time.sleep(0.005)

    t_end = time.perf_counter()
    time.sleep(0.3)  # let a trailing visibility transition be sampled
    ps_after = process_snapshot_full()

    def related_to_helper(pid):
        if helper_pid and pid == helper_pid:
            return True
        if not helper_pid:
            return False
        return helper_pid in ancestry_chain(pid, new_procs, before_ps, ps_after)

    for rec in new_procs.values():
        rec["ancestry"] = ancestry_chain(rec["pid"], new_procs, before_ps, ps_after)
        rec["helper_related"] = related_to_helper(rec["pid"])

    flash = []
    for rec in sampler_records():
        if rec["hwnd"] in pre_visible:
            continue
        flash.append({
            "hwnd": rec["hwnd"], "pid": rec["pid"], "class": rec["class"],
            "title": rec["title"],
            "first_ms": round((rec["t_first"] - t0) * 1000),
            "last_ms": round((rec["t_last"] - t0) * 1000),
            "visible_ms": max(0, round((rec["t_last"] - rec["t_first"]) * 1000)),
            "ancestry": ancestry_chain(rec["pid"], new_procs, before_ps, ps_after),
            "helper_related": related_to_helper(rec["pid"]),
        })
    flash.sort(key=lambda r: r["first_ms"])

    return {
        "helper_seen": helper_seen,
        "helper_pid": helper_pid,
        "helper_start_ms": helper_start_ms,
        "helper_end_ms": helper_end_ms,
        "new_conhost_pids": sorted(new_conhost),
        "new_processes": [new_procs[k] for k in sorted(new_procs)],
        "new_visible_windows": flash,
        "observed_s": round(t_end - t0, 2),
        "schtasks_rc": proc.returncode,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--args", default="--selftest --hide-window")
    ap.add_argument("--task", default="SayAllFlashProbe")
    ap.add_argument("--target-name", default="sayall-helper.exe",
                    help="exe name (lowercase) of the process the task launches; "
                         "used to attribute console hosts/windows to this run")
    ap.add_argument("--runs", type=int, default=2)
    ap.add_argument("--seconds", type=float, default=12.0)
    ap.add_argument("--out", default=None)
    opts = ap.parse_args()

    exe = os.path.abspath(opts.exe)
    if not os.path.isfile(exe):
        print("FATAL: exe not found: %s" % exe, flush=True)
        return 2

    report = []
    def emit(text):
        print(text, flush=True)
        report.append(text)

    emit("console_flash_probe")
    emit("time     : %s" % time.strftime("%Y-%m-%d %H:%M:%S"))
    emit("exe      : %s" % exe)
    emit("sha256   : %s" % sha256(exe))
    emit("args     : %s" % opts.args)
    emit("target   : %s" % opts.target_name)
    emit("task     : %s" % opts.task)

    ok, detail = register_task(opts.task, exe, opts.args)
    emit("register : ok=%s %s" % (ok, detail.strip().replace("\n", " | ")))
    if not ok:
        return 2

    start_window_sampler()
    time.sleep(0.5)

    results = []
    try:
        for i in range(1, opts.runs + 1):
            r = run_once(opts.task, opts.seconds, opts.target_name)
            r["run"] = i
            results.append(r)
            console_new = [w for w in r["new_visible_windows"] if w["class"] in CONSOLE_CLASSES]
            related_windows = [w for w in r["new_visible_windows"] if w["helper_related"]]
            related_console = [w for w in console_new if w["helper_related"]]
            related_conhost = [p for p in r["new_conhost_pids"]
                               if any(proc["pid"] == p and proc["helper_related"]
                                      for proc in r["new_processes"])]
            emit("run %d: helper_seen=%s pid=%s start=%sms new_conhost=%s helper_related_conhost=%s"
                 % (i, r["helper_seen"], r["helper_pid"], r["helper_start_ms"],
                    r["new_conhost_pids"] or "[]", related_conhost or "[]"))
            emit("        windows: total=%d console_class=%d helper_related=%d helper_related_console=%d observed_s=%s"
                 % (len(r["new_visible_windows"]), len(console_new), len(related_windows),
                    len(related_console), r["observed_s"]))
            for p in r["new_processes"]:
                emit("        new process: pid=%s name=%s parent_pid=%s helper_related=%s t=%sms"
                     % (p["pid"], p["name"], p["parent_pid"], p["helper_related"], p["t_ms"]))
            for w in r["new_visible_windows"]:
                emit("        window shown: t=%sms..%sms (visible %sms) hwnd=%s pid=%s class=%s helper_related=%s title=%r"
                     % (w["first_ms"], w["last_ms"], w["visible_ms"], w["hwnd"], w["pid"],
                        w["class"], w["helper_related"], w["title"]))
            time.sleep(1.0)
    finally:
        delete_task(opts.task)
        emit("task deleted")

    def run_related_conhost(r):
        return [p for p in r["new_conhost_pids"]
                if any(proc["pid"] == p and proc["helper_related"] for proc in r["new_processes"])]

    any_flash = any(
        any(w["class"] in CONSOLE_CLASSES and w["helper_related"]
            for w in r["new_visible_windows"]) or run_related_conhost(r)
        for r in results)
    emit("VERDICT: console_flash_detected=%s" % ("true" if any_flash else "false"))

    if opts.out:
        payload = {"exe": exe, "sha256": sha256(exe), "args": opts.args, "task": opts.task,
                   "target_name": opts.target_name,
                   "runs": results, "flash_detected": any_flash, "text": report}
        with open(opts.out, "w", encoding="utf-8") as f:
            json.dump(payload, f, ensure_ascii=False, indent=2)
            f.write("\n")
        emit("evidence : %s" % os.path.abspath(opts.out))
    return 0


if __name__ == "__main__":
    sys.exit(main())
