# 按住说话快捷键录入：与输入法语音和弦相同的组合录不全（完成键边沿被外部钩子吞掉）

- 发现日期：2026-09-27
- 状态：已修复（注入副本路线；真机复测 deferred）

> **根因已于同日两次修正，本节前两条结论已被后续实证推翻，保留作过程记录：**
> 1. 「最新安装最先调用」是**错的**——LL 键盘钩子链为 **FIFO（最早安装最先调用）**，
>    本机实验实证见 `docs/investigations/2026-09-27-ll-hook-chain-order-fifo.md`。
>    因此"重装到链头"（bump）是反向操作，已全部移除。
> 2. 真正的机制是**「吞下 + 重放」**：输入法吞掉完成键的物理边沿后，把整个组合
>    以**注入副本**重放。本应用录入通道按"防自吞"设计跳过注入事件，等于把完成键
>    整对丢弃 → "只剩第一个键"。修复见文末「三次定位」。
- 影响范围：所有含快捷键录入功能的 Windows 版；连接页「修改快捷键」；不影响 RC001/RC003 链路与注入时序
- 功能点：key_gate 录入通道（`crates/sayall-windows/src/key_gate.rs` 的 `set_shortcut_capture_active` / `handle_shortcut_capture`）
- 现象：已设置的按住说话快捷键为 左 Alt + 左 Win 后，重新进入录入按同一组合，保存结果只剩 左 Alt；换一个不重叠的组合（如 左 Win + 右 Alt）则完整可录
- 复现条件：当前生效快捷键 = 输入法（微信输入法）语音和弦（本机 左 Alt + 左 Win），录入时按下该组合
- 正常预期：保存 `[left_alt, left_windows]`，与按键盘面一致
- 证据（`%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`，`pid=10220`）：
  - `2026-09-27T06:01:54Z`（左 Win + 右 Alt，非输入法和弦）：`LeftWindows↓ RightAlt↓ RightAlt↑ LeftWindows↑` 四边沿全部 `phase=observed` → 完整可录
  - `2026-09-27T06:01:49Z` / `06:01:57Z`（左 Alt + 左 Win = 输入法和弦）：只有 `LeftAlt↓ LeftAlt↑`，`LeftWindows` 的 DOWN/UP 完全缺失 → 前端按单修饰键落盘成 左 Alt
  - 同窗口无 `chord_press`/语音会话（05:53/05:56 的会话均已结束）→ 排除应用自身注入占用
  - 应用注入 左 Alt + 左 Win 和弦可稳定触发微信输入法语音（`chord_press result=ok` + `audio_stream phase=started`）→ 反证输入法监听并吞掉的正是该和弦
- 根因：微信输入法等目标会安装自己的 `WH_KEYBOARD_LL` 钩子并在输入焦点变化时重建；LL 钩子按"最新安装在最前"调用，其语音和弦判定先于本应用 key_gate 录入钩子看到物理边沿。当录入组合恰好等于输入法语音和弦时，完成键（左 Win）的 DOWN/UP 整对被输入法吞掉，key_gate 完全观察不到，前端只能按先松开的单个修饰键落盘。key_gate 自身录入分支（成对吞 + 投递 webview）无缺陷——事件根本没到达。
- 修复：录入开始时把 key_gate 钩子提升到 LL 链头（`request_hook_bump`：先挂新钩再卸旧钩，无吞键空窗，Voice_VibeCoding 同款技巧，key_suppressor 已有先例）。录入期间本钩子先于所有外部钩子看到物理边沿，成对吞下并投递录入通道；输入法在录入期间看不到任何按键，也不会误触发语音。`set_shortcut_capture_active(true)` 内自动发起；lib.rs 启动日志追加 `hook_bump=requested` 与 `hook_bump_count` 便于归因。仅改 `key_gate.rs` 与 `src-tauri/src/lib.rs` 日志行；录入协议（`capture_key_code`、配对规则）与前端零改动。
- 验证：`cargo test --workspace` passed（新增 `capture_start_requests_hook_bump_and_gate_off_refused`：bump 请求可观测、门控未就绪时录入失败且不发起 bump）；`cargo fmt --all` 无漂移；`cargo check --workspace` 与 `cargo check -p sayall-windows-app --features runtime-simulation` passed。**真机复测 deferred**：需安装包含本修复的构建后，在快捷键=左 Alt+左 Win 状态下重录该组合确认四边沿全部 `phase=observed` 且保存完整。
- 残留风险（接受并记录）：若外部钩子在录入开始之后、按键之前重新安装到链头（本机实测未发生：输入焦点变化发生在点击"修改快捷键"时，早于录入开始的 bump），边沿仍可能被抢。**2026-09-27 晚更新**：该竞态实为"定期链头 bump 从未执行"所致——`hWnd=NULL` 的线程定时器忽略传入 nIDEvent，原按传入 id 匹配 WM_TIMER 的分支永不命中（见 Bugs\2026-09-27-capture-preheld-keys-block-recording.md 二次定位）；已修正为按 SetTimer 返回值匹配，并新增"每投递一条边沿即重抢链头"。
- 隐私检查：日志仅含虚拟键码、边沿方向与计数，无设备身份、个人路径、语音内容或凭据。
- 边界（属既有语义，本次不改）：
  - "分次输入"（先按 Alt 松开、再按 Win）会在 Alt 松开时即落盘为 Alt（见同日 Bugs\2026-09-27-voice-hotkey-capture-drops-modifier.md）。
  - 录入开始前已被按住的键（pre-held）的 DOWN 对录入不可见（key_gate 为防粘键刻意放行）。

