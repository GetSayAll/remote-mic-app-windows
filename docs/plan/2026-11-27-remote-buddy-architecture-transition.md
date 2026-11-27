# Windows 转向 Remote Buddy 架构：豆包语音 + RC003 全按键

- 状态：计划草案，待评审；未开始实现
- 日期：2026-11-27
- 范围：`remote-mic-app-windows` 公开宿主 + `sayall-private-platform` 私有运行时/移动端接线
- 目标分支：当前 iPhone 附近连接功能分支之后的独立工作项

## 1. 目标

把当前“RC003 Helper + Frida Gadget + 报告层改写”的豆包/全按键实现，替换为 Remote Buddy 式的公开系统接口组合：

1. **豆包语音**：用 HIDMaestro + usbip-win2 创建虚拟 USB 键盘，设备层发送右 Alt，不依赖 Frida、不依赖豆包版本。
2. **RC003 三键**：用 BthPort ETW Event 402 只读捕获返回 / 音量+ / 音量−；这三键在 Windows 侧本来就被 `kbdhid` 丢弃，不需要清键。
3. **普通按键**：继续 Raw Input + `key_gate` 抑制；不再依赖 RC003 Helper。
4. **手机语音**：把 HIDMaestro 触发器接入 `PhoneVoiceAccess.virtual_hid`，让手机语音也能触发豆包。
5. **彻底移除 Frida/RC003 Helper**：包括提权计划任务、Gadget、报告层桥接、注入依赖和对应打包资产。

最终产品行为应与 Remote Buddy 的公开路径一致：

```text
普通按键   Raw Input ──────────────┐
返回/音量  BthPort ETW（只读）──────┼─> button_mapping
豆包/手机  HIDMaestro 虚拟 USB 键盘 ─┘

手机/遥控器音频 -> CABLE Input -> 豆包麦克风 CABLE Output
```

## 2. 非目标

- 不改变 ATVV 音频链路、CABLE 路由、音频增益和语音会话状态机。
- 不引入豆包私有 RPC（`rpc.dll` / `\\.\pipe\ObricIme\oime-server`）。
- 不引入新的进程注入、Frida、LL 钩子改写或第三方输入法 Hook。
- 不重做按键映射 UI；只改“全按键支持 / 语音兼容组件”的设置语义和状态。
- 不把 HIDMaestro 当作输入捕获库；它只负责输出。
- 不承诺 RC001 / ARM64 在未实测前与 RC003 同等可用。

## 3. 当前实现与目标实现对照

| 能力 | 当前 | 目标 |
|---|---|---|
| 豆包物理遥控器语音 | RC003 Helper 注入 WUDFHost，把 `0x003E` 槽替换为 `0x00E6` | HIDMaestro 虚拟 USB 键盘发送右 Alt |
| 豆包手机语音 | `PhoneVoiceAccess.virtual_hid = None`，直接拒绝 | HIDMaestro `VirtualHidTrigger`，press/release |
| 返回/音量± 捕获 | Helper Hook `0x80018483` 读取报告并上报 | BthPort ETW Event 402 读取 `0x0064` 报告 |
| 三键清键 | Helper 在报告层清零 | **不需要**；三键在 `kbdhid` 已丢弃，无原生动作 |
| 普通映射键抑制 | `key_gate` + Helper 报告层清键 | `key_gate`；ETW 只读，不提供清键 |
| 语音键 F5 | Helper 清键/合成 + key_suppressor | HIDMaestro + 独立 F5 guard/key gate |
| 设置 | `rc003_capture_enabled` 同时控制全按键和“支持更多输入工具” | 拆成“全按键捕获”和“语音兼容组件”两个独立状态 |
| 权限 | 提权计划任务 + UAC + Frida 注入 | 签名驱动安装 + ETW 管理员配置；无注入 |
| 维护 | 豆包/Windows/Helper/Gadget 版本耦合 | 豆包版本解耦；ETW 受 Windows/蓝牙栈影响 |

