# 2026-11-25 向导第⑥步豆包语音条拉不起来：报告层未生效 + 失败码误判

- 发现日期：2026-11-25（用户反馈 1.9.7；日志覆盖 2026-11-24T16:31Z–17:25Z，UTC）
- 状态：**应用侧已修复**（自动化 `passed`；真机 `deferred`）——助手为何一次都没连上
  仍缺客户机 `helper-task.log`/任务 XML；本分支已修撕裂帧误判、向导就绪提示与
  “助手从未连上”的 `inactive` 判定，并补了定位慢轮询的日志
- 影响范围：1.9.7（`source_revision=98fd2a2…`）与 1.9.5（`4ead96e…`）同一台机器；
  RC001（日志 `ble_model … model=Rc001`）；豆包输入法；Windows 版本日志未记录
- 功能点：向导第⑥步「按住遥控器语音键试一句话」→ 按住说话快捷键 → 豆包语音条 → 音频 → 文字上屏
- 现象：按住遥控器语音键时豆包语音条不出现，输入框始终没有文字；最后卡片写
  「声音没有完整送达语音设备」。

## 证据

### 1. 向导第⑥步期间及前后的每一次按下都走 SendInput 回落，而豆包按设计丢弃注入键

17:18–17:24 共 15 次按下（会话 63–77，含第⑥步前的试按与会话 64/65 的前台为其它应用），
路径与结果完全一致：

```text
17:23:40.927 voice_attempt session=76 tool=doubao path=send_input synth=0x00E6 synth_confirmed=false fg_self=true fg_pid=5600 fg_class=1656eb49 fg_focus=dc007969
17:23:40.933 voice_path fallback session=76 tool=doubao reason=report_layer_inactive note=tool_ignores_injected_keys
17:23:41.738 voice_verify session=76 tool=doubao marker=doubao at_ms=800 mic=not_observed verdict=not_reacted f5_edges=11 f5_leaked=0 f5_key_down=false
17:23:43.438 voice_verify session=76 tool=doubao marker=doubao at_ms=2501 mic=not_observed verdict=not_reacted f5_edges=37 f5_leaked=0 f5_key_down=false
```

会话 63–77 全部 `path=send_input` + `report_layer_inactive` + `verdict=not_reacted`。
按键链路本身是健康的：`f5_edges` 正常增长、`f5_leaked=0`、`f5_key_down=false`；
`key_host action=start result=passed pid=4860` 与 `HOOK kind=f5/gate installed=1` 表明
LL 钩子宿主活着（2026-10-04 的「钩子在主进程」形态已排除）。

豆包 IME 服务在全局 LL 钩子首查 `LLKHF_INJECTED`（0x10），命中即透传丢弃
（字节级逆向 + 独立复现，见 `docs/investigations/2026-09-04-avoid-driver-signing-input-paths*`
与 `Bugs/2026-10-13-helper-does-not-follow-remote-switch.md`）。因此
**报告层没生效时，豆包语音条必然不出现**——这不是偶发，而是该路径的确定结果。

全日志仅会话 61/62（17:20:18、17:20:22，向导之前、前台是另一个应用 pid 1728）
出现 `mic=observed verdict=reacted`。这两次是否由残留报告层 agent 合成、还是豆包
已有录音导致的假阳性，现有日志无法判定，列为待查边界。

### 2. 1.9.7 的增强捕获助手从启动起就没有连上，且自动拉起是“假成功”

- 1.9.5 时段（16:31–17:16）：`auto_trigger_escalate … reason=helper_not_connected_after_retries`
  一路到 `attempt=380`，整段日志 **0 次** `helper_authenticated`。
- 1.9.7 时段（17:18:09 起）：`auto_trigger phase=trigger attempt=1/2/3` →
  `triggered terminal_result=passed`，随后 `auto_trigger_escalate attempt=5`、
  `attempt=10`，仍 **0 次** `helper_authenticated`。
- 同一日志里唯一一条助手进程日志是 17:16:41（升级期间的卸载清理）：

