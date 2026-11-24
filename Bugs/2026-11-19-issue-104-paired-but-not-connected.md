# issue #104：配对记录在、GATT 链路不在（诊断与向导指引）

- 发现日期：2026-11-19（issue #104 最新 1.9.3 报告；同一 issue 自 2026-09-20 起有多次报告）
- 状态：诊断与向导指引已实现；2026-11-23 已扩展五条 GATT 发现路径的逐路径日志；
  受影响用户机器与 RC001/RC003 故障态复测 `deferred`
- 影响范围：1.9.3（`a4b5512`）、Windows 10.0.26200 x64、MediaTek 蓝牙适配器
  （USB ID 13D3:3602）；issue 内另有 1.8.0 / 1.8.3 / 0.8.6 / 0.3.0 报告
- 功能点：BLE `service_discovery` 诊断日志；首次设置向导②步长时间重连指引
- 现象：Windows 蓝牙设置/设备管理器显示遥控器已配对（用户描述为“已连接”），
  应用停在 `reconnecting`；日志反复出现
  `stage=service_discovery error_code=gatt_unreachable gatt_status=1`。
- 复现条件：已配对遥控器；应用启动或向导②步；遥控器 HID 接口不在场
  （`raw_input phase=awaiting matched=0`）；重连持续失败。
- 正常预期：遥控器唤醒后 HID 接口出现并触发立即重连，GATT 服务发现成功。

## 证据

- 2026-11-09 报告人的 1.8.3 诊断日志（用户附件，未入库）：
  - 16 分钟内 11 次尝试，绝大多数 `service_discovery` 返回
    `gatt_unreachable` / `gatt_status=1`，另有一次 10 秒超时；零成功。
  - `device_from_address` 12ms passed、`device_properties` 0ms passed，
    说明设备对象与名称读取都成功；失败固定在空口服务发现。
  - 同一诊断摘要：`Raw input: phase=awaiting matched=0`、
    `Buttons observed: 0/3`；日志中没有 `device_arrived`。
- 2026-11-19 报告人的 1.9.3 诊断摘要：`Remote: phase=reconnecting`、
  `reconnects=2`、`Buttons observed: 0/3`；其称完整日志已附，但 issue 评论中
  没有可见附件链接。
- 代码路径：`crates/sayall-windows/src/ble.rs` 的 `service_discovery` 使用
  `GetGattServicesForUuidWithCacheModeAsync(..., Uncached)`；
  `gatt_status=1` 为 `GattCommunicationStatus::Unreachable`。
- 既有同类记录：
  `Bugs/2026-11-01-ble-reconnect-cleanup-and-model-probe.md`（一个进程 9 小时
  零成功、HID 全程缺席）与 `Bugs/2026-09-07-ble-unreachable-both-remotes.md`
  （按键唤醒后恢复）。这些记录说明该状态不必然是应用重连逻辑缺陷。

## 根因

未确认。日志已把失败缩到对端/系统链路层：设备对象创建、名称读取都成功，
失败发生在空口服务发现；同时 HID 接口缺席。可能原因包括遥控器休眠/没电、
仍连接手机/电视、Windows 配对记录陈旧、适配器/系统栈问题或遥控器硬件故障。
不能仅凭现有日志认定应用连接逻辑有缺陷；也不把“重启电脑/重新配对”写成
常规解法。

## 修复

1. `crates/sayall-windows/src/ble.rs`：`service_discovery` 失败时、在
   `PendingBleConnection` 释放之前记录
   `ble_connect_link phase=failure stage=service_discovery ... link_status=connected|disconnected|unknown error_code=...`。
   不记录设备地址、设备 ID、设备名称或 `raw_error`；读不到状态写 `unknown`。
   这是下一次报障区分“Windows 认为链路在、但 GATT 失败”与“链路根本不在”的
   关键事实。
