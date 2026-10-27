# 回眸：每个应用第一次语音输入没有记录

- 发现日期：2026-10-25
- 状态：已修复（修复包已安装，等待真机复验）
- 影响范围：Windows 1.0.0（提交 `44ce5f6` 本地测试包）；RC003 报告层语音路径；聊天类富文本编辑器（DimAgent、WorkBuddy）已观察到，Notepad 未复现
- 功能点：`sayall-windows::transcript_capture`、UIA 快照、连续新增判定
- 现象：在同一个应用里，第一次语音输入完成后回眸页没有新增记录；第二次及以后正常。语音文字本身已上屏。
- 复现条件（用户报告）：打开目标应用，首次开启回眸或首次在该应用输入，按住语音键说一句话；第二次重复可生成记录。
- 正常预期：第一次语音实际写入目标输入框的新增文字也应生成一条记录。

## 现有日志证据

日志时间 `2026-10-25T07:26–07:29Z`，安装版 `source_revision=44ce5f6c…`：

- `generation=6`（DimAgent，前台 PID 6108）：`baseline chars=8` → `activate` → 5ms 后 `cancel reason=discontinuous_text_change`。
- `generation=7`（同一前台 PID）：`baseline chars=15` → `activate` → 约 2.2s 后 `save reason=stable chars=24`。
- `generation=8`（WorkBuddy，前台 PID 16396）：`baseline chars=27` → `activate` → 8ms 后 `cancel reason=discontinuous_text_change`。
- `generation=9`（同一前台 PID）：`baseline chars=11` → `activate` → 约 1s 后 `save reason=stable chars=5`。
- `generation=10`（回到 DimAgent）：`baseline chars=11` → `activate` → 3ms 后 `cancel reason=discontinuous_text_change`；`generation=11` 随后保存成功。

当时结论（已被新诊断日志取代）：失败发生在连续新增判定的**第一次差异**，不是 UIA 完全读不到；旧日志没有差异形状与选区，无法确认是选区替换还是普通删除。

## 新诊断日志（2026-10-25T07:49–07:51Z，`source_revision=5d385686…`）

同一应用内连续失败；失败时差异形状如下（只列字符数，不含文字）：

- `generation=1`：基线 8 字、选区 0；当前 20 字；prefix=0、suffix=0、removed=8、inserted=20。
- `generation=3`：基线 20 字、选区 20 字；当前 5 字；prefix=0、suffix=0、removed=20、inserted=5。
- `generation=4`：基线 5 字、选区 5 字；当前 5 字；prefix=1、suffix=0、removed=4、inserted=4。
- `generation=6`：基线 15 字、选区 15 字；当前 8 字；prefix=0、suffix=1、removed=14、inserted=7。
- `generation=7`：基线 8 字、选区 8 字；当前 8 字；prefix=0、suffix=6、removed=2、inserted=2。
- 同一应用纯插入时（基线 8、当前 15、removed=0、inserted=7）可正常保存。

根因确认：语音开始时输入框的**全部文字处于选中状态**（或第一次输入时整个旧字段被替换），语音结果以“替换选区/替换整个字段”的方式写入；旧实现只要发现旧文字被删除就判 `discontinuous_text_change`，因此把第一次有效输入丢掉。第二次输入时基线变成刚写入的文字，常被判定为纯插入，所以表现成“第二次正常”。

## 修复

- 新增 `continuous_insertion_with_selection`：
  - 纯插入继续走原有严格规则；
  - 基线选区覆盖整个输入框时，接受整段替换，只保存替换后的新文字；
  - 无选区但整个旧字段被替换时，接受整段替换（覆盖占位/初始文字）；
  - 局部选区替换仍拒绝，因为 UIA 不提供可靠偏移。
- 捕获线程保留原有 `discontinuous_text_change` 兜底，并对接受的替换写 `reason=whole_selection_replacement|whole_field_replacement`。
- 不保存任何旧文字；日志仍只写字符数与形状。

## 验证

- `cargo test -p sayall-core transcripts::tests`：整段选区替换、整段字段替换、局部替换拒绝均通过。
- `cargo test -p sayall-windows transcript_capture`：新增“整段选中后替换可保存”通过。
- 待新包在 DimAgent/WorkBuddy 上复验首次输入。

## 原诊断字段（前一构建）

只增加脱敏字段，不改变捕获行为：

- `action=baseline` / `activate`：`application_key`、`chars`、`selection_present`、`selection_chars`。
- `action=finish result=pending`：结束命令到达时基线是否已就绪。
- 每次文本差异：`result=candidate|rejected`、`baseline_chars`、`current_chars`、`prefix_chars`、`suffix_chars`、`removed_chars`、`inserted_chars`、选区是否有内容及字符数。
- `action=cancel` / `save`：带上述差异形状与基线字符数。

不记录文字内容、窗口标题、文档名或路径。

## 复验步骤

1. 用修复包在 DimAgent、WorkBuddy 各制造一次“整个输入框被选中后语音输入”。
2. 预期日志出现 `result=candidate reason=whole_selection_replacement`，随后 `save reason=stable`。
3. 再验证纯插入、局部替换（应仍拒绝）、切换应用、快速连按。
4. RC001 与 RC003 分别执行 `Testing/WindowsReflections.md` 的首次与连续用例。

## 验证

- 自动化（诊断构建）：`cargo test --workspace` passed（sayall-windows 458 passed / 16 ignored，sayall-windows-app 173 passed / 1 ignored，含差异形状与捕获状态机）、前端 544 passed / 10 skipped、`vue-tsc --noEmit`、`vite build`、`ci-preflight.ps1` 7/7。
- 诊断测试包（2026-10-25，代码提交 `5d38568`）：`无线麦 SayAll_1.0.0_x64-setup.exe`，SHA-256 `2b36a6f6378b6133325d5dd39ecfcd273db19de989ebcf510c726382764bbedc`，`.sig` 已生成（本机测试密钥，不可发布）；`verify-local-test-install.ps1 -ExpectRevision` passed，内嵌修订号 `5d385686` 命中，安装版启动正常。该包只加日志，已用它复现并取得上面的差异形状。
- 界面：历史标题「全部记录」已与页头「回眸」左对齐，150% DPI 截图 `artifacts/ui-selftest/reflections-aligned.png`；现有 5 条真实记录与图标正常显示。
- 修复包（2026-10-25，代码提交 `c1ac789`）：`无线麦 SayAll_1.0.0_x64-setup.exe`，SHA-256 `a6a94df2f0815fc4ba5e336fdc7d21e014b56394576ce0cf0fd23cbc5c13c307`，`.sig` 已生成（本机测试密钥，不可发布）；`verify-local-test-install.ps1 -ExpectRevision` passed，内嵌修订号 `c1ac789b` 命中，安装版启动正常。
- 自动化（修复）：`cargo test --workspace` passed（sayall-windows 459 passed / 16 ignored，sayall-windows-app 173 passed / 1 ignored），新增整段选区替换/整段字段替换用例与“整段选中后替换可保存”用例。
- 真机首次输入复验：**deferred**（修复包已安装，等待用户在 DimAgent / WorkBuddy 复测）。

## 隐私检查

本记录只包含时间、字符数、稳定原因、应用显示名和匿名应用键，不包含语音文字、输入框全文、窗口标题、文档名、网址、设备地址或个人路径。
