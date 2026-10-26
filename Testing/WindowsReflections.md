# 回眸（Windows 版）真机测试手册

适用版本：2026-10-25 起的「回眸」文字版与原始录音。参考 Mac「回眸」行为，但界面与平台实现是 Windows 版本。

## 测试前准备

1. 用本分支构建的本地测试包安装；安装前先正常退出正在运行的安装版。
2. 准备 RC001 与 RC003 各一支，两支分别完成一轮捕获用例，不能互相替代。
3. 至少准备两个可接收语音输入的目标应用：记事本（UIA 可读）与一个不暴露 UIA 文本的应用（如微信 4.x 聊天主窗口）。
4. 准备一个密码框或搜索框页面，用于隐私过滤用例。
5. 记录安装前的 `%LOCALAPPDATA%\SayAll\transcripts` 是否存在及其内容；测试结束后不要删除该目录。
6. 运行 `pnpm test`、`pnpm build`、`cargo fmt --all -- --check`、`cargo test --workspace`、`cargo check -p sayall-windows-app --features runtime-simulation` 作为自动化前置证据。

## 用例一：入口、默认关闭与空态

1. 打开应用，查看侧栏顺序：推荐 → 回眸 → 权限。
2. 进入「回眸」。
3. 在全新状态目录下查看页面。
4. 打开个人中心，选中一个有统计的日期，分别点「在回眸中查看」、单次语音排行一行、排行底部「查看全部回眸」。

预期：

- 侧栏显示「回眸」；英文界面显示 `Reflections`。
- 个人中心的三个回眸入口都能进入本页，不改变记录或开关。
- 页头标题「回眸」，右上「记录回眸」与「保存原始录音」开关默认都关闭；「保存原始录音」在「记录回眸」关闭时不可操作。
- 空态显示「开启后，新的语音转文字结果会保存在本机。已有记录不会因关闭开关而删除。」。
- 隐私说明为「文字和原始录音仅在对应开关开启时保存在本机；不保存输入框全文、窗口标题、文档名或网址。」与「删除内容会移到回收站，可手动恢复。」两行。
- 页面不出现版本号、分享入口或统计页内容。
- 浅色/深色下文字、开关、空态图标清晰；默认窗口与窄窗口不横向溢出。

## 用例二：开启后捕获真实新增文字（RC001 与 RC003 分别执行）

1. 打开记事本，输入一行原文（例如 `原有内容`），把光标放在末尾。
2. 回到回眸页开启「记录回眸」。
3. 回到记事本，按住遥控器语音键说一句与原文不同的短句，松开。
4. 等待文字上屏并稳定约 1 秒。
5. 回到回眸页。

预期：

- 页面出现今天的日期分组与一条记录。
- 记录文字只包含语音新写入的部分，不包含 `原有内容`。
- 时间、应用名称「记事本」、真实记事本图标正确。
- 「全部应用」与「记事本」应用栏都出现该记录。
- 关闭再开启应用后记录仍在。
- RC001 与 RC003 分别重复，结果一致。

## 用例三：连续语音与快速按键

1. 在记事本中连续完成两段语音输入，中间不离开输入框。
2. 再快速按下/释放语音键一次，不说话或只说很短的内容。

预期：

- 两段文字分别形成记录或按连续新增规则正确结算，不重复、不丢失。
- 快速按键不产生空记录。
- 日志中每次 `begin` / `baseline` / `finish` / `save` 或 `cancel` 成对，`generation` 不倒退。

## 用例四：焦点与应用切换

1. 开始语音后立即切换到另一个应用再松开。
2. 开始语音后在原输入框外的窗口点击，再松开。
3. 关闭「记录回眸」后立即进行一次语音输入。

预期：

- 目标变化或焦点丢失的候选被放弃；页面不出现跨应用错配记录。
- 关闭开关后不再新增记录；已有记录保留。

## 用例五：隐私过滤

1. 在登录页密码框、应用内搜索框、浏览器地址栏分别开始语音并输入。
2. 在记事本设置类只读区域尝试同样操作。

