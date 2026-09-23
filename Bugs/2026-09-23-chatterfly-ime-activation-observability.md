# Chatterfly 输入法激活缺少读回判据

- 发现日期：2026-09-23
- 状态：等待真机验证
- 影响范围：本地 0.2.6 测试包、Windows、Chatterfly；RC001/RC003 未分别验收
- 功能点：语音键按住说话目标输入法切换与诊断日志
- 现象：用户选择 Chatterfly 后，语音输入没有拉起；左 Ctrl + 左 Win 仍由微信输入法响应。
- 复现条件：曾用旧包切换微信输入法；选择 Chatterfly 并按遥控器语音键。
- 正常预期：会话级 TSF 激活 Chatterfly，之后注入左 Ctrl + 左 Win，Chatterfly 开麦、松开后上屏。
- 证据：用户提供的诊断日志截至 2026-09-23 09:44:03Z，最后进程 source_revision=84cf35b；Chatterfly 会话仅有 `target=generic` 和快捷键注入成功，没有 TSF 激活记录。该日志早于提交 2b4f056，不能证明新版已执行。检查时本机 WeType 的 HKCU profile `Enable=0`，Chatterfly 在 HKLM 注册且 Windows 用户语言列表包含其 TIP；SayAll 保存的目标仍是 `wetype`。
- 根因：旧版没有 Chatterfly TSF 激活；新版 2b4f056 已增加调用，但只记录 API 结果，未读回活动 TSF profile，无法区分“调用返回成功”和“目标实际成为活动输入法”。用户当前失败的具体后续环节仍未知。
- 修复：增加 TSF 切换前后读回日志，按公开 profile 标识分类为 `wetype`、`chatterfly`、`other` 或 `none`；保留注入及失败路径。
- 验证：Rust 单元测试与 Tauri 仿真检查 passed；新版真实语音键、Chatterfly 开麦和文字上屏 deferred。
- 隐私检查：日志仅记录产品分类、结果与耗时，不记录输入内容、语音、设备身份、前台窗口标题或路径。
