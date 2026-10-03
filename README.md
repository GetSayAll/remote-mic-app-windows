# 无线麦 SayAll for Windows

<p align="center">
  <img src="Screenshots/sayall-key-mapping.png" alt="无线麦 SayAll Windows 版按键映射界面" width="960">
</p>

<p align="center">小米蓝牙语音遥控器的可视化按键映射</p>

<table>
  <tr>
    <td align="center">
      <img src="Screenshots/wechat-group-qrcode.jpg" alt="无线麦 SayAll Windows 版微信群二维码" width="220"><br>
      <strong>Windows 用户交流群</strong><br>
      微信扫码加入交流群
    </td>
    <td align="center">
      <a href="Screenshots/xhs-sayall.jpg"><img src="Screenshots/xhs-sayall.jpg" alt="无线麦小红书二维码" width="220"></a><br>
      <strong>小红书</strong><br>
      扫码关注无线麦
    </td>
  </tr>
</table>

## 视频介绍

《可能是我最近用过最不像工具的工具》展示了无线麦如何把蓝牙语音遥控器变成随手表达、控制电脑和与 AI 协作的新入口。视频中的产品界面以当时展示版本为准。

<p align="center">
  <a href="https://www.bilibili.com/video/BV13Pep6BEXe">
    <img src="Screenshots/video-introduction-cover.jpg" alt="《可能是我最近用过最不像工具的工具》视频封面" width="960">
  </a>
</p>

<p align="center"><a href="https://www.bilibili.com/video/BV13Pep6BEXe">点击封面或前往 Bilibili 观看原视频</a></p>

视频作者：[可乐不甜的跑焦日记](https://space.bilibili.com/327214328)

无线麦 SayAll Windows 版已完成对小米蓝牙语音遥控器 2（RC001）和小米蓝牙语音遥控器 2 Pro（RC003）的 Windows 真机适配，覆盖设备识别、连接、按键映射和语音桥接等已验证场景。项目采用 Rust、Tauri 2 和 Vue 3，Windows 与 macOS 分别维护和发布。

参考源码仓库：[HD838A/remote-mic-app](https://github.com/HD838A/remote-mic-app)（macOS 版）。Windows 版保持独立的平台实现，仅参考其公开的产品行为、协议经验和测试边界，不回填 macOS 代码。

当前处于预览发布阶段，已具备：

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
- Windows CI 可生成带 SHA-256 和来源元数据的未签名 NSIS Preview artifact；
- Windows CI、来源归属和真机测试手册。

RC001 与 RC003 均已完成 Windows 真机适配；两型号的按键映射真机验收均已通过，语音、安装器、VB-CABLE 和第三方输入法按测试手册分项记录，尚未覆盖的专项继续标记为 `deferred`。公开发布目前仍处于预览阶段，更新包包含 updater minisign 签名，但尚无 Authenticode 代码签名，首次运行可能触发 SmartScreen 提示。

## 用户安装与配置

首次安装、遥控器配对、VB-CABLE、语音输入软件、按键映射、更新和排障步骤见 [安装与配置指南](docs/installation-and-configuration.md)。文档同时给出了 AI Agent 的安全执行边界与可验证的完成标准；各版本的用户可见变更见 [GitHub Releases](https://github.com/GetSayAll/remote-mic-app-windows/releases)。

<p align="center">
  <img src="Screenshots/sayall-connection-audio-setup.png" alt="无线麦 SayAll Windows 版连接与语音输入设置界面" width="960">
</p>

<p align="center">连接遥控器，选择语音设备与输入工具</p>

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

治理与交付规范： [日志规范](LOGGING.md) · [Windows 发布流程](RELEASING.md) · [技术边界](TECHNICAL.md) · [排障指南](TROUBLESHOOTING.md) · [Bug 记录规范](Bugs/README.md) · [发布生命周期测试](Testing/WindowsReleaseBranchLifecycle.md)。

Windows 主机上的完整检查和双型号真机步骤见 [Testing/WindowsRC003Preview.md](Testing/WindowsRC003Preview.md)。

## 开源协议

程序代码使用 GPL-3.0-only。App Logo 和 App Icon 是保留版权的专有品牌资产，不属于 GPL-3.0-only 授权范围；详见 [LOGO-LICENSE.md](LOGO-LICENSE.md)。第三方来源与素材边界见 [ATTRIBUTION.md](ATTRIBUTION.md) 和 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
