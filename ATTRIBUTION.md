# 来源与归属

本仓库是面向 Windows 的 Rust/Tauri 工程。

## 产品与 UI 基准

- `HD838A/remote-mic-app`：无线麦 macOS 原版的信息架构、产品文案、RC003 图片、RC001/RC003 型号识别、ATVV 行为和测试边界；RC001 支持参考提交 `b233a88cc4457b00413dda6b37ec8b4af12c5121`。
  - 2026-09-05 按键映射功能移植补充（均为语义移植，非代码复制）：`RemoteButtonGestureRecognizer` + `HIDRemoteScheduler` 的手势参数（双击窗口 300ms、长按 550ms、连发起始 350ms、返回 50ms/方向与音量 100ms 连发）与"按配置动态启用双击/长按识别、未配置时单击零延迟"的语义；`KeyboardEventSuppressor` 的预测式武装 + 有限窗口匹配吞键模型；`RemoteMappingCanvas` 的按键卡片布局表（锚点/目标 Y 坐标逐键移植）与三态高亮（按下=橙、选中=强调、普通=中性）；`MappingSelectionPolicy` 的"锁定当前按键"默认值。Mac 版 `KeyboardEventSuppressor` 的 UP 沿无配对兜底（DOWN 泄漏+UP 吞下=粘键缺陷）未移植——Windows 版沿用本仓库 2026-09-05 规则（DOWN 漏进 OS 则 UP 必放行）。
- RC003 图片 SHA-256：`658d9333853958c13ff721eb76e1a6816c1dbea16006a84e8577ad410812549f`。

## Windows 行为与测试参考

- **LL 吞键对 Raw Input 交付影响的本机实证（2026-09-05，`docs/investigations/2026-09-05-ll-swallow-vs-raw-input.md`）**：双线程探针（钩子线程 + Raw Input INPUTSINK 线程分离，key_suppressor 同构）两轮一致证实 **WH_KEYBOARD_LL 返回 1 吞掉的键盘事件不会再投递 WM_INPUT**——按键映射门控（`key_gate.rs`）据此采用"被吞键盘边沿由钩子线程直接喂引擎 + 监听器喂 HID 报文与透传键盘事件"双源合并架构；HID 报文归因武装 + 60ms 有界等待沿用 key_suppressor 实证参数。

