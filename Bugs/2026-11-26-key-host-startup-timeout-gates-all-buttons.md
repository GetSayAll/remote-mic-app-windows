# 按键宿主启动超时后映射按键全部失效且错误红字不消失

- 发现日期：2026-11-26
- 状态：已修，等待 RC003 真机/现场日志验证；运行中断连另见 [2026-11-26-key-host-disconnect-no-reconnect.md](2026-11-26-key-host-disconnect-no-reconnect.md)
- 影响范围：SayAll Windows 1.9.8；Windows 11 26200；RC003；所有已映射按键与全按键支持
- 功能点：`key_host` 启动、`key_gate` 门控、`button_mapping` 注入判定、按键页错误提示

## 现象

用户按键页显示“按键监听已就绪”，但按电源键等已映射按键没有任何动作；页面底部同时显示红字“按键监听遇到问题，请到连接页重新连接遥控器后再试。”重新连接遥控器后红字仍在。日志中按键边沿已经到达映射引擎，但被门控拒绝注入：

```text
2026-11-26T05:02:20.284Z pid=22040 key_host action=start result=failed reason=accept_timeout
2026-11-26T05:02:25.435Z pid=22040 map_skip_inject reason=gate_not_alive enabled=true gate_alive=false button=Power trigger=Single
2026-11-26T05:02:25.xxxZ pid=22040 shortcut_capture action=diag phase=heartbeat gate_active=0 capture_active=0 keys_seen=0
```

从 05:02:25 到 05:14:35 的 `shortcut_capture` 心跳始终是 `gate_active=0`；期间没有第二条 `key_host action=start`。05:08:53 RC003 helper 已恢复（`enhanced_capture event=ownership_resumed`、`capture_unavailable ... cleared reason=ownership_acquired`），但门控仍为 0。

## 正常预期

1. 按键宿主子进程首次未在 5 秒内连回时，应用在后台重试启动；重试成功后门控恢复，映射按键可继续使用，不需要重启应用或重新配对。
2. 门控恢复后，由 `gate_not_alive` 写入的 `last_error` 被清除，页面不再显示已经不成立的错误红字。

## 根因

已确认：

- `KeyHostHandle::start()` 在 `accept_timeout` 后直接返回错误；`start_global()` 只记录失败，不重试。`KEY_HOST` 保持未初始化，`gate_active` 在整个应用生命周期内为 0。
- 同一旧实现的 `OnceLock` 也无法处理“宿主曾经启动成功、之后 socket 关闭”的情况；该独立路径见断连记录。
- `fire_gesture()` 在 `gate_alive=false` 时按设计跳过注入（避免双输入），并写入固定的 `last_error`。
- `ButtonMappingSnapshot` 只把 `last_error` 克隆给 UI，没有任何清除路径；因此门控恢复后红字仍然存在。
- 页面“已就绪”来自 Raw Input 监听器，与按键宿主/门控不是同一条链路，造成“已就绪 + 报错”同时出现。

仍未知：

- 旧构建没有记录宿主子进程在超时瞬间的状态，无法区分“子进程还没起来/被安全软件挂起”与“子进程已退出（参数、连接或握手失败）”。本次修复新增 `key_host action=child_state phase=accept_timeout state=running|exited_N` 与宿主侧 `key_host action=boot` 行；下一次现场可直接判定。

## 修复

- `crates/sayall-windows/src/key_host.rs`
  - 首次启动失败后由后台线程按 0.5/1/2/4/8 秒间隔重试，最多 6 次尝试；成功即安装并启动读循环，失败有界停止。
  - 启动路径串行化（`START_LOCK`），防止并发重试留下第二个宿主进程；退出时置位 `SHUTDOWN_REQUESTED`，避免退出过程中再拉起子进程。
  - `KEY_HOST` 槽位改为可替换的 `RwLock<Option<Arc<_>>>`：启动成功后的 socket 断开也会清空当前代并重新走重试，不再被 `OnceLock` 钉死。
  - 超时/握手/运行中断开时记录子进程状态与退出码；握手失败立即回收子进程。
  - 宿主进程启动时记录 `key_host action=boot phase=started pid=... port=...`，便于与父进程的 `child_state`、`child_exit` 对齐。
- `crates/sayall-windows/src/button_mapping.rs`
  - 把门控错误文案收敛为 `GATE_NOT_ALIVE_ERROR`。
  - `snapshot()` 发现门控已恢复且 `last_error` 正是该文案时清除，并记录 `map_error action=cleared reason=gate_recovered`；其他错误不因门控存活而被误清。

## 验证

- 新增 `key_host::tests::startup_retry_schedule_is_bounded_and_backs_off`：覆盖 0.5/1/2/4/8 秒退避与第 7 次不再重试。
- 新增 `button_mapping::tests::gate_recovery_clears_stale_gate_not_alive_error`：门控死亡时写入错误，模拟门控恢复后快照必须清除该错误。
- `cargo test -p sayall-windows --lib -- --test-threads=1`：`641 passed; 0 failed; 22 ignored`。
- `cargo check --workspace`：passed（仅既有 warning）。
- `cargo check -p sayall-windows-app --features runtime-simulation`：passed（仅既有 warning）。
- 本机真实进程探针（`crates/sayall-windows/examples/key_host_startup_probe.rs`，`SAYALL_GATT_LOG` 指向临时日志）：
  - 正常首启：输出 `started=true ready_after_retry=true gate=true`。
  - 设置 `SAYALL_KEY_HOST_PROBE_FORCE_RETRY=1` 强制第一个宿主子进程退出，模拟本次 `accept_timeout`：输出 `started=false ready_after_retry=true gate=true`；日志依次出现
    `key_host action=child_state phase=accept_timeout state=exited_2` →
    `key_host action=start result=failed reason=accept_timeout` →
    `key_host action=retry_scheduled attempt=2 delay_ms=500` →
    新宿主 `key_host action=boot phase=started` →
    `key_host action=start result=passed ... attempt=2` →
    `key_host action=hook_report detail=HOOKkind=gateinstalled=1`。
    这证明父进程在首次失败后实际拉起了第二代宿主并重新安装门控，而不是只验证了 schedule 纯函数。
  - 设置 `SAYALL_KEY_HOST_PROBE_FORCE_DISCONNECT=1` 强制宿主握手后 1 秒退出：日志出现 `child_exit phase=reader_eof state=exited_2` → `disconnected result=degraded` → `reconnect_scheduled` → `start result=passed attempt=2` → `gateinstalled=1`，输出 `started=true ready_after_retry=true gate=true`。
- 真机恢复路径：deferred。需要在受影响机器上看到同样的日志序列，并确认页面红字在 1 秒轮询内消失；本次探针不等同于 RC003 真机验收。
- 冷启动首用、连续会话、快速连按、断连/睡眠恢复仍需按 `Testing/Windows*.md` 在 RC001/RC003 分别验收；本次自动化结果不能替代真机 passed。

## 隐私检查

未包含蓝牙地址、HID 路径、设备身份、语音内容或用户目录；仅保留时间戳、事件名、状态与退出码。