```text
17:16:41.949 pid=17076 component=rc003-helper [ENV] elevated=true pid=17076 port=47831 … target_pid=-
17:16:41.950 pid=17076 component=rc003-helper [CLEANUP] dir=… files_deleted=0 files_pending_reboot=0 dirs_deleted=2 failures=-
17:16:41.951 pid=17076 component=rc003-helper [CLEANUP] 已清理运行时目录（删了 2 个目 / 0 个文件）
```

之后没有任何 `[LISTEN]` / `[HOST]` / `[APP-BRIDGE]` 行。也就是说：
`schtasks /run` 返回 0 只代表任务被受理，**不能代表助手进程真的起来并连上**。
同样的“`triggered passed` 但功能不可用”形态正是
`Bugs/2026-10-26-helper-startup-failure-reported-as-connected.md` 修过的类别；
本轮日志里没有 `capture_unavailable` 行，说明界面侧也没有把“开着但没生效”告诉用户。
第⑥步的三项核对反而全部通过：`ime_status … detected=true matches_expected=true`、
`audio_stream phase=started … endpoint_kind=virtual_cable`、`shortcut … right_alt`，
用户看不到任何“报告层未生效”的信号。

### 3. 失败卡文案与事实相反：音频是满速，却报“声音没有完整送达”

同一次会话 76：

```text
17:23:42.648 audio_session generation=13 action=finish phase=completed terminal_result=passed submitted_samples=26160 queued_samples=0
17:23:42.649 voice_audio_rate result=full rate_hz=14991 samples=26160 duration_ms=1745 expected_hz=16000
17:24:05.928 frontend event=onboarding phase=voice_attempt result=failed reason=voice.audio_delivery_failed step=voice_test detail=d26160_s23280_q0_drain0 elapsed_ms=29084
```

- `voice_audio_rate result=full`、`submitted_samples=26160`、`queued_samples=0` ⇒ 音频投递正常。
- 终态 `detail=d26160_s23280_q0_drain0`：解码增量 26,160，但已提交采样只有 **23,280**
  （会话实际提交 26,160），`drain0` 表示结束时 `audio.phase` 不是 ready。
- 终态出现在 **29.084 s** 之后，且紧跟在会话 77 的 `StreamStart`（17:24:05.878）之后；
  中间 23 s 没有观测到空闲帧。

这与 `Bugs/2026-10-14-voice-no-samples-torn-snapshot.md` 同源：`PlatformSnapshot::snapshot()`
按子系统依次读取，`{voiceState=idle, audio.phase=streaming, submittedSamples=0}`
这种**撕裂帧**被当成了“上一会话结束且音频未投递”，于是判成 `audio_delivery_failed`。
`submittedSamples=0` 是新会话 `BeginSession` 刚重置的结果；`maxSubmitted` 仍是旧值
23,280，正好解释 `s23280`。2026-10-14 只修了“开始”方向的撕裂帧，这次撞的是“结束”方向。

### 4. 前端到 Rust 的 IPC 在故障窗口被阻塞约 15 s

心跳本身每 30 s 一条，`elapsed_ms` 取前端 `performance.now()`：

```text
17:23:21.175 elapsed_ms=311719
17:24:05.916 elapsed_ms=341714   ← 墙钟 +44.7 s，elapsed 只 +30.0 s
17:24:21.174 elapsed_ms=371718   ← 下一次恢复准时
```

若是页面计时器正常触发、日志也即时落盘，两者应同时 +30 s。实际形状是
**心跳在 17:23:51 发出、排队到 17:24:05.9 才被 Rust 处理**（payload 里的 elapsed 仍是
17:23:51 的值）。`get_runtime_snapshot` 是同步 Tauri 命令，跑在主线程；它阻塞时
所有 IPC（含 200 ms 一次语音轮询）都会排队。17:24:05.9 队列释放后，第一个
`get_runtime_snapshot` 恰好落在会话 77 的启动窗口，读到上面的撕裂帧并产生误判。
阻塞发生在 `PlatformSnapshot::snapshot()` 的哪个子系统，现有日志不足以定位
（初次启动的 `runtime_snapshot checkpoint=…` 只记一次）。

## 根因

