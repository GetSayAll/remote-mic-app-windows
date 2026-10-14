# 覆盖安装后的「旧宿主 tap + 无令牌文件」组合：首拉助手退出等人工，恢复窗口内语音键全失败

- 发现日期：2026-10-14
- 状态：已修复（自动化 passed；真机验收 deferred——需随本地测试包在"遥控器连着时覆盖安装"场景复验）
- 影响范围：0.8.6（`6ae4e709` 本地测试包现场）；Windows 11、RC003、「全按键支持」（增强捕获）与语音键；**场景 = 覆盖安装/重装且不重启系统**；RC001 不涉及
- 功能点：RC003 增强捕获清理链（安装器 → 应用优雅退出 → 助手自清 / `--uninstall-cleanup`）+ 首拉决策（`decide_plan` 的 STALE-TAP 分支）+ 应用侧对账（`rc003_auto_trigger_reconcile`）+ Onboarding 第⑤步「按住说话验证」
- 现象：覆盖安装后重走 Onboarding，第⑤步语音验证**前 2 次按下无反应**（豆包不启动、无转写、9.4 s 判 `voice.no_transcript`），第 3 次恢复成功；用户报「豆包输入法前面几次语音输入失败」，并怀疑「Helper 没完全起来」

## 复现条件

1. 遥控器此前连接过，设备宿主 `WUDFHost.exe` 活着，且其内存里有我们注入的 Gadget（旧世代/旧令牌）；
2. 执行覆盖安装/重装（安装器先请求应用退出）——助手清理把 `%ProgramData%\SayAll\rc003-helper\session.token`
   等文件删掉，但宿主未被结束（`frida-gadget.dll` 被映射着删不掉、登记待重启删除）；
3. 装好后启动，走 Onboarding 到「全按键支持」启用（或任何触发首拉助手的路径）；
4. 在「按住说话验证」里于恢复完成前按键。

## 正常预期

- 覆盖安装后首次启用应自动就绪，不需要用户重配对 / 禁用启用设备 / 重启系统（用户侧零介入）；
- 任何"旧 tap 接不上"的现场都应由自动路径收敛，而不是提示人工；
- 恢复窗口内按下语音键不应静默失败（至少应被判定为"未就绪"而不是"没收到转写"）。

## 证据（`%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`，UTC；本机 +08）

**A. 覆盖安装的清理只清了一半（本地 22:21）**

- `14:21:15.243Z` 旧实例 `app_exit graceful_exit_signal phase=completed terminal_result=passed reason=installer_requested_exit`
- `14:21:16.938Z` 助手 12516 `[STOP-SIGNAL] 主程序请求停用，助手退出`
- `14:21:16.945Z` `[CLEANUP] dir=…\rc003-helper files_deleted=3 files_pending_reboot=1 dirs_deleted=0 failures=-`
  ——删了 3 个文件（含 `session.token`），`frida-gadget.dll` 待重启删除；**该自清路径不做宿主释放**（无 `host_release=` 行）
- `14:21:19.018Z` 安装器另起的助手 692：`[CLEANUP] 无法枚举宿主模块，跳过释放：
  CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, 31728) 失败，GetLastError=5`
  ——`--uninstall-cleanup` 的宿主释放因访问被拒失败；宿主 31728 存活、旧 tap 仍在

**B. 首拉撞上组合态（本地 22:22）**

- `14:22:01.861Z` 新实例 19868 启动；`14:22:02.551Z` `onboarding event=migration … source=fresh_install completed_version=0`
- `14:22:23.808Z` 首拉 41056：`[TOKEN] source=generated fp=c4b30949`（磁盘上无令牌文件，现场生成）
- `14:22:23.809Z` `[STALE-TAP] …（或磁盘上本来就没有令牌文件）…无法接管` →
  `[STOP] 退出码 13`（提示三种人工清理法；**不自动清宿主**）

**C. 恢复靠对账重试撞上另一条路径（本地 22:22）**

