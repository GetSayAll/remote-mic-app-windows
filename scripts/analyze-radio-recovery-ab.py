#!/usr/bin/env python3
"""蓝牙无线电恢复 A/B 对照统计（2026-09-16）。

回答一个问题：**关开蓝牙无线电到底有没有用？**

不靠推理，把日志里每次「恢复动作」和它之后那次重连结果配对，算两组恢复率：

- 实验组 `radio_cycle`：决策点判定要走恢复、真的执行了 Off/On，之后那次重连
- 对照组（优先）`skip_recovery`：到了同一个决策点、但**没有**执行 Off/On，之后那次重连
- 对照组（兜底）`no_cycle`：旧版日志没有决策标记，用同一次僵死事件里没有紧接
  Off/On 的重连尝试（`attempt>=1`）

优先用 `skip_recovery` 是因为它和实验组处在**同一个决策时刻**，可比性最好。
`no_cycle` 只用于 2026-09-16 之前、还没有 `ble_recovery_decision` 标记的日志。

对照组口径里排除 `attempt=0`：进程刚启动的 `attempt=0` 属于「健康时首次连接」，
不是「僵死中继续挣扎」，混进来会人为抬高对照组，让 Off/On 显得有用。

输出三选一：**有效** / **无效** / **样本不足**。

用法：
    python scripts/analyze-radio-recovery-ab.py <日志文件或目录>
    python scripts/analyze-radio-recovery-ab.py artifacts/ev_stream_raw.txt

日志来自 SAYALL_GATT_LOG（应用「关于」页可打开日志目录）。
脚本只读文本，不碰任何设备或系统状态。
"""

from __future__ import annotations

import math
import re
import sys
from collections import defaultdict
from pathlib import Path

# 真实日志行形如：
#   2026-09-12T13:27:17.202Z|17624|... pid=17624 ver=0.2.6 \
#     ble_radio_recovery phase=completed terminal_result=failed window=3 cycle=1 ...
# 注意两点：
#   1) 标记前面没有 `note=` 前缀；
#   2) 多个进程会写进同一个文件，**必须按 pid 配对**，
#      否则会把 A 进程的开关和 B 进程的重连结果错配成对。
PID_RE = re.compile(r"\bpid=(\d+)")

# 新版标记（2026-09-16 起）：决策点，action=radio_cycle 表示真的要开关。
DECISION_RE = re.compile(r"ble_recovery_decision\s+action=(\S+)")
# 旧版标记：没有决策日志，只有 Off/On 执行完的结果行。
LEGACY_CYCLE_RE = re.compile(
    r"ble_radio_recovery\s+phase=completed\s+terminal_result=(passed|failed)"
)
ERROR_CODE_RE = re.compile(r"error_code=(\S+)")
REASON_RE = re.compile(r"reason=(\S+)")

CONNECT_RE = re.compile(
    r"ble_connect\s+phase=completed\s+terminal_result=(passed|failed)"
    r"[^\n]*?\battempt=(\d+)"
)

# 判定两组「有实质差异」的阈值（百分点）。
MATERIAL_DIFFERENCE_PP = 10.0
# 小于这个样本量的一组不足以支撑结论。
MIN_SAMPLE = 30

# 僵死态标记。对照组如果基本由这些构成，说明两组是按错误码分开的、不同质。
WEDGED_MARKERS = (
    "windows_resource_exhausted",
    "winrt_operation_aborted",
    "stack_exhausted_proven_ineffective",
    "recovery_proven_ineffective",
)


def wilson_interval(recovered: int, total: int) -> tuple[float, float]:
    """恢复率的 95% Wilson 置信区间（百分数）。

    绝对差值小 ≠ 结论强。两组都接近 0 时，点估计的"几乎相同"可能是
    "两者都被压在接近 0 的后验上限之下"。报出 CI 上限才能区分这两种情形：
    上限低 → 真的都无效；上限高 → 只能说"当前测不出差异"。
    """
    if total == 0:
        return (0.0, 0.0)
    z = 1.96
    p = recovered / total
    denom = 1 + z * z / total
    center = (p + z * z / (2 * total)) / denom
    half = z * math.sqrt(p * (1 - p) / total + z * z / (4 * total * total)) / denom
    return (max(0.0, center - half) * 100, (center + half) * 100)


def collect_log_files(target: str) -> list[Path]:
    path = Path(target)
    if path.is_file():
        return [path]
    if path.is_dir():
        return sorted(
            p for p in path.rglob("*")
            if p.is_file() and p.suffix.lower() in {".log", ".txt"}
        )
    raise SystemExit(f"找不到日志路径：{target}")


