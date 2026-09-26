"""RC003 WUDF 宿主可行性与独占性探针（只读，不注入、不提权）。

背景：RC003 的返回键与音量± 的 HID usage（0x00F1 / 0x0080 / 0x0081）在
Windows 键盘栈映射阶段被丢弃，普通用户态（Raw Input / HID 直读）拿不到。
参考实现（ZSTDJan/windows-remote-mic-app，源自 xxb26553663-star/remote-bridge-hub）
的做法是：定位承载该设备 HID-over-GATT 的 WUDFHost.exe，经提权助手注入
Frida Gadget，在 NtDeviceIoControlFile 的 UMDF 输出复制入口保存并清空报告。

本探针只回答两个**只读**问题，不做任何注入或修改：
  1. 本机是否存在承载所选 RC003 的 WUDFHost，HostPid 是多少、进程是否在跑；
  2. 该宿主是否为「独占 RC003」宿主——参考实现只在独占时走简单绑定路径，
     共享宿主需要额外的来源核验。

依据：HKLM\\SYSTEM\\CurrentControlSet\\Enum\\<Enumerator>\\<Device>\\<Instance>\\
      Device Parameters\\WUDFDiagnosticInfo 的 HostPid 值。

脱敏：输出不打印蓝牙地址；硬件 token 只打印是否命中。
"""

from __future__ import annotations

import argparse
import ctypes
import ctypes.wintypes as wintypes
import re
import sys
from datetime import datetime
from pathlib import Path

try:
    import winreg
except ImportError:  # pragma: no cover - 非 Windows 主机
    winreg = None

ENUM_ROOT = r"SYSTEM\CurrentControlSet\Enum"
DIAGNOSTIC_SUFFIX = r"Device Parameters\WUDFDiagnosticInfo"
HID_SERVICE_PREFIX = "{00001812-0000-1000-8000-00805f9b34fb}"
# RC003 的硬件 token（VID 0x2717 / PID 0x32B8 / REV 00a4），与仓库既有取证一致。
RC003_HARDWARE_TOKEN = "vid&012717_pid&32b8_rev&00a4"

_log_lines: list[str] = []
_log_file: Path | None = None


def log(message: str = "") -> None:
    print(message)
    _log_lines.append(message)
    if _log_file is not None:
        _log_file.write_text("\n".join(_log_lines) + "\n", encoding="utf-8")


# 设备实例标识里真正需要脱敏的只有蓝牙地址：紧跟在 "_" 之后的 12 位十六进制，
# 且其后不是十六进制字符或 UUID 分隔符。用负向断言避免误伤 UUID 片段（如
# 00805f9b34fb）与 VID/PID/REV 字段。
BT_ADDRESS_PATTERN = re.compile(r"(?<=_)[0-9A-Fa-f]{12}(?![0-9A-Fa-f-])")


def mask_token(text: str) -> str:
    """把设备实例标识里的蓝牙地址替换为占位符，其余字符原样保留。"""
    return BT_ADDRESS_PATTERN.sub("<BT-ADDR>", text)


def process_name(pid: int) -> str:
    """用 Toolhelp 快照取进程可执行文件名。

    不用 OpenProcess：WUDFHost 通常运行在 session 0，普通权限进程对其
    OpenProcess 会得到 ERROR_ACCESS_DENIED（err=5），而快照枚举不需要该权限。
    """
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

    class PROCESSENTRY32W(ctypes.Structure):
        _fields_ = (
            ("dwSize", wintypes.DWORD),
            ("cntUsage", wintypes.DWORD),
            ("th32ProcessID", wintypes.DWORD),
            ("th32DefaultHeapID", ctypes.POINTER(ctypes.c_ulong)),
            ("th32ModuleID", wintypes.DWORD),
            ("cntThreads", wintypes.DWORD),
            ("th32ParentProcessID", wintypes.DWORD),
            ("pcPriClassBase", ctypes.c_long),
            ("dwFlags", wintypes.DWORD),
            ("szExeFile", wintypes.WCHAR * 260),
        )

    TH32CS_SNAPPROCESS = 0x00000002
    INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value
    kernel32.CreateToolhelp32Snapshot.argtypes = (wintypes.DWORD, wintypes.DWORD)
    kernel32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    kernel32.Process32FirstW.argtypes = (wintypes.HANDLE, ctypes.POINTER(PROCESSENTRY32W))
    kernel32.Process32FirstW.restype = wintypes.BOOL
    kernel32.Process32NextW.argtypes = (wintypes.HANDLE, ctypes.POINTER(PROCESSENTRY32W))
    kernel32.Process32NextW.restype = wintypes.BOOL
    kernel32.CloseHandle.argtypes = (wintypes.HANDLE,)

    snapshot = kernel32.CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
    if not snapshot or snapshot == INVALID_HANDLE_VALUE:
        return "snapshot_failed"
    try:
        entry = PROCESSENTRY32W()
        entry.dwSize = ctypes.sizeof(PROCESSENTRY32W)
        if not kernel32.Process32FirstW(snapshot, ctypes.byref(entry)):
            return "enum_failed"
        while True:
            if entry.th32ProcessID == pid:
                return entry.szExeFile or "name_empty"
            if not kernel32.Process32NextW(snapshot, ctypes.byref(entry)):
                return "not_running"
    finally:
        kernel32.CloseHandle(snapshot)