## 4. Remote Buddy 参考事实

### 4.1 豆包语音

Remote Buddy 的 `DoubaoUsbKeyboard.cs`：

- `HMContext(HMBackend.Usbip)`
- 创建通用 keyboard profile
- `SubmitKeyboard(HMKeyModifiers.RightAlt, [])` / `SubmitKeyboard(None, [])`
- 普通按键仍走 `SendInput`

依赖：

- HIDMaestro 2.3.x（MIT）
- usbip-win2 0.9.8.1（BSD-2-Clause，微软签名/WHLK）
- 首次安装 USB/IP 主机控制器需要管理员/UAC
- 默认 x64 UMDF2 后端会本机自签并写入 Root/TrustedPublisher；Remote Buddy 选择 USB/IP，避免本机根证书

### 4.2 RC003 三键

Remote Buddy 文档和实现：

- 普通键：Raw Input
- 返回/音量±：BthPort ETW Event 402（HCIRAW）
- 特征 `0x0064` 的 8 字节键盘报告
- 通过 GATT UUID 探针把 ACL 句柄绑定到设备地址
- **只读**；不写报告层；三键在 `kbdhid` 已被丢弃，因此没有原生键需要清

### 4.3 普通键

- Raw Input 是监听接口，Windows 仍可能向前台交付原生键
- Remote Buddy 的三键路径不需要清；普通键也没有我们的报告层清键保证
- 我们继续保留 `key_gate` 做普通键抑制，管理员前台窗口的 UIPI 边界需要单独验收

## 5. ETW 环境依赖（详细）

这是本计划最大的不确定性来源。

### 5.1 注册表与敏感载荷

必须配置：

```text
HKLM\SYSTEM\CurrentControlSet\Services\BthPort\Parameters
  EtwLogSensitiveData = 1   (DWORD)
  MaxEtwBytes        >= 1024 (DWORD)
  EtwDropLargeEvents  = 0   (DWORD)
```

依赖：

- 写 HKLM 需要管理员权限
- `BthPort.sys` 通常在蓝牙适配器周期或重启后应用配置
- 未配置时 ETW 可能只给 ATT 头或脱敏载荷，读不到 8 字节按键报告
- 修改后需要给用户明确的“重启蓝牙/重启电脑”提示

### 5.2 ETW 会话权限

- 实时 ETW 会话通常需要管理员，或属于 Performance Log Users
- Remote Buddy 在进程内启动失败时，会请求 UAC 启动提权采集子进程
- 会话名冲突、残留会话、应用崩溃后的 orphan session 需要清理
- 企业组策略、EDR、杀毒软件可能限制 BthPort provider 或实时会话

### 5.3 OS / 蓝牙栈差异

- Windows 10 / Windows 11 的 BthPort Event 402 行为不同
- Windows 10 19045 曾出现只有入站通知、没有出站探针的情况
- 部分系统/驱动会截断载荷，只保留 3 字节 ATT 头
- 蓝牙适配器厂商、驱动版本、BLE 连接状态都会影响事件
- RC003 在 Windows 11 有 Remote Buddy 真机验证；RC001 仍待验证；ARM64 未验证
- 我们的最低系统是 Windows 10 1809，而 usbip-win2 要求 1903+；Windows 10 19041（2004/20H2）满足该版本下限，但需单独真机验证。需要决定是否把最低版本提到 1903/19041，或为 1809 保留旧系统回退

### 5.4 BLE/GATT 绑定

- 需要保持 BLE 连接和 GATT 服务发现
- 通过带唯一 UUID 的 GATT 查询关联 ACL 句柄
- 没有出站探针时，只能用入站唯一通知/单设备候选兜底
- 多只同型号遥控器、断连重连、冷启动首次按键都可能丢失绑定
- 必须排除 ATVV 音频/控制特征，避免把音频帧误解为按键
- 两条同型号设备必须按地址/句柄隔离

