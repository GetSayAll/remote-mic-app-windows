# 2026-10-03 真机回归：门内延迟（方案 A / 应用信号版）导致语音会话被「按住」、无响应

## 现象（用户现场）

应用选豆包时，**不论系统当前输入法是不是豆包**，都会出现：

- 语音键「被按住」（连接页持续显示「正在接收语音」）；
- 但无任何响应（豆包语音条不出现、也没有文字）；
- 多次复现；关闭「全按键支持」后症状消失。

## 判定（A/B，单变量）

变量 = 应用是否声明 `W 1`（门内延迟的唯一开关，配置目标为 150ms），其余完全一致：

| 组 | 结果 |
|---|---|
| 开启（包 `979c59e`，后并入 main `af6b736f`）| 症状出现；报告层 agent 轨迹异常：两次 `synth:frame seen` 后**无任何后续**、一次 `synth:gate delay_ms=150` 后 `skip reason=nochange`；音频会话 `generation=8 finish` → 紧邻 `generation=9 begin`（生命周期错乱）|
| 关闭（包 `fdc08ab`，仅关声明）| **症状不再出现**（用户复现确认）；豆包恢复此前「有时能拉起、有时拉不起」的行为 |

⇒ 结论：**当时的门内延迟实现（在报告层钩子内 `Thread.sleep(150)`）是引入者。**
该调用实际请求等待 150 秒，并非预期的 150ms；上述 A/B 不能证明正确的 150ms 延迟也会导致同样的回归。

## 机制假设（待受控实验验证）

旧实现请求在 WUDFHost 的 HID ioctl 回调里等待 150 秒。
RC003 是单条 BLE 链路同时承载 HID 报告与 ATVV 语音，宿主侧对 HID 的阻塞可能反压到设备侧：

- 释放帧/后续帧的处理被推迟或异常（对应 `seen 后无后续`）；
- 设备语音会话生命周期错乱（应用侧看到「正在接收」不结束）→ 表现为「语音键被按住」。

另一项待验证的假设：`outPtr` 在 sleep 期间可能被栈复用/失效，sleep 后仍按原指针写入（无失效检查）。
上述反压和指针生命周期假设尚未由受控实验确认。

## 2026-10-05 单位勘误与回归验证

[Frida Thread.sleep API](https://frida.re/docs/javascript-api/#thread) 的参数单位是秒，
而 gate 命令的 `delay_ms`、默认值和日志均使用毫秒。调用边界改为
`Thread.sleep(gateDelayMs / 1000)`，使 150ms 对应 0.15 秒；同步更新 JS 与助手的 `AGENT_BUILD`。
不改变默认值、命令语义或 `VOICE_GATE_DECLARE = false`。

- `passed`：先修正 Node 测试桩与断言的单位，在旧实现上复现 4 项失败（51/55）；
  修复后运行 `node hardware/RC003/helper/agent/agent_logic_test.mjs`，55/55 通过。
  覆盖显式 150ms、默认值、`delay_ms=0` 仍取默认值、2000ms 上限，以及释放帧、observe、关闭门禁不等待。
- `passed`：助手 `--selftest --no-app-bridge`（日志写入任务目录）59/59 通过；
  仅更新 JS 代次时，已有的一致性自检会失败，同步助手代次后恢复通过。
- `deferred`：Node 桩只验证传给 Frida 的参数和报告改写，不实际等待或运行 HID 钩子。
  RC001/RC003 真机时序、首按成功率、阻塞和指针生命周期仍需受控验收；本次不重新启用门内延迟。

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
