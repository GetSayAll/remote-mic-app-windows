"""生成"去掉守卫"的 agent 对照副本，用来证明自检用例 D 与 E 有分辨力。

为什么需要它
------------
用例 D 断言"助手消失后重新上线，agent 只连一次"，用例 E 断言"targets 只许追加
哨兵键、被拒的命令不得改动清空范围"。如果只跑当前版本、看到 PASS 就收工，那 PASS
可能来自**判据本身没有分辨力**。所以必须有一个"缺陷版"，并且它在同一些用例上必须 FAIL。

本脚本做的四处回退：
  1. `ensureConnected` 去掉 `if (connecting) return;`  → connect 可以叠加
  2. `connectOnce` 去掉竞争检查                        → 多个 connect 都写全局 sock
  3. `pump` 的读失败分支退回 `connected = false`       → 断线不计数、不复位状态
  4. `targets` 去掉两条护栏                            → 畸形命令可以改坏清空范围
  其中 1-3 是 2026-09-23 真机现象的成因（3 条连接、2 条被 RST），对应用例 D；
  4 对应用例 E，也是"实现了但没护栏"这种最常见妥协的形状。

用法
----
    python control_make_noguard.py            # 生成对照副本
    python agent_selftest.py ../target/tmp/rc003_agent_noguard.js   # 期望 D 与 E FAIL

实测（2026-09-23）：
    当前版本 → 5/5 PASS（被连 1 次，断线可见=1，targets 护栏生效）
    对照版本 → 用例 D 与 E FAIL，退出码 1
"""

from __future__ import annotations

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
SRC = HERE / "rc003_agent.js"
OUT = HERE.parent / "target" / "tmp" / "rc003_agent_noguard.js"

SUBS: list[tuple[str, str]] = [
    (
        "  if (connecting) return;                       /* 已有一次在飞，别叠加（见 connectOnce） */",
        "  /* [对照实验] 并发守卫已移除 */",
    ),
    (
        """      if (connected || myAttempt !== attemptSeq) {
        stat.connect_raced++;
        try { if (typeof conn.close === 'function') conn.close(); } catch (e) { /* ignore */ }
        return false;
      }
""",
        "      /* [对照实验] 竞争检查已移除 */\n",
    ),
    (
        """    stat.read_errors++;
    dropConnection('read_error', false);""",
        "    connected = false;   /* [对照实验] 退回旧行为 */",
    ),
    (
        """    if (!sameUsageSet(rep, TARGET_USAGES)) {
      stat.cmd_rejected++; stat.targets_rejected++; logLine('targets:rejected_report_not_targets'); return;
    }
    var covers = true;
    for (var ti = 0; ti < rep.length; ti++) if (clr.indexOf(rep[ti]) < 0) covers = false;
    if (!covers) {
      stat.cmd_rejected++; stat.targets_rejected++; logLine('targets:rejected_clear_lacks_report'); return;
    }
""",
        "    /* [对照实验] targets 的两条护栏已移除 */\n",
    ),
]


def main() -> int:
    if not SRC.is_file():
        print(f"找不到 {SRC}")
        return 2
    text = SRC.read_text(encoding="utf-8")
    for old, new in SUBS:
        hits = text.count(old)
        if hits != 1:
            # 关键：改不到就必须失败。静默生成一个"看起来像对照"其实没改的文件，
            # 会让阳性对照变成假证据。
            print(f"替换命中 {hits} 次（应为 1），对照副本不可信：{old[:60]!r}")
            return 3
        text = text.replace(old, new)

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(text, encoding="utf-8")
    before = len(SRC.read_text(encoding="utf-8").splitlines())
    after = len(text.splitlines())
    print(f"对照副本: {OUT}")
    print(f"行数 {before} -> {after}；四处回退均已命中")
    print(f"下一步: python agent_selftest.py {OUT}   （期望用例 D 与 E FAIL，退出码 1）")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
