# 来源与归属

本仓库是面向 Windows 的 Rust/Tauri 工程。

## 治理规范迁移

- `HD838A/remote-mic-app`，提交 `b233a88cc4457b00413dda6b37ec8b4af12c5121`：迁移其平台无关的分支/提交纪律、日志脱敏与完整链路记录、Bug 复现取证顺序、测试手册要求、发布来源可追溯和资产不可变原则；本仓库将其改写为 Windows/RC001/RC003、Tauri/NSIS、updater minisign 与 Authenticode 边界。
- 有意排除：Swift/SwiftPM、CoreBluetooth、AppKit/SwiftUI、Developer ID/Apple 公证、Sparkle、DMG/PKG、Apple Team ID、macOS/iOS/Web 专属流程，以及任何 macOS 私有路径或凭据。
- 迁移文档：`LOGGING.md`、`RELEASING.md`、`TECHNICAL.md`、`TROUBLESHOOTING.md`、`Bugs/README.md` 与 `Testing/WindowsRelease*.md`。这些文件记录的是规范与经验，不复制参考仓库业务代码。

## App Logo 版权

- App Logo 与 App Icon 沿用 `HD838A/remote-mic-app` 的版权边界：属于 HD838A 保留版权的专有品牌资产，不纳入 GPL-3.0-only；Windows 版适用范围和授权条件见 [LOGO-LICENSE.md](LOGO-LICENSE.md)。

## 产品与 UI 基准

- `HD838A/remote-mic-app`：无线麦 macOS 原版的信息架构、产品文案、RC003 图片、RC001/RC003 型号识别、ATVV 行为和测试边界；RC001 支持参考提交 `b233a88cc4457b00413dda6b37ec8b4af12c5121`。
  - 2026-09-05 按键映射功能移植补充（均为语义移植，非代码复制）：`RemoteButtonGestureRecognizer` + `HIDRemoteScheduler` 的手势参数（双击窗口 300ms、长按 550ms、连发起始 350ms、返回 50ms/方向与音量 100ms 连发）与"按配置动态启用双击/长按识别、未配置时单击零延迟"的语义；`KeyboardEventSuppressor` 的预测式武装 + 有限窗口匹配吞键模型；`RemoteMappingCanvas` 的按键卡片布局表（锚点/目标 Y 坐标逐键移植）与三态高亮（按下=橙、选中=强调、普通=中性）；`MappingSelectionPolicy` 的"锁定当前按键"默认值。Mac 版 `KeyboardEventSuppressor` 的 UP 沿无配对兜底（DOWN 泄漏+UP 吞下=粘键缺陷）未移植——Windows 版沿用本仓库 2026-09-05 规则（DOWN 漏进 OS 则 UP 必放行）。
  - 2026-09-09 按键映射配置导入导出补充（本地 Mac 仓库 HEAD `feba1d6`，语义参考，未复制代码）：参考 `AppSettings.exportedConfigurationData/importConfiguration` 的版本化 JSON、导入前完整解码校验与一次性应用，以及 `SettingsView.exportConfiguration/importConfiguration` 的系统文件选择器、用户取消静默、成功/失败反馈。Windows 版仅迁移按键映射，不导入 Mac 专属设置或统计；格式使用独立 `formatVersion: 1` + `buttonMappings` 契约，不宣称与 Mac 配置文件互通。
- RC003 图片 SHA-256：`658d9333853958c13ff721eb76e1a6816c1dbea16006a84e8577ad410812549f`。

## Windows 行为与测试参考

- **LL 吞键对 Raw Input 交付影响的本机实证（2026-09-05，`docs/investigations/2026-09-05-ll-swallow-vs-raw-input.md`）**：双线程探针（钩子线程 + Raw Input INPUTSINK 线程分离，key_suppressor 同构）两轮一致证实 **WH_KEYBOARD_LL 返回 1 吞掉的键盘事件不会再投递 WM_INPUT**——按键映射门控（`key_gate.rs`）据此采用"被吞键盘边沿由钩子线程直接喂引擎 + 监听器喂 HID 报文与透传键盘事件"双源合并架构；HID 报文归因武装 + 60ms 有界等待沿用 key_suppressor 实证参数。