- `14:22:32.809Z` `rc003 feature=enhanced-capture action=auto_trigger phase=trigger attempt=1`
  （30 s Standby 轮询到点；首拉退出已 9 s）
- `14:22:32.874Z` 第二实例 14480：`[TOKEN] source=file fp=c4b30949` → `[ATTACH] action=skip_injection`
  （按可复用令牌接管）→ `14:22:33.120Z` `[REJECT] reason=token_mismatch` →
  `14:22:33.121Z` `[ATTACH] verify=failed reason=no_authenticated_agent action=early_restart_host_clean_inject`
  → `[HOST] action=terminate_stale_tap_host pid=31728`
- `14:22:35.174Z` `[CLEANUP] device_recover=ok remove_ok=true scan_ok=true`；
  `14:22:35.582Z` `[HOST] action=new_host_ready pid=43756`；`14:22:35.778Z` `[INJECT]`
  `14:22:35.901Z` `[HELLO] auth=true`；`14:22:35.941Z` `voice_synth_ack … active=true confirm=agent`；
  `14:22:36.296Z` `enhanced_capture event=ownership_resumed`
- `14:22:37.858Z` `action=auto_trigger phase=completed terminal_result=passed reason=helper_connected trigger_attempts=1`

**D. 用户的按键夹在恢复窗口里（本地 22:22）**

- session 21（`14:22:28.400Z`）：`voice_attempt … path=send_input synth_confirmed=false fg_self=true`；
  `voice_path fallback … reason=report_layer_inactive note=tool_ignores_injected_keys`；`mic=not_observed`
- session 22（`14:22:34.026Z`）：同型（此时恢复注入尚未完成）
- `14:22:35.594Z` 前端 `voice_attempt result=failed reason=voice.no_transcript detail=d51120_s28320_q0_drain1 elapsed_ms=9416`
- session 23（`14:22:40.628Z`）：`path=report_layer synth_confirmed=true` → `[AGENT] synth:frame replace ok to=0xe6`
  → `voice_effect … result=replaced` → `mic=observed`；`14:22:42.848Z` 前端 `voice_attempt result=passed
  reason=late_transcript detail=after_7251` + `step_recovered gate_satisfied`（向导恢复通过，未失败到底）

**E. 状态对照（时间轴上的令牌指纹）**

- `10-14T14:18:23.479Z` 助手 12516：`[TOKEN] source=file fp=19c338c5`（当时文件仍在）；宿主 31728 内的 agent 即这一代
- `14:22:23.808Z`（41056）：`source=generated fp=c4b30949`——文件在 14:18:24–14:22:23 之间消失
  （即 A 段的删除所致）

## 根因

已确认：

1. **清理链**在这条路径上留下「宿主残留旧 tap + 令牌文件被删」的组合：
   - "停用自清"路径（`[STOP-SIGNAL]`）只删运行时文件、不做宿主释放（2026-10-08 的卸载释放机制未接入该路径）；
   - `--uninstall-cleanup`（本应做宿主释放）在安装器上下文里枚举宿主失败（`GetLastError=5`）被跳过——
     而同一宿主随后由常驻助手（计划任务最高权限）成功终止（C 段），两次权限条件不同的原因待查（`deferred`）。
2. **首拉决策**：`decide_plan` 对「有旧 tap + 无令牌文件」返回 `STALE-TAP`（提示人工 + 退出码 13），
   不自动执行「结束宿主 + 干净首注」——尽管该路径所需的安全闸（宿主独占 RC003 + 确有我们的 gadget 映射）
   当时全部具备，且同日的第二实例证明该动作可行（C 段）。
3. **时序**：首拉退出（23.810）与对账下一个 tick（32.809）之间有 9 s 空洞；恢复完成于 36.296——
   从用户启用操作（23.7）到报告层重新生效共 ~12.5 s。首 2 次按键正落在这个窗口；回退路径
   (`send_input`) 对豆包无效（`tool_ignores_injected_keys`，设计已知），失败不可避免。

