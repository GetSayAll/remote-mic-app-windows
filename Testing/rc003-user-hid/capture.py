"""GPL-3.0-only. Explicit, time-limited RC003 HID diagnostics; see README.md.

--probe never imports Frida or attaches. --capture requires an elevated process.
No kernel driver, boot changes, remote listener, keystroke injection, or autostart.
"""
from __future__ import annotations

import argparse
import ctypes
from ctypes import wintypes
from dataclasses import dataclass
from datetime import datetime, timezone
import json
import importlib.util
import os
from pathlib import Path
import queue
import re
import signal
import subprocess
import sys
import threading
import time

FRIDA_VERSION = "17.15.3"
HID_SERVICE = "{00001812-0000-1000-8000-00805f9b34fb}"
HARDWARE = HID_SERVICE + "_dev_vid&012717_pid&32b8_rev&00a4"
HARDWARE_RE = re.compile(re.escape(HARDWARE) + r"(?:_[0-9a-f]{12})?", re.I)
ENUM_ROOT = r"SYSTEM\CurrentControlSet\Enum\BTHLEDevice"
DIAGNOSTIC_KEY = r"Device Parameters\WUDFDiagnosticInfo"
BUTTONS = frozenset({"back", "volume_up", "volume_down"})
SCOPES = frozenset({"rc003", "proxy_unverified"})
BASE = Path(__file__).resolve().parent


class ProbeError(Exception):
    def __init__(self, reason: str, code: int = 0):
        super().__init__(reason)
        self.reason = reason
        self.code = code


def is_admin() -> bool:
    return os.name == "nt" and bool(ctypes.windll.shell32.IsUserAnAdmin())


def api_error(reason: str):
    raise ProbeError(reason, ctypes.get_last_error())


def registry_children(key):
    import winreg
    index = 0
    while True:
        try:
            yield winreg.EnumKey(key, index)
        except OSError as error:
            if error.winerror == 259:
                return
            raise
        index += 1


def present_node(instance: str) -> bool:
    cfg = ctypes.WinDLL("cfgmgr32", use_last_error=True)
    locate = cfg.CM_Locate_DevNodeW
    locate.argtypes = [ctypes.POINTER(wintypes.DWORD), wintypes.LPWSTR, wintypes.ULONG]
    locate.restype = wintypes.ULONG
    status = cfg.CM_Get_DevNode_Status
    status.argtypes = [ctypes.POINTER(wintypes.ULONG), ctypes.POINTER(wintypes.ULONG),
                       wintypes.DWORD, wintypes.ULONG]
    status.restype = wintypes.ULONG
    node, flags, problem = wintypes.DWORD(), wintypes.ULONG(), wintypes.ULONG()
    if locate(ctypes.byref(node), ctypes.create_unicode_buffer(instance), 0) != 0:
        return False
    return status(ctypes.byref(flags), ctypes.byref(problem), node, 0) == 0 and bool(flags.value & 8)


@dataclass(frozen=True)
class Target:
    pid: int
    registry_path: str
    service: str
    device: str
    shared_hid_count: int


def discover() -> Target:
    if os.name != "nt" or ctypes.sizeof(ctypes.c_void_p) != 8:
        raise ProbeError("windows_x64_required")
    import winreg
    candidates = []
    hosts: dict[int, list[str]] = {}
    try:
        with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, ENUM_ROOT) as root:
            for service in registry_children(root):
                if not service.lower().startswith(HID_SERVICE):
                    continue
                with winreg.OpenKey(root, service) as branch:
                    for instance in registry_children(branch):
                        identity = service + "\\" + instance
                        try:
                            with winreg.OpenKey(root, identity + "\\" + DIAGNOSTIC_KEY) as key:
                                pid, kind = winreg.QueryValueEx(key, "HostPid")
                            if kind not in {winreg.REG_DWORD, winreg.REG_QWORD} or not 0 < pid <= 0xFFFFFFFF:
                                continue
                            if not present_node("BTHLEDevice\\" + identity):
                                continue
                            hosts.setdefault(pid, []).append(identity)
                            if HARDWARE_RE.fullmatch(service):
                                candidates.append((pid, identity, service))
                        except FileNotFoundError:
                            continue
    except FileNotFoundError:
        raise ProbeError("rc003_hid_service_missing") from None
    if len(candidates) != 1:
        raise ProbeError("rc003_target_missing_or_ambiguous", len(candidates))
    pid, identity, service = candidates[0]
    query = ctypes.WinDLL("kernel32", use_last_error=True).QueryDosDeviceW
    query.argtypes = [wintypes.LPCWSTR, wintypes.LPWSTR, wintypes.DWORD]
    query.restype = wintypes.DWORD
    names = ctypes.create_unicode_buffer(262144)
    length = query(None, names, len(names))
    if not length:
        api_error("device_names_query_failed")
    targets = set()
    for name in names[:length].split("\0"):
        if not name.lower().startswith("bthledevice#" + service.lower() + "#"):
            continue
        buffer = ctypes.create_unicode_buffer(4096)
        if query(name, buffer, len(buffer)):
            targets.add(buffer.value.lower())
    if len(targets) != 1:
        raise ProbeError("rc003_device_missing_or_ambiguous", len(targets))
    return Target(pid, ENUM_ROOT + "\\" + identity, service, targets.pop(), len(hosts[pid]))


