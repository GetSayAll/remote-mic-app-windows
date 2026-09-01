# 无线麦 Windows Tauri 长期架构与实施路线

## 1. 决策

无线麦 Windows 版在公开仓库 `GetSayAll/remote-mic-app-windows` 中独立开发，采用 Rust、Tauri 2 和 Vue 3。macOS 继续使用 SwiftUI/AppKit；两端独立构建、签名、打包、测试和发布。

本仓库不 fork `mwlt/Voice_VibeCoding`，也不基于其某个提交继续 Git 历史。该项目、PR #249、ZSTDJan Windows 版本和 Vibe Flow 只作为带来源的架构、实现和故障经验参考。

Windows UI 的唯一产品设计基准是无线麦 macOS 原版。Windows 使用本地标题栏、Segoe UI Variable、系统 Accent 和 WebView2，不模拟 macOS 红黄绿窗口按钮或 Liquid Glass。

## 2. 产品范围

第一阶段只支持小米蓝牙语音遥控器 2 Pro / RC003：

- WinRT BLE 发现、连接和重连；
- ATVV 能力协商和 16 kHz IMA/DVI ADPCM；
- 物理按下开始、释放结束的语音会话；
- WASAPI 输出到用户明确选择的端点；
- Windows 能通过公共 API 稳定提供的 Raw Input 按键；
- SendInput、按键映射、统计、诊断和设置；
- Windows 10/11 x64。

第一阶段不包含：

- macOS、iOS、Web 或服务端代码；
- T1、汉王 V60、DJI Mic 2；
- 第三方 App 私有配置、内部数据库或私有协议；
- 依赖 Frida 或管理员权限才能工作的基础语音；
- 未完成 Windows 真机验收的完整 13 键承诺。

## 3. 架构

```text
Vue 3 UI
  ├─ 按键
  ├─ 统计
  ├─ 连接与语音
  ├─ 权限
  └─ 关于
        │
        ▼
Tauri Commands / Events
        │
        ▼
Application State
        │
        ├─ sayall-core
        │    ├─ ATVV
        │    ├─ ADPCM
        │    ├─ Voice Session
        │    ├─ Settings
        │    └─ Statistics
        │
        └─ sayall-windows
             ├─ WinRT BLE
             ├─ Raw Input
             ├─ SendInput
             ├─ WASAPI
             └─ Windows lifecycle
```

### 3.1 `sayall-core`

必须是纯 Rust，不能依赖 Windows、Tauri、WebView 或第三方输入法。核心协议测试应能在 macOS、Linux 和 Windows 上运行。

### 3.2 `sayall-windows`

只包含 Windows 公共 API。所有 Windows 句柄、COM、WinRT、HID 和音频资源必须有明确生命周期。BLE 回调和音频回调不得执行阻塞文件或进程操作。
RC003 断连由 BLE 工作线程统一清理后进入 2–30 秒指数退避；用户主动断开只停止本次运行的重连。Windows 电源回调只投递事件，睡眠前的 GATT/音频释放与恢复后的重新发现仍在同一工作线程串行完成，旧 connection generation 回调继续丢弃。

### 3.3 Tauri Host

Tauri Host 负责应用生命周期、IPC、托盘和窗口，不直接解析 ATVV 或处理音频帧。主程序保持普通用户权限。
设备专属设置写入本应用自己的 Tauri 配置目录；音频端点同时保存稳定 endpoint ID 与用户选择时的名称，不跨设备同步，也不读取 Windows 默认输出配置。启动恢复必须先验证两者一致。

### 3.4 可选高级 Helper

返回、独立音量等 Windows 不稳定提供的 HID usage 单独评估。需要提权的能力必须是可选 Helper，并且失败时不能影响 BLE 语音和可靠按键。

## 4. UI

沿用 macOS 原版的页面层级、遥控器实物图、卡片、状态和侧栏结构。第一版页面顺序：

1. 按键；
2. 统计；
3. 连接与语音；
4. 权限；
5. 关于。

未实现的 Mac 页面不显示空入口。中文最终字号不小于 12pt。设置尽量在大页面中铺平完成，不使用长下拉列表或连续确认弹窗。

界面只能显示真实状态。进程已启动、事件已入队或音频已解码都不能被展示成“用户语音已经可用”。

## 5. 来源策略

