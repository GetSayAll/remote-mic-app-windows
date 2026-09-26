"""RC003 WUDF 宿主 IOCTL 只读 tap 实验（需一次提权，不重启、不改系统设置）。

背景与目的
----------
RC003 的返回键与音量±（usage `0x00F1` / `0x0080` / `0x0081`）声明在键盘页
`report_id=0x01`，但 `kbdhid` 的 HID→VK 映射表缺这三个 usage，在映射阶段被丢弃，
因此普通用户态（Raw Input、HID 直读）拿不到。参考实现
（ZSTDJan/windows-remote-mic-app）显示：报告在承载该设备的 `WUDFHost.exe` 内、
`IOCTL 0x80018483` 的 UMDF 输出复制入口是可读的。

本实验把 `docs/investigations/2026-09-23-zstdjan-hid-host-tap-implementation-review.md`
§5 未验证项 1 从 `structural` 升级为 `passed` / `failed`：用一次提权注入最小探针，
**只读**转储该 IOCTL 的输入/输出缓冲区，直接回答"本机 RC003 的 9 字节报告里
到底有没有这三个 usage"。

只读契约
--------
**不清空缓冲区、不注入按键、不修改任何系统设置、不写注册表。** 因此本实验
即使结论为阳性，也**不代表 tap 可落地**——落地还需要在 `onEnter` 写入
（会改变系统行为）以及产品对补丁级别的决策。

阳性对照（关键）
----------------
阶段 A 要求按【主页】与【确定】——这两个键在 Windows 侧本来就可用的，若 tap
工作正常必然看到它们的 usage。没有阳性对照，"零事件"无法区分"设备没发"与
"探针坏了"。

通道设计
--------
**只使用单向 `send()`。** 2026-09-23 首次真机运行中，Python→JS 的 `post()/recv()`
只送达第一条消息（阶段标签恒为 `pre`、收尾汇总为空），而同一次运行里 JS→Python
的 `send()` 全程正常；`frida_msg_selftest.py` 显示 `post/recv` 在 spawn 与 attach
下都可用，故该异常归因于 session 0 目标环境、原因未定位。因此：
  - 每条记录自带 `t`（毫秒），**阶段归属由 Python 侧按本地时间轴判定**；
  - 累计统计随每次心跳上行，**最后一次心跳即终值**；
  - 判定一律从原始记录重算，不依赖任何汇总字段。

用法
----
    python wudf_ioctl_tap.py --out <log>          # 真机运行（需提权）
    python wudf_ioctl_tap.py --dry-run            # 只定位宿主 + 提权自检
    python wudf_ioctl_tap.py --analyze <log>      # 离线复核既有日志（无需提权）

风险
----
向 session 0 的 `WUDFHost.exe` 注入第三方运行时。探针只读，不改行为；但
若注入/卸载本身异常，最坏情况是该宿主崩溃、RC003 的 HID 通道需重新枚举
（拔插或重连遥控器）。同一宿主在本机为「独占 RC003」，不承载其它设备。

脱敏：只打印 模块名+偏移，不打印指针绝对值；不打印蓝牙地址。
"""

from __future__ import annotations

import argparse
import ctypes
import json
import re
import sys
import time
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from wudf_host_probe import probe_open_denied, scan_hosts  # noqa: E402

JS_PATH = HERE / "wudf_ioctl_tap.js"
TARGET_IOCTL = 0x80018483

USAGE_NAMES = {
    0x0028: "确定/OK (Enter)",
    0x004A: "主页/Home",
    0x004C: "Delete",
    0x0050: "Left",
    0x0051: "Down",
    0x0052: "Up",
    0x0053: "Right",
    0x0080: "音量+ (Volume Up)",
    0x0081: "音量- (Volume Down)",
    0x00F1: "返回 (Back)",
}
TARGET_USAGES = (0x00F1, 0x0080, 0x0081)
POSITIVE_CONTROL_USAGES = (0x0028, 0x004A)

