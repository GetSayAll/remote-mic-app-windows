# 输入法切换后约 2.5 秒内快按首按失败（僵死态；焦点往返可解）

- 发现日期：2026-10-10（现象自 2026-10-03 多轮排查延续；本次为根因定性与修复）
- 状态：已修复（真机 passed；RC001 与组合场景 deferred）
- 影响范围：0.8.5 本地测试包；Windows 11、RC003、豆包/微信输入法；DimAgent（Electron/Chromium）与记事本（Win32）同类
- 功能点：`sayall-windows::ime` 会话切换与语音按下路径（`scope=voice_press|tool_select`）
- 现象：工具切换后"立刻按"语音键拉不起语音条（`voice_verify … mic=not_observed`），再按一次通常恢复；极端序列下目标窗口连物理按键也无响应（僵死态），手动"点走再点回"或切走再切回可即时治愈
- 复现条件：选豆包卡 → 切到目标应用 → 在真实切换完成后约 2.5 秒内按下语音键（越快越稳定复现）
- 正常预期：切换完成后任意时刻按下都应在第一次拉起语音条
- 证据（2026-10-10 当日 47 次按下全量对照，`sayall-diagnostic.log`，UTC）：
  - 记事本：dt（按下 − 最近一次真实切换）1–2 秒桶 **0/4 全败**；≥2.4 秒 10/1 成功（session 9/12/20/21 败；10/11/13/22/38/40/41/42/44 成）
  - DimAgent：快按（≤2.5 秒）高失败（session 35/36 等）；长闲置 70 秒以上全成（45/46/47）；按下自身触发切换的样本 3/4 败（16/24/25）
  - 记事本"第二次按下失败"回归（39/43，dt≈1.1–1.3 秒）＝ 5 秒间隔逻辑把真实切换推迟进按动窗口所致（`gap_applied waited_ms≈3.2s` 紧邻失败样本）
  - 修复后真机：DimAgent session 48（dt≈1.0 秒）、记事本 session 49（dt≈0.8 秒）、DimAgent session 50 全部首按 `mic=observed`；每次切换后出现 `ime_post_switch_bounce`；`voice_heal` 零触发
- 根因：会话级 TSF 切换（`TF_IPPMF_FORSESSION`）完成 ≠ 目标窗口完成输入法上下文重绑；重绑需要一次窗口焦点往返（`WM_KILLFOCUS`/`WM_SETFOCUS` 对），约 2.5 秒内未完成时按下被吞。与目标应用框架无必然关系（Chromium 与 Win32 同构）——"切换先到、按得越快"就越容易命中
- 修复（`crates/sayall-windows/src/ime.rs` + `lib.rs`，PR #239）：
  1. 删除 5 秒最小切换间隔与 `SkippedRecentSwitch`（2026-10-09 的假设性修复，未被验证且制造记事本回归）
  2. 新增 `post_switch_rebind_safeguard()`：`scope=tool_select` 切换成功后 300ms 做一次不可见焦点往返（`AttachThreadInput` + 同线程替身窗口 `SetFocus` 配对，约 80ms；失败路径有界）；日志 `ime_post_switch_bounce`
- 保留机制：失败确认后自动救回 `voice_heal action=focus_bounce`（同款焦点往返，替换原 Win+Space 步进）；按下路径（`scope=voice_press`）不插入焦点往返（和弦已在下行），由救回兜底
- 验证：`cargo test -p sayall-windows --lib` 329 passed / 0 failed / 13 ignored；`cargo check -p sayall-windows` 通过；真机端到端见上（操作人逐项确认"两边都是按一次就出"）。**deferred**：RC001 同矩阵；冷态首用/断连/睡眠恢复组合；记事本退化 bounce 路径观察项（`kicker=0x0 restored=false`，结果仍一次成）
- 隐私检查：本文与日志引用均不含个人路径、设备地址、语音内容或凭据；窗口/进程仅以会话号与类名 hash 引用
