# 1.8.0 重连只在首次成功：断链清理等满 10 秒，正好在系统重连时关闭旧设备

- 发现日期：2026-11-01（用户反馈，附两份 `sayall-diagnostic.log`）
- 状态：**已修（应用侧）**；用户侧真机复测 `deferred`
- 影响范围：1.8.0 预览包（`f3f5212`），Windows 10 22H2 build 19045.4474；
  设备为系统名 `MI RC` 的小米遥控器（具体 RC001 / RC003 未能从日志区分）
- 功能点：BLE 断链重连（`crates/sayall-windows/src/ble.rs`）
- 现象：用户描述“只连接上一次；蓝牙在『已配对』和『已连接』之间来回转”。
  界面显示“连接出现问题，请稍后重试”，电量未知，语音功能一直停在确认中
- 复现条件：首次连接成功并使用过语音；遥控器空闲断链后，应用自动重连一直失败。
  用户日志覆盖 2026-10-28 至 2026-10-31 三个 1.8.0 进程
- 正常预期：断链后按 2/4/8/16/30 秒退避持续重连；遥控器醒来或 HID 接口出现时
  立即重连；清理不阻塞下一轮
- 证据：用户提供的两份诊断日志（未入库）

## 证据摘要

1.8.0 三个进程共 24,879 行；`ble_connect` 的最终结果：

| 进程 | 时间（UTC） | 成功 | `gatt_unreachable` | `operation_timed_out` | `windows_api_failed` |
| --- | --- | --- | --- | --- | --- |
| `11272` | 10-28 02:02–10:07 | 2 | 441 | 63 | 85 |
| `20352` | 10-29 01:19–10:21 | 0 | 813 | 50 | 0 |
| `22860` | 10-31 08:53–09:14 | 0 | 28 | 10 | 8 |

- 失败集中在 `service_discovery`；`device_from_address` 与 `device_properties`
  多数通过。说明设备对象和 Windows 缓存可用，失败发生在 GATT 服务发现/链路层。
- 两次成功：10-28 07:30:34 与 08:05:07。首次成功后，07:55:08 和 09:18:29
  各发生一次 `ble_link status=0` 断链；之后本进程再未成功（第二次断链后直到
  10:07 退出都没有再连上）。
- **关键时序（07:55 与 09:18 两轮相同）**：
  - `ble_link status=Disconnected` → `ble_session_cleanup phase=requested`
  - 两次 `async_wait label=gatt_disable_notify ... reason=timeout timeout_ms=5000`
  - 清理耗时约 10.3–10.8 秒
  - 清理刚完成，出现 `ble_link status=Connected stale=true`，紧接着
    `status=Disconnected stale=true`
  - 也就是说，系统/遥控器在旧会话清理期间已经重连，而应用在清理结束时
    关闭旧 `BluetoothLEDevice`，把这条刚恢复的链路一起断开
- 每次重连前还存在 `gatt_remote_model_service` 5 秒超时（三个进程分别
  56 / 56 / 11 次）。型号是纯展示信息，却排在 ATVV 服务发现之前。
- 三个进程均为 `boot_type=full fast_startup_enabled=true`，不是 2026-10-15
  记录的 hiberboot 断连循环。
- 日志中没有 `raw_input` / `device_arrived`，`matched_device_count=0`；
  遥控器 HID 接口在该时段没有出现。**这不证明应用能凭 GATT 重连把它唤醒**，
  详见下节边界。

## 根因

### 已确认（应用侧）

1. **断链清理做了注定失败的 CCCD 写入**：`BleSession::close` 在物理断开后仍对
   battery / audio / control 三个特征写 CCCD 关闭通知。物理断开后这些写入
   不可达，每次等满 5 秒；两到三个写入让清理从约 0.2 秒膨胀到 10 秒以上。
   这段时间正好覆盖系统/遥控器的自发重连窗口；清理收尾关闭设备对象时，
   新链路被一起拆掉（日志中的 `status=1 stale=true` → `status=0` 紧邻出现）。
   这是“只连上一次”的最强应用侧因果链。
