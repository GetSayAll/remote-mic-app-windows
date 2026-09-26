"""RC003 WUDF 宿主 IOCTL **写入**（拦截）最小实验 —— 需一次提权，不重启、不改系统设置。

要验证的命题
------------
只读实验已证明目标三键（`0x00F1` 返回 / `0x0080` 音量+ / `0x0081` 音量-）在
`WUDFHost.exe` 内的报告层**真实存在**，只是被 `kbdhid` 的 HID→VK 映射丢弃。
本实验回答下一问：**我们在 `onEnter` 改写的字节，是否被 Windows 的键码翻译采纳？**

做法：两条独立通道交叉验证
--------------------------
1. **宿主侧（写入方）**：Frida agent 挂钩 `ntdll!NtDeviceIoControlFile`，命中
   `IOCTL 0x80018483` 时改写 / 擦除报告里的 usage 槽（只碰偏移 3..8），
   `onLeave` 再把原始字节写回（零残留）。
2. **Windows 侧（观测方）**：同一进程内的 Raw Input 后台监听（`RIDEV_INPUTSINK`），
   按 `hDevice` 做**设备归属**，只统计来自 RC003 的键盘事件。

只有"写入发生"和"翻译层看到的确实变了"同时成立，才能说改写生效。任一侧单独成立
都不够——这正是 2026-09-22 那次层错位判定栽的坑。

阶段设计（A/D 同键对照 + 改写/擦除两个方向）
-------------------------------------------
===========  ==============  ==========================================  ==========================
阶段         模式            用户操作                                     期望（Windows 侧，仅 RC003）
===========  ==============  ==========================================  ==========================
pre          observe         唤醒遥控器                                   —
A            observe         确定 ×2                                      VK_RETURN ×2（阳性对照）
B1           rewrite→0x0028  返回 ×2、音量+ ×1、音量- ×1                  VK_RETURN ×4
B2           rewrite→0x0068  返回 ×2、音量+ ×1、音量- ×1                  VK_F13 ×4（无歧义：没有别的来源）
C            erase-target    音量+ ×2、音量- ×2，再按 确定 ×1             目标键 0 事件；确定 仍 VK_RETURN
D            erase-all       确定 ×2（可再按 主页 ×1）                    **0 事件**（与 A 同键对照）
E            observe         确定 ×2                                      VK_RETURN 恢复（无残留）
===========  ==============  ==========================================  ==========================

- **A 是 D 的对照**：同一个键、同一台设备、同一次运行，A 有事件而 D 没有 ⇒ 擦除确
  实生效。"零事件"单独出现时是不可信的（只读基线里三键本来就是零事件）。
- **B1/B2 是写入是否被采纳的正向证据**：改写前该键在 Windows 侧零事件，
  改写后出现按键 ⇒ 下游只看到我们写入的内容，写入发生在翻译之前。
- **B2 用 F13（usage 0x0068）**：本机没有任何东西会自然产生 VK_F13，
  因此它不依赖"用户按对了键"这一前提。若 B2 无事件而 B1 有事件，
  说明该 usage 在本机未被映射，需要换 substitute——这是结论而非故障。

安全边界
--------
- 一次性、可交互的 UAC 提权；**不装内核驱动、不改 Secure Boot / 测试签名 / 驱动
  签名策略、不写注册表、不装计划任务、不需重启**。
- 只写报告缓冲区的偏移 3..8，且 `onLeave` 回写复原；累计写入次数有上限。
- 计划表走完 + 3 秒宽限期后 agent 永久解除武装；进程退出即卸载注入脚本。
- 唯一的系统级副作用：`rewrite` 阶段会把目标键改写为 确定（Enter）或 F13，
  因此实验期间请把鼠标焦点留在桌面或空白区域，不要停在文本编辑器里。
- 最坏情况：宿主异常崩溃 → Windows 自动重启宿主并重新枚举该设备（遥控器重连即可），
  **不需要重启电脑**。

用法
----
    python wudf_ioctl_write.py --out <log>                # 真机全程（需提权，约 73 秒）
    python wudf_ioctl_write.py --out <log> --phases A,B2  # 只补采指定阶段（约 25 秒）
    python wudf_ioctl_write.py --dry-run                  # 只定位宿主 + 提权自检，不注入
    python wudf_ioctl_write.py --selftest                 # 分析层自检（合成日志），不注入
    python wudf_ioctl_write.py --analyze <log> [<log>…]   # 离线复核，可多份合并判定

退出码
------
     0  interception_effective    改写与擦除均在翻译层生效，且停止写入后恢复
     1  定位/参数问题（未连设备、--phases 过滤后为空）
     3  elevation_required        未提权
     4  依赖缺失（frida / Raw Input / JS 文件）
     5  attach_failed             注入被拒（多为 EDR/Defender）
     6  hook_export_null          导出地址不可用
     7  日志不可解析 / 无可判定阶段
     8  no_positive_control       阶段 A 无对照 ⇒ 观测通道不可信，其余一律不采信
     9  rewrite_not_effective     改写未被翻译层采纳
    10  erase_not_effective       擦除未被采纳
    11  not_restored              停止写入后未恢复
    12  phase_not_observed        阶段零命中（**采集缺失**，补采即可，非机制失败）
    13  substitute_not_mapped     改写已发生但替换 usage 未被映射（**技术结论**）

补采说明：`--phases` 只跑部分阶段时，判定只针对实际执行的阶段（未执行标 SKIP）。
务必把阳性对照 A 一起跑，否则本次运行没有可用的观测基准。

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

from raw_input_sink import RawInputWatcher  # noqa: E402
from wudf_host_probe import probe_open_denied, scan_hosts  # noqa: E402
from wudf_ioctl_tap import decode_report, usage_name  # noqa: E402

JS_PATH = HERE / "wudf_ioctl_write.js"
TARGET_IOCTL = 0x80018483
TARGET_USAGES = (0x00F1, 0x0080, 0x0081)
SUBSTITUTE_NAMES = {0x0028: "确定/Enter", 0x0068: "F13"}

# 必须与 JS 侧 `GRACE_MS` 保持一致。agent 只在「计划表总时长 + 宽限期」之后才把
# `disarmed` 置真，因此收尾取统计前必须等过宽限期，否则读到的恒为 False，
# 这条 fail-safe 就等于从未被验证过（2026-09-23 两轮日志都读到 False 即此因）。
GRACE_SECONDS = 3.0

VK_RETURN = 0x0D
VK_F13 = 0x7C

# (名称, 秒数, 模式, substitute, 提示语)
PHASES: list[tuple[str, float, str, int, str]] = [
    ("pre", 10, "observe", 0,
     "预热：先别按。请唤醒遥控器（按一下遥控器上任意键）。"),
    ("A", 10, "observe", 0,
     "基线对照：请按 确定 ×2。期望 Windows 侧出现 VK_RETURN。"),
    ("B1", 10, "rewrite", 0x0028,
     "改写→确定：请按 返回 ×2、音量+ ×1、音量- ×1（本阶段不要按 确定）。期望这 4 次都变成 VK_RETURN。"),
    ("B2", 12, "rewrite", 0x0068,
     "改写→F13：请按 返回 ×2、音量+ ×1、音量- ×1（不要按 确定）。期望出现 VK_F13（只有我们的写入能产生它）。"),
    ("C", 10, "erase-target", 0,
     "选择性擦除：先按 音量+ ×2、音量- ×2，再按 确定 ×1。期望：音量± 无任何事件；确定 仍出现 VK_RETURN。"),
    ("D", 10, "erase-all", 0,
     "全量擦除：请按 确定 ×2（可再按 主页 ×1）。期望：**无任何事件**——与阶段 A 是同一个键。"),
    ("E", 10, "observe", 0,
     "解除武装：请按 确定 ×2。期望 VK_RETURN 恢复出现。"),
    ("post", 3, "observe", 0,
     "收尾：请停手。"),
]

_log_lines: list[str] = []
_log_file: Path | None = None


def log(message: str = "") -> None:
    print(message, flush=True)
    _log_lines.append(message)
    if _log_file is not None:
        _log_file.write_text("\n".join(_log_lines) + "\n", encoding="utf-8")


def sanitize(value: object) -> str:
    """把可能含空格/换行的字段压成单 token，保证日志行整体可被 key=value 解析。"""
    text = "" if value is None else str(value)
    return re.sub(r"\s+", "_", text.strip()) or "-"


def build_schedule(scale: float, only: list[str] | None = None) -> list[dict]:
    """构造计划表。`only` 非空时只保留这些阶段（顺序仍按 PHASES）。

    短跑模式（例如 `--phases A,B2`）用于补采个别缺失阶段：不必重跑全程，
    但**务必让阳性对照阶段 A 留在里面**，否则本次运行没有可用的观测基准。
    """
    wanted = set(only) if only else None
    return [
        {
            "name": name,
            "ms": max(1000, int(round(seconds * scale * 1000))),
            "mode": mode,
            "substitute": substitute,
        }
        for name, seconds, mode, substitute, _hint in PHASES
        if wanted is None or name in wanted
    ]


def describe_report(out_hex: str) -> str:
    decoded = decode_report(out_hex) if out_hex else None
    if decoded is None:
        return ""
    if decoded["report_id"] != 1:
        return f" report_id=0x{decoded['report_id']:02X}"
    names = [f"0x{u:04X}({usage_name(u)})" for u in decoded["active"]]
    return f" report_id=0x01 usage={names or '[]'}"


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


# --------------------------------------------------------------------------
# 记录解析（在线与离线共用）
# --------------------------------------------------------------------------

KV_RE = re.compile(r"^(?P<tag>\[(?:TAP|WIN)\]) (?P<body>.*)$")


def parse_kv(body: str) -> dict:
    """解析 `[TAP]`/`[WIN]` 行的前缀键值对；`|` 之后是人性化说明，不参与解析。"""
    fields: dict[str, str] = {}
    head = body.split("|", 1)[0]
    for token in head.split():
        if "=" in token:
            key, value = token.split("=", 1)
            fields[key] = value
    return fields


def load_records(text: str) -> tuple[list[dict], list[dict]]:
    """从日志文本解析 [TAP] 与 [WIN] 记录。"""
    taps: list[dict] = []
    wins: list[dict] = []
    for line in text.splitlines():
        m = KV_RE.match(line.strip())
        if not m:
            continue
        fields = parse_kv(m.group("body"))
        if m.group("tag") == "[TAP]":
            fields["mutated"] = fields.get("mutated") == "True"
            # 日志行用 orig/new（紧凑），在线 payload 用 orig_hex/new_hex——
            # 统一成后者，避免离线复核时读不到改写内容。
            if "orig" in fields:
                fields["orig_hex"] = fields.pop("orig")
            if "new" in fields:
                fields["new_hex"] = fields.pop("new")
            for key in ("t", "idx", "in_len", "out_len", "ret"):
                if key in fields:
                    try:
                        fields[key] = int(fields[key])
                    except ValueError:
                        pass
            taps.append(fields)
        else:
            if fields.get("rc003") != "True":
                continue
            if "vk" in fields:
                fields["vk"] = int(fields["vk"], 16)
            if "msg" in fields:
                fields["msg"] = int(fields["msg"], 16)
            wins.append(fields)
    return taps, wins


# --------------------------------------------------------------------------
# 判定：只从两类原始记录推导
# --------------------------------------------------------------------------


def _presses(wins: list[dict], phase: str, vk: int) -> int:
    return sum(
        1
        for w in wins
        if w.get("phase") == phase and w.get("kind") == "kb"
        and w.get("msg") == 0x0100 and w.get("vk") == vk
    )


def _rc003_events(wins: list[dict], phase: str) -> int:
    return sum(1 for w in wins if w.get("phase") == phase)


def _mutations(taps: list[dict], phase: str, mode: str | None = None) -> int:
    return sum(
        1
        for r in taps
        if r.get("dir") == "enter" and r.get("phase") == phase
        and r.get("mutated") and (mode is None or r.get("mode") == mode)
    )


def _pairs(taps: list[dict], phase: str) -> list[str]:
    seen: list[str] = []
    for r in taps:
        if r.get("dir") != "enter" or r.get("phase") != phase or not r.get("mutated"):
            continue
        pair = f"{r.get('orig_hex')}→{r.get('new_hex')}"
        if pair not in seen:
            seen.append(pair)
    return seen


ASSERTION_PHASES: dict[str, str] = {
    "A_control_present": "A",
    "B1_rewrite_effective": "B1",
    "B2_rewrite_f13": "B2",
    "C_target_erased_at_layer": "C",
    "C_collateral_ok": "C",
    "D_erase_effective": "D",
    "E_restored": "E",
}


def verdict(taps: list[dict], wins: list[dict],
            active: list[str] | None = None) -> tuple[dict[str, bool], int]:
    """按**实际执行过的阶段**判定；省略 `active` 时按全部阶段判定。

    为什么必须区分：补采个别阶段（如 `--phases A,B2`）时，未执行阶段的断言
    既不能算通过也不能算失败。若一律按全量判定，补跑会被误判成
    "改写未生效"（B1 缺失），把一次正常补采报成机制失败。
    """
    phases = [p[0] for p in PHASES]
    if active is None:
        active = list(phases)
    active_set = set(active)
    log("\n=== 判定（仅由宿主侧写入记录 + Windows 侧按键事件推导）===")
    if active_set != set(phases):
        log(f"（本次只执行了阶段 {'/'.join(active)}，其余阶段不参与判定）")

    log("\n[1] Windows 侧按键事件（仅 RC003 设备，按下计数）")
    for name in active:
        total = _rc003_events(wins, name)
        detail = []
        for vk, label in ((VK_RETURN, "VK_RETURN"), (VK_F13, "VK_F13"),
                          (0x24, "VK_HOME"), (0x41, "VK_A")):
            count = _presses(wins, name, vk)
            if count:
                detail.append(f"{label}×{count}")
        log(f"  {name:<5} 事件 {total:>2}  " + ("  ".join(detail) if detail else "（无）"))

    log("\n[2] 宿主侧写入记录（report 9 字节，只动偏移 3..8）")
    for name in active:
        enters = sum(
            1 for r in taps
            if r.get("dir") == "enter" and r.get("phase") == name
        )
        mutated = _mutations(taps, name)
        log(f"  {name:<5} 命中 {enters:>2} 次  实际改写 {mutated:>2} 次")
        for pair in _pairs(taps, name):
            log(f"        {pair}")
    write_fail = sum(
        1
        for r in taps
        if r.get("dir") == "enter"
        and r.get("write_err") not in (None, "", "-")
        and not r.get("mutated")
        and r.get("mode") in ("rewrite", "erase-target", "erase-all")
    )
    restored = sum(
        1 for r in taps if r.get("dir") == "leave" and r.get("detail") == "restored"
    )
    kernel_changed = sum(
        1 for r in taps
        if r.get("dir") == "leave" and str(r.get("detail", "")).startswith("kernel_changed")
    )
    log(f"\n  写入失败/被闸门拦下: {write_fail}")
    log(f"  onLeave 已回写复原: {restored}")
    log(f"  onLeave 发现内核改写过缓冲区: {kernel_changed}（>0 则放弃回写并如实上报）")

    results: dict[str, bool] = {}
    results["A_control_present"] = _presses(wins, "A", VK_RETURN) >= 1
    results["B1_rewrite_effective"] = (
        _presses(wins, "B1", VK_RETURN) >= 1 and _mutations(taps, "B1") >= 1
    )
    results["B2_rewrite_f13"] = (
        _presses(wins, "B2", VK_F13) >= 1 and _mutations(taps, "B2") >= 1
    )
    results["C_target_erased_at_layer"] = _mutations(taps, "C") >= 1
    results["C_collateral_ok"] = _presses(wins, "C", VK_RETURN) >= 1
    results["D_erase_effective"] = (
        _mutations(taps, "D") >= 1
        and _presses(wins, "D", VK_RETURN) == 0
        and _rc003_events(wins, "D") == 0
    )
    results["E_restored"] = _presses(wins, "E", VK_RETURN) >= 1

    labels = {
        "A_control_present": "A 阳性对照：确定 → VK_RETURN",
        "B1_rewrite_effective": "B1 改写生效（目标键 → VK_RETURN）",
        "B2_rewrite_f13": "B2 改写生效（目标键 → VK_F13，无歧义）",
        "C_target_erased_at_layer": "C 目标键已在报告层被擦除",
        "C_collateral_ok": "C 非目标键（确定）未被波及",
        "D_erase_effective": "D 擦除生效（确定 被静默）",
        "E_restored": "E 停止写入后恢复",
    }
    log("\n[3] 分项结论")
    live = [k for k in results if ASSERTION_PHASES[k] in active_set]
    for key, ok in results.items():
        if key not in live:
            log(f"  SKIP  {labels[key]}（本次未执行）")
            continue
        log(f"  {'PASS' if ok else 'FAIL'}  {labels[key]}")

    if not live:
        log("\n判定: no_assertion_executed（没有任何可判定的阶段）")
        return results, 7

    if "A" in active_set and not results["A_control_present"]:
        log("\n判定: no_positive_control（阶段 A 没读到确定的 VK_RETURN）")
        log("说明: Windows 侧观测通道本身没工作，其它阶段一律不可信，需重跑。")
        return results, 8
    if "B1" in active_set and not results["B1_rewrite_effective"]:
        log("\n判定: rewrite_not_effective（改写未被翻译层采纳）")
        log("说明: 报告里已写入，但 Windows 侧看不到对应按键。可能原因：")
        log("      (a) 写入发生在内核复制之后（时序判据需重估）；")
        log("      (b) 该缓冲区不是被交付给 hidclass 的那一份；")
        log("      (c) 写入被静默丢弃（页保护只读）。日志里 orig→new 与 readback 可区分。")
        return results, 9
    if "D" in active_set and not results["D_erase_effective"]:
        log("\n判定: erase_not_effective（擦除后仍有事件）")
        log("说明: 与阶段 A 的同键对照显示确定仍可达翻译层，擦除未被采纳。")
        return results, 10
    if "E" in active_set and not results["E_restored"]:
        log("\n判定: not_restored（停止写入后未恢复）")
        log("说明: 阶段 E 没读到 VK_RETURN，可能存在残留补丁或设备状态异常。")
        return results, 11

    failed = [k for k in live if not results[k]]
    if failed:
        if failed == ["B2_rewrite_f13"]:
            b2_mut = _mutations(taps, "B2")
            b2_enters = sum(
                1 for r in taps
                if r.get("dir") == "enter" and r.get("phase") == "B2"
            )
            if b2_mut >= 1:
                log("\n判定: substitute_not_mapped（B2 已改写，但替换 usage 未被翻译层映射）")
                log(f"说明: B2 实际改写 {b2_mut} 次，Windows 侧却没有 VK_F13。")
                log("      这是**结论**而非故障：该 usage 在本机 hidclass/kbdhid 路径上未被")
                log("      映射成 F13。改写机制本身已由 B1 独立证明；")
                log("      产品化选取替换键时必须逐键实测，不能假定任意 usage 都能用。")
                return results, 13
            log("\n判定: phase_not_observed（B2 窗口内没有任何报告通过，无法判定）")
            log(f"说明: B2 命中 {b2_enters} 次、改写 {b2_mut} 次——是采集缺失，不是机制失败。")
            log("      补跑命令：--phases A,B2（约 25 秒，仍需一次提权）。")
            return results, 12
        log(f"\n判定: partial_failure（未通过：{', '.join(failed)}）")
        log("说明: 关键链（A 对照 / B1 改写 / D 擦除 / E 恢复）之外的分项未通过，")
        log("      按上面 [1][2] 的原始计数定位。")
        return results, 12

    log("\n判定: interception_effective（改写与擦除均在翻译层生效，且停止写入后恢复）")
    log("说明: 阶段 A 与阶段 D 是同一个键（确定）的对照——A 有事件、D 无事件，")
    log("      而两者唯一的差别是我们在报告层写了字节；阶段 B1 从零事件变为 VK_RETURN、")
    log("      B2 从零事件变为 VK_F13（本机没有别的来源能自然产生 F13），")
    log("      共同证明写入发生在 Windows 键码翻译之前。")
    if active_set != set(phases):
        log(f"注意: 本次只覆盖 {'/'.join(active)}，其余阶段未验。")
    return results, 0


def _synth_log(*, with_control: bool, d_silent: bool,
               include: set[str] | None = None,
               b2_state: str = "ok") -> str:
    """构造一份合成日志（自检专用，不来自真机），格式与真机日志完全一致。

    `include` 模拟 `--phases` 补采：只生成这些阶段的记录**以及对应的时间轴段**，
    使自检能走完 `detect_phases → verdict(active=…)` 的真实链路。
    `b2_state` 三态：`ok`（有改写有 F13）/ `no_event`（有改写无 F13）/
    `no_report`（窗口内没有任何报告通过）。
    """
    out = ["=== 合成日志（自检用）==="]

    def on(name: str) -> bool:
        return include is None or name in include

    def tap(t, phase, mode, direction, orig, new=None, mutated=False,
            write_err="-", detail="-"):
        if not on(phase):
            return
        shown = new if new else orig
        if direction == "enter":
            out.append(
                f"[TAP] t={t} phase={phase} pyp={phase} idx=0 mode={mode} dir=enter "
                f"in_len=8 out_len=9 orig={orig} new={shown} mutated={mutated} "
                f"write_err={write_err} |"
            )
        else:
            out.append(
                f"[TAP] t={t} phase={phase} pyp={phase} idx=0 mode={mode} dir=leave "
                f"ret=0 out={shown} detail={detail} |"
            )

    def win(t, phase, vk, msg=0x0100):
        if not on(phase):
            return
        out.append(
            f"[WIN] t={t} phase={phase} kind=kb vk=0x{vk:02X} make=0x1C flags=0x0000 "
            f"msg=0x{msg:04X} rc003=True |  <<< RC003"
        )

    zero = "010000000000000000"
    ok_report = "010000280000000000"
    back = "010000f10000000000"
    volume_up = "010000800000000000"

    tap(1100, "pre", "observe", "enter", zero)
    tap(2100, "A", "observe", "enter", ok_report)
    if with_control:
        win(2100, "A", VK_RETURN)
        win(2150, "A", VK_RETURN, 0x0101)

    tap(3100, "B1", "rewrite", "enter", back, "010000280000000000", True)
    tap(3100, "B1", "rewrite", "leave", back, "010000280000000000", True,
        detail="restored")
    win(3100, "B1", VK_RETURN)

    if b2_state != "no_report":
        tap(4100, "B2", "rewrite", "enter", back, "010000680000000000", True)
        tap(4100, "B2", "rewrite", "leave", back, "010000680000000000", True,
            detail="restored")
        if b2_state == "ok":
            win(4100, "B2", VK_F13)

    tap(5100, "C", "erase-target", "enter", volume_up, zero, True)
    win(5100, "C", VK_RETURN)

    tap(6100, "D", "erase-all", "enter", ok_report, zero, True)
    if not d_silent:
        win(6100, "D", VK_RETURN)

    tap(7100, "E", "observe", "enter", ok_report)
    win(7100, "E", VK_RETURN)

    # 时间轴段：真实日志里有，判定靠它解析"本次实际执行过哪些阶段"。
    # 合成日志也带上，自检才覆盖 detect_phases → verdict(active) 整条链路。
    out.append("--- 阶段时间轴（本地时钟）---")
    cursor = 1790134399712
    for name, seconds, _mode, _sub, _hint in PHASES:
        if not on(name):
            continue
        cursor_end = cursor + int(seconds * 1000)
        out.append(f"  {name:<5} {cursor} .. {cursor_end}")
        cursor = cursor_end

    return "\n".join(out) + "\n"


def selftest() -> int:
    """分析层自检：合成日志走与真机完全相同的 parse + verdict 路径。

    对应真机侧的"阳性对照"要求——判定逻辑本身没被验证过，"PASS" 就没有意义。
    三个用例分别覆盖：全项通过、阳性对照缺失、擦除未生效。
    """
    cases = [
        ("全程 · 全项通过", {"with_control": True, "d_silent": True}, 0),
        ("全程 · 阳性对照缺失", {"with_control": False, "d_silent": True}, 8),
        ("全程 · 擦除未生效", {"with_control": True, "d_silent": False}, 10),
        ("补采 A,B2 · 均通过", {"with_control": True, "d_silent": True,
                                "include": {"A", "B2"}}, 0),
        ("补采 A,B2 · B2 零命中", {"with_control": True, "d_silent": True,
                                   "include": {"A", "B2"}, "b2_state": "no_report"}, 12),
        ("补采 A,B2 · 有改写无 F13", {"with_control": True, "d_silent": True,
                                      "include": {"A", "B2"}, "b2_state": "no_event"}, 13),
    ]
    all_ok = True
    for label, kwargs, expected in cases:
        text = _synth_log(**kwargs)
        taps, wins = load_records(text)
        active = detect_phases([text])
        log(f"\n########## 自检用例：{label}（期望退出码 {expected}）##########")
        log(f"（解析到 宿主侧记录 {len(taps)} 条 / Windows 侧 RC003 事件 {len(wins)} 条 / "
            f"执行阶段 {'/'.join(active) if active else '全部'}）")
        _, code = verdict(taps, wins, active)
        log(f"实际退出码 {code} —— {'符合期望' if code == expected else '不符合期望'}")
        all_ok = all_ok and code == expected
    log(f"\n=== 分析层自检{'通过' if all_ok else '失败'} ===")
    return 0 if all_ok else 1


TIMELINE_RE = re.compile(
    r"^\s{2}([A-Za-z][A-Za-z0-9_]*)\s+\d{10,}\s+\.\.\s+\d{10,}\s*$", re.M
)


def detect_phases(texts: list[str]) -> list[str] | None:
    """从日志的「阶段时间轴」段解析本次实际执行过的阶段。

    这是唯一权威来源：只按记录推断会漏掉零命中的阶段（例如 B2 没按到），
    而正是这种阶段最需要被如实判为"未采到"。
    找不到时间轴段（如合成日志）时返回 None，由调用方按全量处理。
    """
    order = [p[0] for p in PHASES]
    found: set[str] = set()
    for text in texts:
        found.update(TIMELINE_RE.findall(text))
    if not found:
        return None
    return [name for name in order if name in found]


def analyze(paths: list[Path]) -> int:
    """离线复核。可传多份日志做**合并判定**——例如全程日志 + 补采日志，
    合并后覆盖的阶段取并集，结论与"一次跑满全程"等价（不提权、不需设备在线）。
    """
    texts: list[str] = []
    taps: list[dict] = []
    wins: list[dict] = []
    for path in paths:
        text = path.read_text(encoding="utf-8", errors="replace")
        texts.append(text)
        part_taps, part_wins = load_records(text)
        log(f"=== 离线复核: {path} ===")
        log(f"  解析到 宿主侧记录 {len(part_taps)} 条、Windows 侧 RC003 事件 {len(part_wins)} 条")
        taps.extend(part_taps)
        wins.extend(part_wins)

    if len(paths) > 1:
        log(f"（合并 {len(paths)} 份日志：宿主侧 {len(taps)} 条、Windows 侧 {len(wins)} 条）")
    if not taps:
        log("判定: log_unparseable（没有解析到 [TAP] 记录）")
        return 7
    if not any(r.get("dir") == "enter" and r.get("mutated") for r in taps):
        log("判定: no_mutation_in_log（日志里没有任何实际改写）")
        return 7

    active = detect_phases(texts)
    if active:
        log(f"从时间轴解析到实际执行过的阶段: {'/'.join(active)}")
    return verdict(taps, wins, active)[1]


# --------------------------------------------------------------------------
# 真机运行
# --------------------------------------------------------------------------


def run(scale: float, out: Path | None, do_watch: bool,
        only: list[str] | None = None) -> int:
    global _log_file
    if out:
        out.parent.mkdir(parents=True, exist_ok=True)
        _log_file = out

    schedule = build_schedule(scale, only)
    if not schedule:
        valid = "/".join(p[0] for p in PHASES)
        log(f"判定: no_phase_selected（--phases 过滤后没有任何阶段；可选 {valid}）")
        return 1
    total_ms = sum(p["ms"] for p in schedule)

    log("=== RC003 WUDF 宿主 IOCTL 写入（拦截）最小实验 ===")
    log(f"时间: {datetime.now().isoformat(timespec='seconds')}")
    log("契约: 只写报告偏移 3..8 并在 onLeave 回写复原；不改注册表 / 驱动策略 /"
        " 不装计划任务 / 需一次交互式 UAC 提权；计划表结束即解除武装。")
    log("副作用: 改写阶段会把目标键变成 确定(Enter) 或 F13，"
        "请把焦点留在桌面或空白区域，不要停在文本编辑器里。")

    log("\n--- 步骤 1/6：定位独占 RC003 的 WUDFHost ---")
    host_pid = find_exclusive_rc003_host()
    if host_pid is None:
        log("判定: rc003_wudf_host_absent")
        log("说明: 请先连接并唤醒 RC003 遥控器后重跑。")
        return 1
    log(f"  命中 exclusive_rc003_host，HostPid={host_pid} (0x{host_pid:x})")

    log("\n--- 步骤 2/6：提权与可注入性自检 ---")
    admin = bool(ctypes.windll.shell32.IsUserAnAdmin())
    denied = probe_open_denied(host_pid)
    log(f"  当前进程管理员令牌: {admin}")
    log(f"  OpenProcess(目标宿主) 被拒: {denied}")
    if not (admin and not denied):
        log("判定: elevation_required")
        log("说明: 宿主位于 session 0，普通权限对其 OpenProcess 返回 err=5。")
        log("      请用「以管理员身份运行」的终端重跑本脚本。")
        return 3

    watcher: RawInputWatcher | None = None
    if do_watch:
        log("\n--- 步骤 3/6：启动 Windows 侧 Raw Input 监听（独立观测点）---")

        def on_event(record: dict) -> None:  # noqa: ANN001
            if record["kind"] == "kb":
                log(f"[WIN] t={record['t']:.0f} phase={phase_of(record['t'])} kind=kb "
                    f"vk=0x{record['vk']:02X} make=0x{record['make']:02X} "
                    f"flags=0x{record['flags']:04X} msg=0x{record['msg']:04X} "
                    f"rc003={record['rc003']}"
                    + ("  <<< RC003" if record["rc003"] else ""))
            else:
                log(f"[WIN] t={record['t']:.0f} phase={phase_of(record['t'])} "
                    f"kind={record['kind']} rc003={record['rc003']}")

        watcher = RawInputWatcher(on_event=on_event)
        if not watcher.start():
            log(f"判定: raw_input_sink_failed ({watcher.error})")
            return 4
        for page, usage, note, ok, err in watcher.registration_ok:
            log(f"  注册 {'成功' if ok else f'失败 err={err}'} "
                f"page=0x{page:04X} usage=0x{usage:04X}  {note}")
    else:
        log("\n--- 步骤 3/6：跳过 Windows 侧监听（--no-watch）---")

    try:
        import frida
    except ImportError:
        log("判定: frida_missing")
        log("说明: 请先安装：pip install frida")
        if watcher:
            watcher.stop()
        return 4
    log(f"  frida {frida.__version__}")

    try:
        js_source = JS_PATH.read_text(encoding="utf-8")
    except OSError as exc:
        log(f"判定: js_missing ({exc})")
        if watcher:
            watcher.stop()
        return 4
    if "__SCHEDULE_JSON__" not in js_source:
        log("判定: js_missing_placeholder（计划表占位符不存在，拒绝注入）")
        if watcher:
            watcher.stop()
        return 4
    js_source = js_source.replace("__SCHEDULE_JSON__", json.dumps(schedule))

    log("\n--- 步骤 4/6：attach 并安装写入钩子 ---")
    try:
        session = frida.attach(host_pid)
    except Exception as exc:  # noqa: BLE001
        log(f"判定: attach_failed -> {type(exc).__name__}: {exc}")
        log("说明: 常见原因——未提权；或 EDR/Defender 拦截向系统进程注入。")
        if watcher:
            watcher.stop()
        return 5

    taps: list[dict] = []
    wins: list[dict] = []
    ready: dict = {}
    heartbeats: list[dict] = []
    unknown_kinds: dict[str, int] = {}
    boundaries: list[tuple[float, float, str]] = []

    def phase_of(t: float) -> str:
        for start, end, name in boundaries:
            if start <= t <= end:
                return name
        return "?"

    def on_message(message, data):  # noqa: ANN001, ARG001
        if message.get("type") == "error":
            log(f"  [frida error] {message}")
            return
        payload = message.get("payload") or {}
        kind = payload.get("type")
        if kind == "ready":
            ready.update(payload)
            log(f"  钩子已安装: NtDeviceIoControlFile，目标 IOCTL={payload.get('target_ioctl')}")
            log(f"  导出地址可用: {payload.get('hook_ok')}")
            log(f"  计划表总时长: {payload.get('total_ms')} ms，写入上限 {payload.get('max_mutations')}")
        elif kind == "ioctl":
            t = payload.get("t") or 0
            py_phase = phase_of(t)
            js_phase = str(payload.get("phase"))
            payload["py_phase"] = py_phase
            taps.append(payload)
            if payload.get("dir") == "enter":
                marker = "" if py_phase == js_phase else f"  ⚠ 阶段不一致(py={py_phase} js={js_phase})"
                tail = describe_report(str(payload.get("orig_hex") or ""))
                if payload.get("mutated"):
                    tail += f"  改写 {payload.get('orig_hex')}→{payload.get('new_hex')}"
                elif payload.get("write_err"):
                    tail += f"  [闸门:{payload.get('write_err')}]"
                log(f"[TAP] t={t} phase={js_phase} pyp={py_phase} idx={payload.get('idx')} "
                    f"mode={payload.get('mode')} dir=enter in_len={payload.get('in_len')} "
                    f"out_len={payload.get('out_len')} orig={payload.get('orig_hex')} "
                    f"new={payload.get('new_hex')} mutated={payload.get('mutated')} "
                    f"write_err={sanitize(payload.get('write_err'))} |{tail}{marker}")
            else:
                log(f"[TAP] t={t} phase={js_phase} pyp={py_phase} idx={payload.get('idx')} "
                    f"mode={payload.get('mode')} dir=leave ret={payload.get('ret')} "
                    f"out={payload.get('out')} detail={sanitize(payload.get('detail'))} |")
        elif kind == "hb":
            heartbeats.append(payload)
            log(f"  [心跳] phase={payload.get('phase')} idx={payload.get('idx')} "
                f"mode={payload.get('mode')} 命中={payload.get('target_calls')} "
                f"改写={payload.get('mutations')} 成功={payload.get('write_ok')} "
                f"失败={payload.get('write_fail')} 复原={payload.get('restored')}"
                + ("【已解除武装】" if payload.get("disarmed") else ""))
        else:
            unknown_kinds[str(kind)] = unknown_kinds.get(str(kind), 0) + 1

    script = session.create_script(js_source)
    script.on("message", on_message)
    script.load()

    deadline = time.time() + 6
    while time.time() < deadline and not ready:
        time.sleep(0.1)

    if not ready.get("hook_ok"):
        log("判定: hook_export_null")
        session_detach(session)
        if watcher:
            watcher.stop()
        return 6

    t0 = float(ready.get("t"))
    log(f"  agent 起始时钟 t0={t0:.0f}（与 Python 同一时钟域，可直接配对）")

    log("\n--- 步骤 5/6：分阶段执行（请按屏幕提示操作遥控器）---")
    cumulative = 0.0
    for item in schedule:
        start = t0 + cumulative
        end = start + item["ms"]
        cumulative += item["ms"]
        boundaries.append((start, end, item["name"]))
        hint = next(h for h in PHASES if h[0] == item["name"])[4]
        while time.time() * 1000.0 < start:
            time.sleep(0.05)
        mode_note = item["mode"]
        if item["mode"] == "rewrite":
            mode_note = f"rewrite→0x{item['substitute']:04X} " \
                        f"({SUBSTITUTE_NAMES.get(item['substitute'], '?')})"
        log(f"\n>>> 阶段 {item['name']}：{hint}\n    模式 {mode_note}  时长 {item['ms'] / 1000:.0f} 秒")

        def phase_enters() -> int:
            return sum(
                1 for r in taps
                if r.get("dir") == "enter" and r.get("phase") == item["name"]
            )

        # 零命中即时预警：阶段过半仍无报告通过时就地提醒。
        # 首轮 B2 整个窗口命中 0 次（8 秒内没按到），事后才发现，白跑一轮；
        # 有了它，漏采在窗口内就能被纠正。
        warn_at = start + item["ms"] * 0.45
        warned = False
        while time.time() * 1000.0 < end:
            if not warned and time.time() * 1000.0 >= warn_at:
                if phase_enters() == 0:
                    log(f"  [预警] 阶段 {item['name']} 已过半但仍无任何报告通过——"
                        f"请确认遥控器已唤醒，并按提示操作。")
                warned = True
            time.sleep(0.05)
        if phase_enters() == 0:
            log(f"  [注意] 阶段 {item['name']} 命中 0 次——该阶段数据不可用（采集缺失）。")

    log("\n--- 步骤 6/6：等待自动解除武装、卸载钩子 ---")
    # 等过宽限期再取统计：`disarmed` 只在「计划表总时长 + GRACE_SECONDS」之后才由 agent
    # 自行置真。此前只等 1.5 秒，读到的恒为 False，等于这条 fail-safe 从未被验证过。
    time.sleep(GRACE_SECONDS + 1.0)
    if heartbeats:
        last = heartbeats[-1]
        log(f"  最终统计: 命中={last.get('target_calls')} 改写={last.get('mutations')} "
            f"成功={last.get('write_ok')} 失败={last.get('write_fail')} "
            f"复原={last.get('restored')} 内核改写={last.get('kernel_changed')} "
            f"自动解除武装={last.get('disarmed')}")
        if not last.get("disarmed"):
            log("  [注意] 宽限期已过但 agent 仍未自行解除武装——这条 fail-safe 未生效，如实上报。")
    session_detach(session)

    if watcher:
        watcher.stop()
        wins = [w for w in watcher.events if w["rc003"]]
        for w in wins:
            w["phase"] = phase_of(w["t"])
        log(f"  Raw Input 监听已停止；收到 RC003 事件 {len(wins)} 条")

    if unknown_kinds:
        log(f"\n[注意] 未识别的上行消息类型: {unknown_kinds}")

    log("\n--- 阶段时间轴（本地时钟）---")
    for b_start, b_end, b_name in boundaries:
        log(f"  {b_name:<5} {b_start:.0f} .. {b_end:.0f}")

    return verdict(taps, wins, [p["name"] for p in schedule])[1]


def session_detach(session) -> None:  # noqa: ANN001
    try:
        session.detach()
    except Exception:  # noqa: BLE001
        pass


def main() -> int:
    parser = argparse.ArgumentParser(description="RC003 WUDF 宿主 IOCTL 写入（拦截）最小实验（需提权）")
    parser.add_argument("--out", type=Path, help="日志落盘路径")
    parser.add_argument("--scale", type=float, default=1.0, help="阶段时长缩放，1.0 为默认约 67 秒")
    parser.add_argument("--no-watch", action="store_true",
                        help="不启动 Windows 侧 Raw Input 监听（只做写入演示）")
    parser.add_argument("--dry-run", action="store_true", help="只做定位与自检，不注入")
    parser.add_argument("--selftest", action="store_true",
                        help="分析层自检：合成日志走同一条 parse+verdict 路径，不注入")
    parser.add_argument("--analyze", type=Path, nargs="+",
                        help="离线复核既有日志（可传多份，合并判定），不注入、不提权")
    parser.add_argument("--phases",
                        help="只跑这些阶段（逗号分隔，如 A,B2），用于补采缺失阶段；"
                             "务必让阳性对照 A 在其中，否则本次运行没有观测基准")
    args = parser.parse_args()

    only: list[str] | None = None
    if args.phases:
        only = [p.strip() for p in args.phases.split(",") if p.strip()]
        valid = {p[0] for p in PHASES}
        unknown = [p for p in only if p not in valid]
        if unknown:
            print(f"未知阶段: {unknown}", file=sys.stderr)
            print(f"可选: {'/'.join(p[0] for p in PHASES)}", file=sys.stderr)
            return 2

    if args.selftest:
        return selftest()

    if args.analyze:
        return analyze(args.analyze)

    if args.dry_run:
        global _log_file
        if args.out:
            args.out.parent.mkdir(parents=True, exist_ok=True)
            _log_file = args.out
        log("=== 写入版实验：dry-run（不注入）===")
        log(f"JS 文件存在: {JS_PATH.exists()}")
        if JS_PATH.exists():
            source = JS_PATH.read_text(encoding="utf-8")
            log(f"计划表占位符存在: {'__SCHEDULE_JSON__' in source}")
        host_pid = find_exclusive_rc003_host()
        log(f"独占 RC003 宿主 PID: {host_pid}")
        if host_pid is not None:
            admin = bool(ctypes.windll.shell32.IsUserAnAdmin())
            denied = probe_open_denied(host_pid)
            log(f"  当前进程管理员令牌: {admin}")
            log(f"  OpenProcess(目标宿主) 被拒: {denied}")
        log("\n计划表:")
        for item in build_schedule(args.scale, only):
            log(f"  {item['name']:<5} {item['ms'] / 1000:>5.1f}s  mode={item['mode']:<13} "
                f"substitute=0x{item['substitute']:04X}")
        return 0

    try:
        return run(args.scale, args.out, not args.no_watch, only)
    except KeyboardInterrupt:
        log("\n用户中断。")
        return 130


if __name__ == "__main__":
    sys.exit(main())
