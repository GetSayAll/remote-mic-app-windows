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

## 安装器强杀与"是否关闭应用"弹窗的实际行为

来源：本仓库构建产物（`target/release/nsis/x64/utils.nsh` + `SimpChinese.nsh`，构建时生成，
可用作 CLI 版本的 ground truth）。`CheckIfAppIsRunning` 的实际分支：

```nsis
nsis_tauri_utils::FindProcessCurrentUser "${executableName}"   ; $R0 = 0 表示有实例在跑
  IfSilent kill_...                        ; 静默安装（/S）：不提示，直接强杀
  ${IfThen} $PassiveMode != 1 ${|} MessageBox MB_OKCANCEL ... ${|}
    kill_...:   KillProcessCurrentUser + Sleep 500   ; 交互点"确定" → 强杀
    cancel_...: Abort $R1                            ; 交互点"取消" → 安装中止
```

弹窗文案：`{{product_name}} 正在运行！$\n点击确定以终止运行。`

由此得到三条结论：

1. **弹窗是 Tauri 模板的固定行为，不是本应用加的。** 它只在"实例在跑且本钩子没能
   让它退出"时出现。
2. **交互安装时用户本来就有一次安全选择**：点"取消"= `Abort`，什么都不装、不碰应用；
   只有点"确定"才会强杀。静默（`/S`）与被动模式没有这个选择，直接强杀。
3. 本钩子若成功让应用自行退出，后续检测落空，**连弹窗都不会出现**。

### 2026-09-16 更正：上面"0.2.10 起不会再有弹窗"的结论是错的

用户反馈"装新包时还是弹窗问我是否关闭无线麦"，回查发现**钩子从来没生效过**，
里面有三个叠加的 bug，每一个都足以让它静默失效。均已实测定位并修复：

| # | bug | 证据 | 后果 |
|---|-----|------|------|
| 1 | `FindProcessCurrentUser` 传的是**全路径** `"$INSTDIR\xxx.exe"` | 探针：裸名 → `0`（在跑），全路径 → `1`（不在跑）。见 `artifacts/nsis-probe/sayall-findproc-probe2-result.txt` | 永远判定"没有在跑"，整段等待逻辑被跳过 |
| 2 | 返回值判断**写反**：`${If} $R8 != 0` 才补等 | 探针：返回 `0` = 进程在跑，`1` = 不在跑。见 `sayall-findproc-probe-result.txt` | 进程已退出时白等 6.5s，进程还在（BLE 收尾要 5s）时反而不等 |
| 3 | `System::Call` 输出用了 `.r8` 却判断 `$R8` | 探针：`.R8` → `$R8`（成功 `916`，不存在 `0`）；`.r8` → `$8`。见 `sayall-probe3-result.txt` | `$R8` 恒为空，而空值 `!= 0` 在 NSIS 里为**真** → "事件存在"分支恒真，旧版检测从未触发 |

三者叠加的净效果：**无论运行的是新版本还是旧版本，钩子都会白等 6.5 秒，然后
必然落到 Tauri 的强杀弹窗。**这正是用户看到的现象，与"是不是旧版本"无关。

### 修复后的三条路径（均已在 2026-09-16 实测）

| 路径 | 行为 | 实测结果 |
|------|------|----------|
| 没有实例在跑（含应用内更新路径） | 零等待直接继续 | `passed`，耗时 1s（旧实现在这里也要白等 6.5s） |
| 有实例且能打开事件 | 置位 → **轮询**到进程消失（预算 20s，覆盖应用侧 5s BLE 收尾）→ 继续安装 | `passed`：替身进程打印 `signalled, exiting` 后自行退出，安装器未中止、未强杀，耗时 2s |
| 有实例但打不开事件（无监听的旧版） | **不杀也不装**：立即 `Abort` + 引导走应用内更新 | `passed`：退出码 1639，耗时 1s，进程**存活**（未被杀） |

第三条的取舍：旧版无法被请求退出，强杀会残留未关闭的 GATT 会话把系统蓝牙栈楔死
（只能重启 Windows），装下去又会被旧进程占住链路。因此选择中止并引导用户
在应用内点"检查更新"——旧版的 `on_before_exit` 会**先断开 BLE 再拉起安装器**
（`tauri-plugin-updater` 的 `install_inner`：`on_before_exit` 在 `ShellExecuteW`
之前执行，见插件源码 `updater.rs:843→862→876`），既不重启电脑，也不需要用户
手动退出应用。

