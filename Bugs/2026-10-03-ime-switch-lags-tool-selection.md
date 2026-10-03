# 语音键第一按丢失（换工具后）：输入法切换只在按下时执行，必然晚于报告层合成

- 发现日期：2026-10-03（用户现场复测反馈 + 应用诊断日志；本地测试包 0.5.0 / main `af6b736f` 安装版）
- 状态：已实现修复（选中即对齐 + 失焦执行 + 按下兜底 + 切换自校验）；**真机复测 deferred**
- 影响范围：Windows 11、RC003 + 豆包输入法、报告层合成路径（「全按键支持」开启时）。
  触发面 = **按下瞬间系统活动输入法不是豆包**（在连接页换过工具、或重启后系统输入法未对齐）；
  表现为第一按（或前几按）拉不起豆包语音条，等待/补按后恢复。
- 功能点：输入法切换时机（`crates/sayall-windows/src/ime.rs`、`WindowsPlatform::set_voice_input_tool`、Tauri 窗口事件）
- 现象：换过语音输入工具后，切到目标应用第一次按语音键没反应；隔几秒再按恢复正常（用户原话：「第一按拉不起，得等一会再按」）。
- 复现条件：连接页把语音输入工具选为豆包（或重启后豆包不是当前窗口活动输入法）→ 切到目标应用 → 第一次按语音键。
- 正常预期：按下即拉起语音输入（Andy 2026-10-03 要求：所有情况下都要能拉起；第一按也尽量不丢）。
- 证据：应用诊断日志（本地 2026-10-03 15:49:32 / 16:06:24 / 16:12:57 三处失败行，
  与 `synth:frame replace ok to=0xe6` 同会话比对）：
  - 失败组：该按的报告层替换**先发出**；随后 `ime_activation ... active_is_target=false
    outcome=switched elapsed_ms≈53`——切换在合成之后才完成，这一按落在豆包激活之前。
  - 成功对照组：`active_is_target=true`（already_active），每次替换与音频会话都完整。
  - 判定：失败集 == 「按下瞬间系统活动输入法 ≠ 豆包」的集合；clean burst 未出现多连失败。
- 根因：已确认事实——
  1. Windows 的输入法状态按窗口/会话生效；连接页「选择豆包」只写应用设置，不改变任何系统输入法状态。
  2. 报告层合成在设备按下帧到达时**立即**改写（早于应用知情约一个 BLE 往返），应用侧无知情点。
  3. 切换时机设计只有「按下时」这一个点（2026-10-01 语义，见 `ble.rs` 注释）。
  ⇒ 结构性缺口：「换工具」来源的第一按，合成必然早于切换——**与门（gate）无关**（门已默认关闭，
  见 `Bugs/2026-10-03-gate-hold-ioctl-stuck-voice-session.md`）。
  仍未知边界：系统在什么条件下会自动把豆包设为活动输入法（存在 already_active 成功组），未探究。
- 修复（最小改动）：
  - 新增时机「选中即对齐」：`WindowsPlatform::set_voice_input_tool` 在选中当下若自身不是前台
    → 立即异步切（`ImeSwitchScope::ToolSelect`）；自身在前台 → 布防一次性对齐
    （`ime_align_pending`）。不在自身前台立即切的原因：TSF 会话切换曾致 WebView2 整页重载
    （`Bugs/2026-09-12`）。
  - 新增触发：`src-tauri/src/lib.rs` 的 `WindowEvent::Focused(false)`（main 窗口）→
    `align_ime_after_tool_selection` 取走布防并执行一次切换。
  - 按下路径保留兜底切换（`ImeSwitchScope::VoicePress`）；切换确实晚于按下时记
    `voice_hold action=switch_after_chord`（日志可按 session 归因「这一按为何没反应」）。
  - 切换后自校验：`ime_verify scope=... attempt=1|2 stuck=true|false`（true=读回仍不是
    目标，失败方向）——读回 `GetActiveProfile` 比对，不满足则重试一次 `ActivateProfile`；
    仍不满足返回错误（不是"S_OK 即成功"）。重试路径先落 `attempt=1 stuck=true` 再落终态行，
    便于统计"重试救回"的频率。
  - 新增日志：`ime_tool_select action=immediate|pending_self_foreground|fired|skipped`
    （fired 带 waited_ms）、`ime_verify`、`voice_hold action=switch_after_chord`。
- 覆盖与边界：
  - 覆盖：换工具来源的第一按；冷启动/闲置后首用（选中时已布防，按下兜底仍在）。
  - **不覆盖**：用户手动改走输入法（不经过应用）后的第一按——按下兜底为「下一次生效」，
    已用 `switch_after_chord` 显式留痕；是否消除需真机数据决定（deferred）。
- 验证：
  - 本地：`cargo fmt --all -- --check`、`cargo test -p sayall-windows`、
    `cargo check -p sayall-windows-app --features runtime-simulation`（见提交说明；结果 passed）。
  - **本机真机（安装后启动即验，2026-10-03 17:41 本地）**：新链路端到端跑通——
    `ime_tool_select action=pending_self_foreground`（启动恢复设置时自身在前台，布防）→
    143ms 后 `action=fired trigger=window_blur waited_ms=143`（失焦执行）→
    `ime_verify scope=tool_select attempt=1 stuck=false`（读回一致，未走重试）→
    `ime_activation ... scope=tool_select outcome=switched elapsed_ms=53 last_switch_age_ms=0`。
    ⇒「切换发生在按下之前」这一前提在真机成立；按下的实质效果待用户语音键复测。
  - 真机矩阵（deferred，待用户执行）：RC003 + 豆包——冷启动首按、换工具后立即按、
    连续会话、快速连按、断连恢复；RC001 同矩阵（注入路径为"先切后注"，同样受益于选中即对齐）。
- 隐私检查：仅含工具标签、作用域、会话编号与相对时间；不含 BLE 地址、输入法 GUID、用户路径或语音内容。
