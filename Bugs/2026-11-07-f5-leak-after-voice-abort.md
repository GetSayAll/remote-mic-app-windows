# 长按中语音会话被中止后，遥控器 F5 重复沿逐条泄漏刷新前台页面

- 发现日期：2026-11-06
- 状态：已修复（单元测试 + CI 通过）；中止场景真机复现验证 deferred
- 影响范围：v1.8.3（`dc1f6d2`）、Windows 11、RC003、「其他工具」+ SendInput 路径（报告层未生效）；理论上不限工具与型号
- 功能点：语音键 F5 抑制器（`crates/sayall-windows/src/key_suppressor.rs`）
- 现象：按住语音键说话到一半，浏览器/编辑器突然被 F5 刷新，未保存内容丢失
- 复现条件：长按期间语音会话被中止（`queue_overflow`、`endpoint_not_selected` 等），用户仍按住语音键
- 正常预期：会话中止只结束本次语音；按住中的遥控器 F5 不得进入前台应用

## 证据

现场诊断日志 2026-11-06 12:08 – 2026-11-07 13:05（UTC），v1.8.3，共 308 次语音会话：

- `suppressor_ask dir=down swallow=0` 共 51 条，按 2 秒间隔聚成 15 簇。
- **12 簇（47 条）**紧跟在语音会话中止之后，首条泄漏距中止 **0.32–0.40 秒**；中止原因均为 `error_code=queue_overflow`（`aborted_enqueue_rejected` 9 簇、`aborted_audio_failure` 3 簇）。
- 簇内泄漏间隔约 320–340 ms，而正常 typematic 重复沿间隔约 30–45 ms，且两次泄漏之间的重复沿被吞下。
- 其余 3 簇是一次**新按下的首条** DOWN 沿即泄漏（会话未武装），与本记录无关，不在本次修复范围。

典型时间线（16:13，单次长按）：

```text
16:13:41.022  ATVV 会话开始，F5 连续 swallow=1
16:13:46.671  audio_session ... error_code=queue_overflow
16:13:46.673  chord_release（abort_voice_session → set_session_active(false)）
16:13:47.019  suppressor_ask dir=down swallow=0 hold=2   ← 首条泄漏，页面刷新
16:13:47.022  voice_f5_raw edge=down ... grace_refreshed=true
16:13:47.343  swallow=0
16:13:47.683  swallow=0
16:13:48.005  swallow=0
```

## 根因

已确认：

1. `abort_voice_session` → `release_voice_hold_hotkey` → `set_session_active(false)`，F5 会话保护只剩 250 ms 宽限（`ARM_GRACE_MS`）。
2. 宽限过期后，按住中的 typematic DOWN 沿进入 60 ms 有界等待（`BOUNDED_WAIT_MS`），无武装信号即放行。250 + 60 ≈ 首条泄漏的 0.32–0.40 秒。
3. 被钩子吞下的沿 Raw Input 看不到，`observe_remote_voice_f5` 只能在**已经漏出一条**之后重新武装 250 ms；武装过期后再漏一条，形成约 320 ms 的周期。

未知：触发中止的 `queue_overflow` 本身的根因另见 `2026-10-21-wasapi-pcm-queue-overflow-diagnostics.md` 与对应 issue；本修复不依赖该结论。

## 修复

`key_suppressor.rs` 增加**按住锁存**：本次按住的 DOWN 沿迄今全部被吞（`HOLD_SWALLOWED_ALL`），且距上一条被吞的 DOWN 沿不超过 `HOLD_LATCH_MAX_GAP_MS`（1500 ms）时，后续 DOWN 沿继续吞，直到 UP 沿结束本次按住。

- 与会话/音频状态解耦：中止仍释放按住说话快捷键，只是不再放出 F5。
- 已泄漏或配对未知的按住不锁存，防粘键配对规则（DOWN 漏进 OS 则 UP 必放行）不变。
- 1500 ms 覆盖 Windows 键盘重复延迟上限（1000 ms）。代价：UP 沿丢失时，最多 1.5 秒内按下的一次实体 F5 会被吞。
- 日志 `suppressor_ask dir=down` 新增 `latch_only=` 字段，标记仅靠锁存吞下的沿。

## 验证

- `cargo fmt --all -- --check`：passed。
- 单元测试 `swallowed_hold_stays_latched_until_gap_expires`：passed（fork CI，Windows runner，`cargo test --workspace`）。
- fork CI verify + installer：passed。
- RC003 真机：安装含本修复的构建后短时使用，无泄漏；但期间未出现会话中止，锁存分支未被真机触发 —— **deferred**。验证方法：复现一次长按中的 `queue_overflow`，确认中止后 `latch_only=1` 且无 `swallow=0`。
- RC001：deferred（无设备）。

## 隐私检查

已确认：仅含时间戳、日志事件名与计数；不含个人路径、蓝牙地址、HID 路径、语音内容或凭据。
