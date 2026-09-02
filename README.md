# 无线麦 SayAll for Windows

无线麦 SayAll Windows 版把小米蓝牙遥控器 2（RC001）和小米蓝牙遥控器 2 Pro（RC003）的按键和麦克风桥接到 Windows。项目采用 Rust、Tauri 2 和 Vue 3，Windows 与 macOS 分别维护和发布。

当前仓库处于新架构开发阶段。现阶段已经建立：

- Mac 原版风格的设置界面骨架；
- ATVV、IMA/DVI ADPCM 和语音会话纯 Rust 核心；
- WinRT 已配对设备扫描、标准 GATT Model Number（2A24）型号识别、连接/通知/释放和 RC001/RC003 到 PCM 的会话管线；
- 用户明确选择端点的 WASAPI 共享模式输出、有界 PCM 队列和 padding 排空；
- 以稳定 endpoint ID 和名称持久化用户选择，启动时只恢复身份完全一致的端点；
- 记住用户明确选择的 RC001 或 RC003，并以 2–30 秒指数退避自动重连；Windows 睡眠时主动释放会话，恢复后重建 GATT/ATVV；
- 可区分连接、特征发现、能力确认、就绪、流式接收、排空、断开和失败的真实状态界面；
- 设备路径限定的 Raw Input、批量 SendInput、映射持久化与显式快捷键测试；
- 仅保存在本机的每日按键、完整语音会话和语音时长统计，以及今日、本周、全部和最近 7 天展示；
- Windows 10 1809（build 17763）安装与启动双层版本门禁；
- Windows CI 可生成带 SHA-256 和来源元数据的未签名 NSIS Preview artifact；
- Windows CI、来源归属和真机测试手册。

当前代码尚未在 Windows 主机上分别完成真实 RC001 和 RC003 运行验收，VB-CABLE 回环也未验证，因此不提供公开可安装版本，不能宣称已经可作为系统麦克风使用。CI 生成的 NSIS 仅用于验证打包结构，明确未签名、短期保留，不得作为公开发布包交付。

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

Mac 可以运行前端构建和纯 Rust 测试，但不能证明 WinRT BLE、Raw Input、WASAPI、安装器版本提示或 RC001/RC003 真机行为。
当前平台层已通过 `x86_64-pc-windows-msvc` 交叉静态检查；这只能证明 WinRT、WASAPI API 符号和类型可编译，Windows 运行时、VB-CABLE 回环与 RC001/RC003 真机结果仍以 Windows CI 和测试手册为准。

## 本地检查

```bash
pnpm install
pnpm test
pnpm build
cargo test --workspace
cargo fmt --all -- --check
```

Windows 主机上的完整检查和双型号真机步骤见 [Testing/WindowsRC003Preview.md](Testing/WindowsRC003Preview.md)。

## 开源协议

程序代码使用 GPL-3.0-only。第三方来源与素材边界见 [ATTRIBUTION.md](ATTRIBUTION.md) 和 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
