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
- 顺带纠错：`capture_inactive` 行的 `hint=reconnect_remote` 改为 `hint=auto_restart_helper`。
  hint 字段写的是**下一步动作**，而这条线断在 PC 内部（助手 ↔ 宿主内 agent），遥控器侧
  动作对它无效——该措辞是 2026-10-08 同型现场加上的，本次现场证伪：用户在失效期间按了
  多次遥控器（session 43–49），恢复只发生在换新助手实例那一刻。界面文案若复用这里的
  结论，同样不能把用户往遥控器侧引；遥控器侧「按一下任意键可以加快唤醒」只对 BLE
  连接阶段的 `Unreachable`（「暂时没有找到小米语音遥控器」）成立，依据见
  `crates/sayall-windows/src/ble.rs` 的同名文案与 2026-09-22 实测注释。
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
- 2026-10-14 跟进：「按下无生效证据」这一形态（`reason=voice_synth_no_effect`）已实现并
  真机 passed——同一条 `capture_recover` 路径在 `attempt=1` 上
  `phase=helper_restarted terminal_result=passed`；见
  `Bugs/2026-10-14-foreign-host-agent-takes-slot.md`「跟进」。宿主重建形态本身仍 deferred。

## 未覆盖边界（不在本修复范围；逐条标注决策状态）

1. 「开关开着但还没生效」的**界面提示**：文案已定稿（2026-10-13 Andy，**按状态分叉**，
   实现等 PR #241 合入后基于 main 做——要落在单开关三态设计之上）：
   - 遥控器链路本身也断 → **复用既有句**（`crates/sayall-windows/src/ble.rs` 的
     `Unreachable` 分支，同源不新造）：「暂时没有找到小米语音遥控器（它可能还在休眠），
     正在自动等待它恢复。」；
   - 只有捕获没生效（遥控器正常）→「全按键支持还没有生效，正在自动重试；如果一直不恢复，
     请把开关关掉再打开。」（连接页名称随该页写法换成“支持更多输入工具”）。
   两条都**不得**把用户往遥控器侧引（本条线断在 PC 内部，见上面「顺带纠错」）。
   显示时机（实现时定稿）：**后端判定「没生效」即显示**，与诊断日志同一条判据
   （助手连上 + 目标已下发 + 20 秒宽限内无任何所有权）；`starting`（助手还在连）
   一个字都不显示。落点与判据同产品文案 §7 的 2026-10-13 条。
   **已实现（2026-10-13，本分支）**：`src-tauri/src/lib.rs`（`CaptureHealth` 发布 +
   桥接快照命令带 `captureHealth`）、`src/lib/capture-switch.ts`
   （`shouldShowCaptureInactive` / `captureInactiveText`）、按键页与连接页各一处。
2. 遥控器链路中断（本次 94 秒）时主窗口在托盘、没有任何提示：是否用托盘级提醒、
   提醒阈值与频率**仍未决策**。
3. 复制诊断信息里的 `full_key_support=` 目前读设置、不读实际状态（同一类「显示与事实不一致」）。
   **已实现（2026-10-14，`fix/diagnostics-capture-state`）**：诊断块在 `full_key_support=`
   （开关设置）之后并列 `capture=`（**真实捕获状态**，读回 `CaptureHealth`：off / starting /
   active / inactive；读不到或未知 token 写 `unknown`），并在复制时对开关状态与捕获状态各做
   一次只读刷新（读失败各留一条 `read_capture_switch` / `read_capture_state` 失败日志；
   ①–③步复制不再把开着的开关误报成 off）。「开关开着却没生效」的判据 = `capture=` 不是
   `active`（`inactive` 过宽限仍无所有权；`starting` 助手/桥未就绪、可能长期停留）。
   **真机复核 deferred**（需新包观察真实块输出）。

## 隐私检查

全文未包含个人路径（仅环境变量形式）、设备身份、语音内容或凭据；日志片段只保留稳定 token
（阶段名、错误域/错误码、计数与毫秒数）。
