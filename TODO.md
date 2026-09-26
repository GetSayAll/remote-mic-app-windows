# TODO

## v1 决策（2026-09-04）

- 第一版只做微信输入法听写：按住说话快捷键默认 左Ctrl+左Win（适配微信输入法默认语音热键）；语音可用 = 按住语音键 → 注入快捷键 → ATVV 音频经 CABLE Input → 微信输入法麦克风（CABLE Output）→ 云端识别 → 文字上屏。端到端链路音频段已本机实证（调查报告 evidence/n）；注入段配方约束已本机实证（evidence/p：WeType 拒绝单批零间隔和弦，须逐事件注入；间隔 20/40/60ms 均 4/4 触发、零间隔 0/2，~~默认取 20ms 压低按键延迟~~（2026-09-05 更正：20ms 为热态验证结论，冷/节流态必失败已实证并回退 80ms，见下方"性能已知项"与提交 86e5314——延迟优化必须以成功率保证为前提），Bugs\2026-09-04-wetype-zero-gap-injection.md）；**延迟账目已量化（2026-09-05，evidence/p 端点预热对照实验）**：注入→WeType 开麦固定 ~163ms（冷/热端点中位数差 0.3ms，预热无效；WeType 内部处理，第三方边界不可干预），两型号实际均直接 0x04 推流（无开麦往返可并行），0x04 早于 HID F5 60-90ms（触发点已最早），全链 ≈215-245ms 其中应用侧仅和弦 20ms 可控——**应用侧延迟优化到此收敛**；RC001 遥控器端到端真机 passed（2026-09-04，用户确认文字上屏；用户侧前提=输出端点选 CABLE Input + 系统默认录音设备切 CABLE Output，应用不修改系统默认设备）；RC003 真机待验。豆包（注入判死四层闭环）与 WinUHid 增强轨延后，见 ADR 0002 与 docs/investigations/2026-09-04-avoid-driver-signing-input-paths-final.md。

## 后续产品功能（开发前对照 Mac App）

以下功能均以 Mac App 的产品流程、页面结构、状态反馈、文案语义和异常处理为实现参考；开发前先完成对照调研并记录结论。Windows 侧仍须遵守本仓库架构边界，只使用公开 API、公开协议、全局快捷键和用户可见的辅助功能界面；因平台能力产生的差异须在 `ATTRIBUTION.md`、路线图或对应调查文档中说明，不直接移植 macOS 平台代码。

- [ ] 增加首次使用 Onboarding 流程页面：参考 Mac App 的步骤顺序和完成条件，覆盖遥控器连接、语音输出设备、目标输入法选择、按住说话验证与失败恢复；支持中断后继续、完成后重新进入，并为关键状态、分支和外部调用补齐结构化日志。
- [ ] 支持 Typeless：参考 Mac App 的选择、配置、触发、状态反馈和恢复流程，调研 Windows 公开能力后实现按住说话生命周期、音频路由与失败关闭；分别完成 RC001/RC003、冷态首用、快速连按、断连和睡眠恢复真机验收。
- [ ] 支持豆包输入法：参考 Mac App 的产品行为与配置引导，在不读取或修改豆包私有配置、内部数据库、内存或私有协议的前提下设计 Windows 支持路径；基础能力不得依赖进程注入，若必须使用提权 Helper 或虚拟 HID，须保持独立、显式启用且不影响现有语音主路径，并分别完成 RC001/RC003 真机验收。
- [ ] 完善聚焦输入框处理：参考 Mac App 对目标输入框的识别、焦点保持、恢复和无可编辑目标时的用户提示；Windows 仅使用公开的焦点与辅助功能 API，避免静默把语音结果送入错误窗口，并覆盖焦点切换、窗口关闭、应用切换、Onboarding/设置窗口前后台切换及语音会话中焦点变化。

## Windows RC001 / RC003

- [x] 参考 macOS `SMAppService.mainApp` 实现 Windows 当前用户登录自启动：关于页可开关，使用 `HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run`，启动时同步并记录结构化日志；不需要管理员权限。**2026-09-15 本机 Windows 真机 passed**：release 安装版 0.2.6 注销重登后自动启动，进程父进程为 `explorer`（由登录 shell 拉起，非手动启动），启动链 `document_load finished → vue_mount(80ms) → initial_ipc_ready(337ms)` 完整，日志 `startup feature=launch_at_login action=sync terminal_result=passed enabled=true`。验证要点：`Win+L` 锁屏再解锁**不会**触发 `Run` 项（用户会话未结束），必须注销（`shutdown /l`）或重启才能验证。

- [ ] 鼠标动作映射：支持左/右/中键单击、左键双击、每次 1–100 格滚轮及每次 1–2000 物理像素指针移动；不修改默认绑定。RC001/RC003 实体按键及闲置首按回归仍需分别验收，见 `Testing/WindowsMouseActions.md`。
- [ ] Windows 注册应用扫描与应用库：支持搜索、多选/全选、配置保存及导入导出；扫描或添加应用不会自动启动或绑定。RC001/RC003 实体按键回归仍需分别验收，见 `Testing/WindowsRegisteredApps.md`。
- [ ] 遥控器电量显示：按所选 BLE 对端读取 Windows 缓存电量，断连、睡眠或缺失时显示未知，不额外进行 GATT 操作。持续更新、断连、睡眠及 RC001/RC003 实机验收见 `Testing/WindowsBattery.md`。
- [ ] 关于页"启动行为"与"软件更新"之间补 12px 分组间距：与上方应用标识/外观卡的堆叠节奏一致，两组独立设置不再读成同一张卡的两段。前端类型检查与 AboutPage 仿真测试 passed；纯 CSS 间距无需真机专项。
- [ ] 主窗口 Ctrl+W 关闭快捷键：Ctrl+W 隐藏主窗口并由托盘驻留，语义与点标题栏"X"完全一致（不动 BLE/语音链路、不退出进程，真正退出仍走托盘菜单"退出"）；带 Alt/Shift/Meta 的组合与按住连发均不触发，Rust 侧落 `window_close source=ctrl_w` 结构化日志。不复用前端 `getCurrentWindow().close()`——它在 Windows 上是否触发 `CloseRequested`（→ 隐藏到托盘）取决于 tao 的平台实现，跨版本可能静默改变语义，故显式走 IPC 调 `window.hide()`。自动化验证 passed（前端 vitest 95 例含 Ctrl+W 边沿、`pnpm build`、`cargo check` 含 runtime-simulation、`cargo fmt --check`、`cargo test` 单线程全量）；**Windows 真机 Ctrl+W 按键与托盘恢复 deferred**（worktree git 元数据异常，本次未能出本地包）。

