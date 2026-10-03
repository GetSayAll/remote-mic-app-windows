# 2026-10-03 真机回归：门内延迟（方案 A / 应用信号版）导致语音会话被「按住」、无响应

## 现象（用户现场）

应用选豆包时，**不论系统当前输入法是不是豆包**，都会出现：

- 语音键「被按住」（连接页持续显示「正在接收语音」）；
- 但无任何响应（豆包语音条不出现、也没有文字）；
- 多次复现；关闭「全按键支持」后症状消失。

## 判定（A/B，单变量）

变量 = 应用是否声明 `W 1`（门内延迟 150ms 的唯一开关），其余完全一致：

| 组 | 结果 |
|---|---|
| 开启（包 `979c59e`，后并入 main `af6b736f`）| 症状出现；报告层 agent 轨迹异常：两次 `synth:frame seen` 后**无任何后续**、一次 `synth:gate delay_ms=150` 后 `skip reason=nochange`；音频会话 `generation=8 finish` → 紧邻 `generation=9 begin`（生命周期错乱）|
| 关闭（包 `fdc08ab`，仅关声明）| **症状不再出现**（用户复现确认）；豆包恢复此前「有时能拉起、有时拉不起」的行为 |

⇒ 结论：**门内延迟（在报告层钩子内 `Thread.sleep(150)`）是引入者。**

## 机制假设（待受控实验验证）

门内延迟在 WUDFHost 的 HID ioctl 回调里阻塞 150ms，等于把**设备 HID 读完成推迟 150ms**。
RC003 是单条 BLE 链路同时承载 HID 报告与 ATVV 语音，宿主侧对 HID 的阻塞会反压到设备侧：

- 释放帧/后续帧的处理被推迟或异常（对应 `seen 后无后续`）；
- 设备语音会话生命周期错乱（应用侧看到「正在接收」不结束）→ 表现为「语音键被按住」。

代码侧旁证：`outPtr` 在 sleep 期间可能被栈复用/失效，sleep 后仍按原指针写入（无失效检查）。

## 处置

- **默认关闭**：`VOICE_GATE_DECLARE = false`（`crates/sayall-windows/src/rc003_bridge.rs`）——
  应用不声明 `W 1`，助手不转发 gate，agent 门内延迟为 0；报告层合成本体不变，
  行为回到 2026-10-03 之前。
- 实现保留（一行可重新启用）；**重新启用前必须先完成受控实验**：隔离条件下验证 sleep 对
  ioctl / 设备会话的影响，找到不阻塞钩子的替代实现（或证明某个不同的实现方式安全）。
- 第一按问题回到基线（偶发失败），见
  `Bugs/2026-10-03-first-press-lost-before-ime-switch.md` 的修订节。

## 证据（可复核）

日志：`%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`

- `15:49:32.439 synth:frame seen ... clears_ok=26`（之后无 replace/skip 任何后续）
- `15:52:02.447 synth:gate delay_ms=150` + `replace ok to=0xe6`；同刻 `audio_session generation=8 action=finish`，`.551 audio_session generation=9 action=begin`
- `15:54:32.461` 同型（该会话 `submitted_samples=0`）
- A/B 包：gate on = `979c59e`（并入 `af6b736f`）；gate off = `fdc08ab`（本修复）

## 边界

- 本回归只在 RC003 增强捕获 + 报告层合成（豆包预设）路径；
- 与注入路径（微信等）无关——注入路径 press/release 全程成对（`pending_key_ups=paired`）；
- 不含设备身份、语音内容或用户路径。
