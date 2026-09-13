"""GPL-3.0-only. Restricted Gadget transport for the optional capture helper.

Uses the pinned official binary and Axonkey's LoadLibrary approach, not a kernel
driver. The DLL remains resident; an admin-only watched script enables an
explicit subsequent test without stacking more Gadget instances in the host.
"""
from __future__ import annotations

import ctypes
from ctypes import wintypes
import hashlib
import json
import lzma
import os
from pathlib import Path
import secrets
import socket
import subprocess
import threading
import time

ARCHIVE_NAME = "frida-gadget-17.15.3-windows-x86_64.dll.xz"
ARCHIVE_SHA256 = "b566d70189b6d551ad8f4e0bea24de08a3d4c0f559bb35b2bdb67d45182240c2"
DLL_SHA256 = "6fca4007b2284c765a6c15c967a741f536b5865bf83867326a54029a3b752748"
DLL_NAME = "SayAllRC003Diagnostic.dll"


class GadgetError(Exception):
    def __init__(self, reason, code=0):
        super().__init__(reason)
        self.reason = reason
        self.code = code


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def windows_error(reason):
    raise GadgetError(reason, ctypes.get_last_error())


def secure_directory(host_identity):
    path = Path(os.environ.get("PROGRAMDATA", r"C:\ProgramData")) / "SayAllRc003HidDiagnostic"
    for entry in (path, *path.parents):
        if entry.exists() and entry.lstat().st_file_attributes & 0x400:
            raise GadgetError("runtime_reparse_point_rejected")
    fresh = not path.exists()
    path.mkdir(exist_ok=True)
    # Only this diagnostic folder is changed. LocalService needs RX, not write.
    script = "$p=$args[0]; $a=Get-Acl -LiteralPath $p; "
    script += "$owner=$a.GetOwner([Security.Principal.SecurityIdentifier]).Value; "
    script += "if ($owner -notin @('S-1-5-18','S-1-5-32-544')) { exit 9 }; "
    script += "$s=New-Object Security.AccessControl.DirectorySecurity; "
    script += "$s.SetSecurityDescriptorSddlForm('O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;LS)'); "
    script += "Set-Acl -LiteralPath $p -AclObject $s -ErrorAction Stop"
    powershell = Path(os.environ["SystemRoot"]) / "System32/WindowsPowerShell/v1.0/powershell.exe"
    result = subprocess.run([str(powershell), "-NoProfile", "-NonInteractive", "-Command",
                             "& { " + script + " } '" + str(path).replace("'", "''") + "'"],
                            capture_output=True, timeout=20, creationflags=subprocess.CREATE_NO_WINDOW)
    if result.returncode:
        raise GadgetError("runtime_acl_not_verified", result.returncode)
    child = path / ("host-" + host_identity)
    if child.exists() and child.lstat().st_file_attributes & 0x400:
        raise GadgetError("runtime_reparse_point_rejected")
    child.mkdir(exist_ok=True)
    return child, fresh


def atomic_write(path, content):
    if path.exists() and path.lstat().st_file_attributes & 0x400:
        raise GadgetError("runtime_file_reparse_point_rejected")
    temporary = path.with_name(path.name + "." + secrets.token_hex(4) + ".tmp")
    with temporary.open("xb") as stream:
        stream.write(content)
    os.replace(temporary, path)


