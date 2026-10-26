# 助手反复启动失败被报成"已连接"：界面长期停在启动中且无提示

- 发现日期：2026-10-22（issue #291，用户报告）；2026-10-26 判定层定位并修复
- 状态：已修复（自动化 `passed`；真机 `deferred`）
- 影响范围：1.0.0（`a4c1311`）与 main（`da0a9fa`）；Windows 11 x64；RC003；任何让助手**每次启动都失败**的外部原因（端口被系统保留 / 被别的进程占住 / 其它启动期错误）
- 功能点：RC003 增强捕获的就绪判定与用户可见状态（`rc003_auto_trigger_reconcile`）
- 现象：开关显示开启、返回/音量±无反应，但界面**没有任何提示**，日志里反而每轮都有一条
  `terminal_result=passed reason=helper_connected`；用户侧结论是"应用不认为自己有故障"，
  所以自愈提示（"应用会自动尝试恢复"）从未出现
- 复现条件：助手在启动阶段就退出（最典型的是 #291 的固定端口被 Docker/WSL2 保留区占用）。
  `helper_authenticated` 一旦发生，桥接就进入 `Connected`，与助手下一秒是否退出无关
- 正常预期：助手真的接管了三键（agent 回执所有权）才算生效；连续失败要让用户看见
  "开着但没生效，正在自动重试"
- 证据：
  - issue #291 日志：`[WAIT] event=listen_retry … reason=address_in_use` ×18 → `[STOP]` 退出码 8，
    同时主程序侧 `rc003_bridge event=helper_authenticated helper_pid=16040` +
    `auto_trigger phase=completed terminal_result=passed reason=helper_connected trigger_attempts=1`；
    实测 STOP 后进程列表已无 `sayall-helper`，端口 free，而状态仍是 passed
  - 45 分钟日志内助手被自动拉起 109 次（约 25 s 一轮），每轮主程序都报 passed
  - 代码判据：`hardware/RC003/helper/src/main.rs` 的 `spawn_app_bridge` 原本在端口绑定**之前**
    （注释写着"刻意放在端口绑定之前"）；`src-tauri/src/lib.rs` 的
    `CaptureHealth` 只有在"过了 20 s 宽限仍无所有权"时才发布 `inactive`，
    而每轮助手活不到几秒 ⇒ 永不进入该判定
- 根因（已确认）：两个独立缺陷叠加。
  1. **顺序**：桥接（命名管道）先于端口绑定启动 ⇒ 主程序可以"握手成功"于一个注定要退出的助手，
     `terminal_result=passed` 因此与功能可用性脱钩。
  2. **判据缺口**：`Connected → Trigger` 之间横跳会让 `consecutive` 每轮归零（既到不了第 5 轮
     `/end`+`/run` 兜底，也永远攒不够 20 s 宽限），而 `starting` 在界面上是**不显示文案**的状态 ⇒
     用户侧完全静默。
- 修复（最小改动）：
  - 助手：`spawn_app_bridge` 移到端口绑定成功、`[LISTEN]` 之后（`prepare_runtime` 之前）。
    "握手成功"从此隐含"传输端口已就绪"，两者不再互相掩盖；桥接与捕获链解耦的性质不变
    （主程序没运行时它照旧安静重试）。
  - 应用：新增累计判据 `advance_helper_accept_streak` —— 本次开启以来助手被接受至少
    `HELPER_STARTUP_FAILURE_ACCEPTS = 3` 次且**一次都没拿到所有权** ⇒ 判定启动失败，
    发布 `CaptureHealth::Inactive`（界面既有文案「还没有生效，正在自动重试；如果一直不恢复，
    请把开关关掉再打开」），并落一条
    `action=capture_unavailable reason=helper_startup_failing hint=check_environment_conflict`。
    一旦拿到所有权或用户关闭开关立即清除（成功时补 `phase=cleared reason=ownership_acquired`）。
  - 为什么用"累计接受次数"而不是"连接存活时长"：助手每轮只活几秒时，对账多数轮次根本
    观察不到 `Connected`（快照从 `Listening` 直接变 `Failed`），只有只增不减的累计量能穿过抖动。
- 验证：
  - 自动化 `passed`：`cargo test -p sayall-windows-app --lib` 全绿；新增两项
    `helper_accept_streak_detects_startup_that_never_takes_ownership`、
    `startup_failure_only_overrides_health_without_ownership`（含阳性对照：健康启动必须清零、
    接管后必须回 `active`、开关关着必须保持 `off`）；`cargo fmt --all -- --check` 通过；
    助手 `cargo check` 通过
  - 真机 `deferred`：需要一台"助手必然启动失败"的机器（如 Docker/WSL2 保留 47831）观察
    `[APP-BRIDGE] enabled` 必须晚于 `[LISTEN]`、`capture_unavailable` 出现、界面出现提示，
    以及原因消除后提示消失
- 隐私检查：只引用端口号、退出码、日志字段与 usage，未包含设备身份、个人路径、语音内容或令牌
