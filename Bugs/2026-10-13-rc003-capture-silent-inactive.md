# RC003「全按键支持」静默失效 2.5 天：助手活着但 agent 没了，应用只观察不恢复

- 发现日期：2026-10-13
- 状态：已修复（自动化 passed；真机复现 + 自愈 passed 为 `deferred`）
- 影响范围：0.8.6（增强捕获自 0.8.1 起的同一条链路）；Windows 10/11 x64；RC003 增强捕获（「全按键支持」）——表现为开关显示已开启、增强按键与语音键都不走报告层；RC001 路径不涉及
- 功能点：RC003 增强捕获（提权助手 ↔ 设备宿主进程内的 agent）+ 应用侧常驻对账（`rc003_auto_trigger_reconcile`）
- 用户报障原话（2026-10-13，本机现场）：「网络出现了问题，导致整个 APP 无法使用，但没有任何提示；网络恢复以后，用户需要重新打开一次全按键支持」
- 复现条件：设备宿主进程被系统重建（host 换 PID），宿主内的 agent 随之消失；助手进程**仍然活着**（应用看到桥 `connected`）→ 对账停在「已连接」分支只做观察，永远不会重新解析宿主并注入

## 时间线（诊断日志，时间为 UTC，本机 +08；全部为只读证据）

1. 2026-10-11 02:20:51Z 助手 pid=35756 最后一条 `[RENEW] tick=5920 state=ok`
   （02:20:44Z 最后一条 `[HB] … lease_ok=true`）；此后该进程只剩
   `[WARN] 超过 5 秒没有 agent 心跳`，累计 **34497 条**，直到 2026-10-13 02:47:36Z 被结束。
   ⇒ 宿主里的 agent 从这一刻起再没回来过。
2. 2026-10-11 02:21:09Z 应用记下 `action=capture_inactive terminal_result=failed reason=agent_never_acked retryable=true hint=reconnect_remote`（含桥接对账字段）——
   **这是应用知道自己失效的唯一动作，之后 2.5 天没有任何恢复动作**。
3. 2026-10-13 02:43:45Z 窗口收进托盘；02:43:48Z 应用退出；02:43:53Z 应用重启（现场操作）。
4. 2026-10-13 02:43:54Z–02:45:28Z 蓝牙连不上遥控器：连续 10 次
   `ble_connect phase=completed terminal_result=failed error_code=windows_api_failed`
   （`stage=device_from_address`，raw_error =「蓝牙操作未成功完成」）→ 02:45:18Z
   `error_code=gatt_unreachable elapsed_ms=15410` → 02:45:28.849Z `terminal_result=passed`。
   合计 94 秒；同一时段无互联网（02:44:00Z `updater.check.fail … reason=network_or_status`）。
   ⇒ 用户在这个窗口里按什么都没反应，而主窗口在托盘、界面上没有任何提示。
5. 2026-10-13 02:44:29Z 应用（重启后）又一次记下 `capture_inactive … reason=agent_never_acked`
   （`helper_pid=35756`）——仍只有日志。
6. 2026-10-13 02:45:43Z 起用户按语音键：session 43–49 全部
   `path=send_input synth_confirmed=false`（报告层链路是死的，走回注入兜底）。
7. 2026-10-13 02:47:36Z 用户把「全按键支持」关掉再打开：新助手实例
   （`action=enable` → `elevated_install outcome=helper_exit exit_code=0`）重新解析宿主
   （`[HOST] pid=29120`）并注入 → 02:47:42Z `enhanced_capture event=ownership_resumed` →
   02:47:44Z `action=capture_resumed owned_usages=12` → 02:47:47Z 起
   `voice_attempt session=50 path=report_layer synth_confirmed=true` 恢复正常。
8. 宿主 PID 对照：2026-10-10 02:54:57Z 起多次 `[HOST] pid=14288`，2026-10-13 02:47:41Z
   `[HOST] pid=29120`——两次助手启动之间宿主已不是同一个进程，说明设备宿主被系统重建过，
   旧 tap 随宿主消失。