- `HD838A/remote-mic-app#249`，提交 `090a3cfc24f0e3e733b2347ee2daf87c60e10097`：Windows 独立实现、ATVV 测试夹具、语音边沿、安装升级、公开边界和 Mac 风格 UI 原型；Raw Input 参考了 `hid_identity.py` 与 `raw_input_windows.py`，SendInput 的批量提交、物理修饰键和失败回滚参考了 `win32_input.py` 与 `win32_keys.py`，均以 Rust/windows-rs 重新实现。
- `GetSayAll/hardware-simulation`，提交 `65248499cac7da3ad46cd0c11dca1478f7733255`：RC001 短语音时间线的控制通知、40 + 80 字节音频拆包和停止通知；本仓库只保留纯 ATVV 回放所需字段。
- `ZSTDJan/windows-remote-mic-app`：WinRT BLE、Raw Input、音频输出、发布门禁和真实硬件验证边界；其语音页按语音程序配置"按住说话快捷键"、按下注入 DOWN/松开释放的行为，是本仓库按住说话快捷键设置的产品参考。Round 1 拆解曾记两项技巧参考，后续实证修正（Round 2/3）：**physicalize 技巧——结构性无效（勿模仿）**：`legacy_key_suppressor_windows.py` L142-155 的做法（仅对自家 keybd_event 注入的带 "RMICRC03" 标记右 Alt，在自家钩子的私有副本上清 INJECTED 标志→转发→恢复）曾被解读为"使下游应用钩子视为物理键，前提是自家钩子位于目标应用钩子之前（链头）"——该解读不成立（Round 2 E 三层实证：LL 钩子每钩子收到私有结构副本，修改不跨钩子传播，CallNextHookEx 转发通道不存在，应用层收到原始键；Round 3 J 语义复查：清标志对下游钩子/应用层均不可见，且 ZSTDJan 进程内也无读者——对声明目标是 no-op；其真正能影响豆包读值的是 `doubao_rpc.py` 的 Frida 版 attach 方案，未接线进生产流程，违反本仓库 A2/A4/A5 边界，仅作机理记录）。**WeType 语音触发配方——本机实证有效（Round 3 J 翻案）**：SendInput 注入 Ctrl+Win 按住（纯 wVk 或扫描码配方均可）可唤起 WeType 语音（会话级 TSF 激活前提下：开麦/吞键/释放关麦全链实证，注入 ground truth 由常驻捕获器独立记录）；**Round 2 F 曾判"三配方无反应"，系其 TSF 激活用了线程级 flags（dwFlags=0，会话级应为 TF_IPPMF_FORSESSION=0x20000000）、WeType 从未真正激活所致——教训：测试 IME 行为前必须以会话级激活 + 行为判据（候选框版式）双重确认活动输入法**。**配方形态约束（2026-09-04 P 实证，evidence/p）：和弦必须逐事件注入且两键间隔 ≥80ms——WeType 拒绝单次 SendInput 批量零间隔提交的 Ctrl+Win（sent=2/2 全到达仍无吞键无开麦；逐事件 80ms 两轮 2/2 触发，A 失败→B 通过→A 失败→B 通过交替序列排除状态漂移）**；应用曾把该配方误合并为单批零间隔导致真机不出字（Bugs\2026-09-04-wetype-zero-gap-injection.md，含第二层缺陷：遥控器 F5 须由抑制器吞掉，否则"额外按键"拒绝；钩子链头 bump 加固同日落地），已修复并 RC001 真机端到端 passed（2026-09-04，用户确认文字上屏）。
- `richlearntodo-debug/vibe-flow`，提交 `047f9d3ead54bf30de9b884adf8f7b5adefe9993`：自然 ATVV 会话、WASAPI 音频生命周期和硬件验收清单。
- `mwlt/Voice_VibeCoding`，提交 `c89410aed3b274fee5e571128b82c9c6e6689715`：Rust/Tauri 模块划分、windows-rs API、音频生命周期和托盘窗口工程经验；其语音键按住注入的 Hold 语义（按下先快捷键 DOWN、松手统一释放、SendInput 互斥降级）是本仓库按住说话快捷键注入时序的参考。Round 1 拆解补充其 **LL 钩子吞键工程细节**（本仓库吞键层设计的参考，非逐行复用）：时序窗吞键（音量 recent 200ms、back/home/menu/tv/power 250ms、方向/OK 200ms 或 tap_ready+自定义位图）、钩子链头 bump（重叠安装：先挂新钩再卸旧钩，消除 LL 吞键空窗）、F5 语音键状态机（sticky/correlate 120ms/tail 3s；DOWN 漏进 OS 则 UP 必放行，防粘键）、音量防双格（Tap 转发 + SendInput VK_VOLUME_* + 200ms 吞固件残留）、Alt 和弦用 SendMessageTimeoutW 直发前台避免系统菜单、自家注入放行（EXTRA_INFO 标记或 INJECTED→CallNextHookEx），及 bump 空窗/sticky 粘键/60ms 去抖门等已踩坑清单。本仓库只使用 SendInput 公共 API，不引入其 WinUHid 虚拟键盘驱动。
- `cgutman/WinUHid`（MIT 许可）：用户态 UMDF 虚拟 HID 键盘/鼠标驱动框架（C++/Win32），无预编译 Release，需自建并签名后使用；ADR 0002 增强轨驱动来源的第一候选（须先审计）。签名成本调研结论（**已闭合，2026-09-04**：UMDF 分发不需硬件计划/EV，OV 级 catalog 签名为最低门槛——三层官方原文支撑，`docs\investigations\evidence\g\signing-policy.md`；残余含混=无单句官方原文直书此结论，装机实测 deferred（调查护栏限制））记录于 `docs\investigations\2026-09-04-avoid-driver-signing-input-paths.md`。未经审计的 WinUHid 二进制不进入仓库。
- `QL-4/RemoteMapper`，main 提交 `25ca0c13cf2ff2caf7caae3d9690f9629b7c0df0`（另有 `driverless-keymap` 分支）：小米蓝牙遥控器 → 微信输入法（WeType）语音录入的完整端到端先例——按住语音键唤起 WeType 录入并送入音频，松开结束录入并恢复系统原默认麦克风。可借鉴结论：(1) **双分支分层**：`main` 含 KMDF HID lower filter（需 TESTSIGNING），`driverless-keymap` 无驱动直接交付、但缺返回/音量±三个键（被 kbdhid.sys 丢弃）且 LL 钩子映射会误吞物理键盘同名键——印证本仓库"基础路径免驱动 + 增强轨驱动"分层与"LL 钩子无来源设备 ID"的既有判断；(2) **MiRemoteHidFilter 驱动做法**：extension INF 精确绑定 VID 0x2717 / PID 0x32B8（不匹配其他键盘），修复 kbdhid.sys 丢弃的 usage 0x80/0x81/0xF1，并把普通键改写为 F13–F19、语音键 HID F5 改写为 F20，从源头规避误吞物理键盘同名键；实现为转发 `IRP_MJ_READ`、下层完成后原地等长改写 Report ID 0x01 的 `report[3]`（实测报告格式 `01 00 00 <usage> 00 ...`，report[1]=modifiers、report[2]=reserved），不改 Report Descriptor / Report ID / 报告长度；HVCI 开启下 Windows 11 x64 八键验收通过（KMDF 1.15 + WDK 10.0.26100，过 PREfast/InfVerif/ApiValidator/Inf2Cat）；其"KMDF 正式发布需 Hardware Dev Center attestation/WHCP、UMDF 2 迁移未实现"的结论与本仓库 ADR 0002 签名成本结论互证；(3) **VB-Cable 音频路径**：遥控器音频经 CABLE Input/Output 转发、临时切换系统默认录音设备喂给 WeType——依赖第三方虚拟声卡驱动和默认设备切换，违反本仓库基础路径边界，仅作增强轨/目标 App 适配参考。排除项：其语音键支持单击/双击/长按配置，违反本仓库"语音键只支持按下开始、释放结束"规则，语音键不借鉴；`keymap.json` + 托盘双击映射面板（单击/双击/长按可配、保存即生效、旧 `keymap.txt` 自动迁移）可作普通键映射产品化参考。
- `wasapi-rs` 0.24.0：MIT 许可的 Windows Core Audio 安全封装，用于端点枚举、共享模式渲染与 padding 查询。

