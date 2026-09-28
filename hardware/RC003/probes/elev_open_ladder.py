"""提权后 OpenProcess 权限阶梯诊断（只读 + 一次 attach/detach，不加载脚本）。

背景：wudf_ioctl_synth 的提权守卫用的是 probe_open_denied（只试
PROCESS_QUERY_LIMITED_INFORMATION）。2026-09-28 真机实测：管理员令牌 True
但该权限被拒 → elevation_required 提前退出。而 09-23 同一守卫在同一台机器
提权后通过。哪个环节变了？本脚本用阶梯法拿 ground truth：

  1. 对自己 OpenProcess（阳性对照：阶梯代码本身必须能成功）
  2. 对 RC003 宿主逐档试权限，每档报 GetLastError 原始码
  3. frida.attach + detach（不加载脚本、不改任何字节）——注入可行性的
     最终判据

错误码只报数字不推断原因（err=5 是拒绝、err=87 是参数/进程不存在，
其余交给查表）。
"""

from __future__ import annotations

import ctypes
import os
import sys
from ctypes import wintypes
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from wudf_host_probe import enable_se_debug_privilege, scan_hosts  # noqa: E402

OUT = Path(os.environ.get("ELEV_DIAG_OUT", str(HERE / "elev_diag.log")))
_lines: list[str] = []


def log(message: str = "") -> None:
    _lines.append(message)
    print(message, flush=True)


kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
kernel32.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
kernel32.OpenProcess.restype = wintypes.HANDLE
kernel32.CloseHandle.argtypes = (wintypes.HANDLE,)
kernel32.CloseHandle.restype = wintypes.BOOL

RIGHTS = [
    ("QUERY_LIMITED_INFORMATION", 0x1000),
    ("QUERY_INFORMATION", 0x0400),
    ("VM_OP|VM_READ|VM_WRITE", 0x0008 | 0x0010 | 0x0020),
    ("CREATE_THREAD", 0x0002),
    ("frida-needs", 0x0002 | 0x0400 | 0x0008 | 0x0010 | 0x0020),
    ("ALL_ACCESS", 0x1FFFFF),
]


def ladder(pid: int, label: str) -> None:
    log(f"\n--- {label} (pid={pid}) ---")
    for name, access in RIGHTS:
        handle = kernel32.OpenProcess(access, False, pid)
        err = ctypes.get_last_error()
        if handle:
            kernel32.CloseHandle(handle)
            log(f"  {name:<28} 0x{access:06X} -> 成功")
        else:
            log(f"  {name:<28} 0x{access:06X} -> 失败 err={err}")


def main() -> int:
    log("=== 提权 OpenProcess 阶梯诊断 ===")
    log(f"时间: {datetime.now().isoformat(timespec='seconds')}")
    log(f"python: {sys.executable}")
    log(f"IsUserAnAdmin: {bool(ctypes.windll.shell32.IsUserAnAdmin())}")

    # 阳性对照：对自己全权限打开必须成功，否则阶梯代码本身不可信。
    ladder(os.getpid(), "自我（阳性对照）")

    entries = scan_hosts()
    pids = sorted({e["pid"] for e in entries})
    log(f"\n注册表 WUDFDiagnosticInfo 宿主 PID 集合: {pids}")
    rc003 = [e for e in entries if e["is_rc003"]]
    target = rc003[0]["pid"] if rc003 else (pids[0] if pids else None)
    log(f"RC003 宿主 PID: {target}")

    if target is not None:
        ladder(target, "RC003 宿主 WUDFHost")

        log("\n--- 启用 SeDebugPrivilege 后重跑阶梯（假设：frida 能进靠的是它）---")
        se_ok, se_err = enable_se_debug_privilege()
        log(f"  enable_se_debug_privilege -> {se_ok} (err={se_err})")
        ladder(target, "RC003 宿主 WUDFHost（SeDebug 后）")

        log("\n--- frida attach 测试（不加载脚本，attach 后立即 detach）---")
        try:
            import frida

            log(f"frida {frida.__version__}")
            session = frida.attach(target)
            log("frida.attach 成功")
            session.detach()
            log("detach 完成，无残留")
        except Exception as exc:  # noqa: BLE001
            log(f"frida.attach 失败: {type(exc).__name__}: {exc}")

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("\n".join(_lines) + "\n", encoding="utf-8")
    log(f"\n日志已写 {OUT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
