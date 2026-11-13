# 无限 Reconnecting 期间实体键盘 F5 被吞（issue #368）

- 发现日期：2026-11-13
- 状态：已实现；真机验证 `deferred`
- 影响范围：v1.8.0、v1.9.0、v1.9.1；登录自启且已配对遥控器不在位时；
  RC001 / RC003 需分别真机验证
- 功能点：语音键 F5 抑制器 / BLE 重连相位
- 现象：重启电脑后遥控器未唤醒/未连接，应用进入无限 `Reconnecting`；
  实体键盘 F5 被吞，浏览器不刷新、记事本不插入时间；退出应用后立即恢复
- 复现条件：已配对遥控器；登录自启或手动启动应用；遥控器不在位；
  等待 `ble_reconnect phase=scheduled` 与 `voice_f5_guard active=true`；
  按实体键盘 F5
- 正常预期：遥控器 HID 设备不在位时，实体 F5 原样交付；遥控器唤醒后，
  首条语音键 F5 仍被吞，不触发记事本时间戳或桌面刷新
- 证据：
  - issue #368 现场日志：`voice_f5_guard active=true` 自 03:43 起不再落回
    false；`ble_reconnect phase=scheduled attempt=7`；期间
    `suppressor_ask dir=down swallow=1`；退出应用后恢复
  - `voice_link_guard_active` 把 `Reconnecting` 算作保护相位：
    `crates/sayall-windows/src/ble.rs:1760`
  - 重连退避上限 30s、没有次数/时长上限：
    `crates/sayall-windows/src/ble.rs:68`、`crates/sayall-windows/src/reconnect.rs:27`
  - `decide()` 只看 VK，没有设备维度：
    `crates/sayall-windows/src/key_suppressor.rs:226`
- 根因：`Reconnecting` 是无限重试，不能证明遥控器仍在场；LL 钩子拿不到
  来源设备，`link_guard` 一旦为 true 就把实体 F5 与遥控器 F5 同等吞掉。
  既有 `observe_remote_voice_f5()` 不能作为“最近有遥控器活动”的信号：
  2026-09-05 对照实验证明被 LL 钩子吞掉的边沿不会投递到 Raw Input
  （`docs/investigations/2026-09-05-ll-swallow-vs-raw-input.md`），该回调
  只能在已经漏过一条之后触发，用它做活动门禁会重新引入 2026-09-10 的首沿
  泄漏。
- 修复：
  - `crates/sayall-windows/src/key_suppressor.rs` 新增纯判定
    `link_guard_effective(link_guard, remote_present)`：`link_guard` 必须叠加
    Raw Input 监听器的设备在场信号 `key_gate::listener_active()` 才生效。
  - `decide()` 显式接收 `remote_present` 并在内部组合 effective 值，三个决策
    入口（`swallow_ready`、`F5Up`、`F5Down`）都传入 Raw Input 设备在场信号；
    `latch_only` 同步使用 effective 值，避免日志把“相位在保护但设备不在位”
    误报成锁存。
  - `suppressor_ask` 与 `suppressor_stats` 新增 `remote_present=0|1`
    （`suppressor_ask` 在 60ms 有界等待结束后采样，反映实际裁决时的状态），
    下一次现场日志可直接区分“设备不在位放行”与“设备在位吞下”。
  - 相位集合保持不变（仍含 `Reconnecting`）；Raw Input 绑定到遥控器 HID 时
    `listener_active=true`，设备缺位（Awaiting）、移除或监听器停止时为 false。
- 验证：
  - `cargo test -p sayall-windows --lib key_suppressor`：8 passed / 0 failed
  - `cargo test -p sayall-windows --lib voice_f5_guard`：1 passed / 0 failed
  - `cargo fmt --all -- --check`：passed
  - `cargo test --workspace`：passed；`sayall-windows` 552 passed / 17 ignored，
    `sayall-windows-app` 218 passed / 2 ignored，`sayall-core` 106 + 集成 4，
    `sayall-mcp` 11 + 10，按钮/常用语各 1；全部 0 failed。新用例
    `key_suppressor::tests::link_guard_requires_the_remote_hid_interface_to_be_present`
    在本次运行中 `ok`。
  - `cargo check --workspace`：passed（仅既有 warning）
  - `cargo check -p sayall-windows-app --features runtime-simulation`：passed
    （仅既有 warning）
  - RC001 / RC003 真机：`deferred`。需要覆盖：遥控器不在位时等待
    `Reconnecting` 后按实体 F5（应 `swallow=0 remote_present=0`）；唤醒遥控器后
    首条语音键（应 `swallow=1 remote_present=1`）且记事本无时间戳；
    RC003 增强捕获开启/关闭各一次
- 边界：遥控器 HID 设备在位、但 GATT 长期重连失败时，实体 F5 仍会被挡；
  无驱动/报告层时 LL 钩子没有逐事件的来源设备信息，不能同时保证“遥控器首沿
  零泄漏”和“实体 F5 永不受影响”。RC003 增强捕获开启时语音键 `0x003E`
  已在报告层被清掉（#365），后续可在确认 clear 生效后不再启用 `link_guard`，
  属独立增量。
- 隐私检查：未包含个人路径、设备身份、语音内容或凭据。