## 延迟调研来源（2026-09-05，语音键按下→电平图出现优化专项）

按仓库规则（实现前先调研），本专项调研结论与边界记录如下；对应实测见 `docs/investigations/evidence/p/FINDINGS.md`（端点预热对照实验）：

- **业界 PTT"按下→开麦"模式**：可查证的主流实现均为"音频链路常驻 + 按键只做门控"（Mumble 持续采集+传输模式门控 `mumble.info/documentation/user/audio-settings/`；Zoom 会议内按住空格解除静音 `support.zoom.com` KB0063250；Discord PTT Release Delay 滑杆，页面被反爬，引自搜索摘要）。本仓库渲染端点常驻打开（`audio.rs` SelectEndpoint 打开后跨会话复用）与此一致。
- **WASAPI 冷启动与端点电源**：微软 PortCls 文档——音频设备空闲（示例 1s）进入 D3，恢复 D0 规格要求 ≤35ms/≤300ms（`learn.microsoft.com/windows-hardware/design/device-experiences/audio-subsystem-power-management-for-modern-standby-platforms`）；JUCE 论坛实测 WASAPI 设备冷创建 2-3s、Initialize 数百 ms（`forum.juce.com/t/wasapi-2-3s-delays-on-creating-audio-devices/54971`）；StackOverflow `IAudioClient::Start` 通常 5-6ms（被 Cloudflare 拦截，引自摘要）。**"跨进程保温端点让第三方 Initialize 更快"无公开量化先例**——本仓库已用持锁对照实验自行量化：对 WeType 开麦延迟无效（冷/热中位数差 0.3ms，evidence/p，2026-09-05），该方向就此关闭。
- **WeType/微信输入法语音快捷键形态**：默认按住 Ctrl+Win（微信电脑版 4.1.7+ 同款，可于微信"设置→快捷键"自定义；新浪财经/光明网/callmysoft 报道）；社区帖（linux.do/t/topic/2409202，2026-06-15，早于 2.1.3，引自搜索摘要）称 WeType 语音快捷键"必须以 Ctrl/Alt/Shift 开头，不能设独立单键"——**待 2.1.3 真机复核**；ghxi 评论区提到"单击 Ctrl 触发"模式（懒加载未复核）。讯飞输入法 PC 版默认 F6 单键+长按说话（pconline/3DM/ghxi 教程）——竞品基线，未实测其延迟。
- **竞品/社区对"面板出现延迟"的讨论**：未找到任何量化"按下→微信电平图出现"的公开评测（横评均测识别速度/准确率）；游戏侧有 PTT 激活延迟 1s-5s 的社区案例（Overwatch 官方论坛、Valorant Reddit），第三方全局钩子（如 Razer Synapse）可使 PTT 延迟 3-5s——排查本机钩子干扰的依据。
- **本专项实测结论（evidence/p，2026-09-05）**：注入→WeType 开麦（ConsentStore 精确 FILETIME 判据）稳定 ~163ms（13 试验 ±5ms），端点预热无效；两型号遥控器实际均直接 0x04 开始推流（历史 GATT 日志 0x08 计数为 0，无可并行的开麦往返）；0x04 通知早于 HID F5 键盘事件 60-90ms 到达（evidence/p 2026-09-04 取证），当前"0x04 到达即注入"已是链路最早合法触发点。剩余 ~215-245ms = BLE/固件（~30-60ms）+ 和弦间隔（20ms）+ WeType 内部处理（~163ms，外部不可合法压缩）。