2. **型号查询阻塞在 ATVV 服务发现之前**：`read_remote_model`（0x180A / 2A24）
   在断链时每次等满 5 秒，随后 `Cancel` 并立刻发起 ATVV 发现。它不参与语音，
   却把每次重连额外拖长 5 秒，并给系统 GATT 队列留下一个取消中的操作。
   代码注释早已写明“型号绝不能阻塞语音主路径”，实现没有遵守。
3. **服务发现超时短于系统自身的失败窗口**：系统对不可达 BLE 设备的 GATT
   连接尝试约 7.7 秒才放弃（2026-09-22 实测 7689/7697/7698 ms）；原
   `GATT_ASYNC_TIMEOUT=5s` 会在系统放弃前取消，紧接着重连更容易读到
   `Unreachable`。本次改为发现专用 10 秒上限，仍保留 5 秒用于 CCCD/写入。

### 仍未确认（设备/系统侧）

- 遥控器 HID 接口在失败时段完全没有出现，且 10-29 整个进程 9 小时零成功。
  这与 TODO 中已记录的“链路 Connected 而 Uncached 服务发现 Unreachable，
  重启应用/系统、关开无线电均无效，遥控器断电重连恢复”属同一类对端状态，
  不能仅凭本次日志认定应用可以自愈。
- 用户侧是否需要在失败后给遥控器断电/重新配对，需按下一版日志判定；
  本次不把“断电重连”写成常规解法。

## 修复

1. `BleSession::close`：仅在 `ConnectionStatus == Connected` 时写 CCCD
   关闭通知；断链时跳过并落
   `ble_session_cleanup phase=notify_disable_skipped reason=link_disconnected`。
   Handler 退订、service/device `Close` 仍照常执行。
2. `read_remote_model` 从 `service_discovery` 之前移到 ATVV 会话完整建立之后；
   仅当型号未知且链路 `Connected` 时 best-effort 读取。新增
   `ble_model phase=completed|skipped` 日志，失败保持 `Unknown`，不阻断语音。
3. `find_service` / `find_characteristic` 改用 `GATT_DISCOVERY_TIMEOUT=10s`，
   覆盖系统约 7.7 秒的失败窗口；`GATT_ASYNC_TIMEOUT=5s` 保留给 CCCD、
   写入和电量读取。
4. 新增纯判定单测：`should_read_model_number` 与
   `should_disable_notifications`。

修改文件：`crates/sayall-windows/src/ble.rs`。

## 验证

- `cargo fmt --all -- --check` **passed**
- `cargo test -p sayall-windows --lib` **passed**：
  492 passed / 16 ignored / 0 failed（含本次新增 2 例）
- `cargo check -p sayall-windows-app --features runtime-simulation` **passed**
  （仅存量 warning；worktree 需先放入 `src-tauri/sayall-helper.exe` 与
  `frida-gadget.dll` 资源，与本次改动无关）
- 真机复测 **deferred**：需用户侧 Windows 10 19045 + 同型号遥控器，
  走“首次连接 → 语音 → 空闲断链 → 自动重连”原场景，并观察：
  - `notify_disable_skipped` 是否在断链后立即出现，清理耗时是否降到 <1s；
  - 清理后 `ble_link` 是否不再出现“刚 Connected 就被 Close”；
  - `gatt_find_atvv_service` 超时是否减少；
  - `ble_model phase=skipped reason=link_not_connected` 是否替代 5 秒超时。
- RC001 / RC003 分别验收仍 `deferred`；不能用本次日志推断型号。

## 隐私检查

本文不含个人路径、蓝牙地址、设备 ID、设备真实名称（仅保留用户界面已显示的
通用名 `MI RC` 作为复现条件）、语音内容或凭据。日志附件未入库。
