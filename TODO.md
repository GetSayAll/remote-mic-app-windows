# TODO

## Windows RC001 / RC003

- [x] 建立独立 Rust + Tauri 2 + Vue 3 工程结构。
- [x] 建立 Mac 原版风格设置界面骨架。
- [x] 建立 ATVV、ADPCM 和语音会话纯 Rust 核心。
- [x] 建立 Windows CI 和真机测试手册。
- [x] 实现 WinRT 已配对设备扫描、GATT 连接/释放、ATVV 通知和 PCM 解码代码路径；Windows 与 RC001/RC003 运行验收仍待完成。
- [x] 补充 RC001/RC003 设备名称与标准 GATT Model Number（2A24）识别，在连接快照和界面中传递型号；无法识别时保持 `unknown` 且不阻断 ATVV。Windows 双型号真机验收仍待完成。
- [x] 将 RC001 短语音场景作为 JSON 夹具回放，覆盖 40 + 80 字节拆包、20 次极速空会话、20 次完整会话和中断后首个新会话恢复；该回放不代表真实 Windows/RC001 固件验收。
- [x] 将真实连接阶段、能力与解码采样计数接入 Tauri IPC 和连接页面。
- [x] 使用同一 JSON 契约夹具验证 Rust 序列化与 TypeScript 接口的 `PlatformSnapshot`、`PairedRemote`、camelCase 字段及 RC001/RC003/unknown 枚举值；Windows WebView 运行时 IPC 仍待验收。
- [x] 实现显式 WASAPI 输出端点枚举、选择、16 kHz PCM 写入、有界队列和真实 padding 排空代码路径。
- [x] 实现 RC001/RC003 选择持久化、意外断连指数退避重连和 Windows 睡眠/恢复通知代码路径；真机恢复仍待验收。
- [x] 在 Windows 主机编译 Tauri NSIS Preview 安装包；Windows CI 已生成并复验绑定精确来源 Commit、SHA-256 和未签名状态的 artifact，安装、升级、卸载与正式签名仍待完成。
- [x] 提供去标识化运行诊断摘要和页面内复制入口；自动化已证明不导出设备身份、路径、端点名称或错误原文，Windows WebView 剪贴板仍待运行验收。
- [x] 持久化并展示仅保存在本机的每日按键次数、完整语音会话次数和语音采样时长；Windows/RC001/RC003 真实事件计数与升级保留仍待真机验收。
- [x] 对低于 Windows 10 1809（build 17763）的系统增加 NSIS 安装与应用启动双层拒绝门禁；Windows 10 1809 / Windows 11 提示和安装行为仍待真机验收。
- [x] 在 Windows CI 对 NSIS Preview 执行 `/S` 当前用户安装、启动存活、`/S` 卸载及设置保留边界验证；该自动化不代替可见安装界面、SmartScreen、Windows 10 1809 或真实用户环境验收。
- [ ] 使用真实 RC001 验证型号识别、BLE 配对、连接、断开、重连和首次语音。
- [ ] 使用真实 RC003 验证型号识别、BLE 配对、连接、断开、重连和首次语音。
- [ ] 验证 `STREAM_START → AUDIO → STREAM_STOP` 首次会话完整可用。
- [ ] 在 Windows 真机验证 WASAPI 端点初始化、VB-CABLE 回环、欠载恢复与完整尾音。
- [x] 持久化用户选择的输出端点，并在端点消失或更名时失败关闭；Windows 运行时恢复仍待真机验收。
- [x] 实现设备路径 fail-closed、隐藏消息窗口、Keyboard/HID 双来源合并和停止释放的 Raw Input 代码路径；Windows 与 RC001/RC003 真机按键验收仍待完成。
- [ ] 实现按键映射保存、热加载和 SendInput：独立映射文件、显式热加载、批量 SendInput、部分提交回滚和界面测试已完成；真实 Raw Input 边沿自动执行须分别等待 Windows/RC001/RC003 确认 Keyboard/HID 事件形态，避免重复输入。
- [ ] 完成 Windows 10 1809 / Windows 11 安装、升级和卸载验证。
- [ ] 建立自签 Authenticode、证书指纹和 SHA-256 发布流程。
- [ ] 单独评估返回键、音量键等完整 HID 实验能力。
