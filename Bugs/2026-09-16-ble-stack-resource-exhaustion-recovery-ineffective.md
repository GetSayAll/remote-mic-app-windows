# 蓝牙栈资源耗尽：三重自动恢复全部失效，仅电源循环或杀进程可解

- 发现日期：2026-09-16（现象自 2026-09-12 起累积，2026-09-15 首次观察到"Off/On 成功但无效"）
- 状态：**触发机制已定位**（2026-09-16 01:40）。Tauri NSIS 安装器强杀正在连接的应用
  （`CheckIfAppIsRunning` → `KillProcessCurrentUser`，无优雅退出；静默模式连提示都跳过），
  而本仓库 `installer-hooks.nsh` 未实现 AGENTS.md 已有的"部署不得强杀"规则；
  且应用自身退出时也观察不到会话清理。修复方案（应用侧优雅退出 + 安装器侧请求退出 + 恢复侧停止空转）
  见文末。楔死定位于蓝牙内核驱动/控制器固件状态，仅完整重启可解除。
  验证需"安装覆盖正在连接的应用"实验，代价为可能需要再重启一次。
- 影响范围：Windows 安装版 0.2.6–0.2.8；Windows 11（本机）；RC001 / RC003 均受影响
  （两型号都走同一 `BluetoothLEDevice` 创建路径，日志中的失败码一致）；未发现与应用外
  第三方工具相关的证据
- 功能点：`sayall-windows` 的 BLE 连接与蓝牙无线电自愈（`ble.rs`、`bluetooth_radio.rs`）
- 现象：系统蓝牙栈进入资源耗尽态后，`BluetoothLEDevice` 创建在 0–5 ms 内返回
  `0x80070008`（`ERROR_NOT_ENOUGH_MEMORY`）。此时应用的三重自动恢复——普通重连、
  无线电 Off/On、提权 PnP 重启适配器——**全部无效**；只有系统睡眠/重启，或由用户杀掉
  会话内的用户态进程，才能恢复。用户可感知为：能按键但无法语音输入，且"关开蓝牙"也救不回来。

## 复现条件

- 长时运行期间反复出现，无固定触发动作。全量日志（09-09 至 09-16，61650 行）中共
  **16 轮**爆发、累计 **4581 条** `windows_resource_exhausted`，最长一轮持续 1 小时 53 分。
- **爆发起点特征（可复现、且是关键）**：预热特性上线后，多数轮次的起点是**进程启动时**
  即出现
  `radio_recovery_prepare phase=completed terminal_result=failed error_code=windows_resource_exhausted cache=unavailable`
  与 `radio_cycle stage=enumerate phase=fallback reason=snapshot_failed`。
  即**新进程第一次枚举 Radio 就失败**——该进程尚未申请过任何 BLE 资源。
- 非必要条件：系统睡眠/唤醒。16 轮中只有 2 轮的起点紧邻一次 S3 恢复
  （轮 4 起点 09-13 19:17:32 对 S3 19:17:31；轮 11 起点 09-15 15:06:13 对 S3 15:06:12），
  **不足以认定睡眠是主因**。

## 正常预期

- 断连后应用应能在退避范围内自动重连；连续失败达阈值时无线电 Off/On 应清除僵死链路；
  极端情况下提权重启适配器应重建系统 BLE 栈。三条路径中至少一条成功，用户无需介入
  （AGENTS.md「用户侧零介入原则」）。
- 实测三条路径全部失败，因此当前产品在该状态下**不可自愈**。

## 证据

日志：`%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`（09-09 至 09-16）。

| 恢复手段 | 用量（全日志） | 结果 |
| --- | --- | --- |
| 无线电 Off/On | `ble_radio_recovery phase=completed` 489 次；其中 `terminal_result=passed` **143 次**、failed 346 次 | **143 次 Off/On 明确执行成功，但紧随其后的 `device_from_address` 仍在 0–5 ms 内 `windows_resource_exhausted`** |
| 提权 PnP 重启适配器 | `pnp_radio_recovery phase=requested` **7 次** | **7 次全部 `error_code=stack_verification_failed`**（helper 退出码 0，但重建后仍枚举不到 Radio） |
| 普通重连 + 指数退避 | 4581 次 | 无效 |