def target_current(target: Target) -> bool:
    import winreg
    try:
        with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, target.registry_path + "\\" + DIAGNOSTIC_KEY) as key:
            pid, _ = winreg.QueryValueEx(key, "HostPid")
        return pid == target.pid and present_node("BTHLEDevice\\" + target.registry_path[len(ENUM_ROOT) + 1:])
    except OSError:
        return False


class Luid(ctypes.Structure):
    _fields_ = [("low", wintypes.DWORD), ("high", wintypes.LONG)]


class TokenPrivilege(ctypes.Structure):
    _fields_ = [("count", wintypes.DWORD), ("luid", Luid), ("attributes", wintypes.DWORD)]


class DebugPrivilege:
    """Enable only the current elevated helper's existing right; restore on exit."""
    def __init__(self):
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.advapi = ctypes.WinDLL("advapi32", use_last_error=True)
        self.kernel.GetCurrentProcess.restype = wintypes.HANDLE
        self.kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        self.advapi.OpenProcessToken.argtypes = [wintypes.HANDLE, wintypes.DWORD,
                                               ctypes.POINTER(wintypes.HANDLE)]
        self.advapi.OpenProcessToken.restype = wintypes.BOOL
        self.advapi.LookupPrivilegeValueW.argtypes = [wintypes.LPCWSTR, wintypes.LPCWSTR,
                                                    ctypes.POINTER(Luid)]
        self.advapi.LookupPrivilegeValueW.restype = wintypes.BOOL
        self.advapi.AdjustTokenPrivileges.argtypes = [wintypes.HANDLE, wintypes.BOOL,
            ctypes.POINTER(TokenPrivilege), wintypes.DWORD, ctypes.POINTER(TokenPrivilege),
            ctypes.POINTER(wintypes.DWORD)]
        self.advapi.AdjustTokenPrivileges.restype = wintypes.BOOL
        self.token = wintypes.HANDLE()
        self.previous = TokenPrivilege()
        self.enabled = False
        if not self.advapi.OpenProcessToken(self.kernel.GetCurrentProcess(), 0x28, ctypes.byref(self.token)):
            api_error("helper_token_open_failed")
        try:
            desired = TokenPrivilege(count=1, attributes=2)
            if not self.advapi.LookupPrivilegeValueW(None, "SeDebugPrivilege", ctypes.byref(desired.luid)):
                api_error("debug_privilege_lookup_failed")
            returned = wintypes.DWORD()
            ctypes.set_last_error(0)
            if not self.advapi.AdjustTokenPrivileges(self.token, False, ctypes.byref(desired),
                    ctypes.sizeof(self.previous), ctypes.byref(self.previous), ctypes.byref(returned)):
                api_error("debug_privilege_enable_failed")
            if ctypes.get_last_error() != 0:
                api_error("debug_privilege_not_assigned")
            self.enabled = True
        except BaseException:
            self.close()
            raise

    def close(self):
        restored = True
        if self.token:
            if self.enabled:
                ctypes.set_last_error(0)
                restored = bool(self.advapi.AdjustTokenPrivileges(self.token, False,
                    ctypes.byref(self.previous), 0, None, None)) and ctypes.get_last_error() == 0
            self.kernel.CloseHandle(self.token)
            self.token = None
        return restored


