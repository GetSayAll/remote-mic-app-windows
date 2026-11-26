# 按键宿主运行中断开后不再重连，门控永久失效

- 发现日期：2026-11-26
- 状态：已修，等待 RC003 真机/现场日志验证
- 影响范围：SayAll Windows 1.9.8（`source_revision=44c6a332…`）；Windows 11 26200；RC003；所有已映射按键与全按键支持
- 功能点：`key_host` 生命周期、`key_gate` 门控、`button_mapping` 注入判定

## 现象

遥控器重置并重新连接后，映射按键先正常工作；约 20 分钟后按键再次全部失效。页面可能仍显示“按键监听已就绪”，但日志里门控已经死亡：

```text
2026-11-26T05:32:23.083Z pid=14424 key_host action=start result=passed pid=8052 port=58794
2026-11-26T05:32:23.084Z pid=14424 key_host action=hook_report detail=HOOKkind=gateinstalled=1
2026-11-26T05:52:24.783Z pid=14424 key_host action=disconnected result=degraded reason=socket_closed
2026-11-26T05:52:25.xxxZ pid=14424 shortcut_capture action=diag phase=heartbeat ... gate_active=0 ...
2026-11-26T06:06:06.581Z pid=14424 map_skip_inject reason=gate_not_alive enabled=true gate_alive=false button=Down trigger=Single
```

从 05:52:24 到日志结束（06:08 之后）没有第二条 `key_host action=start`；`shortcut_capture` 心跳始终 `gate_active=0`。期间 RC003 helper 多次重连/设备节点恢复（`helper_authenticated`、`ownership_resumed`），但 key host 槽位没有被重新建立，所以恢复的只是 HID 报告捕获，不是按键门控。

该日志来自**修复前**的 1.9.8 构建（`source_revision=44c6a332…`），不包含 `7cca3af` 的启动重试，也不包含本次断连重连。

## 根因

已确认：

- `reader_loop()` 读到 EOF/错误后把 `ready=false`、调用 `report_host_disconnected()`，然后直接返回。
- 旧实现的 `KEY_HOST` 是 `OnceLock<KeyHostHandle>`：宿主一旦成功安装过，槽位就永远非空；`start_global()` 只会返回 `existing.is_ready()`（false），不会拉起第二代宿主。
- 因此一次 socket 关闭就把门控钉死在 `gate_active=0`，直到重启应用。

仍未知：

- 旧构建没有记录 socket 关闭瞬间宿主子进程是否已退出、退出码是多少。新增 `key_host action=child_exit phase=reader_eof pid=... state=running|exited_N` 后，下一次现场可直接区分“宿主崩溃/被杀”和“宿主仍在运行但传输断裂”。
- 本次断开与 RC003 agent 连接断开发生在同一毫秒附近（05:52:24.782/783），但两者是否为同一设备重置事件导致，当前日志不能证明；不能把 RC003 helper 的恢复当成 key host 恢复。

## 修复

- `crates/sayall-windows/src/key_host.rs`
  - `KEY_HOST` 从 `OnceLock` 改为 `RwLock<Option<Arc<KeyHostHandle>>>`，允许宿主代际替换。
  - `reader_loop()` 只在槽位仍指向自己时清除当前代、报告门控失能和安排重连；旧一代迟到的 EOF 不会覆盖新一代状态。
  - 断开时先记录 `key_host action=child_exit phase=reader_eof`，再回收旧子进程；随后记录 `key_host action=reconnect_scheduled reason=host_disconnected` 并走既有 0.5/1/2/4/8 秒退避。
  - 新宿主握手成功后重新安装 `gate/f5` 钩子，父进程 `HOOK kind=gate installed=1` 会重新把 `gate_active` 置 1。
  - 退出路径 `shutdown_global()` 取出槽位并清理，`SHUTDOWN_REQUESTED` 阻止重连；退出时迟到的 EOF 不再打印误导性的 `stale`。
- 手动探针 `crates/sayall-windows/examples/key_host_startup_probe.rs`
  - 新增 `SAYALL_KEY_HOST_PROBE_FORCE_DISCONNECT=1`：第一个宿主完成握手后 1 秒退出，模拟运行中断开；第二个宿主不应带同样行为。

## 验证

- 本机真实进程探针（`SAYALL_KEY_HOST_PROBE_FORCE_DISCONNECT=1`，`SAYALL_GATT_LOG` 指向临时日志）：
  - 输出 `started=true ready_after_retry=true gate=true`。
  - 日志实际序列：
    `key_host action=start result=passed pid=27464` →
    `key_host action=hook_report detail=HOOKkind=gateinstalled=1` →
    `key_host action=child_exit phase=reader_eof pid=27464 state=exited_2` →
    `key_host action=disconnected result=degraded reason=socket_closed` →
    `key_host action=reconnect_scheduled reason=host_disconnected` →
    `key_host action=retry_scheduled attempt=2 delay_ms=500` →
    新宿主 `key_host action=boot phase=started pid=37600` →
    `key_host action=start result=passed pid=37600 attempt=2` →
    `key_host action=hook_report detail=HOOKkind=gateinstalled=1`。
- `key_host::tests::startup_retry_schedule_is_bounded_and_backs_off` 继续覆盖退避上限。
- `cargo test -p sayall-windows --lib -- --test-threads=1`：`641 passed; 0 failed; 22 ignored`。
- `cargo check --workspace`：passed（仅既有 warning）。
- `cargo check -p sayall-windows-app --features runtime-simulation`：passed（仅既有 warning）。
- 安装包（`89c57ed7`）在本机安装后，强制结束 key host 子进程（pid 30644）：
  `child_exit phase=reader_eof state=exited_-1` →
  `disconnected result=degraded` →
  `reconnect_scheduled reason=host_disconnected` →
  `retry_scheduled attempt=2 delay_ms=500` →
  `start result=passed pid=34520 attempt=2` →
  `gateinstalled=1`；随后心跳 `gate_active=1`、`bumps_ok=2`。这是安装版端到端恢复证据，不只是探针。
- 真机：deferred。需要在受影响机器上看到 `disconnected` 后出现 `reconnect_scheduled`、`attempt=2`、`start result=passed`、`gateinstalled=1`，并确认下一次实体按键实际执行；本机探针不等同于 RC003 真机验收。

## 隐私检查

未包含蓝牙地址、HID 路径、设备身份、语音内容或用户目录；仅保留时间戳、事件名、状态与退出码。
