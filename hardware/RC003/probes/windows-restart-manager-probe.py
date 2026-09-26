#!/usr/bin/env python3
"""只读探针：用 Windows Restart Manager API 判定"哪些进程正在占用某个文件"。

为什么需要它：rc003-helper 反复运行时会卡在「复制 Gadget 到运行时目录」，
报错 `另一个程序正在使用此文件，进程无法访问。 (os error 32)` = ERROR_SHARING_VIOLATION。
想确认"是谁锁的"，常见做法是逐个试 CreateFileW，但那条路有两个坑（本仓库已踩过）：

  * `READ + share=NONE` **不能**检出被映射的 DLL —— 阳性对照（kernel32.dll，本进程自己映射着）
    同样返回 OK，所以"打开成功"推不出"没被占用"。
  * `WRITE + share=NONE` 的 err=5(ERROR_ACCESS_DENIED, ACL) 会**掩盖** err=32(SHARING_VIOLATION)；
    `C:\\ProgramData\\...` 这类提权创建目录，普通用户天生写不进去。

Restart Manager 绕开了这两点：它直接由系统给出占用者列表，不需要写权限。

阳性对照（默认开启）：拿**本进程自己的可执行文件**去问 —— 本进程必然映射着它，
因此占用者列表里必须出现本进程 pid，否则说明判定方法失效。

用法：
    python windows-restart-manager-probe.py <文件路径> [<文件路径> ...]
    python windows-restart-manager-probe.py            # 默认查 RC003 运行时 Gadget

退出码：0 = 探针跑完；1 = 阳性对照未通过（结论不可信）。
"""

from __future__ import annotations

import ctypes
import os
import sys
from ctypes import wintypes

rstrtmgr = ctypes.WinDLL("rstrtmgr", use_last_error=True)

CCH_RM_SESSION_KEY = 32
CCH_RM_MAX_APP_NAME = 255
CCH_RM_MAX_SVC_NAME = 63

RM_INVALID_SESSION = -1
RmRebootReasonNone = 0

# RM_APP_TYPE
RM_UNKNOWN_APP = 0
RM_MAIN_WINDOW = 1
RM_SERVICE = 2
RM_EXPLORER = 3
RM_CONSOLE = 4
RM_CRITICAL = 1000

APP_TYPE_NAMES = {
    RM_UNKNOWN_APP: "Unknown",
    RM_MAIN_WINDOW: "MainWindow",
    RM_SERVICE: "Service",
    RM_EXPLORER: "Explorer",
    RM_CONSOLE: "Console",
    RM_CRITICAL: "Critical",
}


class RM_UNIQUE_PROCESS(ctypes.Structure):
    _fields_ = [
        ("dwProcessId", wintypes.DWORD),
        ("ProcessStartTime", wintypes.FILETIME),
    ]


class RM_PROCESS_INFO(ctypes.Structure):
    _fields_ = [
        ("Process", RM_UNIQUE_PROCESS),
        ("strAppName", wintypes.WCHAR * (CCH_RM_MAX_APP_NAME + 1)),
        ("strServiceShortName", wintypes.WCHAR * (CCH_RM_MAX_SVC_NAME + 1)),
        ("ApplicationType", ctypes.c_uint),
        ("AppStatus", wintypes.ULONG),
        ("TSSessionId", wintypes.DWORD),
        ("bRestartable", wintypes.BOOL),
    ]


rstrtmgr.RmStartSession.restype = wintypes.DWORD
rstrtmgr.RmStartSession.argtypes = [ctypes.POINTER(wintypes.DWORD), wintypes.DWORD, wintypes.LPWSTR]
rstrtmgr.RmRegisterResources.restype = wintypes.DWORD
rstrtmgr.RmRegisterResources.argtypes = [
    wintypes.DWORD,
    ctypes.c_uint,
    ctypes.POINTER(wintypes.LPCWSTR),
    ctypes.c_uint,
    ctypes.c_void_p,
    ctypes.c_uint,
    ctypes.c_void_p,
]
rstrtmgr.RmGetList.restype = wintypes.DWORD
rstrtmgr.RmGetList.argtypes = [
    wintypes.DWORD,
    ctypes.POINTER(ctypes.c_uint),
    ctypes.POINTER(ctypes.c_uint),
    ctypes.POINTER(RM_PROCESS_INFO),
    ctypes.POINTER(wintypes.DWORD),
]
rstrtmgr.RmEndSession.restype = wintypes.DWORD
rstrtmgr.RmEndSession.argtypes = [wintypes.DWORD]