## 三次定位：注入副本才是被吞键的唯一可观测形式（2026-09-27 晚，本机 + 真机）

### 真机数据（第三次安装的构建，`pid=35816`，`%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`）

- 两次「按 左 Ctrl + 左 Win」会话（`10:08:29` / `10:08:32`，各约 1s）：
  - `keys_seen=6`（录入激活期间到达本钩子的键盘事件数）与
    `injected_skipped=4`：**2 条真实事件（`LeftControl↓↑`）+ 4 条注入事件**
  - `action=edge key=LeftControl edge=down/up` 只有 Ctrl 两条被投递
  - `shortcut_settings feature=voice_hold action=save key_count=1`：**前端按单键落盘**
    （`[left_control]`）——这就是"无法录入快捷键"的真身
- 「左 Win + 右 Alt」（非输入法和弦）四边沿全为 `phase=observed`（历史证据）→
  只有"完成键恰好是输入法语音和弦的那一下"被吞。

### 机制

1. 输入法（微信输入法）的 LL 钩子先于本应用（链为 FIFO），其语音和弦
   （= 产品默认的 左 Ctrl + 左 Win，见 `DEFAULT_VOICE_HOTKEY_KEYS`）在**完成键**
   按下时匹配：它吞掉该键的物理 DOWN/UP。
2. 它随后**把整个和弦以注入副本重放**（对应本应用看到的 4 条 `LLKHF_INJECTED`
   事件 = 2 键 × DOWN/UP）。被吞物理边沿的唯一可观测形式就是这些副本。
3. 本应用录入通道出于"防自吞"（避免吞掉自己的注入）**跳过全部注入事件**
   → 完成键被丢弃 → 前端只能在第一个键松开时落盘 → 半截组合。

### 修复

- **`injected_edge_is_passthrough(capture_active, injected)`**（新纯函数，含单测）：
  非录入期的注入事件照旧透传（防自吞），**录入期的注入副本必须作为录入观测接受**。
- 边沿协议新增 `source: "real" | "injected"`（`EdgeSource`，序列化 camelCase），
  `shortcut_capture action=edge` 日志随之带上来源——"完成键来自重放副本"从此可直读。
- 未能映射成协议键的录入事件不再静默：新增
  `unmapped_seen` / `last_unmapped_vk` / `last_unmapped_injected` 诊断字段。
- 前端新增**落盘稳定窗口**（`VOICE_CAPTURE_SETTLE_MS = 200`）：全部按键松开后
  不立即定稿，等重放副本到齐；窗口内出现新的按下沿即取消并重新计时
  （`scheduleVoiceCaptureFinish` / `cancelVoiceCaptureSettle`）。
- 路线①（录入期把录入窗口线程的输入区域切到非 IME 布局）保留：真机日志
  `capture_ime_yield outcome=switched/restored` 均成功、无 WebView2 重载，
  且未使录入变差；但它**不足以**阻止吞键（切换后左 Win 物理边沿仍不可见），
  真正的修复是接受注入副本。

### 验证

- `cargo test --workspace`：全绿（新增 `injected_edges_are_accepted_only_during_capture`、
  `edge_source_distinguishes_injected_copies`，诊断字段契约测试同步更新）
- 前端 `vitest run`：113 passed（新增「被吞键仅以重放副本到达时仍落盘完整组合」回归）
- `vue-tsc --noEmit`、`cargo check --workspace` / `--features runtime-simulation` 通过
- 本地 NSIS 包双次静默安装 + 哈希核对通过
- **真机复测 deferred**：重录 左 Ctrl + 左 Win 应落盘两键；日志中该边沿应为
  `source=injected`，且 `injected_accepted` 随按键增长。