- `HD838A/remote-mic-app#249`，提交 `090a3cfc24f0e3e733b2347ee2daf87c60e10097`：Windows 独立实现、ATVV 测试夹具、语音边沿、安装升级、公开边界和 Mac 风格 UI 原型；Raw Input 参考了 `hid_identity.py` 与 `raw_input_windows.py`，SendInput 的批量提交、物理修饰键和失败回滚参考了 `win32_input.py` 与 `win32_keys.py`，均以 Rust/windows-rs 重新实现。
- `GetSayAll/hardware-simulation`，提交 `65248499cac7da3ad46cd0c11dca1478f7733255`：RC001 短语音时间线的控制通知、40 + 80 字节音频拆包和停止通知；本仓库只保留纯 ATVV 回放所需字段。
- `ZSTDJan/windows-remote-mic-app`：WinRT BLE、Raw Input、音频输出、发布门禁和真实硬件验证边界；其语音页按语音程序配置"按住说话快捷键"、按下注入 DOWN/松开释放的行为，是本仓库按住说话快捷键设置的产品参考。Round 1 拆解曾记两项技巧参考，后续实证修正（Round 2/3）：**physicalize 技巧——结构性无效（勿模仿）**：`legacy_key_suppressor_windows.py` L142-155 的做法（仅对自家 keybd_event 注入的带 "RMICRC03" 标记右 Alt，在自家钩子的私有副本上清 INJECTED 标志→转发→恢复）曾被解读为"使下游应用钩子视为物理键，前提是自家钩子位于目标应用钩子之前（链头）"——该解读不成立（Round 2 E 三层实证：LL 钩子每钩子收到私有结构副本，修改不跨钩子传播，CallNextHookEx 转发通道不存在，应用层收到原始键；Round 3 J 语义复查：清标志对下游钩子/应用层均不可见，且 ZSTDJan 进程内也无读者——对声明目标是 no-op；其真正能影响豆包读值的是 `doubao_rpc.py` 的 Frida 版 attach 方案，未接线进生产流程，违反本仓库 A2/A4/A5 边界，仅作机理记录）。**WeType 语音触发配方——本机实证有效（Round 3 J 翻案）**：SendInput 注入 Ctrl+Win 按住（纯 wVk 或扫描码配方均可）可唤起 WeType 语音（会话级 TSF 激活前提下：开麦/吞键/释放关麦全链实证，注入 ground truth 由常驻捕获器独立记录）；**Round 2 F 曾判"三配方无反应"，系其 TSF 激活用了线程级 flags（dwFlags=0，会话级应为 TF_IPPMF_FORSESSION=0x20000000）、WeType 从未真正激活所致——教训：测试 IME 行为前必须以会话级激活 + 行为判据（候选框版式）双重确认活动输入法**。**配方形态约束（2026-09-04 P 实证，evidence/p）：和弦必须逐事件注入且两键间隔 ≥80ms——WeType 拒绝单次 SendInput 批量零间隔提交的 Ctrl+Win（sent=2/2 全到达仍无吞键无开麦；逐事件 80ms 两轮 2/2 触发，A 失败→B 通过→A 失败→B 通过交替序列排除状态漂移）**；应用曾把该配方误合并为单批零间隔导致真机不出字（Bugs\2026-09-04-wetype-zero-gap-injection.md，含第二层缺陷：遥控器 F5 须由抑制器吞掉，否则"额外按键"拒绝；钩子链头 bump 加固同日落地），已修复并 RC001 真机端到端 passed（2026-09-04，用户确认文字上屏）。
- `richlearntodo-debug/vibe-flow`，提交 `047f9d3ead54bf30de9b884adf8f7b5adefe9993`：自然 ATVV 会话、WASAPI 音频生命周期和硬件验收清单。
  - **Windows 深色模式专项调研补充（2026-09-08，本地参考库 HEAD `b47f7cdce8b753fade0c64c97332bebe80f17d2d`；主应用 UI 源码未开源，依据为 `docs/ARCHITECTURE.md`、`docs/PRODUCT_AUDIT_2026-09-01_ZH.md` 与用户指南）**：其产品支持浅色、深色、跟随 Windows 三档且运行中切换不重启 Host/Bridge/Capture；审计结论要求深色采用低饱和中性色层级，并完成各页实际截图检查。本仓库只借鉴“三档主题、主题是纯显示行为、不得重启后台服务”和视觉验收边界，不复制实现；SayAll 将选择器放在“关于”页面，并通过自身 `SettingsStore` 持久化，详见 `docs/plan/2026-09-08-windows-dark-mode.md`。
  - **按键映射/双响应专项调研补充（2026-09-07，本地参考库 `Documents\Codex\reference-repos\vibe-flow`，HEAD `b47f7cdce8b753fade0c64c97332bebe80f17d2d`；主应用源码未开源，依据为其文档 + `scripts/VoxDeckInputBridge.cs` + `driver/rc003-filter`）**：
    1. **用户态"拦截↔设备身份互斥"独立复证**：其 V1.3 根因报告（`docs/V1_3_INPUT_ROUTING_ROOT_CAUSE_ZH.md`）实测——LL 钩子拦截 → Windows 不投递对应 WM_INPUT → Raw Input 拿不到 RC003 设备身份 → 动作永不执行（旧候选日志：钩子暂存 166 边沿/真正到达 Raw Input 4/配对 0/实体路由 0）。与本仓库 2026-09-05 `ll-swallow-vs-raw-input` 实证同结论，两库独立互证；本仓库以 GATT 前信号（0x04 早于 HID 60-90ms）做武装归因，不受此陷阱影响，为同类实现中结构更优。
    2. **其用户态发布版（V1.5）的答案=接受共存**：钩子对非语音映射键一律放行（不拦截、也不暂存回放），Raw Input（INPUTSINK + 设备句柄指纹）负责设备归因与动作执行，明确接受"遥控器原始键系统效果与配置动作同时发生"，文档要求"不应在 UI 或发布说明中描述为精确拦截"；默认 Profile 全部映射=该键原生效果（上→上、确认→Enter）使共存不可见，非默认 Profile（如左→browserback、上→Ctrl+Z）实际存在与本仓库同款的"原生+注入"双响应。语音键（RC003 固件形态=F5）是唯一在钩子层无条件按 VK 抑制的键（物理键盘 F5 冲突被接受，或交由驱动路径解决）。
    3. **唯一彻底解=KMDF per-device upper filter（候选未发布）**：INF 精确绑定 VID 0x2717/PID 0x32B8（不做键盘类过滤器，普通键盘零影响），按扫描码位图抑制 + 全边沿环形队列入队上抛用户态；250ms 心跳、2s 超时 fail-open 全放行、策略 generation 变更清队列防陈旧事件、控制句柄关闭即解除抑制。与本项目 ADR 0002/Helper 轨定位同构；其 `driver/rc003-filter/README.md` 的 10 项发布门禁（SDV/HLK/微软签名/Secure Boot+内存完整性/卸载回滚/万包压测）可作 Helper 轨验收清单参考。
    4. **已退役路线警示**：独占 GATT 抢占 HID 服务/强制禁用 HID 子设备 → Windows 将键盘子设备判为 critical，`/force` 禁用成 reboot-pending 状态而非安全热交接——本仓库 GATT 归因为并行订阅（不禁用系统 HID 栈），勿走独占抢占路线。
    5. **互证数据点**：RC003 返回/音量±/电源键在钩子/键盘 Raw Input/Consumer Raw Input 全通道不可见（HID GATT 0x1812 特征 AccessDenied；厂商服务 8a7a0001-… 的 Notify 无按键事件；Frida 旁路 WUDFHost 监听 IOCTL 无捕获）→ 其结论"硬件能力缺失给诊断、不宣称映射成功"，与本仓库 RC003 返回/音量±格子禁用同构；WeType 配方 Ctrl+Win/toggle/80ms、语音键=F5、"重连后扫描码偶变→持久语音映射保持权威"均与本仓库实测一致或互补。
    6. **工程细节参考**：RawKeyboardEdgeTracker（keysDown 集合按扫描码身份 add/remove，防钩子/Raw Input/驱动多源双触发）；长按 650ms、连发起始 420ms/间隔 80ms；TV=Win+Tab 任务视图且"方向键仅任务视图激活期间执行映射动作"（拥抱原生效果而非对抗）；动作执行回执（真实 SendInput 结果而非排队即成功）。
