# 助手诊断日志明文记录会话令牌

- 发现日期：2026-10-04（窗口闪现修复的实机复核中顺手发现）
- 状态：已修复（日志侧）；历史日志中已写出的令牌未轮换（见「残留」）
- 影响范围：无线麦 SayAll Windows 版——RC003 增强捕获 Helper 的全部真实运行
  路径（计划任务 / 手动 run / observe）；RC001 不经过该助手。令牌是助手与注入
  agent 之间本地回环/命名管道会话的认证材料
- 功能点：助手日志（`[TOKEN]` 行）与诊断日志的可分享性
- 现象：`[TOKEN]` 行把 `session.token` 的值**明文**写进诊断日志，形如
  `[TOKEN] source=file path=%ProgramData%\SayAll\rc003-helper\session.token value=<32 位十六进制>`
  ——而这份日志是"用户整份发出来排障"的材料
- 复现条件：任意一次真实运行（run / observe；`--selftest` 不经过该路径），
  助手启动阶段即打印
- 正常预期：日志只留可对照、不可用于认证的指纹或长度，绝不写明文
- 证据：
  - 现场日志（修复前，2026-10-04）：`[TOKEN] … value=<32 位明文>`（本记录不摘录
    实际值；原行在 `%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`）
  - 修复后：单测 `imp::token_log_privacy_tests::*`（3 条）与自检项
    「令牌日志脱敏：只写指纹、不含明文」均 PASS；安装包按产品路径复验的
    `[TOKEN]` 行只含 `len=32 fp=<8 位>`
- 根因：三处 `[TOKEN]` 的 `value` 字段调用 `mask_token()`——它只匹配"下划线 +
  12 位十六进制"的蓝牙地址形态（为设备实例名设计），对裸十六进制令牌是
  **空操作**。写日志处以为已经脱敏，实际明文直出。
- 修复（最小改动，`hardware/RC003/helper/src/main.rs`）：
  - 新增 `token_fingerprint()`：`len=<长度> fp=<sha256 前 8 位十六进制>`；
  - 新增 `token_log_fields()`：`[TOKEN]` 日志字段的**唯一构造点**，三处共用；
  - `mask_token()` 文档加红线：只用于设备实例名；令牌一律走指纹；
  - 回归测试：① 指纹稳定性与"不含明文"（纯函数）；② `load_or_create_token`
    端到端写日志断言（直接读日志文本，确认无明文、有指纹）；③ 自检新增一项
    （真机自检即可复验）。
- 验证：
  - 本机：`cargo fmt --check`、`cargo test --release`（26 passed）、
    `--selftest`（含新项）全部通过 = `passed`；
  - 本地测试包重出并安装（`sayall-helper.exe` sha256=`f9faef6d…`、Subsystem=2）后，
    按产品路径（App 启动自动触发 → 计划任务 → 提权助手）复验：新的 `[TOKEN]` 行
    = `len=32 fp=<8 位十六进制>`，行内不含 32 位明文，且用独立 Python 复核
    `sha256(当前令牌)[:8]` 与 fp 一致（指纹实现交叉验证）= `passed`。
- 残留（deferred）：
  - 历史日志里已写出的令牌**仍是当前有效值**（已交叉验证）；是否轮换
    （`--new-token`）需要配合宿主（WUDFHost）重启窗口与 agent 重新落地
    （令牌不一致会走 [STALE-TAP] 分流，影响当前全按键支持），另行决定；
  - 已分享出去的日志无法收回——本次修复只保证今后不再新增泄露。
- 隐私检查：本记录、代码与测试不含真实令牌；示例值均为占位串，日志摘录去掉了
  实际值。
