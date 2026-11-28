# Windows 转向 Remote Buddy 架构：豆包语音 + RC003 全按键

- 状态：计划草案，评审修订；未开始实现
- 日期：2026-11-27；评审修订：2026-11-28
- 范围：`remote-mic-app-windows` 公开宿主 + `sayall-private-platform` 私有运行时/移动端接线
- 目标分支：当前 iPhone 附近连接功能分支之后的独立工作项
- 决策（2026-11-28）：最低 Windows 版本定为 **Windows 10 1903（18362）**；1809 不再进入该架构。Windows 10 19041 满足下限，仍需真机验证。当前应用声明暂不改，实施阶段再调整。
- 评审结论（2026-11-28）：拆成 **豆包语音 / ETW 三键 / 旧 Helper 退役** 三个独立交付；不再以“一次全面替换并删除旧 Helper”为默认路径。本轮修订吸收 P1/P2 评审意见，见 §6.1、§5.8、§8、§9。

## 0. 评审修订摘要

| 评审意见 | 本计划的处理 |
|---|---|
| P1 豆包缺少运行时权限设计 | 新增 §6.1：独立高权限 HIDMaestro broker、服务/pipe 安全、标准用户首按验收、失败回退 |
| P1 删除 Helper 改变普通键行为 | 新增 §5.9 普通键退役门禁；未通过保留 legacy，不随语音/三键删除 |
| P1 RC001 迁移顺序会丢功能 | Phase 3 增加 RC001/RC003 旧版升级门禁；Phase 4 按型号条件退役 |
| P1 快捷键语义遗漏 | 新增 §3.1 左/右 Alt、微信和弦、其他工具、无按键矩阵；明确 release/epoch |
| P2 ETW 何时不能派发/丢 UP | 新增 §5.4 身份绑定、§5.8 派发门禁和 `release_all` |
| P2 旧开关不能当授权/卸载冲突 | §5.1 记录 HKLM 原值/所有权；§6.2/§7.4 共享驱动按所有权清理 |
| P2 支持范围/手机依赖 | §6.3 明确 1903 x64 新架构与 1809 老用户；§3.1 明确 #443 依赖 |

## 1. 目标

把当前“RC003 Helper + Frida Gadget + 报告层改写”的豆包/全按键实现，替换为 Remote Buddy 式的公开系统接口组合：

1. **豆包语音**：用 HIDMaestro + usbip-win2 创建虚拟 USB 键盘，按配置发送单个左/右 Alt（默认右 Alt），不依赖 Frida、不依赖豆包版本。
2. **RC003 三键**：用 BthPort ETW Event 402 只读捕获返回 / 音量+ / 音量−；这三键在 Windows 侧本来就被 `kbdhid` 丢弃，不需要清键。
3. **普通按键**：目标是 Raw Input + `key_gate` 抑制；在 §5.9 门禁通过前，保留旧 Helper 的普通键路径，不随语音/三键一起删除。
4. **手机语音**：把 HIDMaestro 触发器接入 `PhoneVoiceAccess.virtual_hid`；该接口仍依赖 #443，物理遥控器先行。
5. **分阶段移除 Frida/RC003 Helper 的生产依赖**：语音和三键先迁；普通键抑制、RC001、ARM64 各自通过门禁后再退役对应路径，不把“删除 Helper”本身当作目标。

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
- 不把“删除旧 Helper”本身当作验收目标；普通键抑制或某个型号尚未通过门禁时，保留对应旧路径。
- 不承诺 Windows 10 ARM64 支持 HIDMaestro/usbip-win2 新路径；该组合按 usbip-win2 的目标版本单独判定。
- 不把旧 `rc003_capture_enabled=true` 当作写 HKLM 或安装驱动的授权。

## 3. 当前实现与目标实现对照

| 能力 | 当前 | 目标 |
|---|---|---|
| 豆包物理遥控器语音 | RC003 Helper 注入 WUDFHost，把 `0x003E` 槽替换为 `0x00E6` | HIDMaestro 虚拟 USB 键盘发送右 Alt |
| 豆包手机语音 | #443 的 `PhoneVoiceAccess.virtual_hid = None`，直接拒绝；当前 `main` 尚无该接口 | HIDMaestro `VirtualHidTrigger`，press/release |
| 返回/音量± 捕获 | Helper Hook `0x80018483` 读取报告并上报 | BthPort ETW Event 402 读取 `0x0064` 报告 |
| 三键清键 | Helper 在报告层清零 | **不需要**；三键在 `kbdhid` 已丢弃，无原生动作 |
| 普通映射键抑制 | `key_gate` + Helper 报告层清键 | Raw Input + `key_gate`；Helper 退役前必须通过普通键新旧对照；未通过则保留 legacy 仅负责普通键 |
| 语音键 F5 | Helper 清键/合成 + key_suppressor | HIDMaestro + 独立 F5 guard/key gate |
| 设置 | `rc003_capture_enabled` 同时控制全按键和“支持更多输入工具” | 拆成用户意图、授权状态、就绪状态三组；旧开关只迁移意图，不自动授权 |
| 权限 | 提权计划任务 + UAC + Frida 注入 | 普通权限主程序 + 提权 HIDMaestro broker；ETW 管理员配置；无注入 |
| 维护 | 豆包/Windows/Helper/Gadget 版本耦合 | 豆包版本解耦；ETW 受 Windows/蓝牙栈影响 |