预期：

- 这些位置都不产生回眸记录，也不弹窗、不提示技术要求。
- 日志只出现稳定原因（如 `password_field` / `unsupported_control_type` / `unsafe_semantics` / `self_foreground_window` / `self_focused_element` / `destination_changed` / `baseline ... result=waiting`），不包含输入内容、窗口标题或路径。

## 用例六：复制与删除

1. 点击一条记录右侧复制按钮，粘贴到记事本。
2. 点击删除按钮，在确认框点「取消」。
3. 再次删除并确认。
4. 选中某个应用，点「删除当前应用」并确认。
5. 点页面底部「全部删除」并确认。

预期：

- 复制后按钮短暂显示对勾，剪贴板内容与记录文字完全一致。
- 取消不删除；确认后记录消失，页面立即刷新。
- 删除单条后，原日文件进入 Windows 回收站；可从回收站恢复。
- 删除当前应用后，该应用目录进入回收站；其他应用记录保留。
- 全部删除后整个 `v1` 根目录进入回收站；页面回到空态，开关状态不变。
- 任一删除失败时提示「删除失败，记录仍保留。请稍后重试。」，原记录仍可见。

## 用例六 A：原始录音保存、播放、导出与删除（RC001 与 RC003 分别执行）

1. 在「记录回眸」开启的前提下打开「保存原始录音」。
2. 回到记事本，按住语音键说一句短句后松开，等待文字上屏。
3. 回到回眸页，检查该条记录的操作按钮与 `%LOCALAPPDATA%\SayAll\transcripts\v1\audio\<sessionId>`。
4. 点「播放录音」，听完或中途点「停止播放」。
5. 点「导出录音」，在文件对话框选择路径并保存；用系统播放器打开导出的文件。
6. 点该条唯一的「删除这条记录」并确认，检查文字记录与录音目录一起进入回收站。

预期：

- 录音目录包含 `original.wav` 与 `manifest.json`；manifest 使用 camelCase 字段，记录 `format=wav-pcm-s16le`、`sampleRate=16000`、`channels=1`、非零时长与字节数，且 `byteCount` 与实际文件大小一致；新录音还记录 `applicationKey` 与 `applicationName`。
- 页面出现「播放录音 / 停止播放」「导出录音」，每条记录只有一个「删除这条记录」按钮；播放能听到本次语音的原声，自然结束或点「停止播放」后按钮恢复，且不出现「无法播放这段录音，请稍后重试。」。
- 导出的 WAV 可被系统播放器正常播放，内容与页面播放一致。
- 删除这条记录后，页面不再显示该记录，播放/导出按钮消失，关联录音目录进入回收站；已经输入到目标应用中的文字不受影响。
- 只开启「记录回眸」、不开启「保存原始录音」时，不产生新的录音目录或 `original.wav`。
- RC001 与 RC003 分别重复，结果一致；快速按一下不说话不产生可播放录音。

## 用例六 B：发送后立即清空与页面静默刷新

1. 在「记录回眸」开启时，进入一个聊天输入框（如 DimAgent / WorkBuddy），保留一段旧草稿。
2. 按住语音键说一句短句，松开后立刻按 Enter/发送（不要等 900 ms 稳定窗口）。
3. 回到回眸页，查看这条记录的文字与录音。
4. 停留在回眸页，再完成一次语音输入，观察页面刷新过程。

预期：

- 记录文字是本次说出的短句，不是「发送消息...」「Message...」等输入框占位文案。
- 原始录音可播放，内容与文字对应；不改写旧草稿之外的内容。
- 日志出现 `transcript_capture action=change ... result=cleared reason=placeholder_or_empty` 或 `reason=returned_to_baseline`，随后 `action=save ... reason=cleared_after_candidate`；`chars` 为实际听写长度。
- 页面刷新时已显示记录不消失、不出现整页「正在读取回眸记录…」加载卡；标题栏开关不重新滑动。
- 切回回眸页时，本机读取正常情况下也不闪加载卡；只有磁盘/UIA 读取明显变慢（>150 ms）时才出现加载卡。
- 空输入框发送、草稿被整段替换、快速连续语音都不保存占位文案；没有有效候选时日志为 `action=cancel ... reason=cleared_before_candidate`。