- [x] 建立独立 Rust + Tauri 2 + Vue 3 工程结构。
- [x] 建立 Mac 原版风格设置界面骨架。
- [x] 建立 ATVV、ADPCM 和语音会话纯 Rust 核心。
- [x] 建立 Windows CI 和真机测试手册。
- [x] 实现 WinRT 已配对设备扫描、GATT 连接/释放、ATVV 通知和 PCM 解码代码路径；Windows 与 RC001/RC003 运行验收仍待完成。
- [x] 补充 RC001/RC003 设备名称与标准 GATT Model Number（2A24）识别，在连接快照和界面中传递型号；无法识别时保持 `unknown` 且不阻断 ATVV。Windows 双型号真机验收仍待完成。
- [x] 将 RC001 短语音场景作为 JSON 夹具回放，覆盖 40 + 80 字节拆包、20 次极速空会话、20 次完整会话和中断后首个新会话恢复；该回放不代表真实 Windows/RC001 固件验收。
- [x] 将真实连接阶段、能力与解码采样计数接入 Tauri IPC 和连接页面。
- [x] 使用同一 JSON 契约夹具验证 Rust 序列化与 TypeScript 接口的 `PlatformSnapshot`、`PairedRemote`、camelCase 字段及 RC001/RC003/unknown 枚举值；Windows WebView 运行时 IPC 仍待验收。
- [x] 实现显式 WASAPI 输出端点枚举、选择、16 kHz PCM 写入、有界队列和真实 padding 排空代码路径。
- [x] 实现用户显式配置的语音键按住说话快捷键（连接页：默认预设左 Ctrl + 左 Win，可“修改快捷键”自由录入任意组合（OS 级录入，全部按键松开后才落盘），另有“关闭”；2026-09-26 由固定预设改为可自由修改，默认值与注入时序不变）：按下语音键先注入 DOWN 再开始音频会话，释放统一注入 UP，断连/睡眠/中止/退出强制释放；注入时序参考 ZSTDJan 按住说话快捷键与 Voice_VibeCoding 的 Hold 语义，仅使用 SendInput 公共 API（见 ATTRIBUTION.md）。2026-09-04 修复一：和弦改为逐事件提交、事件间 80ms 间隔（WeType 拒绝单批零间隔，evidence/p）。修复二：F5 抑制器会话武装信号误接未启动的旧模块 voice_key_suppressor（ble.rs），遥控器 F5 泄漏进和弦致 WeType "额外按键"拒绝——改接 key_suppressor 并删除旧模块（Bugs\2026-09-04-wetype-zero-gap-injection.md）。2026-09-10 再修复：两套同类 Raw Input 注册互相覆盖导致重连期设备归因失效，改为主监听器统一转发，并在建链四相位临时保护 F5，防止记事本收到 F5 插入时间戳；新增逐阶段 BLE 重连日志（Bugs\2026-09-10-voice-f5-timestamp-during-reconnect.md）。加固：钩子链头 bump（会话开始 + 10s 定时）+ Raw Input 单一注册。**RC001 真机端到端 passed（2026-09-04，用户确认文字上屏；前提=输出端点 CABLE Input + 系统默认录音 CABLE Output）**；RC003 本修复真机待验。
- [ ] 使用真实 RC001/RC003 和第三方语音程序（微信输入法、Win+H 等）验证按住说话快捷键：DOWN/UP 严格成对、无粘键、无重复音频，且断连和睡眠恢复后不残留按住的快捷键。RC001 基本链路与加固版回归均已 passed（2026-09-04，型号经应用 2A24 显示双证）；RC003 基本链路 passed（连接/触发/MIC_EXTEND 续期正常），音频送达率经**重配对后复测 passed**（55%→98.7%，与 RC001 基准持平，文字"一二三四五六七八九十"全对——初次配对的连接参数带宽不足，重配对即修复，已列为标准处置；Bugs\2026-09-04-rc003-voice-quality.md）。剩余待验：快速连按成对性、断连/睡眠恢复残留复验。
- [x] 实现 RC001/RC003 选择持久化、意外断连指数退避重连和 Windows 睡眠/恢复通知代码路径；2026-09-12 修复 BLE MTA 线程误用 UI-thread-only `FromIdAsync` 导致 Windows 资源错误/工作线程卡死，改由配对 ID 的对端地址调用 `FromBluetoothAddressAsync`，并补齐连接阶段、退避与无线电恢复结构化日志；2026-09-13 针对 `0x80070008` 补充启动期 Radio 预热缓存，把“两次恢复后永久耗尽”改为 60 秒冷却后自动重开恢复窗口，并补齐设备创建后所有连接失败路径的事件退订、CCCD、连接参数、GATT service 和设备显式释放，避免重试自身持续泄漏 WinRT BLE 资源；2026-09-14 增加系统 BLE 栈已经连 Radio 枚举都失败时的 BTHUSB 设备节点自动重启兜底（系统 UAC 明示授权、精确选择唯一适配器、独立 WinRT 读回验证），并修复会话 Close 首次失败后不再真正重试 service/device 释放的问题。现场 `0x80070008` 经新兜底恢复 passed；本地包与 RC001/RC003 各自端到端验收见 `Testing/WindowsBleResourceRecovery.md`。
- [x] 在 Windows 主机编译 Tauri NSIS Preview 安装包；Windows CI 已生成并复验绑定精确来源 Commit、SHA-256 和未签名状态的 artifact，安装、升级、卸载与正式签名仍待完成。
- [x] 提供去标识化运行诊断摘要和页面内复制入口；自动化已证明不导出设备身份、路径、端点名称或错误原文，Windows WebView 剪贴板仍待运行验收。2026-09-16 诊断入口从权限页迁到关于页（权限页只保留蓝牙/按键/音频三项状态，仿真新增断言防止入口回流），并在关于页增加"打开日志目录"：目录由 Rust 从日志初始化的实际落盘路径推导、前端不传路径（维持 capabilities 最小权限），打开复用 ShellExecuteW 链路；Windows CI 仿真只验证 WebView → Tauri IPC → Rust → shell 的往返与终态消息，真实桌面资源管理器打开仍 deferred。
- [x] 持久化并展示仅保存在本机的每日按键次数、完整语音会话次数和语音采样时长；Windows/RC001/RC003 真实事件计数与升级保留仍待真机验收。
- [x] 对低于 Windows 10 1809（build 17763）的系统增加 NSIS 安装与应用启动双层拒绝门禁；Windows 10 1809 / Windows 11 提示和安装行为仍待真机验收。
- [x] 在 Windows CI 对 NSIS Preview 执行 `/S` 当前用户安装、启动存活、`/S` 卸载及设置保留边界验证；该自动化不代替可见安装界面、SmartScreen、Windows 10 1809 或真实用户环境验收。
- [x] 在 Windows CI 使用仅测试构建可启用的平台仿真，验证真实 WebView JavaScript → Tauri IPC → Rust command、五页导航、RC001/RC003 扫描、首次 RC001 语音、音频端点、Raw Input、映射、诊断和资源释放闭环；生产 NSIS 已验证不含仿真入口，该结果不代表真实 Windows API 或硬件通过。
- [ ] 使用真实 RC001 验证型号识别、BLE 配对、连接、断开、重连和首次语音。
- [ ] 使用真实 RC003 验证型号识别、BLE 配对、连接、断开、重连和首次语音。
- [ ] 验证 `STREAM_START → AUDIO → STREAM_STOP` 首次会话完整可用。
- [ ] 在 Windows 真机验证 WASAPI 端点初始化、VB-CABLE 回环、欠载恢复与完整尾音。
- [x] CABLE Input 双层静音自愈：端点主静音使用 `IAudioEndpointVolume`；音量合成器里的 SayAll 应用会话静音使用 `ISimpleAudioVolume`，只匹配当前进程在已选 CABLE 端点上的会话。初始化、会话开始、流启动后检查，推流期间每 100ms 低频检查，必要时解除静音并读回；两层均不修改音量。2026-09-07 Windows 真实 CABLE Input 受控复现 passed：端点 open/begin 两检查点，以及应用会话 begin/after_start/stream_watch 三检查点均成功从 muted 恢复到 unmuted；随后真实 RC001 连续 9 次语音均复现“流启动约半秒后会话被外部重新静音”，监视路径 9/9 捕获并恢复为 unmuted（2765 个音频包），9 次开始/停止与快捷键按下/释放均成对；修复版 NSIS 本地包由用户复测确认 RC001 语音功能 passed。边界：完整 RC003 → CABLE → 输入法语音链仍按上一项真机验收。
- [x] 生产诊断日志默认持久化到 LocalAppData：覆盖进程/Tauri/前端/Vue/首次 IPC 启动链，音频端点枚举与 `virtual_cable|bluetooth|other` 脱敏分类、WASAPI 打开/自动转换/推流/排空/中止/失败，以及按键映射和按住说话快捷键的加载、保存、重置；禁止原始音频包、端点名称/ID、自定义应用路径和异常正文进入生产日志。2026-09-08 自动化验证 passed；真实蓝牙耳机故障复现与安装包白屏现场日志验收 deferred。
- [x] 在可见 NSIS 安装完成后检测 VB-CABLE 服务，未安装时说明第三方来源、管理员权限和重启要求并打开官方下载页；应用首次启动复检唯一 CABLE Input 并在无既有选择时自动配置。静默安装不打开网页，真实安装/重启仍待真机验收。
- [ ] 如未来需要捆绑或自动执行 VB-CABLE 驱动包，先取得与 Pack45 内附许可一致的作者书面授权，并实现来源校验、显式 UAC、结果检测和重启流程。
- [x] 持久化用户选择的输出端点，并在端点消失或更名时失败关闭；Windows 运行时恢复仍待真机验收。
- [x] 实现设备路径 fail-closed、隐藏消息窗口、Keyboard/HID 双来源合并和停止释放的 Raw Input 代码路径；Windows 与 RC001/RC003 真机按键验收仍待完成。
- [ ] 实现按键映射保存、热加载和 SendInput：独立映射文件、显式热加载、批量 SendInput、部分提交回滚和界面测试已完成；2026-09-10 修复 Win+L：锁定动作改走公开 `LockWorkStation` API（RC003 电源单击现场 passed）。精确 Win+L 先等待实体 UP、门控完成边沿配对后再锁屏；2026-09-12 进一步实证 `microsoft-edge:` 弹窗并非迟到边沿，而是 TV 原生 Shell 协议动作在后续锁屏时由系统服务创建 `OpenWith.exe`，新增仅在 TV→SayAll 锁屏周期启用的 CREATE 阶段精准拦截（原型四轮现场 passed，产品化安装包待验）。当前主机实证物理 Win+L 无法由普通用户态钩子可靠阻止，链首刷新方案又造成事件丢失，已回退；录入默认保留直接模式，并增加默认关闭的安全模式开关（界面选修饰键、键盘只按主键），两种模式自动化 passed、安全模式安装现场复验 deferred。真实 Raw Input 边沿自动执行仍须分别等待 Windows/RC001/RC003 确认 Keyboard/HID 事件形态，避免重复输入。
- [x] 按键映射页增加“保存配置 / 导入配置 / 导出配置”：沿用保存即热加载，导出版本化且稳定排序的 JSON；导入先做 1 MiB 上限、格式版本、动作与快捷键完整校验，落盘成功后才一次性替换运行态，取消选择不报错。Rust/Vue 自动化与 Windows COM 对话框代码路径 passed；可见文件选择器、跨机器迁移及 RC001/RC003 导入后实体按键回归 deferred。
- [ ] 完成 Windows 10 1809 / Windows 11 安装、升级和卸载验证。
- [x] 在 Windows CI 构建较低版本 NSIS 候选，验证当前用户安装、升级后单一安装身份、设置/映射/统计逐字节保留、降级不替换当前版本和最终卸载保留用户数据；该矩阵不代表真实历史二进制、可见安装界面或 Windows 10 1809 / Windows 11 真机验收。Tauri 2.11.1 静默页不会可靠设置内置降级检查所依赖的版本比较结果，已在既有 preinstall hook 中增加独立 SemVer 门禁；Run 33637195089 通过并确认 predecessor `/S` 返回 1638、当前 0.1.0 与用户数据保持不变。
- [ ] 建立自签 Authenticode、证书指纹和 SHA-256 发布流程。
- [~] 返回键、音量加、音量减已恢复产品配置入口：持久化层、运行时映射引擎和 Vue 编辑器不再剥离/禁用三键；RC003 的按设备源头捕获已真机成立（2026-09-25），**RC001 回归 E1–E5 已全部真机通过（2026-09-25，Andy）**（手册 `Testing/WindowsRC001ThreeKeyMapping.md`）。真机同时推翻旧认知：RC001 三键**同样依赖增强捕获**（开=可映射、关=不映射），与 RC003 一致——不要再按“RC001 免助手”设计。
- [ ] 为返回键、音量加、音量减完成按设备源头捕获：按 2026-09-22 路线文档实现独立 Helper/捕获协议，先修复完成态与握手状态机，再分别完成 RC001/RC003 的 E1–E7 真机验收；未完成前不能宣称三键端到端可用。（**第 ① 段捕获 + 第 ② 段传输（桥接）均已实现，引擎零改动；缺真机联调 E7 与端到端映射生效**）
- ✅ 2026-09-25 更新（**端到端映射在真机成立，且支持与另一台 BLE 键鼠长期共存**）：Andy 在「RC003 + 另一台蓝牙 LE 键鼠同时连接」的环境下实测：另一个键盘打字正常（内容门禁未误清任何字节），RC003 三键映射生效。三条独立证据一致：① 助手日志 `[SHARED-HOST] decision=continue_with_usage_gate members=3 rc003_members=1`（共享宿主已放行，不再 `[STOP]`）；② 心跳 `up=5613s handshake=true lease_ok ioctl_calls=44604 target_hits=48 clears_ok=15 clears_fail=0 restores_ok=15`；③ 主程序诊断日志 `event=helper_authenticated` → `event=first_edge pressed=0x00F1`（边沿真的进了映射引擎）。**放行依据**：agent 的 `targetSetIn` 是内容门禁——报告不含目标 usage（`0x00F1`/`0x0080`/`0x0081`）时 `found.length === 0` 直接 return，清键也只清命中 `clearUsages` 的两字节，宿主独占属过度严格的前提（`--require-exclusive-host` 可恢复旧行为）。残留前提：同一宿主内没有别的设备发出这三个 usage。提交 `2c36ffb`。**仍未验证**：RC001 三键映射回归、`Testing/WindowsRC003EnhancedCapture.md` 的 E2（`run-helper-canary.cmd` 哨兵键）、E4a（强杀助手后的 fail-open）、`--duration` 自动收尾、`[STALE-TAP]` 分流；`agent_selftest.py` 需 frida，本机未装（deferred）。
  - 2026-09-23 更新：**不提权**的免驱动替代路线已穷尽并判定 failed（厂商 GATT 通知静默、GATT HID 特征 AccessDenied、用户态直读 HID 顶层集合被 RIM 独占拒绝、Raw Input 含厂商页注册仍零事件）。详见 [docs/investigations/2026-09-23-rc003-driverless-capture-investigation.md](docs/investigations/2026-09-23-rc003-driverless-capture-investigation.md)。
  - ⚠️ 2026-09-23 二次更正：上一条原称"按设备源头捕获重新成为**唯一**可行形态"——**该"唯一"不成立**。复核 `ZSTDJan/windows-remote-mic-app`（v1.0.44）后确认它根本**不经过 Raw Input**，而是在承载该设备的 `WUDFHost.exe` 内于 `NtDeviceIoControlFile` 的 UMDF 输出复制入口读取并清空报告；本机只读实测已确认该宿主存在、PID 可定位、且为**独占 RC003** 宿主。因此方案空间内**并列**两条：**路线 A（HID 宿主内报告层捕获；提权助手 + 注入框架；无内核驱动、无需 `TESTSIGNING`、无需重启）** 与 **路线 B（`kbdhid` 之下 KMDF lower filter；需 `TESTSIGNING` + 关 Secure Boot + 重启）**。决策点：路线 A / 路线 B / 维持不可用。详见 [docs/investigations/2026-09-23-zstdjan-hid-host-tap-implementation-review.md](docs/investigations/2026-09-23-zstdjan-hid-host-tap-implementation-review.md)。
  - ✅ 2026-09-23 第四次更新（**产品化 spike 就位，已提交 `76e3855`**）：把最小实验装置推进成可交付形态——`hardware/RC003/helper/`（提权助手 + Gadget 侧 agent + 载体锁定/取回脚本 + 自检台）。助手**零第三方依赖**（全走 kernel32/advapi32/shell32 原生 FFI，release 约 400 KB），负责「注册表定位宿主与独占性判定 → 校验 Gadget → 先监听后注入 → 环路 TCP 收 agent 上报」；agent 按 usage **选择性**清空（只清 `0xF1`/`0x80`/`0x81`，确定/主页/方向键保持走 Windows 原生路径）。自检结果：助手 **14/14 PASS**（退出码 0）、agent **3/3 PASS**（退出码 0），两项均**无需提权、无需设备、不注入**，证据见 `hardware/RC003/evidence/{helper_selftest,agent_selftest}.out`。上一行所列"边沿配对 / 租约 / fail-safe 失效"三项**已实现但尚未真机验收**：fail-open 判据必须用**租约**（`Date.now()-lastRenewAt <= LEASE_MS`）而非连接状态——实测 Frida 17 的 Socket API 没有可用存活原语（对端关闭后写入不抛错、pending read 不以 EOF 收尾、连不上要约 2.2s 才 reject）；6 条反直觉语义已写入 agent 文件头。**下一步**：按 `hardware/RC003/helper/` 下的右键入口依次跑 `run-helper-selftest.cmd`（无需提权）→ `run-helper-dryrun.cmd` → **`run-helper-observe.cmd`（零回归风险，先确认三键边沿到得了）** → `run-helper.cmd`（正式拦截）；验三键边沿送达、确定/主页/方向键无回归、杀掉助手后按键自动恢复（fail-open），再写结果文档。
  - ⚠️ 2026-09-23 第五次更新（**真机首跑失败：`HostPid` 读取宽度写错；已修复，并已可在免提权下复验**）：Andy 首次以管理员身份跑 `run-helper-dryrun` 时，助手在第一步即以退出码 11 退出，并提示"设备未连接/未配对"——**该归因是错的**。同一时刻用只读探针做阳性对照：`diag_keys=6`、`HostPid=32684`、`exclusive_rc003_host`，设备与宿主都在。逐层取注册表返回码后定位到根因：本机 `HostPid` 是 **`REG_QWORD`（类型 11，8 字节）而非 `REG_DWORD`**，助手按 4 字节缓冲区读一律得到 `ERROR_MORE_DATA(234)`，而实现把任何非 `ERROR_SUCCESS` 都当 `None` 静默丢弃 ⇒ 6 个节点全丢 ⇒ 计数 0 ⇒ 误报"设备未连接"。**它为什么躲过既有防线**：① Python 探针用 `winreg.QueryValueEx`，它按真实类型自动转换后返回 `int`，从不编码"宽度"这个约束——**探针能跑通 ≠ 4 字节读法可用**（又一例"参考实现能跑通 ≠ 我的原语正确"）；② 错误信息**断言了一个未被检验的原因**。修法：读取改为两步（先用空缓冲区问类型与所需字节数，再按实际宽度读）、新增纯函数 `decode_host_pid`（类型或宽度不符一律拒绝，不猜）、`enum_hosts` 返回诊断计数、归因改为互斥三分支（无诊断节点／有节点但读不出（**本程序的问题**）／有宿主但不是 RC003（设备问题））、读取失败一律打 `[REG-WARN]` 不再静默；并去掉 `--dry-run` 的无谓提权门（它全是只读检查，**正是那道 UAC 门把这个 bug 藏住了**）。自检补两项回归（`decode_host_pid` 五例含"`REG_SZ` 必须拒绝"、实机 `Enum` 扫描"凡存在的节点必须读得出 `HostPid`"），并把日期/时间戳格式也纳入自检；日志改为**追加** + 带本地时间戳的分隔头（原先每轮启动即截断，一次 dry-run 就把首跑失败的原始记录抹掉了）。**现已验证**：助手自检 **21/21 PASS**、免提权 `--dry-run` **退出码 0** 全链通过（`diag_keys=6 / HostPid读出=6 / rc003_instances=1` → `pid=32684 WUDFHost.exe` → `exclusive_rc003_host` → Gadget 尺寸与 SHA-256 校验通过），与只读探针**两个互不认识的实现给出同一 PID**；证据 `hardware/RC003/evidence/{helper_selftest,helper_dryrun}.out` 与 `hardware/RC003/evidence/wudf-host-probe-crosscheck.log`。**仍未做**：真机注入（`run-helper-observe.cmd` / `run-helper.cmd`，需提权）。详见 [docs/investigations/2026-09-23-rc003-helper-hostpid-read-bug.md](docs/investigations/2026-09-23-rc003-helper-hostpid-read-bug.md)。
  - ✅ 2026-09-23 第六次更新（**真机 `observe` 首次跑通：三键捕获成立；同时暴露两处"安全上限失效"，已修**）：Andy 以管理员身份跑 `run-helper-observe.cmd`（`--duration 300`、`elevated=true`、注入宿主 `32684`），按返回/音量+/音量− 各 5 次，共得 **30 条 `[EDGE]`**：`0x00F1`→back ×5、`0x0080`→volume_up ×5、`0x0081`→volume_down ×5，每次按下紧跟一次释放；`target_hits=15` 与按下数 **1:1**、`edges_sent=30`、`clears_ok=0`（observe 从不清报告）。**这是首次由我们自己的助手 + 自己的 agent + 自己的传输把三键边沿送到应用侧可消费的位置**（此前只证明过"报告里有这三键"与"改写会被采纳"，但两次都发生在探针里）。证据 `hardware/RC003/evidence/2026-09-23-observe-run.log`。**但同一次运行也暴露了两处我写下的承诺其实是空的**：① `--duration` **在有 agent 连接时永不到期**——会话读取原本阻塞在 `BufReader::lines()`，主循环被同步阻塞，检查点永远到不了（实测 300 s 的跑到 518 s 仍在运行）；② **Ctrl+C / 关窗不发 `disarm`**——从未安装 `SetConsoleCtrlHandler`，系统直接硬终止进程，`main()` 末尾那段收尾代码永远执行不到。**唯一真正兜住的是 agent 侧租约**——设计上它只是最后一道防线，不该成为唯一一道。修法：① 给 socket 设读超时（`READ_POLL_MS = 250`）改为轮询式读，并把**绝对到期时刻**传进会话循环（收尾逻辑仍只在主循环一处）；② 安装控制台处理器（只置位，由主线程收尾）。自检 21 → **23 项**，新增「会话循环遵守 `--duration`（静默客户端占住连接）」与「控制台处理器可注册」，并**为前者做了阳性对照**（独立小程序 `helper/target/tmp/blocking_vs_poll.rs`，不进产品：同一静默客户端下旧写法 2 s 内不返回、新写法 1547 ms 返回），确认这条回归项在修复前**必然 FAIL**。**仍未做**：`run` 模式的拦截生效、确定/主页/方向键无回归（observe 一个字节都不清，"无回归"在那次运行里是构造上的必然、**不构成证据**）、杀掉助手后的 fail-open 现场验收；另需如实登记「注入的 Gadget 会常驻宿主进程直到其重启，届时功能惰性（租约已失效）但不是零残留」。详见 [docs/investigations/2026-09-23-rc003-observe-first-real-run.md](docs/investigations/2026-09-23-rc003-observe-first-real-run.md)。


  - ✅ 2026-09-23 第七次更新（**"第二次运行必崩"已定位并修：宿主长期映射 Gadget DLL 导致覆盖复制必然 `os error 32`**）：Andy 在 `observe` 首跑成功后再跑三次，三次都在**注入之前**退出，报 `[STOP] 复制 Gadget 失败 ... (os error 32)`。根因两条独立证据：① 注入成功的代价是宿主 `WUDFHost.exe`（pid=32684）**长期映射**运行时目录那份 `frida-gadget.dll`——该宿主启动于 01:23:26、采样时已运行 **14.47 h**，DLL 一旦被 `LoadLibrary` 就锁到宿主退出为止；② 而 `prepare_runtime` 当时是**无条件覆盖复制**，把"宿主还映射着上一代 DLL"这个**正常状态**当成了致命错误。**取证过程本身也留了教训**：先用 `READ + share=NONE` 判断占用是**错的**——阳性对照（同一探针去问本进程映射着的 `kernel32.dll`）同样返回 OK，"能打开"推不出"没被占用"；改 `WRITE` 又被 `C:\ProgramData` 的 ACL 用 `err=5` 掩盖了真正的 `err=32`。最终用 **Restart Manager API** 直接问系统要占用者名单（不需提权、自带阳性对照）。**修法（把状态显式化，不是绕过）**：启动时枚举宿主模块判断是否已有我们的 tap（`[TAP]`）→ 令牌改为**跨运行稳定**（运行时目录 `session.token`；此前每次换令牌，常驻 agent 的 hello 必然被当冒充者拒掉，于是 agent 侧本来就写好的重连路径**在实践中永远走不通**）→ 能接管就**不复制不注入**（`[ATTACH]`）并补发 `arm`/`mode`/`restore`（漏发 `arm` 会导致接管后心跳正常但 `[EDGE]` 永不出现）→ 接不上（本机制之前的旧世代）打 `[STALE-TAP]` + 清理步骤 + 退出码 13，**不再去撞那个必然失败的文件**（实验性退路 `--new-generation`）→ 复制前先算摘要，一致则 `action=reuse`（一个字节都不写）→ 注入后**用模块枚举核实**，不再拿 `LoadLibraryW` 返回值当"已加载"的证据（模块已存在时它同样返回非 0）→ 新增 `--await-hello`（默认 30 s），注入了却没会话就 `[NO-HELLO]` + 退出码 12，不许静默干等。agent 侧补**下行静默看门狗**（frida 17 实测对端关闭后 write 不抛错，断线在 socket 层看不见；但助手每 500 ms 续约，故"静默 > 3 s"是可靠判据——**接管路径能否真的连回来靠的就是它**）与鉴权失败退避（15 s）。自检 23 → **34 项**，新增项**第一次跑就抓到两个真 bug**：Toolhelp 的 `szModule` 是 **256** 不是 260（写错 → `ERROR_BAD_LENGTH(24)` 且模块列表为空；**若只有阴性断言会静默通过**）、分代目录名未强制小写。**本机仍留一次性迁移**：15:10 那次注入的令牌未持久化，属旧世代接不上，需断开重配对 RC003 / 设备管理器禁用再启用 / 重启系统清一次（清理后用 `probes/windows-restart-manager-probe.py` 确认 `users=(none)`）。**未验证**：接管路径（`[ATTACH]`）与 `[STALE-TAP]` 分流的真机端到端、`--new-generation` 的同宿主双实例。详见 [docs/investigations/2026-09-23-rc003-helper-rerun-blocked-by-locked-gadget.md](docs/investigations/2026-09-23-rc003-helper-rerun-blocked-by-locked-gadget.md)。

  - ✅ 2026-09-23 第八次更新（**反复运行验收通过，接管路径首次真机成立；同轮修掉 6 处只污染证据、不影响功能的缺陷**）：Andy 先断开并重新配对 RC003 清掉旧世代 tap（三条只读证据一致：占用者探针 `users=(none)` 且阳性对照通过、宿主 PID `32684` → `31284`、运行时目录无 `session.token` / 无分代目录），随后四次启动的结果：① 16:06:37 **全新注入通过**——`[PREP] action=reuse`（一个字节都没写，`os error 32` 不再出现）、`[VERIFY-MODULE] gadget_modules_in_host=1`、`[HELLO] auth=true`、三键各一组 `[EDGE]`、`how=injected`；② 16:10:08 撞端口（上一轮仍在 `--duration 300` 窗口内，其 `uptime_s=228` 出现在 16:10:25），`os error 10048`、退出码 8；③ 16:10:37 **接管通过**——`[TOKEN] source=file` → `[TAP] resident=1` → `[PREP] skip_copy_attaching_existing_tap` → `[ATTACH] skip_injection` → `[HELLO] auth=true` → 三键各一组 `[EDGE]` → `how=attached_existing_tap`。第 ③ 项是首次证明「接管」这条独立代码路径端到端可用，含最容易漏的一环：收尾发过 `disarm`，所以接管后**必须补发 `arm`**，否则心跳、续约、握手全都正常，但按键仍完全不可见。证据 `hardware/RC003/evidence/2026-09-23-acceptance-run.log`（未改动原文 `…-raw.log`）。**同轮暴露并修掉 6 处证据/鲁棒性缺陷**（共同点：不影响功能，只让下一次排查读到错误结论）：① 运行分隔线由 `Logger::new` 写，而续约线程每轮再建一个 Logger ⇒ 每轮多出一条「新一轮运行」，**把 6 轮数成 11 轮**（我自己就被它误导过；这条分隔线当初加进来唯一的目的就是让每轮起点可辨）；② agent 的 `Socket.connect` 可以叠加（连不上要约 2.2s 才 reject，短于心跳周期 1s）⇒ 接管轮 **3 条连接、2 条被 RST**（`10054` / `10053`），且哪一条赢取决于完成顺序——接管路径变成看运气；③ `pump` 的读失败分支**绕过** `dropConnection` ⇒ 断线重连确实发生了，但 `rx_timeouts=0`、`auth_rejected=0`，**「连接死过」在统计里完全不可见**；④ `discarded` 一个计数管两件事（未连接时丢掉的心跳 vs 令牌不符的命令），后者才意味着有进程在冒充助手，却无法区分；⑤ 接管轮 `[DLL]` 照搬另一条路径的字段，打出 `reused_existing=false sha256_verified=false`，读起来像「没校验就用了」，而事实是那一轮一个文件字节都没碰；⑥ `[CONFIG]` 的 `arm` 恒为 `true`，于是出现 `sent=false arm=true` 这种自相矛盾的行（arm 根本没送到）。修法：分隔线只在 `open_round` 写；agent 加 `attemptSeq` / `connecting` 并发守卫（输家主动 `close`，发 FIN 不发 RST）；读失败改走 `dropConnection(read_error)` 并计数、用 `mine` 核对回调属于哪条连接（否则旧连接的失败会误杀刚接上的新连接）；`discarded` 拆成 `send_dropped` / `cmd_rejected`；`[DLL]` 与 `[CONFIG]` 按这一轮实际做了什么如实汇报；另外把**绑定端口提到准备运行时目录之前**（失败要早，且失败的那一轮不该动磁盘状态），并给 `10048` 可操作文案。自检 34 → **38 项**、agent 自检台 3 → **4 项**（新增用例 D：助手消失后重新上线**只允许连一次**，并要求断线在统计里可见），且**为 D 做了阳性对照**——`agent/control_make_noguard.py` 生成三处回退的副本，同一用例必须 FAIL：实测对照版**被连 3 次、3 条 hello、断线可见=0**，与真机现象一致。**仍未验证**：`--duration` 到期自动收尾在真机上从未跑到（两次真机运行都是手动结束，228s / 40s，无 `[TIMEUP]`；启动器现已接受参数，`run-helper-observe.cmd 60` 可缩短等待）、`run` 模式的拦截生效 + 无回归 + fail-open、`[STALE-TAP]` 分流的真机行为、`--new-generation`。详见 [docs/investigations/2026-09-23-rc003-rerun-acceptance-verdict.md](docs/investigations/2026-09-23-rc003-rerun-acceptance-verdict.md)。

  - ✅ 2026-09-23 第九次更新（**补上一处判据缺口：RC003 上「拦截生效」本来无法观测 → 哨兵键；并落盘 E1–E6 验收手册**）：准备 `run` 模式验收时发现一个**逻辑缺口**——三键在 Windows 侧本来就零事件（`kbdhid` 丢弃 `0xF1`/`0x80`/`0x81`），所以「清掉」与「不清」在外部完全看不出差别：音量不会变、也没有任何字符。于是 E2 的传统判据「按了之后没有原生动作」在 RC003 上**恒为真**，它区分不了「清空生效」与「清空根本没跑到」——`observe` 与 `run` 在现象上也无法分辨，这条判据没有分辨力。处置：助手新增 `--canary-usage <U16>`（十六进制、逗号分隔、可重复），新入口 `run-helper-canary.cmd` 默认额外清**主页 `0x4A`**（Windows 本来能处理的键）。三步直观对照即可**直接**证明两件事：运行前在记事本按主页能跳行首（基线，即阳性对照）→ 运行中毫无反应（**清空真的作用到了报告上**）→ 结束 / 租约到期 / Ctrl+C 之后又能跳（**fail-open 生效，不残留清空、不需重启**）。安全边界：只清该 usage 的 2 个字节，`report_id`/`modifiers`/`reserved` 一律不碰；受 `--duration` 上限与 agent 侧 2 s 租约双重兜底；**默认关闭**。哨兵键**只进清空集合、不进上报集合**——agent 侧拆出 `reportUsages`（恒为三键）与 `clearUsages`（可追加），`[EDGE]` 与心跳里的 `report_usages` 恒为三键，`clear_usages` 才是实际范围；助手在鉴权后随 `arm`/`mode`/`restore` 一并发 `targets` 命令（**走命令而不是 config**：接管轮 agent 早已启动，不会重读 config）。协议护栏两条：`report` 必须**恰好**等于三键（否则等于允许远端关掉上报，那样「看不到按键」会被误读成「按键没到」）、`clear` 必须是 `report` 的**超集**（否则出现「报了但不清」的隐形配置）；越界命令一律拒绝且**不改动现有清空范围**。自检 38 → **42 项**（哨兵键解析与拒绝、清空集合不变量、usage 渲染格式、内嵌 agent 与 helper 的三键常量一致且含 `targets` 命令），agent 自检台 4 → **5 项**（新增用例 E），并把阳性对照扩到**四处**回退：`agent/control_make_noguard.py` 现在也去掉 `targets` 两条护栏，实测对照版 **D 与 E 双双 FAIL**（E：无拒绝日志、清空范围被改坏）。免提权即可复核：`--dry-run --canary-usage 0x4A` 打印 `[CANARY]` 与 `[DRY-RUN-CANARY]`（`clear_usages=0x00f1,0x0080,0x0081,0x004a`）；`0x00F1`（与三键重叠）/`0`/`zz` 分别被 `[STOP]` 或参数错误拒绝，退出码 2，证据 `hardware/RC003/evidence/canary-argcheck.out`。**验收手册落盘**：`Testing/WindowsRC003EnhancedCapture.md`——E1–E6 的命令、判据、日志字段、退出码表、常见失败处置与状态表；E1（report 归属）、E2 的锁屏场景、E4d、E5、E6 标为 `deferred` 并写明依赖；E2/E3/E4a/E4b/E4c 标为「未执行，可直接做」，均需 Andy 本人提权运行。**仍未验证**：上述真机项全部未执行（`run-helper-canary.cmd`、`run-helper.cmd 120`、E4a 强杀助手观察恢复）；`[STALE-TAP]` 分流的真机行为、`--new-generation` 同宿主双实例。
  - ✅ 2026-09-23 第十次更新（**捕获链第 ② 段（传输）接线完成：三键边沿从助手送进映射引擎；引擎零改动，已提交**）：先补上一个此前一直存在的**缺口**——接线前助手收到 `{"type":"edge"}` 只做 `session.edges.push()` 与写日志，全库**没有**任何向主程序转发的通道（无 pipe / 命名管道 / IPC / 共享内存）。所以三键的真实状态是"**能配置、按下去没反应**"：配置层、引擎、UI 画布早就有这三个语义键（`RemoteButton::{Back,VolumeUp,VolumeDown}`），只是永远等不到输入；而主程序侧的 Raw Input 与键盘钩子同样拿不到它们（那正是 `kbdhid` 丢弃的直接后果），边沿**只能**由助手主动送过来。**方向论证**：助手以管理员运行、运行时目录在 `%ProgramData%\SayAll\rc003-helper`，普通权限的主程序既读不到那里的写入也不该去猜；反向（助手监听、主程序连接）因此不成立。改为**主程序监听随机端口（`127.0.0.1:0`）并写出描述文件 `%LOCALAPPDATA%\SayAll\rc003-bridge.ini`（含 port + token），助手读取后回连并出示令牌**；跨账户提权时助手按**固定相对路径**枚举 `C:\Users\*\AppData\Local\SayAll\rc003-bridge.ini` 取最近修改的一份兜底。**引擎零改动的关键**：助手送来的边沿投进 `EngineMessage::GateEdge`——与 RC001 上"被 `key_gate` 吞下的厂商键边沿（直接归因族）"是**同一条**通道，于是手势识别、映射查表、`SendInput` 注入、按住连发、泄漏对冲全部复用，RC003 与 RC001 在引擎下游完全同构。**刻意不用 `HidUsages`**：它是**整体替换**语义（`ButtonStateMerger::update_hid_usages` 直接换掉 `merger.hid`），若同时有 RC001 在跑会把它的 HID 按下状态一起冲掉。**协议**：ASCII 行 `HELLO/OK/DENY/E/P/BYE`，边沿是**绝对状态**（丢行自愈；断线期间的变化无从逐条补发，重连后发**一份**绝对状态即可完全对齐）。**fail-open 三层**：① 助手正常收尾先发释放边沿**再**发 `BYE`（顺序不能反，先 BYE 会被当成"正常告别"）；② 助手被强杀时由主程序侧**静默看门狗**（3 s 无任何行）释放全部并断开——这一层不可省，被任务管理器结束的助手不发 `BYE`；③ agent 租约（2 s）保证的是"键恢复原生行为"，与前两层不可互相替代。**安全边界如实登记**：令牌**不是**安全边界，强度只是"每次启动都不同"级别（时间+PID+栈地址混合，非密码学强度），作用是防误连/防混淆——同用户进程本来就能读描述文件、也能注入主进程。真正要防的场景是"助手没在跑时别的进程占住端口喂伪造边沿"，这由**方向选择**天然消解（主程序是监听方且只接受正确令牌）；**纵深防御在白名单**：只接受 `0x00F1/0x0080/0x0081`，其余 usage 丢弃并计数（`usages_dropped`），即使令牌泄漏也只能伪造这三个键——考虑到映射动作包含"启动任意应用"，这条限制是实质性的。助手侧**只在 `session.authenticated` 为真时**才 `push_edges`。**同轮两个真实缺陷**（都是编译通过、逻辑看着对、只有构造出场景才暴露）：① 收尾时**无条件**清掉当前连接，会把刚接手的新连接一起清掉 ⇒ 新连接下一轮认为自己被替换、主动让位 ⇒ **两个连接互相让位、桥接整体哑掉**（现场表现"助手重启后再按三键毫无反应"，而两边日志都只是安静退出、没有任何错误）；修法是把当前连接改成带**自增编号**的 `CurrentConn{id,stream}` 并按编号判定"我还是不是当前连接"——**不能用 socket 地址替代**，连接被 `shutdown` 后 `local_addr()` 会失败，地址比较退化成"谁都不是当前"，同样误判。② `accept_loop` 里直接串着 `handle_connection`（阻塞到该连接结束）⇒ 旧连接结束前**新连接根本不会被 accept**，"后来的助手顶掉旧连接"这段代码**永远不会执行**（自测里表现为 beta 收不到 `OK`）；改为每连接独立线程。配套修法 ③：**被接管方不得释放状态、也不得改写共享状态**（它手里那份已归接手方所有，释放会误伤接手方刚建立的按下状态，表现为"刚按下就被松开"）。**验证**：`crates/sayall-windows/src/rc003_bridge.rs` 单测 **9 项**，含**真 TCP 端到端**（描述文件 → 连接 → 错令牌被拒 → 正确令牌 → 边沿 → 释放 → Drop 清理描述文件）、接管（`replacement_takes_over_and_survivor_keeps_working`）、看门狗（`silence_watchdog_releases_pressed_buttons`，第一版用 `drop(stream)` 构造"被强杀"是**错的**——reader 持有同一 socket 的另一句柄，连接并不会断，改为 `shutdown(Both)` 后通过）；助手自检 42 → **49 项**（新增：桥接描述文件解析三例**含"版本不符即整份拒绝"的阳性对照**、桥接边沿编码"空集合必须为 `-`"、**与主程序侧的路径约定一致**——只改一边的现象是"双方都在跑却永远连不上"且两边日志都不报错、**只读探测的 absent/ok/invalid 三态**（`--dry-run` 免提权即可看到，可把"两边都在跑却谁也没看见谁"这类问题前移到提权之前））；`cargo test --workspace` 全绿（含 app 侧 28 项），确认接线未破坏既有行为。**仍未验证**：真机联调（**E7，不需要设备、只需一次提权**，判据与处置已写入 `Testing/WindowsRC003EnhancedCapture.md` §7.5）与"三键端到端映射生效"；产品化（助手目前仍需用户手动启动，UI 尚未接 `rc003_bridge_snapshot()`）。设计与威胁模型详见 [docs/investigations/2026-09-23-rc003-app-bridge-transport.md](docs/investigations/2026-09-23-rc003-app-bridge-transport.md)。
  - ✅ 2026-09-23 第十一次更新（**E7 首次真机通过；同轮暴露两处：桥接的可观测性缺口 + 助手侧续约「死亡螺旋」**）：Andy 用本机双击入口 `dev-app.cmd`（本机**没有 pnpm**，见上一条）重启主程序后，诊断日志出现 `source_revision=f2f1826e` + `rc003_bridge phase=listening port=60892`，描述文件写出；跑助手后主程序侧出现 `rc003_bridge event=helper_authenticated helper_pid=11392 version=1`、助手侧出现 **`[APP-BRIDGE] event=connected`** ⇒ **E7 核心判据达成**，捕获链第 ② 段（传输）首次在真机端到端成立。**但按三键后助手侧一条 `[EDGE]` 都没有**，日志停在 `[RENEW] state=write_failed` 之后不再增长——**这不是桥接的问题**，而是助手侧**既有缺陷**（17:42 那轮已出现过同款）：续约线程写失败时只做了 `*guard = None`（清空连接句柄），**没有结束当前的 `serve_connection`**；而主循环是**串行**的（accept → serve 跑完 → 再 accept），于是 agent 重连上来的新连接只能排在 backlog 里没人 accept ⇒ 续约永远恢复不了 ⇒ 死亡螺旋（租约 2 s 过期 → 4 s 后判 `auth_mismatch` → 断开 → 重连 → 又排不上队）。**写会失败不只对端已关闭，还有"对端不再读"（写缓冲写满）**——后一种情况下 socket 并没有断，`serve_connection` 的读一切正常，所以它**不会自己退出**，这正是"只清 shared 不够"的原因。修法：续约写失败时置位 `conn_stale`，`serve_connection` 每轮检查该标志并立即收尾，把主循环还给 `accept`（判据：出现 `[CONN-STALE] …立即收尾` 后应立刻看到新的 `[ACCEPT]`/`[HELLO]` 且 `lease_ok=true`）。自检 49 → **50 项**（新增第 29 项：`deadline=None`，因此**只可能**被 conn_stale 打断；3 s 内不返回即 FAIL），并配阳性对照（有检查 500 ms 收敛 / 无检查永不收敛），证据 `evidence/conn_stale_control.out`。**顺带订正一条标签误导**：agent 侧 `dropped:auth_mismatch` 的真实判据是 `lastRenewAt === 0 && now - tConnect > AUTH_TIMEOUT_MS`，即"**连上了但一直没收到续约**"，与令牌无关（同一轮 `[HELLO] auth=true` 是通的）——看到它不要去查令牌。**另补上桥接的可观测性缺口**：`edges_applied` 原本只存在于内存快照，日志里查不到"边沿有没有真的投进映射引擎"（启动/鉴权/接管/断开都有记录，唯独投递没有）；现加 `event=first_edge`（本轮首次投递，逐条记会淹没日志）与收尾的 `edges=/released=/dropped=`。**下一步**：用新构建重启主程序 + 重跑助手（助手 release 与 app debug exe **均已重建**）；注意主程序运行时会锁住 exe，构建需先关掉它（`LNK1104`）。  ✅ **E7-b 也已通过（2026-09-23 19:11，里程碑）**：按三键后助手侧收到 **18 条 `[EDGE]`**（`back` / `volume_up` / `volume_down` 各 3 次按下 + 3 次释放，三键齐全）；主程序侧 `rc003_bridge event=first_edge pressed=0x00F1`，收尾 `event=closed reason=read_error edges=18 released=0 dropped=0` —— **边沿一条不落地全部投进了映射引擎**，无残留按下状态、白名单零丢弃；全程 `lease_ok=true`（`[CONN-STALE]` 未出现，说明死亡螺旋修复生效）。至此捕获链**三段在真机全部成立**：① 报告层捕获 → ② 桥接传输 → ③ 投递到映射引擎。**唯一剩项**：在「按键」页给某个键配一个动作、再按它，观察动作是否执行（端到端映射生效）。 **19:18 端到端映射也已通过**：在「按键」页给返回键配置「键盘 b」，按遥控器返回键后**记事本打出 b**（本轮 16 条 `[EDGE]` 全为 `back`：8 次按下 + 8 次释放；主程序侧 `first_edge pressed=0x00F1` / `closed edges=16 released=0 dropped=0`）。这是**独立于任何日志的外部观察**，也是整条链唯一的肉眼可见判据。归档证据 `hardware/RC003/evidence/rc003-bridge-e2e-2026-09-23.log`。**至此 RC003 三键在 Windows 上可用**；仍未做的是产品化（助手仍须用户手动提权启动、UI 未暴露桥接状态）、异机复跑与 RC001 不回归验证。 **20:35 桥接状态上 UI 也已验证**：主程序跑到 `source_revision=80cc6c3` 后，按键页顶部显示「三键桥接：助手已连接（端口 65169），已投递 24 条边沿」，且 **UI 的 24 与助手侧 `[EDGE]` 计数 24 逐条对上** —— 显示的是真实投递数而非占位。至此「三键按不动」的每一种原因都能在界面上直接读出（`listening` / `connected` / `failed`），不必再翻两处日志。**仍在收尾清单上的**：① 助手仍须手动提权启动（产品化最大缺口）；② 异机复跑；③ RC001 不回归验证。 **23:00 计划任务按需提权真机全通（阶段 1 验收通过）**：装任务（一次 UAC）→ `schtasks /run` 触发「已过期」任务 → 助手提权运行 → SeDebug 启用 → 注入/接管 → 桥接 → agent 认证 → 按键映射生效，用户全程只操作了两个双击入口。中途证伪并修复两个假设：①「SeDebugPrivilege 由提权隐含获得」（源码无任何启用调用，UAC 只把它放进特权列表且默认禁用，必须显式 AdjustTokenPrivileges，提交 `2cc221e`）；②计划任务触发的运行没有日志（补 `--follow-app` 默认日志，提交 `96f9253`）；另修一个自伤：.cmd 入口写了中文注释，UTF-8 被 cmd 按 GBK 误读后 rem 行被打碎、真命令根本没执行（提交 `28e2dfb`，四个入口已全部校验纯 ASCII）。归档 `hardware/RC003/evidence/rc003-scheduled-task-e2e-2026-09-23.log`。**阶段 2/3（主程序自动触发 + 按键页开关与能力说明）未做**——当前用户仍需双击 trigger-task.cmd 手动拉起。
  - ✅ 2026-09-26 更新（**开启前一次性确认弹窗 + 用户可见命名改为「全按键支持」**）：首次开启「增强捕获」（用户可见名改为**「全按键支持」**，代码标识符不变）前弹一次原生 `<dialog>` 确认（`src/components/EnhancedCaptureConfirmDialog.vue`）：说明做什么、系统会弹窗询问一次、**升级/重装无线麦后需重新开启**、**个别带防作弊的游戏可能冲突**（按键失灵/游戏打不开，玩此类游戏前建议关闭）、只读按键不收集数据；文案全程无内部术语（HID 宿主/UAC/计划任务/注入一律不出现，2026-09-26 Andy 定稿）。只在**首次**开启弹一次（localStorage `sayall.enhancedCapture.confirmShown`），关闭方向永不弹；开关悬停提示与按键页能力说明同步去术语化。测试 `src/pages/ButtonsPage.test.ts` 新增 2 例并修复「mockClear 不清实现」的渗漏（22/22 passed），全前端 96/96 passed，`vue-tsc` + `vite build` 通过。**弹窗 UI 未真机验收**（需出包后现场确认）。