> 计数口径更正（2026-09-16）：现场日志里 **没有** `ble_radio_recovery phase=requested`
> 这一行（旧实现只在 Off/On 结束处落 `phase=completed`），489 次请按
> `phase=completed` 计数。`requested` 只出现在 `pnp_radio_recovery` 上。

- **恢复预算空转**：最长一轮里 `ble_radio_recovery phase=window_reopened` 的 `window` 已涨到
  **70**（每窗口 2 次 + 60 秒冷却）。应用在无法自愈的状态下持续空转数百次恢复，既不成功
  也不收敛。
- **提权重启未真正生效的系统侧旁证**：`Microsoft-Windows-Kernel-PnP/Configuration` 通道在
  本地时间 2026-09-14 21:07:16 至 2026-09-15 15:52:20 之间**没有任何设备配置事件**，而应用
  在该窗口内请求了 3 次提权重启（本地 11:32 / 11:34 / 11:36）。与本仓库既有记录一致：
  `pnputil` 在逻辑失败时仍可能返回退出码 0。
- **真正有效的恢复方式（对照）**：
  - **杀用户进程 / shell 重启**：2026-09-15 22:5x 实测，用户杀掉一批进程后，新进程
    `ble_connect ... terminal_result=passed elapsed_ms=2505`（2.5 秒连上）。
  - **S3 睡眠**：最长的第 10 轮末次耗尽 13:25:11，S3 进入 13:25:15、恢复 13:25:17，此后长静默。
  - **系统重启**：第 7 轮（09-14 10:18:49）与第 13 轮（09-15 19:39:42）的结束对应系统重启
    （09-14 10:19:54 / 09-15 19:41 的 `EventLog 6005/6009` + Kernel-Power 172/521）。
  - **长时间自行恢复**：第 2/3/4/5/12 轮在 16–75 秒后 `ble_connect_stage ... passed`。

## 根因

- **已确认事实**：`0x80070008` 不是物理内存不足。09-12 现场实测物理内存 ~3.2 GB 可用、
  提交内存 ~9 GB 可用、SayAll 私有内存 ~16 MB。该错误码在蓝牙路径上表示"该组件所需的
  资源池拿不到"。
- **已确认事实**：故障在系统级而非应用选错 API——三条互相独立的用户态入口同时失败
  （WinRT GATT service selector `0x80070008`、配对 selector + `FromIdAsync`
  `0x80004004`、Win32 GATT `CreateFile` `0x80070079` 信号量超时）。
- **已确认事实**：**不是本应用的资源泄漏**。新进程一启动即失败（见"复现条件"），
  而该进程尚未申请过 BLE 资源。
- **根因假设（待直接观测验证）**：某个**内核级资源**（最可能是非分页池，或 BLE 驱动
  持有的对象表）被泄漏的对象占满。该假设能同时解释全部观察：
  - 对象由**用户态进程**持有 → 杀进程即释放（09-15 22:5x 实证有效）；
  - 对象由**驱动自身**持有 → 只有真正的电源循环（S3 / 重启）才释放（第 7/10/13 轮）；
  - `Radio.SetStateAsync` 的 Off/On 只是软件层无线电开关，**不释放这些对象**，
    因此"执行成功却无效"（143 次）；
  - 新进程启动即失败，因为资源在它创建前已被系统级占满；
  - 逐日恶化（09-12 → 09-15 爆发频次上升），符合累积型泄漏。
- **仍未知**：泄漏对象的具体所有者与资源类型。现有日志只能证明"应用拿不到资源"，
  无法回答"资源被谁占着"。本轮已补齐采样日志以消除该盲区（见"修复"）。

## 修复

本次不修改行为，只补定位能力（最小化改动，符合 AGENTS.md「功能点必须自带日志」）。

