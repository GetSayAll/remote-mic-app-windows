# Chatterfly 语音触发路线选择（2026-09-23）

> 状态：公开用户态入口已实测收敛；托盘可见动作、官方 API 与可选虚拟 HID 组件仍为 `deferred`。本文不构成驱动开发或发布授权。

## 结论

当前不能通过 SayAll 既有的 `SendInput` 路径稳定拉起 Chatterfly。输入法切换和音频路由已经正常，剩余阻塞点是“如何让 Chatterfly 开始/结束收音”。在不读取私有配置、不调用私有 IPC、不向第三方进程注入的边界内，推荐顺序是：

1. 请求 Chatterfly 提供公开外部触发 API，或允许用户显式开启“接受模拟快捷键”；
2. 检查 Chatterfly 托盘菜单是否暴露用户可见的开始/停止语音动作；若存在，用 Windows UI Automation 做按下/释放生命周期验证；
3. 前两项均不存在时，另立项目评审可选虚拟 HID 兼容组件。它必须显式安装、可卸载、与普通 SayAll 主程序隔离，且不能成为基础语音路径依赖。

## 已验证矩阵

| 路线 | 观察 | 结论 |
| --- | --- | --- |
| 会话级激活 Chatterfly TSF profile | 诊断日志读回 `active_profile=chatterfly`、`matched=true` | `passed`，只证明输入法切换 |
| CABLE 音频路由 | 会话启动、采样提交正常 | `passed`，只证明音频已送出 |
| `SendInput` 左 Ctrl + 左 Win | 扫描码/虚拟键、不同顺序和间隔均提交成功；Chatterfly 不开麦 | `failed` |
| `SendInput` 左 Ctrl + 左 Alt | 实体键可开麦；两种模拟键均无反应 | `failed`，排除 Win 键特例 |
| 直接透传实体 F5 | Chatterfly 快捷键设置不接受 F5 | `failed` |
| TSF preserved key | 有效、聚焦 context 中四种 Ctrl+Win 查询均未注册 | `failed` |
| TSF language-bar button | 4 个公开项目均不属于 Chatterfly | `failed` |
| Chatterfly 设置/首页可见按钮 | 未发现开始/停止语音按钮 | `failed`；托盘菜单尚未核对 |
| 官方公开触发 API | 官方公开资料未发现 | `deferred`，需厂商确认 |
| 虚拟 HID 键盘 | Windows 官方框架和示例证明机制存在；本项目未实现、未签名、未装机 | `deferred` |

## 为什么不再换一种 `SendInput` 时序

实体 Ctrl+Win 和实体 Ctrl+Alt 都能被 Chatterfly 识别，而相同组合的多种 `SendInput` 表达均不能触发。继续改左右键、按键顺序、扫描码或延迟，只是在已经覆盖的同一注入层内枚举，不会改变事件仍是合成输入这一事实。具体过滤发生在低级钩子、Raw Input 还是其他判定层仍未知，不从第三方内部取证猜测。

## 可选虚拟 HID 组件的边界

虚拟 HID 的意义是把组合键作为设备报告送入 Windows，而不是继续合成 `SendInput`。若后续获得开发授权，至少需要单独完成：

- 独立 Helper/驱动包设计，SayAll 主程序保持普通用户权限；
- Ctrl/Win DOWN 与 UP 严格配对，覆盖断连、睡眠、中止、进程退出与迟到回调；
- 仅在用户选择 Chatterfly 且明确启用兼容组件时工作；组件缺失或失败时不残留按键；
- 驱动签名、安装、升级、卸载和恢复测试，不要求用户开启测试签名或关闭 Secure Boot；
- RC001、RC003 分别覆盖冷态首用、连续会话、快速按放、断连和睡眠恢复；
- 用 Chatterfly 实际开麦和文字上屏验收，不能以报告提交成功代替。

## 公开资料

- Microsoft `ITfKeystrokeMgr::GetPreservedKey` / `SimulatePreservedKey`
- Microsoft `ITfLangBarItemMgr::EnumItems` / `ITfLangBarItemButton`
- Microsoft Virtual HID Framework 与 `Windows-driver-samples/hid/vhidmini2`
- Microsoft Driver Signing Policy

具体链接与适用边界记录在 `ATTRIBUTION.md`。
