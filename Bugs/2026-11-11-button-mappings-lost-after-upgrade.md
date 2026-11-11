# 升级/换装后按键配置显示为空并被默认值覆盖

- 发现日期：2026-11-11
- 状态：已修复（存储层与前端自动化验证通过；现场升级路径待用户日志确认）
- 影响范围：Windows 主程序；按键映射（`button-mappings.json`）。可能发生在低版本安装包覆盖高版本、同一版本号的测试包/预览包互相覆盖，以及任何映射文件损坏或校验失败的启动。
- 功能点：按键配置持久化与升级兼容
- 现象：升级或换装后打开「按键」页，所有按键回到默认（未设置），原有自定义动作不可见；随后任意一次保存会把默认值写回文件。
- 复现条件：
  1. 先用映射结构版本较高的构建写一份 `button-mappings.json`（例如 `schemaVersion: 5`）；
  2. 再用支持版本较低、但安装器仍允许覆盖的构建启动（同版本号重装、预览包与正式包同版本、或文件损坏）；
  3. 启动日志出现 `button_mappings schema_version=5 supported_max=4 action=backup` 与 `shortcut_settings feature=button_mapping action=load ... terminal_result=failed ... reason=defaults_applied`；
  4. 在按键页做任意修改或点「总开关」，保存把运行时默认值写回，原配置只剩 `.v5.bak` 旁证。
- 正常预期：旧构建读到更高结构版本的配置时，应尽可能保留并读取已知部分，而不是整份丢弃；读不出的未知动作/字段必须原样保留、不得执行，保存时也不得把结构版本降级或把未知内容抹掉。确实无法解析的文件仍须留备份并拒绝静默覆盖。
- 证据：
  - 本机 Windows 运行日志（2026-11-09、2026-11-10）共 3 次同类失败：`schema_version=5 supported_max=4 action=backup` + `terminal_result=failed reason=defaults_applied`。
  - 日志中 `previous_version=1.8.3 current_version=1.8.3` 说明同版本号的构建被反复覆盖安装；版本字符串不足以阻止旧构建读新文件。
  - 配置目录中可见 `button-mappings.json.v5.bak` 等按结构版本命名的旁证；说明更高版本数据确实被备份，但运行时已回落默认值并可能在随后保存时覆盖原文件。
  - 真实历史文件兼容性探针（当前主线加载管线）：schema 3 / 4 / 5 的真实配置均 `PROBE ok`，按键数 10 / 12 / 12 全部保留。由此排除「正常向前升级时解析失败」。
  - 安装器生命周期测试与 2026-09-27 真机记录均证明 `/S /UPDATE` 升级不删除四份设置/映射文件；NSIS 的「删除应用程序数据」分支仅在 `$DeleteAppDataCheckboxState = 1` 且 `$UpdateMode <> 1` 时执行。
- 根因：
  1. `ButtonMappings::normalized()` 对高于当前支持的 `schemaVersion` 直接返回错误；启动层捕获后把运行时映射替换为 `ButtonMappings::default()`。旧构建因此把“看不懂的新字段”误判成“整份文件不可用”。
  2. `save_button_mappings` 原先只做 `fs::write`，不检查磁盘上的现有文件；前端拿到的运行时默认值一旦保存，就把读不出的原文件覆盖。版本护栏只留下 `.vN.bak`，没有阻止覆盖，也没有让用户看到原因。
  3. 写入不是原子的：升级/退出在写入中途终止进程时可能留下截断 JSON，下一次启动同样回落默认值。
- 修复（前向兼容读取 + 保存兜底）：
  - `ButtonMappings::normalized()` 不再拒绝更高 `schemaVersion`，改为保留其中的较大值；普通保存在磁盘版本更高时也会把内存版本抬回磁盘值，避免旧客户端漏传 `schemaVersion` 时降级（显式恢复默认/导入除外）。
  - `ButtonAction` 改为自定义 `Serialize`/`Deserialize`：未知动作类型保留为 `UnsupportedAction { action_type, raw_json }`，嵌套字段原样保存；加载后既不执行也不丢弃。`button_mapping.rs` 对这类动作记录 `map_skip_inject reason=unsupported_action_type action_type=<token>`（token 仅保留 ASCII 字母数字与 `_`，最多 32 字符）。
  - `ButtonMappings` / `ButtonActions` 增加 `#[serde(flatten)] extra`，未知顶层字段与未知每格字段参与 round-trip，保存时写回。
  - `ButtonActions` 新增 `action_extras`：更高版本写在**已知动作内部**的未知字段（如 `shortcut` 上新增的可选字段）按触发方式保存，序列化时合并回原动作对象；替换动作时清掉对应槽位的 extras，避免旧字段粘到新动作上。
  - 前端 `actionSummary` 对未知类型返回「不支持的动作（来自更新版本）」；编辑面板给出说明，未知动作不会被当成「未设置」，也不能设为「按住连续执行」。第一次选择只解除保护，再次选择才替换，避免误点覆盖。
  - `settings.rs` 保留保存前检查 `inspect_existing_button_mappings`，但「更高结构版本」不再视为不可读；只有非法 UTF-8、JSON 解析失败或当前字段校验失败才拒绝普通保存，并先复制时间戳备份（`button-mappings.json.unreadable-<毫秒>.bak`），记录 `error_code=unreadable_existing_file` 与 `backup_file`（只记文件名）；相同内容只保留一份备份。
  - 更高版本的 `.vN.bak` 改为原子写入并在**每次保存前**再次确认内容一致；加载时复制失败只记录 `result=failed`，保存时备份失败则拒绝覆盖。
  - 新增 `replace_button_mappings`：仅「恢复默认」与「导入配置」使用；允许替换不可读文件，但覆盖前同样留时间戳备份。
  - 映射写入改为临时文件 + `sync_all` + `rename` 原子替换；失败时保留原文件并清理临时文件。
  - `src-tauri/src/lib.rs` 的 `reset_button_mappings` 改走显式覆盖路径；`test_button_mapping` 对未知动作返回明确错误而不是误执行。