### 2026-09-23 三键免驱动通道调研（RC003）

- **四条免驱动通道全部实测 `failed`，RC003 三键的免驱动捕获在结构上不成立**（真机 2026-09-23，RC003 在线）：
  ① 厂商 GATT 服务 `8A7A0001` 的 3 个通知特征订阅成功但 240 s 内零通知（电池特征同窗口有通知，证明回调链路通）；
  ② GATT HID 服务 `0x1812` 特征枚举 8 次全 `AccessDenied`，`0x2A4B` 报告描述符因此拿不到；
  ③ 用户态 `CreateFile` 直读 HID 顶层集合被拒（err=5）——阳性对照 8/17 接口可打开且**全部是 Shared 访问模式**，9 个拒绝全是 Exclusive 的键盘类，与微软 HID 架构文档的访问模式表逐条吻合；
  ④ Raw Input 注册 5 个组合（含从未注册过的厂商页 `0xFF00/0x0001`、`0xFF00/0x0002`）后，返回/音量± **零事件**，而同一采集里 确定 → `VK_RETURN`×2、主页 → `VK_HOME`×1 按次数精确到达。
- **本轮同时修正了一个归因错误**：三键"不可见"**不是设备不上报**。零权限打开 TLC 后用 `HidP_GetButtonCaps` 取到设备**声明**的 usage——`0x80`(音量+)/`0x81`(音量-)/`0xF1`(返回) 明确落在键盘页 `report_id=0x01` 的 `0x0000–0x00FE` 范围内，与能正常工作的 Home(`0x4A`)/确定(`0x28`) 同处一个报告。真正的原因是 **Windows 的 HID→VK 映射表没有这三个 usage**，`kbdhid` 在映射阶段丢弃。
- 因此原写"**路线 B（`kbdhid` 之下的按设备捕获）是唯一可行形态**"——**该"唯一"已由 2026-09-23 二次更正推翻**：上述四条只证明"**不提权、不注入**"拿不到三键，提权注入 HID 宿主（路线 A）从未被测且同样不装内核驱动。当前决策点：路线 A / 路线 B / 维持不可用。
- 证据与探针见 [docs/investigations/2026-09-23-rc003-driverless-capture-investigation.md](docs/investigations/2026-09-23-rc003-driverless-capture-investigation.md)。
- 残余不确定项（均 `deferred`，不影响上述判定）：厂商 write-nr 特征是否需"使能握手"后才推通知（不写入以免触发 OTA/重置）；提权或 `SeTcbPrivilege` 能否读 TLC（即使可行也违反产品约束，故未测）。