## 用例六 C：文字捕获失败时的「仅录音」条目（RC001 与 RC003 分别执行）

1. 在「记录回眸」与「保存原始录音」都开启时，打开 WorkBuddy 或微信 4.x 的聊天输入框。
2. 按住语音键说一句短句后松开，等待语音会话结束（不需要出现文字）。
3. 回到回眸页，查看「仅录音」分组、目标应用名与录音操作。
4. 点「播放录音」，听完或中途点「停止播放」；再点「导出录音」并打开导出的文件。
5. 在应用栏选中该应用，确认只显示该应用的文字与仅录音条目；再选「全部应用」确认最近 7 天范围。
6. 点该条录音的「删除录音」并确认，检查该会话的录音目录进入回收站、文字记录与其他录音不受影响。
7. 对旧版本留下的、manifest 没有应用身份的录音重复查看与播放。

预期：

- WorkBuddy / 微信即使没有文字记录，录音仍以「仅录音」条目出现，按本机日期分组，最新一组默认展开；页面不出现「还没有语音记录」空态。
- 停留在回眸页完成上述语音后，录音落盘即触发静默刷新，新「仅录音」条目自动出现，不闪整页加载卡；不需要切换页面或重启应用。
- 新录音显示目标应用真实名称与图标（取不到图标时显示通用图标）；微信显示「微信」（英文 `WeChat`），不显示 `Weixin`；旧录音归入「未知应用」，仍可播放、导出、删除。
- 录音可播放，内容与本次说话一致；导出文件可被系统播放器打开；自然结束或停止后按钮恢复，不出现「无法播放这段录音，请稍后重试。」。
- 「删除录音」只移动当前会话的录音目录，文字记录和其他录音保留；日志记录 `result=passed`，不含路径或音频内容。
- 选中该应用时显示该应用的全部历史；「全部应用」只显示最近 7 天；应用计数包含仅录音条目。
- 删除当前应用时，该应用的文字记录、关联录音与仅录音条目一起进入回收站；全部删除后 `v1` 根目录进入回收站。

## 用例七：时间范围、折叠与图标

1. 预置或保留一条 7 天以前的记录（可通过系统日期或测试数据准备）。
2. 在「全部应用」查看。
3. 选中该应用查看。
4. 折叠/展开日期分组，展开/收起应用栏。
5. 选一个没有图标缓存的应用，或临时移走 `<root>\icons\<applicationKey>.png`。

预期：

- 「全部应用」只显示最近 7 天；更早记录提示「最近 7 天没有记录。请在应用栏选择具体应用查看更早记录。」。
- 选中具体应用后显示其全部历史。
- 日期倒序，默认展开最新一天；折叠状态在本次页面会话内保持。
- 应用栏单行时可横向浏览，展开后为自适应网格。
- 有缓存时显示真实应用图标；缺失时显示通用图标，不破图、不阻塞页面。

## 用例八：重启、升级与卸载保留

1. 记录若干条后正常退出并重启应用。
2. 覆盖安装同一分支的新包（正常退出安装版，不强制结束进程）。
3. 卸载应用，再重新安装并启动。

预期：

- 重启后记录、开关状态、应用图标都保留。
- 升级后记录保留，旧配置能正常迁移。
- 卸载不会删除 `%LOCALAPPDATA%\SayAll\transcripts`；重装后仍能看到原记录和开关状态（开关关闭时只停止新增）。
- 回收站中能看到删除产生的文件，可恢复。

## 用例九：读取与写入失败

1. 临时把 `%LOCALAPPDATA%\SayAll\transcripts\v1` 设为不可读，打开回眸页。
2. 恢复权限后点「重试」。
3. 临时把 `icons` 目录设为不可写，完成一次语音输入。

预期：

