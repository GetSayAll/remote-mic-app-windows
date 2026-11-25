# RC001 升级 1.9.7 后短暂可用、随后链路长期不在（issue #104 第五现场）

- 发现日期：2026-11-25（用户日志 `Logs(3).zip`，UTC 2026-11-19T00:47Z–2026-11-25T06:51Z；
  本地时间 = UTC+8）
- 状态：调查中；应用侧未发现新的 GATT 路径缺陷；链路长期不在的原因仍未确认
- 影响范围：1.9.7（`98fd2a24`）、Windows 10.0.26300 x64、RC001
  （`ble_model ... model=Rc001`）、issue #104
- 功能点：BLE ATVV 连接与自动重连
- 现象：升级到 1.9.7 后约 5 分钟内连接可用；10:17 后链路不再建立，4.5 小时内
  380 次重连全部失败。连接页持续显示「正在连接遥控器」，设备行显示
  「操作没有完成，请稍后重试。」

## 证据

### 1. 1.9.7 时间线（本地时间，UTC+8）

| 时间 | 事件 |
| --- | --- |
| 10:11:51 | 1.9.7 进程启动；10:11:52 `ble_adapter ... low_energy=yes central_role=yes peripheral_role=yes classic=yes radio_state=on`；`ble_scan total_paired_count=1 supported_count=1 found_count=1` |
| 10:12:30–10:16:54 | 7 次可用连接：`ble_service_discovery` passed 且 `characteristic_probe=passed`；电量 82–83%；两次语音键按下分别采集 20,400 / 4,800 samples |
| 10:17:00、10:17:09 | 两次 `ble_link phase=changed ... status=BluetoothConnectionStatus(0)`；10:17:09 的 `ble_connect passed` 与断链几乎同刻（见「应用侧观察」） |
| 10:17:23 | 最后一次 `ble_link phase=observed`（connected → disconnected）；此后没有新的 connected |
| 10:19:24 | 一次仅 1.1 s 的 connected → disconnected |
| 10:29:56 | Windows `session_end`；10:31:30 新进程启动，`boot_type=full`、`fast_startup_enabled=true` |
| 10:53:29 | HID `device_arrived`；10 s 后 `device_missing`；10:53:35 链路在 5.4 s 后仍 disconnected |
| 14:51:05 | 日志结束；最后一次重连 `attempt=380`、`delay_ms=30000` |

### 2. 1.9.7 聚合（16,568 行）

- 467 次连接尝试：8 次 `ble_connect passed`（其中 10:17:09 一次是断链竞态；
  实际可用 7 次），459 次失败。
- 456 次 `ble_service_discovery` 失败；`ble_connect_link phase=failure` 共 459 次：
  454 次在 service_discovery 且 `link_status=disconnected`，2 次在 service_discovery
  超时且 `link_status=connected`，3 次在 `characteristic_control`/`characteristic_audio`
  阶段断开；458 条服务路径以 `gatt_unreachable` 失败（456 主路径 + 2 cached）；
  `gatt_access_denied` 0 次。
- 29 次 `ble_link phase=observed`：18 次 connected、11 次 disconnected；8 次
  `ble_link phase=changed` 全部是 `BluetoothConnectionStatus(0)`。
- 49 条 `raw_input` 日志：三次进程启动中两次 `binding_initial
  matched_device_count=0 phase=awaiting`（首次为 matched=1）；23 次
  `device_arrived`；`keys_seen=0`。HID 接口只在 10:15、10:17、10:53 等短窗口
  出现。
- 3 次 `ble_adapter phase=observed` 均为 `low_energy=yes central_role=yes
  peripheral_role=yes classic=yes radio_state=on`。
- 2 次 cached 服务路径只有在 `characteristic_probe=passed` 后才被接受，
  1.9.6 的 cached 假阳性修复在实际现场生效。
- 无 `system_suspend` / `system_resume`；无 `windows_api_failed`、资源耗尽或
  `ble_partial_cleanup` 失败。

### 3. 同一台机器的 1.9.3 历史

- 11-19 → 11-25：5,013 次 `device_from_address` 失败（`ble_connect_stage`
  原始错误为「操作成功完成。 (0x00000000)」，即 S_OK 但返回空设备对象）、
  335 次 `gatt_unreachable`、7 次超时、9 次成功。
- 11-21 08:04 → 11-23 01:46 的一个进程连续 5,002 次空设备对象失败、0 次成功，
  持续约 2 天。
