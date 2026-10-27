# 回眸：无线麦自身与微信文字不可捕获的原因与替代路线（2026-10-25）

- 状态：调研完成；已实现日志 reason 拆分，未改变捕获行为。
- 范围：解释 `self_foreground` 与微信 `unsafe_destination` 两个失败路径；评估 UIA、MSAA、剪贴板、TSF、截图 OCR、音频 ASR 六类路线。
- 环境：Windows 11 26200；微信 4.1.15.56；SayAll 1.0.0，`source_revision=c1ac789b`。
- 隐私：本文不包含聊天正文、用户语音内容、窗口标题、设备地址或截图文件。调研截图含聊天内容，已移入回收站，不归档。

## 1. 结论

1. **无线麦自身失败是产品性排除，不是权限或 UIA 故障。** 前台 PID 和焦点元素 PID 各有一道 `self_foreground` 守卫。日志在 2026-10-25T08:07:25 明确记录 `fg_self=true fg_pid=36216`。
2. **微信失败是应用不提供可读文本接口。** 微信 4.x 聊天主窗口是 Qt 自绘窗口。实测 UIA 树只有 `Pane`，没有 `Edit` / `Document` / `TextPattern`；`WM_GETOBJECT` 的 `OBJID_CLIENT` 和 `UiaRootObjectId` 都返回 0。拆分前的日志只给出 `unsafe_destination`；按树结构，最可能是控件类型白名单拒绝。拆分后同一场景应记录 `unsupported_control_type`。即使放开控件类型，也会在 `read_text` 因 `no_text_pattern` 失败。
3. **截图 OCR 技术上能做，但不能直接塞进现有回眸状态机。** `PrintWindow` 能截到微信内容，系统 OCR 可离线识别；但整窗 OCR 会读聊天记录，系统 OCR 对 16–24 px 中文字仍有错字、漏标点和插空格，且不提供置信度。现有状态机要求精确连续文本，OCR 噪声会直接判为不连续。
4. **截图 OCR 只能作为明确的 opt-in 兜底。** 最小可行范围是：用户框选输入框区域、语音开始时输入框为空或只有占位文字、只 OCR 该区域、连续两帧稳定后结算、截图不落盘、失败就跳过。首版不支持已有草稿的复杂 diff。
5. **音频 ASR 是唯一不依赖目标应用的路线，但它记录“用户说了什么”，不是“输入框实际新增了什么”。** 这两件事可能不同：输入框没有焦点、输入法没有上屏、用户随后修改，都会让 ASR 与回眸语义分裂。`System.Speech` 中文实测把“无线麦”识别成“无线买”，置信度 0.478，且无标点，不能作为生产方案。本地 Whisper 需要单独 spike。
6. **剪贴板、TSF 跨进程读取、注入、私有数据都不可接受。** 剪贴板会破坏用户选区、可能读到聊天列表、无法无损恢复；TSF 没有跨进程公开 API；注入和私有数据违反仓库规则。
7. **短期最合理的动作不是加 OCR，而是补日志 reason 码和明确产品边界。** 本次已拆分 reason 码；如果用户确实要支持微信，先决定是“接受不支持”还是“做 opt-in OCR 区域兜底”；两条路的产品承诺不同。

## 2. 两个失败路径

### 2.1 无线麦自身：两道排除，不是一道

`WindowsSnapshotProvider::snapshot()` 在读取焦点前先取前台窗口 PID：

- `crates/sayall-windows/src/transcript_capture_windows.rs:108`：前台 PID 等于当前进程，返回 `SnapshotError::SelfForegroundWindow`。
- 同文件 `:117`：UIA 焦点元素 PID 等于当前进程，返回 `SnapshotError::SelfFocusedElement`。
- 同文件 `:122`：应用身份使用焦点元素 PID，不是前台 PID。

`read()` 在每次轮询时重新执行 `read_focus()`（`:135-153`）。因此只放开基线、不放开读取，仍然会在轮询阶段失败。

日志证据（拆分前；只含状态和字符数，不含文字）：

```text
08:07:25.706 transcript_capture action=baseline phase=completed result=waiting generation=10 reason=self_foreground
08:07:25.728 voice_attempt session=112 fg_self=true fg_pid=36216
08:07:27.020 transcript_capture action=baseline phase=completed result=failed reason=initial_focus_unavailable generation=10
```