- `mwlt/Voice_VibeCoding`，提交 `c89410aed3b274fee5e571128b82c9c6e6689715`：Rust/Tauri 模块划分、windows-rs API、音频生命周期和托盘窗口工程经验；其语音键按住注入的 Hold 语义（按下先快捷键 DOWN、松手统一释放、SendInput 互斥降级）是本仓库按住说话快捷键注入时序的参考。Round 1 拆解补充其 **LL 钩子吞键工程细节**（本仓库吞键层设计的参考，非逐行复用）：时序窗吞键（音量 recent 200ms、back/home/menu/tv/power 250ms、方向/OK 200ms 或 tap_ready+自定义位图）、钩子链头 bump（重叠安装：先挂新钩再卸旧钩，消除 LL 吞键空窗）、F5 语音键状态机（sticky/correlate 120ms/tail 3s；DOWN 漏进 OS 则 UP 必放行，防粘键）、音量防双格（Tap 转发 + SendInput VK_VOLUME_* + 200ms 吞固件残留）、Alt 和弦用 SendMessageTimeoutW 直发前台避免系统菜单、自家注入放行（EXTRA_INFO 标记或 INJECTED→CallNextHookEx），及 bump 空窗/sticky 粘键/60ms 去抖门等已踩坑清单。本仓库只使用 SendInput 公共 API，不引入其 WinUHid 虚拟键盘驱动。
- `cgutman/WinUHid`（MIT 许可）：用户态 UMDF 虚拟 HID 键盘/鼠标驱动框架（C++/Win32），无预编译 Release，需自建并签名后使用；ADR 0002 增强轨驱动来源的第一候选（须先审计）。签名成本调研结论（**已闭合，2026-09-04**：UMDF 分发不需硬件计划/EV，OV 级 catalog 签名为最低门槛——三层官方原文支撑，`docs\investigations\evidence\g\signing-policy.md`；残余含混=无单句官方原文直书此结论，装机实测 deferred（调查护栏限制））记录于 `docs\investigations\2026-09-04-avoid-driver-signing-input-paths.md`。未经审计的 WinUHid 二进制不进入仓库。
- `QL-4/RemoteMapper`，main 提交 `25ca0c13cf2ff2caf7caae3d9690f9629b7c0df0`（另有 `driverless-keymap` 分支）：小米蓝牙遥控器 → 微信输入法（WeType）语音录入的完整端到端先例——按住语音键唤起 WeType 录入并送入音频，松开结束录入并恢复系统原默认麦克风。可借鉴结论：(1) **双分支分层**：`main` 含 KMDF HID lower filter（需 TESTSIGNING），`driverless-keymap` 无驱动直接交付、但缺返回/音量±三个键（被 kbdhid.sys 丢弃）且 LL 钩子映射会误吞物理键盘同名键——印证本仓库"基础路径免驱动 + 增强轨驱动"分层与"LL 钩子无来源设备 ID"的既有判断；(2) **MiRemoteHidFilter 驱动做法**：extension INF 精确绑定 VID 0x2717 / PID 0x32B8（不匹配其他键盘），修复 kbdhid.sys 丢弃的 usage 0x80/0x81/0xF1，并把普通键改写为 F13–F19、语音键 HID F5 改写为 F20，从源头规避误吞物理键盘同名键；实现为转发 `IRP_MJ_READ`、下层完成后原地等长改写 Report ID 0x01 的 `report[3]`（实测报告格式 `01 00 00 <usage> 00 ...`，report[1]=modifiers、report[2]=reserved），不改 Report Descriptor / Report ID / 报告长度；HVCI 开启下 Windows 11 x64 八键验收通过（KMDF 1.15 + WDK 10.0.26100，过 PREfast/InfVerif/ApiValidator/Inf2Cat）；其"KMDF 正式发布需 Hardware Dev Center attestation/WHCP、UMDF 2 迁移未实现"的结论与本仓库 ADR 0002 签名成本结论互证；(3) **VB-Cable 音频路径**：遥控器音频经 CABLE Input/Output 转发、临时切换系统默认录音设备喂给 WeType——依赖第三方虚拟声卡驱动和默认设备切换，违反本仓库基础路径边界，仅作增强轨/目标 App 适配参考。排除项：其语音键支持单击/双击/长按配置，违反本仓库"语音键只支持按下开始、释放结束"规则，语音键不借鉴；`keymap.json` + 托盘双击映射面板（单击/双击/长按可配、保存即生效、旧 `keymap.txt` 自动迁移）可作普通键映射产品化参考。
- `wasapi-rs` 0.24.0：MIT 许可的 Windows Core Audio 安全封装，用于端点枚举、共享模式渲染与 padding 查询。
- **CABLE Input 端点静音自愈（2026-09-07）**：依据 Microsoft Core Audio `IAudioEndpointVolume` / Endpoint Volume Controls 公共 API（`learn.microsoft.com/windows/win32/api/endpointvolume/nn-endpointvolume-iaudioendpointvolume`、`learn.microsoft.com/windows/win32/coreaudio/endpoint-volume-controls`），共享模式端点的主静音属于端点级状态，不是应用 WASAPI 写入成功即可证明可听。本仓库仅对名称确认的 VB-CABLE 渲染端点在打开时及每次语音会话开始前调用 `GetMute` → 必要时 `SetMute(FALSE)` → `GetMute` 读回确认；不修改物理输出设备，也不覆盖用户音量标量。调用结果、检查点和耗时写入结构化 GATT 诊断日志。
- **SayAll 会话静音自愈（2026-09-07）**：用户现场观察到音量合成器左侧 CABLE Input 端点未静音，但右侧“无线麦 SayAll”应用会话在开始推流后很快重新静音。依据 Microsoft `IAudioClient::Initialize` 文档，渲染会话默认会跨应用重启持久化音量与静音状态；依据 `ISimpleAudioVolume::GetMute/SetMute`，应用会话静音独立于端点主静音。实现使用 `IAudioSessionManager2::GetSessionEnumerator` + `IAudioSessionControl2::GetProcessId`，只锁定当前 SayAll 进程在用户已选 CABLE 端点上的会话；初始化、语音会话开始、`IAudioClient::Start` 后读回，并在推流期间每 100ms 低频检查，发现静音才解除，不修改会话音量、不碰系统声音或其他进程。初始化时另以 `IAudioSessionControl2::SetDuckingPreference(TRUE)` 让 SayAll 会话退出 Windows 默认通信自动压低机制；该预防措施不作为外部静音来源已经归因的证据。官方依据：`learn.microsoft.com/windows/win32/api/audioclient/nf-audioclient-iaudioclient-initialize`、`learn.microsoft.com/windows/win32/api/audioclient/nf-audioclient-isimpleaudiovolume-setmute`、`learn.microsoft.com/windows/win32/api/audiopolicy/nf-audiopolicy-iaudiosessionmanager2-getsessionenumerator`、`learn.microsoft.com/windows/win32/api/audiopolicy/nf-audiopolicy-iaudiosessioncontrol2-getprocessid`、`learn.microsoft.com/windows/win32/api/audiopolicy/nf-audiopolicy-iaudiosessioncontrol2-setduckingpreference`。

