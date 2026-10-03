# 无线麦 SayAll Windows 版：开发与构建

面向开发者与维护者。用户安装与使用说明见 [README.md](README.md) 与 [安装与配置指南](docs/installation-and-configuration.md)；技术边界与验收词汇约定见 [TECHNICAL.md](TECHNICAL.md)。

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

## 当前技术能力

- Mac 原版风格的按键、连接、权限、设置四个页面；
- ATVV、IMA/DVI ADPCM 和语音会话纯 Rust 核心；
- WinRT 已配对设备扫描、标准 GATT Model Number（2A24）型号识别、连接/通知/释放和 RC001/RC003 到 PCM 的会话管线；
- 用户明确选择端点的 WASAPI 共享模式输出、有界 PCM 队列和 padding 排空；
- 以稳定 endpoint ID 和名称持久化用户选择，启动时只恢复身份完全一致的端点；同时支持经典 2 通道与新版 16 通道的 VB-CABLE 端点；
- 记住用户明确选择的 RC001 或 RC003，并以 2–30 秒指数退避自动重连；Windows 睡眠时主动释放会话，恢复后重建 GATT/ATVV；
- 可区分连接、特征发现、能力确认、就绪、流式接收、排空、断开和失败的真实状态界面；
- 设备路径限定的 Raw Input、批量 SendInput 与映射持久化；单击 / 双击 / 长按动作支持预设快捷键、打开应用和自定义组合键，配置可导出 / 导入或恢复默认；
- 连接页按输入工具组织：豆包输入法、微信输入法、Vokie 和其他工具，选中后自动配置按住说话快捷键并给出准备清单；
- 「全按键支持」（连接页同一开关叫“支持更多输入工具”）让返回 / 音量+ / 音量− 三个按键也能参与映射，每次开启都需要通过 Windows 系统授权；
- 设置页：外观（跟随系统 / 浅色 / 深色）、登录时自动启动、应用图标（默认 / 几何鸭）、检查更新（含预览通道）和问题反馈入口；
- 权限页：蓝牙、按键监听与映射、音频设备三项状态，以及不含设备地址和语音内容的诊断摘要与日志目录入口；
- Windows 10 1809（build 17763）安装与启动双层版本门禁；
- 可见的 VB-CABLE 缺失提示与官方下载入口，以及唯一 CABLE 设备首次启动时的自动检测和配置；
- Windows CI 可生成带 SHA-256 和来源元数据的未签名 NSIS Preview artifact。

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
# 一键前置自检（推荐，push 前跑，约 1-2 分钟；通过 = CI 的快速步骤必过）
powershell -ExecutionPolicy Bypass -File scripts\ci-preflight.ps1

# 或分步执行：
pnpm install
pnpm test
pnpm build
cargo test --workspace
cargo fmt --all -- --check
```

发布前深度自检：`ci-preflight.ps1 -Full`（追加 runtime-simulation 构建）。纯文档/非功能改动（**.md、docs/、Testing/、artifacts/）不触发 CI。完整提交纪律见 [BRANCH_MANAGEMENT.md](BRANCH_MANAGEMENT.md)。

## 测试与真机验收

- Windows 主机上的完整检查和双型号真机步骤见 [Testing/WindowsRC003Preview.md](Testing/WindowsRC003Preview.md)；
- 发布生命周期测试见 [Testing/WindowsReleaseBranchLifecycle.md](Testing/WindowsReleaseBranchLifecycle.md)；
- RC001 与 RC003 的真机结果分别记录；仿真、交叉静态检查与 CI 结果不替代真机验收。

## 其他仓库与本仓库的关系

参考源码仓库：[HD838A/remote-mic-app](https://github.com/HD838A/remote-mic-app)（macOS 版）。Windows 版保持独立的平台实现，仅参考其公开的产品行为、协议经验和测试边界，不回填 macOS 代码。

## 治理与交付规范

[日志规范](LOGGING.md) · [Windows 发布流程](RELEASING.md) · [技术边界](TECHNICAL.md) · [排障指南](TROUBLESHOOTING.md) · [Bug 记录规范](Bugs/README.md) · [提交与分支纪律](BRANCH_MANAGEMENT.md) · [来源归属](ATTRIBUTION.md)。