2. 首次设置向导②步：
   - 删除“最多等约 60 秒”的承诺，改为“不需要重新配对；超过约 60 秒仍未恢复时，
     下方会给出排查步骤”。
   - 连续重连满 60 秒（或 `reconnectAttempt >= 5`），且 `raw_input` 仍为
     `awaiting`、尚未收到按键时，显示「遥控器还没有真正连上」排查卡：
     按普通按键 → 关闭手机/电视蓝牙 → 删除设备后按「TV」键约 3 秒重新配对；
     动作按钮「打开蓝牙设置」「重新扫描」。
   - 纯判定在 `src/onboarding/reconnect-guidance.ts`；组件计时与渲染在
     `src/onboarding/steps/RemoteStep.vue`。

## 2026-11-23 五路径诊断（新增）

原日志只有一条 `BluetoothLEDevice` 级 Uncached 查询的结果，无法区分“服务真的
不可达”与“这条访问路径不可用”。现在 `crates/sayall-windows/src/ble/gatt_discovery.rs`
按固定顺序尝试五条路径，并逐条落结构化日志：

| order | path | 含义 |
| --- | --- | --- |
| 1 | `device_uuid_uncached` | 历史主路径：`BluetoothLEDevice` 级 UUID 查询，Uncached |
| 2 | `device_uuid_cached` | 同一查询的 Cached 形态 |
| 3 | `device_all_services_uncached` | 枚举全部服务后按 UUID 筛选，Uncached |
| 4 | `device_all_services_cached` | 枚举全部服务后按 UUID 筛选，Cached |
| 5 | `service_selector_from_id` | `GattDeviceService` 接口选择器 + `FromIdAsync`（共享访问候选） |

每次连接会落：

- `ble_device phase=created|properties`：设备对象创建后与读取名称后的链路状态、
  名称是否存在、名称推断型号；不记录名称正文。
- `ble_service_discovery phase=requested|completed`：路径总数、尝试数、失败数、
  `path_results=` 全量汇总（passed/failed/skipped 都出现）。
- `ble_service_path phase=requested|completed|skipped|failure_detail`：每条路径的
  `gatt_status`、`service_count`、`interface_count`、`match_reason`、`hresult`、
  `elapsed_ms` 与失败原始错误。
- 主路径成功时，其余四条落 `skipped reason=earlier_path_succeeded`；某条路径超时
  后，剩余路径落 `skipped reason=earlier_timeout`，不把重连继续拖长。

`FromIdAsync` 单独有状态检查：Windows 10 实测可能出现 **`S_OK`/HRESULT 0 但返回
空服务对象**；现在落 `match_reason=from_id_s_ok_null hresult=0x00000000`，不会再被
压成一句无法归因的“操作成功完成”。

### 本机真机探测（Windows 10 19045，已配对受支持遥控器）

- `real_remote_service_discovery_paths`：`passed`。主路径 130ms 成功；
  其余四条均按预期落 `skipped`，汇总行包含全部五条路径。
- `real_remote_probes_each_service_path`：`passed`（测试只断言主路径可用）。
  逐条结果：
  - `device_uuid_uncached`：`passed`，`gatt_status=0`，`service_count=1`
  - `device_uuid_cached`：`passed`，`gatt_status=0`，`service_count=1`
  - `device_all_services_uncached`：`passed`，`gatt_status=0`，`service_count=9`
  - `device_all_services_cached`：`passed`，`gatt_status=0`，`service_count=9`
  - `service_selector_from_id`：`failed`，`interface_count=1`，
    `match_reason=from_id_s_ok_null`，`hresult=0x00000000`
- 结论边界：selector 路径在本机 Windows 10 **不是可靠修复**；它现在只作为最后
  一条可观察候选。真正可能救回的是 Cached / 全服务枚举路径，必须等受影响用户
  日志确认哪条成功。不能把 selector 当已确认根因或已确认修复。

### 2026-11-23 第二份用户日志（链路已断开）

另一用户（Windows 11、MediaTek、小米蓝牙遥控器 2）的 41,148 行日志，覆盖
1.8.3 / 1.9.3 / 1.9.5：

- 1.8.3：39 次成功、634 次失败；1.9.3：21 次成功、279 次失败。
- 1.9.5：**0 次成功、68 次失败**；68 条 `ble_connect_link` **全部**
  `link_status=disconnected`。