def probe_open_denied(pid: int) -> bool:
    """记录普通权限能否打开该宿主——用于说明注入为何必须提权。"""
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
    kernel32.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
    kernel32.OpenProcess.restype = wintypes.HANDLE
    kernel32.CloseHandle.argtypes = (wintypes.HANDLE,)
    handle = kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if handle:
        kernel32.CloseHandle(handle)
        return False
    return True


def scan_hosts() -> list[dict]:
    """枚举全部 WUDFDiagnosticInfo，返回 {pid, rd3, is_rc003, instance} 列表。"""
    entries: list[dict] = []
    with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, ENUM_ROOT) as root:
        for i in range(winreg.QueryInfoKey(root)[0]):
            enumerator = winreg.EnumKey(root, i)
            with winreg.OpenKey(root, enumerator) as enum_key:
                for j in range(winreg.QueryInfoKey(enum_key)[0]):
                    device = winreg.EnumKey(enum_key, j)
                    with winreg.OpenKey(enum_key, device) as device_key:
                        for k in range(winreg.QueryInfoKey(device_key)[0]):
                            instance = winreg.EnumKey(device_key, k)
                            try:
                                with winreg.OpenKey(
                                    device_key, f"{instance}\\{DIAGNOSTIC_SUFFIX}"
                                ) as diag_key:
                                    host_pid, _ = winreg.QueryValueEx(diag_key, "HostPid")
                            except (FileNotFoundError, OSError):
                                continue
                            if not host_pid:
                                continue
                            folded_device = device.casefold()
                            entries.append(
                                {
                                    "pid": int(host_pid),
                                    "enumerator": enumerator,
                                    "device": device,
                                    "instance": instance,
                                    "is_rc003": (
                                        enumerator.casefold() == "bthledevice"
                                        and folded_device.startswith(HID_SERVICE_PREFIX)
                                        and RC003_HARDWARE_TOKEN in folded_device
                                    ),
                                }
                            )
    return entries


def report(selected_key: str | None) -> int:
    if winreg is None:
        log("非 Windows 主机，无注册表可读。")
        return 2

    log("=== RC003 WUDF 宿主探测（只读）===")
    log(f"时间: {datetime.now().isoformat(timespec='seconds')}")

    entries = scan_hosts()
    log(f"\n带 WUDFDiagnosticInfo 的实例总数: {len(entries)}")

    rc003_entries = [e for e in entries if e["is_rc003"]]
    log(f"其中命中 RC003 硬件 token 的实例: {len(rc003_entries)}")

    log("\n--- RC003 宿主 ---")
    if not rc003_entries:
        log("未找到承载 RC003 的 WUDF 实例。")
        log("\n判定: rc003_wudf_host_absent")
        log("说明: 设备未连接/未配对时该节点可能不存在——请先连接遥控器后重跑。")
        return 1

    host_pids = sorted({e["pid"] for e in rc003_entries})
    for entry in rc003_entries:
        name = process_name(entry["pid"])
        denied = probe_open_denied(entry["pid"])
        log(
            f"  instance={entry['instance']}  HostPid={entry['pid']} (0x{entry['pid']:x})  "
            f"exe={name}  open_denied={denied}"
        )
    log(f"宿主 PID 集合: {host_pids}")
    log(
        "说明: WUDFHost 由驱动管理器启动、通常位于 session 0；普通权限对其"
        "OpenProcess 返回 err=5 属预期，也正说明注入必须由提权助手执行。"
    )

    # 参考实现的独占性判据：该宿主在整棵 Enum 树里只对应 RC003 一个成员。
    log("\n--- 宿主成员核对 ---")
    for pid in host_pids:
        members = [e for e in entries if e["pid"] == pid]
        rc003_members = [e for e in members if e["is_rc003"]]
        log(f"  HostPid={pid}: 总成员 {len(members)}，其中 RC003 成员 {len(rc003_members)}")
        for member in members:
            tag = "RC003" if member["is_rc003"] else "其它"
            log(f"      [{tag}] {member['enumerator']}\\{mask_token(member['device'])}")

    verdicts = []
    for pid in host_pids:
        members = [e for e in entries if e["pid"] == pid]
        if len(members) == 1 and members[0]["is_rc003"]:
            verdicts.append((pid, "exclusive_rc003_host"))
        else:
            verdicts.append((pid, "shared_host"))

    log("\n=== 判定 ===")
    for pid, verdict in verdicts:
        log(f"  HostPid={pid}: {verdict}")
    log("\n结论解读:")
    log("  - exclusive_rc003_host → 参考实现的简单绑定路径在本机适用；")
    log("  - shared_host → 本机该宿主还承载其它设备，参考实现需要额外的来源核验路径。")
    log("  - 本探针不做注入，不代表 tap 已可用；实际拦截仍需提权注入 + 真机复验。")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="RC003 WUDF 宿主可行性与独占性探测（只读）")
    parser.add_argument("--out", type=Path, help="同时写出日志文件")
    parser.add_argument("--selected-key", help="保留参数，用于与产品选择摘要对齐")
    args = parser.parse_args()

    global _log_file
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        _log_file = args.out
    return report(args.selected_key)


if __name__ == "__main__":
    sys.exit(main())
