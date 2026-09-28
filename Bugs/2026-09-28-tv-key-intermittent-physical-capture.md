# 全按键支持下 TV 键偶发拦截物理按键（2026-09-28 调查）

- 发现日期：2026-09-28
- 现象：全按键支持（RC003 增强捕获）开启状态下，物理键盘反引号（TV 键，VK 0xC0）
  **有时候**被拦截（按下无反应）。用户报告时无法给出具体复现时刻。

## 结论（一句话）

**增强所有权租约丢失窗口内，方案 C 常驻抑制接管 TV——物理反引号随之被吞，属
fallback=legacy 设计内降级行为；「有时候」= 用户按压恰落窗口。**

## 机制链（三本日志 + 代码闭环）

1. **续租链路三段**：helper→agent `renew`（500ms）→ agent→helper `hb`（500ms，
   携带 `lease_ok`）→ helper→app `Ownership`（每条 lease_ok=true 的 hb 推一次）。
   应用侧 `OWNERSHIP_TIMEOUT = 1500ms`（rc003_bridge.rs），正常余量 1s。
2. **任何一段抖动 → 所有权停流 1500ms** → `ownership_timeout` →
   `set_enhanced_owned_mask(0)` → `gate_ready(TV)` 由 false 翻 true →
   key_gate 方案 C 常驻抑制立即恢复（TV 已映射 + 遥控器在线，无需武装直接吞）。
3. 窗口内：遥控器 TV 按压仍走映射动作（key_gate 处理，功能自洽）；**物理反引号
   被吞**（LL 层无法区分设备来源——TV/Home 走常驻抑制正是因为武装对其不可靠）。
4. 恢复：agent 租约恢复 → helper 推 Ownership → mask 重新置位 → 停止吞。
   **恢复沿此前无日志（盲区），窗口时长与影响面不可观测**——本次补齐。

## 日志证据

- **诊断日志**（`%LOCALAPPDATA%/SayAll/Logs/sayall-diagnostic.log`）：
  `ownership_timeout` ×21、`ownership_released` ×23（成对 = 同一 timeout 路径），
  分布 UTC 03:25→07:44；pid=24304 在 04:07 前后 13s/16s/19s 间隔连续三次超时
  （所有权恢复后 1.5~3s 内再次超时 = 抖动期）。另有孤立 timeout ×4（pid=7048，
  无 released 配对 = targets 空集场景，owned 本来为空）。
- **helper 日志**（`C:/ProgramData/SayAll/rc003-helper/helper-task.log`）：
  agent 心跳 stat `lease_expired=32`（agent 侧租约过期 32 次 = 抖动源头）、
  `lease_ok=false` ×80 / ×140479、`[RENEW] state=write_failed` 多次、
  `send_dropped=409`、`read_errors=4`。
- 行级无时间戳（仅会话头），精确对齐不可行，但数量与形态足以闭合证据链。

## 已加观测（3e2ac06）

- `key_gate.rs`：`PERSISTENT_SWALLOW_TOTAL` 原子计数——常驻抑制路径吞键（按下沿）
  时 `fetch_add`（钩子线程无 IO/锁，守护栏）；`attributed` 判定拆出
  `via_persistent` / `via_direct`（direct 族 VK 与 TV/Home 不相交）。
- `rc003_bridge.rs`：补 **`ownership_resumed`** 恢复沿日志（此前静默）；
  released / timeout / resumed 三沿均带 `persistent_swallow=` 快照——
  **做差即得窗口内被吞按压次数**。
- 判读示例：`ownership_timeout ... persistent_swallow=10` →
  `ownership_resumed ... persistent_swallow=12` ⇒ 该窗口内吞了 2 次按压。

## 已知边界与遗留

- 窗口内物理反引号被吞**无法在不牺牲遥控器 TV 可用性的前提下消除**
  （去掉常驻抑制 = 所有权丢失时遥控器 TV 失灵）。
- 抖动根因（renew 写失败 / agent 重连）在 Frida Gadget socket 层，未深挖；
  若日志显示窗口频繁且长，下一步查 helper→agent 写路径。
- 孤立 timeout（targets 空集时 owned 恒空）会周期性打日志，属噪音，
  可考虑 targets 为空时跳过所有权跟踪。
- 需出包真机复验新日志，等待窗口复现。