## 延迟调研来源（2026-09-05，语音键按下→电平图出现优化专项）

按仓库规则（实现前先调研），本专项调研结论与边界记录如下；对应实测见 `docs/investigations/evidence/p/FINDINGS.md`（端点预热对照实验）：

- **业界 PTT"按下→开麦"模式**：可查证的主流实现均为"音频链路常驻 + 按键只做门控"（Mumble 持续采集+传输模式门控 `mumble.info/documentation/user/audio-settings/`；Zoom 会议内按住空格解除静音 `support.zoom.com` KB0063250；Discord PTT Release Delay 滑杆，页面被反爬，引自搜索摘要）。本仓库渲染端点常驻打开（`audio.rs` SelectEndpoint 打开后跨会话复用）与此一致。
- **WASAPI 冷启动与端点电源**：微软 PortCls 文档——音频设备空闲（示例 1s）进入 D3，恢复 D0 规格要求 ≤35ms/≤300ms（`learn.microsoft.com/windows-hardware/design/device-experiences/audio-subsystem-power-management-for-modern-standby-platforms`）；JUCE 论坛实测 WASAPI 设备冷创建 2-3s、Initialize 数百 ms（`forum.juce.com/t/wasapi-2-3s-delays-on-creating-audio-devices/54971`）；StackOverflow `IAudioClient::Start` 通常 5-6ms（被 Cloudflare 拦截，引自摘要）。**"跨进程保温端点让第三方 Initialize 更快"无公开量化先例**——本仓库已用持锁对照实验自行量化：对 WeType 开麦延迟无效（冷/热中位数差 0.3ms，evidence/p，2026-09-05），该方向就此关闭。
- **WeType/微信输入法语音快捷键形态**：默认按住 Ctrl+Win（微信电脑版 4.1.7+ 同款，可于微信"设置→快捷键"自定义；新浪财经/光明网/callmysoft 报道）；社区帖（linux.do/t/topic/2409202，2026-06-15，早于 2.1.3，引自搜索摘要）称 WeType 语音快捷键"必须以 Ctrl/Alt/Shift 开头，不能设独立单键"——**待 2.1.3 真机复核**；ghxi 评论区提到"单击 Ctrl 触发"模式（懒加载未复核）。讯飞输入法 PC 版默认 F6 单键+长按说话（pconline/3DM/ghxi 教程）——竞品基线，未实测其延迟。
- **竞品/社区对"面板出现延迟"的讨论**：未找到任何量化"按下→微信电平图出现"的公开评测（横评均测识别速度/准确率）；游戏侧有 PTT 激活延迟 1s-5s 的社区案例（Overwatch 官方论坛、Valorant Reddit），第三方全局钩子（如 Razer Synapse）可使 PTT 延迟 3-5s——排查本机钩子干扰的依据。
- **本专项实测结论（evidence/p，2026-09-05）**：注入→WeType 开麦（ConsentStore 精确 FILETIME 判据）稳定 ~163ms（13 试验 ±5ms），端点预热无效；两型号遥控器实际均直接 0x04 开始推流（历史 GATT 日志 0x08 计数为 0，无可并行的开麦往返）；0x04 通知早于 HID F5 键盘事件 60-90ms 到达（evidence/p 2026-09-04 取证），当前"0x04 到达即注入"已是链路最早合法触发点。剩余 ~215-245ms = BLE/固件（~30-60ms）+ 和弦间隔（20ms）+ WeType 内部处理（~163ms，外部不可合法压缩）。
- **macOS 版输入目的地/输入源设计（HD838A/remote-mic-app，本机 clone `Documents\Codex\remote-mic-app`）**：`VoiceInputDestinationCoordinator.swift`——语音触发由"聚焦目的地就绪"门控（AX 系统级聚焦快照：role ∈ {AXTextArea/AXTextField/AXComboBox} + enabled + editable + 非保护内容 + 语义文本不含 password/search/设置 等敏感词；不就绪 UI 提示等待/不可用，5s 超时不注入）；`PreferredInputSourceMonitor`——保证配置的语音工具是活动输入源。**Windows 版 IME 专项（2026-09-05）借鉴其输入源职责**：实测 WeType 语音热键仅在自身为会话活动输入法时生效（微软拼音活跃 2/2 不触发、切回 2/2 恢复、激活后零延迟注入 3/3 触发，evidence/p），已实现 `ime.rs`（TSF `ActivateProfile` + `TF_IPPMF_FORSESSION` 会话级激活，公开 API，失败不阻断）。macOS 的 AX 聚焦目的地门控在 Windows 未采用——焦点实验证明 WeType 开麦不依赖文本焦点（6/6，桌面/资源管理器照常触发），聚焦门控留给未来 UIA 版本按需评估。

