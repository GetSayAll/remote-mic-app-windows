# 优雅退出信号的陈旧置位导致应用「启动即退出」

- 发现日期：2026-10-15
- 状态：已修复（自动化 passed）
- 影响范围：`GracefulExitSignal`（2026-09-16 引入）所有版本；触发需要「事件对象在无应用等待时被置位、且有第三方进程持有句柄（对象不销毁）」——**正常安装器路径不受影响**（NSIS 侧 `SetEvent` 后立即 `CloseHandle`，置位由等待中的应用消费后双方句柄关闭、对象随之销毁）
- 功能点：应用生命周期 / 部署收尾通道（`crates/sayall-windows/src/graceful_exit.rs`）
- 现象：应用启动约 1 秒后进入正常收尾并退出（用户视角「双击图标闪一下就没了」，无任何报错）
- 复现条件：
  1. 应用未在等待（无实例运行或即将退出）；
  2. 第三方进程 `OpenEventW` 打开 `Local\SayAll-GracefulExit`、`SetEvent` 置位、**不关句柄**（事件对象存活且保持置位）；
  3. 启动应用 → watcher 立即等到该陈旧置位 → 启动即退出。
- 正常预期：只有真正等待中的实例被唤醒退出；不存在等待实例时，置位不应影响后续启动
- 证据：诊断日志中 `pid=14140` / `pid=33756` 的 `app_exit ble_session_shutdown → platform_shutdown` 出现在启动后约 1 秒（同一时刻 `COMMON_PHRASES ACCESS ... enabled_dev_build` 等启动日志正常）；事件为**手动重置**（`CreateEventW(None, true, false, name)`）——置位后保持，直到显式复位或对象销毁（全部句柄关闭）
- 根因：`GracefulExitSignal::create_named` 创建/打开事件后**从不复位**。手动重置事件一经置位保持；只要还有任一持有者（如诊断/测试脚本不关句柄）对象不销毁——新实例创建时即处于置位态，`wait()` 立即返回、启动即退出。本次由一次"请求旧实例优雅退出"的诊断操作（P/Invoke 未关句柄）触发
- 修复：`create_named` 在 `CreateEventW` 成功后立即 `ResetEvent`（清除陈旧置位）。竞态说明：安装器若在复位**之前**置位，针对的是上一个实例（本实例尚未运行、无需退出），清除无碍；复位**之后**到达的信号正常唤醒等待（毫秒级窗口；极端时序下安装器退回其有界等待与强杀路径，行为退化但不误伤）
- 验证：`cargo test -p sayall-windows --lib -- graceful_exit` **5 passed**——新增回归 `a_stale_signalled_event_is_cleared_on_create`（修复前按预期失败：新实例立即等到陈旧信号、启动即退出）。**真机复验（2026-10-15 本机）passed**：先制造「残留置位 + 第三方持有句柄」场景（临时脚本 `CreateEventW/OpenEventW` + `SetEvent` 后不关句柄、持有 120 秒），再用桌面快捷方式启动——实例正常存活（15 秒后窗口在、`COMMON_PHRASES ACCESS … enabled_dev_build` 正常）；修复前同一场景必秒退（本记录 14140 / 33756 两实例实证）
- 隐私检查：未包含个人路径、设备身份、语音内容或凭据
