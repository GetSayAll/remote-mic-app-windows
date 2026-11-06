# 2026-11-06 腾讯电脑管家截图工具：语音条出现但不进文字框

- 发现日期：2026-11-06
- 状态：调查中（已排除按键 / 音频 / 输入法选择 / 提权差异；剩余为第三方文字控件兼容边界）
- 影响范围：无线麦 SayAll Windows 1.8.3（`source_revision=9a163dbe…`）、Windows 11
  26200、RC003（日志中 `component=rc003-helper` 生效）、腾讯电脑管家 18.3.31143.301
  的截图组件 `QMCapture64.exe`
- 功能点：RC003 语音键 → 报告层右 Alt 合成 → ATVV 音频 → CABLE Output →
  豆包输入法转写 → 目标应用文字上屏；回眸「仅录音」条目
- 现象：在电脑管家截图的「文字」工具里，手动输入可用；按住遥控器语音键时语音条
  出现，但松开后没有文字进入截图文字框。回眸只生成 `QMCapture64` 的「仅录音」条目。
- 正常预期：语音条结束后，输入法把转写文字写进当前截图文字框；回眸同时生成文字记录。

## 复现条件

1. 电脑管家截图工具已打开，选择「文字」工具并把光标放进文字框。
2. SayAll 连接 RC003，输入工具选豆包，按住遥控器语音键说话后松开。
3. 观察截图文字框与回眸记录。

## 证据

### 1. 语音触发与音频链路正常

2026-11-06 17:35（本地，日志为 UTC 09:35）连续会话：

```text
voice_attempt session=77 tool=doubao path=report_layer synth=0x00E6 synth_confirmed=true
voice_effect session=77 result=replaced window_ms=500
audio_session generation=4 action=finish terminal_result=passed submitted_samples=20640
voice_audio_rate result=full rate_hz=13983 samples=20640 duration_ms=1476
```

同段 `audio_stream` 为 `virtual_cable`，`audio_pump` 正常排空；不是 2026-10-24 的
蓝牙带宽降级（本次 `rate_hz` 约 14k，满速 16k）。

### 2. 前台目标确实是 QMCapture64

```text
voice_attempt session=77 ... fg_self=false fg_pid=2492 fg_class=26fd2491 fg_hkl=0x08040804
transcript_capture action=baseline ... result=failed reason=initial_focus_unavailable last_reason=unsupported_control_type
```

`fg_class=26fd2491` 与仓库 `ime::class_token` 的 FNV-1a 算法对
`Qt51514QWindowToolSaveBits`（Qt 5.15 Tool 窗口）计算值一致（推断，不是直接读回类名）。
`unsupported_control_type` 说明回眸看到的焦点元素不是标准 `Edit` / `Document` /
`Group` 控件；截图的文字工具是 Qt 自绘文字编辑（二进制约含 `TextItem`、
`TextItemInner`、`QGraphicsTextItem` 与 `TextEditorInteraction`），不是 Win32 编辑框。

### 3. 截图工具以提权进程运行

只读探针 `OpenProcess` + `GetTokenInformation`：

```text
QMCapture64.exe（承载截图窗口的主进程，pid 2492）：Elevated=1，完整性 12288（High）
sayall-windows-app.exe（pid 37796）：Elevated=0，完整性 8192（Medium）
rc003-helper.exe（pid 28496）：Elevated=1，完整性 12288（High）
```

因此语音键能到达提权窗口（报告层合成由提权 helper 完成）。继续核对输入法侧：
豆包 `ImeService.exe`（pid 39480，`OimeVoiceWaveWindow` / `OimeDirectUIWindow` 的宿主）
也是 `Elevated=1`、完整性 12288（High），与目标同级；其语音窗
`WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST`，不会主动抢焦点。提权差异与
语音窗抢焦点两条均暂被排除。

### 4. 音频不是完全静音

回眸原始录音只做电平统计（未转写、未回放内容）：2026-11-06 的 QMCapture64 录音
时长 1.26–2.37 s，多数峰值达到满幅（有削波），RMS 约 830–1338（16-bit 满幅
32768）；最后两次峰值仅 885 / 1768，明显更轻。至少一次会话包含可用语音能量，
但不能仅凭电平判断豆包一定拿到可识别语音。

### 5. 用户侧对照：手动中文可以上屏，活动输入法是豆包

2026-11-06 用户确认：同一个截图文字框里，手动用输入法打中文可以正常上屏；截图
文字框聚焦时任务栏/输入法状态条显示的是**豆包输入法**。这排除了：