## BLE 僵死链路自动恢复调研来源（2026-09-05，重连健壮性专项）

场景：应用被强杀（未走正常关闭）后 Windows 侧残留僵死 GATT/HID 链路或服务缓存，普通重试永不恢复（本机真机取证：CCCD 订阅写入 E_ABORT、HID 接口从系统消失；examples\radio_probe 与 examples\gatt_snoop 探针复现）。已实现 `bluetooth_radio.rs` 自动恢复（重连循环连续失败达 5 次时关开蓝牙无线电一次，每周期最多 2 次），真机验证：无线电开关周期后重连循环立即成功（Testing\investigation\sayall-gatt-20260905-live.log T/C 能力交换取证）。关键参考：

- **微软官方 GATT 客户端文档**（Dispose 后系统"小超时"自动断开、重建设备对象按需重连；BluetoothLEDevice.Close 仅当本应用是唯一持有者才关连接；GATT 连接/发现可能因系统队列等待数分钟且当前不能取消）：`learn.microsoft.com/windows/apps/develop/devices-sensors/gatt-client`、`learn.microsoft.com/uwp/api/windows.devices.bluetooth.bluetoothledevice.close`
- **微软 BluetoothLEDevice 构造入口文档**：`FromIdAsync` 明确要求从 UI 线程调用（可能触发访问授权）；`FromBluetoothAddressAsync` 无此线程要求，并支持从已进入系统缓存的配对设备地址重建设备对象。2026-09-12 现场的 MTA `FromIdAsync` 先返回 Windows 资源错误，后续日志时序显示下一次请求占住 BLE 工作线程（阶段日志缺失，属结合代码的推断），故改用配对 AssociationEndpoint ID 内的对端地址调用后者；不记录真实地址。官方依据：`learn.microsoft.com/uwp/api/windows.devices.bluetooth.bluetoothledevice.fromidasync`、`learn.microsoft.com/uwp/api/windows.devices.bluetooth.bluetoothledevice.frombluetoothaddressasync`。
- **MS Q&A 99038**（只 Dispose 设备不 Dispose 服务则无法重连）、**MS Q&A 2280559**（RPA 解析滞后导致进程重启后首次 GetGattServicesAsync 必 Unreachable，官方建议 3 次重试 ×1s + Uncached）、**MS Q&A 1685221**（FromBluetoothAddressAsync 返回 null 僵死 bug，Win11 2024.01D 已修；MaintainConnection 遇 bond 丢失会重连循环）
- **Qt 论坛 156281**（实测：OS 侧服务缓存僵死，重启应用无效，**关开蓝牙是唯一有效修复**——与本机取证一致，是本仓库选择无线电恢复的直接依据）：`forum.qt.io/topic/156281`

### 2026-09-10 重连窗口 F5 泄漏补充

- **微软 `RegisterRawInputDevices` 文档**：同一进程、同一 Raw Input 设备类只能
  有一个接收窗口，最后一次注册覆盖前者；文档因此明确警告库内注册会干扰宿主
  自己的 Raw Input 处理。该约束解释了旧版 `key_suppressor.rs` 的键盘注册被
  `raw_input_windows.rs` 覆盖、断线期 F5 设备归因失效：
  `learn.microsoft.com/windows/win32/api/winuser/nf-winuser-registerrawinputdevices`。
- **参考实现复核**：本机 `reference-repos/vibe-flow` 提交
  `b47f7cdce8b753fade0c64c97332bebe80f17d2d` 的 `VoxDeckInputBridge.cs` 对语音
  F5 使用 LL 钩子兜底，并在重连扫描码变化时仍以持久语音映射为准；它接受实体
  键盘 F5 冲突。本仓库采用边界更窄的做法：主 Raw Input 窗口统一归因，只有
  Connecting/Discovering/AwaitingCapabilities/Reconnecting 建链窗口临时兜底，
  稳定状态继续保留实体键盘 F5。
