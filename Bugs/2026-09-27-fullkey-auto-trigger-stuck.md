# 全按键支持「正在启动」永不结束：启动自动拉起静默失败 + 协议版本混装

- 发现日期：2026-09-27
- 状态：应用侧已修复（真机回归 deferred，需重出安装包验证）；helper 日志文案已修复（随下次出包生效）
- 影响范围：Windows 桌面应用全按键支持（增强捕获）开关；应用每次重启后的助手自动拉起路径
- 功能点：按键映射 → 全按键支持开关 → 计划任务自动触发助手
- 现象：开关打开（或重启后开关保持开启）后，界面一直显示「全按键支持已开启，正在启动」，助手没有连上来；手动把开关关了再开（弹 UAC 重装任务）才恢复。
- 复现条件：开启全按键支持后重启应用（或安装器触发的应用重启）。
- 正常预期：设置 enabled 且任务在系统里时，应用启动应自动拉起助手并在数秒内连上桥；失败必须有日志、有界重试与自愈，UI 不允许无限「正在启动」。
- 证据：
  - 诊断日志（%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log）：18:05 会话 bridge `phase=listening` 后 38 秒内无任何 `helper_authenticated`，也无 `rc003 feature=enhanced-capture` 事件——启动触发的失败完全不可见；16:42 会话同形状持续 8.7 分钟。
  - 助手日志（%ProgramData%\SayAll\rc003-helper\helper-task.log）：17:58:35 一次运行（pid 33120）`[APP-BRIDGE] event=unavailable reason=descriptor_invalid_or_version_mismatch tries=1..210+` 持续 8 分钟——助手在跑但读不懂描述文件，且 note 误导为「主程序未运行属正常现象」。
  - 代码：启动自动触发为 `std::thread::spawn(|| { let _ = rc003_task::task_trigger(); })`——无日志、无重试；且触发点位于 `create_platform()` 之前，早于 bridge 描述文件写出。
  - 协议版本：分支头两侧 `BRIDGE_PROTOCOL_VERSION=2`，当日安装版应用（写入 `version=1`）与分支头助手（要求 `version=2`）混装时，助手按上述理由永久拒绝描述文件。
- 根因：四层叠加。
  1. **触发先于描述文件**：自动触发在 platform 创建（bridge bind + 写 `rc003-bridge.ini`）之前执行，助手可能读到上一轮主程序的过期端口。
  2. **触发失败完全静默**：`schtasks /run` 被拒（上一次任务实例未退出 + `MultipleInstancesPolicy=IgnoreNew`、电池策略、路径漂移）时无日志、无重试。
  3. **无自愈对账**：桥停在 listening 且无助手连接时，没有任何机制重新触发或如实报错，UI 永远「正在启动」。
  4. **版本割裂无告警**：新 helper + 旧 app（或反之）混装时 `descriptor_invalid_or_version_mismatch` 永久复现，note 却归因为「主程序未运行」。
- 修复：
  - `src-tauri/src/lib.rs`：自动触发移到 platform 创建之后；`let _ = task_trigger()` 替换为 `rc003_auto_trigger_reconcile`——每轮以桥接快照（独立外部观察）判定（`classify_auto_trigger` 纯函数 + 回归测试），最多 4 轮触发、每轮间隔 5s、重试前重读设置（用户中途关闭即收手）；全部落空后 `/end`（清卡住实例）+ `/run` 兜底一次；全程结构化日志（`action=auto_trigger` / `auto_trigger_escalate`）。
  - `hardware/RC003/helper/src/main.rs`：`[APP-BRIDGE] event=unavailable` 的 note 按 reason 区分，版本不符给出「检查两侧 BRIDGE_PROTOCOL_VERSION」的指引，不再误导为「主程序未运行」。
- 验证：
  - `cargo test -p sayall-windows-app --lib`：36 passed（含新增 `auto_trigger_check_routes_by_bridge_phase`、`auto_trigger_check_aborts_on_simulation_default_snapshot`），`passed`。
  - `cargo check -p sayall-windows-app`（默认 + `--features runtime-simulation`）、`cargo check`（helper crate）、`cargo fmt -p sayall-windows-app -- --check`：`passed`。
  - 真机验证（重启应用后助手自动连上、UI 状态翻转）`deferred`——需重出安装包并覆盖：普通重启、安装器触发重启、开关关闭中重启三种场景。
- 隐私检查：日志只记录动作、阶段、尝试次数与错误分类，不记录设备身份、路径或用户输入。
