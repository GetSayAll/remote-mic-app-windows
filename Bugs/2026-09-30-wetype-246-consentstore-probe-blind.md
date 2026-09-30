# 微信输入法 2.1.4.6 起录音不写 ConsentStore，开麦观测失明导致重放和弦拆掉进行中的语音会话

- 对应报告：issue [#118](https://github.com/GetSayAll/remote-mic-app-windows/issues/118)（2026-09-23 社区报告，0.2.12）；
  社区修复尝试：PR [#119](https://github.com/GetSayAll/remote-mic-app-windows/pull/119)（未合入）。
- 发现：2026-09-23；本仓处理：2026-09-30。
- 状态：**症状在本机可复现**（0.3.0 + WeType 2.1.4.6，2026-09-30）；改动单元验证 `passed`；
  真机验收 `deferred`（需在复现时刻用新构建采集 `evidence=marker`，见"验证"节）。
- 影响版本：0.2.12 起（开麦观测判据自 0.2.3 引入），0.3.0 仍存在。

## 本机证据（2026-09-30，Windows + WeType 2.1.4.6 + 0.3.0）

同一台机器上，**症状与"探测正常"在同一天交替出现**，说明根因是状态相关的，而不是社区报告
所称的"2.1.4.6 完全不写 ConsentStore"：

- `sayall-diagnostic.log` 当日 16 次 `reacted=false`、2 次 `chord_retry result=ok`。示例（本地时间 16:55）：

  ```text
  08:55:36.274Z audio_stream phase=started ...        ← 用户按住
  08:55:36.963Z wetype_check reacted=false attempt=0 epoch=8 reviving
  08:55:37.017Z wetype_revive result=ok attempt=0 epoch=8
  08:55:39.184Z chord_retry result=ok attempt=1 epoch=8   ← +2.9s：和弦被释放并重按
  08:55:39.885Z wetype_check reacted=false attempt=1 epoch=8 reviving
  ```

  与 issue 报告的时间线（+0.7s 判未响应、+2.9s 重放和弦）逐项一致。
- 20 分钟后的会话（17:11:01 / 17:11:10 / 17:13:20）全部 `reacted=true attempt=0`，无 `reviving`。
- 同一时刻 ConsentStore 里 WeType 条目**确有更新**：`C:\Program Files\Tencent\WeType\2.1.4.6\wetype_update.exe`
  start=17:13:21 → stop=17:13:22，与 17:13:20.682 chord_press 的会话一一对应。

结论：判据通道**存在时好时坏的失明**，而不是"该版本一律不写"。16:55 那两次 `reacted=false`
究竟是"真休眠（WeType 确实没开麦）"还是"盲判（WeType 在录音但没写/没及时写 ConsentStore）"，
现有日志无法区分——**这正是引入存活标记通道要回答的问题**。

## 现象

按住说话（hold 模式，示例和弦 左Ctrl+左Win）说话正常开始，但按住约 **2.9 秒**时注入的和弦被"释放→重新按下"，微信输入法进行中的听写随之中断；物理松手后音频链路正常收尾（`audio_session finish` 样本数与按住时长吻合）。表现为"每次按住只能说两三秒"，长句语音输入不可用。

## 根因

`wetype_check` 的"微信输入法是否已响应本次按住"判据读 HKCU `CapabilityAccessManager\ConsentStore\microphone\NonPackaged` 下名字含 `wetype` 的条目（[wetype_revive.rs](../crates/sayall-windows/src/wetype_revive.rs)）。

微信输入法 **2.1.4.6 起录音不再写入该键**（社区报告 2026-09-23 实测：历史版本可见 `wetype_*.exe` 条目，2.1.4.6 下整个按住期间无变化）。于是判据恒为 `NotObserved`——**哪怕听写已经启动**。两次相隔约 3 秒的判定都返回 false（非 700ms 窗口过短的时序问题），说明是探测通道**失明**，而不是观测缺失：观测缺失（`Unknown`）路径本来就失败安全、不会触发恢复。

**本机（2026-09-30）对该结论提出反证**：同一版本在这里既出现过连续 `reacted=false`（16:55），也出现过连续 `reacted=true`（17:11/17:13），且 17:13 的录音确实写入了 ConsentStore。因此更准确的表述是：该判据在 2.1.4.6 上**时好时坏**（可能取决于进程/NamedPipe 时序或录音由哪个进程持有），失明时后果与报告一致。

`NotObserved` → 配置切换 → 等待 2/3/5s → 复检仍 `NotObserved` → 工作线程释放旧和弦并重注入（[ble.rs](../crates/sayall-windows/src/ble.rs) 的 `spawn_wetype_check` / `WorkerMessage::RetryVoiceChord`）。这次重放正是拆掉进行中会话的动作。

## 修复

引入**与版本解耦的正面存活证据**：微信输入法钩子存活时会吞掉和弦的 LWin 边沿并自注入一对 `0xFC` break key（extra="WTYP"），休眠时边沿泄漏、无该标记——该判据与开麦时间戳 100% 交叉一致（2026-09-05 kb-live 全解码，见 [ATTRIBUTION.md](../ATTRIBUTION.md)）。

- 观测（钩子线程内，无 IO/无锁，仅原子递增）：[key_suppressor.rs](../crates/sayall-windows/src/key_suppressor.rs) `note_key_event` 记录注入形态的 `0xFC`，暴露 `wetype_marker_count` / `wetype_marker_last_extra`。
- 裁决（纯函数，单元覆盖）：[wetype_revive.rs](../crates/sayall-windows/src/wetype_revive.rs) `reaction_verdict`——**标记前进即判 `Reacted`**，优先于开麦观测；两个判据都确认未触发才是 `NotReacted`。
- 接线三处（[ble.rs](../crates/sayall-windows/src/ble.rs)）：按下前取标记基线（与开麦基线同时机，必须在注入之前）；初次 700ms 检测；稳定期后复检与 `RetryVoiceChord` 工作线程守卫——三处任一判 `Reacted`/`Unknown` 都不再释放并重注入和弦。

**只减不增**：标记通道缺失（例如未来版本不再注入标记）时裁决退化为原行为，不会比修复前更差；真休眠场景（无标记 + 无开麦，2026-09-05 七次发作的原型）仍可执行恢复阶梯，人工兜底提示也仍可达。

日志：健康会话 `wetype_check reacted=true ... evidence=mic|marker marker_extra=0x57545950`；被门禁拦下 `chord_retry skipped reason=wetype_alive evidence=marker`；观测不可用 `reason=observation_unavailable`。标记计数与 extra 魔数不含路径、语音或设备身份。

## 验证

- 单元 `passed`：`wetype_revive::tests::{marker_evidence_vetoes_recovery_when_mic_probe_is_blind, dormant_hook_without_marker_and_without_mic_opening_stays_recoverable, marker_evidence_beats_unavailable_mic_observation, unavailable_mic_observation_without_marker_is_never_dormancy, mic_opening_without_marker_is_still_reacted}`；`key_suppressor::tests::only_injected_wetype_marker_counts_as_liveness_evidence`；`cargo test --workspace` 全绿（含 `cargo check --workspace`、`cargo check -p sayall-windows-app --features runtime-simulation`）。
- 真机 `deferred`（**本机具备条件**，WeType 2.1.4.6 + 遥控器）：用本分支的新构建按住语音键若干次（含 ≥15s 长按），在能复现 `reacted=false` 的时段采集判据：
  - 若出现 `chord_retry skipped reason=wetype_alive evidence=marker`：说明那些 `reacted=false` 是**盲判**（WeType 实际在工作），门禁按设计挡住了破坏性重放 → #118 修复在本机得到验证。
  - 若始终只有 `reacted=true ... evidence=mic`：说明该时段判据正常，需等到复现窗口再采；标记通道是否可用另需一次 `evidence=marker` 观测佐证。
  - 若长按期间出现 `wetype_check skipped_retry reason=observation_unavailable`：说明标记也缺席且开麦不可用，此时按设计不动手（保持按住）。
- 判据与用例见 [Testing/WindowsWeTypeConsentHistory.md](../Testing/WindowsWeTypeConsentHistory.md)。
- 回退边界：若真机显示 2.1.4.6 在某些时段既不写 ConsentStore **也不注入标记**，本改动对那种时段无效，需临时采用 PR #119 式开关（默认关闭自动恢复）——但需同时修掉其连带问题：默认关闭会让真休眠用户既失去自动恢复、也看不到"打开微信输入法界面"的人工提示。

## 已知边界

- 本改动覆盖"探测失明导致误判未响应"（issue #118）。若出现**新形态**"钩子活着但不开麦"
  （例如微信输入法已登出、麦克风权限被撤销、其语音面板报错）：日志会显示
  `reacted=true evidence=marker` 且无 `reviving`/`chord_retry`——按了没反应但不再被拆会话，
  与真休眠（`reviving` 后仍 `reacted=false`）在日志上可区分。该形态需另案处理，本改动不覆盖。
- 存活标记是进程级全局计数：用户在按住期间**物理**按下同一和弦（Ctrl+Win）也可能产生标记，
  从而抑制本轮重试。后果无害（只减少破坏性动作，且用户的物理和弦本身多已起听写），
  但排障时需注意该混淆项。
- 标记观测依赖 key_suppressor 的常驻 LL 钩子；钩子未就绪（应用刚启动、线程退出中）时
  计数不前进，裁决退化为修复前行为。

## 未采纳的社区方案要点（PR #119）

- 正确的部分：在无法确认休眠时不做破坏性动作（与本次门禁同向）。
- 问题：`WETYPE_AUTO_REVIVE_ENABLED = false` 是**编译期默认关闭整个恢复阶梯**，真休眠场景（探测正确地返回未响应）也一并关停；且 early return 会跳过 `attempt >= MAX` 的人工提示分支；无回归测试与文档同步。

## 隐私

只读公开的 Windows 麦克风访问历史（ConsentStore）与钩子层的按键标记；extra 为第三方自定义魔数，非用户数据。日志不含路径、语音内容、设备身份或输入文本。