- 验证：
  - 新增/改写用例（先红后绿）：`load_and_save_preserve_newer_button_mappings`（schema 9 + 未知动作 + 未知字段 load → save 后完整保留，版本不降级）、`save_ensures_version_backup_even_without_load`、`save_merges_existing_forward_compatible_extras`（调用方漏带未知字段时仍保留；动作被替换时不继承旧 extras）、`export_preserves_unknown_actions_and_known_action_extras`、`save_refuses_to_overwrite_corrupt_button_mappings_and_backs_it_up`、`save_refuses_to_overwrite_non_utf8_button_mappings`、`repeated_failed_saves_reuse_the_same_unreadable_backup`、`empty_button_mappings_file_is_treated_as_missing_for_writes`、`explicit_replace_can_overwrite_unreadable_button_mappings_after_backup`。
  - `crates/sayall-windows`：`future_schema_version_is_read_and_preserved_with_unknown_actions` 覆盖 schema 99、未知动作嵌套载荷、未知顶层/每格字段、**已知动作内部未知字段**与二次 round-trip；`legacy_single_action_unknown_fields_round_trip` 覆盖旧式单动作形状；`unknown_long_action_does_not_reject_single_hold_repeat` 覆盖未知长按 + 有效 `holdRepeat=single`；`unknown_long_action_does_not_hide_invalid_single_hold_repeat` 确保空/不可重复目标仍被判无效；`allows_repeat` 对未知动作返回 `false`。
  - `cargo test --workspace -j 1`：全绿（`sayall-windows` 544 passed / 16 ignored；`sayall-windows-app` 218 passed / 2 ignored；其余 crate 与 doctest 全通过）；`cargo test -p sayall-windows-app settings::tests`：42 passed；`cargo fmt --all -- --check`、`cargo check -p sayall-windows-app --features runtime-simulation` 通过。首次复跑时既有 `macro_runtime::tests::runtime_rejects_same_macro_reentry_and_reports_terminal` flake 出现一次（`Completed` vs `Cancelled`），单独重跑通过，随后全量通过；本次未改该模块。
  - 前端 `pnpm test`：41 个文件 / 679 passed / 10 skipped；新增 `bridge` 未知动作摘要、`ButtonsPage`「未知动作不会被自动清掉 holdRepeat」「未知长按不会清掉单击 holdRepeat」「未知长按不能掩盖无效 holdRepeat」「未知动作需二次选择才替换」用例。`pnpm build`（`vue-tsc --noEmit` + `vite build`）通过。
- 已排除/待确认的其它路径：
  - 应用内更新（`/P /UPDATE /R`）与静默 `/S /UPDATE` 不删除用户配置：NSIS 的删除应用数据分支要求 `$DeleteAppDataCheckboxState = 1` 且 `$UpdateMode <> 1`；生命周期测试对设置与映射做逐字节哈希断言，2026-09-27 真机记录同样为四份文件不变。
  - 手动双击安装包覆盖安装：默认选项会先调用旧卸载器且不带 `/UPDATE`，此时会显示「删除应用程序数据」复选框；**只有用户主动勾选**才会删除 `%APPDATA%\app.getsayall.remote-mic.windows`。这类删除没有备份，现有存储层修复无法恢复；需向报障用户确认升级方式与该复选框状态。
  - 正常向前升级：schema 3 / 4 → 5 的真实历史文件由当前加载管线成功读取（10 / 12 / 12 个按键全部保留），不是本次报告的根因。
- 边界：
  - **修复只对包含本次改动的构建生效**：已经发布的 1.8.3（schema 3）无法追溯获得前向兼容；用户必须先升级到包含修复的版本，之后再用旧构建读取新文件才会保住已知内容。
  - 已被旧构建覆盖的历史配置不能自动恢复；需要从 `button-mappings.json.vN.bak` 或 `button-mappings.json.unreadable-*.bak` 恢复。当前版本没有自动恢复 UI。
  - 未知动作只保留、不执行；依赖它的按键在旧构建上表现为「不执行」，需要升级到写入它的版本才能恢复。
  - 未知**按键名**（`actions` 映射的 key 不在当前 `RemoteButton` 集合内）仍会导致整份反序列化失败，走备份 + 拒绝覆盖的兜底；本次未改按键名解析。
  - `AppFocusProfile`、`CustomAppPick`、`ScrollRegionTarget` 的未知子字段仍未做 round-trip；这类嵌套业务对象需要时再按同一策略扩展。
  - 未来新增的枚举值（例如新的 `holdRepeat` 取值、新的 `ScrollDirection`）仍可能让整份文件解析失败；当前只对“未知动作类型”和“未知字段”做前向兼容。
  - 本次未复现用户现场的完整升级链（缺少用户版本、安装包来源与日志）；上述根因由本机同类失败日志 + 可重复单元场景确认，现场路径仍标记 `deferred`。
  - 组合动作库 `combined-actions.json` 与快捷键文件仍使用直接写入；同类升级/截断风险未在本次范围内处理。
- 隐私检查：未包含个人路径、蓝牙地址、HID 路径、语音内容或凭据；备份文件名为相对文件名，不含用户目录。