### 2026-09-23 参考实现复核（RC003，路线 A 未被证伪）

- 按用户要求复核 `ZSTDJan/windows-remote-mic-app`（提交 `1e6b1d285f9cd50f30c5bc92ac7787a693fc993d`，v1.0.44），确认其三键捕获**不经过 Raw Input**，而是在 `WUDFHost.exe` 内挂 `ntdll!NtDeviceIoControlFile`，命中 IOCTL `0x80018483`（in 8 / out 9）后在 `onEnter` 清空 9 字节键盘报告偏移 3 起的 6 字节 usage 槽；技术上游为 `xxb26553663-star/remote-bridge-hub`。
- **本机只读实测（`structural`）**：RC003 的 HID 由 `mshidumdf` + `WUDFRd`（`HidOverGatt`）承载于 `WUDFHost.exe`，`Device Parameters\WUDFDiagnosticInfo\HostPid` 可读且为**独占 RC003** 宿主；普通权限 `OpenProcess` 返回 `err=5`（宿主在 session 0）。证据 `hardware/RC003/evidence/wudf-host-probe.log`、探针 `hardware/RC003/probes/wudf-host-probe.py`。
- **代价更正**：路线 A 不需要内核驱动、`TESTSIGNING`、关闭 Secure Boot、重启或 WHCP 签名；代价是提权助手 + 第三方注入框架（Frida Gadget，SHA256 固定）。~~与 ADR 0002 §3「不使用 Frida」冲突，需重开 ADR 或改为自研注入组件。~~ → **2026-09-23 已解除**：ADR 0002 §3 修订为"允许注入框架，但须固定版本 + 校验 SHA256 + 登记来源与许可"，R5 口径同步收窄。
- ~~**下一步（需一次提权，不需重启、不改系统设置）**：先做**只读**最小注入实验……~~ → **已于 2026-09-23 执行，判定 `passed`**，见下节。