def analyze(files: list[Path]) -> dict:
    """按 pid 把「恢复动作」与其后第一次重连结果配对。"""
    # pid -> (组名, 细节)；细节用于再拆分（如 cycle 自身是否执行成功）
    armed: dict[str, tuple[str, str]] = {}
    # 组 -> {"recovered": n, "total": n}
    groups: dict[str, dict[str, int]] = defaultdict(lambda: {"recovered": 0, "total": 0})
    # 对照组按 attempt 序号细分，便于核对为什么要排除 attempt=0
    control_by_attempt: dict[int, dict[str, int]] = defaultdict(
        lambda: {"recovered": 0, "total": 0}
    )

    for file in files:
        with file.open("r", encoding="utf-8", errors="replace") as handle:
            for line in handle:
                pid_match = PID_RE.search(line)
                pid = pid_match.group(1) if pid_match else "unknown"

                decision = DECISION_RE.search(line)
                if decision:
                    action = decision.group(1)
                    if action == "skip_recovery":
                        armed[pid] = ("skip_recovery", _detail(line, default="unknown"))
                    else:
                        armed[pid] = ("radio_cycle", _detail(line, default="executed"))
                    continue

                legacy = LEGACY_CYCLE_RE.search(line)
                if legacy:
                    armed[pid] = ("radio_cycle", f"cycle_{legacy.group(1)}")
                    continue

                outcome = CONNECT_RE.search(line)
                if not outcome:
                    continue
                recovered = outcome.group(1) == "passed"
                attempt = int(outcome.group(2))

                if pid in armed:
                    group, detail = armed.pop(pid)
                    bucket = groups[f"{group} / {detail}"]
                else:
                    group = "no_cycle"
                    bucket = control_by_attempt[attempt]
                bucket["total"] += 1
                if recovered:
                    bucket["recovered"] += 1

    # 对照组主口径：attempt>=1（排除进程/窗口刚启动的那次 attempt=0）
    control = {"recovered": 0, "total": 0}
    for attempt, bucket in control_by_attempt.items():
        if attempt >= 1:
            control["recovered"] += bucket["recovered"]
            control["total"] += bucket["total"]
    groups["no_cycle / attempt>=1"] = control
    groups["no_cycle / attempt=0（不参与对照）"] = dict(
        control_by_attempt.get(0, {"recovered": 0, "total": 0})
    )

    return {"groups": dict(groups), "files": len(files)}


def _detail(line: str, default: str) -> str:
    code = ERROR_CODE_RE.search(line)
    if code:
        return code.group(1)
    reason = REASON_RE.search(line)
    return reason.group(1) if reason else default


def rate(recovered: int, total: int) -> float:
    return (recovered / total * 100.0) if total else 0.0