**用户可见失败的直接根因（已确认）**：报告层（增强捕获助手）没有生效，语音键只能走
`SendInput` 注入；豆包 IME 按设计丢弃 `LLKHF_INJECTED` 事件，因此语音条不会被拉起。
1.9.5 已连续 380 轮拉不起助手，1.9.7 启动后同样 0 次鉴权成功，说明这是机器上的
长期状态，不是 1.9.7 新引入。

**仍未知**：
1. 助手为什么连不上——任务被 `/run` 受理却没有留下任何 `[ENV]` 行；可能是任务 XML 的
   `<Command>` 失效、启动即退出、或日志落点失败。需要
   `%ProgramData%\SayAll\rc003-helper\helper-task.log` 与
   `schtasks /query /tn "SayAll RC003 Helper" /xml`。
2. 会话 61/62 的 `mic=observed` 是否真实（残留 agent 合成 / 豆包已有录音）。
3. 第 4 节 15–23 s IPC 阻塞的具体子系统。
4. 报告层恢复后，自身窗口前台时是否仍需对齐输入法：所有向导会话都是
   `ime_activation outcome=skipped_self_foreground`（应用为自身前台时按设计跳过 TSF 切换），
   `voice_heal … skipped=self_foreground` 同样不动作；而 `ime_status detected=true
   matches_expected=true` 只证明“本机装了豆包”，不证明“SayAll 窗口的活动输入法是豆包”。
   若修复报告层后仍不触发，应在自身前台也做一次只读的 `ime_query`（不切换）取回
   `active_is_target`，再决定是否允许在向导内对齐。

**应用侧产品缺口（已确认）**：第④步的 `available` 只表示“本机架构可用 + 助手文件存在”
（`rc003_task::capture_support`），不表示助手已接管；`captureHealth=inactive` 时
开关旁仍显示「已开启」，第⑥步仍让用户做一次注定失败的测试，最后给出与事实相反的
`audio_delivery_failed` 卡片。

## 2026-11-25 用户复测：保留数据升级 → 故障；清数据重装 → 恢复

用户后续反馈：

- 1.9.7 安装时选择**保留数据** → 豆包第⑥步仍故障；
- 再次安装 1.9.7（这次进入卸载页），**清除应用数据** → 重装正常；但键位/设置需要重配。

判断：**是“保留的授权/助手任务状态”问题，不是 `button-mappings.json` 的键位 bug。**

- 1.9.7 启动加载键位成功：
  `shortcut_settings feature=button_mapping action=load phase=completed terminal_result=passed enabled=true schema_version=5 button_count=7 … unsupported_count=0`；
  没有 `schema_version … action=backup`、`defaults_applied` 或 unreadable 备份。键位文件读取正常，
  排除 `Bugs/2026-11-11-button-mappings-lost-after-upgrade.md` 那类“旧构建读新 schema”问题。
- 故障在升级前就存在：1.9.5 时段（16:31–17:16）已有
  `auto_trigger_escalate … attempt=380`，整段 **0 次** `helper_authenticated`。升级只是把
  `settings.json` 里的 `rc003_capture_enabled=true` 和旧的 `SayAll RC003 Helper` 计划任务
  （设计上跨升级保留，用来承载授权）一起带了过来。
- 1.9.7 启动后只走 `auto_trigger reason=app_startup` + `schtasks /run`；没有
  `enable_capture` / 任务重建（重建需要 UAC）。`schtasks /run` 的 `passed` 只代表任务被受理，
  不代表助手进程真的启动——本轮助手连一条 `[ENV]` 都没有。
- 清数据把 `settings.json` 的 `rc003_capture_enabled` 复位为 false；用户重新开启时走
  `enable_capture` → 提权重装任务 → 助手正常。键位/设置需要重配只是“清数据”的副作用。

应用侧缺口：启动对账检查任务文件、目标**文件名**与电源策略，但不检查
`<Command>` 的全路径是否存在、任务是否 Enabled、登录类型/`LastTaskResult`。
任务目标文件仍是 `sayall-helper.exe` 时 `task_target_matches` 会通过；如果路径/状态已失效，
`/run` 仍可能返回成功而助手不启动。这是“保留数据升级”能带过故障的机制。

