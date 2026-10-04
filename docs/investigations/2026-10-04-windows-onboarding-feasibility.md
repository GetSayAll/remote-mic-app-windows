# Windows Onboarding 可行性探针（P0）记录

- 日期：2026-10-04
- 分支：`feat/onboarding`（基线 `origin/main` @ `154ac7b9`）
- 设计稿（本地记录，`artifacts/` 不入库）：`artifacts/design/2026-10-04-onboarding-design.html` §9
- 范围：设计稿 P0 四项探针——① 测试输入框接收输入法文字；② 一次 attempt 的终态证据；③ `ms-settings:` 打开方式；④ 物理键观察窗口。
- 规则：探针未通过前不实现对应依赖项（第⑤步「按住说话验证」的文字门禁依赖 ①）。

## ① 测试输入框接收输入法文字（真机已执行：根因方向=会话输入法）

**问题**：第⑤步门禁要求「第三方工具把文字写进向导窗口自己的测试输入框」，且必须能区分手动键盘输入与语音写入。这只能在本机用真实遥控器 + 真实输入法验证，无法由自动化或仿真替代。

**现场探针计划**（约 15 分钟，三种工具各跑一遍）：

1. 准备：RC003 已配对；VB-CABLE 就绪；分别选择豆包 / 微信 / Vokie。
2. 正常路径：点进测试输入框 → 按住遥控器语音键说一句 → 松开。期望：文字出现在输入框；应用侧 attempt 证据齐全（会话开始 + 解码采样 > 0 + 投递排空 + 会话结束）。
3. 阴性对照：不碰遥控器，直接用键盘打字。期望：判定为手动输入（不通过），日志 `manual_input=true`。
4. 边界：快速点按（<0.5 秒）→ 失败可见且可重试；松开后迟到的文字不超过 3 秒窗口仍被计入同一 attempt。
5. 记录：输入框 DOM 事件序列（composition / input）、焦点读回、物理键计数、日志片段（脱敏后归档）。

**2026-10-04 决定修订（Andy）**：第⑤步不再等探针①先行——按「先做正式那一步，现场测试即验收」执行。
因此探针①的现场会话与第⑤步验收合并进行：正常路径（豆包 / 微信 / Vokie 各一遍）、阴性对照（键盘打字 →
判定为手动输入）、焦点用例（未聚焦 / 测试中失焦 → 对应失败码）。判定与证据仍回填本文；探针①的原始问题
（输入法能否把文字写进向导输入框、DOM 事件是否可观测）由验收过程直接回答。

**现场结论（2026-10-04 真机，RC003）**：❌「工具把文字写进向导自己的 WebView 输入框」这个前提**当前不成立**：

- 物理键盘右 Alt 在向导 WebView2 输入框内**无法唤起豆包语音条**（少数情况手动切换输入法后出现，
  且语音条出现后"卡住"）；键盘拼音可正常输入该框（输入法本体工作）；语音尝试后无"迟到文字"；
- 同一操作在**记事本 / 微信 / Chrome 里正常**；Chrome（同为 Chromium）里右 Alt 可唤出语音条
  → 不是 Chromium 通病；与本窗口"会话活动输入法不是目标工具输入法"的机制一致
  （`ime.rs` 模块注释：语音热键只有当目标输入法是当前会话活动输入法时才生效，2026-09-05 持锁实证）；
- 微信输入法（左 Ctrl + 左 Win 和弦）在向导框内同样**不能出字**；Vokie 未验证；
- 自动观察链路本身完好：语音会话、采样、投递、排空、手动输入检测都正常（日志实证）。

**修复方向（2026-10-04 复核，Andy 明确：保持嵌入网页输入框，修根因）**：

1. 读回本窗口会话的活动 TSF 配置（`examples/doubao_ime_probe` 已有 active 读回；真机确认自身
   窗口前台时读到的确是本窗口会话）；
2. 若确认为"会话输入法不是目标工具"，验证一种**不触发 WebView2 页重载**的会话切换方式
   （候选：向自身窗口 `WM_INPUTLANGCHANGEREQUEST` + 目标配置的 HKL；避开 Bugs/2026-09-12
   记录的 `ActivateProfile` 重载路径）；
3. 接入第⑤步（进入步骤或开始 attempt 时校准会话输入法并落日志），验收重跑并回填本节。

## ② 一次 attempt 的终态证据（代码盘点：可行，有明确缺口清单）

现状（代码事实）：

- 会话开始/结束：`ConnectionSnapshot.voice_state`（`idle / streaming / draining`）随 1 秒运行快照轮询；GATT 会话段另有日志。
- 解码采样：`ble.rs:1832`（`decoded_samples` 累计）。
- 音频提交：`AudioSnapshot.queued_samples / submitted_samples`；排空完成 = `audio.rs:1164 is_drained()` → `audio.rs:848 finish_drain()` → 回到 ready，并落
  `audio_session action=finish phase=completed terminal_result=passed submitted_samples=… queued_samples=…`（`audio.rs:571`）。
