# 语音会话触发 WebView 整页重载，应用回退默认「按键」页

- 发现日期：2026-09-12
- 状态：已修复（真机验收通过 2026-09-12）
- 影响范围：预览版 v0.2.2–v0.2.6；Windows 10/11 x64；RC001/RC003 语音链路共用；与微信输入法是否安装/激活无关（见根因）
- 功能点：按住说话快捷键的 IME 会话级激活（`crates/sayall-windows/src/ime.rs`，由 `ble.rs` StreamStarted 调用）；前端导航（`src/App.vue`）
- 现象：按住说话快捷键预设开启时，焦点在 SayAll 窗口内按住遥控器语音键，WebView 白屏一瞬后整页重载，应用回到默认「按键」页，当前页面与滚动位置全部丢失。
- 复现条件（真机日志实证，2026-09-11 20:36–21:40 会话段）：
  1. 快捷键预设 ≠ 「关闭」（预设关闭时 `ime.rs` 不会被调用，不复现）；
  2. 焦点（前台窗口）为 SayAll 自身；
  3. SayAll 会话的活动输入法**不是**微信输入法（走 `ime_activation` 冷路径）；
  4. 按下遥控器语音键。
  四条同时满足必现；微信输入法在 SayAll 会话已活跃（热路径，`active_is_wetype=true`）时不复现。
- 正常预期：IME 会话级激活即使失败也不应破坏宿主自身 WebView；即使 WebView 因任何原因重载，用户所在页面也不应回退。
- 证据（全部来自本机 2026-09-11/12 实测，`sayall-diagnostic.log` + GUI 注入实验）：
  - 日志相关性 100%：每个执行过 `ime_activation` 的会话，13–30ms 后出现 `webview document_load phase=started` + `frontend vue_mount` 全套前端重启（如 21:32:25.798 [04 03 02 58] → 25.821 ime_activation failed → 25.834 document_load）；热路径会话（`ime_query active_is_wetype=true`，无 activation 行）零重载（21:42:12、21:42:24、21:45:10、21:53:46、21:57:14 均无 document_load）。
  - 对照实验（SayAll v0.2.6 焦点窗口 GUI 注入）：Ctrl+Win 和弦（80ms 间隔）→ 不重载；孤立 F5 keyUP → 不重载；完整 F5 down/up → 立即重载回默认页（另一条同症状路径：WebView2 把 F5 作浏览器加速键）。
  - 排除渲染进程崩溃：`EBWebView/Crashpad/reports` 为空，Windows 事件日志无 msedgewebview2 崩溃记录（仅一条 2026-06-27 的 RADAR 内存泄漏事件，与本 bug 无关）。
  - 2026-09-11 20:36–20:39 的 `suppressor_stats leaked` 0→149 暴涨与重载窗口重合：WebView 重载期间武装/吞键时序被扰动，遥控器 F5 重复流漏进系统，属重载的次生现象而非根因。
- 根因：
  - 已确认：`ble.rs` StreamStarted 在注入和弦前调用 `ime::activate_wetype_session()`（STA 线程 TSF `ActivateProfile`，`TF_IPPMF_FORSESSION`）。该调用作用于**前台焦点窗口的会话**——焦点在 SayAll 自身时即作用于自己的 WebView2 会话。实证该 TSF 操作会使 WebView2 整页重载（激活失败与否无关，`error_code=activation_failed` 时同样触发）。热路径（会话已是微信输入法，仅查询不切换）不触发。
  - 已证伪（本轮对照实验）：Ctrl+Win 和弦本身、F5 解粘 UP 注入、渲染进程崩溃——均非真机路径的触发原因。
  - 未定边界：TSF 会话切换使 WebView2 重载的内部机制（导航 vs 渲染进程复位）未取证；用户日志中少量会话（21:32:46–21:33:31 等）激活失败但未见 document_load，存在未知的第三个变量（疑似前台窗口在激活瞬间已不是 SayAll）。
- 修复：
  1. **前端兜底（已实施，`src/navigation.ts` + `src/App.vue`）**：当前页持久化（localStorage，重载后停在原页面）；F5/Ctrl+R 页面层拦截（WebView2 预处理加速键可能先于页面拦截，仅纵深防御——Edge 实测 F5 仍会刷新，此层对加速键无效但无害）；活性心跳 + `webview_reload_recovery` 前端事件（日志可区分冷启动与重载恢复）。
  2. **Rust 根因修复（已实施，`crates/sayall-windows/src/ime.rs`）**：`activate_wetype_session()` 入口新增 `foreground_is_self()` 守卫（`GetForegroundWindow` + `GetWindowThreadProcessId` 对比 `std::process::id()`，与同文件 `foreground_process_name` 同款 API 模式）——前台属于 SayAll 自身进程时跳过激活，日志记 `ime_activation outcome=skipped_self_foreground`，返回 `Ok(WeTypeActivation::SkippedSelfForeground)`（非错误，不置 UI last_error）。跳过是严格更优：此时注入的和弦本来就落在自己窗口上，激活没有任何收益。修复验证：`cargo check -p sayall-windows` 通过、`cargo test -p sayall-core` 30 项 passed、`cargo fmt --all -- --check` 通过（本地 GNU 工具链）；`sayall-windows --lib` 测试在 windows-gnu 工具链上启动即 0xc0000005，已用 git stash 对照证实为 GNU 工具链环境限制（无改动基线同样崩溃），与本次修复无关，MSVC（CI）路径不受影响。
- 验证：
  - 自动化：`pnpm test` 65 项 passed（含 navigation 新增 8 项：持久化往返、非法值忽略、心跳新鲜度、刷新键识别）；`pnpm build`（vue-tsc + vite）passed；Rust 侧 `cargo check -p sayall-windows`、`cargo test -p sayall-core`（30 项）、`cargo fmt --check` passed（2026-09-12 本地 GNU 工具链）。
  - 同引擎浏览器重载验证：修复版经 `pnpm dev` 在 Edge（Chromium，与 WebView2 同引擎族）加载，停在「连接与语音」页注入 F5 触发真实整页刷新 → 页面恢复到「连接与语音」而非默认「按键」页，`passed`。
  - WebView2 真机（CI 产物安装，2026-09-12）：fork CI（run 34660987204）MSVC 构建 unsigned NSIS 成功，SHA256 校验一致后静默安装；聚焦 SayAll 注入 F5 → **页面不再重载**（无新 `document_load`/`vue_mount`，页面层拦截在真实 WebView2 生效），停留在「连接与语音」页，`passed`。待遥控器语音键原四条件场景复核 `skipped_self_foreground`。
  - 真机（RC003，2026-09-12 08:41–08:43 用户实按）：焦点在 SayAll 按语音键 3 次 → 全部记 `ime_activation outcome=skipped_self_foreground`（守卫生效）且**零 `document_load`**，页面自始至终停留在「连接与语音」页，`passed`；焦点在其他应用的回归 3 次 → `already_active` 热路径、`wetype_check reacted=true`、每次会话推流 5–9 万样本（约 4–6 秒语音）正常，`passed`。两项合计 `passed`。
- 隐私检查：本文档与修复代码不包含个人路径、设备身份、语音内容或凭据；心跳与上报仅含时间戳和固定事件名。
