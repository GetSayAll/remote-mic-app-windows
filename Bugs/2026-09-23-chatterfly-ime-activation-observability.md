# Chatterfly 输入法激活缺少读回判据

- 发现日期：2026-09-23
- 状态：诊断日志已验证；Chatterfly 语音触发仍失败，待公开接口可行路径
- 影响范围：本地 0.2.6 测试包、Windows、Chatterfly；本轮 RC003 语音触发 failed，RC001 deferred
- 功能点：语音键按住说话目标输入法切换与诊断日志
- 现象：用户选择 Chatterfly 后，语音输入没有拉起；左 Ctrl + 左 Win 仍由微信输入法响应。
- 复现条件：曾用旧包切换微信输入法；选择 Chatterfly 并按遥控器语音键。
- 正常预期：会话级 TSF 激活 Chatterfly，之后注入左 Ctrl + 左 Win，Chatterfly 开麦、松开后上屏。
- 证据：用户提供的旧日志截至 2026-09-23 09:44:03Z，最后进程 source_revision=84cf35b；Chatterfly 会话仅有 `target=generic` 和快捷键注入成功，没有 TSF 激活记录。该日志早于提交 2b4f056。检查时本机 WeType 的 HKCU profile `Enable=0`，Chatterfly 在 HKLM 注册且 Windows 用户语言列表包含其 TIP。
- 新版实测：安装 source_revision=5bf2a81 的本地包并选择 Chatterfly 后，RC003 三次语音会话（14:33:15Z、14:33:20Z、14:33:57Z）均记录 `ime_query active_profile=chatterfly`、`ime_readback matched=true`、`chord_press result=ok`、虚拟声卡音频流启动与采样提交；但 Chatterfly 麦克风访问时间未更新，用户确认没有开麦或上屏。实体键盘左 Ctrl + 左 Win 在同一文本框可开麦并上屏，麦克风访问时间于 14:35:56Z 和 14:42:48Z 更新。
- 隔离实验：`chatterfly_hotkey_probe` 通过公开 `SendInput` 测试扫描码/虚拟键、80ms/0ms 间隔以及 Ctrl→Win/Win→Ctrl 顺序；每次 DOWN/UP 交付均为 1，Chatterfly 麦克风访问时间没有更新。用户确认前两次模拟按键期间文本框焦点保持；随后实体键盘对照仍成功。Win→Ctrl 零间隔测试的焦点情况尚未得到用户确认，不单独作为决定性证据。
- 实体 F5 候选路径：2026-09-23 在 Chatterfly 可见设置页尝试把语音输入快捷键改为 F5；用户用实体键盘试录后确认“不支持修改为 F5”。退出录入后界面重新显示原来的左 Ctrl + Win，未留下空快捷键。首页未发现可见的开始/结束语音按钮。因此不能通过直接透传 RC003 的原生 F5 完成当前按住说话适配；此结论不依赖 SendInput 是否被过滤。
- 非 Win 修饰键对照：用户确认 Chatterfly 快捷键仅支持 Ctrl、Alt、Shift、Win；临时设为左 Ctrl + 左 Alt 后，实体键盘可触发 Chatterfly 开麦（当次未上屏，因此只判开麦 passed）。同一文本框、焦点保持时，用公开 `SendInput` 分别注入扫描码和虚拟键形式的左 Ctrl + 左 Alt，两个按下及释放沿均已提交，Chatterfly 无反应，麦克风访问时间未更新。由此排除“仅 Win 参与组合键导致失败”的假设；具体是注入标志过滤、Raw Input 还是其他第三方判定逻辑仍未知。试验结束已由实体键盘恢复原左 Ctrl + 左 Win，并在可见设置页读回确认。
- 根因范围：旧版缺 Chatterfly TSF 激活已由 2b4f056 修复；当前 TSF 读回和音频路由均正常，失败收敛到 Chatterfly 对模拟快捷键的响应环节。`SendInput` 只保证事件插入输入流，不保证第三方应用处理；Chatterfly 是否过滤注入标志或只接收设备级 Raw Input 尚属推断，不能写成已证实根因。
- 修复：增加 TSF 切换前后读回日志，按公开 profile 标识分类为 `wetype`、`chatterfly`、`other` 或 `none`；保留注入及失败路径。
- 验证：TSF 读回日志、Rust 单元测试与 Tauri 仿真检查 passed；本轮 RC003 触发 Chatterfly 开麦及文字上屏 failed；RC001 和其他生命周期场景 deferred。当前本地包仅用于诊断，不应宣称 Chatterfly 适配完成。
- 隐私检查：日志仅记录产品分类、结果与耗时，不记录输入内容、语音、设备身份、前台窗口标题或路径。