- **记事本行为旁证**：Microsoft Q&A 的 Windows/Notepad 条目确认 F5 会插入当前
  日期时间。2026-09-10 本机现象格式与系统区域格式一致，结合诊断日志
  `seen=74 swallowed=0 leaked=74`，可排除 ASR 把语音识别成日期的解释。
- **Bleak winrt client 源码**（Unreachable 重试 10×1s；断开全量清理序列 CCCD=None→退订→逐服务 Close 带 0.1s 防挂起延迟）、**btleplug winrtble**（Uncached 触发连接、特征发现 5s 超时回退 Cached——#325：部分驱动 Uncached 请求无限挂起，本仓库 connect 尚无该超时，列为后续加固项）、**微软官方 BluetoothLE 示例 Scenario2_Client**（FromIdAsync→RequestAccessAsync→Uncached 发现→清理序列）
- **Windows.Devices.Radios.Radio**（微软 `RequestAccessAsync` / `SetStateAsync`
  文档）：改变无线电前先请求权限并检查 `RadioAccessStatus::Allowed`；
  `SetStateAsync` 返回只表示请求是否获准，实际状态异步转换，应观察
  `StateChanged` 或复读 `State` 确认。2026-09-12 统一包实测旧实现 0-5ms
  即误判两轮恢复失败，据此改为进程内缓存 Allowed、Off/On 有界复读确认。
  官方依据：`learn.microsoft.com/uwp/api/windows.devices.radios.radio.requestaccessasync`、
  `learn.microsoft.com/uwp/api/windows.devices.radios.radio.setstateasync`。
- **Radio 设备查询兜底**（微软 `Radio.GetDeviceSelector` / `Radio.FromIdAsync`
  文档）：官方允许以 AQS + `DeviceInformation.FindAllAsync` 枚举后通过 ID
  重建 Radio，并说明硬件异常/移除场景下它比 `GetRadiosAsync` 更可靠。
  2026-09-12 现场两条路径均返回 `0x80070008`，据此把“公开 API 已穷尽”的
  人工提示边界固定下来。官方依据：
  `learn.microsoft.com/uwp/api/windows.devices.radios.radio.getdeviceselector`、
  `learn.microsoft.com/uwp/api/windows.devices.radios.radio.fromidasync`。

外部实现只作为带来源的参考。第三方应用进程注入、私有配置读取和来源不明二进制不进入稳定主路径。

## Windows 系统快捷键录入与锁屏动作（2026-09-10）

- **执行端**：微软 `SendInput` 文档说明它把事件串行插入输入流、受 UIPI 与当前键态影响；`LockWorkStation` 是交互桌面进程可调用的公开锁屏 API，成功返回只表示异步锁屏请求已发起。Hooks 文档说明全局钩子事件局限于调用线程所在桌面。按键映射中的精确 `Win+L` 因而先等待实体键释放、由门控成对处理 DOWN/UP，再调用 `LockWorkStation`；其他快捷键仍走既有 `SendInput` 并保持按下即响应。官方依据：`learn.microsoft.com/windows/win32/api/winuser/nf-winuser-sendinput`、`learn.microsoft.com/windows/win32/api/winuser/nf-winuser-lockworkstation`、`learn.microsoft.com/windows/win32/winmsg/hooks`。
- **录入端**：微软 `LowLevelKeyboardProc` 文档明确低级键盘钩子在按键消息进入目标线程队列前运行，处理后返回非零可阻止继续传递，并要求回调快速把工作移交后台线程。本仓库复用常驻 `WH_KEYBOARD_LL` 门控：录入期间先吞物理 DOWN，所有对应重复 DOWN/UP 即使录入已经结束仍按同一次按住吞完；录入开始前已经按住的键则全程放行，避免不对称边沿。钩子只 `try_send`，Tauri 事件由独立线程发出。官方依据：`learn.microsoft.com/windows/win32/winmsg/lowlevelkeyboardproc`。
- **边界**：`Ctrl+Alt+Del` 等安全注意序列不属于普通快捷键录入能力；Win+L 在当前
  Windows 主机上即使低级钩子返回吞下仍会锁屏。因此自定义录入默认保留直接模式，
  并提供用户显式开启的“界面选择修饰键 + 物理键盘只按主键”安全模式；安全模式
  不在输入流中生成系统组合。
- **钩子链顺序补充（第二轮现场复验）**：微软 Hooks Overview 说明钩子按链调用，
  已处理事件可停止继续传给后续钩子/目标；`LowLevelKeyboardProc` 也明确非零返回
  阻止后续传递。现场观察到本钩子吞下 Win/L 后仍被系统锁屏；链首重挂又导致边沿
  完全丢失，实证 failed 并回退。产品路径不再依赖钩子链顺序屏蔽系统保留组合。
  官方依据：`learn.microsoft.com/windows/win32/winmsg/about-hooks`、
  `learn.microsoft.com/windows/win32/winmsg/lowlevelkeyboardproc`。
- **TV→锁屏的协议选择器兜底（2026-09-12）**：微软 Raw Input 文档明确
  `RIDEV_NOLEGACY` 只适用于鼠标/键盘，不能据此阻止消费控制 HID 的独立 Shell
  动作；`SetWinEventHook` 提供跨进程、out-of-context 的对象事件观察，
  `EVENT_OBJECT_CREATE` 早于 SHOW。现场证明 Windows 会在 SayAll 锁屏约 4 秒后
  由系统服务创建 `OpenWith.exe`；SHOW 阶段隐藏仍偶发闪帧，CREATE 阶段终止精确
  helper 连续四轮无可见弹窗。产品路径只在“已观察 TV→下一次 SayAll 锁屏”的
  15 秒窗口启用，并只处理 Windows `System32` 下映像名精确为 `OpenWith.exe`
  的进程。官方依据：
  `learn.microsoft.com/windows/win32/api/winuser/ns-winuser-rawinputdevice`、
  `learn.microsoft.com/windows/win32/api/winuser/nf-winuser-setwineventhook`、
  `learn.microsoft.com/windows/win32/winauto/event-constants`、
  `learn.microsoft.com/windows/win32/api/processthreadsapi/nf-processthreadsapi-terminateprocess`。