class HostHandle:
    def __init__(self, pid: int):
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        self.kernel.OpenProcess.restype = wintypes.HANDLE
        self.kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        self.kernel.CloseHandle.restype = wintypes.BOOL
        self.kernel.QueryFullProcessImageNameW.argtypes = [wintypes.HANDLE, wintypes.DWORD,
                                                        wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD)]
        self.kernel.QueryFullProcessImageNameW.restype = wintypes.BOOL
        self.kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
        self.kernel.WaitForSingleObject.restype = wintypes.DWORD
        self.handle = self.kernel.OpenProcess(0x1000 | 0x100000, False, pid)
        if not self.handle:
            api_error("host_query_denied")
        try:
            path = ctypes.create_unicode_buffer(32768)
            size = wintypes.DWORD(len(path))
            if not self.kernel.QueryFullProcessImageNameW(self.handle, 0, path, ctypes.byref(size)):
                api_error("host_path_query_failed")
            windows = ctypes.create_unicode_buffer(32768)
            self.kernel.GetWindowsDirectoryW.argtypes = [wintypes.LPWSTR, wintypes.UINT]
            self.kernel.GetWindowsDirectoryW.restype = wintypes.UINT
            if not self.kernel.GetWindowsDirectoryW(windows, len(windows)):
                api_error("windows_directory_query_failed")
            expected = Path(windows.value) / "System32" / "WUDFHost.exe"
            if os.path.normcase(path.value) != os.path.normcase(str(expected)):
                raise ProbeError("unexpected_host_executable")
            self.path = expected
            self.kernel.GetProcessTimes.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.FILETIME),
                ctypes.POINTER(wintypes.FILETIME), ctypes.POINTER(wintypes.FILETIME), ctypes.POINTER(wintypes.FILETIME)]
            self.kernel.GetProcessTimes.restype = wintypes.BOOL
            times = [wintypes.FILETIME() for _ in range(4)]
            if not self.kernel.GetProcessTimes(self.handle, *(ctypes.byref(value) for value in times)):
                api_error("host_start_time_query_failed")
            self.identity = str(pid) + "-" + str((times[0].dwHighDateTime << 32) | times[0].dwLowDateTime)
        except BaseException:
            self.close()
            raise

    def alive(self) -> bool:
        return self.kernel.WaitForSingleObject(self.handle, 0) == 0x102

    def verify_signature(self):
        powershell = self.path.parent / "WindowsPowerShell" / "v1.0" / "powershell.exe"
        escaped = str(self.path).replace("'", "''")
        command = "$s=Get-AuthenticodeSignature -LiteralPath '" + escaped + "'; "
        command += "[pscustomobject]@{valid=($s.Status -eq 'Valid'); microsoft=($s.SignerCertificate.Subject -match 'O=Microsoft Corporation(?:,|$)')} | ConvertTo-Json -Compress"
        try:
            result = subprocess.run([str(powershell), "-NoProfile", "-NonInteractive", "-Command", command],
                                    capture_output=True, text=True, timeout=20,
                                    creationflags=subprocess.CREATE_NO_WINDOW, check=False)
            data = json.loads(result.stdout)
            if result.returncode or data.get("valid") is not True or data.get("microsoft") is not True:
                raise ProbeError("host_signature_not_verified")
        except (subprocess.TimeoutExpired, ValueError):
            raise ProbeError("host_signature_check_failed") from None

    def inspect(self):
        mitigation = self.kernel.GetProcessMitigationPolicy
        mitigation.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, ctypes.c_size_t]
        mitigation.restype = wintypes.BOOL
        result = {"host_alive": self.alive()}
        for name, policy in (("dynamic_code", 2), ("signature", 8), ("image_load", 10)):
            flags = wintypes.DWORD()
            ok = mitigation(self.handle, policy, ctypes.byref(flags), ctypes.sizeof(flags))
            result[name + "_flags"] = flags.value if ok else None
            result[name + "_query_error"] = 0 if ok else ctypes.get_last_error()
        info = self.kernel.GetProcessInformation
        info.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
        info.restype = wintypes.BOOL
        level = wintypes.DWORD()
        ok = info(self.handle, 7, ctypes.byref(level), ctypes.sizeof(level))
        result["protection_level"] = level.value if ok else None
        return result

    def close(self):
        if self.handle:
            self.kernel.CloseHandle(self.handle)
            self.handle = None