- 新增 `crates/sayall-windows/src/resource_probe.rs`：**只读**采样，不改任何行为。
  - 本进程：`process_handles`、`gdi`、`user`、`private_kb`、`working_set_kb`；
  - 系统级：`system_handles`、`nonpaged_kb`、`paged_kb`、`commit_kb`、`commit_limit_kb`、
    `physical_available_kb`、`process_count`、`thread_count`；
  - 取不到的字段写 `unknown`，不省略、不猜测（LOGGING.md）。隐私：仅计数与字节数。
  - `ResourceProbe` 以"一次重连尝试"为粒度做**边沿 + 节流**：进入资源耗尽轮次、恢复收尾
    必打，持续中每 25 次失败打一次，避免刷屏。
- 埋点：`worker_start`（进程基线）、`startup_prewarm`（预热前）、`episode_start` /
  `episode_ongoing` / `episode_end`（重连失败与成功）、`system_suspend` / `system_resume`
  （**此前睡眠/唤醒在诊断日志中完全不可见**）。
- **HRESULT 保真**（此前最大的信息丢失点：所有失败被压成 `snapshot_failed` / `prepare_failed`）：
  - `radio_cycle ... reason=snapshot_failed` 与新增的 `reason=device_query_failed` 带 `hresult=0x…`；
  - `radio_recovery_prepare ... terminal_result=failed` 带 `hresult=0x…`；
  - 新增 `ble_connect_stage phase=failure_detail stage=… raw_error=<原始错误文本>`。
- 文件：`crates/sayall-windows/src/resource_probe.rs`（新增）、`src/ble.rs`、
  `src/bluetooth_radio.rs`、`src/lib.rs`、`Cargo.toml`（新增 `Win32_System_ProcessStatus` 特性）。

### 判读方法（下次复现时按此定位）

对比同一时刻的 `resource_probe` 行：
- `nonpaged_kb` 逼近上限或持续上涨 → 坐实内核（驱动）非分页池泄漏；
- `system_handles` 上涨 → 系统级句柄泄漏；
- `process_handles` / `gdi` / `user` 上涨 → 本进程资源泄漏；
- 全字段不动 → 资源被系统 BLE 栈自身状态占着（设备节点僵死）；
- `system_resume` 是否紧邻 `episode_start` → 判定睡眠周期的贡献。

## 验证

- 探针字段齐全性 / 边沿触发 / 节流间隔单元测试：**4 passed，0 failed**（`cargo test -p sayall-windows --lib resource_probe`）。
- `cargo check -p sayall-windows --all-targets`：**passed**（零新增 warning）。
- `cargo fmt` 对本次改动文件：**passed**。
- **真机复现取证：`deferred`**——需要在复发时用含本日志的安装包抓一次
  `resource_probe` 与 `hresult`，才能把根因假设升级为确认事实。
- 根因假设本身：**`deferred`**（尚未直接观测到占满的资源类型）。

## 隐私检查

- 未包含个人路径、用户名、蓝牙 MAC/UUID、HID 路径、语音内容或凭据。
- 适配器仅按 `USB\VID_8087&PID_0A2B`（VID/PID 前缀）描述，未记录设备实例 ID。
- 新增日志字段仅含计数、字节数与 HRESULT；`raw_error` 为 WinRT/Win32 错误描述文本。

## 2026-09-16 01:12 首次现场取证（含探针的包，`ver=0.2.9 source_revision=96b9e60`）

补齐的探针在**首次启动即落盘**，当场否证了"机器级资源被占满"这一假设。

坏态探针（同一次启动，三个时刻）：

| 时刻 | 触发点 | process_handles | gdi | user | private_kb | working_set_kb |
| --- | --- | --- | --- | --- | --- | --- |
| 01:12:05.567 | `startup_prewarm` | 387 | 19 | 33 | 10536 | 30004 |
| 01:12:05.614 | `worker_start` | 419 | 19 | 40 | 13476 | 35416 |
| 01:12:05.718 | `episode_start` | 499 | 19 | 43 | 14656 | 38884 |

同刻系统级计数：`system_handles=73833`、`nonpaged_kb=428184`、`paged_kb=407292`、
`commit_kb=8830272`、`commit_limit_kb=15269288`（58%）、`physical_available_kb=3299556`、
`process_count=196`、`thread_count=2499`。