- 读取失败显示「读取回眸记录失败：…」与「重试」，页面不崩溃。
- 恢复后重试成功，记录正常显示。
- 图标写入失败不阻止记录保存；日志出现 `transcript_archive action=write_icon phase=completed result=failed reason=icon_cache_failed`，页面使用通用图标。
- 临时把 `audio` 目录设为不可写后开启录音并完成一次语音：文字记录仍保存，页面不出现播放按钮；日志出现 `transcript_recording action=start ... result=failed reason=open_failed`。
- 删除或移走某条录音的 `original.wav` 后点播放：显示「无法播放这段录音，请稍后重试。」，页面不崩溃；不自动删除文字记录。
- 正常播放到自然结束：不显示「无法播放这段录音，请稍后重试。」，播放按钮恢复为「播放录音」。
- 不出现永久删除、覆盖原记录或静默清空。

## 日志与证据

诊断日志位置以 `LOGGING.md` 为准。回眸相关稳定字段：

- `transcript_capture action=begin|baseline|activate|finish|save|cancel`
- `result=passed|skipped|waiting|failed|dropped|retry|cleared`
- `reason=feature_disabled|initial_focus_unavailable|destination_changed|discontinuous_text_change|placeholder_or_empty|returned_to_baseline|cleared_after_candidate|cleared_before_candidate|timeout|transcript_too_long|...`
- 基线失败细分：`self_foreground_window|self_focused_element|password_field|target_disabled|unsupported_control_type|unsafe_semantics|no_focus|no_text_pattern|read_only|uia_timeout`
- 重试原因变化：`result=waiting reason=<新原因> changed=true`；最终失败额外带 `last_reason=<最后原因>`。
- `generation=<n>`、`chars=<n>`、`duration_ms=<n>`
- `transcript_archive action=append|write_icon|delete_record|delete_application|delete_all`
- `transcript_archive action=read_audio|export_audio|delete_recording`
- `transcript_archive action=resolve_root ... result=fallback reason=localappdata_unavailable`
- `transcript_recording action=start|append|finish|cancel phase=completed result=passed|failed reason=... duration_ms=<n> bytes=<n>`
- `transcript_settings action=save|save_audio phase=completed result=passed enabled=<bool>`

隐私红线：日志不得出现记录文字、输入框全文、窗口标题、文档名、网址、设备地址、进程完整路径、录音文件路径或音频字节。若发现任一项，立即停止验收并记录 Bug。

## 结果记录模板

| 用例 | RC001 | RC003 | 证据（日志行/截图/回收站） | 结论 |
| --- | --- | --- | --- | --- |
| 一 入口与空态 |  |  |  |  |
| 二 真实捕获 |  |  |  |  |
| 三 连续与快速按键 |  |  |  |  |
| 四 焦点与开关 |  |  |  |  |
| 五 隐私过滤 |  |  |  |  |
| 六 B 发送后清空/静默刷新 |  |  |  |  |
| 六 复制与删除 |  |  |  |  |
| 六 A 原始录音 |  |  |  |  |
| 六 C 仅录音条目 |  |  |  |  |
| 七 时间范围与图标 |  |  |  |  |
| 八 重启升级卸载 |  |  |  |  |
| 九 失败路径 |  |  |  |  |

结论只能写 `passed` / `failed` / `deferred`。未执行真机时写 `deferred`，不得用自动化或仿真结果代替。

## 2026-10-25 本机安装版验证记录