### 3.1 语音快捷键的升级语义

不能把所有非豆包快捷键都无条件改成 `SendInput`。当前主线在增强捕获开启时，单键左 Alt 和右 Alt 都可以走报告层合成；「其他工具 + 左 Alt」是界面可保存配置。新路径按**快捷键形状**而不是只按工具名选择：

| 用户配置 | 新路径 | 失败/边界 |
|---|---|---|
| 豆包 + 右 Alt（默认） | HIDMaestro broker 发设备级右 Alt | broker 不可用 → 豆包明确不可用，不静默改 SendInput |
| 豆包 + 左 Alt | HIDMaestro broker 发设备级左 Alt（需真机确认豆包接受） | 不接受则 UI 限制为右 Alt；不能假装可用 |
| 微信 + 组合键（如 Ctrl+Win） | 保持 `SendInput` 和弦 | 和弦无法用单键 HIDMaestro 表达；沿用现有路径 |
| 其他工具 + 左/右 Alt 单键 | HIDMaestro broker 发设备级 Alt | broker 不可用时才回落 `SendInput`，并在诊断中标记降级；只接受设备键的工具可能失效 |
| 其他工具 + 和弦 | `SendInput` | 与当前一致；不承诺设备级 |
| 不按键 | 不注入任何键 | 音频/会话仍按现有状态机；F5 不得漏入前台 |

补充规则：

- 物理语音键仍会以 F5 usage 上报；F5 guard/key gate 必须独立于 HIDMaestro 生效，HIDMaestro 本身不发送 F5。
- 物理释放立即结束本轮；不引入“最短 hold”作为验收判据。
- 每次语音会话有 epoch；旧会话迟到的 release 不得松开新一轮的键。快速按放、按住时断连、手机/遥控器交替触发都要覆盖。
- 手机语音依赖仍开放的 #443（`PhoneVoiceAccess.virtual_hid` / `phone_voice.rs`）；当前 `main` 没有这些接口。物理遥控器可先独立验证，手机路径在 #443 合并后接入。

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
- 这是机器级敏感 BLE 日志开关，会影响所有经过该适配器的 BLE 流量；必须在用户明确同意后写，不能由旧 `rc003_capture_enabled` 直接推导授权
- 写入前记录原值/写入值/时间；关闭或卸载时只在当前值仍等于 SayAll 写入值、且未被其他软件改过时恢复；被改过则不覆盖并提示
- 如果注册表值由其他程序预置，SayAll 只读取/使用，不声明所有权、不在卸载时删除

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
- 最低系统定为 Windows 10 1903（18362），与 usbip-win2 的 1903+ 要求一致；Windows 10 1809 不再进入该架构
- Windows 10 19041（2004/20H2）满足版本下限，但需单独真机验证

### 5.4 BLE/GATT 绑定

- 需要保持 BLE 连接和 GATT 服务发现
- 通过带唯一 UUID 的 GATT 查询关联 ACL 句柄
- **必须确认事件属于当前选中的设备**：地址 + ACL 句柄 + 连接代号三者一致才派发；只有一台已知遥控器在线不构成身份证明
- 没有出站探针、无法确认归属时，不派发三键；显示“未绑定/不可用”，不要猜
- 多只同型号遥控器、断连重连、冷启动首次按键都可能丢失绑定；绑定失败必须 fail-closed
- 必须排除 ATVV 音频/控制特征，避免把音频帧误解为按键
- 两条同型号设备必须按地址/句柄隔离；切换选中设备时清空旧绑定和未完成按键状态

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
- DOWN 已派发但 UP 丢失时，仅把状态改成“不可用”不够；已开始的长按动作必须收尾
- 关闭全按键、ETW 停止、睡眠、断连、切换设备、会话结束、应用退出时，强制释放所有 active 三键并取消未完成的长按/补点按
- 新连接建立前先清理旧连接 epoch 的未完成状态；旧连接迟到的 UP 不得释放新连接或触发新动作
- watchdog 依据连接健康/会话结束，不使用会破坏长按的固定短超时

### 5.8 ETW 派发门禁与释放

三键边沿进入 `button_mapping` 前必须通过以下门禁：

