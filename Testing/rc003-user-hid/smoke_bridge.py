"""Explicit local packaged-helper smoke test. No keyboard/action injection."""
import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
from pathlib import Path
import re
import secrets
import socket
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
import capture


def launch(path, port, token):
    class ExecuteInfo(ctypes.Structure):
        _fields_ = [("cbSize", wintypes.DWORD), ("fMask", wintypes.ULONG),
                    ("hwnd", wintypes.HWND), ("lpVerb", wintypes.LPCWSTR),
                    ("lpFile", wintypes.LPCWSTR), ("lpParameters", wintypes.LPCWSTR),
                    ("lpDirectory", wintypes.LPCWSTR), ("nShow", ctypes.c_int),
                    ("hInstApp", wintypes.HINSTANCE), ("lpIDList", ctypes.c_void_p),
                    ("lpClass", wintypes.LPCWSTR), ("hkeyClass", wintypes.HKEY),
                    ("dwHotKey", wintypes.DWORD), ("hIcon", wintypes.HANDLE), ("hProcess", wintypes.HANDLE)]
    execute = ctypes.WinDLL("shell32", use_last_error=True).ShellExecuteExW
    execute.argtypes = [ctypes.POINTER(ExecuteInfo)]
    execute.restype = wintypes.BOOL
    info = ExecuteInfo(cbSize=ctypes.sizeof(ExecuteInfo), fMask=0x140,
                       lpVerb="runas", lpFile=str(path), lpParameters=f"--port {port} --token {token}", nShow=0)
    if not execute(ctypes.byref(info)):
        raise RuntimeError("helper_launch_denied")
    return info.hProcess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--helper", type=Path, required=True)
    parser.add_argument("--seconds", type=int, default=20, choices=range(5, 31))
    parser.add_argument("--disconnect", action="store_true")
    args = parser.parse_args()
    target = capture.discover()
    peer = target.service.lower().removeprefix(capture.HARDWARE + "_")
    if not re.fullmatch(r"[0-9a-f]{12}", peer):
        raise RuntimeError("peer_suffix_not_available")
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    kernel.WaitForSingleObject.restype = wintypes.DWORD
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    process = None
    ready_at = None
    finished = None
    events, buffer, sequence = [], b"", 0
    token = secrets.token_hex(32)
    try:
        with socket.socket() as listener:
            listener.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
            listener.bind(("127.0.0.1", 0))
            listener.listen(1)
            listener.settimeout(45)
            process = launch(args.helper.resolve(), listener.getsockname()[1], token)
            connection, _ = listener.accept()
        with connection:
            connection.settimeout(0.2)
            deadline, next_lease, stop_sent = time.monotonic() + 75, time.monotonic(), False
            authenticated = False
            while time.monotonic() < deadline:
                now = time.monotonic()
                if authenticated and not stop_sent and now >= next_lease:
                    connection.sendall(b'{"kind":"lease"}\n')
                    next_lease = now + 1
                if ready_at is not None and now - ready_at >= args.seconds and not stop_sent:
                    if args.disconnect:
                        break
                    connection.sendall(b'{"kind":"stop"}\n')
                    stop_sent = True
                try:
                    chunk = connection.recv(2048)
                except socket.timeout:
                    continue
                if not chunk: break
                buffer += chunk
                if len(buffer) > 8192: raise RuntimeError("ipc_too_large")
                while b"\n" in buffer:
                    line, buffer = buffer.split(b"\n", 1)
                    message = json.loads(line)
                    if message.get("seq") != sequence: raise RuntimeError("sequence_mismatch")
                    sequence += 1
                    kind = message.get("kind")
                    if not authenticated:
                        if kind != "hello" or not secrets.compare_digest(message.get("token", ""), token):
                            raise RuntimeError("authentication_failed")
                        authenticated = True
                        connection.sendall((json.dumps({"kind": "accepted", "peer": peer}) + "\n").encode())
                    elif kind == "ready":
                        ready_at = now
                        print(json.dumps({"event": "packaged_bridge_ready", "physical_actions": False}), flush=True)
                    elif kind == "stopped":
                        finished = {"reason": message.get("reason"), "cleanup": message.get("cleanup")}
                    events.append(kind)
                if finished is not None: break
        exited = process is not None and kernel.WaitForSingleObject(process, 20000) == 0
        elapsed = round(time.monotonic() - ready_at, 2) if ready_at else 0
        result = {"ready": ready_at is not None, "heartbeat_count": events.count("heartbeat"),
                  "state_count": events.count("state"), "elapsed_seconds": elapsed,
                  "helper_exited": exited, "stop_result": finished, "disconnect_test": args.disconnect,
                  "target_still_present": capture.target_current(target),
                  "helper_sha256": hashlib.sha256(args.helper.read_bytes()).hexdigest(),
                  "sayall_mapping_actions_verified": False}
        print(json.dumps(result), flush=True)
        return 0 if result["ready"] and exited and (args.disconnect or finished == {"reason": "stop_requested", "cleanup": True}) else 1
    finally:
        if process: kernel.CloseHandle(process)


if __name__ == "__main__":
    raise SystemExit(main())