- 11-24 06:27 一次成功（attempt 302）后链路稳定约 19.5 小时；11-25 09:58
  优雅退出时 `ble_session_shutdown` 5 s 超时（`shutdown_timeout`），随后重启
  的 1.9.3 在 10:05–10:10 又成功连接 4 次，直到升级。
- 1.9.3 也记录过 3 次 `ble_link phase=changed`（含 11-25 升级前 2 次），说明
  “偶发掉链后恢复”在这台机器上早已存在；1.9.7 的异常是 **10:17 后不再恢复**，
  不是第一次掉链。

## 根因

已确认：

- `FromBluetoothAddressAsync` 成功、配对记录在（`is_paired=yes`），失败发生在
  链路建立而不是 ATVV 服务/路径选择。
- 失败段每次都读到 `link_status=disconnected` + `gatt_unreachable` + HID 接口
  不在场；剩余四条路径按 `link_disconnected` 跳过是设计行为，不是路径选择缺陷。
- 1.9.6 的 cached 假阳性修复已验证生效（cached 路径只有 transmit 探测通过才被接受）。

未确认：

- 10:17 之后链路为何不再建立。候选包括遥控器休眠/没电/超出范围/被手机或电视
  占用、Windows 配对记录陈旧、遥控器固件或硬件故障、USB 蓝牙适配器或
  Windows 10.0.26300 BLE 栈状态。10:29 的完整重启（`boot_type=full`）没有恢复；
  10:17 时电量仍为 83%，不能用“没电”直接解释。
- 现有应用日志不能区分以上候选；需要用户侧 A/B 或 Windows 蓝牙事件/设备状态。

不支持：应用 GATT 路径回归、cached 服务假阳性、权限拒绝、资源耗尽、无线电
关闭、系统睡眠。

## 应用侧观察（非本次根因）

- 10:17:09.359 `ble_model` 已读到 `link_not_connected`，10:17:09.360 仍写
  `ble_connect passed`，10:17:09.362 才收到 `ble_link phase=changed status=0`。
  `BleSession::connect` 结束前再读一次 `ConnectionStatus` 可以避免这类“断链后
  仍记成功”的竞态；它不影响本次故障的判断。
- 连接页在连续失败后只显示「操作没有完成，请稍后重试。」；「遥控器还没有真正
  连上」的排查卡目前只在首次设置向导第②步
  （`src/onboarding/reconnect-guidance.ts`、`RemoteStep.vue`），连接页没有复用。
  这是用户可见的产品缺口。
- `ble_device phase=access ... is_enabled=no` 在成功连接期间也一直是 `no`，
  不能作为设备离线的判据。

## 诊断补齐（2026-11-25，本分支）

现有日志只能证明“链路不在”，不能回答“为什么不在”。本轮只补只读日志，不改变
重连/GATT/按键决策：

1. **空口广告探测**：连续失败第 5 次（约 60 秒）做第一次被动扫描，之后每 10 次
   失败一次；窗口 2.5 秒。日志 `ble_adv phase=requested|completed` 记录目标是否
   出现、是否可连接、广告类型、RSSI、扫描到的广告总数与 watcher 状态。
   - `seen=no total_advertisements=0`：适配器/扫描没收到任何广告；
   - `seen=no total_advertisements>0`：无线电工作，但目标没广播（休眠/没电/
     超出范围/被其它主机占用）；
   - `seen=yes connectable=no`：目标在广播但不可连接（通常已被另一台主机占用）；
   - `seen=yes connectable=yes`：目标可连接，失败在 Windows 链路/ATT/配对层。
   只比较调用方传入的地址，不记录地址、名称、厂商/服务数据。整个探测在独立
   短生命周期线程执行，外层 4 秒上限；超时落 `error_code=probe_timeout` 后
   继续原重连流程。
2. **Raw Input 枚举统计**：`raw_input scan phase=completed` 在监听器启动时记录
   一次，之后只在设备枚举签名变化时记录；字段 `total_devices` /
   `keyboard_devices` / `hid_devices` / `name_failures` / `remote_paths` /
   `matched_device_count` / `target_known` / `reason=...`。用于区分
   “系统里一个 HID 都没有”“HID 枚举正常但遥控器接口不在”
   “遥控器接口在但被目标地址过滤掉”。不记录路径、名称、VID/PID。
3. **断链上下文**：`ble_link phase=changed` 追加 `session_ms` 与 `phase`
   （`ready|streaming|draining|...`），用于判断掉链发生在连上几秒、语音结束
   之后还是长时间空闲后。

实现位置：

