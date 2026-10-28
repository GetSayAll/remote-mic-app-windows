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

## 对语音链路（识别）的影响评估（2026-10-27 审查）

结论：**本修复不改变任何音频采集、报告层合成与注入路径**，因此不影响语音识别精度；净效果是
让更容易被这条缺陷拖成"报告层永不生效"的机器恢复走报告层。逐条依据：

- **助手不碰音频**：`hardware/RC003/helper/src/main.rs` 里 `WASAPI` / `IMMDevice` /
  `IAudioClient` 出现 **0 次**。音频采集与 ASR 都在应用侧（VB-Cable + 豆包/微信输入法），
  与本提交改动无交集。
- **语音键的 DOWN/UP 不依赖主程序桥接**：合成发生在宿主内的 agent（报告层单槽替换），
  按下沿与释放沿成对来自同一份物理报告，经 HID 栈直接进 OS。`APP-BRIDGE` 只负责把**边沿通知**
  送给应用做映射动作/界面高亮，缺席不会造成粘键或丢音频。
- **合成门禁只看 agent 回执**：`crates/sayall-windows/src/key_gate.rs` 的
  `VOICE_SYNTH_ACTIVE` 由 `set_voice_synth_active` 写入，唯一来源是 `rc003_bridge.rs`
  的 `synth_confirmed && voice_synth_to.is_some()`（配置回执 ∧ 当前目标存在）。
  它**不读** `CaptureHealth`，因此本提交新增的 `inactive` 发布不会让语音键改走
  `SendInput` 回落（那条回落才会被豆包按 `LLKHF_INJECTED` 丢弃）。
- **`CaptureHealth` 的消费面只有文案与诊断**：Rust 侧只在 `Rc003BridgeView` 里作为
  IPC 字符串返回（`get_rc003_bridge_snapshot`）；前端消费点为开关旁提示
  （`shouldShowCaptureInactive`）、语音**失败卡**文案（`voiceFailureText`）、诊断串。
  没有任何采集/注入/门禁分支读它。
- **顺序改动的时序差可忽略**：`spawn_app_bridge` 是 `std::thread::Builder::spawn`（非阻塞，
  毫秒级）。移动后从启动到 `[LISTEN]` 的耗时不变；固定端口重试期间（最多 3 s）桥接不起，
  但 agent 连的正是这个端口——端口就绪前 agent 不可能产生边沿，因此不存在"边沿被桥接缺席吞掉"
  的窗口。
- **净收益方向相反**：旧行为下助手反复重启会让每次鉴权都
  `set_voice_synth_active(false)`（`rc003_bridge.rs:1532`），语音键在这段时间里走注入回落。
  修复减少重启次数 ⇒ 门禁不再被反复重置 ⇒ 语音路径更稳定。

## 文案：不再给无效建议（2026-10-27 补完）

原先的遗留问题：`inactive` 复用既有「还没有生效，正在自动重试；如果一直不恢复，请把开关关掉
再打开」。对"端口被外部保留/占用"这一类原因，关开是无效操作（#291 现场已试过），
精确指引只写进诊断日志（`hint=check_environment_conflict`）。

现在补一条独立的"原因"通道：

- Rust：新增 `CaptureUnavailable`（`none` / `startup_failing`），由常驻对账在
  `startup_failing` 变化时发布，经 `Rc003BridgeView.captureUnavailable` 给界面；
  与 `CaptureHealth` 分开（health 回答"生效了吗"，原因回答"为什么没生效"）。
- 前端：`captureInactiveText(name, unavailable)` 据此选句。`startup_failing` 时改为
  「{名称}暂时没能启动，正在自动重试。如果一直不恢复，请把开关关掉再打开一次；还是不行就重启这台电脑。」
  （**2026-10-27 二次定稿**：第一版写"退出那个程序后会自动恢复"，用户当场指出他无从定位
  是哪个程序——那等于没给操作。定稿只给两步做得到的事）；读不到原因（`unknown`）一律退回
  通用句，不做猜测。
- 同时（同日）：端口被**真实进程**占着也不再算"启动失败"——助手查清占用者后一律换端口，
  只有"另一支 sayall-helper 占着"才退出。见
  `Bugs/2026-10-24-rc003-helper-port-phantom-reservation.md` 的"第三段"。
- 术语边界：不出现端口、进程、助手、驱动（`docs/product-copy.md` §3 红线）；
  决定已记入同文件 §7（2026-10-27 条）。
- 测试：Rust `capture_unavailable_token_matches_frontend_contract`（token 与前端类型逐字对应 +
  发布/读取往返）；前端 `captureInactiveText` 的 startup_failing 分支（含"不得出现其他程序"
  与术语红线断言）、`normalizeCaptureUnavailable` 未知值回落、连接页渲染断言。