仍未知（不写死）：无——`err=5` 的成因已确认（`--uninstall-cleanup` 分支在
`enable_se_debug()` 之前就 exit，见「修复」2）。

## 修复

1. **helper：旧世代 tap 自动恢复**（`6819741`，`hardware/RC003/helper/src/main.rs`）：
   - `decide_plan` 新增 `allow_stale_recovery`（安全闸 = 宿主独占 RC003 且确有我们的 gadget，
     与既有核验升级路径同一判据）；满足时返回新变体 `AttachStale`（暂定接管）——不再
     `STALE-TAP` 退出等人工；不满足时保留原人工路径，并把判据值（members/rc003_members/
     resident_taps）落日志。
   - `AttachStale` 时接管**必失败**（旧 agent 不可能握着本进程刚生成的令牌）：**不等 8 秒
     核验窗口**，第一轮直接升级到"结束宿主 + 干净首注"（复用
     `should_restart_host_for_clean_inject` / `restart_rc003_host`）；新增纯判定
     `stale_attach_escalates_immediately`。
   - `[ATTACH]`/`[DLL]` 对新变体如实标注（`reason=no_reusable_token`、`action=attach_stale_existing_tap`）；
     `[SUMMARY]` 的 `how=` 写**本轮实际**拿回 tap 的动作（升级轮 = `restart_host_clean_inject`，
     不再照抄启动时的放置计划名——2026-10-14 复核修正）。
2. **卸载清理释放宿主前先启用 SeDebugPrivilege**（`fd110c7`，同文件）：`--uninstall-cleanup`
   分支在正常流程的 `enable_se_debug()` 之前就 exit——宿主是 session 0 的 SYSTEM 进程，
   没有 SeDebug 时 `enum_modules` 直接 `GetLastError=5`，`release_injected_host` 被
   "跳过释放"。现于该分支 `host_release_still_needed` 时先启用；失败不中止清理
   （仍回退"安排到重启删除"），日志落 `[CLEANUP] sedebug=not_enabled reason=`。
3. **开关命令唤醒常驻对账**（`07eeb29` + 复核修订，`src-tauri/src/lib.rs`）：新增
   `CAPTURE_RECONCILE_WAKE`（世代计数）+ `wake_capture_reconcile()`；enable/disable
   命令成功后各唤醒一次（disable 侧在设置落盘后即唤醒，写停用信号失败也会尽快发布 Off）；
   对账 4 处长睡眠改为 `interruptible_reconcile_sleep`（≤250 ms 粒度、世代消化）。
   **唤醒轮只重查、不重复 `/run`**（`phase=recheck reason=wake`）——enable 自己已
   task_trigger 过，立刻再触发会在 `IgnoreNew` 下被拒、留误导性 failed 行，还会让
   consecutive 提前起算（2026-10-14 复核）。现场实测 enable 后曾要等 9.1 秒
   （Standby 周期到点）才触发重试。

**评估后不做**（记录理由）：
- "停用自清"路径（`[STOP-SIGNAL]` 场景）**不接入宿主释放**：该职责在 `--uninstall-cleanup`
  收口；修复 2 后安装器清理即可成功释放（宿主消失 → 组合不再产生）。避免应用退出路径做
  8 秒级重活、以及与安装器清理的双进程重叠（两者都会动同一个设备节点）。
- 向导第⑤步的就绪提示：见「后续项」。

## 验证

自动化（本分支全绿）：

- helper：`cargo test --manifest-path hardware/RC003/helper/Cargo.toml` **70 passed**
  （基线 65 + `stale_tap_recovery_tests` 5 例）；`--selftest` **74 项全过**
  （新增「放置策略」2 例、「[DLL] 汇报」1 例、「快速升级」1 例）；helper `cargo fmt --check` 干净。