### 5.5 载荷与固件

- RC003 读取 `0x0064` 的 8 字节报告
- 解析 `0x00F1` / `0x0080` / `0x0081`
- 固件升级可能改变报告形状
- 需要 HCI ACL → L2CAP → ATT 重组；长度/分片/加密状态都要处理
- 需要限制记录内容，不把 BLE 原始载荷写入用户日志

### 5.6 只读语义与原生键

- ETW 不能阻止 Windows 继续处理按键
- 三键本来被 `kbdhid` 丢弃，所以无需清键
- 普通键若同时被 Raw Input 和 Windows 原生路径处理，可能出现双动作
- 我们的 `key_gate` 可以抑制普通键，但管理员前台窗口/UIPI 边界可能看不到
- 如果产品要求“所有键都无原生副作用”，ETW 只读不能完全替代 Helper；需要接受边界或保留 Helper

### 5.7 生命周期

- ETW 启动/停止延迟
- 睡眠唤醒、蓝牙适配器关闭再打开、驱动重启
- 语音会话与普通按键共享 BthPort 流量
- ETW 丢失/背压时，按键会丢失
- 应用退出、崩溃、升级后要清理会话和绑定

## 6. HIDMaestro 依赖

- HIDMaestro 2.3.x MIT；usbip-win2 0.9.8.1 BSD-2-Clause
- HIDMaestro 是 .NET/C# SDK；需要 sidecar 或原生 AOT helper 与 Rust/Tauri 通信
- 首次安装 USB/IP 主机控制器需要管理员/UAC；可能重启
- Windows 10 1903+；Windows 10 19041 满足版本下限，需单独验证；ARM64 支持 USB/IP 后端
- Windows S Mode、Secure Boot、企业驱动策略可能拒绝
- 驱动是共享的；其他 HIDMaestro 程序可能同时存在，不能随意卸载
- 虚拟键盘在 consumer 进程被强杀时可能留下按住的键，需要 watchdog/最短 hold/release
- HIDMaestro 输出的是虚拟设备，反作弊/游戏可能检测；产品需说明
- 需要引入 MIT/BSD 许可证与第三方通知
- 不能复制 Remote Buddy 的 GPL-3.0 代码；只参考其公开行为

## 7. 受影响模块

### 7.1 公开 Windows 宿主

- `crates/sayall-windows/src/lib.rs`
  - `WindowsPlatform` 组装
  - `set_enhanced_capture_enabled`
  - `set_voice_hold_hotkey`
  - `PhoneVoiceAccess` 的 virtual HID
- `crates/sayall-windows/src/ble.rs`
  - 语音键路径、`voice_synth` 门禁、SendInput fallback
- `crates/sayall-windows/src/phone_voice.rs`
  - `VirtualHidTrigger` 生产实现
  - Doubao 不再因 `virtual_hid.is_none()` 直接失败
- `crates/sayall-windows/src/rc003_bridge.rs`
  - 目标：删除或降级为 legacy fallback
- `crates/sayall-windows/src/key_gate.rs`
- `crates/sayall-windows/src/key_suppressor.rs`
- 新增 `crates/sayall-windows/src/bthport_etw.rs`
- 新增 `crates/sayall-windows/src/hidmaestro_trigger.rs`（或 sidecar client）

### 7.2 私有 Windows 运行时

- `packages/windows-remote/host/hook.rs`
  - `phone_voice::press(..., voice.virtual_hid.clone(), ...)`
- `packages/windows-remote/src/*`
  - 不改变协议；voice error code 仍保持稳定
- 语音工具选择与设置持久化

### 7.3 Tauri / 前端

- `src-tauri/src/platform.rs`
  - `set_enhanced_capture_enabled` 拆分
  - 新增 HIDMaestro/ETW 状态命令
- `src-tauri/src/lib.rs`
  - `enable_rc003_capture` / `disable_rc003_capture` 语义重命名
  - 新增驱动/ETW 配置命令