1. 当前功能开关为开；
2. 事件地址/ACL 句柄/连接代号匹配当前选中的遥控器；
3. 事件不是重复、截断或 ATVV 特征；
4. 该键当前没有由另一来源（旧 Helper）持有，保证一次按压只有一个派发者。

释放规则：

- UP 必须与 DOWN 同 epoch、同设备、同 usage；不匹配则忽略并记录。
- 任一失效条件（关开关、ETW 会话停止、睡眠、断连、切换设备、客户端退出）都触发 `release_all`，长按/方向保持立即收尾。
- 重连时先为旧 epoch 的 active 键合成释放，再接受新 epoch 的 DOWN。
- 不把“单设备候选”作为兜底派发条件；宁可三键暂不可用，也不把别的 BLE 设备事件映射成遥控器按键。

### 5.9 普通键退役门禁（评审 P1）

当前增强路径由 Helper 在报告层清掉已映射的普通键；低级键盘钩子和 Raw Input 都拿不到足够可靠的“物理来源”身份来只吞遥控器。`key_gate` 用时间窗近似，已知会出现：

- 闲置后的第一个方向键或 Enter 先送到前台；
- 遥控器活动后约 4 秒内，物理键盘的同名键可能被误吞；
- 把遥控器方向键映射成另一个动作时，用户可能同时看到光标移动和映射动作。

这些行为在普通权限的记事本里就可能出现。因此**删除普通键 Helper 必须单独过门禁**，不能随语音/三键一起删：

1. 方向键映射成不同动作后，闲置首按只执行映射动作，前台不先收到方向/Enter。
2. 紧接着按物理键盘同名键，不被吞、不触发遥控器动作。
3. 映射 Home/TV 后，物理键盘对应键仍正常。
4. 标准用户 + 普通窗口 + 管理员前台窗口分别记录结果；UIPI 边界单独判定。
5. 旧版（Helper 清键）与新版（Raw Input + key_gate）逐项对照，记录差异。

任一项未通过：保留旧 Helper 仅负责普通键（或该型号），语音/三键仍可独立迁移。通过后才允许把普通键退役列入清理阶段。

保留 legacy 时必须做**能力隔离**：启动握手关闭 `voice_synth` 和三键转发，只保留普通键 clear targets；日志/诊断显示 owner。不能让同一个 Helper 既作为普通键 fallback，又继续合成语音或转发三键。

## 6. HIDMaestro 依赖与运行权限

- HIDMaestro 2.3.x MIT；usbip-win2 0.9.8.1 BSD-2-Clause
- HIDMaestro 是 .NET/C# SDK；需要 sidecar/broker 或 NativeAOT 进程与 Rust/Tauri 通信
- 首次安装 USB/IP 主机控制器需要管理员/UAC；可能重启
- usbip-win2 的目标版本是 **Windows 10 1903+ x64、Windows 11 ARM64**；Windows 10 19041 x64 满足版本下限，仍需单独验证
- Windows S Mode、Secure Boot、企业驱动策略可能拒绝
- 驱动是共享的；其他 HIDMaestro 程序可能同时存在，不能随意卸载
- HIDMaestro 输出的是虚拟设备，反作弊/游戏可能检测；产品需说明
- 需要引入 MIT/BSD 许可证与第三方通知
- 不能复制 Remote Buddy 的 GPL-3.0 代码；只参考其公开行为

### 6.1 运行时权限：HIDMaestro broker（评审 P1）

**关键事实**：HIDMaestro 2.3.0 的 `CreateController` 本身要求管理员，即使 USB/IP 驱动已经安装；USB/IP 分支创建键盘时还会取得进程所有权。因此“安装时提权一次”不够。主程序必须在普通权限下运行，不能把整套 SayAll 用管理员启动来规避。

推荐设计：

- 新增独立 **`sayall-hid-broker.exe`**（.NET self-contained/NativeAOT），由安装器注册为 Windows 服务或开机计划任务，以 `LocalSystem`/最高权限运行；它创建并持有 HIDMaestro 虚拟键盘。
- SayAll 主程序保持普通权限，通过受限 named pipe 请求 broker 执行**语音键 press/release**；broker 只接受白名单内的左/右 Alt 和预定义释放，不接受任意键注入、不开放网络。
- pipe ACL 只允许当前交互用户/会话；broker 校验客户端 PID、会话、可执行文件路径/签名，并拒绝跨会话/远程连接；日志不记录 BLE 或键盘载荷。
- broker 开机/登录后可用，首次按住不需要再弹 UAC；如果服务缺失、被禁用或驱动未就绪，设置页提供“修复语音组件”，必要时提权安装，但不阻塞基础音频/微信路径。
- 客户端退出、崩溃、注销、锁屏、睡眠、broker 停止时释放所有键；broker 自身 watchdog 防止粘键。
- 安全边界：这是高权限输入注入面，必须做威胁建模和代码审查；如果 Phase 0 证明 Session 0 服务无法创建可见虚拟键盘，则退回“每次启动 UAC broker”，并明确标准用户无法免 UAC，不能虚假承诺。