class CaptureLock:
    def __init__(self):
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.kernel.CreateMutexW.argtypes = [ctypes.c_void_p, wintypes.BOOL, wintypes.LPCWSTR]
        self.kernel.CreateMutexW.restype = wintypes.HANDLE
        self.kernel.ReleaseMutex.argtypes = [wintypes.HANDLE]
        self.kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        ctypes.set_last_error(0)
        self.handle = self.kernel.CreateMutexW(None, True, "Local\\SayAllRc003UserHidDiagnostic")
        if not self.handle:
            api_error("capture_lock_denied")
        if ctypes.get_last_error() == 183:
            self.kernel.CloseHandle(self.handle)
            self.handle = None
            raise ProbeError("capture_already_running")

    def close(self):
        if self.handle:
            self.kernel.ReleaseMutex(self.handle)
            self.kernel.CloseHandle(self.handle)
            self.handle = None


class Ledger:
    def __init__(self, emit):
        self.emit = emit
        self.states = {}
        self.cycles = {scope: dict.fromkeys(sorted(BUTTONS), 0) for scope in SCOPES}
        self.rejected = 0

    def state(self, message):
        stream, scope, active = message.get("stream"), message.get("scope"), message.get("active")
        if (type(stream) is not int or not 1 <= stream <= 16 or
                not isinstance(scope, str) or scope not in SCOPES or
                not isinstance(active, list) or len(active) > 3 or
                any(not isinstance(button, str) or button not in BUTTONS for button in active) or
                len(set(active)) != len(active)):
            self.rejected += 1
            return
        existing = self.states.get(stream)
        if existing is not None and existing[0] != scope:
            self.rejected += 1
            return
        previous = set() if existing is None else existing[1]
        current = set(active)
        self.states[stream] = (scope, current)
        for edge, buttons in (("up", previous - current), ("down", current - previous)):
            for button in sorted(buttons):
                if edge == "up":
                    self.cycles[scope][button] += 1
                self.emit("key", scope=scope, stream=stream, button=button, edge=edge)

    def finish(self):
        held = sum(len(current) for _, current in self.states.values())
        self.states.clear()
        return {"complete_cycles": self.cycles, "held_at_stop": held, "rejected_messages": self.rejected,
                "physical_keys_verified": False, "sayall_integrated": False}


class Log:
    def __init__(self, path: Path | None):
        self.file = path.open("x", encoding="utf-8") if path else None
        self.lock = threading.Lock()

    def emit(self, event, **fields):
        record = {"time": datetime.now(timezone.utc).isoformat(timespec="milliseconds"),
                  "schema": 1, "event": event, **fields}
        line = json.dumps(record, sort_keys=True)
        with self.lock:
            if self.file:
                self.file.write(line + "\n")
                self.file.flush()
            if sys.stdout:
                print(line, flush=True)

    def close(self):
        if self.file:
            self.file.close()


def security_state():
    import winreg
    try:
        with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE,
                            r"SYSTEM\CurrentControlSet\Control\SecureBoot\State") as key:
            secure = winreg.QueryValueEx(key, "UEFISecureBootEnabled")[0] == 1
    except OSError:
        secure = None
    try:
        with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE,
                            r"SYSTEM\CurrentControlSet\Services\Rc003HidFilter"):
            driver = True
    except FileNotFoundError:
        driver = False
    return {"secure_boot_enabled": secure, "experimental_filter_present": driver}


def timed_frida_call(frida, seconds, call):
    cancel = frida.Cancellable()
    timer = threading.Timer(seconds, cancel.cancel)
    timer.daemon = True
    timer.start()
    try:
        with cancel:
            return call()
    finally:
        timer.cancel()