def _sum(groups: dict, predicate) -> dict:
    out = {"recovered": 0, "total": 0}
    for name, bucket in groups.items():
        if predicate(name):
            out["recovered"] += bucket["recovered"]
            out["total"] += bucket["total"]
    return out


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__)
        return 2

    files = collect_log_files(sys.argv[1])
    result = analyze(files)
    groups = result["groups"]

    print("=== 蓝牙无线电恢复 A/B 对照 ===")
    print(f"扫描日志文件 {result['files']} 个\n")

    if not any(bucket["total"] for bucket in groups.values()):
        print("没有读到任何重连记录。可能原因：")
        print("  1) 日志不是 SAYALL_GATT_LOG 产物；")
        print("  2) 连接一直正常，从未进入恢复流程（好事）。")
        return 1

    width = max(len(name) for name in groups)
    print(f"{'组'.ljust(width)}  样本    恢复    恢复率")
    for name in sorted(groups):
        bucket = groups[name]
        print(
            f"{name.ljust(width)}  {bucket['total']:>5}  {bucket['recovered']:>5}  "
            f"{rate(bucket['recovered'], bucket['total']):>7.2f}%"
        )

    cycle = _sum(groups, lambda n: n.startswith("radio_cycle"))
    skip = _sum(groups, lambda n: n.startswith("skip_recovery"))
    control = groups.get("no_cycle / attempt>=1", {"recovered": 0, "total": 0})
    cold_start = groups.get("no_cycle / attempt=0（不参与对照）", {"recovered": 0, "total": 0})

    print()
    print(f"实验组 radio_cycle（开关后首次重连）: {cycle['total']:>5} 次，"
          f"恢复 {cycle['recovered']:>3} 次 → {rate(cycle['recovered'], cycle['total']):.2f}%")
    if skip["total"]:
        print(f"对照组 skip_recovery（同一决策点未开关）: {skip['total']:>5} 次，"
              f"恢复 {skip['recovered']:>3} 次 → {rate(skip['recovered'], skip['total']):.2f}%")
    print(f"对照组 no_cycle  （同事件内普通重试）: {control['total']:>5} 次，"
          f"恢复 {control['recovered']:>3} 次 → {rate(control['recovered'], control['total']):.2f}%")
    print(f"（参考）进程冷启动 attempt=0        : {cold_start['total']:>5} 次，"
          f"恢复 {cold_start['recovered']:>3} 次 → "
          f"{rate(cold_start['recovered'], cold_start['total']):.2f}%  ← 不参与对照：那是健康态首次连接")

    # 对照组口径：优先 skip_recovery（同一决策时刻，可比性最好），
    # 旧版日志没有决策标记时退回到 no_cycle / attempt>=1。
    if cycle["total"] < MIN_SAMPLE:
        print()
        print(f"结论：**样本不足**——实验组只有 {cycle['total']} 条（需 ≥{MIN_SAMPLE}），"
              "说明这段日志里几乎没有执行过 Off/On。")
        return 0
    if skip["total"] < MIN_SAMPLE and control["total"] < MIN_SAMPLE:
        print()
        print(f"结论：**样本不足**（每组需 ≥{MIN_SAMPLE} 条）。")
        print(f"  实验组 radio_cycle  {cycle['total']} 条")
        print(f"  对照组 skip_recovery {skip['total']} 条 / no_cycle {control['total']} 条")
        print("  两组都没有够量的样本时无法对照——这不是脚本故障。")
        if skip["total"] == 0 and cycle["total"] >= MIN_SAMPLE:
            print("  注：只看得到实验组，通常是分流已生效——僵死态不再执行 Off/On，")
            print("      因此不再产生对照样本。此时应直接引用")
            print("      Testing/WindowsBleResourceRecovery.md 的既有结论，不必重算。")
        return 0

    if skip["total"] >= MIN_SAMPLE:
        control_used, control_label = skip, "skip_recovery（同一决策点未开关）"
        # 分流上线后，分组由错误码决定：僵死码 → skip、其它 → cycle。
        # 此时两组不同质（僵死态本身更难恢复），比出来的"有效"是假阳性。
        wedged_share = sum(
            bucket["total"]
            for name, bucket in groups.items()
            if name.startswith("skip_recovery")
            and any(marker in name for marker in WEDGED_MARKERS)
        ) / max(skip["total"], 1)
        if wedged_share >= 0.5:
            print()
            print(f"⚠️  对照组里 {wedged_share * 100:.0f}% 是僵死码样本——两组是按错误码分开的，")
            print("    不是同一错误码内的随机对照。僵死态本身更难恢复，")
            print("    因此本表比出的差异**不能**作为「Off/On 有效/无效」的依据。")
            print()
            print("结论：**不可判定**（对照组由僵死码构成，两组不同质）。")
            print("  要下结论必须在同一错误码内做对照：")
            print("  在 bluetooth_radio 里加诊断开关，让同一错误码下交替决定是否执行 Off/On。")
            print("  僵死态的结论直接引用 Testing/WindowsBleResourceRecovery.md 的既有表格。")
            return 0
    else:
        control_used, control_label = control, "no_cycle（同事件内普通重试）"

    print()
    print(f"（本次对照组口径：{control_label}）")

    rate_cycle = rate(cycle["recovered"], cycle["total"])
    rate_control = rate(control_used["recovered"], control_used["total"])
    lo_c, hi_c = wilson_interval(cycle["recovered"], cycle["total"])
    lo_k, hi_k = wilson_interval(control_used["recovered"], control_used["total"])
    print(f"  实验组恢复率 95% 置信区间: {lo_c:.2f}% – {hi_c:.2f}%")
    print(f"  对照组恢复率 95% 置信区间: {lo_k:.2f}% – {hi_k:.2f}%")

    diff = rate_cycle - rate_control
    if abs(diff) < MATERIAL_DIFFERENCE_PP:
        print(
            f"结论：**无线电 Off/On 无效**。两组恢复率相差 {diff:+.2f} 个百分点"
            f"（判定阈值 ±{MATERIAL_DIFFERENCE_PP}），没有可观测的边际价值——"
            "\n恢复是普通重连自己等到的，不是开关换来的。应删除 Off/On，只保留按错误码分流。"
        )
        # 别把"测不出差异"说过头：CI 上限高说明实验本身分辨力不足。
        ceiling = max(hi_c, hi_k)
        if ceiling > MATERIAL_DIFFERENCE_PP:
            print(
                f"\n⚠️  但注意：置信区间上限达 {ceiling:.2f}%，说明本次样本还不足以排除"
                f"\n    「恢复率其实在 {MATERIAL_DIFFERENCE_PP:.0f}% 左右、只是没被观测到」。"
                "\n    对「是否该继续投入开关」的决策够用（不值得为不超过该上限的收益"
                "保留一条会打断链路的路径），"
                "\n    但不要引用成「开关完全零效果」的普适结论。"
            )
    elif diff > 0:
        print(
            f"结论：**无线电 Off/On 有效**。开关组恢复率高 {diff:+.2f} 个百分点，应保留。"
        )
    else:
        print(
            f"结论：**无线电 Off/On 无效且有害**。开关组恢复率反而低 {abs(diff):.2f} 个百分点"
            "（开关本身会打断链路），应删除。"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
