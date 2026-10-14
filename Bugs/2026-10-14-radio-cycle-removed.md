# 移除"自动关开蓝牙无线电"恢复：应用开启时整个蓝牙周期性中断

- 发现日期：2026-10-14（外部报障，版本 0.8.6）；本机日志复核同日完成
- 状态：**已移除（代码层）**；报障用户侧真机复测 `deferred`
- 影响范围：0.8.6 及更早所有含 `bluetooth_radio.rs` 的 Windows 版本；与遥控器
  型号无关（RC001 / RC003 都会在"连续重连失败"时触发）；Windows 10/11
- 功能点：BLE 重连失败恢复（`crates/sayall-windows/src/bluetooth_radio.rs` +
  `crates/sayall-windows/src/ble.rs` 的失败分支）
- 现象（报障原文）："开启 Windows 版本软件的时候，蓝牙设备会周期性地中断，
  大概 1 小时一次；关闭退出软件大概 5 分钟后，连接恢复稳定。" 报障人补充：
  断的是**整个蓝牙**（图标/适配器级别）
- 复现条件：已选择遥控器且它持续不可达（休眠 / 离线；错误码 `gatt_unreachable`、
  `windows_api_failed`、超时等**非僵死码**）→ 连续 5 次重连失败 → 触发
  `cycle_bluetooth_radio()`：**关闭整个蓝牙无线电、2 秒后重开**（期间全部蓝牙
  设备断开、图标消失）；每窗口 2 次、冷却 60 秒后可再次进入 → 可持续循环
- 正常预期：失败只按既有退避持续重连；应用不应执行会波及用户全部蓝牙设备的
  系统级动作，且该动作的收益必须有证据
- 证据（本机诊断日志，`%LOCALAPPDATA%\SayAll\Logs`）：

  | 时间（本地） | 触发前的失败形态 | 执行结果 | 事后恢复 |
  | --- | --- | --- | --- |
  | 2026-10-08 20:21 | `gatt_unreachable` ×5（每次超时 7.7s） | 关开成功（off 20ms + on 3.8s，共约 4 秒全局断） | 关开后首次重试**仍失败**；26 秒后设备 HID `device_arrived` 才恢复——与本次关开**无因果证据** |
  | 2026-10-13 10:44 | `device_from_address` 瞬间失败 ×4+1（`windows_api_failed`） | 关开成功（off 8ms + on 3.9s） | 关开后该步骤立即成功；但单样本无法与"时间自然恢复"区分 |

  - 两次均为 `window=1 cycle=1` 单发；本机 6 天仅 2 次，无"循环"样本；
  - 报障用户侧"周期性"描述无日志（`deferred`）；
  - 历史依据与对照：`ATTRIBUTION.md`「2026-09-16 A/B 对照」（僵死态下
    Off/On 与不开关统计上不可区分，0.61% vs 0.62%）、
    `Testing/WindowsBleResourceRecovery.md`。
- 根因（设计层）：该机制本用于"OS 侧链路僵死"的兜底，但**触发条件只排除了
  僵死码**——"遥控器不可达/休眠"这类最常见的**设备侧**失败也会走到"关开整个
  电脑蓝牙"。而关开电脑蓝牙叫不醒休眠的遥控器：对设备侧失败无效、对用户
  全局破坏（一次断开全部蓝牙设备）。
- 修复（2026-10-14，已实现）：
  1. **删除** `cycle_bluetooth_radio()`、`RadioRecoveryBudget`(状态机)、
     `recover_bluetooth_stack_with_pnp()`（PnP 提权重启）及配套缓存/预热
     （`prepare_bluetooth_radio_recovery`、`refresh_bluetooth_radio_cache`）、
     常量与单测；删除 `src-tauri` 启动期的预热调用。`bluetooth_radio.rs`
     仅保留僵死态错误码分类（`is_stack_exhausted`）。
  2. 失败分支改为与普通失败一致：只按退避持续重连；僵死态改记
     `ble_failure_class action=persistent_retry`（不再有 `ble_recovery_decision`
     与任何系统级动作）。
  3. 僵死态用户文案（"蓝牙链路暂时不可用，正在持续重试…"）改在
     `user_facing_connection_error` 内分流，行为与文案不回退。
  4. 同步更新：`AGENTS.md`（用户侧零介入/禁止强杀两节）、`TROUBLESHOOTING.md`、
     `ATTRIBUTION.md`、`Testing/WindowsBleResourceRecovery.md`。
- 验证：
  - `cargo fmt --all -- --check` passed；
  - `cargo test --workspace` passed（sayall-core 32、sayall-windows 331、
    sayall-windows-app 110，0 failed；共减少 6 个普通单测 + 2 个硬件
    `ignore` 用例，均为被删机制专属）；
  - `cargo check -p sayall-windows-app --features runtime-simulation` passed；
  - `cargo check --workspace` passed（仅存量警告，与本次改动无关）；
  - 报障用户侧真机复测（"开启软件后整蓝牙是否还会周期性断"）`deferred`：
    需下一版安装包在报障机器上按原场景复测；
  - 本机侧真机行为验证 `deferred`（需复现"遥控器持续不可达"场景后观察
    "关开蓝牙不再发生、只按退避重连"）。
- 隐私检查：本文不含个人路径、设备身份、蓝牙地址、语音内容或凭据。