- 1.9.5 的 `ble_scan` 每次都 `total_paired_count=3 supported_count=1
  found_count=1`：只证明配对记录在，不证明设备在当前无线电上。
- 1.9.5 的 `binding_initial` 全部是 `matched_device_count=0 phase=awaiting`；
  24 次 `device_arrived` 没有一次转成 `binding_audit action=present`。
- 因此这**不是**上一份日志的“Windows 认为 connected、GATT Unreachable”形态；
  它是链路确实断开、HID 接口不在场。Cached/全服务/选择器都无法凭空建立链路，
  继续叠加只会多等几个 7.7 秒。

已按这份日志加保护：主路径失败后如果 `link_status=disconnected`，剩余四条路径
直接落 `skipped reason=link_disconnected`，不再继续试。日志仍保留主路径的首次
尝试，因为它可能触发系统重新连接。

边界：1.9.5 段 0 成功不能单独证明是应用回归；同一遥控器在 1.8.3/1.9.3 有成功
记录，也可能是遥控器休眠/电量/被其它主机占用，或 Windows 11 蓝牙栈状态。需要
下一次用户复现时确认 HID 是否重新到场、按遥控器键是否能让链路恢复；不能把
“休眠/没电”直接写成结论。

### 2026-11-24 第三份用户日志（USB 蓝牙适配器，链路未建立）

同一 issue 的另一位用户（Windows 10.0.26300 x64，主板无蓝牙，使用 USB 蓝牙
适配器；1.9.5 `4ead96e`）追加了一份 862 行、只覆盖最近一次启动的日志：

- `ble_scan` 在删除设备后为 `total_paired_count=0`，重新配对后变为 `1`；
  配对记录恢复，但连接结果不变。
- 该段 `ble_connect_link` 共 26 条，**全部 `link_status=disconnected`**；
  77 条 `gatt_unreachable`、3 条 `operation_timed_out`，0 次成功。
- `device_from_address` / `device_properties` 均通过；失败固定在
  `service_discovery`，没有一次走到特征/订阅阶段。
- 全程无 `device_arrived`，`raw_input matched_device_count=0`，`keys_seen=0`；
  用户明确说明重连时按过普通按键，遥控器没有响应，也没有同时连接手机/电视。
- Windows 蓝牙设置里的状态在“已连接/已配对”之间跳；日志在失败瞬间读到的
  仍是 `disconnected`。资源探测正常，`ble_partial_cleanup result=ok`。

这份日志把范围进一步压到 **GATT 之前的链路建立**：不是“链路在、ATVV 服务
不可达”，而是 Windows 没有维持可用链路，HID 接口也不在场。因此五路径 fallback
不会被触发；不能把 selector/Cached 路径当作此现场的修复。应用侧暂不改连接
决策，下一步先补只读诊断（见下）并由用户做适配器/遥控器 A/B。

本次诊断补充（只读，不改变重连/GATT 决策）：

- `ble_adapter phase=observed ...`：进程启动后由独立短生命周期线程记录
  `BluetoothAdapter` 的 LE/中央/外围/Classic 能力和 Radio 状态。
- `ble_device phase=access ...`：每次尝试在服务发现前记录
  `access_status`、`is_paired`、`is_present`、`is_connected`、`is_enabled`；
  在场/已连接来自 AEP 属性，读不到写 `unknown`。
- `ble_link phase=observer ...` + `ble_link phase=observed ...`：在设备对象创建后
  立即注册只写 `gatt_note` 的 `ConnectionStatusChanged` 观察器，成功/失败路径
  都成对释放；用于确认 Windows 是否在失败尝试中短暂连上。
- 不记录适配器名称/地址、设备名称/ID、HID 路径或原始错误文本。

验证（2026-11-24）：

- `cargo fmt --all -- --check`：`passed`
- `cargo test -p sayall-windows --lib`：`passed`（591 passed / 19 ignored；
  新增 `adapter_capability_note_is_stable_and_identity_free`、
  `device_access_note_distinguishes_access_and_pairing`、
  `early_link_observer_records_transitions_without_identity`）
