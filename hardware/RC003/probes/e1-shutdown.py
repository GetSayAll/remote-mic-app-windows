"""确认采集实例是否还在，并看看有没有干净退出的办法。

约束（AGENTS.md）：不得强杀正在连接的应用（强杀会留下未正常关闭的 BLE 会话，
是链路僵死主要诱因）。优先走应用自身退出路径。

做法：
  1. 列出所有 sayall-windows-app.exe 进程（pid / 内存 / 命令行）
  2. 找窗口标题，尝试发 WM_CLOSE（等价于点标题栏 X = 应用自身退出路径）
  3. 等 3 秒复查，确认是否已退出
不发送 TerminateProcess。
"""
import ctypes
from ctypes import wintypes
import subprocess, time, json

out = []
def p(s=""):
    out.append(str(s))

# --- 1. 枚举进程 ---
ps = subprocess.run(
    ["powershell", "-NoProfile", "-Command",
     "Get-Process -Name sayall-windows-app -ErrorAction SilentlyContinue | "
     "Select-Object Id,WorkingSet64,StartTime | ConvertTo-Json -Compress"],
    capture_output=True, text=True, timeout=60)
p("Get-Process 原始输出: %r" % ps.stdout.strip())
procs = []
if ps.stdout.strip():
    d = json.loads(ps.stdout)
    procs = d if isinstance(d, list) else [d]
p("存活实例数: %d" % len(procs))
for x in procs:
    p("  pid=%s ws=%sMB start=%s" % (x["Id"], x["WorkingSet64"] // 1048576, x.get("StartTime")))
p()

if not procs:
    p("没有存活的采集实例，无需操作。")
    open("e1-shutdown.out", "w", encoding="utf-8").write("\n".join(out))
    print("WROTE e1-shutdown.out")
    raise SystemExit(0)

# --- 2. 找窗口并发 WM_CLOSE ---
user32 = ctypes.windll.user32
EnumWindows = user32.EnumWindows
EnumWindowsProc = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

target_pids = {x["Id"] for x in procs}
found = []

def cb(hwnd, lparam):
    pid = wintypes.DWORD()
    user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    if pid.value in target_pids:
        n = user32.GetWindowTextLengthW(hwnd)
        buf = ctypes.create_unicode_buffer(n + 1)
        user32.GetWindowTextW(hwnd, buf, n + 1)
        cls = ctypes.create_unicode_buffer(256)
        user32.GetClassNameW(hwnd, cls, 256)
        vis = bool(user32.IsWindowVisible(hwnd))
        found.append(dict(hwnd=hwnd, pid=pid.value, title=buf.value,
                          cls=cls.value, visible=vis))
    return True

EnumWindows(EnumWindowsProc(cb), 0)
p("属于采集实例的顶层窗口: %d" % len(found))
for w in found:
    p("  hwnd=0x%08X pid=%d visible=%s class=%r title=%r"
      % (w["hwnd"], w["pid"], w["visible"], w["cls"], w["title"]))
p()

WM_CLOSE = 0x0010
sent = []
for w in found:
    if w["visible"] and w["title"]:
        r = user32.PostMessageW(w["hwnd"], WM_CLOSE, 0, 0)
        sent.append((w["hwnd"], w["title"], r))
        p("PostMessage WM_CLOSE -> hwnd=0x%08X title=%r ret=%s" % (w["hwnd"], w["title"], r))

if not sent:
    p("没有可发 WM_CLOSE 的带标题可见窗口（应用可能是托盘/无窗模式）。")
    p("=> 交由用户从托盘菜单退出，不做任何强制动作。")

time.sleep(3)

# --- 3. 复查 ---
ps2 = subprocess.run(
    ["powershell", "-NoProfile", "-Command",
     "(Get-Process -Name sayall-windows-app -ErrorAction SilentlyContinue | "
     "Measure-Object).Count"],
    capture_output=True, text=True, timeout=60)
p()
p("3 秒后存活实例数: %s" % ps2.stdout.strip())

open("e1-shutdown.out", "w", encoding="utf-8").write("\n".join(out))
print("WROTE e1-shutdown.out")
