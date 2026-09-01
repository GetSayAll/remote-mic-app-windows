# 无线麦 SayAll for Windows

无线麦 SayAll Windows 版把小米蓝牙语音遥控器 2 Pro（RC003）的按键和麦克风桥接到 Windows。项目采用 Rust、Tauri 2 和 Vue 3，Windows 与 macOS 分别维护和发布。

当前仓库处于新架构开发阶段。现阶段已经建立：

- Mac 原版风格的设置界面骨架；
- ATVV、IMA/DVI ADPCM 和语音会话纯 Rust 核心；
- Windows 平台能力边界；
- Windows CI、来源归属和真机测试手册。

当前代码尚未在 Windows 主机和真实 RC003 上完成运行验收，不提供可安装版本。

## 技术结构

```text
Vue 3 UI
   ↓ Tauri IPC
Tauri App Host
   ↓
sayall-core       ATVV、ADPCM、会话、配置、统计
sayall-windows    WinRT BLE、Raw Input、SendInput、WASAPI
```

详细方案见 [Windows Tauri 长期架构与实施路线](docs/architecture/windows-tauri-roadmap.md)。

## 开发环境

- Windows 10 1809 或更高版本，x64；
- Rust stable；
- Node.js 22 或更高版本；
- pnpm 9 或更高版本；
- Visual Studio Build Tools，包含“使用 C++ 的桌面开发”；
- WebView2 Runtime。

Mac 可以运行前端构建和纯 Rust 测试，但不能证明 WinRT BLE、Raw Input、WASAPI、安装器或 RC003 真机行为。
当前平台层已通过 `x86_64-pc-windows-msvc` 交叉静态检查；Windows 运行时和真机结果仍以 Windows CI 与测试手册为准。

## 本地检查

```bash
pnpm install
pnpm test
pnpm build
cargo test --workspace
cargo fmt --all -- --check
```

Windows 主机上的完整检查和真机步骤见 [Testing/WindowsRC003Preview.md](Testing/WindowsRC003Preview.md)。

## 开源协议

程序代码使用 GPL-3.0-only。第三方来源与素材边界见 [ATTRIBUTION.md](ATTRIBUTION.md) 和 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
