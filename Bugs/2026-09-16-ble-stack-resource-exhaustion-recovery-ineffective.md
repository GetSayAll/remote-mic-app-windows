# 蓝牙栈资源耗尽：三重自动恢复全部失效，仅电源循环或杀进程可解

- 发现日期：2026-09-16（现象自 2026-09-12 起累积，2026-09-15 首次观察到"Off/On 成功但无效"）
- 状态：调查中。2026-09-16 01:12 首次用含探针的包取证，**已排除"机器级资源被占满"**，
  主假设修正为"蓝牙栈楔死 + 活动 GATT 会话被强杀"（见文末追加节）；已具备可主动复现的实验路径。
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
| 无线电 Off/On | `ble_radio_recovery phase=requested` 489 次；其中 `terminal_result=passed` **143 次**、failed 346 次 | **143 次 Off/On 明确执行成功，但紧随其后的 `device_from_address` 仍在 0–5 ms 内 `windows_resource_exhausted`** |
| 提权 PnP 重启适配器 | `pnp_radio_recovery phase=requested` **7 次** | **7 次全部 `error_code=stack_verification_failed`**（helper 退出码 0，但重建后仍枚举不到 Radio） |
| 普通重连 + 指数退避 | 4581 次 | 无效 |

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