规避（不丢配置）：不要清全部数据；把「全按键支持 / 支持更多输入工具」**关掉再打开**
（会重新弹一次 UAC）即可重建计划任务，`settings.json` 与 `button-mappings.json` 保留。
2026-11-25 的 5 轮 `helper_not_connected` + 提示会让用户看到这一步。

待确认/修复决定：

- 失败状态的 `helper-task.log` 与任务 XML 已被清数据覆盖，无法确认具体是 `<Command>` 路径、
  Enabled、登录类型还是 `LastTaskResult`；需测试机人为构造 stale task 复现。
- **启动对账增加任务健康检查（本轮实现）**：读 `<Command>` 全路径是否存在、是否指向当前
  助手，任务是否 `Enabled`，登录类型/运行级别是否仍是助手注册时的形态；失败则
  `CaptureRevoke` 回落开关并显示既有「重新打开一次」文案。
- **安装器直接刷新任务不可行（2026-11-25 修正）**：安装器是 `installMode=currentUser`，
  没有管理员权限；`--install-task` 需要提权。无条件 `ExecShell runas` 会让每次升级都多弹
  一次 UAC，违背「升级保留授权」。条件式修复（只在读取到任务不健康时才弹 UAC）可作为
  后续产品决策，本轮不做；先由启动对账把问题从“静默失败”变成“明确提示 + 拨一次开关”。
- 日志补一行启动任务摘要（只布尔/枚举：`task_installed`、`command_present`、
  `command_exists`、`command_matches`、`task_enabled`、`logon_type`、`run_level`、
  `power_policy`），下一次无需清数据后才能取证。

## 修复（2026-11-25，本分支）

### 1. 结束方向撕裂帧不再误判为投递失败

- `src/onboarding/voice-attempt.ts` 新增 `isTornVoiceEndFrame`：`voiceState=idle` +
  `audioPhase=streaming|draining` + `audioLastError=null` + `queuedSamples=0`。正常结束时后端
  先让 audio 回到 ready、再让 voiceState 回到 idle，因此这个组合只可能是撕裂。
- `observe` 在 `maxSubmitted > 0` 且命中上面判据时，把本次结束视为“上一会话已投递”
  （`endFrameTorn=true`、`drainObserved=true`），进入 `waiting_transcript`；队列非空或
  音频错误仍判 `voice.audio_delivery_failed`，不放过真实失败。
- 终态 `detail` 追加 `_torn1`；页面第一次看到该帧时落
  `voice_attempt reason=torn_end_frame detail=vs_…_ap_…_s…_q…_d…`（原始字段）。
- 回归用例：撕裂结束帧 → `voice.no_transcript` + `_torn1`；真未排空仍
  `audio_delivery_failed`；侧栏“音频输出正常”仍为 ok。

### 2. 向导显示捕获是否真的生效

- `OnboardingPage.vue`：第④/⑤/⑥步每 5 s 读 `get_rc003_bridge_snapshot`；
  `captureHealth=inactive` 且已选豆包时显示 `captureInactiveText`（复用既有文案）。
  第④步替换「已开启」句、第⑤步开关旁、第⑥步输入框上方各一张提示卡。
  `available` 仍只表示架构可用，不再被当成“已生效”。
- 后端 `rc003_auto_trigger_reconcile` 新增“桥在监听、连续 5 轮没有助手连接”判据
  （第一次 `/end`+`/run` 兜底轮，约 25–30 s；10 轮约 2.5 分钟，向导会等不到），
  与既有 accepts 判据并列；发布 `capture_unavailable=startup_failing` 与 `inactive`，
  日志 `reason=helper_not_connected accepts=0 rounds=…`（accepts>0 仍为
  `helper_startup_failing`）。这正覆盖本机“助手一次 [ENV] 都没有”的形态。
- 自动测试：`helper_not_connected_failing_starts_after_planned_rounds`；向导页面新增
  第⑥步进入提示 + capture 日志断言。

### 3. 下一次能直接定位慢轮询