# (阶段名, 秒数, 提示语)
PHASES: list[tuple[str, int, str]] = [
    ("pre", 15, "预热。请不要按遥控器——请拿好遥控器并唤醒它（按一下任意可用键）。"),
    ("A", 12, "阳性对照。请只按：主页 2-3 次，再按 确定 2-3 次。"),
    ("B", 16, "目标三键。请依次按：返回 2-3 次，音量+ 2-3 次，音量- 2-3 次。"),
    ("post", 4, "收尾。请停止按键。"),
]

_log_lines: list[str] = []
_log_file: Path | None = None

RECORD_RE = re.compile(
    r"\[(?P<phase>[^\]/]+)/(?P<dir>enter|leave)\]\s+"
    r"(?:t=(?P<t>\d+)\s+)?"
    r"in_len=(?P<in_len>\d+)\s+out_len=(?P<out_len>\d+)\s+"
    r"in=(?P<in_hex>\S*)\s+out=(?P<out_hex>\S*)"
    r"(?P<rest>.*)$"
)


def log(message: str = "") -> None:
    print(message, flush=True)
    _log_lines.append(message)
    if _log_file is not None:
        _log_file.write_text("\n".join(_log_lines) + "\n", encoding="utf-8")


def usage_name(usage: int) -> str:
    return USAGE_NAMES.get(usage, "?")


def decode_report(out_hex: str) -> dict | None:
    """解 9 字节键盘报告：report_id + modifiers + reserved + 3 × uint16 usage。"""
    if len(out_hex) < 18:
        return None
    try:
        b = [int(out_hex[i * 2 : i * 2 + 2], 16) for i in range(9)]
    except ValueError:
        return None
    usages = [b[3] | (b[4] << 8), b[5] | (b[6] << 8), b[7] | (b[8] << 8)]
    return {
        "report_id": b[0],
        "modifiers": b[1],
        "reserved": b[2],
        "usages": usages,
        "active": [u for u in usages if u != 0],
    }


def find_exclusive_rc003_host() -> int | None:
    entries = scan_hosts()
    rc003 = [e for e in entries if e["is_rc003"]]
    if not rc003:
        return None
    for pid in sorted({e["pid"] for e in rc003}):
        members = [e for e in entries if e["pid"] == pid]
        if len(members) == 1 and members[0]["is_rc003"]:
            return pid
    log(f"  RC003 成员所在宿主 PID: {sorted({e['pid'] for e in rc003})}，但均为 shared_host。")
    return None


def check_elevation(pid: int) -> bool:
    admin = bool(ctypes.windll.shell32.IsUserAnAdmin())
    denied = probe_open_denied(pid)
    log(f"  当前进程管理员令牌: {admin}")
    log(f"  OpenProcess(目标宿主) 被拒: {denied}")
    return admin and not denied


# --------------------------------------------------------------------------
# 判定：只从原始记录推导
# --------------------------------------------------------------------------


