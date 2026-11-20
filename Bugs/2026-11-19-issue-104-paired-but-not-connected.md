# issue #104：配对记录在、GATT 链路不在（诊断与向导指引）

- 发现日期：2026-11-19（issue #104 最新 1.9.3 报告；同一 issue 自 2026-09-20 起有多次报告）
- 状态：诊断与向导指引已实现；真机复测 `deferred`
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
