# 蓝牙模式 MX Anywhere 3 在应用运行时卡住

- 发现日期：2026-10-13（反馈）；2026-10-14 确认反馈来自旧版。
- 状态：调查中；旧版自动关开蓝牙的修复已合入主线，本报告的新版真机复测 `deferred`。
- 影响范围：反馈机器的 Windows 版本、适配器型号、遥控器型号和安装包 SHA 未知；鼠标为 MX Anywhere 3，蓝牙模式出现卡住，USB 接收器模式未出现。
- 功能点：BLE 连接参数、遥控器重连与蓝牙鼠标共存。
- 现象：应用运行时蓝牙鼠标会卡住。退出应用后恢复；晃动并点击鼠标可使其从卡住状态恢复。

## 已知事实与边界

1. 反馈发生在移除自动关开蓝牙无线电之前的版本。旧实现可在遥控器连续重连失败后关闭整个蓝牙适配器至少 2 秒，影响同一适配器上的鼠标。该路径已由提交 `74aa3bb` 移除，见 [2026-10-14-radio-cycle-removed.md](2026-10-14-radio-cycle-removed.md)。**本报告没有故障时段日志，不能确认那次卡住是否恰好命中无线电切换。**
2. 主线仍在连接遥控器时申请 `ThroughputOptimized`，并持有到连接结束。微软文档说明该模式减少其他 BLE 外设可用的并发连接容量。它是新版若仍复现时的候选因素，尚未由鼠标 A/B 实验确认。
3. 2026-09-22 的仓库调查已发现：Windows 10 build 19041 上该 API 不可用。因此，历史 RC001 送达率变化不能归因于此请求。不能以旧记录为由直接改变请求时机，也不能以请求返回成功推断实际连接参数。
4. USB 接收器走独立接收路径。该对照把范围收窄到笔记本蓝牙路径，但不能区分无线电切换、连接调度和鼠标自身省电/重连。

## 本轮日志补强

- `ble_recovery_policy mode=retry_only radio_cycle=disabled`：运行中的新代码不会自动关开蓝牙。
- `conn_params phase=requested|completed|observed|monitor|monitor_closed|released`：区分请求、实际参数读回、变化监听与资源释放。Windows 11 上的预设构造失败保留 HRESULT，不误报为旧版系统；监听退订失败在设备尚未关闭时重试。`terminal_result=submitted` 只代表 API 接受请求；`observed` 记录遥控器链路的连接间隔、延迟和超时原始单位，**不代表鼠标链路状态**。
- `ble_link phase=changed`：记录遥控器连接状态和是否为旧代次回调。
- 所有新字段只含代次、枚举、数值、HRESULT；不含设备名称、地址或 HID 路径。

## 最小修复判据

1. 先在含 `74aa3bb` 的本地测试包上按原场景复测。若不再卡住，且日志显示 `radio_cycle=disabled`，将既有无线电恢复删除记为本症状的有效修复；仍需观察足够覆盖原故障间隔的时长。
2. 若新版仍卡住，记录本地时间、应用构建 SHA、鼠标连接模式和卡住持续时间；对照 `ble_link`、`conn_params` 以及 Windows 蓝牙适配器/鼠标事件。只有故障与实际参数变化相关，才做单变量 A/B：在测试包中取消 `ThroughputOptimized` 请求，保持其余连接行为不变。
3. A/B 必须同时检查 RC001 与 RC003 的冷态首用、连续语音和送达率。未证明语音成功率不降前，不把取消请求或在按下沿切换参数作为生产修复。

## 验证

- 自动化：`scripts/ci-preflight.ps1` passed（7/7）；其中 `sayall-windows` 库测试 352 个 passed、11 个 ignored，Rust 工作区测试、前端测试与构建、格式检查、Tauri 主程序和 runtime simulation 编译检查均 passed。日志记录语义的定向回归测试 passed（3 个）。
- 真实 Windows + MX Anywhere 3 蓝牙模式：`deferred`，反馈机器与故障时段日志当前不可用。
- RC001、RC003 语音与蓝牙鼠标共存：`deferred`，需按 [WindowsBluetoothMouseCoexistence.md](../Testing/WindowsBluetoothMouseCoexistence.md) 执行。
- 隐私检查：本文不含个人路径、蓝牙地址、HID 路径、语音内容或凭据。
