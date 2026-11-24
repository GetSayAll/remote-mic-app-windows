# BLE cached 服务假阳性：服务发现成功但 transmit 特征不可达

- 发现日期：2026-11-24
- 状态：已修复；用户安装 1.9.6 本地测试包（`abbc02f1`）后连接成功；
  语音链路仍待一次真实按住说话复测
- 影响范围：1.9.6（`7860681`）、Windows 10 19045 x64、RC003、issue #104
- 功能点：BLE ATVV 服务发现 / 连接
- 现象：Windows 显示遥控器已配对，连接循环持续重试；日志中
  `device_uuid_cached` 服务发现成功并返回 `service_count=1`，但紧接着
  `characteristic_transmit` 失败为 `gatt_unreachable`，连接始终不进入
  `Connected`。
- 复现条件：1.9.6 受影响的 RC003 现场；服务发现主路径
  `device_uuid_uncached` 返回 `gatt_unreachable`；缓存中仍有该设备的服务记录。
- 正常预期：只有能解析 transmit 特征的服务路径才算成功；不可用的 cached
  服务不能阻止后续 `device_all_services_uncached` / selector 路径。

## 证据

### 1.9.6 用户日志（修复前）

同一用户日志（1.9.6 `7860681`）中：

- 31 次连接尝试、0 次成功。
- `ble_service_discovery`：
  - 30 次 `device_uuid_cached` 通过，`service_count=1`；
  - 随后 30 次 `characteristic_transmit` 失败，
    `gatt_unreachable` / `gatt_status=1`；
  - 1 次主路径失败后链路短暂 disconnected，剩余路径按保护逻辑 skipped；
  - `device_all_services_uncached`、`device_all_services_cached`、
    `service_selector_from_id` 全部因 cached 先“成功”而 skipped。
- `ble_link phase=observed`：7 次 disconnected、10 次 connected；多次 cached
  服务返回后紧接着链路断开。
- `ble_device phase=access`：`access_status=allowed`、`is_paired=yes`；
  `is_present` / `is_connected` 在这台机器上为 `unknown`，不能据此判断设备离线。
- `raw_input` 仍有 `device_arrived matched_device_count=1` 98 次；该短窗口内
  `keys_seen=0`，用户没有按键，不能据此判断按键失效。

结论：cached 服务对象只是 Windows 本地缓存，返回成功不等于 ATT 可达；
原实现把它当成最终成功路径，导致真正可能可用的路径没有机会执行。

### 2. 修复后的用户现场

用户安装本地测试包 `abbc02f1` 后反馈连接成功。日志（540 行，单进程）：

- 7/7 次 `ble_service_discovery` 通过，全部选中
  `device_uuid_uncached`；
- 7/7 次 `characteristic_probe=passed`；
- 4 次 `ble_connect` passed；
- 另有 3 次在 `subscribe_audio` 返回瞬时 `windows_api_failed`
  （`AsyncStatus(3)`），随后重试成功；
- 无 `ble_service_path` 失败，无 `ble_connect_link` 失败段。

这次用户现场走的是正常主路径，没有触发 cached 假阳性；因此它证明新包能
正常连接，但不能单独证明 fallback 路径在实际故障态下被选中。修复前的
1.9.6 日志仍是 cached 假阳性的直接证据。

## 根因

`gatt_discovery::discover_atvv_service` 原先只根据服务发现结果选择路径：

1. 任一服务路径返回 `Ok(GattDeviceService)` 就立即返回；
2. cached 路径可以只返回本地缓存中的服务对象，不保证设备当前响应 ATT；
3. 后续 `find_characteristic(..., TRANSMIT_UUID, ...)` 使用 Uncached 查询时
   才暴露 `gatt_unreachable`；
4. 由于 cached 路径已被标记成功，全服务与 selector 路径不会执行。

这是应用侧的路径选择缺陷；Windows/遥控器为何让 cached 服务与当前 ATT 状态
不一致仍未完全确认，不把该底层原因写成已证实结论。

## 修复

- `crates/sayall-windows/src/ble/gatt_discovery.rs`
  - `discover_atvv_service` 增加 `validate_service` 回调；
  - 服务路径返回后必须先解析 transmit 特征，成功才标记路径 passed；
  - 特征探测失败时关闭该服务对象，标记路径 failed，并在链路仍 connected
    且非超时的情况下继续下一条路径；
  - 新增 `ble_service_path phase=service_ready`、
    `phase=characteristic_probe`，最终 `phase=completed` 带
    `characteristic_probe=passed|failed`；
  - 超时或链路 disconnected 时停止剩余路径，避免拖长重连。
- `crates/sayall-windows/src/ble.rs`
  - 调用方把 `find_characteristic(..., TRANSMIT_UUID, "transmit")` 作为
    validator 传入；
  - 直接使用返回的 transmit 特征，避免重复查询；
  - 音频/控制特征失败也记录具体 stage 与 `link_status`。
- `LOGGING.md`、issue #104 记录同步。

## 测试

新增纯逻辑与接线回归：

- `ble::gatt_discovery::tests::transmit_probe_failure_marks_a_service_path_failed`
- `ble::gatt_discovery::tests::transmit_probe_failure_continues_while_link_is_connected`
- `ble::gatt_discovery::tests::transmit_probe_timeout_stops_remaining_paths`
- `ble::gatt_discovery::tests::production_selection_requires_transmit_probe_before_marking_path_passed`
  - 防止以后删除 probe 调用、在 probe 前标记 passed，或在 `ble.rs` 绕过
    transmit validator。

真机手工测试（默认 ignored）：

- `real_remote_service_discovery_paths`
- `real_remote_service_and_transmit_paths`
- `real_remote_probes_each_service_path`

## 验证

- `cargo fmt --all -- --check`：`passed`
- `cargo test -p sayall-windows --lib`：`passed`（596 passed / 20 ignored）
- `cargo test --workspace`：`passed`（sayall-windows 596 / 20 ignored；
  sayall-windows-app 226 / 2 ignored）
- `cargo check --workspace`：`passed`
- `cargo check -p sayall-windows-app --features runtime-simulation`：`passed`
- `scripts/ci-preflight.ps1`：7/7 `passed`（`CARGO_BUILD_JOBS=2`；首次运行在
  Rust workspace 步骤因本机页面文件/链接内存不足出现 `LNK1102`，非代码失败）
- PR #411 `verify`：`passed`（11m46s）
- 本地测试包 `abbc02f1`：安装校验 PASS，内嵌修订号 `abbc02f1`，启动后连接
  正常；PR #411 评论记录了用户现场连接成功。

## 边界

- 用户复测只确认“连接成功”；`voice_sessions=0`，还没有一次真实按住说话的
  语音会话，语音链路标记 `deferred`。
- 用户成功日志走的是 `device_uuid_uncached`，没有实际触发 cached 假阳性后的
  fallback；fallback 的行为由修复前日志、单测和代码路径覆盖。
- 3 次 `subscribe_audio` 瞬时 `windows_api_failed` 后重试成功，暂不作为本
  缺陷的根因；若持续出现，另开记录。
- RC001 未复测，状态 `deferred`；本 Bug 的现场为 RC003。
- 不把本地测试包或 `.sig` 当作正式发布资产。

## 隐私检查

本文不含个人路径、蓝牙地址、设备 ID、语音内容或凭据；用户日志附件未入库。