- 测试包：`无线麦 SayAll_1.0.0_x64-setup.exe`，SHA-256 `56560ded7cf3144d4b3b3887f6714c2e4a7592dd7c9de96efaed73cc5ff0ac3a`，代码提交 `44ce5f6`。
- 自动化：前端 544 passed / 10 skipped，`vue-tsc --noEmit`、`vite build`、`cargo test --workspace`、`cargo check -p sayall-windows-app --features runtime-simulation`、`ci-preflight.ps1` 7/7 passed。
- 安装/升级：`verify-local-test-install.ps1 -ExpectRevision` passed（helper/gadget 哈希、app exe 差 3 字节、内嵌修订号 `44ce5f6c`、启动窗口）。
- 卸载/重装：静默卸载后 `%LOCALAPPDATA%\SayAll\transcripts\v1` 的标记文件与设置保留；重装并启动后回眸页正常。
- 页面与交互 passed：侧栏顺序「推荐 → 回眸 → 权限」；默认关闭空态；开启后空态；开关持久化（schema 6）与重启保留；关闭开关保留已有记录。
- 预置数据页面验证 passed：真实记事本图标、应用名称、日期分组、全部应用最近 7 天（2 条）、单应用全部历史（3 条，含 7 天前）、复制内容逐字一致、删除单条后原日文件进入回收站、全部删除后 `v1` 目录进入回收站、关闭开关不删除记录。
- 说明：预置记录仅用于页面与回收站验证，不是真实 UIA 捕获证据；验证后已全部删除，测试标记文件移入回收站，开关恢复为关闭。
- **deferred**：RC001 与 RC003 的真实 UIA 捕获、连续语音、快速按键、焦点切换、密码/搜索/地址栏过滤、真实图标随捕获落盘；需按本手册用例二至七真机执行。

## 2026-10-25 原始录音本机安装版验证记录（E708）

- 测试包：`无线麦 SayAll_1.0.0_x64-setup.exe`，SHA-256 `e708e890171c52f2d4a65beb2eff395d78c51f790a17ec16c0a1b19d15fca274`，工作树 HEAD `1b2dffb`（含未提交修改），本机测试密钥签名，**不可发布**。
- 自动化：`pnpm test` 549 passed / 10 skipped、`pnpm build`、`cargo fmt --all -- --check`、`cargo test --workspace`（sayall-windows 464 passed / 16 ignored，sayall-windows-app 175 passed / 1 ignored）、`cargo check -p sayall-windows-app --features runtime-simulation`、`ci-preflight.ps1` 7/7、runtime simulation 26 steps passed。
- 安装：`verify-frontend-embedded.ps1` passed（16 个资源嵌入）；静默安装后 `verify-local-test-install.ps1 -ExpectRevision -NoLaunch` passed（哈希对齐、app exe 差 3 字节、内嵌修订号 `1b2dffbf`）。
- 页面与开关 passed：页头出现「保存原始录音」，默认关闭；「记录回眸」关闭时录音开关禁用；开启后 `%APPDATA%\app.getsayall.remote-mic.windows\settings.json` 写入 schema 7，`local_transcript_history_enabled` 与 `local_original_audio_recording_enabled` 读回 true；恢复关闭后两者均读回 false。
- 页面音频操作 passed（测试数据）：本地生成 1 秒 16 kHz WAV 与 camelCase manifest，挂到已有文字的 `sessionId`；页面出现播放/导出/删除按钮，播放日志 `transcript_archive action=read_audio ... result=passed bytes=32044`，1 秒后恢复播放按钮；导出对话框可打开，取消后日志 `result=cancelled`；删除录音后目录消失、文字记录仍在，日志 `result=passed removed=true`。
- **deferred**：以上 WAV 是测试数据，不是真实遥控器录音。真实 BLE PCM 写入 `audio/`、真实播放与导出、录音中关闭开关/断连/退出清理，仍需 RC001 与 RC003 分别按用例六 A 验收。runtime simulation 的直接 pipeline 语音会话不经过 BLE 录音接入，不能作为本项证据。

## 2026-10-25 22:19 录音修复包（用户报告）

- 现象：开关已打开，但回眸记录没有录音操作。日志显示旧包每轮只有 `transcript_recording action=start`，下一次会话开始前出现 `cancel ... bytes=0`，没有 `append` / `finish`。
- 处理：`cargo clean -p sayall-windows` 强制重建；录音收尾从 `finish_session` 移到 BLE `StreamStopped` 的 `finish_audio` 显式调用；新增 `activate`、`cancel requested was_active`、首次 `append`（passed/inactive）与 `finish requested/skipped` 探针。
- 测试包：SHA-256 `266e73af1ef73b7bbc0eed112d728641f42e8b09422e31517b0daf5bcc4ce6af`，已静默安装并启动；`verify-local-test-install.ps1 -ExpectRevision -NoLaunch` passed；二进制已确认包含 `append phase=requested` 与 `finish phase=requested` 探针。
- **真实录音复验 pending**：需用 RC001 或 RC003 再做一次语音输入；日志应出现 `append ... samples=...`、`finish phase=requested`、`finish phase=completed ... duration_ms=...`，且 `audio/<sessionId>/original.wav` 与 `manifest.json` 存在。未完成前不得标记为 passed。

