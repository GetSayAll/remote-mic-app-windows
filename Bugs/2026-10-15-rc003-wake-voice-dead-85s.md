# RC003 睡眠唤醒后语音键失效约 85 秒（issue #264）

- 现场：v1.0.0（`a4c1311`），Windows 10.0.26300 x64，RC003 + 全按键支持开启 + 豆包输入法
- 报障：<https://github.com/GetSayAll/remote-mic-app-windows/issues/264>
- 处理：2026-10-20，分支 `fix/rc003-wake-fast-recover`

## 现场时间线（UTC，报障人日志）

| 时刻 | 事件 |
| --- | --- |
| 09:33:45.831 | `resource_probe reason=system_suspend action=entering_sleep`（进入睡眠） |
| 09:33:54.308 | `resource_probe reason=system_resume`（唤醒） |
| 09:33:54.967 | `capture_inactive reason=agent_never_acked`（唤醒后 0.66 秒就判出失效） |
| 09:33:55.711 | `ownership_resumed`（一度拿回所有权） |
| 09:33:56.424 | 宿主内 agent `[BYE]`（宿主被重建）→ `ownership_timeout` |
| 09:34:00.576 | 按下：`path=report_layer synth_confirmed=true` → `voice_effect result=missing reason=no_frame` |
| 09:34:55.115 | `capture_recover` 才发起（60 秒阈值） |
| 09:35:10.242 | `capture_resumed`（睡眠 → 可用共 84.4 秒） |

## 根因（两条独立缺陷叠加）

### 1. 合成门禁只跟"配置回执"走，不跟"所有权"走

`crates/sayall-windows/src/rc003_bridge.rs`

- 门禁（`key_gate::VOICE_SYNTH_ACTIVE`，决定按下走报告层还是走 `SendInput` 注入）
  此前只在三处被清零：S 行变更、socket 收尾、HELLO。
- 宿主重建后 agent 退出，但**主程序不知道**：agent 的 `bye` 只落在助手侧
  （`hardware/RC003/helper/src/main.rs` 的 `"bye" => logger.kv("[BYE]", …)`，
  不外传给主程序），助手进程还活着 ⇒ app↔助手的 socket 不断 ⇒ 三个清零点都不触发。
- 结果：`synth_confirmed=true` 一直成立，按下全选报告层路径，而报告层已经没人改写。
  日志同时出现"配置回执全绿"与"连续三帧缺失"两种互相矛盾的判断。

### 2. 恢复阈值是固定的 60 秒，不区分"慢启动"与"断了"

`src-tauri/src/lib.rs`

- `CAPTURE_RECOVER_AFTER_MS = 60_000`：失效满 60 秒才 `/end` + `/run` 重来一次助手。
- 唤醒后检测很快（那一轮对账恰好落在唤醒后 0.66 秒），但"动作"仍要等满 60 秒；
  加上新助手重新解析宿主 → 注入 → 握手（现场实测约 14 秒），总时长 84 秒。
- 判据本身也没有事件驱动：`system_resume` 只落一条 `resource_probe` 日志，
  不唤醒常驻对账（`CAPTURE_RECONCILE_WAKE` 只由开关动作自增）。

## 修复

**提交 `fe71089`**：门禁 = 配置回执 ∧ 所有权租约（`synth_gate_active` 纯函数）

- `clear_ownership()` 一并落下门禁（所有权是"报告层还归我们"的唯一证据）；
- HELLO 清掉上一代连接的租约（快照不再对外谎报"已接管"）；
- `A` 行回执与 `O` 行所有权都按新判据重算，状态迁移各留一条
  `rc003_bridge event=voice_synth_gate active=… reason=…`；
- 所有权超时作废回执（`synth_confirmed=false`）⇒ 重发 S 行、由新 agent 重新回执；
  瞬时超时不会把门禁永久打死（复归路径有测试）。

**提交 `2493dd9`**：恢复阈值按"起来过没有"分叉 + 唤醒事件驱动

- `capture_recover_after_ms(ever_active, owned_usages)`：本次开启真的接管过、现在所有权
  空了 ⇒ **8 秒**动手；从来没起来过（设备没枚举完 / 助手正在注入握手）⇒ 仍等 60 秒。
  `voice_synth_no_effect`（所有权还在、改写没发生）保持长阈值。
- `crates/sayall-windows/src/wake_signal.rs`：电源回调收到 resume 即自增一代，
  常驻对账的睡眠比对代次后立刻醒来重查；失效期间观察周期 15s → 2s。
- 唤醒造成的重连**不重发** 20 秒宽限锚点（`capture_grace_survives_resume`）：
  否则"提前醒来"反而会踩进"桥在监听、助手没连上"的窗口，把 20 秒又塞回等待路径。
  其余掉线（助手崩溃、`/end`+`/run` 之后）照旧重发，免得把正在注入的新实例判成失效。