设计原因：

- 避免把 SayAll 自己的设置、搜索、宏编辑器、回眸列表当成用户目标。
- 避免回眸页显示历史文字，保存后又被当成“新增文字”，形成反馈环。
- 避免记录 onboarding 的测试输入，污染真实历史。
- 与输入法激活路径一致：`ime.rs:326` 在自身前台时直接 `SkippedSelfForeground`。

当前自身页面的 UIA 证据：SayAll 的 WebView2 页面根是 `Document`，`ValuePattern.IsReadOnly=true`。这说明即使删除 `self_foreground`，页面根仍会被 `read_text` 的只读检查拒绝。真正可命中的只有实际可编辑的 `<input>` / `<textarea>`。

组合动作搜索框（2026-10-25 只读 UIA 探针）：`ControlType.Edit`，`ClassName=search-input`，`AutomationId` 为空，`Name=搜索组合动作`，`ValuePattern` 非只读。当前即使放开 `self_foreground`，它仍会命中 `UNSAFE_TERMS` 的“搜索”并被拒绝为 `unsafe_semantics`。要记录它必须同时绕过搜索排除，这属于产品隐私决策，不是单纯的自捕获白名单。用户当天确认：语音文字能进入该搜索框，只是回眸不记录，因此 B1 已满足，不改 IME 自身前台路径。

### 2.2 微信：没有文本 provider，不是权限问题

实测对象：微信 4.1.15.56，主窗口隐藏时 0 个 UIA 元素；2026-10-01 的微信 4.0 可见聊天主窗口为 2 个 `Pane`。既有证据见 `docs/investigations/2026-10-01-windows-input-focus-uia-feasibility.md:31`。

| 证据 | 结果 | 含义 |
| --- | --- | --- |
| 窗口类 | `Qt51514QWindowIcon` + `MMUIRenderSubWindowHW` | Qt 5.15 自绘，不是原生 Edit 子窗口 |
| UIA 树 | 2 个 `Pane`，0 个 `Edit` / `Document` | 没有输入控件语义 |
| `TextPattern` | 0 | 没有文本流 |
| `OBJID_CLIENT` | 0 | 没有 MSAA `IAccessible` |
| `UiaRootObjectId` | 0 | 没有自定义 UIA provider |
| 日志（拆分前） | `08:06:18 baseline result=waiting reason=unsafe_destination` | 旧口径把密码/禁用/控件类型/敏感语义混在一起；按树结构最可能是控件类型 |
| 后续（拆分前） | `08:06:19 failed reason=initial_focus_unavailable` | 旧日志没有保留最后原因；拆分后额外记录 `last_reason=` |

`read_focus()` 的拒绝顺序：

1. `IsPassword` → `password_field`；`!IsEnabled` → `target_disabled`（`:177-182`）。
2. `!HasKeyboardFocus` → `no_focus`（`:183`）。
3. 控件类型不是 `Edit` / `Document` / `Group` → `unsupported_control_type`（`:186-191`）。
4. 敏感语义命中 `UNSAFE_TERMS` → `unsafe_semantics`（`:192-195`）。
5. 无 `ValuePattern` 且无 `TextPattern` → `no_text_pattern`（`:214`）。
6. `ValuePattern.IsReadOnly=true` → `read_only`（`:218-225`）。

按既有树结构，微信焦点元素是 `Pane`，最可能在第 3 条被拒。即使把 `Pane` 加入白名单，第 5 条仍会失败。**这不是“改一个判断就能读”的问题。**

同一批日志里，WorkBuddy 在 2026-10-25T08:05 捕获成功（16/17/26 字符），而 2026-10-01 调查记录 WorkBuddy 只有 2 个 `Pane`。这说明 UIA 可用性会随应用版本和环境变化，必须按当前版本实测。不能把微信的结论外推到所有 Qt 或 Electron 应用，也不能把一次“无树”当成永久结论。

旧口径把“密码/禁用”和“控件类型/敏感语义”混在 `unsafe_destination`，日志无法区分微信具体是哪一条。2026-10-25 已拆成稳定 reason 码，行为不变：

