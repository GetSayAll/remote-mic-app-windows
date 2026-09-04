# TODO

## v1 决策（2026-09-04）

- 第一版只做微信输入法听写：按住说话快捷键默认 左Ctrl+左Win（适配微信输入法默认语音热键）；语音可用 = 按住语音键 → 注入快捷键 → ATVV 音频经 CABLE Input → 微信输入法麦克风（CABLE Output）→ 云端识别 → 文字上屏。端到端链路音频段已本机实证（调查报告 evidence/n）；注入段配方约束已本机实证（evidence/p：WeType 拒绝单批零间隔和弦，须逐事件注入；间隔 20/40/60ms 均 4/4 触发、零间隔 0/2，默认取 20ms 压低按键延迟，Bugs\2026-09-04-wetype-zero-gap-injection.md）；**延迟账目已量化（2026-09-05，evidence/p 端点预热对照实验）**：注入→WeType 开麦固定 ~163ms（冷/热端点中位数差 0.3ms，预热无效；WeType 内部处理，第三方边界不可干预），两型号实际均直接 0x04 推流（无开麦往返可并行），0x04 早于 HID F5 60-90ms（触发点已最早），全链 ≈215-245ms 其中应用侧仅和弦 20ms 可控——**应用侧延迟优化到此收敛**；RC001 遥控器端到端真机 passed（2026-09-04，用户确认文字上屏；用户侧前提=输出端点选 CABLE Input + 系统默认录音设备切 CABLE Output，应用不修改系统默认设备）；RC003 真机待验。豆包（注入判死四层闭环）与 WinUHid 增强轨延后，见 ADR 0002 与 docs/investigations/2026-09-04-avoid-driver-signing-input-paths-final.md。