### PR #249

迁移测试夹具、会话边沿、音频排空、升级兼容、统计和公开边界检查。放弃 Python/PySide6、Qt、C++ 迁移骨架、第三方输入法进程注入和内部协议。

### ZSTDJan Windows 版本

参考 WinRT 缓存、PortAudio/WASAPI 端点、Raw Input、安装器和许可证门禁。其硬件结论需要本项目独立复验。

### Vibe Flow

参考自然 ATVV 生命周期、三进程故障隔离思想、100 次按下/释放和 60 秒边界验收。其单体 C# UI 不作为代码基线。

### Voice VibeCoding

参考 Rust/Tauri 结构、windows-rs、音频占用策略和窗口恢复。不得继承 Git 历史、品牌、配置目录或未经审计的 VB-CABLE、Frida、WinUHid 二进制。

## 6. 开发阶段

### 阶段 A：工程与来源基线

- Rust Workspace、Tauri、Vue；
- Windows CI；
- UI 静态壳；
- 来源矩阵；
- Mac 可运行的纯逻辑测试。

### 阶段 B：RC003 语音核心

- 唯一候选发现；
- GATT 特征发现和通知；
- ATVV 能力；
- ADPCM；
- WASAPI；
- 会话排空、重连和错误恢复。

当前实现状态：候选扫描、GATT 连接与通知、ATVV 能力、ADPCM/PCM、连接代次隔离、显式端点 WASAPI 输出、基于 padding 的会话排空、稳定 endpoint ID + 原名称恢复，以及 RC003 选择持久化、2–30 秒退避重连和 Windows 睡眠/恢复通知代码已落地并通过静态与纯逻辑检查。音频端点缺失或名称变化会失败关闭；BLE 清理、重连和恢复都不接受旧代次回调。Raw Input 已建立精确设备路径选择、隐藏消息窗口、Keyboard/HID 报告解析、双来源并集去重和停止释放代码路径；语音键在此层明确排除，继续由 ATVV 独占。Windows 运行、真实睡眠恢复、VB-CABLE 回环和 RC003 真机验证尚未完成。

### 阶段 C：可靠按键

- Raw Input；
- 设备身份；
- SendInput；
- 普通按键单击、双击和长按；
- 语音键保持即时生命周期。

当前实现状态：批量 SendInput 引擎、最多四键快捷键、左右修饰键物理 scan code、扩展键标记、部分提交与未知交付状态回滚，以及独立 `button-mappings.json` 持久化和运行时热加载已实现。按键页面只允许用户显式测试已保存快捷键。Raw Input 事件尚不自动触发 SendInput：Windows Raw Input 不能按设备阻止前台程序已经收到的原始 Keyboard 事件，真机确认 RC003 的 Keyboard/HID-only/双路径行为前自动串联会有重复输入风险。

### 阶段 D：产品化

- Onboarding；
- 设置、统计、诊断、日志；
- 中英文和主题；
- 安装、升级、卸载和更新；
- 自签 Authenticode、证书指纹和 SHA-256。

### 阶段 E：实验性完整 HID

单独研究返回和音量键，不阻塞 Preview。模拟、驱动存在或 HID Tap ready 都不能代替真实按下/释放验收。

## 7. 验证边界

Mac 开发机可以证明：

- Vue 类型检查和生产构建；
- Rust 核心测试；
- 格式化和静态检查；
- 非 Windows 平台不会伪造可用状态。

Mac 开发机不能证明：

- WinRT BLE 能连接 RC003；
- Windows Raw Input 报告；
- WASAPI 和 VB-CABLE；
- SendInput 对真实目标应用有效；
- Windows 安装、升级、签名和 SmartScreen；
- 真实语音首次会话完整可用。

这些项目必须在 Windows 主机按 `Testing/WindowsRC003Preview.md` 完成。

## 8. 完成标准

公开 Preview 至少满足：

- Windows CI 从干净提交构建；
- 第一次真实 `STREAM_START → AUDIO → STREAM_STOP` 成功；
- 快速按下/释放和连续会话没有重复、卡键或尾音丢失；
- BLE 断开、休眠和重连后恢复；
- 稳定按键不误伤普通键盘；
- 安装升级保留配置且只有一个安装条目；
- Release 包含来源、许可证、SHA-256、已完成和未完成的真机边界。
