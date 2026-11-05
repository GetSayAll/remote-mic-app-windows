# 自身主窗口置顶压住「打开应用」的目标窗口

- 发现日期：2026-11-05
- 状态：修复完成，等待本地包安装与实体按键验证
- 影响范围：Windows 10/11；1.8.3 本地运行实例；按键映射 → 打开应用（预设应用、注册应用、自定义路径）；已运行的 RC003 三键链路
- 功能点：按键映射 → 打开应用（`app_launcher`）
- 现象：无线麦主窗口在前台时，TV 键映射的「打开应用」把目标应用叫不到用户眼前；无线麦窗口始终盖在最上面。目标应用其实已经拿到焦点（日志 `foreground_observed` 全绿）。
- 复现条件：主窗口位于前台且带 `WS_EX_TOPMOST`；按映射到「打开应用」的按键（现场为 TV 键 → WorkBuddy，另有 37 次连续按击）。
- 正常预期：目标窗口成为前台且用户在视觉上能看到它；做不到时必须记为失败，不能把"提交成功"当"用户可见"。
- 证据：
  - 日志 `%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`，2026-11-04T16:40:45Z–16:46:06Z（本地 11-05 00:40–00:46）：37 次 `map_fire button=Tv action=open_app`，全部
    `app_launcher action=show_and_force_foreground terminal_result=passed target_result=foreground_observed self_window=false set_foreground_ok=true` 与 `map_launch result=ok`。
  - 只读窗口查询：主窗口 `hwnd=0x140e32`（pid 35500）exstyle `0x00040118`，含 `WS_EX_TOPMOST (0x8)`；目标 WorkBuddy 窗口（pid 16396）非 topmost。
  - 对照实验（可复原，已还原）：topmost 在位时 `SetForegroundWindow(WorkBuddy)` 成功、`GetForegroundWindow()==WorkBuddy`，但 Z 序 `z(main)=31 < z(WorkBuddy)=56`（主窗口在上）；`SetWindowPos(main, HWND_NOTOPMOST, …)` 成功后同一序列 `z(WorkBuddy)=51 < z(main)=56`（目标在上）。
- 根因：
  - **已确认**：`WS_EX_TOPMOST` 属于 Z 序层，`SetForegroundWindow` 只改焦点归属、不改层级。主窗口带该位时它在视觉上永远压住非 topmost 的目标窗口。既有成功判据只读 `GetForegroundWindow()`，因此完全看不见这一层——这是本次"日志全绿但用户看不到"的直接原因。
  - **仍未确认**：置顶位的写入者。已排除的证据：仓库内没有任何 `SetWindowPos` / `HWND_TOPMOST` / `WS_EX_TOPMOST` 写操作；唯一的 `always_on_top(true)` 是常用语浮层（另一个窗口）；tao 0.35.3 的 `set_focus` 走 `force_window_active`（Alt 边沿 + `SetForegroundWindow`），`WindowFlags::ALWAYS_ON_TOP` 只在显式调用时置真，而 `tauri.conf.json` 的主窗口未配置 `alwaysOnTop`。清掉置顶位后经「最小化 → 恢复 → 强制前台」均未回弹，故判断为一次性历史状态或外部窗口工具所致。
- 修复（结果层兜底，不依赖写入者）：
  - `crates/sayall-windows/src/app_launcher.rs`：新增 `SELF_DROP_TOPMOST` 回调注册（`set_self_drop_topmost` / `request_self_drop_topmost`）、`drop_window_topmost(isize)` 与 `win_impl::drop_topmost`（读 exstyle → `SetWindowPos(HWND_NOTOPMOST)` → 读回确认，幂等；失败写 `drop_self_topmost terminal_result=failed`）。
  - 触发点在 `activate_or_launch` 入口（`should_drop_self_topmost`）：已运行置前与冷启动（`launch_explicit` 直接 return 的分支、注册应用激活）都由同一点覆盖；`sayall` 自身目标不撤顶。
  - `src-tauri/src/lib.rs`：注册回调，先读 Win32 样式（常规路径零跨线程 dispatch），只有真清掉时才 `set_always_on_top(false)` 同步 tao 的 `ALWAYS_ON_TOP` flag，避免它下次 `set_window_flags` 把 `WS_EX_TOPMOST` 再 OR 回来。
  - `show_and_force_foreground` 日志增加 `target_topmost=`，便于下次直接从日志判断目标/自身的置顶状态。
- 验证：
  - `drop_topmost_clears_externally_set_style`：自建 `WS_EX_TOPMOST` 的 `STATIC` 窗口，断言首次调用清掉、二次调用返回 false。已做阳性对照——临时让实现直接 `return false` 时，测试红在「首次调用必须真的清掉置顶位」，还原后 `passed`。
  - `only_foreign_targets_drop_own_topmost`：`sayall` 不撤顶、其余目标撤顶，`passed`。
  - `cargo fmt --all -- --check`、`cargo check -p sayall-windows`、`cargo check -p sayall-windows-app` 通过；`cargo test -p sayall-windows` 499 passed / 0 failed / 16 ignored。
  - 真机：主窗口置顶位已清除，Z 序对照验证目标可见；**本地包安装后按一次 TV 键的端到端确认仍为 `deferred`**。
- 已知边界：成功判据仍是 `GetForegroundWindow()`，本次只消除了「自身置顶」这一种遮挡。若将来**第三方**窗口以 `WS_EX_TOPMOST` 压住目标，日志依旧会全绿而用户看不到；需要时再补 `GW_HWNDPREV` 链上的可见性判据，不在本次范围内。
- 隐私检查：日志与文档只含布尔值、窗口类名、Z 序序号和进程号，不含窗口标题、应用身份、个人路径、设备地址或语音内容。
