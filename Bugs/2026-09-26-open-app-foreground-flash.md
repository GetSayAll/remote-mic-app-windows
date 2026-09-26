# 打开应用偶发只闪任务栏、未切到前台

- 发现日期：2026-09-26
- 状态：等待遥控器真机验证
- 影响范围：Windows 10/11 桌面应用；预设应用、自定义 exe/lnk 与 AppsFolder 注册应用；RC001/RC003 行为验收待完成
- 功能点：按键映射 → 打开应用
- 现象：映射动作能找到或启动目标应用，但偶发只让任务栏图标闪烁，目标窗口没有成为前台窗口。
- 复现条件：SayAll 在后台线程处理遥控器映射，目标应用已运行且当前前台属于其他进程时触发“打开应用”。
- 正常预期：目标主窗口恢复/显示并成为前台；若 Windows 最终拒绝，应明确记录失败，不能把调用已提交误报为成功。
- 证据：`app_launcher.rs` 原实现调用 `SetForegroundWindow` 后不读回前台窗口，`activate_running` 无论 API 是否成功都把 `activated` 置为 true；日志中的 `map_launch result=ok` 因而不能证明用户可见结果。微软官方文档明确说明后台进程的前台请求可能被拒绝并改为闪烁任务栏。
- 根因：已确认成功判据错误；同时，原有 `AttachThreadInput` 只共享输入队列，微软文档不承诺它绕过 foreground lock，因此单一路径不能覆盖后台映射线程。
- 修复：区分 `NotFound | Activated | ForegroundDenied`；首次前置后以 `GetForegroundWindow` 所属进程读回验收，失败才提交成对 Alt DOWN/UP 包住一次重试；物理 Alt 已按住时跳过注入；ShellExecuteW/ShellExecuteExW 启动与注册应用激活也在同一前台授权窗口内提交。Alt UP 部分提交失败时额外补发释放，避免粘键。
- 验证：
  - 状态机回归测试覆盖“SetForegroundWindow 返回 true 但前台读回失败”并确认必须重试，`passed`。
  - 2026-09-26 Windows 独立锁持有进程探针：子进程成为前台并调用 `LockSetForegroundWindow(LOCK)`；父测试进程从后台激活已运行记事本。日志实际观察到首次 `set_foreground_ok=false target_is_foreground=false`，随后 `alt_unlock_submitted=true`，第二次 `set_foreground_ok=true target_is_foreground=true`，终态 `target_result=foreground_observed attempt_count=2`；测试 `foreground_lock_retry_reaches_observed_notepad` passed。
  - RC001/RC003 实体按键以及不同 AppsFolder 应用的现场回归为 `deferred`。
- 隐私检查：日志只记录布尔结果、尝试次数、目标类别和错误分类，不记录窗口标题、应用身份、路径或用户输入。
