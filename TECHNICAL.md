# 无线麦 SayAll Windows 技术边界

## 支持范围

- Windows 10 1809（build 17763）及更高版本；
- 架构：**x64 是完整支持的目标**（主程序、安装器与更新通道均为 x64 单包）；
  **ARM64 上基础语音与基础按键可用**，「全按键支持」（增强捕获）需要安装包内按架构
  携带的 arm64 助手与 Gadget，运行时按系统原生架构（`IsWow64Process2` 的 `nativeMachine`）
  选择；助手或 Gadget 缺失时该功能明确提示暂不可用，基础路径不受影响。范围与依据见
  [docs/decisions/0003-arm64-enhanced-capture-scope.md](docs/decisions/0003-arm64-enhanced-capture-scope.md)；
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

“全按键支持”开启时，独立 Helper 只接管当前已配置映射的 13 个普通按键，并通过带代次的 ACK/心跳把逐键所有权交给主程序；主程序只有在所有权有效时才停用该键的旧全局键盘门控。关闭、失联、超时或映射移除必须先撤销所有权并恢复旧 Raw Input/门控逻辑。语音键不参与；返回/音量±在功能关闭时维持既有不可用边界，其它按键必须继续可用。

动作「聚焦输入框」把键盘焦点放到**当前前台应用**的可编辑控件（不切换应用、不注入文字），只用公开 UI Automation 读取控件语义（类型 / AutomationId / ClassName / 是否可聚焦 / 是否只读 / 是否密码框 / 相对矩形），不读取任何输入内容；候选必须可聚焦、非只读、非密码且语义不含敏感词。聚焦请求交给单一后台线程串行执行，带重试与总预算，聚焦后必须用系统焦点读回确认；「打开应用」可选择只打开、注入该应用自己的聚焦快捷键或聚焦用户记录的输入框。记录的输入框特征（含 `name` 与窗口标题）属本地配置数据，只写本机配置文件，不进日志、不随诊断上传。目标以管理员身份运行时不可读（UIPI），按失败提示处理，不要求用户提权；无 UIA 树的应用不做位置点击兜底。

## 界面语言

应用语言只有 `zh-Hans` / `en` 两种有效界面语言，设置值另有 `system`（按 Windows 显示语言解析：中文主语言映射 `zh-Hans`，其余映射 `en`）。

- 前端以**中文原文作为翻译 key**（如 `t("设置")`），英文词条按页面/模块放在 `src/lib/i18n/en/*.ts`，由 `src/lib/i18n.ts` 汇总。`effectiveLanguage` 是响应式值，页面/组件直接调用 `t()` 即可在切换时原地重渲染；不得在模块加载时把 `t()` 结果缓存成常量。
- 同一中文原文在不同语境含义不同时使用命名空间：侧栏与页面标题用 `tNav()`（`nav.*`），设备徽标「推荐」仍用 `t("推荐")`（Recommended），向导阶段「设置」用 `t("设置")`（Set Up）。新增导航/页面标题必须走 `tNav`。
- 原生外壳由 Rust 同步：`apply_native_language` 设置主窗口标题与托盘 tooltip，并在每次切换时用**新的** `Menu` / `MenuItem` 调 `set_menu`，不复用已挂载菜单项（规避 Mac 2026-08-01 语言切换菜单重复挂载异常）。启动时在读取设置后应用；WebView 崩溃提示标题也按应用语言选择。
- 自动化门禁：`src/lib/i18n.test.ts` 扫描所有 `t("…")` / `tNav("…")` 字面量 key、检查英文词条缺失与跨 fragment 同 key 冲突；页面测试另断言英文渲染无中文残留。真实 Windows 真机切换矩阵见 [Testing/WindowsUiLanguage.md](Testing/WindowsUiLanguage.md)。

## 音频与连接

BLE 连接、特征订阅、能力确认、流式接收、PCM 解码、WASAPI 入队/播放/排空和断连恢复必须分别可观测。端点与应用会话静音自愈只作用于用户明确选择的 CABLE 端点和当前进程会话，不修改系统默认设备或其他应用。

## 验证边界

`passed` 只用于实际执行并观察通过，`failed` 用于实际执行但不满足预期，`deferred` 用于当前不可用的 Windows 主机、真实硬件或第三方工具。Mac 上的构建、纯 Rust 测试、运行时仿真和交叉静态检查不能证明 Windows/RC001/RC003 真机行为。
