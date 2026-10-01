# 全按键支持停在「正在启动」：助手始终未连上桥（用户现场 2026-10-01）

- 发现日期：2026-10-01
- 状态：调查中（等待用户侧助手日志与计划任务状态）
- 影响范围：Windows 用户现场；应用为 0.3.0 本地测试包（`source_revision=df41737267756f130fe65a847fd23577be9d3982`，2026-09-28 构建）、RC003、全按键支持（增强捕获）开关；基础 BLE 语音与其它按键路径不受影响
- 功能点：按键页 → 全按键支持 → 计划任务 `SayAll RC003 Helper` → 主程序桥接（捕获链第 ② 段）
- 现象：开关开着、界面停在「全按键支持已开启，正在启动」，助手始终没有连上；用户已自行尝试重启应用、重启遥控器、重启 PC，均无效
- 复现条件：用户现场；本机未复现（`deferred`）
- 正常预期：开关开启（或应用启动自动触发）后助手在数秒内完成 `helper_authenticated`，界面转为「全按键支持已开启」

## 证据

用户提供 `%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`（覆盖 2026-09-28T16:50Z–2026-10-01T09:12Z；**日志时间戳为 UTC，本地时间为 +8**）。

- 主程序侧最后一次成功握手：**10-01 10:26:04（本地）**`rc003_bridge event=helper_authenticated helper_pid=23388 version=2`；10:50:09 桥接断开 `event=closed reason=read_error edges=24 released=0 dropped=0`。
- **10-01 16:54（本地）起**，用户反复开关与两次应用重启（16:55:16 / 16:59:52）：
  - 每轮 `rc003 feature=enhanced-capture action=enable phase=completed terminal_result=passed`（说明助手 exe 找得到、`schtasks /run` 命令成功）；
  - `action=auto_trigger` 共 6 轮触发 + 一次兜底：`auto_trigger_escalate phase=task_ended terminal_result=passed` → `retriggered passed` → `completed terminal_result=failed reason=helper_still_not_connected retryable=false`；
  - **期间没有任何一条 `rc003_bridge event=helper_authenticated`**。
- 主程序侧不是故障点（三条独立观察）：每次启动都有 `rc003_bridge phase=listening port=<随机端口> descriptor=…`；同期遥控器 BLE 与语音会话正常（`audio_session generation=4 action=finish terminal_result=passed submitted_samples=102720`）；其它按键映射链路有正常活动（`map_edges` / `map_fire` / `map_inject result=ok`）。
- **缺失证据**：助手侧日志 `%ProgramData%\SayAll\rc003-helper\helper-task.log`（`--follow-app` 默认日志）未采集；计划任务状态（`schtasks /query /tn "SayAll RC003 Helper" /v /fo LIST` 的「任务要运行的程序」「上次运行结果」）未采集。

## 根因

**未确认。** 已确认的边界：

- 故障段在「计划任务 → 提权助手 → 回连 127.0.0.1 桥」，不在主程序、不在 BLE、不在映射引擎。
- 与已知问题 [2026-09-29-helper-scheduled-task-loopback-refused.md](2026-09-29-helper-scheduled-task-loopback-refused.md) **同形状**：该文记录了计划任务助手的 loopback 连接被本机 WFP/TUN 改写到其它端口后拒绝（`WSAECONNREFUSED 10061`），而同一份助手手动提权运行立即 `[APP-BRIDGE] event=connected`。该问题的修复是「主路径改为固定命名管道 `SayAll.Rc003Bridge`」，提交 `3b2e769`，2026-10-01 02:26 经 PR #148 合入 main——**用户手上的 0.3.0（df41737）早于该修复**。
- 仍待排除的候选（可由助手日志一次区分）：① 助手进程根本没起来（任务被拒 / 目标路径失效）；② 助手启动即退出（`[STOP]`，退出码 7/8/9/11/13，或读到停用信号 `[STOP-SIGNAL]`）；③ 连接到达但被拒（协议版本或 token 不符）——**这条路径主程序侧当前不留任何日志**（`rc003_bridge.rs` 的 DENY 分支只累加 `denied_total`），这正是本次无法只凭主程序日志定位的原因。

## 修复

根因未确认，因此**不做实现改动**；按 AGENTS.md「日志不足先补日志」先落地可观测性改进
（2026-10-01，用户明确要求「补齐日志 + 助手日志与主程序放一起，不要单独的文件」）：

1. `crates/sayall-windows/src/rc003_bridge.rs`
   - 鉴权被拒新增 `rc003_bridge event=helper_denied reason=<version_mismatch|token_mismatch> helper_pid=<pid> denied_total=<n>`（纯函数 `deny_note`，含字段与隐私断言测试）——把「助手没来」与「助手来了被拒」分开；
   - 描述文件新增 `log=<主程序诊断日志路径>`（纯函数 `descriptor_body`，测试覆盖"未初始化不得凭空写路径"与"解析方透明"）。
2. `src-tauri/src/lib.rs`：兜底失败行追加桥接对账摘要 `bridge_phase/bridge_port/accepted_total/denied_total/replaced_total/malformed_total/helper_pid`（纯函数 `bridge_health_summary` + 测试）。
3. `hardware/RC003/helper/src/main.rs`：助手日志与主程序**写进同一个文件**——优先描述文件 `log=`，其次按约定回退 `<描述文件目录>\Logs\sayall-diagnostic.log`，都不可写才回退助手自己的 `helper-task.log`（回退记 `[LOG] event=helper_local_fallback`）；行首统一为 UTC ISO 8601 + `pid=… component=rc003-helper`，写前对个人路径脱敏；panic 与续约线程同步改到该落点。
4. `LOGGING.md`：新增「单一日志文件：主程序 + 提权助手」一节。

**尚未交付的候选**：含命名管道传输修复的新包（本地包 `artifacts/windows-preview/*0.5.0*`，`sourceCommit=949dace`，主程序与助手均含 `SayAll.Rc003Bridge`）。

## 验证

- `cargo test -p sayall-windows --lib rc003_bridge`：18 passed（含新增 `descriptor_body_publishes_optional_pipe_and_log_fields`、`deny_note_distinguishes_rejection_from_silence`）。
- `cargo test --manifest-path hardware/RC003/helper/Cargo.toml`：15 passed（含新增 `shared_log_tests` 五例：`log=` 优先、约定回退、脱敏、UTC 时间戳形态、共用落点优先 + 回退）。
- `sayall-helper.exe --selftest`：全部通过（免提权、无设备）。
- `cargo fmt --all -- --check`、`cargo fmt --manifest-path hardware/RC003/helper/Cargo.toml -- --check`：`passed`。
- **真机复测 `deferred`**——需要用户侧助手日志与计划任务状态；换包后需重跑：开关开启 → 助手连上 → 三键与其余按键映射生效。

## 隐私检查

本文不含个人路径（一律写 `%LOCALAPPDATA%` / `%ProgramData%`）、不含设备身份、蓝牙地址、语音内容或凭据；用户原始日志不提交仓库。