| 位置 | 拆分前 | 拆分后 |
| --- | --- | --- |
| 前台 PID 自身 | `self_foreground` | `self_foreground_window` |
| 焦点元素 PID 自身 | `self_foreground` | `self_focused_element` |
| `IsPassword` | `unsafe_destination` | `password_field` |
| `!IsEnabled` | `unsafe_destination` | `target_disabled` |
| 控件类型 | `unsafe_destination` | `unsupported_control_type` |
| 敏感语义 | `unsafe_destination` | `unsafe_semantics` |

`transcript_capture.rs` 的 `read` 分支已同步：`FocusChanged` 和四个新的不安全目标原因仍统一结算为 `destination_changed`，不改变“不记录”的行为。基线重试只在原因变化时记录一行，最终失败额外带 `last_reason=`。

## 3. 替代路线矩阵

| 路线 | 微信 4.x | 无线麦自身 | 隐私 | 实现成本 | 判定 |
| --- | --- | --- | --- | --- | --- |
| 现有 UIA（Value/TextPattern） | 否 | 可读页面根，但只读；输入框需白名单 | 高 | 已实现 | 保留；微信静默跳过 |
| MSAA / IAccessible2 / LegacyIAccessible | 否 | 否 | 高 | 低 | 不可行；微信没有 `IAccessible` |
| UIA TextPattern2 / TextEdit / TextChanged / 属性事件 | 否 | 部分 | 高 | 低 | 不可行；没有 provider 就没有事件 |
| 剪贴板 Ctrl+A/C | 技术上有概率 | 可以，但破坏性 | 低 | 中 | 不建议；选区无法恢复 |
| TSF / IME 跨进程读取 | 否 | 否 | 高 | 高 | 不可行；没有公开跨进程 API |
| 截图 OCR | 有条件可行 | 可以 | 最低 | 高 | 仅 opt-in 区域兜底 |
| 音频 ASR | 应用无关 | 应用无关 | 中高 | 高 | 唯一可选兜底；语义不同 |
| 注入 / 私有数据库 / 内存 | 能 | 能 | 违反规则 | 高 | 禁止 |

### 3.1 UIA / MSAA / 事件

UIA 是 provider-client 模型。客户端不能凭空建树，只能读取 provider 暴露的元素和模式。微信的 `WM_GETOBJECT(OBJID_CLIENT)` 和 `WM_GETOBJECT(UiaRootObjectId)` 都返回 0，说明它没有实现 MSAA 或 UIA provider。现有的 2 个 `Pane` 更可能是 UIA 默认 HWND provider 给出的窗口级信息，不是微信自己的文本树。

因此：

- `TextPattern2`、`TextEditPattern`、`TextChanged`、`AutomationPropertyChanged` 都没有基础。
- `ActiveTextPositionChanged` 只用于只读文本导航，不是输入框内容。
- 订阅焦点变化只能知道焦点换了，拿不到文字。
- 不能靠 `UIA` 的“唤醒”消息修复没有 provider 的应用。

### 3.2 剪贴板

技术上可以发送 `Ctrl+A`、`Ctrl+C`，再读 `CF_UNICODETEXT`。但对微信不可接受：

- 选区无法恢复。`Ctrl+A` 后整个输入框全选，下一次打字可能覆盖整段消息。
- 焦点可能不在输入框。微信没有可读的焦点语义，`Ctrl+C` 可能复制聊天记录。
- 空输入框不会改变剪贴板，可能把旧内容误记成这次语音。
- 剪贴板可能是图片、文件、OLE 对象、延迟渲染内容，无法无损恢复。
- Windows 剪贴板历史和云同步可能留下消息文字。
- 违反“用户主动使用剪贴板”的系统契约。

只适合用户显式点击“读取当前输入框”的一次性动作，不适合回眸自动记录。

### 3.3 TSF / IME

`ITfThreadMgr` 由应用通过 `CoCreateInstance(CLSID_TF_ThreadMgr, CLSCTX_INPROC_SERVER)` 创建，`Activate` 只对调用线程生效。没有公开 API 查询另一个进程的 `ITfDocumentMgr` / `ITfContext` 文本。唯一可行方式是安装 TIP DLL，让 TSF 把它加载进目标进程，这本质上是注入第三方进程，仓库规则禁止。

`ImmGetCompositionString` 只取组合串，不是最终上屏全文；对 Qt 自绘窗口也不可靠。输入法私有历史没有公开 API，读取私有数据库被规则禁止。

### 3.4 截图 OCR

见第 4 节。

### 3.5 音频 ASR

见第 5 节。

## 4. 截图 OCR 的可行边界