def filetime_to_unix(ft: wintypes.FILETIME) -> float:
    ticks = (ft.dwHighDateTime << 32) | ft.dwLowDateTime
    return ticks / 10_000_000 - 11644473600


def users_of(paths: list[str]) -> list[RM_PROCESS_INFO]:
    """返回占用给定文件的进程列表；失败抛 RuntimeError。"""
    session = wintypes.DWORD(0)
    key = ctypes.create_unicode_buffer(CCH_RM_SESSION_KEY + 1)
    rc = rstrtmgr.RmStartSession(ctypes.byref(session), 0, key)
    if rc != 0:
        raise RuntimeError(f"RmStartSession rc={rc}")
    try:
        wide = (wintypes.LPCWSTR * len(paths))(*paths)
        rc = rstrtmgr.RmRegisterResources(session, len(paths), wide, 0, None, 0, None)
        if rc != 0:
            raise RuntimeError(f"RmRegisterResources rc={rc}")

        needed = ctypes.c_uint(0)
        count = ctypes.c_uint(0)
        reasons = wintypes.DWORD(0)
        rc = rstrtmgr.RmGetList(session, ctypes.byref(needed), ctypes.byref(count), None, ctypes.byref(reasons))
        if rc == 0 and needed.value == 0:
            return []
        if rc != 234:  # ERROR_MORE_DATA
            raise RuntimeError(f"RmGetList(size query) rc={rc}")
        if needed.value == 0:
            return []
        arr = (RM_PROCESS_INFO * needed.value)()
        count = ctypes.c_uint(needed.value)
        rc = rstrtmgr.RmGetList(session, ctypes.byref(needed), ctypes.byref(count), arr, ctypes.byref(reasons))
        if rc != 0:
            raise RuntimeError(f"RmGetList rc={rc}")
        return [arr[i] for i in range(count.value)]
    finally:
        rstrtmgr.RmEndSession(session)


def query(paths: list[str], label: str) -> list[RM_PROCESS_INFO]:
    print(f"--- {label} ---")
    for p in paths:
        print(f"  file: {p}")
    try:
        procs = users_of(paths)
    except RuntimeError as exc:
        print(f"  <Rm 查询失败: {exc}>")
        return []
    if not procs:
        print("  users = (none)")
        return []
    for pi in procs:
        kind = APP_TYPE_NAMES.get(pi.ApplicationType, str(pi.ApplicationType))
        print(
            f"  user pid={pi.Process.dwProcessId:<6d} type={kind:<10s} "
            f"started={filetime_to_unix(pi.Process.ProcessStartTime):.0f} "
            f"app={pi.strAppName!r}"
        )
    return procs


def main() -> int:
    argv = [a for a in sys.argv[1:] if not a.startswith("--")]
    no_control = "--no-positive-control" in sys.argv

    targets = argv or [r"C:\ProgramData\SayAll\rc003-helper\frida-gadget.dll"]
    targets = [t for t in targets if os.path.exists(t)]
    if not targets:
        print("no existing target file given")
        return 2

    control_ok = True
    if not no_control:
        me = sys.executable
        procs = query([me], f"positive control (本进程必然映射) pid={os.getpid()}")
        control_ok = any(pi.Process.dwProcessId == os.getpid() for pi in procs)
        print(f"  control_detects_mapping = {control_ok}")
        if not control_ok:
            print("  !! 阳性对照未通过：本方法无法自证，下面的结论不可信。")
        print()

    query(targets, "target")

    if not control_ok:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
