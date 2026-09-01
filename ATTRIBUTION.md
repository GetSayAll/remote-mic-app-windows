# 来源与归属

本仓库是独立建立的 Windows Rust/Tauri 工程，不是任何现有仓库的 Git fork。

## 产品与 UI 基准

- `HD838A/remote-mic-app`：无线麦 macOS 原版的信息架构、产品文案、RC003 图片、ATVV 行为和测试边界。
- RC003 图片 SHA-256：`658d9333853958c13ff721eb76e1a6816c1dbea16006a84e8577ad410812549f`。

## Windows 行为与测试参考

- `HD838A/remote-mic-app#249`，提交 `090a3cfc24f0e3e733b2347ee2daf87c60e10097`：Windows 独立实现、ATVV 测试夹具、语音边沿、安装升级、公开边界和 Mac 风格 UI 原型。
- `ZSTDJan/windows-remote-mic-app`：WinRT BLE、Raw Input、音频输出、发布门禁和真实硬件验证边界。
- `richlearntodo-debug/vibe-flow`，提交 `047f9d3ead54bf30de9b884adf8f7b5adefe9993`：自然 ATVV 会话、WASAPI 音频生命周期和硬件验收清单。
- `mwlt/Voice_VibeCoding`，提交 `c89410aed3b274fee5e571128b82c9c6e6689715`：Rust/Tauri 模块划分、windows-rs API、音频生命周期和托盘窗口工程经验。
- `wasapi-rs` 0.24.0：MIT 许可的 Windows Core Audio 安全封装，用于端点枚举、共享模式渲染与 padding 查询。

外部实现只作为带来源的参考。第三方应用进程注入、私有配置读取和来源不明二进制不进入稳定主路径。