- `src/lib/bridge.ts`
- `src/pages/ConnectionPage.vue`
- `src/pages/ButtonsPage.vue`
- `src/components/MobileRemoteSection.vue`
- `src/onboarding/*`（第④步全按键、豆包步骤）
- `src/lib/i18n/*`

### 7.4 安装包与清理

- `src-tauri/tauri.conf.json`
- 构建脚本、staged resource、NSIS installer
- 移除 `hardware/RC003/helper`、`frida-gadget*.dll`、`rc003_agent.js`
- 移除计划任务和运行时目录
- 新增 HIDMaestro helper、usbip-win2 驱动资源、许可证
- 迁移旧版 Helper 授权/残留

## 8. 实施阶段

### Phase 0：验证与决策（2–4 天）

- 在独立分支做 HIDMaestro sidecar spike：
  - 创建通用 keyboard profile
  - 发右 Alt，确认 Doubao 语音条
  - 确认 press/release、崩溃释放、设备卸载
- 做 ETW spike：
  - 配置 BthPort 注册表
  - 捕获 RC003 返回/音量± 的 8 字节报告
  - 验证不清键时无原生动作
- 产出：
  - Windows 10 19041/22H2、Windows 11 真机结果（19041 满足 usbip-win2 的 1903+ 下限）
  - RC003 结果；RC001 标记 deferred
  - 是否接受 usbip-win2 最低版本 1903（19041 可直接支持）
  - 是否接受 ETW 管理员/重启

**退出条件**：两个 spike 都能在至少一台目标机完成 Doubao/三键端到端；否则不进入全面替换。

### Phase 1：HIDMaestro 接入豆包/手机语音（5–8 天）

- 新增 HIDMaestro sidecar（.NET self-contained 或 NativeAOT）
- Rust 侧 `VirtualHidTrigger` 实现
- `PhoneVoiceAccess.virtual_hid = Some(...)`
- 物理遥控器 + 手机语音都使用 HIDMaestro 发右 Alt
- 删除/停用 `set_voice_synth` 作为豆包主路径
- 保留 SendInput 作为 WeChat/其他工具路径
- F5/语音键 guard
- 设置/诊断：
  - “语音兼容组件”状态：未安装、安装中、可用、不可用
  - UAC 取消、驱动失败、Doubao 未安装的明确错误
- 退出条件：豆包物理/手机语音均可用；不再要求开启全按键支持

### Phase 2：ETW 三键捕获（6–9 天）

- 新增 BthPort ETW 模块
- BthPort 注册表配置检测/提示（不自动写，用户确认后提权）
- ETW 会话启动、清理、重连
- GATT UUID 探针 + ACL 句柄绑定
- `0x0064` 8 字节报告解析
- ETW 边沿 → `ButtonMappingRuntime` 的 `GateEdge`
- 全按键开关只控制 ETW，不再控制豆包
- 不做报告层清键
- 退出条件：RC003 三键可映射、可高亮；关闭开关后无事件；不误触物理键盘

### Phase 3：设置/UI/升级迁移（5–7 天）

- 拆分设置：
  - `voice_tool_compat` / HIDMaestro 状态
  - `full_key_capture` / ETW 状态
- 移除“支持更多输入工具 = 豆包必须开启”的文案
- 向导：
  - 全按键步骤说明 ETW/UAC/重启
  - 豆包步骤说明安装语音兼容组件
- 诊断：
  - HIDMaestro 驱动版本、sidecar 状态
  - ETW 配置、会话、绑定设备、载荷截断
- 迁移旧用户：
  - 保留旧映射/设置
  - 删除旧 Helper 计划任务/运行时目录
  - 旧“全按键”开关映射到新 ETW 状态
  - 旧“支持更多输入工具”不再强制豆包

### Phase 4：移除 Frida/RC003 Helper（2–3 天）