**验收场景（必须先于全面接入）**：

1. 标准用户登录，SayAll 主程序为普通权限，闲置后第一次按住语音键即出字。
2. 注销/重启后不需要管理员启动主程序。
3. 服务被禁用、UAC 拒绝、broker 崩溃：豆包明确不可用；基础音频、微信/其他工具路径不受影响；不出现静默无反应。
4. 客户端崩溃/注销/睡眠：虚拟键盘不粘键；下一轮新连接不受旧 release 影响。

### 6.2 安装、共享与卸载

- 安装器负责驱动/服务/许可；安装失败或用户取消时保留基础功能，并明确豆包语音不可用。
- 驱动可能由其他 HIDMaestro 程序预装或共用：卸载只清理 **SayAll 创建的虚拟键盘、broker、ETW 会话、计划任务**；不无条件删除共享驱动。
- 只有在确认驱动由 SayAll 独占安装且用户明确选择时才提供驱动卸载；否则保留并说明。
- 升级要处理旧版 Helper 任务、Frida 资产、旧授权记录；不能让旧 Helper 和新 broker 同时派发同一个语音键。

### 6.3 系统支持范围与 1809 老用户

- 当前 `TECHNICAL.md` 声明的产品下限是 Windows 10 1809（17763）；那是**旧实现**的范围，不是新架构范围。
- 新架构的安装包/服务/驱动最低为 **Windows 10 1903（18362）x64**；19041 x64 满足版本下限，但豆包与 ETW 仍需各自真机验证。
- usbip-win2 的目标版本是 Windows 10 1903+ x64、Windows 11 ARM64；Windows 10 ARM64 不进入新路径承诺。
- 计划采用：1809 用户停留在最后一个兼容版本，不自动跨过 1903 升级；基础功能在该版本继续可用。最终发布/维护线策略见 §12-7。
- 如果产品要同时维护 1809 基础功能线，需要单独排期，不能把“新架构 1903”当成整套产品的下限变更。

## 7. 受影响模块

### 7.1 公开 Windows 宿主

- `crates/sayall-windows/src/lib.rs`
  - `WindowsPlatform` 组装
  - `set_enhanced_capture_enabled`
  - `set_voice_hold_hotkey`
  - `PhoneVoiceAccess` 的 virtual HID
- `crates/sayall-windows/src/ble.rs`
  - 语音键路径、`voice_synth` 门禁、SendInput fallback
- `crates/sayall-windows/src/phone_voice.rs`（#443 分支；当前 `main` 尚未合并）
  - `VirtualHidTrigger` 生产实现；依赖 #443 合并后的接口
  - Doubao 不再因 `virtual_hid.is_none()` 直接失败
- `crates/sayall-windows/src/rc003_bridge.rs`
  - 目标：按型号/功能降级为 legacy fallback；不无条件删除
- `crates/sayall-windows/src/key_gate.rs`
- `crates/sayall-windows/src/key_suppressor.rs`
- 新增 `crates/sayall-windows/src/bthport_etw.rs`（绑定、epoch、release 门禁）
- 新增 `crates/sayall-windows/src/hidmaestro_broker_client.rs`
- 新增 `src/bin/sayall-hid-broker`（或独立 .NET 工程）：高权限 HIDMaestro broker、named pipe、watchdog、只允许语音键

### 7.2 私有 Windows 运行时

- `packages/windows-remote/host/hook.rs`
  - `phone_voice::press(..., voice.virtual_hid.clone(), ...)`；依赖 #443 的接口
- `packages/windows-remote/src/*`
  - 不改变协议；voice error code 仍保持稳定
- 语音工具选择与设置持久化
- 手机路径不得在 #443 合并前作为当前 `main` 的已完成能力；物理遥控器验证可独立进行

### 7.3 Tauri / 前端

- `src-tauri/src/platform.rs`
  - `set_enhanced_capture_enabled` 拆分
  - 新增 HIDMaestro/ETW 状态命令
- `src-tauri/src/lib.rs`
  - `enable_rc003_capture` / `disable_rc003_capture` 语义重命名
  - 新增驱动/ETW 配置命令
  - 设置三组状态：用户意图（想用）、授权（同意写注册表/装 broker）、就绪（组件实际可用）
  - 旧 `rc003_capture_enabled` 只迁移意图；写 HKLM/装驱动前必须二次确认