## 根因

- 对账的 `Connected` 分支只观察：`capture_inactive` 只落一条日志，**没有任何动作**
  （2026-10-08 的同类修复也只做到「让状态可分流的可见」，见
  [2026-10-08-rc003-stale-tap-self-heal.md](2026-10-08-rc003-stale-tap-self-heal.md)）。
- 助手进程存活时不会重新解析宿主：agent 消失后这条链路在**它手里**已经是死路，
  应用不换实例就永远等不到 `ownership`。
- 用户的手动关开一次 = 起一个新助手实例 → 重新解析宿主 → 注入 → 恢复。
  这正是本修复要点：**把那次手动操作交给应用自己**（用户侧零介入）。
- 「网络」与本案的关系：本应用的基础链路（遥控器 → BLE → 音频 → 虚拟声卡 → 输入法）
  **不依赖互联网**；本次网络时段里坏掉的是 ① 蓝牙链路（94 秒后自愈）与
  ② 输入法云端识别（应用边界之外）。全按键支持的失效早于网络事件 2.5 天，
  网络事件只是让用户终于去按、去关开一次——**不需要也不要**做「无网络页面」。

## 修复

- 应用侧（`src-tauri/src/lib.rs`）：`Connected` 分支在「已下发目标 + 一个所有权都没有」
  持续到阈值时**自己重来一次助手**——`rc003_task::task_stop()` 后 `task_trigger()`
  （此刻助手拿不到所有权，清掉它不误杀任何正在工作的实例；新实例重新解析宿主 →
  注入或接管 → 所有权回来即恢复）。
  - `CAPTURE_RECOVER_AFTER_MS = 60_000`：先给足注入 / 握手时间。
  - 防抖 `CAPTURE_RECOVER_MIN_INTERVAL_MS = 60_000`，连续 3 次无效后降速到
    `CAPTURE_RECOVER_STEADY_MS = 300_000`（不修链路，也不每 60 秒反复重启）。
  - 新增日志：`action=capture_recover phase=started reason=agent_never_acked attempt= inactive_ms= next_interval_ms=`、
    `action=capture_recover phase=helper_restarted terminal_result=passed|failed attempt=`；
    `capture_resumed` 增加 `recover_attempts=`，让「这次恢复是自愈救回来的」在日志里可判。
- 未采用的方案：助手侧自检「没有 agent 会话」后自我重启（助手进程是提权实例，
  重启本身还是要走计划任务，等价于应用侧动作，却把状态判断留在了两处）。

## 验证

- 自动化 passed：新增单测 `capture_restarts_helper_only_after_inactive_grace_and_throttled`
  （阈值、防抖、尝试分档、阈值与宽限/观察间隔的常量关系）；
  `cargo fmt --all -- --check`、`cargo test --workspace`（含 `sayall-windows-app` 108 passed / 1 ignored）、
  `cargo check --workspace`、`cargo check -p sayall-windows-app --features runtime-simulation`。
- **RC003 真机「宿主重建 → agent 消失 → 自愈」链路：deferred**——需要构造宿主进程被系统
  重建的现场（本次为自然发生，未受控复现）；本修复复用已被真机验证过的手动路径
  （关开一次开关的那条 `/end` + `/run` + 重新注入），不新增注入逻辑。

## 未覆盖边界（另行决策，不在本修复范围）

1. 「开关开着但还没生效」的**界面提示**仍缺：界面侧要落在单开关设计（PR #241，开关只有
   关闭 / 开启 / 正在操作三态、只有失败才给文案）之上，新句需 Andy 定稿。
2. 遥控器链路中断（本次 94 秒）时主窗口在托盘、没有任何提示：是否用托盘级提醒、
   提醒阈值与频率未定。
3. 复制诊断信息里的 `full_key_support=` 目前读设置、不读实际状态（同一类「显示与事实不一致」）。

## 隐私检查

全文未包含个人路径（仅环境变量形式）、设备身份、语音内容或凭据；日志片段只保留稳定 token
（阶段名、错误域/错误码、计数与毫秒数）。