### 4.1 截图 API

| API | 微信 | 遮挡窗口 | 最小化 | 用户授权 | 判定 |
| --- | --- | --- | --- | --- | --- |
| `PrintWindow(PW_RENDERFULLCONTENT=2)` | 有内容 | 可截到内容 | 只得到标题栏/空 | 无系统弹窗 | 首选；同步调用需专用线程 |
| `BitBlt` 屏幕 DC | 前台可见时可用 | 截到遮挡窗口 | 只得到标题栏/空 | 无 | 仅 fallback |
| `Windows.Graphics.Capture` | 未实测 | 按语义可 | 未实测 | picker + 黄色边框 | v1 不用 |

已实测：

- 微信窗口 1371×1302，`PrintWindow` 截图非黑像素采样 1089/1155，说明有内容。
- 截图 OCR 整窗 29 行/328 字符。这个数字本身说明整窗包含大量聊天文字，不可用于回眸。
- `PW_RENDERFULLCONTENT` 在 Windows SDK `WinUser.h` 中定义，但 Microsoft Learn 的 `PrintWindow` 页面没有列出这个 flag。它属于可用但未文档化行为，必须记录风险。
- `PrintWindow` 是同步调用，目标卡死可能阻塞；必须放在专用线程并加 watchdog。
- 最小化窗口不要截；直接拒绝，记 `capture_minimized`。

`Windows.Graphics.Capture` 需要 picker 和黄色边框。关闭边框需要 MSIX capability 和用户授权；本产品用 NSIS 非打包安装，不能声明该 capability。`CreateForWindow` 要求 Windows 10 1903+，而本仓库最低支持 1809。v1 不选它。

### 4.2 系统 OCR 的能力与限制

本机安装 `zh-Hans-CN` OCR 语言包，离线可用。`OcrEngine.MaxImageDimension` 本机为 10000。引擎返回行、词、词边界框；**不返回置信度**。

合成中文测试（Microsoft YaHei UI，含中文、英文、数字、标点、多行）：

| 场景 | 结果 |
| --- | --- |
| 16 px | 明显错字：`这是`→`汶是`、`用于`→`厍于`、`句子`→`旬子`；漏标点；英文句号→`0`；汉字之间插空格 |
| 16 px 先 2× 放大 | 中文错字基本消失；仍有 `English0`、空格、个别标点损失 |
| 24 px | 仍有 `标点空格` 漏顿号、`English0`、空格 |
| 标点密集行 | 21 个标点只识别出 5 个；不可靠 |
| 中英混合 / 数字日期 | 英文和 `.` `-` 容易错；需逐项验收 |

结论：

- OCR 结果不能当“精确原文”。
- 汉字之间的空格必须做 CJK 归一化，否则回眸会显示“语 音 输 入”。
- 没有置信度，无法按阈值过滤；只能靠区域、空基线、连续两帧一致、字符集检查来降低错误。
- 标点、英文、数字、换行不能承诺 100%；产品文案必须说明。

### 4.3 为什么不能直接接现有状态机

| 现有状态机要求 | OCR 现实 |
| --- | --- |
| 注入前取基线 | BLE 在 `begin_pending` 后很快注入；截图 + OCR 冷启动可能晚于首字 |
| 同一 `focus_id` | OCR 没有 UIA 元素，只能用 HWND + 区域 + DPI |
| 精确连续插入 | OCR 插空格、错字、漏标点，会被判 `discontinuous_text_change` |
| 选区/整段替换 | OCR 读不到选区，`selection_text` 只能为空 |
| 密码/只读/搜索/设置排除 | 像素无法判断 `IsPassword`、`IsReadOnly`、控件类型或语义 |
| 隐私：只记录新增，不保存全文 | 整窗 OCR 会读聊天列表、联系人、通知；差异算法不能保证只取输入框 |
| 125 ms 轮询 | OCR 太重；需要预热、独立 worker、更长稳定窗 |
| 记录来源 | 需要新的 capture method/version，不能和 UIA 记录混在一起 |

因此 OCR 不能作为 `WindowsSnapshotProvider` 的另一个实现直接塞进现有状态机。它需要独立的功能、独立的状态机、独立的隐私说明。

### 4.4 推荐的最小 OCR 方案（如果用户明确要做）

范围：