- `cargo test --workspace`：`passed`
- `cargo check --workspace`：`passed`
- `cargo check -p sayall-windows-app --features runtime-simulation`：`passed`
- `scripts/ci-preflight.ps1`：7/7 `passed`（首次运行出现一次已有的
  `button_mapping` 并行测试偶发失败，单测复跑与完整自检复跑均通过）
- 安装包/真实 RC001/RC003 上观察新增日志：待本次诊断构建的本地包验证

## 验证

- `cargo fmt --all -- --check`：`passed`
- `cargo test -p sayall-windows --lib`：`passed`（570 passed / 17 ignored /
  0 failed）；新增 `ble::tests::connect_failure_records_the_windows_link_state`
  `passed`
- `cargo check --workspace`：`passed`
- `cargo test --workspace`：`passed`（sayall-windows 570 passed / 17 ignored；
  sayall-windows-app 222 passed / 2 ignored；其余 crate 与文档测试 0 failed）
- `cargo check -p sayall-windows-app --features runtime-simulation`：`passed`
- `pnpm test`：`passed`（705 passed / 10 skipped）；新增纯逻辑 6 例、组件 3 例、
  向导集成 1 例
- `pnpm build`（`vue-tsc --noEmit && vite build`）：`passed`
- 真机复测 `deferred`：需要 RC001 / RC003 分别复现“配对在、链路不在”，确认
  `link_status` 与 HID 到场信号；1.9.3 报告人的完整日志仍缺失。

2026-11-23 多路径日志补充验证：

- `cargo fmt --all -- --check`：`passed`
- `cargo test -p sayall-windows --lib`：`passed`（588 passed / 19 ignored /
  0 failed；新增 `gatt_discovery` 纯逻辑 6 例、真机手工测试 2 例）
- `cargo test --workspace --no-fail-fast`：`passed`（sayall-windows 588 passed /
  19 ignored；sayall-windows-app 225 passed / 2 ignored；其余 crate 与文档测试
  0 failed）
- `cargo check --workspace`：`passed`
- `cargo check -p sayall-windows-app --features runtime-simulation`：`passed`
- 本机真机：`real_remote_service_discovery_paths` `passed`；
  `real_remote_probes_each_service_path` `passed`（五条路径中 4 条成功；
  selector 以 `from_id_s_ok_null` 失败，见上节）
- 受影响用户机器、RC001 与 RC003 故障态：`deferred`。下一次报障只需拉一份
  `sayall-diagnostic.log`，按 `ble_service_discovery ... path_results=` 即可看到
  哪条路径成功或失败；无需再让用户取电池或重配对。

## 本地测试包（2026-11-20）

- 包：`target/release/bundle/nsis/无线麦 SayAll_1.9.3_x64-setup.exe`
- SHA-256：`6E6F3E186A3512AF2701AAFE323C844E8D106128C28ABD6B1EAC93E1A54F253E`
- 本机缺 arm64 工具链，包为 x64-only；安装目录保留了上一版的 arm64 载荷，
  x64 测试不受影响。
- 前端嵌入校验：`scripts/verify-frontend-embedded.ps1` 对 app exe 18/18 passed；
  阴性对照 `sayall-mcp.exe` 按预期失败（18/18 缺失）。
- 静默升级安装：`installer_exit=0`；旧进程日志
  `reason=installer_requested_exit`，随后 `ble_session_shutdown` 与
  `platform_shutdown` 均 passed（未强杀）。
- 安装目录载荷：`sayall-helper.exe`、`frida-gadget.dll`、`sayall-mcp.exe`
  三件 SHA-256 与 staged 一致；安装版 app exe 与 `target/release` 差 3 字节
  （NSIS bundle 标记，良性），内嵌 `source_revision=e3cfb94...`、版本 1.9.3。
- 启动：安装版 pid 39708，`service_discovery` 299ms passed；PrintWindow 截图
  确认是应用界面（按键映射页），不是目录列表；未强杀，当前保持运行。
- 未做：向导卡片的安装版视觉复验（当前安装已完成引导，未改用户向导状态，
  由页面/组件测试覆盖）；RC001 / RC003 真机连接。

## 隐私检查

本文不含个人路径、蓝牙地址、设备 ID、语音内容或凭据；用户日志附件未入库。
