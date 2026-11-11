# 关机/重启时每秒弹出 schtasks.exe 0xc0000142 错误框

- 发现日期：2026-11-10
- 状态：代码已修复，自动化 passed；安装包与真实关机/重启验证 deferred
- 影响范围：无线麦 SayAll Windows 1.8.3 x64；按键页或连接页处于打开状态时；
  与是否开启「全按键支持」无关（只要界面在轮询任务状态，就会启动
  `schtasks.exe`）
- 功能点：按键页 / 连接页的全按键支持状态对账（`get_rc003_task_status`）
- 现象：关机或重启时连续弹出「schtasks.exe - Application Error /
  应用程序无法正常启动(0xc0000142)」，约每秒一个，单次关机 8–11 个，
  关机被拖慢；用户点击错误框的「确定」后下一轮仍会再弹。
- 复现条件：1.8.3 正在运行，当前页为按键页（默认页）或连接页；执行关机
  或重启。不需要连接遥控器，也不需要开启全按键支持。
- 正常预期：状态轮询不得启动子进程；关机时应用安静退出，不出现系统错误框。

## 证据

### 1. 旧版 1.8.3 的实际进程行为（本机，2026-11-10）

把应用切到按键页后，以 20 ms 间隔采样 `schtasks.exe`，连续 12 秒共观察到
12 个新进程，起止时间严格为每秒一个，父进程均为
`sayall-windows-app.exe`：

```text
21:05:53  21:05:54  21:05:55  21:05:56  21:05:57  21:05:58
21:05:59  21:06:00  21:06:01  21:06:02  21:06:03  21:06:04
```

采样期间应用未执行任何开关操作；这证明这是**常驻轮询**，不是退出清理。

### 2. 代码路径

- `src/pages/ButtonsPage.vue:2001`：`snapshotTimer = setInterval(..., 1_000)`，
  每轮调用 `reconcileRc003Task()`；后者在 `src/pages/ButtonsPage.vue:1705`
  调用 `getRc003TaskStatus()`。
- `src/pages/ConnectionPage.vue:1165`：同一秒级轮询，每轮调用
  `reconcileRc003Capture()`；后者在 `src/pages/ConnectionPage.vue:545` 调用
  `getRc003TaskStatus()`。
- `src-tauri/src/lib.rs:1487`：`get_rc003_task_status` →
  `rc003_task::status(enabled)`。
- `src-tauri/src/rc003_task.rs`（修复前）：`status()` → `task_installed()` →
  `schtasks /query /tn "SayAll RC003 Helper"`。

因此每秒一次是**前端轮询 + 后端每次轮询都启动 `schtasks.exe`** 的结果，
不是「应用收到关机通知后开始清理任务」。退出收尾函数
`shutdown_platform_for_exit` 没有调用任何 `rc003_task` 接口。

### 3. 关机时为什么是 0xc0000142

系统进入关机阶段后，新进程的初始化会被拒绝；`schtasks.exe` 无法完成
DLL 初始化时由系统弹出「应用程序无法正常启动(0xc0000142)」。
`0xc0000142` 是这条路径的通用失败码，不代表 `schtasks.exe` 文件损坏——
本机在正常状态下执行同一条 `/query` 命令退出码为 0。

旧版在收到失败后没有停止轮询：`status()` 只把结果用于 `installed` 字段，
前端仍按 1 秒间隔继续调用。应用退出前的收尾等待（BLE 会话清理上限 5 秒）
与系统等待叠加，正好能产生 8–11 个错误框。

## 根因

`rc003_task::status()` 被界面每秒调用，却在函数内部启动
`schtasks.exe /query`。系统关机时该子进程初始化失败，Windows 为每次失败
弹出一个模态错误框；前端轮询不停，错误框就每秒增加一个，阻塞关机。

## 修复（最小改动）

文件：`src-tauri/src/rc003_task.rs`

1. `status()` 不再调用 `task_installed()`，改为直接检查计划任务 XML 文件是否
   存在（`task_file_present()`）。这是一次文件元数据读取，不创建子进程。
   `installed` 只用于诊断，界面开关不读它。
2. `task_installed()` **保持 `schtasks /query` 语义**，只用于启动对账与
   一次性操作（任务文件存在但调度器不可用时仍应返回 false）；`status()`
   不再调用它。
3. 新增 `system_is_shutting_down()`（`GetSystemMetrics(SM_SHUTTINGDOWN)`），
   在 `run_schtasks()`、`schtasks()` 与 `task_install_elevated()` 启动子进程/
   提权流程前拦截：系统正在关机时直接返回错误 / `None`，不再给系统机会
   弹出错误框。
4. `task_xml()` 复用同一个任务文件路径，避免路径逻辑两处漂移。

## 验证

| 检查 | 结果 |
| --- | --- |
| 旧版 1.8.3 按键页 12 秒采样 | `passed`：12 个 `schtasks.exe`，每秒一个 |
| 修复后 `status()` 回归连续执行 100 次，同期采样 | `passed`：0 个 `schtasks.exe` |
| 新增回归 `status_reads_task_file_when_present` | `passed` |
| 新增回归 `status_reports_not_installed_when_task_file_is_missing` | `passed` |
| `cargo test -p sayall-windows-app` | `passed`：209 passed / 1 ignored |
| `cargo test --workspace` | `passed`：全部 crate 通过（app 209 / 1 ignored、windows 540 / 16 ignored、core 106 等） |
| `cargo fmt --all -- --check` | `passed` |
| `cargo check -p sayall-windows-app --features runtime-simulation` | `passed` |
| 新安装包 + 真实关机/重启 | `deferred`：需要出包并在一台真机上执行关机/重启，确认不再出现错误框 |

回归测试的设计：两条测试通过仅在 `cfg(test)` 下存在的线程局部
`TASK_FILE_OVERRIDE` 覆盖任务文件路径，分别指向存在和不存在的临时文件；
生产构建编译期就删除覆盖分支。旧实现走真实 `schtasks`，在「有真实任务但
没有临时文件」或「没有真实任务但有临时文件」的机器上必有一条失败；新实现
只读覆盖后的任务文件，两条都稳定通过。覆盖是线程局部的，不修改
`SystemRoot` 或任何进程级环境变量，并行测试不会互相污染。

## 隐私检查

本文不包含个人路径、蓝牙地址、HID 路径、语音内容或凭据；进程名与相对代码
位置可公开。用户提供的截图未纳入仓库。