- `src/lib/bridge.ts`
- `src/pages/ConnectionPage.vue`
- `src/pages/ButtonsPage.vue`
- `src/components/MobileRemoteSection.vue`
- `src/onboarding/*`（第④步全按键、豆包步骤）
- `src/lib/i18n/*`

### 7.4 安装包与清理

- `src-tauri/tauri.conf.json`
- 构建脚本、staged resource、NSIS installer
- 条件移除 `hardware/RC003/helper`、`frida-gadget*.dll`、`rc003_agent.js`；未通过门禁的型号/普通键路径保留
- 移除对应计划任务和运行时目录；清理前确认没有老版本进程占用
- 新增 HIDMaestro broker、usbip-win2 驱动资源、许可证；注册/卸载服务或计划任务
- 记录 SayAll 对 HKLM 值、驱动、虚拟键盘、ETW 会话的所有权；共享驱动不自动卸载
- 迁移旧版 Helper 授权/残留；旧开关只迁移意图，不迁移授权

## 8. 实施阶段（评审后拆分）

### Phase 0：可行性 spike（4–6 天）

- HIDMaestro 运行权限 spike：
  - 用 Windows 服务/开机任务以 SYSTEM 创建 HIDMaestro 虚拟键盘
  - 主程序保持普通权限，通过 named pipe 请求 press/release
  - 标准用户登录 + 闲置后第一次按住出字
  - 验证崩溃/注销/睡眠释放、服务禁用、UAC 拒绝
  - 如果 Session 0 无法创建可见虚拟键盘，记录退回“每次 UAC broker”的后果，标准用户免 UAC 目标不能宣称
- ETW spike：
  - 配置 BthPort 注册表；捕获 RC003 返回/音量± 的 8 字节报告
  - 验证地址 + ACL 句柄 + epoch 绑定；未绑定/歧义时不派发
  - 验证 DOWN 后 UP 丢失、关开关、睡眠/重连的释放
  - 验证不清键时无原生动作
- 普通键对照 spike：
  - 旧 Helper 清键 vs Raw Input + `key_gate`
  - 闲置首按方向/Enter、物理键盘同名键、Home/TV、记事本
- 产出：Windows 10 19041/22H2、Windows 11 真机结果；RC003 结果；RC001 结果（至少判定是否可继续）；标准用户运行结论；普通键回归清单。

**退出条件**：标准用户在普通权限主程序下能完成豆包语音首按；ETW 三键能绑定且释放可收尾；普通键差异已量化。否则不进入全面接入。

### Phase 1：HIDMaestro 语音（7–10 天）

- 实现 `sayall-hid-broker` + 安装/服务/pipe 安全
- Rust 侧 `VirtualHidTrigger` 实现；主程序普通权限
- 物理遥控器先接豆包；手机语音在 #443 合并后再接
- 按 §3.1 快捷键矩阵实现：
  - 豆包单 Alt → HIDMaestro
  - 其他工具单 Alt → HIDMaestro，broker 不可用时才降级 SendInput
  - 微信/和弦 → 保持 SendInput
  - 无按键 → 不注入
- 停用 `voice_synth` 作为豆包主路径；保留旧 Helper 时保证互斥
- F5/语音键 guard；epoch 释放规则
- 诊断：broker 状态、服务状态、UAC 取消、驱动失败、豆包未安装
- 退出条件：标准用户豆包物理语音可用；微信/其他工具回归通过；手机路径标记 #443 依赖

### Phase 2：ETW 三键捕获（7–10 天）

- 新增 BthPort ETW 模块
- BthPort 注册表配置检测/授权/提示（不自动写，用户确认后提权）
- ETW 会话启动、清理、重连
- GATT UUID 探针 + ACL 句柄 + 连接 epoch 绑定
- `0x0064` 8 字节报告解析
- ETW 边沿 → `ButtonMappingRuntime` 的 `GateEdge`
- §5.8 的派发门禁和 `release_all`
- 全按键开关只控制 ETW，不再控制豆包
- 不做报告层清键
- 退出条件：RC003 三键可映射、可高亮；未绑定不派发；关开关/断连/睡眠能释放；不误触物理键盘

### Phase 3：设置、迁移与升级门禁（6–8 天）

- 拆分设置：
  - 用户意图：`full_key_capture_requested`、`voice_compat_requested`
  - 授权：`etw_config_authorized`、`hid_broker_authorized`
  - 就绪：`etw_ready`、`broker_ready`
- 移除“支持更多输入工具 = 豆包必须开启”的文案
- 向导：
  - 全按键步骤说明 ETW/UAC/重启
  - 豆包步骤说明安装语音兼容组件
- 诊断：
  - HIDMaestro 驱动/服务/pipe 状态
  - ETW 配置、会话、绑定设备、载荷截断
  - 当前每个键的派发 owner（legacy/etw/none）