## 2026-10-26 播放收尾与合并删除修复包（用户报告）

- 现象一：真实录音可以播放，但自然播放结束后页面显示「无法播放这段录音，请稍后重试。」。根因：`stopPlayback` 清空 `audio.src` 会触发 WebView2 的 `error`，旧错误回调无条件写提示。修复：停播前先解绑 `onended` / `onerror`，并用 `playbackAttempt` 丢弃过期读取/播放回调。
- 现象二：同一行有「删除录音」和「删除这条记录」两个删除入口。修复：每条记录只保留「删除这条记录」；后端删除单条记录本来就会级联删除关联录音，确认文案改为「记录和关联的录音将一起移到回收站，不会影响已经输入到目标应用中的文字。」。
- 测试包：`无线麦 SayAll_1.0.0_x64-setup.exe`，SHA-256 `510765ffe4e97c65dcb2a9c98200290b7982866270bd5d651746fd8af78d741a`，工作树 HEAD `1b2dffb`（含未提交修改），本机测试密钥签名，**不可发布**；本包只含 x64 两件，不含 arm64 载荷。
- 自动化：前端 551 passed / 10 skipped（含新增「自然结束不误报」与「单按钮删除文字+录音」回归）、`vue-tsc --noEmit`、`vite build`（16 个前端资源嵌入）；Rust 侧沿用同日录音修复后的 `cargo test --workspace` 全量结果（sayall-windows 465 passed / 16 ignored，sayall-windows-app 175 passed / 1 ignored）。
- 安装：`verify-local-test-install.ps1 -ExpectRevision` 全部 PASS（helper/gadget 哈希对齐、app exe 差 3 字节、内嵌修订号 `1b2dffbf`、启动窗口）。
- 真实录音 passed（本机真机，型号未单独核对）：`audio/<sessionId>` 出现 3 条真实录音，`original.wav` 分别为 121964 / 287084 / 370604 字节；日志 `transcript_recording action=finish ... result=passed` 与 `transcript_archive action=read_audio ... result=passed` 均命中。
- 播放收尾 passed：安装新包后播放 370604 字节录音至自然结束，`停止播放` 按钮恢复为 `播放录音`，页面精确错误文案「无法播放这段录音，请稍后重试。」数量为 0；再播放 287084 字节录音并手动停止，同样无误报。
- 合并删除 passed：页面每行只有「播放录音 / 导出录音 / 复制 / 删除这条记录」，「删除录音」按钮数量为 0；确认框显示合并删除文案。删除 18:56 的测试条目后，记录行、文字 JSON 项与该会话的 `audio/<sessionId>` 目录同时消失，页面记录数 17→16、可播放录音数 3→2；回收站中可看到该录音目录与日文件，来源分别为 `...\transcripts\v1\audio` 与 `...\transcripts\v1\DimAgent-...`（被删测试句为「将两个删除按钮合并成一个，删除的时候需要将文字和录音都删掉」，可从回收站恢复）。
- **deferred**：RC001 与 RC003 分型号验收；导出、回收站恢复操作、长录音与断连/退出清理仍未在本包逐项复验。

## 2026-10-26 页面刷新闪屏与发送后占位文案修复包（用户报告）