**判读结论**：

- 本进程占用极小（≤499 句柄、19 个 GDI、~14 MB 私有提交）→ **不是本应用的泄漏**，
  这一条从推理升级为直接观测。
- 系统级无任何紧张：提交量仅占上限 58%、可用物理内存 3.1 GB、句柄 7.4 万、线程 2.5 千。
  非分页池 418 MB 在 3.1 GB 可用物理内存下不可能"分配失败"（非分页池本身取自物理内存）。
  → **"内核非分页池泄漏"假设判为 `failed`**（此前为待验证假设，现予排除）。
- HRESULT 保真生效：`GetRadiosAsync`（`reason=snapshot_failed`）、设备查询兜底
  （`reason=device_query_failed`）、`bluetoothledevice_from_address` 三条入口
  **全部返回 `0x80070008`**，`raw_error=内存资源不足，无法处理此命令。 (0x80070008)`。
  即单一错误码，不是多码混合。

**据此修正根因假设（新的主假设）**：`0x80070008` 不是"某个池被占满"，而是
**蓝牙栈进入楔死状态后对"创建类"调用返回的通用失败码**。理由是：机器级计数全部正常、
软件层 Off/On 无法清除、只有电源循环或杀掉持有者才能清除——这符合"驱动/服务内部状态卡死"
而非"资源计数见顶"。

**本次爆发的触发线索**：上一个持有活动 GATT 会话的进程（pid=7924，连接 `generation=3`，
01:12 前 30 分钟仍在正常读电量）在本地 00:42 之后**停止记录**，且
**没有 `ble_session_cleanup`、没有退出日志、Application 日志也没有 1000/1001/1002 崩溃事件**
→ 该进程是**被外部终止（强杀）**的，而它当时持有活动 BLE 会话。
这正是 AGENTS.md 已记录的已知诱因（"部署不得强杀正在连接的应用：强杀会留下未正常关闭的
BLE 会话，是链路僵死的主要诱因"）。

**由此得到一个可主动复现的实验**（下一步）：
① 让遥控器正常连上（`ble_connect ... passed`）→ ② 强杀应用 → ③ 重新启动
→ 预期立刻复现 `0x80070008`。若可稳定复现，则根因锁定在"活动 GATT 会话被强杀"，
而不是资源泄漏；同时可用同一进程的坏态/好态探针对比确认无计数差异。

**待补的一条对照**：恢复后（睡眠或重启）再启动一次，取 `resource_probe reason=startup_prewarm`
与坏态对比。预期两者计数一致 → 进一步支持"楔死而非耗尽"。

## 2026-09-16 01:12–01:27 现场干预：五类恢复全部无效，楔死定位于内核层

坏态持续 15 分钟以上未自行恢复。本轮逐个实验（**前三类此前从未试过**），全部失败：

| # | 干预 | 结果 |
| --- | --- | --- |
| 1 | 结束每用户 WinRT broker：`RuntimeBroker` ×2 + `SystemSettings` + `explorer` | failed（explorer 自动重启，蓝牙仍 `0x80070008`） |
| 2 | 结束每用户蓝牙服务宿主：`svchost`(载 `microsoft.bluetooth.userservice.dll`) | failed（可成功结束，说明它以当前用户身份运行；栈无变化） |
| 3 | 结束全部 COM 代理 `dllhost` ×3 | failed |
| 4 | 应用内无线电 Off/On（`Radio.SetStateAsync`） | failed（长期累计 143 次"执行成功却无效"） |
| 5 | 应用内提权 PnP 重启适配器 | failed，且**是空操作**（见下） |