def verdict_from_records(records: list[dict], stats: dict | None = None) -> tuple[str, int]:
    """从原始记录重算判定。返回 (判定名, 退出码)。"""
    entries = [r for r in records if r.get("dir") == "enter"]
    leaves = [r for r in records if r.get("dir") == "leave"]

    log("\n=== 判定 ===")
    log(f"命中 IOCTL 0x{TARGET_IOCTL:08X} 的记录数: enter={len(entries)} leave={len(leaves)}")
    if stats:
        log(f"agent 侧累计: {json.dumps({k: v for k, v in stats.items() if k != 'type'}, ensure_ascii=False)}")

    if not entries:
        log("判定: tap_not_verified")
        log("说明: 钩子生效但该 IOCTL 未出现——可能 Windows/驱动链版本不同。")
        return "tap_not_verified", 7

    in_lens = sorted({r["in_len"] for r in entries})
    out_lens = sorted({r["out_len"] for r in entries})
    selectors = sorted(
        {
            f"{r['in_hex'][8:10]}/{r['in_hex'][10:12]}"
            for r in entries
            if len(r.get("in_hex", "")) >= 12
        }
    )
    callers = sorted({r.get("caller", "?") for r in entries})
    log(f"输入长度集合: {in_lens}（参考判据 8）")
    log(f"输出长度集合: {out_lens}（参考判据 9）")
    log(f"输入第 5/6 字节（operation/selector）集合: {selectors}（参考判据 02/01）")
    log(f"调用方模块: {callers}")

    criteria_ok = in_lens == [8] and out_lens == [9] and selectors == ["02/01"]
    log(f"与参考实现判据一致: {criteria_ok}")

    usage_counts: dict[int, int] = {}
    report_count = 0
    for r in entries:
        decoded = decode_report(r.get("out_hex", ""))
        if decoded is None or decoded["report_id"] != 1:
            continue
        report_count += 1
        for u in decoded["active"]:
            usage_counts[u] = usage_counts.get(u, 0) + 1

    log(f"\nreport_id=0x01 的报告数: {report_count}")
    log("其中非零 usage 统计（已按名称渲染）:")
    if not usage_counts:
        log("  （无）")
    for usage in sorted(usage_counts):
        tag = ""
        if usage in TARGET_USAGES:
            tag = "  ← 目标三键"
        elif usage in POSITIVE_CONTROL_USAGES:
            tag = "  ← 阳性对照键"
        log(f"  0x{usage:04X} {usage_name(usage)} × {usage_counts[usage]}{tag}")

    changed = [r for r in leaves if r.get("out_changed")]
    log(
        f"\nonLeave 时输出缓冲区发生变化: {len(changed)}/{len(leaves)}"
        "（0 表示报告在 onEnter 已就位、内核在调用期间不改写它）"
    )

    positive = [u for u in POSITIVE_CONTROL_USAGES if u in usage_counts]
    if not positive:
        log("判定: tap_unconfirmed_no_positive_control")
        log("说明: 未捕获到主页/确定的 usage，无法区分'设备没发'与'探针没读到'。")
        return "tap_unconfirmed_no_positive_control", 8

    log(
        "\n阳性对照通过: 捕获到 "
        + "、".join(f"0x{u:04X}({usage_name(u)})" for u in sorted(positive))
        + "，报告通道确实被读到。"
    )

    found = [u for u in TARGET_USAGES if u in usage_counts]
    if found:
        log("\n判定: three_keys_present_in_report（阳性，路线 A 前提在报告层成立）")
        for u in found:
            log(f"  0x{u:04X} {usage_name(u)} 出现 {usage_counts[u]} 次")
        log("说明: 本机 RC003 的报告里**确实存在**目标三键；三键在系统侧不可见的原因是")
        log("      Windows 键码映射阶段丢弃，而不是设备不上报。")
        log("      本轮只读、未清空——三键在系统侧仍不可用，属预期。")
        return "three_keys_present_in_report", 0

    log("\n判定: three_keys_absent_in_report（阴性，与参考实现预期不符）")
    log("说明: 本机报告层没有目标三键，需按'异机须核对驱动链与真实报告'重新评估。")
    return "three_keys_absent_in_report", 9


def analyze(path: Path) -> int:
    """离线复核既有日志：只解析，不注入、不提权。"""
    global _log_file
    text = path.read_text(encoding="utf-8", errors="replace")
    records: list[dict] = []
    for line in text.splitlines():
        m = RECORD_RE.search(line)
        if not m:
            continue
        rest = m.group("rest") or ""
        changed = re.search(r"out_changed=(True|False)", rest)
        records.append(
            {
                "phase": m.group("phase"),
                "dir": m.group("dir"),
                "t": int(m.group("t")) if m.group("t") else None,
                "in_len": int(m.group("in_len")),
                "out_len": int(m.group("out_len")),
                "in_hex": m.group("in_hex"),
                "out_hex": m.group("out_hex"),
                "out_changed": None if not changed else changed.group(1) == "True",
                "caller": (re.search(r"caller=(\S+)", rest) or [None, "?"])[1]
                if "caller=" in rest
                else "?",
            }
        )
    log(f"=== 离线复核: {path} ===")
    log(f"解析到记录 {len(records)} 条（enter/leave 各半）")
    _, code = verdict_from_records(records, stats=None)
    return code