- 删除 `hardware/RC003/helper` 产品化路径、Gadget、agent、计划任务
- 删除 `rc003_bridge.rs` 或降级为可选 legacy
- 删除 `set_capture_targets` / `voice_synth` 的生产依赖
- 删除安装包里的 Frida 资产和 SHA/许可条目
- 卸载清理：计划任务、ProgramData、旧 Gadget 映射（需重启）
- 更新 ATTRIBUTION、LOGGING、DEBUGGING、产品文案
- 退出条件：安装后无 helper 进程、无计划任务、无 Frida 文件

### Phase 5：验收与灰度（5–8 天）

- 按第 9 节矩阵验收
- 灰度：
  - 先内部 RC003 + Windows 11
  - 再 RC003 Windows 10
  - 再 RC001
  - ARM64 作为独立门禁
- 失败回退：
  - 语音组件不可用 → 明确错误/微信
  - ETW 不可用 → 三键禁用；普通键不受影响
  - 不保留 Frida 默认回退；旧 Helper 只在迁移版本中作为可选 legacy

## 9. 验证矩阵

### 9.1 自动化

- ETW 8 字节报告解析：F1/80/81、按下/松开、重复、空槽
- HCI ACL/L2CAP/ATT 重组与截断
- GATT 探针 → ACL 句柄 → 设备地址绑定
- 多设备同型号隔离
- ETW 会话启动/停止/残留清理
- HIDMaestro `press/release` 成对、崩溃释放、最短 hold
- 设置迁移：旧 `rc003_capture_enabled` → 新两开关
- 全按键关闭时不派发 ETW 边沿
- 手机语音：`virtual_hid` 可用/不可用/错误码
- 无 Frida：构建产物不包含 Gadget/agent/计划任务

### 9.2 真机

| 维度 | 覆盖 |
|---|---|
| 系统 | Windows 10 1903/22H2、Windows 11 23H2/24H2；x64；ARM64 如可得 |
| 遥控器 | RC003、RC001；两只同型号同时在线 |
| 豆包 | 物理遥控器语音、手机语音、冷启动、快速连按、连续会话、断连重连 |
| 微信 | 回归 SendInput 路径 |
| 全按键 | 返回/音量+/音量−、所有 13 键映射、按下高亮、关闭开关 |
| 无原生副作用 | 三键无系统动作；普通键 `key_gate` 抑制；管理员前台窗口边界 |
| 驱动 | 首次安装、UAC 取消、升级、其他 HIDMaestro 程序共存、卸载 |
| ETW | 未配置、配置后重启、载荷截断、无出站探针、会话残留、睡眠唤醒 |
| 权限 | 标准用户、管理员、企业策略、杀软拦截 |
| 清理 | 卸载后无 helper/gadget/任务/ETW 会话/驱动残留 |

### 9.3 通过判据

- 豆包物理/手机语音一次按下即出现语音条并出字
- 三键映射生效且无原生动作
- 关闭全按键后三键无事件，普通键仍正常
- 无 Frida/Helper 进程和文件
- 安装/卸载可重复，驱动/ETW 状态可解释
- 失败时用户看到明确原因，不出现静默无反应

## 10. 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| ETW 载荷截断/脱敏 | 三键不可用 | 检测并提示配置/重启；记录 `report=redacted`；ETW 不可用时禁用三键 |
| ETW 需管理员/重启 | 普通用户门槛高 | 首次配置向导一次完成；解释原因；提供回退 |
| RC001/ARM64 未验证 | 兼容性回归 | 分机型门禁；未验证机型保留 legacy 或禁用承诺 |
| Windows 10 1809 不支持 usbip-win2 | 旧系统豆包不可用 | 提高最低版本或 1809 回退旧 Helper |
| HIDMaestro 驱动安装失败/杀软 | 豆包不可用 | 明确状态；WeChat/其他工具不受影响；提供诊断 |
| 虚拟键盘粘键 | 用户按键卡住 | watchdog、release on exit、最短 hold、HIDMaestro 限制说明 |
| ETW 只读，普通键原生透传 | 双动作 | `key_gate`；管理员 UIPI 边界文档化；必要时保留 legacy |
| ETW 会话冲突/残留 | 采集失败 | 唯一会话名、启动清理、崩溃恢复 |
| HIDMaestro .NET sidecar 体积 | 安装包/启动 | NativeAOT/self-contained；延迟启动 |
| Remote Buddy GPL | 许可证污染 | 只参考行为；使用 MIT HIDMaestro/BSD usbip；不复制 GPL 代码 |
| 迁移残留 | 旧 helper 注入/任务 | 升级清理、重启提示、卸载校验 |