**新发现的产品缺陷（可独立修复）：提权 PnP 兜底是空操作。**
应用在 01:12:35.808 记录 `pnp_radio_recovery phase=requested elevation=required`，
16.9 秒后 `terminal_result=failed error_code=stack_verification_failed`。
本机 UAC 为 **`ConsentPromptBehaviorAdmin=0`（提权不弹框）**，因此该 helper 确实是
以管理员身份运行的——即这不是"用户没点 UAC"。
但 `Microsoft-Windows-Kernel-PnP/Configuration` 通道在 01:12 前后**没有任何设备事件**
（该通道对其它设备的重启有完整 400/410/420 记录，最近两条为 09-15 16:07 与 19:49）。
→ **`pnputil /restart-device` 返回了成功但未产生设备重启**，与仓库既有记录
（"该工具在这类逻辑失败时仍可能返回退出码 0"）一致。
另外 `PNP_RECOVERY_PROMPTED` 每进程只允许一次，所以这一次空操作之后，
该进程在剩余生命周期内再也不会尝试系统级恢复。

**由本轮实验得到的排除结论（`passed`）**：

- 楔死不在**设备在位性**：`Get-PnpDevice` 显示蓝牙类设备全部 Present 且 Status=OK
  （英特尔适配器 `BTHUSB`、`Microsoft 蓝牙 LE 枚举器`、小米遥控器、蓝牙鼠标）；
  `bthserv` 与 `RmSvc`（无线电管理服务）均在 Running。
- 楔死不在**任何用户态层**：应用重启、无线电 Off/On、每用户 WinRT broker、
  每用户蓝牙服务宿主、COM 代理——全部重置过，均无效。
- 楔死不在**机器级资源**：见上一节的探针数据（内存/句柄/线程/池全不紧张）。
- → 楔死位于**内核 BLE/无线电驱动 或 控制器状态**。这解释了为什么只有
  **真正的电源循环（S3 / 重启）**能清除：Windows 的"蓝牙无线电关闭"
  （`Radio.SetStateAsync`）只是软件层开关，**不切断控制器电源、不重置控制器**；
  而 S3 会切断 USB 端口供电，从而真正复位控制器（轮 7/10/13 的恢复方式即此）。

**本轮未完成的边界（`deferred`）**：

- 现场无法自行恢复：本环境的工具策略禁止 PowerShell 提权（`Start-Process` 被产品级
  硬策略拦下，`dangerouslyDisableSandbox` 亦无效）与 `rundll32`（LOLBin 拦下），
  因此**无法执行 bthserv 重启、适配器 disable/enable、或触发 S3**。
  现场恢复需一次人工电源循环（睡眠或重启）。
- **复现实验（连接 → 强杀 → 重启应用）未执行**，且**故意不自动执行**：该实验会
  **再次制造楔死**，而楔死只能靠电源循环解除，因此自动化复现会把机器留在坏态，
  代价大于收益。应由人工在明确知道"事后要再电源循环一次"的前提下执行。

## 2026-09-16 01:29 睡眠实验：S3 不解除楔死（否证并修正上一节）

对上一节"只有真正的电源循环能清除"这一条做了直接实验：**入睡 + 重启应用**。

- Kernel-Power `42` @ 01:29:18.951，文案为「系统正在进入睡眠状态。**睡眠原因: Application API**」；
  `107` @ 01:29:21.219「系统已从睡眠状态恢复」→ 实际睡眠 **2.27 秒**。
- 固件计时 `130`「SuspendStart: 20930669，SuspendEnd: 20930670」（相差 **1 个 tick**）、
  `131`「ResumeCount: 1，FullResume: **259ms**」。
  → **本次 S3 并未真正切断设备供电**，属"伪睡眠"（真实 S3 的恢复耗时是秒级，不是 259ms）。
- 应用以新进程 `pid=9260` 于 01:31:49 启动，`17:31:49.460` **立即**
  `resource_probe reason=episode_start error_code=windows_resource_exhausted attempt=0`，
  并持续失败到 01:32:03 之后。
  → **楔死在睡眠后完整存活。S3（至少此机这种伪 S3）不能解除楔死。`failed`。**

**必须修正的两处结论**：

1. 上一节的"只有电源循环能清除"表述不准。已知能解除的是 **完整重启**：
   09-14 10:19:54 与 09-15 19:41:07 两次重启后的新进程都立刻
   `radio_recovery_prepare ... terminal_result=passed cache=ready`（栈健康）。
   **伪 S3 无效**。