- 主仓：`cargo test --workspace`（`sayall-windows` 350 + `sayall-windows-app` 112 + 集成 2 项全过）；
  `cargo check --workspace`；`cargo check -p sayall-windows-app --features runtime-simulation`；
  `cargo fmt --all -- --check`；`scripts/ci-preflight.ps1` **7 步全过**（与 push 后 CI 的对应步骤等价）。
- 真机验收（旧计划措辞已被本轮实证修正，见下）：~~覆盖安装应见 host_release~~ ——
  **静默覆盖安装不会触发卸载清理**（实证：无 `[STOP-SIGNAL]` / `[CLEANUP]` / `--uninstall-cleanup` 运行）；
  原 bug 的"无令牌 + 残留"组合来自**卸载 / 重装**路径。

**RC003 真机（2026-10-14 晚，passed）**：

- 环境：本地测试包 `无线麦 SayAll_0.8.6_x64-setup.exe`（SHA-256 `AE698F9B…4B62`，
  内嵌修订 `3acebc3`）；安装后 `sayall-helper.exe` 与构建产物哈希逐字节一致、gadget 哈希与锁一致、
  app exe 与构建产物仅差 3 字节（已知良性 bundle 标记）；`verify-frontend-embedded.ps1` 10/10
  （阴性对照按预期失败）。
- **修复 1 现场（passed）**：构造"无令牌文件 + 宿主残留旧 tap"（操作序列：首次启用 → 助手正常
  接管 → 关闭开关触发助手自清删除运行时令牌 → 再次启用）。再启用时：
  `[STALE-TAP] action=auto_recover_stale_tap`（23:13:27.085 本地）→ `[ATTACH] verify=failed
  reason=no_reusable_token action=restart_host_clean_inject`（**不等 8 秒核验窗口**）→
  `[HOST] terminate_stale_tap_host pid=43756` → `[CLEANUP] device_recover=ok` →
  `[HOST] new_host_ready pid=40688` → `[INJECT]` → `[HELLO] auth=true` → `ownership_resumed`
  （23:13:30.236）。**恢复窗口 3.1 秒、全程无人工**（旧行为：退出码 13 + 提示重配对/禁用启用/重启
  + 最长 30 秒对账等待 + ~12 秒恢复）。
- **恢复后首按即成功（passed）**：用户按住语音键一次——`[AGENT] synth:frame replace ok to=0xe6`
  + `voice_effect result=replaced` + `voice_verify … mic=observed`（session 35），文字上屏（用户确认）。

- **修复 2 直接验证（passed，2026-10-14 晚，操作人执行卸载/重装）**：卸载清理实例 24980
  （elevated，`--uninstall-cleanup`）在"dll 被宿主映射"时：`[CLEANUP] host_release=terminating
  gadget_maps=1` → `host_release=released` → 复删 `files_deleted=1 files_pending_reboot=0
  dirs_deleted=2` → `device_recover=ok` → **`已清理运行时目录（删除了 2 个目录 / 1 个文件）`**
  ——宿主被结束、dll 当场删掉、无待重启残留（旧行为：`GetLastError=5` 跳过释放、dll 留待重启、
  宿主带旧 tap 留下）；随后全新安装正常启动。
- **修复 3 运行时效果真机复验（passed，2026-10-15 凌晨，0.8.6 新包内嵌 `0fe3c8d`）**：升级安装后
  （16:26:40 启动）`action=auto_trigger phase=triggered → completed terminal_result=passed
  reason=helper_connected trigger_attempts=1`；随后用户重启应用并操作开关，16:38:51 出现
  `action=auto_trigger phase=recheck terminal_result=passed reason=wake`，其后 `completed` 为
  `trigger_attempts=0`——**唤醒轮只重查、没有再次 trigger**，与复核修订的行为一致。