def prepare_runtime(base, parameters, host_identity, pid):
    archive = base / ".assets" / ARCHIVE_NAME
    if not archive.is_file() or sha256(archive) != ARCHIVE_SHA256:
        raise GadgetError("gadget_archive_missing_or_hash_mismatch")
    runtime, _ = secure_directory(host_identity)
    dll = runtime / DLL_NAME
    if dll.exists() and dll.lstat().st_file_attributes & 0x400:
        raise GadgetError("runtime_file_reparse_point_rejected")
    if not dll.exists():
        raw = lzma.decompress(archive.read_bytes(), memlimit=256 * 1024 * 1024)
        if hashlib.sha256(raw).hexdigest() != DLL_SHA256:
            raise GadgetError("gadget_dll_hash_mismatch")
        atomic_write(dll, raw)
    if sha256(dll) != DLL_SHA256:
        raise GadgetError("existing_gadget_dll_hash_mismatch")
    # Check before touching the watched script: a reload can start immediately.
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel.OpenProcess.restype = wintypes.HANDLE
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    process = kernel.OpenProcess(0x410, False, pid)
    if not process:
        windows_error("host_module_query_denied")
    try:
        verify_exclusive_modules(module_map(process), dll)
    finally:
        kernel.CloseHandle(process)
    config = {"interaction": {"type": "script", "path": "diagnostic.js", "on_change": "reload"},
              "runtime": "qjs", "teardown": "minimal"}
    config_path = runtime / "SayAllRC003Diagnostic.config"
    encoded_config = (json.dumps(config, indent=2) + "\n").encode("utf-8")
    if config_path.exists():
        if config_path.lstat().st_file_attributes & 0x400 or config_path.read_bytes() != encoded_config:
            raise GadgetError("existing_gadget_config_mismatch")
    else:
        atomic_write(config_path, encoded_config)
    script = "const RUN_PARAMETERS = " + json.dumps(parameters) + ";\n"
    script += (base / "hid_tap.js").read_text(encoding="utf-8") + "\n"
    script += (base / "gadget_adapter.js").read_text(encoding="utf-8")
    atomic_write(runtime / "diagnostic.js", script.encode("utf-8"))
    return dll