### 2026-09-23 HID 宿主报告层只读实证（RC003，`passed`）

- **判定：`three_keys_present_in_report`（`passed`）**。本机 RC003 的 9 字节键盘报告里**确实存在**目标三键，
  与参考实现的判据**逐条吻合**：`IOCTL 0x80018483`、`in_len=8`、`out_len=9`、
  输入第 5/6 字节（operation/selector）= `02/01`、`report_id=0x01`，30/30 命中全部一致。
- **原始计数**（47 s 采集窗口，`pre` 15 s / `A` 阳性对照 12 s / `B` 目标三键 16 s / `post` 4 s）：
  `0x0028` 确定 ×4、`0x004A` 主页 ×2（阳性对照）；**`0x00F1` 返回 ×3、`0x0080` 音量+ ×3、`0x0081` 音量− ×3**。
  每次按键 = 按下报告（带 usage）+ 抬起中性报告（usage 全 0），边沿计数与用户实际按键一致。
- **因果链闭合**：设备上报（本次实测）→ 报告到达 `WUDFHost.exe` 层（本次实测）→ Windows 事件层零事件
  （`raw-input-dump2.log`）⇒ 丢弃发生在 **HID→VK 键码映射（`kbdhid`）**。此前是推断链，现在是实测。
- **附带证实参考实现的时序结论**：30 次命中中 `onLeave` 输出缓冲区变化 **0/30**——报告在 `onEnter`
  就已完整就位、内核在调用期间不改写它，因此"清空必须发生在 `onEnter`"在本机成立。