- 只对用户明确启用的应用生效；默认关闭。
- 用户框选一次输入框区域；保存为客户区相对矩形，底部锚定。
- 语音开始时 OCR 基线；基线为空或等于占位文字才继续。
- 基线非空（已有草稿）→ 首版直接跳过，不尝试复杂 diff。
- 语音结束后只 OCR 同一区域；每 150–300 ms 一次，最多 8 秒。
- 连续两帧文本一致（约 600 ms）才结算。
- 输入框被发送清空 → 保留最后一个非空候选。
- 最后一行贴区域底边 → 可能截断，放弃。
- 候选超过 8000 字符 → 放弃。
- 区域外变化、窗口移动、失焦、最小化、应用切换 → 放弃。
- 截图只在内存；不落盘；不写日志；不进入 IPC。

归一化：

- 去掉 CJK 字符之间的空格。
- 保留原文用于显示；比较时统一全/半角标点。
- 中文行直接拼接；英文行之间补一个空格。
- 不做繁简转换，不自动“纠正”成用户可能没输入的内容。

第二阶段才考虑非空基线：

- 只接受“结束文本 = 基线文本 + 单一连续插入”。
- 中间插入、替换、多段修改、选区替换 → 放弃。
- 仍然只允许用户框选区域，不整窗。

### 4.5 OCR 的隐私红线

- 禁止整窗 OCR 作为默认。
- 禁止自动截取“窗口底部 20%”并直接记录；可以做建议区域，但必须让用户确认。
- 禁止把截图写入磁盘、临时目录、崩溃报告或日志。
- 禁止把 OCR 原文写入诊断日志；只写字符数、行数、耗时、失败原因。
- 必须用测试账号和 canary 字符串验证：聊天区 canary 绝不出现在记录、日志、IPC 或崩溃转储中。
- 企业安全软件可能把截图 API 当作风险行为；需要在目标环境验证。

### 4.6 OCR 验收门槛（建议，不是既成事实）

只有以下条件全部满足，才考虑进入产品：

- 10–50 字单行中文，在 100% / 150% / 200% DPI 下，归一化后完全匹配率 ≥ 90%，CER ≤ 5%。
- 标点、英文、数字、换行分别记录；不把“基本可读”当成“精确”。
- 裁剪区域 OCR 在预热后 p95 < 100 ms；语音注入和音频链路无可测变慢。
- 空基线、占位文字、已有草稿、发送即清空、连续语音全部有明确结果。
- 失焦、切应用、关闭微信、断连、睡眠、退出都不崩溃、不卡语音、不留下截图。
- RC001 与 RC003 分别真机 passed；Windows 10 1809 与 Windows 11 分别记录；未测标 `deferred`。

## 5. 音频 ASR 的边界

### 5.1 已测结果

本机用 `System.Speech` 中文听写识别合成语音：

```text
TEXT=这是一个用于无线买回眸测试的句子 CONF=0.478
```

预期是“无线麦”，实际是“无线买”；没有标点；置信度低。旧 SAPI 声学模型不适合作为生产方案。

### 5.2 可选引擎

| 方案 | 中文质量 | 延迟 | 离线 | 包体/许可 | 隐私 |
| --- | --- | --- | --- | --- | --- |
| `System.Speech` | 低；本次实测有错字 | 中 | 是 | 系统组件 | 本地 |
| `Windows.Media.SpeechRecognition` | 一般；中文取决于语言包和在线听写设置 | 短句快 | 听写可能依赖在线 | 系统组件 | 可能上传音频 |
| whisper.cpp / Whisper | 需实测；large-v3 中文通常更好，tiny/base 不适合 | CPU 可能数秒 | 是 | MIT；模型 75 MiB–2.9 GiB | 本地 |
| 云端 ASR | 通常最好 | 网络往返 | 否 | API/密钥/费用 | 音频离开设备 |

`Windows.Media.SpeechRecognition` 的公开接口按系统麦克风工作，没有 `SetInputToAudioStream`。要把 ATVV 的 PCM 喂进去，需要虚拟音频设备或换引擎，不是一个直接调用。

### 5.3 语义差异

ASR 输出的是“音频的转写”，不是“目标输入框实际新增的文字”。可能的差异：

- 输入法选择同音字、云端纠错、用户候选，和独立 ASR 不同。
- 语音期间焦点丢失、输入法未激活、目标不可编辑，文字根本没上屏，但 ASR 仍有结果。
- 用户随后手动修改、删除、换行。
- 语音键在非输入场景被按下，ASR 仍会产生记录。