- 迁移旧用户：
  - 保留旧映射/设置
  - 旧“全按键”开关只迁移意图，不自动写 HKLM 或装 broker
  - 旧“支持更多输入工具”不再强制豆包
- **升级门禁（必须在清理前完成）**：
  - RC003：从已开启全按键、已有三键映射的旧版升级
  - RC001：同样的旧版升级；新路径未通过则保留 RC001 legacy
  - 普通键：按 §5.9 完成新旧对照
  - 每个键只能有一个派发者；旧 Helper 与 ETW 不得同时处理同一次按压
- 退出条件：RC003/RC001 都有 passed 或明确的 legacy fallback；普通键门禁有结论；无双派发。

### Phase 4：条件退役旧路径（2–4 天）

- 只删除已通过门禁的型号/功能的 Frida/Helper 路径；未通过部分保留
- 删除对应 `hardware/RC003/helper` 资产、Gadget、agent、计划任务
- `rc003_bridge.rs` 降级为按型号 legacy，而非全局删除
- 删除已迁移功能的 `set_capture_targets` / `voice_synth` 生产依赖
- 更新 ATTRIBUTION、LOGGING、DEBUGGING、产品文案
- 卸载清理：计划任务、ProgramData、旧 Gadget 映射（需重启）
- 共享 usbip/HIDMaestro 驱动不自动删除；只清理 SayAll 创建的虚拟键盘/会话/任务
- 退出条件：已退役范围内无 helper 进程、无计划任务、无 Frida 文件；未退役范围有明确 owner 和用户可见状态

### Phase 5：验收与灰度（6–9 天）

- 按第 9 节矩阵验收
- 灰度：
  - 先内部 RC003 + Windows 11
  - 再 RC003 Windows 10
  - 再 RC001；未通过则保持 legacy
  - Windows 11 ARM64 作为独立门禁；Windows 10 ARM64 不承诺新路径
- 发布边界（拟采用，最终策略见 §12-7）：
  - 新架构安装包最低 Windows 10 1903 x64
  - 1809 用户停留在最后兼容版本，更新器不得跨最低版本推送
  - 基础功能、旧版回退和共享驱动策略在发布说明中写清
- 失败回退：
  - 语音 broker 不可用 → 明确错误；微信/其他工具路径按配置继续
  - ETW 不可用 → 三键禁用；普通键按已通过的门禁路径处理
  - 旧 Helper 只在对应型号/功能的 legacy 范围内保留；不保留 Frida 默认回退

## 9. 验证矩阵

### 9.1 自动化

- broker IPC：只接受白名单语音键、同用户/会话、拒绝远程/错误签名、客户端崩溃释放、服务停止/重启
- broker 权限：标准用户主程序、服务禁用、UAC 拒绝、pipe 超时
- 语音矩阵：豆包左/右 Alt、其他工具左/右 Alt、微信和弦、无按键、F5 guard
- 语音释放：快速按放、旧 release 迟到、按住时断连、手机/遥控器交替
- ETW 8 字节报告解析：F1/80/81、按下/松开、重复、空槽
- HCI ACL/L2CAP/ATT 重组与截断
- GATT 探针 → ACL 句柄 → 地址 + epoch 绑定
- 多设备同型号隔离；未绑定/歧义不派发
- ETW 会话启动/停止/残留清理；DOWN 无 UP、关开关、睡眠/重连的 `release_all`
- 旧 epoch 迟到事件不触发新连接
- 设置：旧 `rc003_capture_enabled` 只迁移意图；授权/就绪分离
- HKLM 写入记录/恢复；值被第三方修改时不覆盖
- 派发 owner：同一键不会同时由 legacy 和 ETW 处理
- 手机语音：`virtual_hid` 可用/不可用/错误码；#443 未合并不做通过结论
- 构建：已退役范围不含 Gadget/agent/计划任务；未退役范围保留并标记 legacy

### 9.2 真机

| 维度 | 覆盖 |
|---|---|
| 系统 | Windows 10 1903/19041/22H2 x64、Windows 11 23H2/24H2 x64；Windows 11 ARM64 独立门禁；Windows 10 ARM64 不承诺新路径 |
| 遥控器 | RC003、RC001；两只同型号同时在线；切换选中设备 |
| 豆包 | 标准用户闲置首按、物理遥控器、手机语音（#443 后）、冷启动、快速连按、连续会话、断连重连 |
| 快捷键 | 豆包左/右 Alt、其他工具左/右 Alt、微信和弦、不按键 |
| 微信 | 回归 SendInput 路径 |
| 全按键 | 返回/音量+/音量−、所有 13 键映射、按下高亮、关闭开关 |
| 普通键 | 旧 Helper vs 新路径：闲置首按方向/Enter、物理键盘同名键、Home/TV、记事本、管理员前台 |
| 升级 | RC003/RC001 从开启全按键、已有映射的旧版升级；无丢失、无双派发、失败有 legacy |
| 无原生副作用 | 三键无系统动作；普通键按已通过门禁；管理员前台 UIPI 边界单独记录 |
| 驱动/权限 | 首次安装、服务注册、标准用户、UAC 取消、服务禁用、其他 HIDMaestro 程序共存 |
| ETW | 未配置、配置后重启、载荷截断、无出站探针、未绑定/歧义、会话残留、睡眠唤醒、DOWN 无 UP |
| 清理 | 已退役范围无 helper/gadget/任务/ETW 会话/SayAll 虚拟键盘；共享驱动按所有权保留 |

