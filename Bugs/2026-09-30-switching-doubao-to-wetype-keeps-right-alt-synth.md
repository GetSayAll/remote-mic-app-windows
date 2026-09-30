# 从豆包切回微信后仍残留右 Alt 合成

- 发现日期：2026-09-30
- 状态：RC003 真机通过
- 影响范围：Windows 11、0.3.0 本地测试包、RC003 增强捕获 Helper；RC001 与未启用 Helper 的基础路径不受影响
- 功能点：按住说话快捷键在“豆包输入法（右 Alt）”与“微信输入法（左 Ctrl + 左 Win）”之间切换
- 现象：豆包语音条可正常出现；切回微信输入法后，应用日志显示微信和弦注入成功，但按住遥控器语音键没有微信语音条。
- 正常预期：切回微信预设时立即撤销报告层 `0x003E → 0x00E6`，物理语音键只触发应用注入的左 Ctrl + 左 Win；Helper 或应用重连也必须重放同一关闭态。

## 定位证据

- 应用已经向 Helper 下发 `S -`，并走完微信输入法激活、和弦按下与释放；系统键态探针确认左 Ctrl、左 Win、右 Alt 均未粘住。
- Helper 把 `S -` 编成 `{"type":"synth","from":0,"to":0}`；agent 只有 `off:true` 才进入关闭分支，`0/0` 会被 usage 白名单拒绝。
- 同轮 agent 心跳中 `synth_rejected` 增加，而 `synth_applied` 不增加；按语音键时 `synth_hits` 继续增加，证明旧 RightAlt 映射仍在报告层生效。
- Helper 重启后，应用握手代码只在目标为 `Some` 时重放 S 行；关闭态 `None` 被省略，resident agent 因而还能保留上一轮映射。

## 根因

关闭态在两层协议边界上都没有保持“绝对状态”语义：

1. Helper 使用了 agent 不支持的隐式 `from=0,to=0` 关闭编码；
2. 应用在新 Helper 会话握手时不发送显式 `S -`。

两者叠加后，UI 与应用状态已经切回微信，但 WUDFHost 内常驻 agent 仍把物理语音键改写成 RightAlt；RightAlt 与注入的左 Ctrl + 左 Win 同时到达第三方输入法，微信语音入口不会启动。

## 修复

- Helper 对 `S -` 编码为 agent 已支持的 `{"type":"synth","off":true}`；继续兼容当前 resident agent，无需把第三方进程注入作为主路径，也无需为本修复强制重启 WUDFHost。
- 应用在 Helper 的每次 HELLO 后都重放 S 行：开启时发送目标 usage，关闭时发送 `S -`；日志同时记录 `scope=hello` 与 `active`。
- 保留原有 fail-open：桥断开时应用侧 synth active 门禁回落为 false，Helper 失去应用事实源时也把配置回落为关闭。

## 回归验证

- TDD 红灯：Helper 单测先期望 `off:true`，旧实现实际返回 `from:0,to:0`，按预期失败：`passed`（证明测试覆盖旧缺陷）。
- `cargo test --manifest-path hardware/RC003/helper/Cargo.toml`：10/10 `passed`。
- `node hardware/RC003/helper/agent/agent_logic_test.mjs`：45/45 `passed`。
- `cargo test -p sayall-windows named_pipe_bypasses_loopback_filters_and_delivers_edges -- --nocapture`：先因 HELLO 后缺 `S -` 失败，修复后 `passed`。
- RC003 从豆包切回微信，按住语音键出现微信语音条：`passed`（2026-09-30，操作人确认）。同轮 Helper/agent 旁证：依次收到 `to=0x00E6` 与 `to=off`；`synth_applied` 从 3 增至 5，`synth_rejected` 保持 8，证明关闭报文已被应用而非再次拒绝。
- 冷/闲置首用、连续切换、快速按放、断连与睡眠恢复：`deferred`。

- 隐私检查：本文未包含个人路径、设备身份、语音内容、bridge token 或凭据；原始现场日志不提交。
