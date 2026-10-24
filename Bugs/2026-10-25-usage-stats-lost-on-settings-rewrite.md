# 个人中心统计在其它构建写回设置时丢失按控件明细

- 发现日期：2026-10-25
- 状态：已修复
- 影响范围：Windows 本地测试包；个人中心统计（累计按键、两个排行榜、语音排行、最长单次语音）
- 功能点：`UsageStatistics` 持久化与个人中心读模型
- 现象：累计按键 220 次，但「实体按键排行」合计只有 22 次；2026-10-24 的 164 次按键没有任何按控件明细，语音排行明细也曾整批消失。
- 复现条件：
  1. 用包含 `button_presses_by_control` / `voice_session_ranking` 的构建记录若干按键与语音；
  2. 安装并运行不含这些字段的构建（例如 main 分支 `d199a61`），期间触发一次设置写入（语言、主题等）；
  3. 再回到包含统计字段的构建，打开个人中心。
- 正常预期：换构建/升级不应删除已有的按控件计数与语音排行。
- 证据：
  - `<配置目录>\settings.json` 中 `usage_statistics.days["2026-10-24"]` 为 `button_presses=164`、`button_presses_by_control` 缺失；`2026-10-25` 为 `button_presses=56`，其中 `voice=34`、实体键合计 22，两者一致。
  - `sayall-diagnostic.log` 中 2026-10-25 的 `usage_stats action=voice_begin` 共 34 条（与 byControl 的 `voice=34` 吻合），说明当前记录逻辑没有错；缺口来自旧日期。
  - 同日 23:49 的设置写入发生在 main 构建运行期间，之后才出现 `byControl=0` 的历史日期。
- 根因：统计与其它设置共用 `settings.json`。Rust `AppSettings` 只序列化当前版本已知字段；不含统计字段的构建写回设置时，会把 `button_presses_by_control`、`voice_session_ranking` 等一起删除。累计按键是较早字段，因此保留下来，造成「总数在、明细没了」的错位。
  - Mac 对照：每日统计存在 UserDefaults 的 `usage.dailyStatistics`，语音排行在 `usage.voiceSessionRanking`，与其它设置分开保存，因此不存在同类覆盖。
- 修复（2026-10-25）：
  - 统计迁移到与 `settings.json` 同目录的独立文件 `usage-statistics.json`（`UsageStore`，原子地整文件写入；写失败保留事件重试）。
  - 首次启动把旧 `settings.json` 的 `usage_statistics` 迁移一次（日志 `usage_stats action=migrate ... source=settings`），之后设置写入不再触碰统计。
  - `get_usage_profile` 改用内存快照，不再依赖设置文件读取。
  - 文件：`src-tauri/src/usage.rs`、`src-tauri/src/settings.rs`、`src-tauri/src/lib.rs`。
- 验证：
  - `cargo test -p sayall-windows-app usage::tests`：7 passed（批次写入、写失败重试、迁移只执行一次、设置写入不触碰统计）。
  - `scripts/ci-preflight.ps1`：7/7 passed。
  - 安装版首次启动后生成 `usage-statistics.json`，日志出现 `usage_stats action=migrate ... terminal_result=passed`。
- 边界：已经丢失的历史明细无法恢复；2026-10-24 的 164 次只能继续计入累计值。
- 隐私检查：未包含个人路径、设备身份、语音内容或凭据。