- **成本账**：一次**交互式 UAC 提权**；**无内核驱动、无 `TESTSIGNING`、不改 Secure Boot、未重启、
  未改任何系统设置、未装计划任务**；注入的探针全程只读（不写目标缓冲区、不 `SendInput`）。
- **边界（仍未验）**：① ~~写入路径（`onEnter` 清零 + 应用侧重映射）未执行~~ → **2026-09-23 写入版实验已实测通过
  （7/7 PASS，`interception_effective`）；尚未验的是清零后的**应用侧重映射**整链**；
  ② 边沿配对 / 接管就绪门 /
  租约 / tap 失联时"全部自定义映射停用"等 fail-safe 合同未验；③ 异机复跑（本机 Windows 11 25H2
  Build 26200.7171，全链版本已记录）；④ 共享宿主路径；⑤ 更严格 EDR/HVCI 策略下的现场行为；⑥ RC001 不可外推。
- **探针工程坑（可复用，已写入结果文档 §5）**：① **session 0 目标上 `post()/recv()` 反向通道只送达第一条消息**
  （阶段标签停在 `pre`、收尾汇总为空），而同一次运行的 `send()` 全程正常；自检显示 `post/recv` 在
  spawn/attach 下对普通进程均可用 → 已改为**单向 `send()` + 每条记录自带时间戳 + 判定从原始记录重算**；
  ② **并行 agent（`codex.exe`）会覆盖同一 worktree 的文件**，探针改完必须回读校验；
  ③ **提权动作只能由用户本人执行**（agent 代提权被安全策略拦截），agent 负责准备入口、读日志与判定。