- 失败：`audio.last_error`、`fail_audio` 日志（`audio.rs:909` 等）；入队失败走 `abort_voice_session`（`ble.rs:1817-1829`）。

缺口（P2 需要补齐，不改变既有语义）：

1. **没有「一次 attempt」的结构化终态载体**：开始/结束/排空/失败目前分散在快照字段与多条日志里，前端 attempt 需要一个唯一终态。
2. **没有 push 事件**：1 秒轮询捕获不到短暂会话中的中间事实；短按（按下-立即松开）的时序边界不可靠。
3. **缺少 attempt 级聚合**：enqueue 失败次数、中断次数、排空完成布尔、代次（generation）没有与 attempt 关联的单一记录。

建议（与设计稿 §5.5 / §8 一致）：新增 `voice-session` 事件（`phase=started|ended` + `generation` + 解码/提交/队列增量 + `drain_complete` + 错误分类），前端 attempt 控制器以 `attempt_id` 关联并落唯一终态；或等价方案 = 测试页 ~100–200ms 快照轮询 + 会话终态聚合。**实现前先在真机做正反对照**（正常会话 vs 中途断连/拔电），确认字段能区分「没开始 / 没采样 / 投递失败 / 未结束」。

**2026-10-04 决定**：采用等价方案（快照轮询 + 终态聚合），控制器已落地为
`src/onboarding/voice-attempt.ts`（纯逻辑 + 10 项测试）：`decodedSamples` 用差值
（跨会话累计）、`submittedSamples` 用 `begin_session` 归零后的会话值、结束形态 =
「回到 idle 且 audio ready 且队列为 0」；BLE 推送事件留作后续可选增强。
真机正反对照仍是第⑤步上线前的验收项。

## ③ `ms-settings:` 打开方式（只读部分 passed；打开动作待真机）

- 只读检查（2026-10-04 本机）：`HKCR\ms-settings` 协议处理器存在。
- 方案：Rust 新增命令 `open_windows_settings(section)`，`section` 为枚举（`bluetooth` / `sound` / `microphone`），内部把固定 URI 交给 `ShellExecuteW`；不接受任意字符串，失败返回原因。命令名与枚举先入 `src-tauri/src/lib.rs`，并与 `capabilities` 最小权限原则一致（与「打开日志目录」同模式）。
- 真机验证（打开一次系统设置页）列入验收清单；自动化不弹系统窗口。

## ④ 物理键观察窗口（可行性 passed：代码证据）

- `crates/sayall-windows/src/key_gate.rs` 的低级键盘钩子已能区分注入与物理边沿：`LLKHF_INJECTED` 读取（`key_gate.rs:471-472`）、放行语义（`499-508`）、计数（`CAPTURE_INJECTED_ACCEPTED` 等）。
- P2 实现：新增「观察窗口」（开始/结束 IPC + 窗口期内**非注入** keydown 计数），供第⑤步的手动输入检测；检测不可靠时 fail-open（例：计量失败按未知处理，不误判为手动输入）。
- **2026-10-04 实现落地**：`begin_key_observation(exclude_vks)` / `end_key_observation(window_id)`
  （IPC + 前端 bridge；计数核心在 `crates/sayall-windows/src/key_gate.rs`，2 项单测）。计数语义细化为
  「**可能进入 OS 的物理按下沿**」：非注入 + 未被门控吞下（被吞的遥控器孪生边沿到不了输入框）+
  不在调用方排除集内（报告层合成把「按住说话」和弦以非注入形态送进 OS、必须到达输入法，
  见 `ble.rs` 合成门禁——不排除会污染每一次正常 attempt）。窗口 id 单调递增；陈旧 id / 重复关闭 /
  钩子中途停止 → `None`（计量不可靠 → fail-open，不得据此判定手动输入）。
  真机正反对照（正常语音会话不误报 vs 物理键盘打字必报）仍是第⑤步上线前验收项。
- 注意：本机沙箱禁止合成输入（`SetCursorPos`/`SetForegroundWindow` 被拒），窗口行为的真机验证需要操作人手动按键。

## 结论

| 探针 | 状态 | 影响 |
| --- | --- | --- |
| ① 输入框接收文字 | 真机已执行：前提不成立 → 转"会话输入法校准"修复（见上） | 第⑤步文字门禁改为网页输入框 + 会话输入法校准 |
| ② attempt 终态 | 盘点完成，缺口已列 | P2 增加事件/聚合后实现第⑤步 |
| ③ ms-settings | 只读 passed；打开待真机 | 第②步「打开蓝牙设置」按钮先按此方案实现 |
| ④ 物理键窗口 | 代码可行 → **已实现**（IPC + 单测；真机正反对照待现场） | 第⑤步手动输入检测的前置就绪 |

下一步：P1 骨架（状态迁移 + 流程状态机 + 向导壳 + 步骤①②③）继续；②③④ 的实现按 P2 排期，① 在下次现场会话执行并回填本文。