def module_map(process):
    psapi = ctypes.WinDLL("psapi", use_last_error=True)
    enum = psapi.EnumProcessModulesEx
    enum.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.HMODULE), wintypes.DWORD,
                     ctypes.POINTER(wintypes.DWORD), wintypes.DWORD]
    enum.restype = wintypes.BOOL
    name = psapi.GetModuleFileNameExW
    name.argtypes = [wintypes.HANDLE, wintypes.HMODULE, wintypes.LPWSTR, wintypes.DWORD]
    name.restype = wintypes.DWORD
    modules = (wintypes.HMODULE * 2048)()
    used = wintypes.DWORD()
    if not enum(process, modules, ctypes.sizeof(modules), ctypes.byref(used), 2):
        windows_error("host_module_query_failed")
    if used.value > ctypes.sizeof(modules):
        raise GadgetError("host_module_count_exceeded")
    found = {}
    for module in modules[:used.value // ctypes.sizeof(wintypes.HMODULE)]:
        buffer = ctypes.create_unicode_buffer(32768)
        if not name(process, module, buffer, len(buffer)):
            windows_error("host_module_name_query_failed")
        found[os.path.normcase(buffer.value)] = module
    return found


def verify_exclusive_modules(modules, dll):
    own = os.path.normcase(str(dll))
    for path in modules:
        filename = Path(path).name.lower()
        if any(token in filename for token in ("axonkeyrc003hidtap", "frida-gadget", "frida_agent", "frida-agent")):
            raise GadgetError("another_frida_runtime_present")
        if filename == DLL_NAME.lower() and path != own:
            raise GadgetError("gadget_module_path_mismatch")


def load_once(pid, dll, log):
    k = ctypes.WinDLL("kernel32", use_last_error=True)
    k.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    k.OpenProcess.restype = wintypes.HANDLE
    k.CloseHandle.argtypes = [wintypes.HANDLE]
    k.VirtualAllocEx.argtypes = [wintypes.HANDLE, ctypes.c_void_p, ctypes.c_size_t, wintypes.DWORD, wintypes.DWORD]
    k.VirtualAllocEx.restype = ctypes.c_void_p
    k.VirtualFreeEx.argtypes = [wintypes.HANDLE, ctypes.c_void_p, ctypes.c_size_t, wintypes.DWORD]
    k.VirtualFreeEx.restype = wintypes.BOOL
    k.WriteProcessMemory.argtypes = [wintypes.HANDLE, ctypes.c_void_p, ctypes.c_void_p,
                                   ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
    k.WriteProcessMemory.restype = wintypes.BOOL
    k.GetModuleHandleW.argtypes = [wintypes.LPCWSTR]
    k.GetModuleHandleW.restype = wintypes.HMODULE
    k.GetProcAddress.argtypes = [wintypes.HMODULE, ctypes.c_char_p]
    k.GetProcAddress.restype = ctypes.c_void_p
    k.GetModuleHandleExW.argtypes = [wintypes.DWORD, ctypes.c_void_p, ctypes.POINTER(wintypes.HMODULE)]
    k.GetModuleHandleExW.restype = wintypes.BOOL
    k.GetModuleFileNameW.argtypes = [wintypes.HMODULE, wintypes.LPWSTR, wintypes.DWORD]
    k.GetModuleFileNameW.restype = wintypes.DWORD
    k.CreateRemoteThread.argtypes = [wintypes.HANDLE, ctypes.c_void_p, ctypes.c_size_t,
                                     ctypes.c_void_p, ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p]
    k.CreateRemoteThread.restype = wintypes.HANDLE
    k.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    k.WaitForSingleObject.restype = wintypes.DWORD
    process = k.OpenProcess(0x43A, False, pid)
    if not process:
        windows_error("gadget_process_open_denied")
    remote = thread = None
    completed = False
    try:
        modules = module_map(process)
        own = os.path.normcase(str(dll))
        verify_exclusive_modules(modules, dll)
        if own in modules:
            log.emit("gadget_loaded", reused=True, dll_resident=True)
            return
        # Resolve the containing module and RVA; do not assume equal ASLR bases.
        address = k.GetProcAddress(k.GetModuleHandleW("kernel32.dll"), b"LoadLibraryW")
        containing = wintypes.HMODULE()
        if not address or not k.GetModuleHandleExW(6, address, ctypes.byref(containing)):
            windows_error("loader_address_query_failed")
        path = ctypes.create_unicode_buffer(32768)
        if not k.GetModuleFileNameW(containing, path, len(path)):
            windows_error("loader_module_query_failed")
        remote_base = modules.get(os.path.normcase(path.value))
        if remote_base is None:
            raise GadgetError("remote_loader_module_missing")
        remote_loader = remote_base + address - containing.value
        encoded = (str(dll) + "\0").encode("utf-16-le")
        remote = k.VirtualAllocEx(process, None, len(encoded), 0x3000, 4)
        if not remote:
            windows_error("gadget_path_allocation_failed")
        buffer = ctypes.create_string_buffer(encoded)
        written = ctypes.c_size_t()
        if not k.WriteProcessMemory(process, remote, buffer, len(encoded), ctypes.byref(written)):
            windows_error("gadget_path_write_failed")
        if written.value != len(encoded):
            raise GadgetError("gadget_path_partial_write")
        thread = k.CreateRemoteThread(process, None, 0, remote_loader, remote, 0, None)
        if not thread:
            windows_error("gadget_load_thread_denied")
        wait = k.WaitForSingleObject(thread, 15000)
        if wait != 0:
            raise GadgetError("gadget_load_thread_not_completed", wait)
        completed = True
        if own not in module_map(process):
            raise GadgetError("gadget_library_not_loaded")
        log.emit("gadget_loaded", reused=False, dll_resident=True)
    finally:
        if thread:
            k.CloseHandle(thread)
        if remote and (thread is None or completed):
            k.VirtualFreeEx(process, remote, 0, 0x8000)
        elif remote:
            log.emit("cleanup", phase="remote_path_buffer", result="deferred_to_host_exit")
        k.CloseHandle(process)


def authenticated(message, token, pid):
    return (isinstance(message, dict) and message.get("kind") == "hello" and
            type(message.get("pid")) is int and message["pid"] == pid and
            isinstance(message.get("token"), str) and message["token"].isascii() and
            len(message["token"]) == 64 and secrets.compare_digest(message["token"], token))


class Backend:
    def __init__(self, base, target, host, log, on_message, seconds, renewable=False):
        self.base, self.target, self.host = base, target, host
        self.log, self.on_message, self.seconds = log, on_message, seconds
        self.renewable = renewable
        self.token = secrets.token_hex(32)
        self.listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.listener.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(1)
        self.listener.settimeout(0.25)
        self.connection = None
        self.reader = None
        self.exit = threading.Event()
        self.ready = threading.Event()
        self.detached = threading.Event()
        self.failed = threading.Event()

    def receive(self):
        try:
            deadline = time.monotonic() + 25
            while not self.exit.is_set() and time.monotonic() < deadline:
                try:
                    connection, _ = self.listener.accept()
                except socket.timeout:
                    continue
                connection.settimeout(1)
                buffered = b""
                accepted = False
                client_deadline = time.monotonic() + 3
                try:
                    while not self.exit.is_set():
                        try:
                            chunk = connection.recv(2048)
                        except socket.timeout:
                            if not accepted and time.monotonic() >= client_deadline:
                                break
                            continue
                        if not chunk:
                            break
                        buffered += chunk
                        if len(buffered) > 8192:
                            raise GadgetError("gadget_message_too_large")
                        while b"\n" in buffered:
                            line, buffered = buffered.split(b"\n", 1)
                            message = json.loads(line)
                            if not accepted:
                                if not authenticated(message, self.token, self.target.pid):
                                    raise GadgetError("gadget_authentication_failed")
                                accepted = True
                                self.connection = connection
                                connection.sendall(b'{"kind":"accepted"}\n')
                            else:
                                if not isinstance(message, dict):
                                    raise GadgetError("gadget_invalid_message")
                                self.on_message({"type": "send", "payload": message}, None)
                                if message.get("kind") == "ready":
                                    self.ready.set()
                                if message.get("kind") == "stopped":
                                    self.detached.set()
                    if accepted:
                        return
                except (ValueError, OSError, GadgetError):
                    if accepted:
                        self.failed.set()
                        return
                finally:
                    connection.close()
        except OSError:
            pass
        finally:
            if not self.detached.is_set():
                self.failed.set()

    def start(self):
        self.reader = threading.Thread(target=self.receive, daemon=True)
        self.reader.start()
        parameters = {"port": self.listener.getsockname()[1], "token": self.token,
                      "capture": {"device": self.target.device, "seconds": self.seconds,
                                  "renewable": self.renewable,
                                  "proxy_diagnostics": self.target.shared_hid_count == 1}}
        dll = prepare_runtime(self.base, parameters, self.host.identity, self.target.pid)
        if not self.host.alive():
            raise GadgetError("host_exited_before_gadget_load")
        load_once(self.target.pid, dll, self.log)
        deadline = time.monotonic() + 15
        while not self.ready.wait(0.1):
            if self.failed.is_set() or time.monotonic() >= deadline:
                raise GadgetError("gadget_handshake_not_completed")
        self.log.emit("gadget_authenticated", dll_resident=True)

    def renew_lease(self):
        if not self.renewable or self.connection is None or self.failed.is_set():
            raise GadgetError("gadget_lease_unavailable")
        self.connection.sendall(b'{"kind":"lease"}\n')

    def stop(self):
        if self.connection is not None and not self.detached.is_set():
            try:
                self.connection.sendall(b'{"kind":"stop"}\n')
            except OSError:
                pass
            self.detached.wait(5)
        self.exit.set()
        if self.connection is not None:
            self.connection.close()
        self.listener.close()
        if self.reader is not None:
            self.reader.join(2)
        confirmed = self.detached.is_set()
        self.log.emit("cleanup", phase="gadget_hook", result="passed" if confirmed else "unknown",
                      dll_may_remain_resident=True)
        return confirmed
