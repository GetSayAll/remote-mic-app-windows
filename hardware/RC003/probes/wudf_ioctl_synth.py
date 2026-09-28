"""报告层**合成按键**实验：能否产出「不带 LLKHF_INJECTED」的真实按键。

要回答的问题
------------
豆包输入法对 `SendInput` 注入的按键一律不响应——逆向证据显示它的 `ImeService`
全局 LL 钩子回调首查 `LLKHF_INJECTED` 位，命中即丢弃（见
`Bugs/2026-09-04-doubao-voice-hold-hotkey.md` 四层闭环）。而同一个键用**物理键盘**
按下时豆包正常唤起。

两者唯一的差别就是那一位。那么问题变成：

    我们在 `WUDFHost.exe` 的报告层改出来的 HID 报告，走到 Windows 输入流时，
    到底带不带 LLKHF_INJECTED？

**如果读回是 injected=0**，说明它在 Windows 眼里与物理按键等价，
豆包那条唯一的门槛就不存在了——这是唯一成本远低于虚拟键盘驱动（WinUHid，
需 OV 证书 + 装机签名）的路径。

双阳性对照设计（本文件的核心方法论）
------------------------------------
本报告不信任任何"看起来成功"的单侧证据，每条断言都要求一个对照：

    阶段 A  按真实主页键          -> 读到 VK_HOME, injected=0   （观测通道工作，且基线正常）
    阶段 S  SendInput 注入右 Alt  -> 读到 VK_RMENU, injected=1  （**观测器能识别合成输入**）
    阶段 B  报告层把主页改成右Alt -> 读到 VK_RMENU, injected=?  （被测对象）

只有 S 成立，B 才有意义：若观测器根本读不到 injected=1，那么 B 读到的
injected=0 也可能只是"这个观测器永远返回 0"，结论不成立。
反过来说，A 与 S 同在一次运行、同一台机器、同一个观测器、同一个键，
却给出相反的标志位——这正是"这条判据有分辨力"的证明。

为什么用主页键 0x004A 做载体
----------------------------
见 `wudf_ioctl_synth.js` 头部：主页键有可见后果（光标跳行首）、HID 报告确定
存在、释放报告确定存在。用一个"本来就零事件"的键做载体，会让"生效"与
"没生效"看起来一模一样——那是这个项目已经踩过的坑。

阶段表
------
===========  ==============  ==========================================  =======================================
阶段         模式            用户操作                                     LL 侧期望
===========  ==============  ==========================================  =======================================
pre          observe         唤醒遥控器后停手                            —
A            observe         按 主页 ×2                                 VK_HOME，injected=0（观测通道对照）
S            observe         不要按键（脚本 SendInput 右 Alt）           VK_RMENU，injected=1（注入标志对照）
B            rewrite→0x00E2  按住 主页 ×2（每次约 0.5~1 秒）            VK_RMENU，injected=0，且不再有 VK_HOME
C            observe         按 主页 ×2                                 VK_HOME 恢复（无残留）
post         observe         停手                                        —
===========  ==============  ==========================================  =======================================

安全边界
--------
- 一次性交互式 UAC 提权；不装内核驱动、不改 Secure Boot / 测试签名 / 驱动签名
  策略、不写注册表、不装计划任务、不需重启。
- 只写报告缓冲区的偏移 3..8，`onLeave` 回写复原，累计写入有上限，
  计划表走完 + 3 秒宽限期后 agent 永久解除武装，进程退出即卸载。
- 唯一的系统级副作用：阶段 B 会把主页键变成**右 Alt**。请把焦点留在桌面或
  空白区域，不要停在文本编辑器里（单独按 Alt 会激活窗口菜单栏）。
- 最坏情况：宿主崩溃 → Windows 自动重启宿主并重新枚举设备（遥控器重连即可）。

用法
----
    python wudf_ioctl_synth.py --out evidence/synth.log      # 真机全程（需提权，约 55 秒）
    python wudf_ioctl_synth.py --phases A,S,B --out ...      # 只跑关键三项（约 32 秒）
    python wudf_ioctl_synth.py --dry-run                     # 只定位宿主 + 提权自检，不注入
    python wudf_ioctl_synth.py --selftest                    # 分析层自检（合成日志），不注入
    python wudf_ioctl_synth.py --analyze <log> [<log>…]      # 离线复核，可多份合并

退出码
------
     0  synth_physical_equivalent   合成按键与物理按键同标志（**核心结论成立**）
     1  定位/参数问题
     2  参数错误
     3  elevation_required          未提权
     4  依赖缺失（frida / LL 钩子 / JS 文件）
     5  attach_failed               注入被拒（多为 EDR/Defender）
     6  hook_export_null            导出地址不可用
     7  日志不可解析
     8  no_positive_control         阶段 A 或 S 未通过 ⇒ 观测不可信，其余一律不采信
     9  substitute_not_mapped       报告层已改写，但 LL 侧没有 VK_RMENU（usage 未被翻译）
    10  synth_flagged_injected      出现了 VK_RMENU 但 injected=1（机制未达预期）
    11  edges_not_paired            释放沿丢失 ⇒ 粘键，**安全性判据**
    12  not_suppressed / not_restored / phase_not_observed
    13  stable_failure              其它分项失败
    14  wrong_device_bound          绑定抓错设备（重跑即可，非机制结论）

脱敏：只打印虚拟键码与标志位，不打印指针绝对值、蓝牙地址、窗口标题或文本内容。
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
from ll_flag_logger import (  # noqa: E402
    LLKeyWatcher,
    count_pairs,
    format_line,
    send_key,
)

JS_PATH = HERE / "wudf_ioctl_synth.js"

# 必须与 JS 侧 GRACE_MS 一致。太早读统计会恒为 False，
# 那条 fail-safe 就等于从未被验证过（既有 probe 踩过这个坑）。
GRACE_SECONDS = 3.0

# 用哪个阶段定设备维度绑定：该阶段仍为 observe（一个字节都不写），只是统计
# "含目标 usage 的键盘报告"来自哪个 FileHandle，供后续改写阶段按设备过滤。
BIND_PHASE = "A"

VK_HOME = 0x24
VK_RMENU = 0xA5          # 右 Alt
VK_F13 = 0x7C

TRIGGER_USAGE = 0x004A   # 主页
SYNTH_USAGE = 0x00E2     # Keyboard RightAlt
HOLD_MS = 200            # S 阶段 SendInput 按住时长

USAGE_NAMES = {
    0x00E0: "LeftCtrl", 0x00E1: "LeftShift", 0x00E2: "RightAlt",
    0x00E3: "LeftGUI", 0x0028: "确定/Enter", 0x004A: "主页",
    0x0068: "F13", 0x00F1: "返回", 0x0080: "音量+", 0x0081: "音量-",
}

# (名称, 秒数, 模式, substitute, 提示语)
PHASES: list[tuple[str, float, str, int, str]] = [
    ("pre", 8, "observe", 0,
     "预热：请先唤醒遥控器（按一下任意键），然后停手。"),
    ("A", 14, "observe", 0,
     "对照 A + 设备绑定：请连按遥控器 主页 ×3（本阶段**只按遥控器**，不要碰其它键盘）。"
     " 期望 LL 侧读到 VK_HOME 且 injected=0，同时探针据此绑定该设备的 FileHandle。"),
    ("S", 8, "observe", 0,
     "对照 S（注入标志）：请不要按键——脚本会自动用 SendInput 注入右 Alt。"
     " 期望读到 VK_RMENU 且 injected=1。"),
    ("B", 16, "rewrite", SYNTH_USAGE,
     "被测阶段 B：请按住 主页 ×2，每次按住约 0.5~1 秒再松开。"
     " 期望读到 VK_RMENU 且 injected=0，同时不再出现 VK_HOME。"),
    ("C", 12, "observe", 0,
     "恢复对照 C：请再按 主页 ×2。期望 VK_HOME 重新出现（无残留）。"),
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
    text = "" if value is None else str(value)
    return re.sub(r"\s+", "_", text.strip()) or "-"


def build_schedule(scale: float, only: list[str] | None = None) -> list[dict]:
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


def pick_rc003_host() -> tuple[int | None, list[str], int]:
    """选出承载 RC003 的 WUDFHost，返回 (pid, 同宿主的其它设备数说明, 宿主设备总数)。

    **为什么不再要求"独占宿主"**
    本机实测：RC003 所在的 WUDFHost 里同时还躺着两个 BLE HID 设备（另一副蓝牙
    键鼠）。要求独占会让探针在本机永远跑不起来。正确的做法不是放弃，而是接受
    共享宿主 + 在 JS 侧按 FileHandle 做设备维度绑定（见 `wudf_ioctl_synth.js`
    的「设备维度绑定」节）。这里如实报告共享情况，让每一次运行都知道自己
    是在什么环境里得出的结论。
    """
    entries = scan_hosts()
    rc003 = [e for e in entries if e["is_rc003"]]
    if not rc003:
        return None, [], 0
    pids = sorted({e["pid"] for e in rc003})
    if len(pids) > 1:
        log(f"  ⚠ RC003 分布在多个宿主 {pids}，本探针一次只挂一个，将取第一个。")
    pid = pids[0]
    members = [e for e in entries if e["pid"] == pid]
    others = [
        f"{e['enumerator']}/{e['device'].split('_')[-1][:12]}"
        for e in members if not e["is_rc003"]
    ]
    return pid, others, len(members)


# --------------------------------------------------------------------------
# 记录解析（在线与离线共用）
# --------------------------------------------------------------------------

TAP_RE = re.compile(r"^\[TAP\] (?P<body>.*)$")
LL_RE = re.compile(r"^\[LL\] (?P<body>.*)$")
TIMELINE_RE = re.compile(r"^\s{2}([A-Za-z][A-Za-z0-9_]*)\s+\d{10,}\s+\.\.\s+\d{10,}\s*$", re.M)


def parse_kv(body: str) -> dict:
    """解析 `|` 之前的 key=value；`|` 之后是人性化说明，不参与解析。"""
    fields: dict[str, str] = {}
    head = body.split("|", 1)[0]
    for token in head.split():
        if "=" in token:
            key, value = token.split("=", 1)
            fields[key] = value
    return fields


def load_records(text: str) -> tuple[list[dict], list[dict]]:
    taps: list[dict] = []
    lls: list[dict] = []
    for line in text.splitlines():
        line = line.strip()
        m = LL_RE.match(line)
        if m:
            f = parse_kv(m.group("body"))
            f["vk"] = int(f["vk"], 16)
            f["up"] = f["up"] == "1"
            f["injected"] = f["injected"] == "1"
            f["lower_il"] = f.get("lower_il") == "1"
            f["t"] = float(f["t"])
            lls.append(f)
            continue
        m = TAP_RE.match(line)
        if m:
            f = parse_kv(m.group("body"))
            f["mutated"] = f.get("mutated") == "True"
            # 日志行用 orig/new（紧凑），在线 payload 用 orig_hex/new_hex——
            # 统一成后者，离线复核与在线判定才走同一条代码路径。
            if "orig" in f:
                f["orig_hex"] = f.pop("orig")
            if "new" in f:
                f["new_hex"] = f.pop("new")
            for key in ("t", "idx", "in_len", "out_len", "ret"):
                if key in f:
                    try:
                        f[key] = int(f[key])
                    except ValueError:
                        pass
            taps.append(f)
    return taps, lls


def _slot_usages(hex_str: str) -> list[int]:
    """取报告三个 usage 槽的值（小端 16 位）。解析失败返回空表。"""
    if not hex_str or len(hex_str) < 18:
        return []
    try:
        raw = [int(hex_str[i * 2:i * 2 + 2], 16) for i in range(9)]
    except ValueError:
        return []
    return [raw[o] | (raw[o + 1] << 8) for o in (3, 5, 7)]


def _bound_handles(taps: list[dict], phase: str = BIND_PHASE) -> dict[str, int]:
    """绑定阶段里，「含目标 usage 的键盘报告」分别来自哪些 handle_id。"""
    hits: dict[str, int] = {}
    for r in taps:
        if r.get("dir") != "enter" or r.get("phase") != phase:
            continue
        orig = r.get("orig_hex") or ""
        if len(orig) < 18 or orig[:2] != "01":
            continue
        if TRIGGER_USAGE in _slot_usages(orig):
            hid = r.get("handle_id") or "-"
            hits[hid] = hits.get(hid, 0) + 1
    return hits


def _guard_blocked(taps: list[dict], phase: str, reason: str) -> int:
    """某阶段因指定原因被闸门拦下的次数（用于区分"没按到"与"绑错设备"）。"""
    return sum(
        1 for r in taps
        if r.get("dir") == "enter" and r.get("phase") == phase
        and r.get("write_err") == reason
    )


def detect_phases(texts: list[str]) -> list[str] | None:
    order = [p[0] for p in PHASES]
    found: set[str] = set()
    for text in texts:
        found.update(TIMELINE_RE.findall(text))
    if not found:
        return None
    return [name for name in order if name in found]


# --------------------------------------------------------------------------
# 判定
# --------------------------------------------------------------------------

ASSERTION_PHASES: dict[str, str] = {
    "A_control_present": "A",
    "A_handle_bound": BIND_PHASE,
    "S_injected_flag_seen": "S",
    "S_synth_key_seen": "S",
    "B_synth_key_seen": "B",
    "B_not_injected": "B",
    "B_home_suppressed": "B",
    "B_edges_paired": "B",
    "C_restored": "C",
}


def _phase_ll(lls: list[dict], phase: str, vk: int | None = None) -> list[dict]:
    sel = [e for e in lls if e.get("phase") == phase]
    if vk is not None:
        sel = [e for e in sel if e["vk"] == vk]
    return sel


def _mutations(taps: list[dict], phase: str) -> int:
    return sum(
        1 for r in taps
        if r.get("dir") == "enter" and r.get("phase") == phase and r.get("mutated")
    )


def _enters(taps: list[dict], phase: str) -> int:
    return sum(1 for r in taps if r.get("dir") == "enter" and r.get("phase") == phase)


def verdict(taps: list[dict], lls: list[dict],
            active: list[str] | None = None) -> tuple[dict[str, bool], int]:
    phases = [p[0] for p in PHASES]
    if active is None:
        active = list(phases)
    active_set = set(active)

    log("\n=== 判定（仅由宿主侧写入记录 + LL 侧按键flags推导）===")
    if active_set != set(phases):
        log(f"（本次只执行了阶段 {'/'.join(active)}，其余不参与判定）")

    log("\n[1] LL 侧按键观测（全系统，按阶段归类）")
    for name in active:
        seg = _phase_ll(lls, name)
        summary: dict[str, int] = {}
        for e in seg:
            key = f"0x{e['vk']:02X}{'↑' if e['up'] else '↓'}"
            summary[key] = summary.get(key, 0) + 1
        flag = ""
        rmenu = [e for e in seg if e["vk"] == VK_RMENU]
        if rmenu:
            flag = f"   VK_RMENU injected={[int(e['injected']) for e in rmenu]}"
        log(f"  {name:<5} 事件 {len(seg):>3}  "
            + ("  ".join(f"{k}×{v}" for k, v in summary.items()) or "（无）") + flag)

    log("\n[2] 宿主侧报告层改写记录")
    for name in active:
        log(f"  {name:<5} IOCTL 命中 {_enters(taps, name):>3} 次  实际改写 {_mutations(taps, name):>3} 次")
        pairs = {f"{r.get('orig_hex')}→{r.get('new_hex')}"
                 for r in taps
                 if r.get("dir") == "enter" and r.get("phase") == name and r.get("mutated")}
        for pair in sorted(pairs):
            log(f"        {pair}")
    write_fail = sum(
        1 for r in taps
        if r.get("dir") == "enter" and r.get("write_err") not in (None, "", "-")
        and not r.get("mutated")
        and r.get("mode") in ("rewrite", "erase-target", "erase-all")
    )
    restored = sum(1 for r in taps if r.get("dir") == "leave" and r.get("detail") == "restored")
    log(f"\n  写入失败/被闸门拦下: {write_fail}")
    log(f"  onLeave 已回写复原: {restored}")

    log("\n[3] 设备维度绑定（同一宿主里有多个 HID 设备时的归因依据）")
    bound_hits_tbl = _bound_handles(taps)
    if bound_hits_tbl:
        for hid, count in sorted(bound_hits_tbl.items(), key=lambda kv: -kv[1]):
            log(f"  {BIND_PHASE} 阶段来自 {hid} 的目标 usage 报告: {count} 次")
        if len(bound_hits_tbl) > 1:
            log("  ⚠ 检测到多个来源——绑定阶段可能碰了同宿主的其它 HID 设备，"
                "请只按遥控器重跑。")
    else:
        log(f"  {BIND_PHASE} 阶段没有任何含目标 usage 的键盘报告（未绑定）")
    other_blocked = _guard_blocked(taps, "B", "other_device")
    nobind_blocked = _guard_blocked(taps, "B", "no_bound_handle")
    if other_blocked or nobind_blocked:
        log(f"  B 阶段被设备闸门拦下: other_device={other_blocked} "
            f"no_bound_handle={nobind_blocked}")

    log("\n[4] 边沿配对（粘键判据，按边沿去重后再数）")
    pair_info: dict[str, dict] = {}
    for name in ("S", "B"):
        if name not in active_set:
            continue
        info = count_pairs(_phase_ll(lls, name, VK_RMENU), VK_RMENU)
        pair_info[name] = info
        log(f"  {name:<5} VK_RMENU 按下={info['downs']} 释放={info['ups']} "
            f"配对={info['pairs']} autorepeat={info['repeats']} "
            f"孤立释放={info['stray_up']} 结束时仍按住={info['stuck']}")

    a_home = _phase_ll(lls, "A", VK_HOME)
    s_rmenu = _phase_ll(lls, "S", VK_RMENU)
    b_rmenu = _phase_ll(lls, "B", VK_RMENU)
    b_home = _phase_ll(lls, "B", VK_HOME)
    c_home = _phase_ll(lls, "C", VK_HOME)
    b_info = pair_info.get("B", {"stuck": False, "pairs": 0, "stray_up": 0})

    dominant_bound = max(bound_hits_tbl.values()) if bound_hits_tbl else 0
    results = {
        "A_control_present": len(a_home) >= 1 and all(not e["injected"] for e in a_home),
        "A_handle_bound": dominant_bound >= 1,
        "S_synth_key_seen": len(s_rmenu) >= 1,
        "S_injected_flag_seen": any(e["injected"] for e in s_rmenu),
        "B_synth_key_seen": len(b_rmenu) >= 1,
        "B_not_injected": any(not e["injected"] for e in b_rmenu),
        "B_home_suppressed": len(b_home) == 0,
        "B_edges_paired": bool(b_info["pairs"] >= 1 and not b_info["stuck"]
                               and b_info["stray_up"] == 0),
        "C_restored": len(c_home) >= 1,
    }

    labels = {
        "A_control_present": "A 对照：真实主页键 -> VK_HOME 且 injected=0",
        "A_handle_bound": "A 对照：已按 FileHandle 绑定到单一来源（设备维度成立）",
        "S_synth_key_seen": "S 对照：SendInput 的右 Alt 被观测到",
        "S_injected_flag_seen": "S 对照：SendInput 的按键带 injected=1（观测器有分辨力）",
        "B_synth_key_seen": "B 被测：报告层产出 VK_RMENU",
        "B_not_injected": "B 被测：该 VK_RMENU 的 injected=0（**核心**）",
        "B_home_suppressed": "B 被测：原主页键已被替换（无 VK_HOME）",
        "B_edges_paired": "B 被测：按下/释放严格配对（无粘键）",
        "C_restored": "C 对照：解除武装后主页键恢复",
    }
    log("\n[5] 分项结论")
    live = [k for k in results if ASSERTION_PHASES[k] in active_set]
    for key in labels:
        if key not in live:
            log(f"  SKIP  {labels[key]}")
            continue
        log(f"  {'PASS' if results[key] else 'FAIL'}  {labels[key]}")

    if not live:
        log("\n判定: no_assertion_executed")
        return results, 7

    # 阳性对照缺失 => 其余一律不采信
    missing_control = []
    if "A" in active_set and not results["A_control_present"]:
        missing_control.append("A(真实按键未被观测到)")
    if "S" in active_set and not (results["S_synth_key_seen"]
                                  and results["S_injected_flag_seen"]):
        missing_control.append("S(SendInput 未读到 injected=1)")
    if missing_control:
        log(f"\n判定: no_positive_control（{', '.join(missing_control)}）")
        log("说明: 观测通道本身没有证明它能分辨注入标志，B 阶段无论读到什么都不可信。")
        log("      请先单独跑：python ll_flag_logger.py --manual 8")
        return results, 8

    if "B" in active_set:
        if not results["B_synth_key_seen"]:
            b_entered = [r for r in taps
                         if r.get("dir") == "enter" and r.get("phase") == "B"]
            b_targets = [r for r in b_entered
                         if TRIGGER_USAGE in _slot_usages(r.get("orig_hex") or "")]
            if _enters(taps, "B") == 0:
                log("\n判定: phase_not_observed（B 窗口内没有任何报告通过）")
                log("说明: 采集缺失而非机制失败——确认遥控器已唤醒并按的是主页键。")
                return results, 12
            if b_targets and all(r.get("write_err") == "other_device" for r in b_targets):
                log("\n判定: wrong_device_bound（目标 usage 的报告全被判为其它设备）")
                log(f"说明: B 窗口里有 {len(b_targets)} 份含 {TRIGGER_USAGE:#06x} 的报告，")
                log("      但没有一份来自绑定阶段选定的 FileHandle。最可能是绑定阶段")
                log("      抓到的其实是同宿主的另一台 HID 设备。请只在阶段 A 按遥控器")
                log("      重新采集——这不是机制结论，重跑即可。")
                return results, 14
            if not b_targets:
                log("\n判定: phase_not_observed（B 窗口内没有含目标 usage 的报告）")
                log(f"说明: 有 {len(b_entered)} 次 IOCTL 命中，但没有一份含 {TRIGGER_USAGE:#06x}。")
                log("      通常是阶段 B 没按到主页键——采集缺失，重跑即可。")
                return results, 12
            log("\n判定: substitute_not_mapped（报告已改写，LL 侧没有 VK_RMENU）")
            log(f"说明: 阶段 B 命中 {_enters(taps, 'B')} 次、改写 {_mutations(taps, 'B')} 次，")
            log(f"      但 Windows 侧没有 {USAGE_NAMES.get(SYNTH_USAGE)}。"
                "即 usage 0x00E2 在本机 kbdhid 路径上未映射到 VK_RMENU。")
            log("      这是**技术结论**而非故障：替换 usage 必须逐键实测。")
            return results, 9
        if not results["B_not_injected"]:
            log("\n判定: synth_flagged_injected（产出的按键仍带 injected=1）")
            log("说明: 报告层合成未达到与物理按键等价的效果，本路线不成立。")
            return results, 10
        if not results["B_edges_paired"]:
            log("\n判定: edges_not_paired（存在粘键或孤立释放）")
            log("说明: 这是安全性判据——粘住的 Alt 会污染用户后续每一次键盘输入。")
            return results, 11
        if not results["B_home_suppressed"]:
            log("\n判定: not_suppressed（原主页键仍在穿透）")
            log("说明: 替换未发生在翻译之前，或该键有第二条上报路径。")
            return results, 12

    if "C" in active_set and not results["C_restored"]:
        log("\n判定: not_restored（解除武装后主页键未恢复）")
        return results, 12

    failed = [k for k in live if not results[k]]
    if failed:
        log(f"\n判定: stable_failure（未通过：{', '.join(failed)}）")
        return results, 13

    log(f"\n判定: synth_physical_equivalent")
    log(f"说明: 同一次运行里，A（真实按键）读到 injected=0、S（SendInput）读到 injected=1，")
    log(f"      证明观测器具备双向分辨力；B（报告层把主页 {TRIGGER_USAGE:#06x} 改成 "
        f"{SYNTH_USAGE:#06x}）"
        f"读到的 VK_RMENU 是 injected=0，")
    log(f"      且按下/释放严格配对——即它在 Windows 输入流中与物理按键不可区分。")
    log(f"      这意味着豆包那条唯一门槛（LLKHF_INJECTED 检查）不成立。")
    if active_set != set(phases):
        log(f"注意: 本次只覆盖 {'/'.join(active)}，其余阶段未验。")
    return results, 0


# --------------------------------------------------------------------------
# 分析层自检
# --------------------------------------------------------------------------

def _synth_log(*, with_home: bool = True, s_flagged: bool = True,
               b_keys: bool = True, b_injected: bool = False,
               b_stuck: bool = False, c_home: bool = True,
               wrong_device: bool = False,
               include: set[str] | None = None) -> str:
    """构造合成日志（自检专用），格式与真机日志完全一致。

    参数逐个对应真实运行里最可能出错的**单一维度**，这样自检失败时
    能精确定位到是哪条断言在守护什么，而不是笼统的"判定不对"。
    """
    out = ["=== 合成日志（自检用）==="]
    base = 1790134399712.0

    def on(name: str) -> bool:
        return include is None or name in include

    def ll(t, phase, vk, up=False, injected=False):
        if not on(phase):
            return
        out.append(
            f"[LL] t={base + t} phase={phase} vk=0x{vk:02X} up={int(up)} "
            f"flags=0x{0x0010 if injected else 0x0000:04X} "
            f"injected={int(injected)} lower_il=0 altdown=0 | x"
        )

    def tap(t, phase, mode, orig, new=None, mutated=False, handle="h0",
            write_err="-"):
        if not on(phase):
            return
        out.append(
            f"[TAP] t={base + t} phase={phase} idx=0 mode={mode} dir=enter "
            f"in_len=8 out_len=9 orig={orig} new={new or orig} "
            f"mutated={mutated} write_err={write_err} handle_id={handle} | x"
        )

    home_down = "0100004a0000000000"
    home_up = "010000000000000000"

    tap(1000, "pre", "observe", home_up, handle="h0")
    # wrong_device 场景：绑定阶段抓到的其实是同宿主的另一台设备（h1），
    # B 阶段遥控器（h0）的主页报告全部被设备闸门拦下。
    a_handle = "h1" if wrong_device else "h0"
    b_handle = "h0"
    b_err = "other_device" if wrong_device else "-"
    tap(9000, "A", "observe", home_down, handle=a_handle)
    tap(11000, "A", "observe", home_down, handle=a_handle)
    if with_home:
        ll(9000, "A", VK_HOME)
        ll(9150, "A", VK_HOME, up=True)
        ll(11000, "A", VK_HOME)
        ll(11150, "A", VK_HOME, up=True)

    ll(16000, "S", VK_RMENU, injected=s_flagged)
    ll(16200, "S", VK_RMENU, up=True, injected=s_flagged)

    tap(20000, "B", "rewrite", home_down, "010000e20000000000", True,
        handle=b_handle, write_err=b_err)
    if b_keys and not wrong_device:
        ll(20000, "B", VK_RMENU, injected=b_injected)
        ll(20450, "B", VK_RMENU, injected=b_injected)   # autorepeat
        if not b_stuck:
            ll(20500, "B", VK_RMENU, up=True, injected=b_injected)
    tap(26000, "C", "observe", home_down, handle="h0")
    if c_home:
        ll(26000, "C", VK_HOME)
        ll(26150, "C", VK_HOME, up=True)

    out.append("--- 阶段时间轴（本地时钟）---")
    cursor = int(base)
    for name, seconds, _m, _s, _h in PHASES:
        if not on(name):
            continue
        end = cursor + int(seconds * 1000)
        out.append(f"  {name:<5} {cursor} .. {end}")
        cursor = end
    return "\n".join(out) + "\n"


def selftest() -> int:
    cases = [
        ("全程 · 核心结论成立", {}, 0),
        ("全程 · A 未观测到真实按键", {"with_home": False}, 8),
        ("全程 · S 读不到 injected=1", {"s_flagged": False}, 8),
        ("补采 A,S,B · 三项均通过", {"include": {"A", "S", "B"}}, 0),
        ("补采 A,S,B · usage 未映射", {"b_keys": False,
                                        "include": {"A", "S", "B"}}, 9),
        ("全程 · usage 未映射", {"b_keys": False}, 9),
        ("全程 · 产出键带 injected=1", {"b_injected": True}, 10),
        ("全程 · 粘键（缺释放沿）", {"b_stuck": True}, 11),
        ("全程 · 解除武装后未恢复", {"c_home": False}, 12),
        ("全程 · 绑定抓错设备", {"wrong_device": True}, 14),
    ]
    all_ok = True
    for label, kwargs, expected in cases:
        text = _synth_log(**kwargs)
        taps, lls = load_records(text)
        active = detect_phases([text])
        log(f"\n########## 自检用例：{label}（期望 {expected}）##########")
        log(f"（解析到 宿主侧 {len(taps)} 条 / LL 侧 {len(lls)} 条 / "
            f"阶段 {'/'.join(active) if active else '全部'}）")
        _, code = verdict(taps, lls, active)
        ok = code == expected
        all_ok = all_ok and ok
        log(f"实际退出码 {code} —— {'符合' if ok else '不符合'}期望")
    log(f"\n=== 分析层自检{'通过' if all_ok else '失败'} ===")
    return 0 if all_ok else 1


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
        log("判定: no_phase_selected（--phases 过滤后没有任何阶段）")
        return 1
    total_ms = sum(p["ms"] for p in schedule)

    log("=== 报告层合成按键实验：能不能产出不带 LLKHF_INJECTED 的真实按键 ===")
    log(f"时间: {datetime.now().isoformat(timespec='seconds')}")
    log(f"契约: 载体 usage={TRIGGER_USAGE:#06x}({USAGE_NAMES.get(TRIGGER_USAGE)})"
        f" -> 合成 usage={SYNTH_USAGE:#06x}({USAGE_NAMES.get(SYNTH_USAGE)})；"
        "只写报告偏移 3..8 并回写复原")
    log("      不写注册表 / 驱动策略 / 计划任务，不需重启；需一次交互式 UAC 提权。")
    log("副作用: 阶段 B 会把主页键变成右 Alt——请把焦点留在桌面，"
        "不要停在文本编辑器里，期间不要触碰物理键盘。")

    log("\n--- 步骤 1/5：定位独占 RC003 的 WUDFHost ---")
    host_pid, other_members, member_count = pick_rc003_host()
    if host_pid is None:
        log("判定: rc003_wudf_host_absent")
        log("说明: 请先连接并唤醒 RC003 遥控器后重跑。")
        return 1
    log(f"  命中 RC003 宿主 HostPid={host_pid} (0x{host_pid:x})，"
        f"该宿主共承载 {member_count} 个设备")
    if other_members:
        log(f"  ⚠ 宿主非独占，同宿主还有 {len(other_members)} 个设备："
            f" {', '.join(other_members)}")
        log("    因此写入必须走 FileHandle 绑定（见 JS 的「设备维度绑定」），"
            "否则会误伤同宿主的其它 HID 设备。")
    else:
        log("  宿主独占 RC003。")

    log("\n--- 步骤 2/5：提权与可注入性自检 ---")
    admin = bool(ctypes.windll.shell32.IsUserAnAdmin())
    denied = probe_open_denied(host_pid)
    log(f"  当前进程管理员令牌: {admin}")
    log(f"  OpenProcess(目标宿主) 被拒: {denied}")
    if not (admin and not denied):
        log("判定: elevation_required")
        log("说明: 宿主位于 session 0，普通权限 OpenProcess 返回 err=5。")
        log("      请用「以管理员身份运行」的终端重跑本脚本。")
        return 3

    watcher: LLKeyWatcher | None = None
    lls: list[dict] = []
    if do_watch:
        log("\n--- 步骤 3/5：启动 LL 钩子观测（读回 LLKHF_INJECTED 位）---")

        def on_event(record: dict) -> None:  # noqa: ANN001
            record["phase"] = phase_of(record["t"])
            log(format_line(record, record["phase"]))

        watcher = LLKeyWatcher(on_event=on_event)
        if not watcher.start():
            log(f"判定: ll_hook_failed ({watcher.error})")
            return 4
        log("  已安装 WH_KEYBOARD_LL 钩子（免提权）")
    else:
        log("\n--- 步骤 3/5：跳过 LL 观测（--no-watch；本实验无此选项的判据不可用）---")

    try:
        import frida
    except ImportError:
        log("判定: frida_missing（pip install frida）")
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
    required = ("__SCHEDULE_JSON__", "__TARGET_USAGES_JSON__",
                "__USAGE_NAMES_JSON__", "__BIND_PHASE__")
    missing = [token for token in required if token not in js_source]
    if missing:
        log(f"判定: js_missing_placeholder（缺少占位符 {missing}，拒绝注入）")
        if watcher:
            watcher.stop()
        return 4
    js_source = (js_source
                 .replace("__SCHEDULE_JSON__", json.dumps(schedule))
                 .replace("__TARGET_USAGES_JSON__",
                          json.dumps({TRIGGER_USAGE: USAGE_NAMES.get(TRIGGER_USAGE)}))
                 .replace("__USAGE_NAMES_JSON__", json.dumps(USAGE_NAMES))
                 .replace("__BIND_PHASE__", json.dumps(BIND_PHASE)))

    log("\n--- 步骤 4/5：attach 并安装报告层钩子 ---")
    try:
        session = frida.attach(host_pid)
    except Exception as exc:  # noqa: BLE001
        log(f"判定: attach_failed -> {type(exc).__name__}: {exc}")
        log("说明: 常见原因——未提权；或 EDR/Defender 拦截向系统进程注入。")
        if watcher:
            watcher.stop()
        return 5

    taps: list[dict] = []
    ready: dict = {}
    heartbeats: list[dict] = []
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
            log(f"  目标 usage: {payload.get('target_usages')}")
            log(f"  计划表总时长 {payload.get('total_ms')} ms，写入上限 {payload.get('max_mutations')}")
        elif kind == "ioctl":
            payload["py_phase"] = phase_of(payload.get("t") or 0)
            taps.append(payload)
            if payload.get("dir") == "enter":
                tail = ""
                if payload.get("mutated"):
                    tail = f"  改写 {payload.get('orig_hex')}→{payload.get('new_hex')}"
                elif payload.get("write_err"):
                    tail = f"  [闸门:{sanitize(payload.get('write_err'))}]"
                log(f"[TAP] t={payload.get('t')} phase={payload.get('phase')} "
                    f"mode={payload.get('mode')} dir=enter "
                    f"orig={payload.get('orig_hex')} new={payload.get('new_hex')} "
                    f"mutated={payload.get('mutated')} |{tail}")
            else:
                log(f"[TAP] t={payload.get('t')} phase={payload.get('phase')} dir=leave "
                    f"ret={payload.get('ret')} out={payload.get('out')} "
                    f"detail={sanitize(payload.get('detail'))} |")
        elif kind == "hb":
            heartbeats.append(payload)
            log(f"  [心跳] phase={payload.get('phase')} mode={payload.get('mode')} "
                f"命中={payload.get('target_calls')} 改写={payload.get('mutations')} "
                f"成功={payload.get('write_ok')} 失败={payload.get('write_fail')} "
                f"复原={payload.get('restored')}"
                + ("【已解除武装】" if payload.get("disarmed") else ""))

    script = session.create_script(js_source)
    script.on("message", on_message)
    script.load()

    deadline = time.time() + 6
    while time.time() < deadline and not ready:
        time.sleep(0.1)
    if not ready.get("hook_ok"):
        log("判定: hook_export_null")
        _detach(session)
        if watcher:
            watcher.stop()
        return 6

    t0 = float(ready.get("t"))
    log(f"  agent 起始时钟 t0={t0:.0f}")

    log("\n--- 步骤 5/5：分阶段执行 ---")
    cumulative = 0.0
    for item in schedule:
        start = t0 + cumulative
        end = start + item["ms"]
        cumulative += item["ms"]
        boundaries.append((start, end, item["name"]))
        hint = next(h for h in PHASES if h[0] == item["name"])[4]
        while time.time() * 1000.0 < start:
            time.sleep(0.05)
        mode_note = f"{item['mode']}→{USAGE_NAMES.get(item['substitute'], '-')}" \
            if item["mode"] == "rewrite" else item["mode"]
        log(f"\n>>> 阶段 {item['name']}（{item['ms'] / 1000:.0f}s，{mode_note}）：{hint}")

        warned = False
        warn_at = start + item["ms"] * 0.45
        while time.time() * 1000.0 < end:
            if item["name"] == "S" and not warned \
                    and time.time() * 1000.0 >= start + item["ms"] * 0.3:
                warned = True
                log("  [自动对照] SendInput 注入右 Alt（必然带 LLKHF_INJECTED）……")
                ok = send_key(VK_RMENU, hold_ms=HOLD_MS)
                log(f"  [自动对照] SendInput 返回 {ok}（0 表示被 UIPI 拦下，判据不可用）")
            time.sleep(0.05)
        enters = sum(1 for r in taps
                     if r.get("dir") == "enter" and r.get("phase") == item["name"])
        if enters == 0 and item["name"] != "S":
            log(f"  [注意] 阶段 {item['name']} 命中 0 次——该阶段数据不可用（采集缺失）。")

    log("\n--- 等待自动解除武装 ---")
    time.sleep(GRACE_SECONDS + 1.0)
    if heartbeats:
        last = heartbeats[-1]
        log(f"  最终统计: 命中={last.get('target_calls')} 改写={last.get('mutations')} "
            f"成功={last.get('write_ok')} 失败={last.get('write_fail')} "
            f"复原={last.get('restored')} 解除武装={last.get('disarmed')}")
        if not last.get("disarmed"):
            log("  [注意] 宽限期已过但 agent 仍未自行解除武装——fail-safe 未生效。")
    _detach(session)

    if watcher:
        watcher.stop()
        lls = list(watcher.events)
        log(f"  LL 钩子已卸载；共捕获 {len(lls)} 条按键事件")

    log("\n--- 阶段时间轴（本地时钟）---")
    for b_start, b_end, b_name in boundaries:
        log(f"  {b_name:<5} {b_start:.0f} .. {b_end:.0f}")

    return verdict(taps, lls, [p["name"] for p in schedule])[1]


def _detach(session) -> None:  # noqa: ANN001
    try:
        session.detach()
    except Exception:  # noqa: BLE001
        pass


def analyze(paths: list[Path]) -> int:
    texts: list[str] = []
    taps: list[dict] = []
    lls: list[dict] = []
    for path in paths:
        text = path.read_text(encoding="utf-8", errors="replace")
        texts.append(text)
        part_taps, part_lls = load_records(text)
        log(f"=== 离线复核: {path} ===")
        log(f"  宿主侧 {len(part_taps)} 条 / LL 侧 {len(part_lls)} 条")
        taps.extend(part_taps)
        lls.extend(part_lls)
    if not taps and not lls:
        log("判定: log_unparseable")
        return 7
    active = detect_phases(texts)
    if active:
        log(f"从时间轴解析到执行过的阶段: {'/'.join(active)}")
    return verdict(taps, lls, active)[1]


def main() -> int:
    parser = argparse.ArgumentParser(
        description="报告层合成按键实验：产物是否等价于物理按键（需提权）")
    parser.add_argument("--out", type=Path, help="日志落盘路径")
    parser.add_argument("--scale", type=float, default=1.0, help="阶段时长缩放")
    parser.add_argument("--no-watch", action="store_true",
                        help="不启动 LL 观测（仅演示写入，判据不可用）")
    parser.add_argument("--dry-run", action="store_true", help="只做定位与自检，不注入")
    parser.add_argument("--selftest", action="store_true",
                        help="分析层自检：合成日志走同一条 parse+verdict 路径")
    parser.add_argument("--analyze", type=Path, nargs="+", help="离线复核既有日志")
    parser.add_argument("--phases", help="只跑指定阶段（逗号分隔，如 A,S,B）")
    args = parser.parse_args()

    only: list[str] | None = None
    if args.phases:
        only = [p.strip() for p in args.phases.split(",") if p.strip()]
        valid = {p[0] for p in PHASES}
        unknown = [p for p in only if p not in valid]
        if unknown:
            print(f"未知阶段: {unknown}（可选 {'/'.join(p[0] for p in PHASES)}）",
                  file=sys.stderr)
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
        log("=== dry-run（不注入）===")
        log(f"JS 存在: {JS_PATH.exists()}")
        if JS_PATH.exists():
            source = JS_PATH.read_text(encoding="utf-8")
            for token in ("__SCHEDULE_JSON__", "__TARGET_USAGES_JSON__",
                          "__USAGE_NAMES_JSON__", "__BIND_PHASE__"):
                log(f"  占位符 {token}: {token in source}")
        host_pid, other_members, member_count = pick_rc003_host()
        log(f"RC003 宿主 PID: {host_pid}（该宿主共 {member_count} 个设备）")
        if other_members:
            log(f"  同宿主其它设备: {', '.join(other_members)}")
        if host_pid is not None:
            log(f"  管理员令牌: {bool(ctypes.windll.shell32.IsUserAnAdmin())}")
            log(f"  OpenProcess 被拒: {probe_open_denied(host_pid)}")
        log("\n计划表:")
        for item in build_schedule(args.scale, only):
            log(f"  {item['name']:<5} {item['ms'] / 1000:>5.1f}s  mode={item['mode']:<10} "
                f"substitute=0x{item['substitute']:04X}")
        return 0

    try:
        return run(args.scale, args.out, not args.no_watch, only)
    except KeyboardInterrupt:
        log("\n用户中断。")
        return 130


if __name__ == "__main__":
    sys.exit(main())