如果接入回眸，必须新增来源类型（例如“语音转写”），明确标注“可能未上屏”，不能和 UIA 记录混在一起。默认关闭、按应用 opt-in、不保存音频、不上传。

### 5.4 建议的 ASR spike

1. 收集 20–30 条中文短语音，覆盖标点、数字、英文、同音字。
2. 用目标引擎离线跑，和实际上屏文字逐字对比，统计 CER、标点/数字错误率。
3. 测冷启动、5–10 秒语音的 p50/p95 延迟、CPU/内存峰值。
4. 对比 whisper.cpp small / large-v3 或中文专用模型，记录模型体积和分发方式。
5. 只有 CER、延迟、产品语义都可接受，才进入实现。

## 6. 建议

### P0：保持现状，补清边界

- 微信 4.x 聊天输入框不支持；不要把它描述成权限或设置问题。
- 不把 OCR/剪贴板作为默认兜底。
- `unsafe_destination` 已拆成稳定 reason 码，下一次日志能直接定位；行为仍是“不记录”。

### P1：无线麦自身

- 默认继续排除。
- 如果确实有某个自身输入框需要记录，先给该控件加稳定 `id`，再做白名单：
  - 前台窗口属于 SayAll，用前台 PID 判断，不用元素 PID。
  - 只允许 `Edit`，必须有 `ValuePattern` 且非只读。
  - 精确匹配 `AutomationId`；不做名称/类名模糊匹配。
  - 排除 onboarding 测试框、搜索、设置、宏名称/URL、回眸页、数字输入。
  - `snapshot` 和 `read` 使用同一门禁。
  - 单独测试共享 UIA worker 的尾延迟；必要时给回眸单独 worker。
- 如果只是为了 onboarding 验证，不要做自捕获；现有 DOM 读回已经足够。

### P2：微信，如果必须支持

二选一：

1. **opt-in 截图 OCR 区域兜底**：最接近“读输入框”，但只支持空基线，准确率和标点有限，需要区域校准和隐私评审。
2. **独立“语音转写”功能**：应用无关，但记录的是语音，不是上屏文字；需要 ASR spike 和单独的产品语义。

不建议：

- 整窗 OCR。
- 剪贴板自动读取。
- TSF 注入。
- 为微信放开 `self_foreground`。
- 把 OCR 结果直接接入现有精确 diff 状态机。

## 7. 待验证（`deferred`）

| 项 | 状态 | 说明 |
| --- | --- | --- |
| 微信 4.1.15.56 可见聊天主窗口 UIA 复测 | deferred | 当前主窗口隐藏；已有 4.0 可见窗口 2 Pane 证据，当前版本日志已复现失败 |
| 微信输入框 OCR 准确率 | deferred | 需要受控测试账号、固定句子、100/150/200% DPI |
| 微信输入框 Ctrl+A/C 实际行为 | deferred | 未实测；即使可用，选区恢复问题仍在 |
| 本地 Whisper 中文 CER/延迟 | deferred | 需要真实语音和目标硬件 |
| SayAll 自身输入框的 UIA 属性与 worker 延迟 | deferred | 需要打开一个明确输入框做只读探针 |
| Windows 10 1809 的 OCR/PrintWindow | deferred | 本机是 Windows 11 26200 |

## 8. 主要证据与来源

仓库代码与文档：

- `crates/sayall-windows/src/transcript_capture_windows.rs:108`、`:117`、`:122`、`:160-225`。
- `crates/sayall-windows/src/transcript_capture.rs:288-305`、`:347-373`。
- `crates/sayall-windows/src/focus_windows.rs:112-114`、`:758-781`。
- `crates/sayall-windows/src/ime.rs:326`。
- `docs/product/reflections.md:12`、`:35`。
- `docs/plan/2026-10-25-reflections-windows.md:37`。
- `docs/investigations/2026-10-01-windows-input-focus-uia-feasibility.md:31`、`:41-45`。
- `Testing/probe-msaa-provider.ps1`、`Testing/probe-uia-focus.ps1`。

日志：

- `%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`，2026-10-25T08:04–08:08。
- 只使用 `reason`、`generation`、`fg_pid`、`fg_self`、字符数等字段；未读取或输出文字。