### 现场记录（2026-09-16 09:05 左右，0.2.9 → 0.2.10）

**升级 `@tauri-apps/cli` 时必须复核**：tauri `dev` 分支已把该宏改为走 Restart Manager
（`RSTRTMGR::RmShutdown` + `RmForceShutdown`，交互取消同样是 `Abort`）。两种实现都不
请求应用自行退出，本钩子对两者都成立；但升级 CLI 后需重新查看生成的 `utils.nsh`，
并重跑契约测试与端到端测试。

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
| `FindProcessCurrentUser` 返回值语义（0=在跑 / 1=不在跑） | passed | 探针 `sayall-findproc-probe-result.txt` |
| 该函数**只按进程名匹配**，传全路径恒返回 1 | passed | 探针 `sayall-findproc-probe2-result.txt` |
| `System::Call` 输出寄存器大小写语义（`.R8`→`$R8`，`.r8`→`$8`） | passed | 探针 `sayall-probe3-result.txt` |
| 安装器钩子在**安装段与卸载段**都能汇编（标签解析） | passed | `artifacts/nsis-probe/sayall-hook-compile-test.nsi` 用 `makensis` 编译 |
| 三条路径的运行时行为（无进程 / 有监听 / 无监听） | passed | 见上表；替身 `crates/sayall-windows/examples/installer_exit_mock.rs` |
| 安装器契约测试（事件名一致、两道钩子都请求退出、宽限 > 应用预算、钩子无强杀、裸进程名、寄存器一致、轮询等待） | passed | `cargo test -p sayall-windows-app --lib`，6 项 |
| 退出收尾只执行一次（不产生误导性 failed 日志） | passed | `claim_exit_shutdown` 单测 |
| 应用侧命名事件（创建/等待/置位/无事件时报错） | passed | `cargo test -p sayall-windows --lib graceful_exit`，4 项 |
| 应用运行中执行安装 | deferred | 需真机（会改动机器安装状态），CI 步骤 `Test installer graceful exit while the app is running` 覆盖 |
| 应用运行中执行卸载 | deferred | 同上 |
| 真机"升级期间正在使用的遥控器语音链路" | deferred | 需 RC001/RC003 实机；本脚本的 CI 版本无蓝牙硬件，会话清理是空操作 |
| 从 0.2.9（无监听）升级到 0.2.10 的现场 | passed | 弹窗出现（旧实例不会响应）；该次日志无 `installer_requested_exit`；新实例 2 秒后落 `listening`、2.84s 连上遥控器，此后 3 小时零 `windows_resource_exhausted`——**这是运气，不是保证**，不能据此认为强杀可接受 |
| "有监听的新版本 → 更新版本"不再出现弹窗 | passed | 2026-09-16 用 `installer_exit_mock` 替身实测：应用自行退出，安装器未中止、未强杀 |

## 边界

- CI 机器无蓝牙硬件，因此 CI 只断言**退出机制**，不断言"真实 GATT 会话在升级中被正确关闭"。
- 未覆盖可见安装界面、SmartScreen、Windows 10 1809 与代码签名。
- 应用侧宽限（`GRACEFUL_EXIT_TIMEOUT` = 5s）必须始终小于安装器轮询预算
  （`SETTLE 1500ms + 轮询上限 20000ms`）；契约测试守着这个不等式，改动任一侧都要重跑它。
- **改 NSIS 钩子后必须真跑一次 `makensis`**：`System::Call` 的类型串、输出寄存器名、
  标签解析都是**运行时/汇编期**才检查的，编译通过不等于行为正确——本页记录的三个
  bug 全都是"看起来对、跑起来静默失效"。最小复现脚手架见
  `artifacts/nsis-probe/sayall-hook-compile-test.nsi`。
- 事件名改动即破坏兼容：`crates/sayall-windows/src/graceful_exit.rs` 与
  `src-tauri/windows/installer-hooks.nsh` 必须逐字一致（已由契约测试守住）。