## Windows RC001 / RC003

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
- [x] 实现用户显式配置的语音键按住说话快捷键（连接页预设：关闭、右 Alt、F5、Win+H、左 Ctrl+左 Win）：按下语音键先注入 DOWN 再开始音频会话，释放统一注入 UP，断连/睡眠/中止/退出强制释放；注入时序参考 ZSTDJan 按住说话快捷键与 Voice_VibeCoding 的 Hold 语义，仅使用 SendInput 公共 API（见 ATTRIBUTION.md）。2026-09-04 修复一：和弦改为逐事件提交、事件间 80ms 间隔（WeType 拒绝单批零间隔，evidence/p）。修复二：F5 抑制器会话武装信号误接未启动的旧模块 voice_key_suppressor（ble.rs），遥控器 F5 泄漏进和弦致 WeType "额外按键"拒绝——改接 key_suppressor 并删除旧模块（Bugs\2026-09-04-wetype-zero-gap-injection.md）。加固：钩子链头 bump（会话开始 + 10s 定时）+ Raw Input 归因独立线程。**RC001 真机端到端 passed（2026-09-04，用户确认文字上屏；前提=输出端点 CABLE Input + 系统默认录音 CABLE Output）**；RC003 真机待验。
- [ ] 使用真实 RC001/RC003 和第三方语音程序（微信输入法、Win+H 等）验证按住说话快捷键：DOWN/UP 严格成对、无粘键、无重复音频，且断连和睡眠恢复后不残留按住的快捷键。RC001 基本链路与加固版回归均已 passed（2026-09-04，型号经应用 2A24 显示双证）；RC003 基本链路 passed（连接/触发/MIC_EXTEND 续期正常），音频送达率经**重配对后复测 passed**（55%→98.7%，与 RC001 基准持平，文字"一二三四五六七八九十"全对——初次配对的连接参数带宽不足，重配对即修复，已列为标准处置；Bugs\2026-09-04-rc003-voice-quality.md）。剩余待验：快速连按成对性、断连/睡眠恢复残留复验。
- [x] 实现 RC001/RC003 选择持久化、意外断连指数退避重连和 Windows 睡眠/恢复通知代码路径；真机恢复仍待验收。
- [x] 在 Windows 主机编译 Tauri NSIS Preview 安装包；Windows CI 已生成并复验绑定精确来源 Commit、SHA-256 和未签名状态的 artifact，安装、升级、卸载与正式签名仍待完成。
- [x] 提供去标识化运行诊断摘要和页面内复制入口；自动化已证明不导出设备身份、路径、端点名称或错误原文，Windows WebView 剪贴板仍待运行验收。
- [x] 持久化并展示仅保存在本机的每日按键次数、完整语音会话次数和语音采样时长；Windows/RC001/RC003 真实事件计数与升级保留仍待真机验收。
- [x] 对低于 Windows 10 1809（build 17763）的系统增加 NSIS 安装与应用启动双层拒绝门禁；Windows 10 1809 / Windows 11 提示和安装行为仍待真机验收。
- [x] 在 Windows CI 对 NSIS Preview 执行 `/S` 当前用户安装、启动存活、`/S` 卸载及设置保留边界验证；该自动化不代替可见安装界面、SmartScreen、Windows 10 1809 或真实用户环境验收。
- [x] 在 Windows CI 使用仅测试构建可启用的平台仿真，验证真实 WebView JavaScript → Tauri IPC → Rust command、五页导航、RC001/RC003 扫描、首次 RC001 语音、音频端点、Raw Input、映射、诊断和资源释放闭环；生产 NSIS 已验证不含仿真入口，该结果不代表真实 Windows API 或硬件通过。
- [ ] 使用真实 RC001 验证型号识别、BLE 配对、连接、断开、重连和首次语音。
- [ ] 使用真实 RC003 验证型号识别、BLE 配对、连接、断开、重连和首次语音。
- [ ] 验证 `STREAM_START → AUDIO → STREAM_STOP` 首次会话完整可用。
- [ ] 在 Windows 真机验证 WASAPI 端点初始化、VB-CABLE 回环、欠载恢复与完整尾音。
- [x] 在可见 NSIS 安装完成后检测 VB-CABLE 服务，未安装时说明第三方来源、管理员权限和重启要求并打开官方下载页；应用首次启动复检唯一 CABLE Input 并在无既有选择时自动配置。静默安装不打开网页，真实安装/重启仍待真机验收。
- [ ] 如未来需要捆绑或自动执行 VB-CABLE 驱动包，先取得与 Pack45 内附许可一致的作者书面授权，并实现来源校验、显式 UAC、结果检测和重启流程。
- [x] 持久化用户选择的输出端点，并在端点消失或更名时失败关闭；Windows 运行时恢复仍待真机验收。
- [x] 实现设备路径 fail-closed、隐藏消息窗口、Keyboard/HID 双来源合并和停止释放的 Raw Input 代码路径；Windows 与 RC001/RC003 真机按键验收仍待完成。
- [ ] 实现按键映射保存、热加载和 SendInput：独立映射文件、显式热加载、批量 SendInput、部分提交回滚和界面测试已完成；真实 Raw Input 边沿自动执行须分别等待 Windows/RC001/RC003 确认 Keyboard/HID 事件形态，避免重复输入。
- [ ] 完成 Windows 10 1809 / Windows 11 安装、升级和卸载验证。
- [x] 在 Windows CI 构建较低版本 NSIS 候选，验证当前用户安装、升级后单一安装身份、设置/映射/统计逐字节保留、降级不替换当前版本和最终卸载保留用户数据；该矩阵不代表真实历史二进制、可见安装界面或 Windows 10 1809 / Windows 11 真机验收。Tauri 2.11.1 静默页不会可靠设置内置降级检查所依赖的版本比较结果，已在既有 preinstall hook 中增加独立 SemVer 门禁；Run 33637195089 通过并确认 predecessor `/S` 返回 1638、当前 0.1.0 与用户数据保持不变。
- [ ] 建立自签 Authenticode、证书指纹和 SHA-256 发布流程。
- [ ] 单独评估返回键、音量键等完整 HID 实验能力。
- [ ] 按 ADR 0002 立项可选 Helper（虚拟键盘驱动 + 按设备吞键）：物理按键对照已完成（2026-09-04，豆包/微信物理可唤起、注入不可，见 Bugs/2026-09-04）；待完成 RC001/RC003 按键形态真机确认；驱动来源初查完成（cgutman/WinUHid，MIT，源码小可审计，无预编译 Release 需自构建签名），详见路线图阶段 E；不得进入基础路径，不阻塞 Preview。

### 2026-09-05 附加

- **BLE 僵死链路自动恢复（bluetooth_radio.rs，新增）**：应用被强杀后 OS 侧 GATT/HID 链路或服务缓存可能僵死，普通重试永不恢复（真机取证 + Qt 论坛同结论：关开蓝牙是唯一有效公开 API 手段）。重连循环连续失败 5 次（约 60s）自动关开蓝牙无线电一次（Off→2s→On），每僵死周期最多 2 次防抖动，UI 提示全程可见；WinRT Radio API 未打包进程可用、无需提权（真机验证：开关周期后重连立即成功）。详见 docs\investigations\evidence\p\FINDINGS.md 2026-09-05 节与 ATTRIBUTION.md BLE 恢复调研来源。
- 语音键 F5 抑制器补防粘键配对（VVC 同款"DOWN 漏进 OS 则 UP 必放行"）：按下沿 60ms 有界等待超时泄漏时，释放沿放行，杜绝"F5 粘住→和弦全部被拒"的整机失效模式。

### 2026-09-05 IME 专项

- **语音"无法唤起"根因 = 会话活动输入法不是微信输入法**（WeType 语音热键仅在自身活跃时生效；焦点无关，桌面/资源管理器聚焦 6/6 照常开麦）。修复（ime.rs）：注入和弦前用公开 TSF API 会话级激活 WeType（TF_IPPMF_FORSESSION，零延迟 3/3 实证），失败不阻断。参考 macOS 版 PreferredInputSourceMonitor 职责设计。