def capture(args, target: Target, log: Log):
    if not is_admin():
        raise ProbeError("administrator_launch_required")
    if security_state()["experimental_filter_present"]:
        raise ProbeError("do_not_stack_filter_and_tap")
    lock = CaptureLock()
    host = session = script = privilege = gadget = None
    ledger = Ledger(log.emit)
    stopped = threading.Event()
    overflow = threading.Event()
    inbox = queue.Queue(maxsize=512)
    reason = "unknown"
    stats = {}
    logged_stats = None
    cleanup_ok = True
    attach_attempted = False
    stop_file = args.log.with_suffix(".stop") if args.log else None
    signal.signal(signal.SIGINT, lambda *_: stopped.set())
    signal.signal(signal.SIGTERM, lambda *_: stopped.set())

    def message_received(message, _data):
        try:
            inbox.put_nowait(message)
        except queue.Full:
            overflow.set()
            stopped.set()

    try:
        privilege = DebugPrivilege()
        log.emit("debug_privilege", scope="current_helper_only", enabled=True)
        host = HostHandle(target.pid)
        host.verify_signature()
        inspection = host.inspect()
        if (inspection["dynamic_code_flags"] is None or inspection["dynamic_code_flags"] & 1 or
                inspection["signature_flags"] is None or inspection["signature_flags"] & 3 or
                inspection["protection_level"] != 0xFFFFFFFE):
            raise ProbeError("host_security_policy_not_compatible")
        if not host.alive() or not target_current(target):
            raise ProbeError("target_changed_before_attach")
        log.emit("attach_started", host_pid=target.pid, frida_version=FRIDA_VERSION,
                 host_signature_verified=True, capture_seconds=args.seconds, backend=args.backend)
        if args.backend == "gadget":
            spec = importlib.util.spec_from_file_location("sayall_gadget_backend", BASE / "gadget_backend.py")
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            gadget = module.Backend(BASE, target, host, log, message_received, args.seconds)
            gadget.start()
        else:
            import frida
            if frida.__version__ != FRIDA_VERSION:
                raise ProbeError("unexpected_frida_version")
            device = frida.get_local_device()
            attach_attempted = True
            session = timed_frida_call(frida, 15, lambda: device.attach(target.pid, persist_timeout=0))
            session.on("detached", lambda *_: stopped.set())
            if not host.alive() or not target_current(target):
                raise ProbeError("target_changed_after_attach")
            script = timed_frida_call(frida, 10, lambda: session.create_script(
                (BASE / "hid_tap.js").read_text(encoding="utf-8"), name="sayall-rc003-diagnostic", runtime="qjs"))
            script.on("message", message_received)
            timed_frida_call(frida, 10, script.load)
            timed_frida_call(frida, 10, lambda: script.exports_sync.start({
                "device": target.device, "seconds": args.seconds,
                "proxy_diagnostics": target.shared_hid_count == 1}))
        log.emit("capture_ready", host_pid=target.pid, shared_hid_count=target.shared_hid_count,
                 proxy_diagnostics=target.shared_hid_count == 1, keyboard_injection=False)
        deadline = time.monotonic() + args.seconds
        next_health = time.monotonic() + 2
        last_heartbeat = time.monotonic()
        reason = "deadline"
        while time.monotonic() < deadline:
            if stopped.is_set():
                reason = "message_overflow" if overflow.is_set() else "session_detached_or_cancelled"
                break
            if stop_file and stop_file.exists():
                reason = "stop_requested"
                break
            if time.monotonic() >= next_health:
                if gadget is not None and gadget.failed.is_set():
                    reason = "gadget_connection_lost"
                    break
                if not host.alive() or not target_current(target):
                    reason = "host_or_device_changed"
                    break
                if time.monotonic() - last_heartbeat > 12:
                    reason = "heartbeat_lost"
                    break
                next_health = time.monotonic() + 2
            try:
                message = inbox.get(timeout=0.2)
            except queue.Empty:
                continue
            if message.get("type") != "send" or not isinstance(message.get("payload"), dict):
                reason = "agent_script_error"
                break
            payload = message["payload"]
            kind = payload.get("kind")
            if kind == "state":
                ledger.state(payload)
            elif kind in {"ready", "stats", "stopped"}:
                last_heartbeat = time.monotonic()
                stats = {key: value for key, value in payload.items()
                         if key in {"total_ioctl", "ioctl", "direct", "proxy", "unrelated", "pending", "errors",
                                    "wrong_length", "unspecified_length", "invalid_report", "reports", "stream_limit", "streams"}
                         and type(value) is int and value >= 0}
                if kind != "stats" or stats != logged_stats:
                    log.emit("agent_" + kind, **stats)
                    logged_stats = stats
                if kind == "stopped":
                    break
            elif kind == "stream" and payload.get("scope") in SCOPES:
                log.emit("stream_detected", scope=payload["scope"])
            elif kind == "completion":
                fields = {key: value for key, value in payload.items()
                          if key in {"io_status", "information", "declared_length", "returned_status"}
                          and type(value) is int and -1 <= value <= 0xFFFFFFFF}
                log.emit("completion_metadata", **fields)
    except BaseException as error:
        candidate = getattr(error, "reason", "capture_exception")
        reason = candidate if isinstance(candidate, str) and re.fullmatch(r"[a-z0-9_]{1,100}", candidate) else "capture_exception"
        log.emit("capture_error", reason=reason, error_type=type(error).__name__,
                 error_code=getattr(error, "code", 0))
    finally:
        if gadget is not None:
            cleanup_ok = gadget.stop() and cleanup_ok
        if attach_attempted and session is None:
            # A failed attach can still have loaded native runtime code.
            cleanup_ok = False
            log.emit("cleanup", phase="failed_attach_runtime", result="unknown")
        if script is not None:
            for phase, call in (("stop_hook", script.exports_sync.stop), ("unload_script", script.unload)):
                try:
                    timed_frida_call(frida, 5, call)
                    log.emit("cleanup", phase=phase, result="passed")
                except Exception as error:
                    cleanup_ok = False
                    log.emit("cleanup", phase=phase, result="unknown", error_type=type(error).__name__)
        if session is not None:
            try:
                timed_frida_call(frida, 5, session.detach)
                log.emit("cleanup", phase="detach_session", result="passed")
            except Exception as error:
                cleanup_ok = False
                log.emit("cleanup", phase="detach_session", result="unknown", error_type=type(error).__name__)
        alive = host.alive() if host is not None else None
        if host is not None:
            host.close()
        if privilege is not None:
            restored = privilege.close()
            cleanup_ok = cleanup_ok and restored
            log.emit("cleanup", phase="restore_helper_privilege", result="passed" if restored else "unknown")
        lock.close()
        log.emit("capture_finished", reason=reason, cleanup_confirmed=cleanup_ok,
                 host_alive=alive, **ledger.finish(), **security_state())
    return 0 if reason in {"deadline", "stop_requested"} and cleanup_ok and alive else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_mutually_exclusive_group(required=True)
    modes.add_argument("--probe", action="store_true")
    modes.add_argument("--capture", action="store_true")
    modes.add_argument("--inspect-host", action="store_true")
    parser.add_argument("--seconds", type=int, default=90)
    parser.add_argument("--backend", choices=("frida", "gadget"), default="frida")
    parser.add_argument("--log", type=Path)
    args = parser.parse_args()
    if not 15 <= args.seconds <= 180:
        parser.error("seconds must be between 15 and 180")
    log = Log(args.log)
    try:
        mode = "capture" if args.capture else "inspect_host" if args.inspect_host else "probe"
        log.emit("started", elevated=is_admin(), mode=mode, helper_pid=os.getpid())
        target = discover()
        log.emit("preflight", host_pid=target.pid, target_count=1, live_device_count=1,
                 shared_hid_count=target.shared_hid_count, **security_state())
        if args.probe:
            log.emit("probe_finished", result="passed", attached=False, physical_keys_verified=False)
            return 0
        if args.inspect_host:
            if not is_admin():
                raise ProbeError("administrator_launch_required")
            privilege = DebugPrivilege()
            host = None
            try:
                host = HostHandle(target.pid)
                host.verify_signature()
                log.emit("host_inspection", host_signature_verified=True, attached=False, **host.inspect())
            finally:
                if host:
                    host.close()
                log.emit("cleanup", phase="restore_helper_privilege",
                         result="passed" if privilege.close() else "unknown")
            return 0
        return capture(args, target, log)
    except Exception as error:
        log.emit("failed", reason=error.reason if isinstance(error, ProbeError) else "preflight_exception",
                 error_type=type(error).__name__, error_code=getattr(error, "code", 0))
        return 1
    finally:
        log.close()


if __name__ == "__main__":
    raise SystemExit(main())