## WeType 热键休眠自动恢复调研来源（2026-09-05，热键休眠专项 v2）

场景：WeType 2.1.3.18 后台约 40 分钟后"TSF 存活但全局键盘钩子休眠"——和弦注入 LWin 穿透、无 0xFC、ConsentStore 时间戳不动；打开 WeType 任意自身界面立即复活（kb-live 会话 23-26 真机取证）。跨进程 `SetProcessInformation(ProcessPowerThrottling)` 解除节流**真机证伪**（对其他进程 E_INVALIDARG 0x80070057，wetype_service 打开即 0x80070005，15:04 live12 取证），该路线已从 `wetype_revive.rs` 移除。v2 已实现（`ble.rs` + `ime.rs`）：检测（注入后 700ms ConsentStore 时间戳未动）→ TSF 配置切换唤醒（`cycle_wetype_profile`：激活微软拼音 80ms 后切回，公开 API）→ 300ms 后经 `WorkerMessage::RetryVoiceChord` 在工作线程释放旧和弦并重注入 → 二次检测未响应才提示人工。关键参考与实测：

- **TSF profile 管理 API**（`ITfInputProcessorProfileMgr::ActivateProfile`/`EnumProfiles`，`TF_IPPMF_FORSESSION` 会话级激活）：微软官方文档 `learn.microsoft.com/windows/win32/api/msctf/nf-msctf-itfinputprocessorprofilemgr-activateprofile`。选择理由：切换配置会向所有 TSF 感知进程广播激活事件，是唯一能从外部触达 WeType 的公开 API 路径。
- **会话 47 真机实测（live13 + kb-live.log 全解码，2026-09-05 15:21）**：休眠中按键 → 检测未响应 → 配置切换真实完成（STA 线程，切微软拼音 clsid 9D2B2E2B 再切回）→ **346ms 后重注入的和弦同样未开麦**（LL 钩子日志见注入的 5B 边沿泄漏可见、无 0xFC 标记）。
- **16:44-17:35 七次休眠发作实测（kb-live.log，2026-09-05 晚间复盘）**：用户大量使用语音键期间钩子反复休眠/复活，七个发作簇全部同构：首和弦失败（5B 泄漏）→ v2 自动重试（cycle+300ms 时序精确吻合）**7/7 失败**→ 用户在 cycle 后 **1.24/1.28/1.68/1.85/1.9/2.28s** 的再按全部成功（FC 标记 + ConsentStore 开麦交叉验证）。**结论：配置切换确实能复活休眠钩子，复活延迟实测 ∈ (300ms, ~2.3s]（一次疑似 ≤6s）**；+300ms 重注入恒过早。注意：该时段应用为并行会话部署的无日志实例（pid 11692，含共享分支上的 v2 代码），cycle 隐形运行——与 kb-live 时序吻合。据此实现重试阶梯（WETYPE_RETRY_SETTLE_MS=[2000,3000]，两轮 cycle+重注入，最后才提示人工）。
- **LL 钩子日志判据（2026-09-05 新增，kb-live.log 全天 128 和弦窗口解码）**：WeType 钩子存活时**消费注入的和弦 LWin 边沿并注入自己的 0xFC 标记对**（每边沿一对瞬时 D/U）；钩子休眠时 5B 边沿泄漏可见、无 0xFC。此判据与 ConsentStore 开麦时间戳 100% 交叉验证一致，成为"钩子死活"的即时观测手段（无需开麦）。全天时间线（毫秒时间戳锚定）：休眠形成于 15:08:21（最后一次成功会话结束）→15:16:10（首次失败）之间的**方向键-only 活动窗口（≤8 分钟）**；此前 14:13、15:04 等休眠段落与手动复活（打开 WeType 界面）全部对齐。**休眠形成是偶发的**：15:21:34 复活后钩子存活 ≥80 分钟（跨两次探针开麦会话、用户离开/打字交替），未再休眠——"40 分钟规律"不成立，形成条件未定。
- **健全性实测（2026-09-05 16:41 持锁）**：cycle profile（STA）后 1s 注入和弦照常开麦——**配置切换不破坏活钩子的和弦触发**，v2 复活路径前提成立。剩余验证：重试阶梯版待下一次自然休眠发作做端到端确认（一次按住内自动恢复）。
- **首按失败根因终局定论与验证（2026-09-05 21:34-21:38，commit 1b55cca 部署后）**：真正的根因是 **F5 泄漏三键拒绝**——遥控器闲置后应用自身被后台节流，0x04→抑制器武装的链路（经工作线程队列）拖 ~120ms，F5 的 60ms 有界等待超时泄漏 → 和弦变成 F5+Ctrl+Win 被微信输入法拒绝；断连重连变体中首个 F5 在 0x04 前泄漏、UP 丢失致 OS 键态粘 F5。修复（三重防线）：GATT 回调线程直接武装 + 和弦前 F5 解粘 UP + 抑制器决策计数日志。**验证结果：4/4 会话首按成功**（含一次 25 分钟闲置后首按），suppressor_stats leaked=0（135 个 F5 全部吞下，其中 1 个冷启动 F5 由 GATT 回调武装+有界等待兜住），kb-live 零 F5 D 泄漏、全部和弦带 FC 成功标记，解粘 UP 按设计仅在需要时进入 OS。"WeType 钩子休眠"理论正式退役：全部证据与 F5 泄漏 + 20ms 间隔冷态拒绝两个机制一致；重试阶梯保留为无害安全网。
- **`SetProcessInformation` 权限边界**：微软文档明确 ProcessPowerThrottling 仅作用于调用进程自身；对其他进程返回 E_INVALIDARG。真机取证一致（live12）。
- **WeType 进程布局（本机取证）**：开麦方为 `wetype_update.exe`（ConsentStore 条目，拥有顶层窗口 StatusBarWnd）；另有 wetype_service/wetype_server/wetype_renderer。休眠的是钩子所在后台进程，TSF DLL 运行于前台应用进程内不受影响——这解释了为何 TSF 路径（中文输入）存活而全局钩子休眠。

