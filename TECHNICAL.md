# 无线麦 SayAll Windows 技术边界

## 支持范围

- Windows 10 1809（build 17763）及更高版本，x64；
- 小米蓝牙遥控器 2 / RC001 与小米蓝牙遥控器 2 Pro / RC003；
- Rust + Tauri 2 + Vue 3；Windows 平台 API 只位于 `sayall-windows`。

## 模块边界

| 模块 | 职责 |
| --- | --- |
| `crates/sayall-core` | ATVV、ADPCM、PCM、会话、统计和配置；不得依赖 Windows API、Tauri 或 WebView |
| `crates/sayall-windows` | WinRT BLE、GATT、Raw Input、HID 归因、SendInput、WASAPI、输入法和电源恢复 |
| `src-tauri` | Tauri 命令、诊断、更新器和应用生命周期 |
| `src/` | Vue 页面、IPC 客户端和用户可见状态 |

基础语音路径不得依赖 Frida、管理员权限或虚拟 HID 驱动。需要提权的增强能力必须是显式启动的独立 Helper，不得改变普通用户主路径。

## 语音与按键

语音键只有按下开始、释放结束的实时生命周期；快捷键注入只叠加在 ATVV 会话上，断连、睡眠、取消和退出必须统一释放。普通按键映射可使用 Raw Input 与公开 SendInput；不能读取第三方 App 私有配置、数据库、内存或协议，也不能以进程注入作为稳定路径。

## 音频与连接

BLE 连接、特征订阅、能力确认、流式接收、PCM 解码、WASAPI 入队/播放/排空和断连恢复必须分别可观测。端点与应用会话静音自愈只作用于用户明确选择的 CABLE 端点和当前进程会话，不修改系统默认设备或其他应用。

## 验证边界

`passed` 只用于实际执行并观察通过，`failed` 用于实际执行但不满足预期，`deferred` 用于当前不可用的 Windows 主机、真实硬件或第三方工具。Mac 上的构建、纯 Rust 测试、运行时仿真和交叉静态检查不能证明 Windows/RC001/RC003 真机行为。