- `crates/sayall-windows/src/ble/advertisement_probe.rs`（新增）
- `crates/sayall-windows/src/ble.rs`（触发与 `session_ms`/`phase`）
- `crates/sayall-windows/src/disconnect_loop.rs`（`session_elapsed` 只读辅助）
- `crates/sayall-windows/src/raw_input_windows.rs`（枚举统计与边沿日志）
- `LOGGING.md`「BLE 空口探测与 Raw Input 枚举统计（2026-11-25）」

验证：

- `cargo fmt --all -- --check`：`passed`
- `cargo test -p sayall-windows --lib`：`passed`（606 passed / 20 ignored；
  新增 5 个广告探测、2 个触发/phase token、1 个 `session_elapsed`、3 个
  Raw Input 枚举统计用例）
- `cargo test --workspace`：`passed`（sayall-windows 606 / 20 ignored；
  sayall-windows-app 226 / 2 ignored；其余 crate 与文档测试 0 failed）
- `cargo check --workspace`：`passed`
- `cargo check -p sayall-windows-app --features runtime-simulation`：`passed`
- 本地测试包：`target/release/bundle/nsis/无线麦 SayAll_1.9.7_x64-setup.exe`
  （13,559,580 字节，SHA-256
  `738FD5278671DC898F956011FE4D23DB57186D2BF6A0AFBB38750CD057F4D54C`）；
  `scripts/verify-frontend-embedded.ps1` 18/18 `passed`；app exe 内嵌
  `source_revision=744e4966026030566770b858242e8da416a11b0e`。本地测试签名密钥
  与 CI updater pubkey 不匹配，属预期；该包与 `.sig` 不可发布。
- 安装/升级与真机复现：`deferred`（需要用户机器与遥控器；包未在本机安装覆盖）。
- 真机探测：`deferred`（需要用户机器与遥控器在失败态复现；本机无该现场）

下次报障只需一份 `sayall-diagnostic.log`，加一份
`scripts/collect-bluetooth-adapter.ps1` 的输出。先看 `ble_adv seen/connectable`、
`raw_input scan reason`、`ble_adapter bus_type/power_state`，再看脚本的
`adapter_count`/`selective_suspend`/`bluetooth_event`/`pnp_event` 计数，即可区分
空口、链路/ATT、适配器电源/驱动和系统电源事件，不再靠猜。

## 适配器侧诊断补齐（2026-11-25，本分支）

两份现场日志都来自「台式机 + USB 蓝牙适配器」环境。应用日志能证明链路不在，但看不到
适配器总线类型、最近电源状态、驱动版本、蓝牙/USB 事件和系统电源事件。样本只有两份，
不能把「台式机+适配器」直接认定为根因（选择偏差、USB 3 干扰、选择性挂起、旧驱动、
快速启动都可能是放大器），但需要把这些事实变成可回传的证据。

改动（只读，不改变重连/GATT/按键决策）：

1. `ble_adapter` 在 `trigger=startup` 和 `trigger=failure_streak`（第 5 次及之后
   每 10 次失败）追加 `attempt`、`bus_type`、`power_state`；`attempt` 与同次
   `ble_adv` 对齐。`bus_type` 来自 PnP 枚举器名（`usb|internal|bth_stack|sdio|unknown`）；
   `power_state` 来自 `DEVPKEY_Device_PowerData.PD_MostRecentPowerState`
   （`d0|d1|d2|d3|unknown`，最近一次状态，不是实时瞬态）。`BluetoothAdapter.DeviceId`
   是设备接口路径，应用只在本地转换成 PnP 实例 ID 后查询，不落盘。
2. 新增 `scripts/collect-bluetooth-adapter.ps1`：普通权限采集物理适配器
   （USB/PCI/ACPI/SDIO 枚举器）、`MSPower_DeviceEnable`、驱动版本、蓝牙服务状态、
   System 日志中的 BTH/Bluetooth 事件计数、Kernel-Power（睡眠/恢复/非正常关机）
   和 Kernel-PnP（411/219）计数。不输出设备名称、实例 ID、蓝牙地址、驱动路径、
   用户路径或事件原文。