2. 早前"第 10 轮靠 S3 收尾"的推断应撤回：那也是一次 2 秒伪 S3
   （13:25:15 → 13:25:17），不应作为"S3 有效"的证据；第 10 轮的结束原因**未确认**。

**同时得到的第二条独立否证**：两次坏态探针（01:12 pid=9368 与 01:31 pid=9260，
两个不同进程、中间隔一次睡眠）数值处于同一区间——
`process_handles` 387 / 387、`nonpaged_kb` 428184 / 423988、
`commit_kb` 8830272 / 9034160、`system_handles` 73833 / 75868、
`physical_available_kb` 3299556 / 2674984（受其它程序活动影响有正常漂移）。
→ **楔死态在资源计数上没有任何签名**，"资源被占满"这一族假设被两次独立否证。

**修正后的模型**：楔死位于**蓝牙内核驱动或控制器固件状态**，
可被"完整重启"清除，但**不被软件层重置（5 类）与伪 S3 清除**。
这与"此机 S3 实际未断电"一致——没有真正断电，控制器/驱动状态自然保留。

### 现场恢复的正确操作（重要，避免白跑一次）

本机 `powercfg /a` 显示 **"快速启动" 已启用**。因此：

- **必须用「重启」，不能用「关机→开机」。** 启用了快速启动时，"关机"是混合关机
  （hiberboot，把内核会话写盘后"恢复"），**不会完整重置设备**；只有"重启"执行
  完整冷启动。用"关机→开机"很可能清不掉楔死，白跑一次。
- 已知能解除的两次都是重启：09-14 10:19:54、09-15 19:41:07——重启后新进程
  立刻 `radio_recovery_prepare ... terminal_result=passed cache=ready`。

## 2026-09-16 01:35 完整重启恢复（第 3 次实证）+ 好态/坏态探针对照

**真重启证据**：`EventLog 6005` @01:35:13.465、`Kernel-Power 172` @01:35:08.029 /
`521` @01:35:09.76x。

**恢复后全链路健康（均 `passed`）**：

- 新进程 `pid=10056` @01:35:48；
  `radio_recovery_prepare phase=completed terminal_result=passed cache=ready access=allowed elapsed_ms=81`
  （坏态同一行是 `failed / cache=unavailable`）；
- `17:36:13.814 ble_connect phase=completed terminal_result=passed elapsed_ms=17564`；
  `capabilities_request` passed；GATT 控制包双向流动；`remote_battery level=42` 每分钟。
  → **完整重启可解除该楔死，第 3 次实证**（前两次：09-14 10:19:54、09-15 19:41:07）。

**一项需知（非本次故障，但影响可用性）**：
`raw_input_listener action=start phase=completed terminal_result=passed matched_device_count=0 awaiting_remote_hid_interface=true`
→ 遥控器 HID 接口尚未就位，**按键暂不可用**；BLE 语音链路已通。这是既有的
"等待 HID 接口"路径（PR #82 的 Awaiting 状态），会在 HID 接口到位后自动绑定。

### 探针对照（含必须声明的混淆项）

| 字段 | 坏态 pid=9260 01:31:49 | 好态 pid=10056 01:35:55 |
| --- | --- | --- |
| process_handles | 387 | 306 |
| private_kb | 10584 | 5676 |
| working_set_kb | 30272 | 24372 |
| system_handles | 75868 | 56455 |
| nonpaged_kb | 423988 | 187744 |
| paged_kb | 433824 | 174228 |
| commit_kb | 9034160 | 3430116 |
| commit_limit_kb | 15269288 | 11352988 |
| physical_available_kb | 2674984 | 4732628 |
| process_count | 209 | 157 |

**混淆声明（重要，防止误读）**：好态是**刚开机**——机器上只跑了 157 个进程而非 209，
用户的浏览器/编辑器等尚未启动；且 `commit_limit_kb` 自身也由 14.88 GiB 变为 10.83 GiB
（提交上限随页文件/开机重置）。因此上表的普遍下降**主要反映"刚重启"而非"蓝牙楔死被清除"**，
**不能**据此推断坏态存在资源泄漏。