Microsoft 文档：

- [UI Automation Providers Overview](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-providersoverview)
- [WM_GETOBJECT](https://learn.microsoft.com/en-us/windows/win32/winauto/wm-getobject)
- [AccessibleObjectFromWindow](https://learn.microsoft.com/en-us/windows/win32/api/oleacc/nf-oleacc-accessibleobjectfromwindow)
- [Text and TextRange Control Patterns](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-about-text-and-textrange-patterns)
- [IUIAutomationTextPattern2](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nn-uiautomationclient-iuiautomationtextpattern2)
- [IUIAutomationTextEditPattern](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nn-uiautomationclient-iuiautomationtexteditpattern)
- [PrintWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-printwindow)
- [BitBlt](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-bitblt)
- [Screen capture](https://learn.microsoft.com/en-us/windows/uwp/audio-video-camera/screen-capture)
- [GraphicsCapturePicker](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturepicker)
- [OcrEngine](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr.ocrengine)
- [OcrWord](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr.ocrword)
- [PowerToys Text Extractor](https://learn.microsoft.com/en-us/windows/powertoys/text-extractor)
- [TSF Architecture](https://learn.microsoft.com/en-us/windows/win32/tsf/architecture)
- [ITfThreadMgr::Activate](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfthreadmgr-activate)
- [About the Clipboard](https://learn.microsoft.com/en-us/windows/win32/dataxchg/about-the-clipboard)
- [Speech recognition](https://learn.microsoft.com/en-us/windows/apps/design/input/speech-recognition)
- [System.Speech SpeechRecognitionEngine](https://learn.microsoft.com/en-us/dotnet/api/system.speech.recognition.speechrecognitionengine)

第三方：

- [OpenAI Whisper](https://github.com/openai/whisper)
- [whisper.cpp](https://github.com/ggerganov/whisper.cpp)
- [Tesseract](https://github.com/tesseract-ocr/tesseract)
- [PaddleOCR](https://github.com/PaddlePaddle/PaddleOCR)
- [RapidOCR](https://github.com/RapidAI/RapidOCR)

## 9. 2026-10-26 发送清空与占位文案

用户反馈：一条记录录音正确，但在聊天输入框按下发送后，回眸文字变成「发送消息...」。

只读 UIA 探针（DimAgent 主窗口）：

| 项目 | 读数 |
| --- | --- |
| 焦点控件 | `ControlType.Group` |
| 类名 | `tiptap ProseMirror outline-none ProseMirror-focused` |
| `ValuePattern` | 无 |
| `TextPattern.DocumentRange.GetText(-1)` | `发送消息...\n`（空输入框的占位文案） |
| `Name` / `HelpText` | 空 |

日志（session `2f7395ba-…`，2026-10-26 19:12，原始录音 205484 字节）：

- `baseline ... chars=23 selection_chars=23`，整个旧字段被选中，随后观察到多次 `whole_selection_replacement` 候选；
- 按下发送后 `change ... reason=whole_selection_replacement ... current_chars=8`；
- 900 ms 稳定后 `save ... chars=8`，保存了占位文案；录音本身正确。

同一天 18:25 的 session `2ef9d179-…` 也出现 `baseline_chars=25 → current_chars=8 → save chars=8` 的相同形状（该条没有录音）。

根因：`TextPattern` 在空输入框返回占位文案；旧状态机把占位文案当成整字段替换候选，覆盖了此前真实的插入候选。不是 UIA 读数错误，也不是录音问题。

修复（2026-10-26）：

- 新增 `sayall_core::transcripts::is_placeholder_text`，识别常见聊天占位文案（忽略空白、大小写、省略号和句末标点）；
- 状态机把空文本或占位文本视为“已清空”；连续两次确认后，有候选则保存最后一次非空候选（`reason=cleared_after_candidate`），没有候选则放弃（`cleared_before_candidate`）；
- 输入框回到与基线相同的文本时同样保留候选（覆盖空输入框发送的场景）；
- 日志只记录 `result=cleared`、原因和字符数，不记录占位文案或听写内容。

已知边界：占位文案依赖应用文案；当前覆盖「发送消息 / 输入消息 / 说点什么 / Ask anything / Send a message」等常见形态。新增应用占位文案时需要补列表或用真实 UIA 探针复核。修复只影响新写入的记录，已有「发送消息...」记录不会自动改写。