### 9.3 通过判据

- 标准用户 + 普通权限主程序，闲置后第一次按住豆包语音键即出现语音条并出字
- 语音快捷键矩阵逐项通过；豆包失败不静默降级到 SendInput
- 三键映射生效且无原生动作；未绑定/歧义时不误派发
- 关闭全按键、断连、睡眠、ETW 停止时，长按/保持立即收尾
- RC003 和 RC001 的旧版升级路径分别通过，或明确保留该型号 legacy
- 普通键新旧对照通过；未通过则旧 Helper 保留，不宣称退役
- 每个键只有一个派发 owner；无旧 Helper 与 ETW 双派发
- 卸载只清理 SayAll 创建项，共享驱动/其他程序不受影响
- 失败时用户看到明确原因，不出现静默无反应

## 10. 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| ETW 载荷截断/脱敏 | 三键不可用 | 检测并提示配置/重启；记录 `report=redacted`；ETW 不可用时禁用三键 |
| ETW 需管理员/重启 | 普通用户门槛高 | 首次配置向导一次完成；解释原因；提供回退 |
| HIDMaestro 创建要求管理员 | 标准用户无法直接创建虚拟键盘 | 独立 broker 服务/最高权限任务；主程序普通权限；Phase 0 验证标准用户首按 |
| broker 高权限输入面 | 权限提升/滥用风险 | 仅白名单语音键、同用户/会话 ACL、客户端校验、无网络、审计、最小权限 |
| 普通键只有时间窗抑制 | 首按泄漏、物理同名键被吞 | §5.9 新旧对照；未通过保留 legacy；不随语音/三键一起删 |
| RC001/ARM64 未验证 | 兼容性回归 | 分型号门禁；未验证型号保留 legacy 或禁用承诺 |
| Windows 10 1809/ARM64 不满足新路径 | 旧系统/架构豆包不可用 | 已定最低 1903 x64；1809 停留最后兼容版本；Win10 ARM64 不承诺；Win11 ARM64 单独验 |
| HIDMaestro 驱动安装失败/杀软 | 豆包不可用 | 明确状态；WeChat/其他工具不受影响；提供诊断 |
| 虚拟键盘粘键 | 用户按键卡住 | 客户端崩溃/注销/睡眠释放、epoch 隔离、broker watchdog；不以“最短 hold”作为判据 |
| ETW 只读，普通键原生透传 | 双动作 | `key_gate`；管理员 UIPI 边界文档化；未通过门禁保留 legacy |
| ETW 会话冲突/残留 | 采集失败 | 唯一会话名、启动清理、崩溃恢复 |
| HIDMaestro .NET broker 体积 | 安装包/启动 | NativeAOT/self-contained；延迟启动 |
| Remote Buddy GPL | 许可证污染 | 只参考行为；使用 MIT HIDMaestro/BSD usbip；不复制 GPL 代码 |
| HKLM 敏感日志被误改 | 隐私/其他 BLE 设备数据 | 明确同意；记录原值；只在未被第三方修改时恢复；共享值不声明所有权 |
| 迁移残留/双派发 | 三键或语音动作重复 | 按型号/功能 owner；升级门禁；旧 Helper 与新路径互斥；卸载按所有权 |
| 手机语音依赖 #443 | 当前 main 无接口 | 物理遥控器先行；#443 合并后再接手机；不提前宣称通过 |

## 11. 工作量评估

| 工作项 | 工程师日 |
|---|---:|
| Phase 0 spike（含标准用户 broker） | 4–6 |
| HIDMaestro broker/服务/pipe/驱动接入 | 7–10 |
| ETW 捕获/绑定/epoch/释放 | 7–10 |
| 设置/UI/授权/迁移 | 6–8 |
| 普通键/RC001 升级门禁 | 4–6 |
| 条件退役 Helper/Frida/打包清理 | 2–3 |
| 真机矩阵/灰度/发布 | 6–9 |
| **合计** | **36–52 工程师日** |