- 「截图工具完全不支持输入法」；
- 「目标窗口实际不是豆包，语音条是别的输入法拉的」；
- 「Qt 文字控件没有可编辑的 text store」。

因此焦点、输入法选择、手动 TSF/IME 组合路径都成立，失败只发生在豆包把**语音识别
结果写入该 Qt 文字控件**这一步。

### 6. 复现仍在，且与音频满速同时出现

2026-11-06 18:54 本地（日志 UTC 10:54）再次复现：`fg_pid=2492`、
`fg_class=26fd2491`，`voice_effect session=80 result=replaced`，
`voice_audio_rate result=full rate_hz=14424`，同段仍是
`transcript_capture ... last_reason=unsupported_control_type`。

## 根因

**已把范围缩到「豆包语音结果写入 Qt 文字控件」这一步，尚未拿到输入法侧的直接证据。**

已排除：

- SayAll 的按键/音频链路：报告层右 Alt、语音条、满速音频、前台进程均正常；
- 输入法选择：用户确认截图文字框的活动输入法是豆包；
- 截图工具不支持输入法：手动中文可正常上屏；
- 提权差异：QMCapture64、豆包 ImeService、RC003 helper 均为 High（12288），
  SayAll 为 Medium；同级进程之间不存在 UIPI 拦截；
- 语音窗抢焦点：`OimeVoiceWaveWindow` 带 `WS_EX_NOACTIVATE`。

剩余假设（按可能性排序）：

1. **豆包语音结果使用 TSF/直接插入路径，而截图工具的 Qt 文字项只完整支持手动
   组合/键盘路径**：手动中文走组合提交，语音结果走直接文本插入，Qt 的
   `QGraphicsTextItem` text store 可能未实现/未正确转发后者。
2. **豆包自身对「提权目标」的语音结果投递有额外限制**：ImeService 虽为 High，
   但语音结果可能经过 Medium 的 UI 子进程或另一条 IPC；这条尚未证明。

`ime_query active_is_target=true` 仍是 SayAll 自己 STA 线程上的 `GetActiveProfile`
读数，属于已知假阳性；本条结论不依赖它。

## 修复

- 应用名（与本条语音问题独立）：`QMCapture64` 的 exe 版本信息 `FileDescription`
  是「腾讯电脑管家截图工具」，但 `SHGetFileInfoW(SHGFI_DISPLAYNAME)` 只返回文件名。
  已在 `foreground::application_display_name` 增加映射，并在 `reflections-view` 增加
  旧记录别名与英文翻译；稳定键仍用 `QMCapture64-…`，不拆已有记录。
- 语音问题尚未改代码：手动中文已证明目标控件支持输入法，失败点落在豆包语音结果的
  写入路径；SayAll 既不接收也不持有转写文字，无法在这一侧做「兜底上屏」。下一步
  应做「提权记事本」对照（标准 RichEdit 控件，排除 Qt 因素）和「微信输入法」对照
  （换一条输入法的写入路径），再决定是否属于可修复范围。

## 验证

- 应用名映射：`cargo test -p sayall-windows --lib known_executables_use_product_names`
  passed；`vitest run src/pages/reflections-view.test.ts` 7 passed；全前端
  `vitest run` 623 passed / 10 skipped；`npm run build` passed。
- 语音触发与音频：2026-11-06 17:35 与 18:54 两次会话均 `voice_effect=replaced`、
  `voice_audio_rate=full`、前台 `QMCapture64`；语音条出现（用户观察）。
- 手动中文输入：用户确认在同一个截图文字框可上屏（passed，用户侧观察）。
- 活动输入法：用户确认截图文字框为豆包（passed，用户侧观察）。
- 豆包 ImeService 与语音窗：只读探针读到 High 完整性、`WS_EX_NOACTIVATE`
  （passed，本机探针）。
- **deferred**：提权记事本（标准 RichEdit）语音对照——已打开提权记事本，但未在
  窗口内完成一次语音会话，日志无 `fg_pid=36052` 记录；该对照用于最终区分
  「Qt 文字项」与「提权目标」。
- **deferred**：换微信输入法在同一截图文字框复测，用于判断是否豆包专属写入路径。

## 隐私检查

未包含语音内容、窗口标题、文档名、设备地址、用户名或个人路径；只保留进程名、
窗口类哈希、会话号、采样率与电平统计。