# --------------------------------------------------------------------------
# 真机运行
# --------------------------------------------------------------------------


def run(seconds_scale: float, out: Path | None) -> int:
    global _log_file
    if out:
        out.parent.mkdir(parents=True, exist_ok=True)
        _log_file = out

    log("=== RC003 WUDF 宿主 IOCTL 只读 tap 实验 ===")
    log(f"时间: {datetime.now().isoformat(timespec='seconds')}")
    log("契约: 只读转储，不清空缓冲区、不注入按键、不改系统设置。")

    log("\n--- 步骤 1/4：定位独占 RC003 的 WUDFHost ---")
    host_pid = find_exclusive_rc003_host()
    if host_pid is None:
        log("判定: rc003_wudf_host_absent")
        log("说明: 请先连接并唤醒 RC003 遥控器后重跑。")
        return 1
    log(f"  命中 exclusive_rc003_host，HostPid={host_pid} (0x{host_pid:x})")

    log("\n--- 步骤 2/4：提权与可注入性自检 ---")
    if not check_elevation(host_pid):
        log("判定: elevation_required")
        log("说明: 宿主位于 session 0，普通权限对其 OpenProcess 返回 err=5。")
        log("      请用「以管理员身份运行」的终端重跑本脚本。")
        return 3
    log("  提权自检通过。")

    try:
        import frida
    except ImportError:
        log("判定: frida_missing")
        log("说明: 请先安装：pip install frida")
        return 4
    log(f"  frida {frida.__version__}")

    try:
        js_source = JS_PATH.read_text(encoding="utf-8")
    except OSError as exc:
        log(f"判定: js_missing ({exc})")
        return 4

    log("\n--- 步骤 3/4：attach 并安装只读钩子 ---")
    try:
        session = frida.attach(host_pid)
    except Exception as exc:  # noqa: BLE001 - 现场需要看到原始错误
        log(f"判定: attach_failed -> {type(exc).__name__}: {exc}")
        log("说明: 常见原因——未提权；或 EDR/Defender 拦截向系统进程注入。")
        return 5

    records: list[dict] = []
    stats: dict = {}
    ready = {"hook_ok": False, "target_ioctl": ""}
    timeline: list[tuple[float, float, str]] = []
    unknown_kinds: dict[str, int] = {}

    def on_message(message, data):  # noqa: ANN001, ARG001
        if message.get("type") == "error":
            log(f"  [frida error] {message}")
            return
        payload = message.get("payload") or {}
        kind = payload.get("type")
        if kind == "ready":
            ready["hook_ok"] = bool(payload.get("hook_ok"))
            ready["target_ioctl"] = payload.get("target_ioctl", "")
            log(f"  钩子已安装: NtDeviceIoControlFile，目标 IOCTL={ready['target_ioctl']}")
            log(f"  导出地址可用: {ready['hook_ok']}")
        elif kind == "ioctl":
            records.append(payload)
            _log_record(payload, timeline)
        elif kind == "stats":
            stats.clear()
            stats.update(payload)
            log(
                f"  [心跳] 总调用={payload.get('total_calls')} "
                f"目标 IOCTL={payload.get('target_calls')} "
                f"记录={payload.get('records_sent')}"
                + ("（已达上限，截断）" if payload.get("truncated") else "")
            )
        else:
            unknown_kinds[str(kind)] = unknown_kinds.get(str(kind), 0) + 1

    script = session.create_script(js_source)
    script.on("message", on_message)
    script.load()
    time.sleep(1.0)

    if not ready["hook_ok"]:
        log("判定: hook_export_null")
        _detach(session)
        return 6

    log("\n--- 步骤 4/4：分阶段采集（请按屏幕提示操作遥控器）---")
    for name, base_seconds, hint in PHASES:
        duration = max(2.0, base_seconds * seconds_scale)
        log(f"\n>>> 阶段 {name}：{hint}  时长 {duration:.0f} 秒")
        start = time.time() * 1000.0
        _countdown(duration)
        timeline.append((start, time.time() * 1000.0, name))

    log("\n--- 阶段时间轴（本地时钟，用于把记录归到阶段）---")
    for start, end, name in timeline:
        log(f"  {name}: {start:.0f} .. {end:.0f}")

    if unknown_kinds:
        log(f"\n[注意] 未识别的上行消息类型: {unknown_kinds}")

    _detach(session)
    return verdict_from_records(records, stats)[1]


