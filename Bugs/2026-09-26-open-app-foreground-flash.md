# 打开应用偶发只闪任务栏、未切到前台

- 发现日期：2026-09-26
- 状态：等待遥控器真机验证
- 影响范围：Windows 10/11 桌面应用；预设应用、自定义 exe/lnk 与 AppsFolder 注册应用；RC001/RC003 行为验收待完成
- 功能点：按键映射 → 打开应用
- 现象：映射动作能找到或启动目标应用，但偶发只让任务栏图标闪烁，目标窗口没有成为前台窗口。
- 复现条件：SayAll 在后台线程处理遥控器映射，目标应用已运行且当前前台属于其他进程时触发“打开应用”。
- 正常预期：目标主窗口恢复/显示并成为前台；若 Windows 最终拒绝，应明确记录失败，不能把调用已提交误报为成功。
- 证据：`app_launcher.rs` 原实现调用 `SetForegroundWindow` 后不读回前台窗口，`activate_running` 无论 API 是否成功都把 `activated` 置为 true；日志中的 `map_launch result=ok` 因而不能证明用户可见结果。微软官方文档明确说明后台进程的前台请求可能被拒绝并改为闪烁任务栏。
- 根因：已确认三层身份/成功判据错误。普通 EXE 路径原来相信 `SetForegroundWindow` 返回值；AppsFolder 路径只确认 `ShellExecuteExW` 接受请求，日志明确落为 `target_result=unknown`，上层却仍记录 `map_launch result=ok`；首次修复又假定激活契约或 Shell 返回 PID 就拥有主窗口。实际既有多进程 MSIX/Electron 应用，也有 AppsFolder 传统桌面条目：后者可能由启动器/中转进程承接 Shell 请求，真实窗口属于同路径的另一进程。
- 修复：普通 EXE 区分 `NotFound | Activated | ForegroundDenied`；首次前置后以 `GetForegroundWindow` 所属进程读回验收，失败才提交成对 Alt DOWN/UP 包住一次重试。AppsFolder 统一解析公开身份并依次匹配窗口级 AUMID、进程级 AUMID、`System.Link.TargetParsingPath` 对应的完整可执行路径，Shell 返回 PID 只作最后兜底；商店应用优先使用 `IApplicationActivationManager::ActivateApplication`，传统桌面注册项保留 `ShellExecuteExW`。所有路径只有读回目标前台才成功。物理 Alt 已按住时跳过注入；Alt UP 部分提交失败时额外补发释放，避免粘键。
- 验证：
  - 状态机回归测试覆盖“SetForegroundWindow 返回 true 但前台读回失败”并确认必须重试，`passed`。
  - 2026-09-26 Windows 独立锁持有进程探针：子进程成为前台并调用 `LockSetForegroundWindow(LOCK)`；父测试进程从后台激活已运行记事本。日志实际观察到首次 `set_foreground_ok=false target_is_foreground=false`，随后 `alt_unlock_submitted=true`，第二次 `set_foreground_ok=true target_is_foreground=true`，终态 `target_result=foreground_observed attempt_count=2`；测试 `foreground_lock_retry_reaches_observed_notepad` passed。
  - 2026-09-26 当前菜单键配置的 ChatGPT AppsFolder 目标实机探针：首版仅按激活契约 PID 查找窗口为 `failed`；改为按精确 AUMID 关联多进程窗口后，生产函数在约 130ms 内读回目标前台，`configured_registered_app_reaches_observed_foreground` passed。
  - 2026-09-26 菜单键改绑传统 AppsFolder 桌面应用后的现场日志：`ActivateApplication` 按设计失败并回退 Shell，Shell PID 只有隐藏窗口，连续读回 `foreground_denied`。读取 AppsFolder 的 `System.Link.TargetParsingPath` 后按完整映像路径关联全部真实进程，普通实机探针 passed；独立进程持有 foreground lock 的最差场景连续 5/5 passed（每轮重新建锁，测试总耗时 0.96–1.10s）。ChatGPT AUMID 路径回归 passed。
  - RC001/RC003 实体按键以及不同 AppsFolder 应用的现场回归为 `deferred`。
- 隐私检查：日志只记录布尔结果、尝试次数、目标类别和错误分类，不记录窗口标题、应用身份、路径或用户输入。