3. 验证：
   - `cargo fmt --all -- --check`：`passed`
   - `cargo test -p sayall-windows --lib adapter`：`passed`（新增接口路径→实例 ID
     解析、bus/power token、备注格式用例）
   - `cargo test -p sayall-windows --lib real_adapter_pnp_facts_are_readable -- --ignored`：
     `passed`，本机 MediaTek USB 适配器读回 `bus_type=usb power_state=d2`
   - 脚本在本机普通权限运行：`passed`，`adapter_count=1 bus=usb status=OK
     service=BTHUSB driver_version=1.1044.0.556 selective_suspend=true`，System
     与电源事件均有计数输出；无设备身份信息。
   - `cargo test --workspace`：`passed`（sayall-windows 633 passed / 21 ignored；
     sayall-windows-app 233 passed / 2 ignored；其余 crate 与文档测试 0 failed）
   - `cargo check --workspace`：`passed`
   - `cargo check -p sayall-windows-app --features runtime-simulation`：`passed`
   - `scripts/ci-preflight.ps1`：`passed`（见本记录末尾）
   - RC001/RC003 真机复现：`deferred`（需要用户机器与遥控器在失败态）。

判读边界：

- `bus_type=usb` + 反复 `power_state=d2/d3` + `pnp_event`/`bluetooth_event` 计数上升，
  支持「USB 选择性挂起/重置」方向；但这些事件计数是**系统级**的，不是适配器专属，
  必须与 `sayall-diagnostic.log` 时间戳对齐。
- `bus_type=bth_stack` 只说明 WinRT 返回 BTH 栈节点，不能区分内置/外置。
- `selective_suspend=true` 是允许挂起，不是已经挂起；`wake_armed` 常为 `unknown`，
  不作为判据。
- 不覆盖 USB Hub 供电、2.4 GHz 干扰和厂商驱动内部状态；这些仍是 `deferred`。

## 下一步

1. 用户侧 A/B（按顺序，先做只读项）：
   - 按一次遥控器普通按键，观察连接页是否从「正在连接」变为已连接；这一步只
     验证 HID/链路是否到场。
   - 关闭附近手机/电视的蓝牙，或把遥控器从它们的已配对列表中删除。
   - 在 Windows 蓝牙设置里删除该遥控器；若仍无响应，长按「主页」+「菜单」
     重置遥控器，再按「TV」键约 3 秒重新配对；只在自动重试和完整重启都无效后做，
     不作为常规解法。
   - 换一个 USB 口/另一台电脑测试同一只遥控器；另一台正常则重点查本机适配器
     与 Windows BLE 栈，仍失败则重点查遥控器。
2. 应用侧（未改动，待产品决策）：
   - 把 `reconnect-guidance` 卡复用到连接页（连续 60 s 或 ≥5 次重连、且 HID
     仍 `awaiting` 时显示）。
   - `BleSession::connect` 结束前复查 `ConnectionStatus`，不在已断开时写
     `passed`。
3. 机器侧取证（下次报障）：运行 `scripts/collect-bluetooth-adapter.ps1`，并对照
   Windows `Microsoft-Windows-Bluetooth-BthLEEnum` Operational 事件；脚本已覆盖
   物理适配器、选择性挂起、驱动版本、蓝牙/PnP/电源事件计数。应用日志现在也会
   给出 `bus_type`/`power_state`。应用日志只能证明链路不在，不能区分上述候选。

## 验证

- 日志聚合：467 次尝试 / 8 次 passed / 459 次失败 / 457 次
  `link_status=disconnected` / 0 次 access denied / 0 次 suspend；统计命令与
  输出在本记录第 2 节。结论 `passed`（实际观察到）。
- 真机 A/B：`deferred`（需要用户机器与遥控器）。
- 代码改动：`5bb32c2ecea7fcf79a9a18dc65e04190b874f087`
  （`diag(ble): 补适配器 PnP 总线与电源诊断`）；只读诊断，不改变重连/GATT/按键。
- 本地测试包：`target/release/bundle/nsis/无线麦 SayAll_1.9.7_x64-setup.exe`
  （13,608,559 字节，SHA-256
  `5ac7a4211e2f0f3eae7a36e0bf3be717edd4d9d0d10ad0b0b393b78b66683179`）；
  裸 exe 内嵌 `source_revision=5bb32c2ecea7fcf79a9a18dc65e04190b874f087`，
  `verify-frontend-embedded.ps1` 18/18 `passed`。本地测试签名与 CI updater
  pubkey 不匹配，属预期；该包与 `.sig` 不可发布。
- 安装/升级/启动：`deferred`（未在本机覆盖安装；需用户机器验证）。
- RC001/RC003 真机复现：`deferred`（需要用户机器与遥控器在失败态）。

## 隐私检查

只使用时间戳、计数、稳定事件名、进程号与型号枚举；未包含设备名称、蓝牙地址、
设备 ID、HID 路径、用户目录或语音内容。用户日志附件未入库。