- 现象一：回眸页每次收到新捕获记录都会先显示整页「正在读取回眸记录…」再恢复，切换页面重新挂载时也会闪一下。
- 现象二：聊天输入框发送后清空，记录文字变成「发送消息...」，但原始录音正确。只读 UIA 探针确认 DimAgent 的 ProseMirror 输入框无 `ValuePattern`，空态时 `TextPattern.DocumentRange.GetText(-1)` 返回 `发送消息...\n`；旧状态机把它当作整字段替换候选，覆盖了真实听写候选。
- 修复：`loadHistory` 首次读取延迟 150 ms 才显示加载卡，后续刷新静默完成并加 `loadAttempt` 丢弃过期响应；新增 `is_placeholder_text` 与 `cleared_reads`，输入框变空/占位或回到基线时连续两次确认，保存最后一次非空候选（`reason=cleared_after_candidate`），不再保存占位文案。
- 测试包：`无线麦 SayAll_1.0.0_x64-setup.exe`，SHA-256 `a681592dea603db82e208a65dedbc9783e51fac598ce709834e74a145a159dee`，工作树 HEAD `1b2dffb`（含未提交修改），本机测试密钥签名，**不可发布**；仅 x64 载荷。
- 自动化：前端 553 passed / 10 skipped（ReflectionsPage 17，含首次/后续刷新无加载卡、占位文案保留最后候选回归）；`vue-tsc --noEmit`；`cargo test --workspace`（sayall-windows 467 passed / 16 ignored，sayall-windows-app 175 passed / 1 ignored，sayall-core 14）；`cargo fmt --all -- --check`；`vite build`（16 个前端资源嵌入）。
- 安装：`verify-local-test-install.ps1 -ExpectRevision` 全部 PASS（helper/gadget 哈希对齐、app exe 差 3 字节、内嵌修订号 `1b2dffbf`、启动窗口）。
- 安装版页面切换实测（非语音刷新）：UIA 轮询 2.5 秒内「正在读取回眸记录…」出现 0 次，回眸内容可见；说明快速首次读取不再显示加载卡。
- **真实硬件复验 pending**：需在聊天输入框保留草稿、按住语音键说一句短句后立即按 Enter 发送；记录文字应为实际短句（不是「发送消息...」），日志应出现 `result=cleared reason=placeholder_or_empty` 或 `returned_to_baseline`，随后 `save reason=cleared_after_candidate`。同时在回眸页完成一次语音输入，确认刷新不闪加载卡、开关不重新滑动。旧记录「发送消息...」不会自动改写。

## 2026-10-26 回眸「仅录音」条目本机安装版验证记录