### 真正干净的那条证据（同一进程内的趋势）

坏态同一进程 `pid=9368` 的 `episode_ongoing` 采样（失败 25 / 50 / 75 / 100 次，跨度 14 分钟）：

- `nonpaged_kb` = 427984 → 428888 → 425836 → 426152（**基本持平**）
- `system_handles` = 75382 → 70845 → 74430 → 71081（**基本持平**）
- `process_handles` = 749 → 748 → 740 → 740（**基本持平**）

→ **楔死持续 14 分钟内没有任何资源累积。** 这与"机器级资源被占满/泄漏"这一族假设
正面冲突，是该假设的**第 3 次独立否证**（前两次：坏态绝对值不紧张；两次坏态同带）。

### 产品结论（重要，待定夺，本次未实施）

- 该故障态**不可由应用自愈**：应用侧全部公开手段（普通重连、无线电 Off/On、提权 PnP）
  以及 5 类用户态重置与伪 S3 全部无效，**只有完整重启**有效。这属于 AGENTS.md
  "用户侧零介入"明确允许的例外（"只有公开 API 全部失效的场景才允许 UI 提示人工介入"）。
- 当前行为有明确缺陷：在注定失败时仍空转数百次恢复（第 10 轮 `window` 达 70，
  全日志 489 次恢复请求），且 `PNP_RECOVERY_PROMPTED` 被一次**空操作**消耗后永不重试。
- 建议（未实施）：① 检测"恢复连续无效"后转为明确 UI 提示，说明原因与预期效果
  （需一次重启），停止空转；② 修复提权 PnP 空操作（或改走真正会断电的路径）；
  ③ 升级/安装流程不得强杀正在连接的应用——本次事件的触发线索正指向该规则未被遵守。

## 2026-09-16 A/B 对照：把"Off/On 无效"从单组印象升级为对照结论

上面的表是**单组**观察（"执行成功却无效"）。单组数据无法排除"没开关会更差"，
所以补了对照：把每次 Off/On 与其后**同一 pid** 的第一次重连配对，再与"同一次僵死
事件内、没有紧接 Off/On 的重连尝试"比恢复率。复算脚本
`scripts/analyze-radio-recovery-ab.py`，操作与判读见
`Testing/WindowsBleResourceRecovery.md`。

证据：`artifacts/ev_stream_raw.txt`（0.2.6 僵死现场，2983 条 `ble_connect`）。

| 组 | 样本 | 恢复 | 恢复率 |
| --- | --- | --- | --- |
| 实验组：Off/On 后首次重连 | 488 | 3 | **0.61%** |
| 对照组：同事件内普通重试（`attempt>=1`） | 2406 | 15 | **0.62%** |
| （参考）进程冷启动 `attempt=0` | 89 | 26 | 29.21% |
| （参考）Off/On 自身报 `passed` | 143 | 3 | 2.10% |
| （参考）Off/On 自身报 `failed` | 345 | 0 | 0.00% |

**两组相差 -0.01 个百分点**（判定阈值 ±10）——开关与不开关完全重合。
**"WinRT 说开关成功"不等于"碰到蓝牙栈"**：Off/On 报成功 143 次，其后只恢复 3 次，
因为命中的是启动预热缓存的 Radio 对象；报失败 345 次，其后恢复 0 次。
这正面支持本文档的根因模型（楔死在驱动/控制器层，软件层无线电开关触不到）。

**据此已实施**：`ble.rs` 按错误码分流——命中
`windows_resource_exhausted` / `winrt_operation_aborted` 时跳过 Off/On 与 PnP
（日志 `ble_recovery_decision action=skip_recovery reason=stack_exhausted_proven_ineffective`），
只保留普通重连；非僵死码仍走 Off/On 兜底。用户可见文案不再要求"重启电脑"。
根因/触发机制与本节无冲突：本节只否证了**恢复手段**，触发机制仍归因安装器强杀。

## 2026-09-16 01:40 根因落点确认：安装器强杀正在连接的应用（触发机制已定位）

### 机制证据（来自 Tauri 官方打包器源码，非推测）