- 报告：[docs/investigations/2026-09-23-rc003-hid-host-readonly-tap-result.md](docs/investigations/2026-09-23-rc003-hid-host-readonly-tap-result.md)；
  日志：[hardware/RC003/evidence/wudf-ioctl-tap.log](hardware/RC003/evidence/wudf-ioctl-tap.log)；
  探针：`hardware/RC003/probes/wudf_ioctl_tap.{js,py}` + `run-wudf-ioctl-tap.{ps1,cmd}`。

### 2026-09-23 写入（拦截）最小实验（**实测完成：7/7 通过，判定 `interception_effective`**）

- **要验证的命题**：在 `onEnter` 写入报告缓冲区的字节，**是否被 Windows 的键码翻译采纳**。
  这决定"报告层清空 + 应用侧重映射"能否落地。
- **装置**：`hardware/RC003/probes/wudf_ioctl_write.{js,py}` + `raw_input_sink.py` + 提权启动器
  （全程 `run-wudf-ioctl-write.{ps1,cmd}`、补采 `run-wudf-ioctl-write-b2.{ps1,cmd}`）；
  判定可离线复算（`--analyze`，支持多份日志合并，不需提权、不需设备在线）。
- **方法要害（两条独立通道 + 同键对照）**：
  ① 宿主侧记录改写（只动偏移 3..8，`onLeave` 回写复原，`kernel_changed` 如实上报）；
  ② Windows 侧 Raw Input 后台监听按 `hDevice` 做设备归属，只统计 RC003 事件；
  ③ 阶段 `A`（只观察，按确定）与 `D`（全清，按确定）是**同一个键**的对照——A 有 `VK_RETURN`、D 没有，
  唯一差别是我们写了字节 ⇒ 擦除生效（单独看 D 的"零事件"没有信息量，目标三键基线本来就是零事件）；
  ④ `B1`（→ `0x0028`）/`B2`（→ `0x0068`=F13）是正向证据：目标键改写前零事件、改写后出现按键 ⇒ 写入发生在翻译之前；
  ⑤ `C` 验证选择性擦除不波及非目标键；`E` 验证解除武装后恢复。
- **契约**：一次交互式 UAC 提权；不装内核驱动、不改 Secure Boot / 测试签名 / 驱动签名策略、不写注册表、
  不装计划任务、不需重启；计划表走完 + 3 秒宽限期后永久解除武装，写入次数上限 400。
- **实测（两轮合并判定为 `interception_effective`，退出码 0）**：
  - **首轮全程（2026-09-23 11:33，`evidence/wudf-ioctl-write.log`）**：五项通过。
    `A` 阳性对照（确定 → `VK_RETURN`×2）、`B1` 改写生效（4 次改写 → `VK_RETURN`×4，一一对应）、
    `C` 目标键报告层已擦除 + 非目标键未波及、`D` 擦除生效（确定 被静默，同键对照成立）、
    `E` 停止写入后恢复；`B2` 窗口内宿主侧命中 **0 次**（钩子存活——紧随的 `C` 立刻恢复到命中 10 次）
    ⇒ **采集缺失**，非机制失败。写入失败 0、`onLeave` 复原 10、`kernel_changed` 0；计划表 69000 ms、改写 10 次。
  - **补采 `B2`（2026-09-23 11:51，`evidence/wudf-ioctl-write-b2.log`）**：**PASS**。
    4 次改写（返回/音量+/音量− → `0x0068`）→ Windows 侧 **`VK_F13`×4**；该窗口内宿主侧 8 次命中
    全部在本探针的改写之下（4 条按下报告被改写 + 4 条抬起报告 usage 全零），**不存在**未被改写的
    目标键报告 ⇒ 结论不依赖"操作者是否按对键"。计划表 22000 ms、改写 4 次。
  - **合并命令**：`--analyze evidence/wudf-ioctl-write.log evidence/wudf-ioctl-write-b2.log`。
- **`B1` 的推理在日志内闭合**：4 次改写的 `orig` 全是目标三键，期间**没有任何**未被改写的 `0x0028` 报告
  ⇒ 那 4 次 `VK_RETURN` 只能来自我们写入的字节，写入发生在键码翻译之前。
- **指纹级旁证（扫描码）**：Raw Input 的 `MakeCode` 显示 Windows 用**改写后的 usage 重新推导**了
  整条 `HID usage → (VK, 扫描码)`——原生/改写 确定 均 `vk=0x0D make=0x1C`，改写 F13 得
  `vk=0x7C make=0x64`（与 `MapVirtualKeyW(VK_F13)=0x64` 吻合），而 `SendInput` 注入的事件 `make=0x00`。
  ⇒ 不是"顺带读到"，而是在翻译链最上游被消费；且替换键会得到**正常扫描码**，对按物理键位/扫描码
  响应的应用同样有效。
- **顺带成立的产品约束**：`0x0068` 可作替换键（未触发退出码 13），但**不可推广**——
  其它替换 usage 仍须逐键实测，判定器已把该约束编码为 `13 substitute_not_mapped`。
- **本轮修掉的两处装置缺陷**（均已修，详见结果文档 §4.8）：
  - 判定层：对 `B2` 报 `FAIL` 却**仍返回 0**（只在关键链短路，对非关键断言失败无反馈）。
    已修为：只对实际执行过的阶段判定（未执行标 `SKIP`）、从日志「阶段时间轴」段解析执行范围
    （唯一权威来源）、`--analyze` 支持多日志合并、退出码区分 `12 phase_not_observed`（采集缺失，
    可补采）与 `13 substitute_not_mapped`（技术结论）。分析层自检扩到六例（`0/8/10/0/12/13` 全符合期望）。
  - 收尾覆盖缺口：两轮日志的「解除武装=False」恒为 False——agent 只在「计划表总时长 + 宽限期 3 秒」
    之后才置真，而收尾只等 1.5 秒。「计划表走完自动停写」这条 fail-safe 因此**一次都没被验证过**。
    已修：Python 侧新增与 JS 对齐的 `GRACE_SECONDS`，收尾等过宽限期再取统计，仍未置真则主动提示。
    （两份既有日志产自修复前；其 False **不代表残留**——步骤 6/6 随后 detach 卸载整个注入。）
  - 运行层（上轮已加）：`--phases A,B2` 补采短跑 + 阶段内**零命中即时预警**。