- 背景：用户报告 WorkBuddy 和微信拿不到录音文件；Mac 端拿不到文字时仍保留原始录音。Windows 原先把录音只挂到匹配 `sessionId` 的文字记录，导致文字捕获失败（UIA 不可读）时 WAV 留在磁盘但不显示。
- 测试包：`无线麦 SayAll_1.0.0_x64-setup.exe`，SHA-256 `9bde4bebeaa1fdba40a9fb2c138c683d644b8dc92125b54f78513aef5e75411c`，工作树 HEAD `1b2dffb`（含未提交修改），本机测试密钥签名，**不可发布**；仅 x64 载荷。
- 自动化 passed：前端 555 passed / 10 skipped（`ReflectionsPage` 19，含仅录音跨日期默认展开、录音落盘事件静默刷新、播放/导出/删除）；`vue-tsc --noEmit`；`pnpm build`；`cargo fmt --all -- --check`；`cargo test --workspace`（sayall-windows 469 passed / 16 ignored；sayall-windows-app 177 passed / 1 ignored；含仅录音快照、应用汇总、删除应用联动、录音双开关判定、落盘回调）；`cargo check -p sayall-windows-app --features runtime-simulation`。
- 安装 passed：`verify-local-test-install.ps1 -ExpectRevision` 全部 PASS（helper/gadget 哈希对齐、app exe 差 3 字节、内嵌修订号 `1b2dffbf`、启动窗口）；最终包重装后 UIA 复核仍为「全部应用 34」「未知应用 9」。
- 刷新事件：录音落盘回调 `transcript-recording-finished` 由单元测试验证（`reports_successful_recording_finish_with_session_id`）；页面订阅该事件的静默刷新由前端回归验证。真实新录音能否在页面即时出现仍需下面的真机语音验证。
- 旧孤儿录音页面 passed：安装版「全部应用」显示 34 条（25 条文字 + 9 条仅录音），应用栏出现「未知应用 9」；「仅录音」分组下 `2026年10月26日 · 9 段录音`，19:36 的 45164 字节微信旧录音与 18:38 的 71084 字节 WorkBuddy 旧录音均在列表中。选中「未知应用」后页面显示 9 个播放、9 个导出、9 个「删除录音」按钮，`仅录音` 标题 1 个；切回「全部应用」后播放/导出按钮 17 个（8 条文字记录关联录音 + 9 条仅录音），「删除录音」仍为 9 个。
- 播放 passed：从页面播放 18:38 的 71084 字节旧录音，日志 `transcript_archive action=read_audio phase=completed result=passed bytes=71084`；自然结束后按钮恢复为「播放录音」，无「无法播放这段录音，请稍后重试。」。
- 导出 passed（对话框）：对仅录音条目打开导出对话框，取消后日志 `transcript_archive action=export_audio phase=completed result=cancelled`；未写入测试文件。
- 删除边界：页面为 9 条仅录音各提供「删除录音」按钮；为避免误删用户真实录音，本包未对真实条目执行删除点击，删除存储与 API 由自动化测试覆盖。
- 截图：`artifacts/ui-selftest/reflections-recording-only.png`（「仅录音」分组与未知应用旧录音）。
- **deferred**：新录音写入真实应用身份（`applicationKey` / `applicationName`）需用户用 RC001 或 RC003 在 WorkBuddy/微信各做一次语音后检查 manifest 与页面名称；旧manifest没有身份，统一显示「未知应用」。RC001 与 RC003 分型号验收仍按用例六 C 执行。

## 2026-10-26 回眸应用名与图标修复（用户报告）

- 现象：微信「仅录音」显示 `Weixin`；微信和 WorkBuddy 显示通用文档图标，不是各自应用图标。
- 根因：微信 exe 的版本资源 `ProductName`/`FileDescription` 都是 `Weixin`，回眸身份没有使用已有的微信产品名映射；图标缓存只在文字记录成功保存时写入，文字捕获失败的仅录音应用没有缓存。
- 修复：微信/WeChat 显示名统一为「微信」（稳定键仍用原始名，避免拆应用）；旧 manifest 名称由前端别名；图标缺失时按显示名只读扫描当前进程，用公开 Shell 图标补缓存。
- 测试包：`无线麦 SayAll_1.0.0_x64-setup.exe`，SHA-256 `041ec05f9df555f72e72d7a8c242c9eb48b1814bd7713df3b36069470dd296ee`，工作树 HEAD `0fad4d99`，本机测试密钥签名，**不可发布**；仅 x64 载荷。
- 自动化 passed：前端 557 passed / 10 skipped；`vue-tsc --noEmit`；`pnpm build`；`cargo fmt --all -- --check`；`cargo test --workspace`（sayall-windows 470 passed / 16 ignored；sayall-windows-app 177 passed / 1 ignored）。
- 安装 passed：`verify-local-test-install.ps1 -ExpectRevision` 全部 PASS（helper/gadget 哈希对齐、app exe 差 3 字节、内嵌修订号 `0fad4d99`、启动窗口）。
- 页面 passed：图标缓存生成 `Weixin-7e04e5c6931ccef3.png` 与 `WorkBuddy-8e9eab770187d406.png`；筛选「微信」显示中文名与微信绿色图标，筛选 `WorkBuddy` 显示 WorkBuddy 图标；截图 `artifacts/ui-selftest/reflections-wechat-filtered.png`、`artifacts/ui-selftest/reflections-workbuddy-filtered.png`。
- **deferred**：用 RC001/RC003 在微信/WorkBuddy 各做一次新语音，确认新 manifest 的 `applicationName` 直接写「微信」；旧 manifest 的 `Weixin` 名称不会改写，只做界面别名。