## BLE 僵死链路自动恢复调研来源（2026-09-05，重连健壮性专项）

场景：应用被强杀（未走正常关闭）后 Windows 侧残留僵死 GATT/HID 链路或服务缓存，普通重试永不恢复（本机真机取证：CCCD 订阅写入 E_ABORT、HID 接口从系统消失；examples\radio_probe 与 examples\gatt_snoop 探针复现）。已实现 `bluetooth_radio.rs` 自动恢复（重连循环连续失败达 5 次时关开蓝牙无线电一次，每周期最多 2 次），真机验证：无线电开关周期后重连循环立即成功（Testing\investigation\sayall-gatt-20260905-live.log T/C 能力交换取证）。关键参考：

- **微软官方 GATT 客户端文档**（Dispose 后系统"小超时"自动断开、重建设备对象按需重连；BluetoothLEDevice.Close 仅当本应用是唯一持有者才关连接）：`learn.microsoft.com/windows/apps/develop/devices-sensors/gatt-client`、`learn.microsoft.com/uwp/api/windows.devices.bluetooth.bluetoothledevice.close`
- **MS Q&A 99038**（只 Dispose 设备不 Dispose 服务则无法重连）、**MS Q&A 2280559**（RPA 解析滞后导致进程重启后首次 GetGattServicesAsync 必 Unreachable，官方建议 3 次重试 ×1s + Uncached）、**MS Q&A 1685221**（FromBluetoothAddressAsync 返回 null 僵死 bug，Win11 2024.01D 已修；MaintainConnection 遇 bond 丢失会重连循环）
- **Qt 论坛 156281**（实测：OS 侧服务缓存僵死，重启应用无效，**关开蓝牙是唯一有效修复**——与本机取证一致，是本仓库选择无线电恢复的直接依据）：`forum.qt.io/topic/156281`
- **Bleak winrt client 源码**（Unreachable 重试 10×1s；断开全量清理序列 CCCD=None→退订→逐服务 Close 带 0.1s 防挂起延迟）、**btleplug winrtble**（Uncached 触发连接、特征发现 5s 超时回退 Cached——#325：部分驱动 Uncached 请求无限挂起，本仓库 connect 尚无该超时，列为后续加固项）、**微软官方 BluetoothLE 示例 Scenario2_Client**（FromIdAsync→RequestAccessAsync→Uncached 发现→清理序列）
- **Windows.Devices.Radios.Radio**（RequestAccessAsync 文档要求 + 可能弹同意框；本机实测未打包桌面进程 SetStateAsync 直接 RadioAccessStatus=Allowed 无需提权；本仓库为避免无人值守弹框，不调 RequestAccessAsync，被拒时按错误上报走人工提示）

外部实现只作为带来源的参考。第三方应用进程注入、私有配置读取和来源不明二进制不进入稳定主路径。
