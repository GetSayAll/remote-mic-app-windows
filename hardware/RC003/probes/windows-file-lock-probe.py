#!/usr/bin/env python3
"""只读探针：用 CreateFileW 的共享模式矩阵试探某个文件是否被其它进程占用。

> ⚠️ **本探针的读侧判据是错判据，保留它是为了记住怎么错的。**
> 2026-09-23 实测：`READ + share=NONE` **不能**检出被映射的 DLL——阳性对照
> （同一套判据问本进程自己映射着的 `kernel32.dll`）同样返回 OK。所以"能打开"
> **推不出**"没被占用"。要拿确定答案请用
> `windows-restart-manager-probe.py`（Restart Manager API，自带阳性对照）。
> 本脚本的价值：① 记录这次失败判据与它的对照组；② 在**提权**环境下给出一条
> `err=32` 的旁证（提权后 ACL 不再掩盖 SHARING_VIOLATION）。

用途：rc003-helper 反复运行时会卡在「复制 Gadget 到运行时目录」这一步
（`另一个程序正在使用此文件，进程无法访问。 (os error 32)` = ERROR_SHARING_VIOLATION）。
本探针把「文件被谁占用」这一事实与助手的报错解耦取证——不改动任何东西。

判据（相互独立的两个）：
  1. `READ share=NONE` 失败(err=32)  → **已证伪，不能作为被映射的判据**（见上）
  2. `WRITE share=NONE` 失败(err=32) → 写不进去（典型：DLL 被 LoadLibrary）；
     错误码 5 = ERROR_ACCESS_DENIED（ACL 造成，与占用无关，会**掩盖**真实的 32）

阳性对照（`--positive-control`，默认开启）：
  用一个**确定被映射**的文件（kernel32.dll，本进程自己就映射着）跑同一套判据。
  若它给出 err=32，说明判据有效，"没锁"的结论才可信；若它也返回 OK，
  说明判据坏了，此时"没锁"是假象。

用法：
    python windows-file-lock-probe.py [目标文件] [--pid N] [--no-positive-control]

退出码：0 = 探针本身跑完；2 = 目标文件不存在。
"""

from __future__ import annotations

import argparse
import ctypes
import os
import sys
from ctypes import wintypes

k32 = ctypes.WinDLL("kernel32", use_last_error=True)

GENERIC_READ = 0x80000000
GENERIC_WRITE = 0x40000000
DELETE = 0x00010000
OPEN_EXISTING = 3
FILE_ATTRIBUTE_NORMAL = 0x80
FILE_SHARE_ALL = 0x00000007
FILE_SHARE_READ = 0x00000001
INVALID_HANDLE = ctypes.c_void_p(-1).value

PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
PROCESS_QUERY_INFORMATION = 0x0400
PROCESS_VM_READ = 0x0010
LIST_MODULES_ALL = 0x03

k32.CreateFileW.restype = wintypes.HANDLE
k32.CreateFileW.argtypes = [
    wintypes.LPCWSTR,
    wintypes.DWORD,
    wintypes.DWORD,
    ctypes.c_void_p,
    wintypes.DWORD,
    wintypes.DWORD,
    wintypes.HANDLE,
]
k32.OpenProcess.restype = wintypes.HANDLE
k32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]


def try_open(path: str, access: int, share: int) -> tuple[bool, int]:
    ctypes.set_last_error(0)
    h = k32.CreateFileW(
        path, access, share, None, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, None
    )
    err = ctypes.get_last_error()
    ok = h is not None and h != INVALID_HANDLE and h != -1
    if ok:
        k32.CloseHandle(h)
    return ok, err


def matrix(path: str) -> dict[str, tuple[bool, int]]:
    return {
        "READ_SHARE_READ": try_open(path, GENERIC_READ, FILE_SHARE_READ),
        "READ_SHARE_NONE": try_open(path, GENERIC_READ, 0),
        "WRITE_SHARE_NONE": try_open(path, GENERIC_WRITE, 0),
        "DELETE_SHARE_ALL": try_open(path, DELETE, FILE_SHARE_ALL),
    }


def fmt(name: str, res: tuple[bool, int]) -> str:
    ok, err = res
    if ok:
        return f"  {name:18s} OK"
    msg = ctypes.FormatError(err) if err else ""
    return f"  {name:18s} FAIL err={err}  {msg}"