- 日志：`capture_recover … after_ms= ever_active=`、唤醒轮
  `auto_trigger phase=recheck reason=system_resume`。

预期：同场景 84 秒 → **约 22–25 秒**（唤醒即判 + 8 秒阈值 + 约 14 秒重新注入/握手）。
若唤醒后助手迟迟不重连（走到 Trigger 分支且未主动重启过），上限约 35 秒。

## 验证

- `cargo test -p sayall-windows`：365 passed | 11 ignored。新增
  `synth_gate_follows_ownership_not_config_ack`（TCP 四段：只有回执→false、
  回执+所有权→true、所有权超时→false、重新接管+回执→恢复）；**修复前第一段即失败**
  （旧实现"回执即置真"）。
- `cargo test -p sayall-windows-app`：128 passed。新增
  `capture_recover_waits_long_only_when_it_never_worked`（修复前在 60000 vs 8000 处失败）、
  `wake_reconnect_keeps_the_grace_clock_it_already_served`、
  `reconcile_wake_reason_prefers_user_command`、
  `interruptible_sleep_wakes_early_and_reports_why`（含 resume 路径）；
  电源回调用例断言唤醒代次 +1。
- `cargo fmt --all -- --check`、`cargo check -p sayall-windows-app --features runtime-simulation`。

## 未覆盖（deferred）

- **真机睡眠唤醒复测**：需 RC003 + 宿主被重建的现场（拔电/合盖 ≥40 分钟）。判据：
  `auto_trigger phase=recheck reason=system_resume` → `capture_recover after_ms=8000
  ever_active=true` → `capture_resumed recover_attempts=1`，总时长应显著小于 60 秒。
- **阴性对照**：宿主未被重建的正常唤醒必须 0 次 `capture_recover`（防"每次睡醒都重起
  一次助手"）。
- 门禁修复只让状态诚实；豆包用户在这次失效窗口内仍不可用（豆包按设计丢弃注入键），
  恢复速度只能靠第 2 条。
- 助手侧"agent 丢失后自己 re-resolve/re-inject"仍是后续候选（可把 14 秒再压到秒级，
  并消掉 `/end` 后端口仍被旧实例占用导致的 `listen_retry address_in_use`）。

**提交 `550d82b`**（附加观察甲）：开关被应用关掉时必须说明原因

- 后端新增 `CaptureRevoke`（六个稳定 token，`AtomicU8` 发布，与 `CaptureHealth` 同模式），
  四个回落分支各自发布原因；重新开启成功后清回 `none` 并留
  `action=revoke_notice terminal_result=cleared reason=re_enabled`。
- 前端 `captureRevoked` + `captureRevokedText` / `shouldShowCaptureRevoked`：只在
  **关闭态 + 确有原因**时显示「…因为系统授权需要重新确认，已经被关掉了；重新打开一次就能
  继续使用。」；落点与失败句、「还没有生效」句同一位，优先级 操作失败 > 被关掉 > 未生效。
- 文案决策入册：`docs/product-copy.md` §7（2026-10-15 条）。

**提交 `3774997`**（附加观察乙）：`docs/release-notes-1.0.0.md` 第 19 行与「已知边界」、
`docs/product-copy.md`、`DEVELOPMENT.md` 的授权口径统一为「升级/覆盖安装保留授权、不弹窗；
之后每次开启都会再弹一次」，并补一条"升级后开关变关闭是在确认授权"的说明。

**提交 `f869876`**（小项 3）：`/end` 之后按快照 `helper_pid` 等旧实例真正退出（上限 2 秒）
再 `/run`，消掉现场那 5 次 `listen_retry reason=address_in_use`；
日志 `action=capture_recover phase=previous_helper_wait terminal_result=passed|timeout`。

## 仍未覆盖

- **真机睡眠唤醒复测**（同上，需 RC003 + 宿主重建现场）与「宿主未重建 ⇒ 0 次 capture_recover」
  的阴性对照。
- **升级/覆盖安装后首屏目视**：开关被回落时那句说明是否出现、重新开启后是否收回。
- 助手侧"agent 丢失后自己 re-resolve / re-inject"仍是后续候选：可把 `/end`+`/run` 的
  ~14 秒再压到秒级（helper 已有 `[AGENT-REFRESH]` 与 `SERVED_HOST_PID` 跟随新宿主的雏形，
  但不在 agent bye 后自己重来）。须真机实验，不能凭推理上线。
- `复制诊断信息` 里暂未含 `capture_revoked=`（原因目前只在日志与桥快照里）。