- **剩余门槛（机制侧已无阻塞）**（R6 类）：边沿配对 / 接管就绪门 / 租约 / tap 失联 fail-safe；
  以及异机复跑、共享宿主、严格 EDR/HVCI、RC001 适配（RC001 走厂商键 VK `0xFF`，不可外推）。

### 2026-09-23 约束修订（ADR 0002 §3 与 R5）

- **ADR 0002 §3**：删除「不使用 Frida」。新口径：允许注入框架（含 Frida Gadget）作为增强轨的脚本载体，
  但须**固定版本 + 校验 SHA256 + 登记来源与许可**；仍不得继承来源不明或未审计的第三方二进制。
  详见 [docs/decisions/0002-dual-track-injection-optional-helper.md](docs/decisions/0002-dual-track-injection-optional-helper.md) 的修订记录。
- **R5**（路线选型文档 §1）口径收窄为"不引入**不可逆或全局性**的系统级改动"：仍禁内核驱动 / Secure Boot /
  测试签名 / 驱动签名策略 / 注册表过滤项 / 重启；**不再禁**显式安装、可逆卸载、经用户一次性授权的提权 Helper
  与固定计划任务。
- **`AGENTS.md`**：基础路径条款去掉「Frida」；新增"HID 宿主内报告层捕获属可选 Helper 增强轨"；
  并澄清"第三方应用边界"三条的禁止对象是**第三方 App 进程**，系统 HID 宿主（`WUDFHost.exe`）内的
  按设备报告层捕获不在此列。

### 2026-09-05 附加

- **应用内更新（tauri-plugin-updater + GitHub Releases，新增）**：关于页"检查更新"手动入口 + 启动静默检查（失败完全无声）+ 下载进度 + passive 安装自动重启；默认稳定通道使用 `releases/latest/download/latest.json`，用户可显式开启“检查预览版更新”，经 GitHub Releases Atom feed 选择最高 SemVer 的已发布版本（包含 Pre-release）；开关默认关闭并持久化，两个通道均由 minisign 强制验签。安装器启动前经 `on_before_exit` 显式断开 BLE 链路（插件在 Windows 上 `std::process::exit(0)` 不走 Drop 清理）。待完成边界：① 已安装 0.2.1 不含预览通道开关，无法自行发现 Pre-release，0.2.2 首次引导需单独处理；② GitHub Secret `TAURI_SIGNING_PRIVATE_KEY` 未配置时 CI 用一次性密钥兜底、正式 Release workflow 直接失败；③ Authenticode 代码签名仍待建立（updater minisign 验签独立于 Authenticode）；④ 大陆访问 GitHub 的网络可用性未量化（插件支持多端点兜底与系统代理，已留扩展位）。参考与源码核对记录见 ATTRIBUTION.md 更新调研节。
- **BLE 僵死链路自动恢复（bluetooth_radio.rs，新增）**：应用被强杀后 OS 侧 GATT/HID 链路或服务缓存可能僵死，普通重试永不恢复。重连循环连续失败 5 次后自动执行 Off→2s→On；每窗口最多 2 次，之后冷却 60 秒并自动开启下一窗口，既防抖又不永久停止自愈。2026-09-12 按微软文档补 `RequestAccessAsync` + Allowed 检查 + Off/On 有界状态确认；2026-09-13 再补 Tauri UI setup 阶段预先取得权限并缓存 Radio 对象，使系统稍后进入 `0x80070008` 时无需重新枚举即可恢复，连接恢复后也会补建缓存；同时以 `PendingBleConnection` 保证服务/特征发现和订阅任一步失败都显式回滚已取得的 WinRT BLE 资源，防止自愈重试反过来扩大资源耗尽。2026-09-14 现场进一步证明参考实现 `FromIdAsync`、直接 GATT selector 和 Win32 GATT 均无法穿透已经僵死的内核蓝牙栈；新增仅在 `0x80070008`/`0x80004004` 且 Radio 路径失败时触发的 PnP 兜底：SetupAPI 精确定位唯一 `BTHUSB` 设备节点，使用系统 `pnputil /restart-device` 请求 UAC 后重启，并以 WinRT Radio 重新枚举作为成功判据。现场恢复测试 1.95s passed，随后设备对象创建恢复；会话 Close 的 service/device 失败也改为后续调用真正重试。完整本地包启动验证待本次交付，RC001/RC003 各自制造僵死后的自动连接仍 deferred。详见 Bugs/2026-09-07-ble-unreachable-both-remotes.md、Testing/WindowsBleResourceRecovery.md 与 ATTRIBUTION.md BLE 恢复调研来源。
- 语音键 F5 抑制器补防粘键配对（VVC 同款"DOWN 漏进 OS 则 UP 必放行"）：按下沿 60ms 有界等待超时泄漏时，释放沿放行，杜绝"F5 粘住→和弦全部被拒"的整机失效模式。

### 2026-09-05 IME 专项

- **语音"无法唤起"根因 = 会话活动输入法不是微信输入法**（WeType 语音热键仅在自身活跃时生效；焦点无关，桌面/资源管理器聚焦 6/6 照常开麦）。修复（ime.rs）：注入和弦前用公开 TSF API 会话级激活 WeType（TF_IPPMF_FORSESSION，零延迟 3/3 实证），失败不阻断。参考 macOS 版 PreferredInputSourceMonitor 职责设计。

### 2026-09-05 性能已知项（评估归档，供后续修复）

背景：首按失败根因修复（80ms 回退 + F5 三重防线，PR #19）验证通过后，对语音链路做整体性能评估。当前全链延迟 ~300ms（按键→开麦），其中外部因素 ~200ms。逐项账目与处置边界如下，**勿盲改**——每一条都有实测依据。

**外部边界（第三方/固件，不可干预，勿再投入）**：

- WeType 内部识别和弦→开麦固定 ~163ms（evidence/p 13 次实测 ±5ms，端点预热无效）。
- BLE/固件按键→0x04 通知 ~30-60ms（0x04 早于 HID F5 60-90ms，触发点已最早合法位置）。

**正确性取舍（"延迟优化必须保证成功率"规则项，勿回退）**：

- 和弦间隔 80ms：20ms（cef24d3）冷/节流态必失败（2026-09-05 用户实证 7 次发作），86e5314 回退。热态验证 4/4 不代表可交付。
- F5 解粘 20ms（和弦前保险 UP + 间隔）：跨应用重启的 OS 粘键状态无法便宜检测，须无条件执行。

**待修复项（按优先级）**：

- [ ] **笔记本功耗：后台节流豁免改为条件化**（bf03f0e 当前全局豁免——它是首按修复的组成部分，全局豁免代价是闲置功耗略高）。方向：仅在遥控器连接期间豁免，断连后恢复参与节流；重连可靠性已由 WakeReconnect + 无线电自愈兜底。台式机无影响；上笔记本场景前处理。
- [ ] **失败恢复加速（可选）**：wetype_check 检测判据从 ConsentStore 注册表（700ms 检查窗）换 LL 钩子 0xFC 标记观察（毫秒级，kb-live 已验证与开麦 100% 交叉一致）。仅加速失败路径的重试触发，成功路径零收益；需抑制器/钩子层新增观察通道，注意钩子线程不做 IO。
- [ ] **冷态管道 ~120ms（低优先级）**：闲置后首按应用内部链路（GATT 回调→武装→工作线程→IME 查询→和弦）实测可拖 ~120ms（不失败但慢）。节流豁免已生效仍有首次线程调度延迟；武装已内联到 GATT 回调。进一步压缩收益 ~100ms 冷态延迟，风险中（动的是刚修好的链路），无用户报障不动。

### 2026-09-07 按键映射单响应专项

- **冷首按原生残留（2026-09-08 用户调整策略）**：武装族按键（确定/方向）在闲置 >4s 后的首次按压会附带一次原生按键动作；同键映射由泄漏对冲保证净单响应，不同键映射仍会同时出现原生动作与配置动作。左键已恢复自定义，与上/下/右/确定使用相同的逐键 4s 武装机制。Home/TV 继续采用方案 C"遥控器优先"常驻抑制；零代价终局仍是 Helper 轨（ADR 0002）。
- **返回/音量±全型号禁用（2026-09-07 用户决策，真机验证时确认）**：此前 RC001 上三键以 VK 0xFF 厂商键可达且可直接归因（可正常映射），RC003 上输入栈不可见（格子禁用）——两型号行为不一致造成用户困惑（连 RC001 时格子放开）。决策：统一全型号禁用（RC001 也不开放），UI 格子禁用 + 持久化层/引擎层双重剥离；电源/菜单保持可配（直接归因且用户未要求禁用）。
  - ⚠️ **已被 2026-09-23 产品策略变更取代**：该禁用策略已撤销，三键恢复可配置（见上方"返回键、音量加、音量减"两条条目）。本条目仅作历史记录保留。
- RC003 按键映射真机验收 passed（2026-09-07 01:13–01:15 用户全键测试，remote-capture 逐事件比对：同键映射×4 两路径单响应、菜单直接归因、TV/主页 open_app 生效、物理键盘无劫持）。
- RC001 按键映射真机验收 passed（2026-09-07 下午，用户真机验证 c70767d 构建确认：返回/音量±/左键三键禁用策略符合设计、Home/TV 遥控器优先严格单响应含闲置首按、语音链路无回归）——两型号按键映射验收均已通过，详见调查档案"RC001 真机验收记录"节。