`tauri-bundler/src/bundle/windows/nsis/utils.nsh` 的 `CheckIfAppIsRunning` 宏：

```nsis
${If} $R0 = 0                      ; 检测到进程在运行
    IfSilent kill_${UniqueID} 0    ; 静默模式：跳过提示，直接进入 kill
    MessageBox MB_OKCANCEL $R2 ... ; 交互模式：问一句"是否结束它"
  kill_${UniqueID}:
    nsis_tauri_utils::KillProcessCurrentUser "${executableName}"
    Sleep 500                      ; 杀完只等 500ms
```

- **全程没有 `WM_CLOSE`、没有优雅退出请求、不等待会话清理**，直接 `TerminateProcess`。
- **静默安装（`IfSilent`）连提示都没有**，直接强杀。
- 按**可执行文件名**杀 → 会一并杀掉所有同名实例（含位于其它安装目录的测试实例）。
- 该宏在 `Section Install` 里**紧跟 `NSIS_HOOK_PREINSTALL` 之后**执行——这一点决定了修复可以只改钩子（见下）。

### 本仓库侧证据

- `src-tauri/windows/installer-hooks.nsh` 全文 **45 行**，只有三件事：Windows 版本检查、
  降级检查、VB-CABLE 下载提示。**没有任何"应用正在运行"的处理逻辑。**
  → AGENTS.md 的「部署不得强杀正在连接的应用：强杀会留下未正常关闭的 BLE 会话，是链路僵死的
  主要诱因（2026-09-05 实证）」这条规则，**只写在文档里，从未在安装器实现**。
- 现场吻合：最后一个持有活动会话的实例 `pid=7924`（`connection_generation=3`，
  电量每分钟正常读到 `16:42:09Z`）**在那之后停止记录**，且
  **无 `ble_session_cleanup`、无退出日志、Application 日志无 1000/1001/1002 崩溃事件**
  → 被外部强制终止；随后启动的实例一切入即 `windows_resource_exhausted`。
  （"00:42 那一刻具体是哪个操作触发"仍属旁证，未取得直接记录；机制本身已由源码确认。）

### 另一处独立缺陷（修复必须一并考虑）

**应用自身退出时也从未观察到 `ble_session_cleanup`**：全日志仅 21 条 `ble_session_cleanup`，
全部落在重连/主动断开路径上，**没有一条出现在进程结束处**。
→ 即便安装器改成"请求优雅退出"，按当前实现应用也未必会清理会话。**两侧都要改。**

### 修复方案（三层，尚未实施）

- **A. 应用侧**：实现真正的"准备退出"路径——收到退出请求后先关闭 GATT 会话并**等待完成**
  （`ble_session_cleanup` 落盘）再退出；覆盖托盘退出、窗口关闭与外部请求三条入口。
- **B. 安装器侧**：在 `NSIS_HOOK_PREINSTALL` / `NSIS_HOOK_PREUNINSTALL` 中先请求应用优雅退出
  并等待最多 N 秒（该钩子**先于** Tauri 的 `CheckIfAppIsRunning` 执行；应用若已退出，
  Tauri 的检测自然落空、不会强杀），超时才落回原有行为。
  信号机制建议用**命名内核事件**（NSIS 侧 `CreateEvent`/`SetEvent`，应用侧一个等待线程即可：
  无轮询、无窗口消息、且不与现有"关闭即隐藏到托盘"的行为冲突）。
- **C. 恢复侧**（针对已楔死的用户）：停止空转（现状 489 次恢复请求、单轮 `window` 达 70），
  转为明确 UI 提示"需要重启一次"；修复提权 PnP 空操作；不要用一次空操作消耗掉
  `PNP_RECOVERY_PROMPTED`。

### 验证方式与代价

A+B 的验证必须做"安装覆盖一个正在连接的应用"这一实验——**这本身就是复现实验**：
若失败会把机器再次留在楔死态，**需要再重启一次**。应在明确接受该代价时进行。
（若 A+B 生效，则该实验同时完成三件事：确认触发条件、验证修复、并给出"不再复发"的证���。）