def verdict_file(path: str) -> None:
    print(f"target = {path}")
    if not os.path.exists(path):
        print("target_missing = true")
        return
    st = os.stat(path)
    print(f"size   = {st.st_size}   mtime = {st.st_mtime:.0f}")
    res = matrix(path)
    for k, v in res.items():
        print(fmt(k, v))

    read_none_ok = res["READ_SHARE_NONE"][0]
    read_ok = res["READ_SHARE_READ"][0]
    write_ok, write_err = res["WRITE_SHARE_NONE"]
    print("--- verdict ---")
    if not read_ok:
        print("readable = false  (连共享读都打不开，结论不可用)")
    elif read_none_ok:
        print("mapped_by_someone = false  (READ share=NONE 成功 → 无其它读者 → 未被映射)")
    else:
        print("mapped_by_someone = true   (READ share=NONE 被拒 → 有其它读者 → 被映射/占用)")
    if not write_ok and write_err == 32:
        print("write_blocked_by_share = true")
    elif not write_ok and write_err == 5:
        print("write_blocked_by_share = unknown (err=5 是 ACL 拒绝，掩盖了可能的 32；需提权复测)")


def positive_control() -> bool:
    """用一个必然被映射的文件验证判据有效性。返回判据是否可用。"""
    path = os.path.join(os.environ.get("SystemRoot", r"C:\Windows"), "System32", "kernel32.dll")
    print(f"--- positive control ({path}) ---")
    if not os.path.exists(path):
        print("  <控制文件不存在，改用本进程 exe>")
        path = sys.executable
        print(f"  control = {path}")
    res = matrix(path)
    for k, v in res.items():
        print(fmt(k, v))
    ok = not res["READ_SHARE_NONE"][0]
    print(f"  control_detects_mapping = {ok}")
    return ok


def probe_process(pid: int) -> None:
    print(f"--- process {pid} ---")
    for access, label in (
        (PROCESS_QUERY_LIMITED_INFORMATION, "QUERY_LIMITED"),
        (PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, "QUERY|VM_READ"),
    ):
        h = k32.OpenProcess(access, False, pid)
        if not h:
            err = ctypes.get_last_error()
            print(f"  OpenProcess({label}) FAIL err={err}  {ctypes.FormatError(err) if err else ''}")
            continue
        print(f"  OpenProcess({label}) OK")
        ft = (wintypes.FILETIME * 4)()
        if k32.GetProcessTimes(h, *[ctypes.byref(x) for x in ft]):
            ticks = (ft[0].dwHighDateTime << 32) | ft[0].dwLowDateTime
            # FILETIME epoch 1601-01-01；转 Unix 秒
            unix = ticks / 10_000_000 - 11644473600
            import datetime

            print(f"  started = {datetime.datetime.fromtimestamp(unix)}")
        else:
            err = ctypes.get_last_error()
            print(f"  GetProcessTimes FAIL err={err}")
        k32.CloseHandle(h)


def main() -> int:
    ap = argparse.ArgumentParser(add_help=True)
    ap.add_argument("target", nargs="?", default=r"C:\ProgramData\SayAll\rc003-helper\frida-gadget.dll")
    ap.add_argument("--pid", type=int, default=None)
    ap.add_argument("--no-positive-control", action="store_true")
    ns = ap.parse_args()

    control_ok = True
    if not ns.no_positive_control:
        control_ok = positive_control()

    verdict_file(ns.target)
    if not control_ok:
        print("WARN: 阳性对照未通过 —— 上面的 'mapped_by_someone = false' 不可信。")

    d = os.path.dirname(ns.target)
    if os.path.isdir(d):
        print(f"--- dir {d} ---")
        for name in sorted(os.listdir(d)):
            fp = os.path.join(d, name)
            try:
                size: object = os.path.getsize(fp)
            except OSError as exc:
                size = f"<{exc.errno}>"
            print(f"  {name}  len={size}")

    if ns.pid is not None:
        probe_process(ns.pid)

    return 0 if os.path.exists(ns.target) else 2


if __name__ == "__main__":
    raise SystemExit(main())