def _phase_of(t: int | None, timeline: list[tuple[float, float, str]]) -> str:
    if t is None:
        return "?"
    for start, end, name in timeline:
        if start <= t <= end:
            return name
    if timeline and t < timeline[0][0]:
        return "pre-hook"
    return "tail"


def _log_record(payload: dict, timeline: list[tuple[float, float, str]]) -> None:
    phase = _phase_of(payload.get("t"), timeline)
    direction = payload.get("dir")
    out_hex = payload.get("out_hex") or ""
    decoded = decode_report(out_hex)
    detail = ""
    if decoded is not None:
        names = [f"0x{u:04X}({usage_name(u)})" for u in decoded["active"]]
        detail = f" report_id=0x{decoded['report_id']:02X} usage={names or '[]'}"
    changed = payload.get("out_changed")
    tail = "" if changed is None else f" out_changed={changed}"
    t = payload.get("t")
    log(
        f"  [{phase}/{direction}] t={t} in_len={payload.get('in_len')} "
        f"out_len={payload.get('out_len')} in={payload.get('in_hex')} "
        f"out={out_hex} caller={payload.get('caller')}{detail}{tail}"
    )


def _countdown(seconds: float) -> None:
    remaining = seconds
    while remaining > 0:
        step = min(4.0, remaining)
        time.sleep(step)
        remaining -= step
        if remaining > 0:
            print(f"    ... 还剩 {remaining:.0f} 秒", flush=True)


def _detach(session) -> None:  # noqa: ANN001
    try:
        session.detach()
    except Exception:  # noqa: BLE001
        pass


def main() -> int:
    parser = argparse.ArgumentParser(description="RC003 WUDF 宿主 IOCTL 只读 tap 实验（需提权）")
    parser.add_argument("--out", type=Path, help="日志落盘路径")
    parser.add_argument("--scale", type=float, default=1.0, help="阶段时长缩放，1.0 为默认约 47 秒")
    parser.add_argument("--dry-run", action="store_true", help="只做定位与自检，不注入")
    parser.add_argument("--analyze", type=Path, help="离线复核既有日志，不注入、不提权")
    args = parser.parse_args()

    if args.analyze:
        return analyze(args.analyze)

    if args.dry_run:
        global _log_file
        if args.out:
            args.out.parent.mkdir(parents=True, exist_ok=True)
            _log_file = args.out
        log("=== 只读 tap 实验：dry-run（不注入）===")
        log(f"JS 文件存在: {JS_PATH.exists()}")
        host_pid = find_exclusive_rc003_host()
        log(f"独占 RC003 宿主 PID: {host_pid}")
        if host_pid is not None:
            check_elevation(host_pid)
        return 0

    try:
        return run(args.scale, args.out)
    except KeyboardInterrupt:
        log("\n用户中断。")
        return 130


if __name__ == "__main__":
    sys.exit(main())
