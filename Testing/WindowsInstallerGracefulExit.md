# Windows 安装器优雅退出验证（应用运行中执行安装/卸载）

日期：2026-09-16

## 验证目标

确认"应用正在运行"时执行安装或卸载**不会强杀应用**，而是：

1. 安装器先请求应用退出（会话内命名事件 `Local\SayAll-GracefulExit`）；
2. 应用收到请求后**自己**关闭 BLE 会话并等待 `ble_session_cleanup` 落盘；
3. 应用在安装器的强杀时刻之前自行退出，因此 Tauri 模板里的
   `CheckIfAppIsRunning` 自然落空。

背景：Tauri 的 NSIS 模板在 `Section Install` / `Section Uninstall` 中把钩子**之后**
紧跟 `CheckIfAppIsRunning`（`nsis_tauri_utils::KillProcessCurrentUser`，直接
`TerminateProcess`）。应用在持有活动 BLE GATT 会话时被强杀会留下未关闭的会话，使系统
蓝牙栈进入僵死态，此后所有 WinRT 入口返回 `0x80070008`，应用内全部自愈手段与睡眠都无效，
**只能重启电脑**。详见
[../Bugs/2026-09-16-ble-stack-resource-exhaustion-recovery-ineffective.md](../Bugs/2026-09-16-ble-stack-resource-exhaustion-recovery-ineffective.md)。

## 自动化步骤

```powershell
# 前置：先清掉本机已装版本（脚本要求干净起点，与其它安装矩阵脚本一致）
pnpm tauri build --bundles nsis
./scripts/test-windows-installer-graceful-exit.ps1
```

脚本覆盖两个场景，任一失败即抛错：

| 场景 | 步骤 | 判据 |
| --- | --- | --- |
| 安装覆盖运行中的应用 | `/S` 安装 → 启动应用 → 等 `reason=listening` → `/S /UPDATE` 覆盖安装 | 新增日志含 `reason=installer_requested_exit`、`ble_session_shutdown … terminal_result=passed`、`platform_shutdown … terminal_result=passed`；进程在 **8s 内**（安装器强杀时刻之前）自行消失 |
| 卸载运行中的应用 | 启动应用 → 等 `reason=listening` → 静默卸载 | 同上，走 `NSIS_HOOK_PREUNINSTALL` 路径 |

脚本自身的收尾**不再强杀**：先请求优雅退出，仍不退才强杀并打印告警。

手工复核（等价判据，安装过程中随时可看）：

```bash
LOG="%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log"
grep -aE "app_exit " "$LOG" | tail -5
# 期望顺序：reason=listening → reason=installer_requested_exit
#          → ble_session_shutdown phase=completed terminal_result=passed
#          → platform_shutdown phase=completed terminal_result=passed
```

## 现场结果

| 项目 | 结果 | 证据边界 |
| --- | --- | --- |
| 安装器钩子语法（`makensis` 汇编） | passed | 0.2.10 构建产出安装包，含本钩子 |
| NSIS `System::Call` 宽字符串 + 指针句柄在**运行时**有效 | passed | 独立探针（`Local\SayAll-Probe-GracefulExit`，对生产零影响）：PowerShell 侧事件被置位、`CloseHandle` 分支进入 |
| 安装器契约测试（事件名一致、两道钩子都请求退出、宽限 > 应用预算、钩子无强杀） | passed | `cargo test -p sayall-windows-app --lib`，5 项断言 |
| 退出收尾只执行一次（不产生误导性 failed 日志） | passed | `claim_exit_shutdown` 单测 |
| 应用侧命名事件（创建/等待/置位/无事件时报错） | passed | `cargo test -p sayall-windows --lib graceful_exit`，4 项 |
| 应用运行中执行安装 | deferred | 需真机（会改动机器安装状态），CI 步骤 `Test installer graceful exit while the app is running` 覆盖 |
| 应用运行中执行卸载 | deferred | 同上 |
| 真机"升级期间正在使用的遥控器语音链路" | deferred | 需 RC001/RC003 实机；本脚本的 CI 版本无蓝牙硬件，会话清理是空操作 |

## 边界

- CI 机器无蓝牙硬件，因此 CI 只断言**退出机制**，不断言"真实 GATT 会话在升级中被正确关闭"。
- 未覆盖可见安装界面、SmartScreen、Windows 10 1809 与代码签名。
- 应用侧宽限（`GRACEFUL_EXIT_TIMEOUT` = 5s）必须始终小于安装器宽限
  （`SETTLE 1500ms + TAIL 6500ms` = 8s）；契约测试守着这个不等式，改动任一侧都要重跑它。
- 事件名改动即破坏兼容：`crates/sayall-windows/src/graceful_exit.rs` 与
  `src-tauri/windows/installer-hooks.nsh` 必须逐字一致（已由契约测试守住）。