## 应用内更新（tauri-plugin-updater + GitHub Releases）调研来源（2026-09-05）

场景：Windows 应用内"检查更新 + 下载安装"（GitHub-only、零自建服务器）。关键行为均以插件源码/官方文档原文核对，非推测：

- **官方 updater 插件文档**（`v2.tauri.app/plugin/updater/`，免费开源，MIT/Apache-2.0）：静态 JSON 端点模式官方示例即 `https://github.com/<owner>/<repo>/releases/latest/download/latest.json`（GitHub 302 到最新**稳定** Release 的资产，草稿/prerelease 不参与 latest）；`latest.json` 必需字段 `version`/`platforms.<target>.url`/`platforms.<target>.signature`，`signature` 为 `.sig` 文件**内容**（非路径）；签名强制不可关闭，私钥丢失即无法再向存量用户推送更新。
- **tauri-plugin-updater 源码**（plugins-workspace v2，`updater.rs`/`config.rs`，按 2.11.0 核对）：`check()` 对 204 返回"无更新"、200 解析 JSON 后按 SemVer `release.version > current` 判定；平台键按 `windows-x86_64-nsis` → `windows-x86_64` 顺序回退查找（latest.json 只需提供 `windows-x86_64`，dev 与 NSIS 安装态通用）；`Update.timeout` 默认 None（下载不限时），builder 的 timeout 只作用于 check 请求；下载完成后**先验签**再安装。**Windows 安装时序**：`install_inner` = 解包 → `on_before_exit` 回调 → `ShellExecuteW` 启动安装器（NSIS 参数 `/P`（passive）+ `/UPDATE` + `/R`（装完自动重启应用））→ `std::process::exit(0)`——**Drop 清理不会执行**，必须把 BLE 断开等成对清理放进 `on_before_exit`（本仓库 2026-09-05"部署不得强杀/强杀残留"教训的更新路径版）；`config.rs::validate_endpoints`：debug 构建 http 端点仅警告放行，release 构建强制 https（本仓库本地 E2E 用 `SAYALL_UPDATER_ENDPOINT` 覆盖端点 + dev 构建走 http，正式配置不含任何 dangerous 开关）。
- **tauri-cli/bundler 构建约束**（tauri issues #13259、#15638 + 组织讨论 #6013 佐证，并在本机复验）：`tauri.conf.json` 配置 `plugins.updater.pubkey`（及 `createUpdaterArtifacts`）后，构建时缺 `TAURI_SIGNING_PRIVATE_KEY` 环境变量会直接失败——现有 Windows CI 的无签名预览构建必须配套处理（无 Secret 时生成一次性临时密钥保 CI 绿灯；正式 Release workflow 缺 Secret 直接失败，防止发布不可用更新包）。
- **tauri-action**（官方构建+发布 Action，自动生成 latest.json）：评估未采用——它整体接管 build+release，无法嵌入本仓库既有的 verify-windows-bundle、安装生命周期矩阵等既有验收步骤；改为保留既有构建流程 + 自写 `generate-updater-manifest.ps1`（latest.json 生成逻辑对齐 tauri-action 的字段来源：version←tauri.conf.json、signature←`.sig` 文件内容、url←Release 资产直链）。
- **发布资产命名**：NSIS 产物名含中文与空格（`无线麦 SayAll_*.exe`），GitHub 资产直链需 percent-encoding；为消除编码风险，Release 资产在 CI 中复制为纯 ASCII 名（`SayAll-Windows-<version>-x64-setup.exe`）后上传，本地构建产物名不变（CI 全部脚本按 `*-setup.exe` 过滤定位，实测不受新增 `.sig` 影响）。
- **NSIS 与既有安装器门禁的相互作用**：updater 以 `/P`（passive）+ `/UPDATE` 运行，既有 installer-hooks.nsh 的 PREINSTALL SemVer 降级门禁照常生效（升级路径不受影响）；POSTINSTALL 的 VB-CABLE 提示在 passive（非 Silent）模式下仍会弹出——仅影响未装 VB-CABLE 的用户，与首装行为一致，保留。
- **预览版通道（2026-09-08 增补）**：Tauri 官方 Runtime Configuration 文档明确支持通过 `UpdaterBuilder::endpoints` 在运行时选择 stable/beta 等独立通道；本仓库据此保持默认稳定端点不变，仅在用户显式开启“检查预览版更新”后覆盖端点。GitHub Releases 页面公开提供标准 Atom feed（`releases.atom`），包含已发布的正式版与 Pre-release、排除 Draft；实现从本仓库 feed 的 `alternate` 链接读取 SemVer tag，选择最高版本并自行构造本仓库 `https://github.com/GetSayAll/remote-mic-app-windows/releases/download/<tag>/latest.json`，避开匿名 REST API 每 IP 60 次/小时限流。最终安装包仍由 Tauri minisign 强制验签。