- **语音链路真机复验（passed，同晚，0.8.6 新包）**：语音会话 41–44（豆包 + 报告层合成）逐会话
  `[AGENT] synth:frame replace ok to=0xe6` → `voice_effect result=replaced window_ms=500` →
  `voice_verify mic=observed`（800 / 2500 ms 两次）；其中 session 41 为向导第⑤步现场
  （`frontend … phase=voice_attempt result=passed reason=passed elapsed_ms=3606`）；**文字上屏由
  用户确认**（2026-10-15）。同场按键侧 `[EDGE]` 捕获 back / volume_up / volume_down 正常；
  升级未写入重授权标记（授权与开关状态按升级语义保留）。
- `AttachStale` 在"旧 agent 从不重连"下的快速升级：判据已自动化；真机构造依赖旧 agent 静默现场
  （`deferred`）。

## 未覆盖边界

- `AttachStale`"旧 agent 不重连"窗口：见上（`deferred`）。
- 对账唤醒的**运行时效果**（`phase=recheck reason=wake`、无重复 trigger、disable 后状态快速发布
  Off）：自动化覆盖纯判定与打断语义（Rust 单测含真实打断用例）；`reason=wake` 与
  `trigger_attempts=0` 已获真机证据（见上"修复 3 运行时效果真机复验"）；disable 方向的状态发布
  仍为 `deferred`（新包现场未复现 disable 操作）。
- "停用自清"不释放宿主的决策理由见「修复——评估后不做」；若将来再出现"安装器清理同样失败"
  的残余现场，再评估该路径。

## 合入前独立复核与修订（2026-10-14 晚）

两路独立复核（helper / 应用+前端）结论：helper 侧 2 项必改 + 6 项建议、应用前端侧可合入 +
若干完善，本分支已全部处理：

- helper：`--help` 重复行删除；`[SUMMARY] how=` 按**实际恢复动作**取值（抽 `summary_how` + 单测）；
  启动门 gadget 计数限定"运行时目录归属"（`auto_recover_stale_tap_gate` + 单测/selftest，堵住
  "别人注入的同名 frida-gadget 也被判可杀宿主"的新入口）；显式钉 `--token` 时不做自动恢复
  （保守留人工路径）；三处"等核验"文案改为"第一轮即升级"；`[DLL]` 的 `touched_files` 在
  AttachStale 轮移除（该轮实际会写 config/agent/token，字段名不再误导）；`inject_new_generation`
  保底分支补人工清理指引；README / Testing 手册 / run-helper.ps1 三处文档同步。
- 应用/前端：唤醒轮跳过重复 `/run`；disable 唤醒移到设置落盘后；失败解释补 `off` 分支与
  `failure_text_branch detail=capture_<health>` 日志；细化回调陈旧保护加 `!voiceLateAccepted`
  防御；补 3 个页面级回归（在途细化作废 / 读失败回落 / 非豆包不分流）+ `normalizeCaptureHealth`
  用例 + Rust 真实打断用例；三个既有"迟到接受"用例显式模拟"开关开着"的生产现场（原 mock 默认
  `off` 会命中新分流）。

## 后续项

- 向导第⑤步失败解释**已按捕获状态分流**（`1cfe352` + 复核修订，原"就绪提示"评估的落地形态）：
  `voice.no_transcript` 失败瞬间现读 `capture=`——`starting` →「还在准备中…等几秒后点“重新测试”」、
  `inactive` →「关掉再打开」指引、`off` →「现在是关闭的，请先打开它」、`active`/读不到 → 保持原
  排查文案；仅对豆包分流（唯一按设计忽略注入键、必须走报告层合成的工具）。分流落定落
  `failure_text_branch detail=capture_<health>` 日志。起始引导不改（`starting` 期不打断
  "等待按键"，维持既有决定）；真机目视 deferred（下次出包时顺带看一眼失败卡）。
- 顺带发现**已修**：`target_address_tests` 的错位 `#[test]` 已归位（`7b33186`），
  `target_body_requires_its_own_version_and_a_full_address` 恢复运行（单跑 1 passed）。

## 隐私检查

全文未包含个人路径（仅环境变量形式）、设备身份、语音内容或凭据；令牌只保留指纹；
宿主与进程仅以 pid 引用。
