# TODO

## Windows RC003

- [x] 建立独立 Rust + Tauri 2 + Vue 3 工程结构。
- [x] 建立 Mac 原版风格设置界面骨架。
- [x] 建立 ATVV、ADPCM 和语音会话纯 Rust 核心。
- [x] 建立 Windows CI 和真机测试手册。
- [x] 实现 WinRT 已配对设备扫描、GATT 连接/释放、ATVV 通知和 PCM 解码代码路径；Windows 与 RC003 运行验收仍待完成。
- [x] 将真实连接阶段、能力与解码采样计数接入 Tauri IPC 和连接页面。
- [ ] 在 Windows 主机编译 Tauri 安装包。
- [ ] 使用真实 RC003 验证 BLE 配对、连接、断开和重连。
- [ ] 验证 `STREAM_START → AUDIO → STREAM_STOP` 首次会话完整可用。
- [ ] 实现并验证 WASAPI 音频端点枚举与写入。
- [ ] 实现并验证可靠 Raw Input 按键。
- [ ] 实现按键映射保存、热加载和 SendInput。
- [ ] 完成 Windows 10/11 安装、升级和卸载验证。
- [ ] 建立自签 Authenticode、证书指纹和 SHA-256 发布流程。
- [ ] 单独评估返回键、音量键等完整 HID 实验能力。