- 前端：`phase=poll reason=slow_snapshot detail=rt_<ms>`（往返 ≥ 1 s）。
- Rust：`runtime_snapshot phase=completed terminal_result=slow`，带
  `connection_ms/audio_ms/raw_input_ms/send_input_ms/button_mapping_ms/voice_priority_ms`。
- 规则见 `LOGGING.md`「向导语音测试慢轮询与撕裂结束帧（2026-11-25）」。
- `get_runtime_snapshot` 仍为同步命令；本轮只加取证，未在没有证据时改线程模型。

### 4. 启动对账识别“任务存在但助手起不来”

- `src-tauri/src/rc003_task.rs` 新增 `TaskSnapshot` / `TaskIssue`：
  - `<Command>` 完整路径是否为空、是否存在、是否等于当前 `locate_helper_exe()` 指向的助手
    （Windows 路径大小写/正斜杠/`\\?\` 前缀规范化，XML 实体反转义）；
  - 任务级 `<Settings><Enabled>`（**不能**读触发器的 `<Enabled>`）；
  - `<LogonType>` 为 `Password` / `S4U` / `ServiceAccount` 时判不可用；
  - `<RunLevel>` 不是 `HighestAvailable`/`Highest` 时判不可用；
  - 电池策略沿用既有判据。
- `lib.rs` 启动对账先落一行任务摘要（无路径、无身份），再按
  `target_file_matches` → `TaskIssue` → `power_policy` → `reauth/task_missing` 的顺序回落。
  新 token `task_unusable` 只用于界面“系统授权需要重新确认”的通用文案；具体原因
  （`task_command_missing` / `task_command_invalid` / `task_disabled` /
  `task_logon_type_mismatch` / `task_run_level_mismatch`）只进日志。
- 这覆盖 2026-11-25 现场：任务在、目标文件名也对，`/run` 被受理，但 `<Command>` 指向的旧
  路径已不存在；旧对账只比文件名，于是静默空转。新对账会在启动时把开关回落并提示用户
  拨一次开关重建（重建仍然需要 UAC，符合“每次开启都重新授权”）。
- 回归用例：健康任务、旧路径不存在、`<Command>` 缺失、任务停用、密码/非最高权限、
  触发器 `<Enabled>false>` 与任务级 `<Enabled>true>` 并存、`<Enabled>` 缺失不猜、
  XML 实体路径反转义。**安装器未改**：它是 `currentUser` 非提权安装，不能直接刷新任务。

### 仍需机器侧

**机器侧先恢复报告层**：更新到含本轮修复的构建后，启动对账会识别不可用任务并把
「全按键支持」回落为关闭，界面显示「系统授权需要重新确认，重新打开一次就能继续使用」；
用户拨一次开关（UAC）即可重建任务。确认日志出现 `helper_authenticated` 且按下时
`path=report_layer`、`synth_confirmed=true`；在报告层恢复前，豆包无法通过向导第⑥步。
若要确认旧任务的具体失效字段，需要客户机提供升级前的
`%ProgramData%\SayAll\rc003-helper\helper-task.log` 与
`schtasks /query /tn "SayAll RC003 Helper" /xml`；清数据后这两份已经不存在。

## 验证

- 前端：`pnpm test` **48 files / 734 passed / 10 skipped**；`pnpm build`
  （`vue-tsc --noEmit` + `vite build`）通过。
- Rust：`cargo fmt --all -- --check`、`cargo check --workspace` 通过；
  `cargo test --workspace` 在 `ci-preflight.ps1` 中全绿（`sayall-windows` 597 passed /
  20 ignored，`sayall-windows-app` 233 passed / 2 ignored，含新增用例）。
- `cargo check -p sayall-windows-app --features runtime-simulation` 通过；
  `scripts/ci-preflight.ps1` **7/7 PASS**。
- 说明：一次手动 `cargo test --workspace` 并行跑到 3 个 `agent_access::tests`
  `local_component_missing` 失败；同批用例 `--test-threads=1` 19/19 passed，
  随后的 preflight 全量重跑 233/233 passed，确认是已知的并发临时目录抖动，与本改动无关。
- 新增/更新用例：撕裂结束帧、真未排空对照、侧栏音频行、第⑥步进入提示 + capture 日志、
  `helper_not_connected_failing`、慢快照阈值。
- **本轮追加（任务健康检查，2026-11-25）**：`rc003_task` 21 passed（含 6 个
  `TaskSnapshot` 用例：健康、旧路径不存在、`<Command>` 缺失、停用、密码/非最高权限、
  触发器/任务级 `<Enabled>` 区分、缺失 Enabled 不猜、XML 实体反转义）；前端
  `pnpm test` **48 files / 734 passed / 10 skipped**、`pnpm build` passed；
  `task_unusable` 的 Rust → bridge → normalize/显示链路已加测试。
- **本地测试包（2026-11-25，`a29a097` + 工作树并行未提交修改）**：
  `target\release\bundle\nsis\无线麦 SayAll_1.9.8_x64-setup.exe`，SHA-256
  `CC0EED21650979D78C40D7BBC7C33AE76BF021A67AE45336F5D4EE87CB4DC627`，本机测试密钥签名
  （**不可发布**）。前端嵌入 **18/18 passed**；静默安装、helper/gadget/sayall-mcp 哈希、
  app exe 差 3 字节、内嵌修订号 `a29a0977` passed；安装后 `settings.json` 的
  `rc003_capture_enabled=true` 保留，任务 XML 为 `InteractiveToken` / `HighestAvailable` /
  `Enabled=true` / 当前安装目录助手；安装版二进制含 `task_unusable` /
  `task_command_invalid` 等新标记。**未启动安装版**（沙箱禁止代启，需用户双击）；
  包内含并行未提交的 `ble.rs`/注释修改，**不是纯净提交包**。
- **2026-11-25 用户实机复测（1.9.8 本地测试包，`a29a097`）passed（健康任务路径）**：
  - `10:56:47.797Z` 启动对账摘要全部通过：
    `task_installed=true helper_path_known=true task_command_present=true
    task_command_exists=true task_command_matches=true task_file_matches=true
    task_enabled=true task_logon_type=InteractiveToken task_run_level=HighestAvailable
    task_power_policy=true`，**没有** `terminal_result=revoked`。
  - `10:56:47.809Z` `auto_trigger` attempt 1；attempt 2 后
    `10:56:53.242Z helper_authenticated helper_pid=704`；
    `10:56:58.099Z auto_trigger phase=completed terminal_result=passed
    reason=helper_connected trigger_attempts=2`。
  - `10:56:58` / `10:57:19` / `10:57:33` 三次语音会话均完成
    `usage_stats voice_begin/voice_finish terminal_result=passed`；
    每次 `voice_attempt ... path=report_layer synth_confirmed=true`；
    session 4 `voice_verify ... verdict=reacted`。用户目视语音正常。
  - 前两次 session 的 `voice_verify` 在 2.5s 内是 `not_reacted`（探测标记，不是语音失败；
    `voice_begin/finish` 仍 passed），本轮未深挖。
  - 最后一条会话有 `transcript_capture action=baseline result=failed
    reason=unsupported_control_type|initial_focus_unavailable`，属于回眸文字抓取，
    与语音/报告层无关，本轮未处理。
  - **stale task 故障路径仍未实机触发**：原故障任务已被用户清数据后重建为健康任务；
    本机已验证“健康任务不被误回落”，`TaskSnapshot` 的失败分支由单测覆盖。
- **deferred（真机）**：客户机恢复报告层后，豆包第⑥步出现语音条/文字；以及现场
  `torn_end_frame` / `slow_snapshot` / `helper_not_connected` 三行的实际出现。
- **deferred（机器侧）**：助手启动失败原因；需客户机的 `helper-task.log` 与任务 XML。
- **deferred（本轮新增边界）**：未做真实的旧任务升级端到端（需要一台已有 stale task 的
  Windows 主机安装本地测试包，观察 `rc003 ... action=reconcile` 摘要与 `reason=task_*`）；
  安装器未改（`currentUser` 非提权，直接刷新任务会多弹 UAC）；RC001/RC003 真机未执行。

## 隐私检查

只使用会话号、稳定 token、计数、窗口类哈希、时间戳与进程号；未包含蓝牙地址、
设备名、用户路径、语音内容或凭据。