- **单人顺序开发：约 9–12 周**
- **两人并行（ETW 与 HIDMaestro 分开）：约 6–8 周**
- 评审后增加的主要成本：标准用户 broker、普通键对照、RC001/RC003 升级门禁、共享驱动清理
- 不含 RC001/ARM64 深度补测的额外时间
- 若只做“豆包语音 HIDMaestro、保留全按键 Helper”：约 2–3 周（含 broker）
- 若只做“ETW 三键、保留豆包 Helper”：约 2 周

## 12. 需要产品决策的问题

1. 是否接受 HIDMaestro + usbip-win2 的**签名驱动安装**，以及为普通用户提供**高权限 broker 服务/任务**？如果 Phase 0 只能每次 UAC，是否接受标准用户不能免 UAC？
2. 是否接受 ETW 的**管理员/注册表/重启**成本？
3. RC001 未验证时：禁用三键、保留该型号旧 Helper，还是标 beta？
4. 是否接受 ETW 只读导致的管理员前台原生键边界？
5. 普通键门禁未通过时，是否允许旧 Helper 只保留普通键路径（语音/三键已迁移）？
6. HIDMaestro 以 broker sidecar 引入还是自研 UMDF（前者快、后者可控但更贵）？
7. Windows 10 1809 的更新策略：停留最后兼容版本，还是提供独立基础功能维护线？
8. Windows 11 ARM64 是否进入首个发布门禁，还是先标 deferred？

已定（2026-11-28）：最低 Windows 版本为 **10 1903（18362）**；19041 受支持；1809 不再进入该架构。

评审建议的执行顺序：**先独立验证豆包语音（含标准用户 broker）→ 再独立验证 ETW 三键 → 最后按型号/普通键门禁决定旧 Helper 退役**。任一项未通过时，不把“全面替换”作为发布条件。

## 13. 完成定义

- 标准用户 + 普通权限主程序，闲置后第一次按住豆包语音键即出字；broker 权限路径有验收证据。
- 豆包物理语音不再依赖 Frida/RC003 Helper；手机语音依赖 #443 合并后验收。
- RC003 三键由 ETW 捕获；未绑定/歧义不派发；无报告层清键；不清也不产生原生动作。
- 普通键按 §5.9 门禁通过后才退役 Helper；未通过则保留 legacy 并明确 owner。
- RC001/RC003 分别完成旧版升级门禁；未通过型号保留 legacy，不出现“升级后失去三键”。
- 设置拆分为用户意图/授权/就绪；豆包不再要求全按键；旧开关不自动授权 HKLM/驱动。
- 安装包包含 HIDMaestro/broker/驱动/许可；卸载清理 SayAll 创建的虚拟键盘、ETW 会话、任务；共享驱动按所有权保留。
- Windows 11 ARM64 有明确 passed/deferred 结论；Windows 10 ARM64 不承诺新路径。
- 失败路径对用户可见：broker/服务失败、UAC 取消、ETW 截断、未绑定设备、共享驱动状态都有明确提示。

## 14. 参考

- Remote Buddy:
  - https://github.com/zfdang/remote-buddy-windows
  - `docs/windows-remote-input.md`
  - `docs/windows-ble-input-layers.md`
  - `docs/xiaomi-rc003-buttons.md`
  - `src/RemoteBuddy.Platform.Windows/Input/BthPortRegistryHelper.cs`
  - `src/RemoteBuddy.Platform.Windows/Input/DoubaoUsbKeyboard.cs`
- HIDMaestro: https://github.com/hifihedgehog/HIDMaestro
  - `sdk/HIDMaestro.Core/HMContext.cs` 的 `CreateController` / USB/IP 创建路径（v2.3.0）
- usbip-win2: https://github.com/vadimgrn/usbip-win2
  - `README.md#requirements`（Windows 10 1903+ x64、Windows 11 ARM64）
- PR #443（手机语音 `PhoneVoiceAccess` / `phone_voice.rs` 依赖）
- 当前实现：
  - `crates/sayall-windows/src/rc003_bridge.rs`
  - `crates/sayall-windows/src/lib.rs`
  - `crates/sayall-windows/src/ble.rs`
  - `crates/sayall-windows/src/key_gate.rs`
  - `crates/sayall-windows/src/remote_target.rs`
  - `crates/sayall-windows/src/button_mapping.rs`
  - `TECHNICAL.md`（当前支持范围仍写 1809）
  - `Testing/WindowsRC003EnhancedCapture.md`
  - `Testing/WindowsRC001ThreeKeyMapping.md`
  - `docs/installation-and-configuration.md`
- 评审依据：
  - `Bugs/2026-11-25-onboarding-doubao-voice-bar-report-layer-inactive.md`
  - `Bugs/2026-11-18-fullkey-shared-host-keyboard-same-usage.md`
  - `docs/investigations/2026-11-25-rc001-rc003-key-path-evidence.md`