## 11. 工作量评估

| 工作项 | 工程师日 |
|---|---:|
| Phase 0 spike | 2–4 |
| HIDMaestro sidecar + 驱动接入 | 5–8 |
| ETW 捕获/绑定/解析 | 6–9 |
| 设置/UI/迁移 | 5–7 |
| 移除 Helper/Frida/打包清理 | 2–3 |
| 真机矩阵/灰度 | 5–8 |
| **合计** | **25–39 工程师日** |

- **单人顺序开发：约 6–8 周**
- **两人并行（ETW 与 HIDMaestro 分开）：约 4–6 周**
- 不含 RC001/ARM64 深度补测的额外时间
- 若只做“豆包语音 HIDMaestro、保留全按键 Helper”：约 1–2 周
- 若只做“ETW 三键、保留豆包 Helper”：约 1–2 周

## 12. 需要产品决策的问题

1. 是否接受 HIDMaestro + usbip-win2 的**签名驱动安装**和 UAC？
2. 是否接受 ETW 的**管理员/注册表/重启**成本？
3. 最低 Windows 版本是否从 1809 提到 1903/19041？19041 已满足 usbip-win2 的 1903+ 下限。
4. RC001 未验证时：禁用三键、保留旧 Helper，还是标 beta？
5. 是否接受 ETW 只读导致的管理员前台原生键边界？
6. 是否保留一个版本的 legacy Helper 作为回退，之后再删除？
7. HIDMaestro 以 sidecar 引入还是自研 UMDF（前者快、后者可控但更贵）？

## 13. 完成定义

- 豆包物理/手机语音不再依赖 Frida/RC003 Helper。
- RC003 三键由 ETW 捕获；无报告层清键；不清也不产生原生动作。
- 普通键继续 Raw Input + key_gate；管理员前台边界有文档/验收结论。
- 设置拆分为“全按键捕获”和“语音兼容组件”；豆包不再要求全按键。
- 安装包包含 HIDMaestro/驱动/许可；卸载后无 Helper、Gadget、计划任务、驱动残留。
- RC003 Windows 11 通过；RC001/ARM64 有明确 passed/deferred 结论。
- 失败路径对用户可见：驱动失败、ETW 截断、未绑定设备、UAC 取消都有明确提示。

## 14. 参考

- Remote Buddy:
  - https://github.com/zfdang/remote-buddy-windows
  - `docs/windows-remote-input.md`
  - `docs/windows-ble-input-layers.md`
  - `docs/xiaomi-rc003-buttons.md`
  - `src/RemoteBuddy.Platform.Windows/Input/BthPortRegistryHelper.cs`
  - `src/RemoteBuddy.Platform.Windows/Input/DoubaoUsbKeyboard.cs`
- HIDMaestro: https://github.com/hifihedgehog/HIDMaestro
- usbip-win2: https://github.com/vadimgrn/usbip-win2
- 当前实现：
  - `crates/sayall-windows/src/rc003_bridge.rs`
  - `crates/sayall-windows/src/phone_voice.rs`
  - `crates/sayall-windows/src/lib.rs`
  - `crates/sayall-windows/src/ble.rs`
  - `Testing/WindowsRC003EnhancedCapture.md`
  - `docs/installation-and-configuration.md`
