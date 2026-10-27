# Windows「回眸」实施计划

- 日期：2026-10-25
- 状态：**文字版、原始录音与「仅录音」条目已实现；自动化与本地安装包页面验证通过；RC001/RC003 真机分型号验收 deferred**（按 [Testing/WindowsReflections.md](../../Testing/WindowsReflections.md) 执行）
- 设计稿：[回眸 Windows 设计稿](../../artifacts/design/2026-10-25-reflections/README.md)（本地设计稿，`artifacts/` 按仓库规则不入库）
- 仓库：`D:\SayAll\src\GetSayAll\remote-mic-app-windows`
- 参考实现：Mac 仓库 `D:\SayAll\src\GetSayAll\remote-mic-app`（只参考行为与数据边界，不复制 Swift 源码）

## 0. 范围与阶段

- 第一阶段（2026-10-25 已完成自动化）：文字回眸；侧栏位置放在「推荐」之后、「权限」之前（用户指定）。
- 第二阶段（2026-10-25 追加，用户要求继续参考 Mac）：回眸原始录音的保存、播放、导出、删除。
- 第三阶段（2026-10-26 追加，用户报告 WorkBuddy/微信拿不到录音）：文字捕获失败但录音成功的会话以「仅录音」条目展示，并记录目标应用身份。
- 仍不做：本地 Agent 访问 / MCP、云同步、跨端导入。录音只在「记录回眸」开启时可用。

## 0.1 原始录音实现要点（第二阶段）

- 本地 WAV PCM s16le、16 kHz、单声道；路径 `<transcripts-v1>\audio\<sessionId>\original.wav` + `manifest.json`。
- 设置 schema 升到 7，新增 `local_original_audio_recording_enabled`，默认关闭；schema 6 及更早配置迁移为关闭。
- 录音跟随语音会话开始/结束/取消；队列溢出或写入失败丢弃本次录音，不影响语音、文字上屏或文字记录。
- 页面只读 manifest 列表；播放时按会话号读取 WAV 并转成 data URL；导出通过系统保存对话框；删除走 Windows 回收站。
- 录音成功落盘后发出只含会话号的 `transcript-recording-finished` 事件，页面静默刷新，没有文字记录时也能及时出现。
- 文字记录只有一个合并删除入口：删除文字记录时，关联录音一并删除；「仅录音」条目提供自己的删除入口。
- 文案、隐私边界和真机用例分别落实到 `docs/product-copy.md`、`docs/product/reflections.md`、`Testing/WindowsReflections.md`；格式与生命周期决策见 `docs/investigations/2026-10-25-windows-reflections-original-audio.md`。

## 1. 目标

把 Mac 已经验证过的「回眸」核心行为复刻到 Windows：

- 用户主动开启后，记录语音输入**实际写入目标输入框**的文字。
- 按目标应用和本机日期整理，倒序查看。
- 支持复制单条、删除单条、删除当前应用、全部删除。
- 默认关闭；关闭只停止新增，不删除已有记录。
- 完全本地保存，不上传；不保存输入框全文、窗口标题、文档名、网址或设备标识。
- 可选保存原始录音，支持播放、导出、删除；没有文字记录的录音以「仅录音」条目展示；录音同样只保存在本机。
- 删除走 Windows 回收站，可手动恢复。

## 2. 非目标

- 为旧录音回填或猜测应用身份（升级前 manifest 没有身份，统一显示「未知应用」）。
- 本地 Agent 读写接口、MCP Helper、客户端快速连接、授权管理。
- 统计 / 个人中心页面本身不在回眸首版；个人中心已有独立页面，只提供回眸跳转，回眸仍保留侧栏入口。
- 云同步、全文搜索、AI 总结、跨设备同步、后台常驻服务。
- 导入 Mac 的回眸历史。两个平台的数据目录和身份字段不同；首版不承诺跨端迁移。
- 读取第三方应用的私有配置、内存、数据库或私有协议。
- 为读不到文字的应用做 OCR、剪贴板兜底或注入。

## 3. 复刻依据与已经确认的事实

### 3.1 Mac 的行为

- 页面名称「回眸」，英文 `Reflections`；开关「记录回眸」。
- 页头：标题 + 两行隐私说明 + 最右侧开关；底部不重复隐私卡。
- 默认展示全部应用最近 7 天；选中具体应用后展示该应用的全部历史。
- 顶部应用栏固定「全部应用」，其余应用按最近使用排序；可单行横向浏览，也可在页面内展开为自适应网格。
- 日期倒序，默认展开最新一天，可逐日折叠。
- 记录行显示时间、目标应用真实图标和名称、文字；支持复制、删除。
- 删除单条、按应用删除、全部删除；Mac 用废纸篓，Windows 对应回收站。
- 捕获只在能通过 Accessibility 读取的安全可编辑输入区域生效；密码框、搜索框、地址栏、设置类输入区域不记录。
- 语音开始后先取输入框基线；语音结束后最多等 8 秒，只接受原位置附近的一段连续新增；文字稳定约 900 ms 后保存。
- 目标应用切换、焦点丢失、文字不连续、关闭开关都会放弃或取消。
- 连续语音时，上一段已经观察到的候选会在下一段开始前结算。
- 存储为版本化 JSON，按 `<应用>/<YYYY-MM-DD>.json` 组织；应用身份不直接暴露在界面和日志中。

### 3.2 Windows 现有基础

已经存在的、可以复用的部分：

- 语音会话生命周期：`crates/sayall-windows/src/ble.rs` 的 `PipelineOutput::MicrophoneOpenRequested`、`StreamStarted`、`StreamStopped`；`StreamStarted` 里已经有一段 20 ms 的注入前等待，可作为基线快照的时间窗口。
- UI Automation：`crates/sayall-windows/src/focus_windows.rs` 已有 MTA UIA 工作线程、Per-Monitor V2 DPI、候选扫描、焦点读回、密码/只读过滤、Chromium/Electron 兼容处理。
- 前台应用识别：`focus_windows::foreground_process_id`、`focus_windows::target_window`、`app_launcher::process_app_user_model_id`、`registered_apps` 的 AUMID 口径。
- 应用启动/图标：`src-tauri/src/app_icon.rs` 只负责本应用图标；目标应用图标需要新写 Shell 提取，但 `Win32_UI_Shell`、`Win32_Graphics_Gdi` feature 已经启用。
- 设置：`AppSettings`、`SettingsStore`、版本化迁移、Tauri 命令模式。
- 页面：Vue 3 + Tauri 2；`src/navigation.ts`、`src/pages/*`、`src/lib/bridge.ts`；现有设置页、组合动作页提供卡片、开关、`<dialog>`、空态和错误文案模板。
- 验证：`pnpm test`、`pnpm build`、`cargo fmt --all -- --check`、`cargo test --workspace`、`cargo check -p sayall-windows-app --features runtime-simulation`、`scripts/ci-preflight.ps1`。

### 3.3 还没有被仓库实测证明的部分

这些必须在 Phase 1 的探针里拿到证据，不能凭“UIA 应该可以”直接写产品代码：

- Windows UIA 是否能稳定读到目标输入框的**完整文字**（`ValuePattern` 或 `TextPattern.DocumentRange.GetText`）。
- 是否能读到**光标/选区位置**。读不到时，只能用全文前后缀做单一连续差异；这可能产生误判。
- 在 Chromium/Electron/WinUI/记事本中的读取耗时，是否能在不拖慢语音注入的前提下拿到基线。
- 输入法组合文本（composition）未提交时的中间态，是否会造成不连续差异。
- 打包应用（ChatGPT 等）的前台进程可能是 `ApplicationFrameHost`，应用身份和图标是否能正确归属。

已知反例（来自既有调查）：微信 4.x 聊天主窗口、WorkBuddy 不暴露 UIA 文本输入。首版对这类应用静默跳过，不报错、不弹窗、不要求管理员权限。

## 4. 用户可见行为与验收口径

### 4.1 页面入口与开关

- 侧栏新增「回眸」，位置在「推荐」之后、「权限」之前。
- 页头标题「回眸」，右侧「记录回眸」开关，默认关闭。
- 开关开启/关闭立即持久化；关闭时正在进行的捕获取消。
- 关闭后已有记录照常显示；只有没有任何记录时才显示关闭态空状态。

### 4.2 记录范围

- 只记录本次语音会话实际新增的文字，不记录输入框原文。
- 每条记录包含：开始/结束时间、目标应用名称、稳定应用键、本机日期、时区、来源（当前只有遥控器语音）、文字、捕获方法版本。
- 不保存：音频、输入框全文、前后文、窗口标题、文档名、URL、设备地址、进程完整路径、剪贴板内容。
- 单条文字上限 8000 字符（与 Mac 一致）。超过上限的候选放弃保存并写日志。

### 4.3 应用与日期

- 全部应用视图只显示最近 7 天。
- 选中具体应用后显示该应用的全部记录。
- 应用栏固定「全部应用」；其余按最近使用排序；可展开为网格。
- 日期倒序；默认展开最新一天；逐日折叠。
- 应用图标用真实系统图标；取不到时用通用应用图标。

### 4.4 复制、删除与恢复

- 复制：写入系统剪贴板，图标短暂变对勾 1.5 秒；失败显示页面内错误。
- 删除单条：确认后把当日文件移到回收站，再写回剩余记录。
- 删除当前应用：把该应用目录移到回收站。
- 全部删除：把整个 `v1` 根目录移到回收站。
- 回收站操作失败时不删除、不覆盖，页面给出可行动的错误。
- 关闭开关不触发删除。

### 4.5 捕获失败的用户可见策略

- 读不到文字、目标不可读、焦点变化、敏感输入框：**不记录，不打扰用户**。
- 只在结构化日志里记录稳定原因，界面不显示 UIA/辅助功能/捕获失败等技术词。
- 如果保存失败（磁盘、权限、回收站），页面打开时显示一行非阻塞错误；不在语音过程中弹窗。

## 5. 技术方案

### 5.1 分层

| 层 | 文件 | 职责 |
| --- | --- | --- |
| 核心纯逻辑 | `crates/sayall-core/src/transcripts.rs` | 记录模型、日文件、应用键、校验、7 天策略、分组排序、文本连续差异；不依赖 Windows/Tauri |
| Windows 平台 | `crates/sayall-windows/src/transcript_capture.rs` | UIA 文本快照/轮询、前台应用识别、敏感目标过滤、图标提取、捕获状态机 |
| 应用宿主 | `src-tauri/src/transcripts.rs` | 本地归档、回收站删除、图标缓存、Tauri 命令、事件 |
| 设置 | `crates/sayall-core/src/settings.rs`、`src-tauri/src/settings.rs` | `local_transcript_history_enabled` 默认关闭、持久化、迁移 |
| 前端 | `src/pages/ReflectionsPage.vue`、`src/lib/bridge.ts`、`src/navigation.ts`、`src/components/Sidebar.vue` | 页面、筛选、折叠、复制、删除确认、空态、深浅色 |
| 文档 | `docs/product/reflections.md`、`Testing/WindowsReflections.md`、`docs/product-copy.md`、`TODO.md`、`ATTRIBUTION.md` | 产品口径、验收、命名、待办、来源 |

不把 UIA、Shell 图标、回收站 API 放进 `sayall-core`。`sayall-core` 保持跨平台可编译、可单测。

### 5.2 数据模型

建议与 Mac 保持语义对齐，但使用 Windows 可表达的字段：

```text
TranscriptRecord
  schemaVersion: 1
  id: UUID
  sessionId: UUID
  startedAt: RFC3339（带本地偏移）
  endedAt: RFC3339（带本地偏移）
  localDateKey: YYYY-MM-DD
  timeZoneId: Windows 时区 ID
  utcOffsetMinutes: i32
  applicationKey: 安全路径组件（显示名清洗 + 稳定哈希）
  applicationName: 目标应用显示名
  source: bluetooth_remote
  originalTranscript: String
  captureMethodVersion: 1
```

- `applicationKey` = 清洗后的显示名 + `-` + 稳定哈希（FNV-1a 64 或 SHA-256 截断）。哈希输入是应用身份（AUMID 或可执行文件完整路径），但**完整路径不落盘、不进日志**。
- `applicationName` 从版本信息 `ProductName`/`FileDescription` 读取；取不到用可执行文件主名；打包应用从 AUMID 末段或 Shell 显示名回退。
- 图标缓存：`%LOCALAPPDATA%\SayAll\transcripts\v1\icons\<applicationKey>.png`，64×64 或 128×128 PNG。记录本身不存路径。
- 日文件：`%LOCALAPPDATA%\SayAll\transcripts\v1\<applicationKey>\<YYYY-MM-DD>.json`。
- 日文件格式：`formatVersion: 1`、`applicationKey`、`applicationName`、`localDateKey`、`records`。
- 读取时跳过格式不符、日期不符、应用键不符、`schemaVersion` 不符的记录；跳过数量写日志，不删除原文件。
- 单日文件上限、总记录上限、单条文字上限都要有常量并在测试里钉住。建议首版总记录上限 10,000 条、单日 2,000 条、单条 8,000 字符；超过时不再新增并记录稳定原因。

### 5.3 存储与删除

- 目录：`%LOCALAPPDATA%\SayAll\transcripts\v1`。不放 `%APPDATA%`，避免把可能较大的本地文字历史漫游到其他机器。
- 原子写：写 `<date>.json.tmp` → `fsync` → 替换目标文件；写入前保留旧文件。Windows 用 `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)` 或 `ReplaceFileW`。
- 删除单条：
  1. 读取日文件，移除目标记录。
  2. 把原日文件移到回收站（`IFileOperation` / `SHFileOperationW`，`FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT`）。
  3. 如果有剩余记录，写回新日文件；如果为空，不重建。
  4. 第 2 步失败则不写回，保留原状并返回错误。
- 删除应用/全部：直接把目录移到回收站；失败不假装成功。
- 所有删除都只走回收站，不永久删除。
- 归档写入在独立工作线程执行，不阻塞 BLE/音频/注入；失败只记日志和页面错误，不让语音链路失败。

### 5.4 捕获状态机

与 Mac 的时序对齐，但为 Windows 的 UIA 延迟做非阻塞设计：

```text
语音键按下
  → MicrophoneOpenRequested：创建 pending session，异步启动基线快照
  → StreamStarted：绑定 session_id/generation，照常注入快捷键和音频
  → 基线就绪：记录目标快照（应用、焦点身份、文字、选区、安全判定）
  → StreamStopped：记录 endedAt，开始轮询
      - 125 ms 一次，总预算 8 s
      - 文字稳定 900 ms → 保存
      - 下一段语音开始 → 先结算上一段已观察到的候选
      - 目标切换/焦点丢失/敏感目标/关闭开关 → 取消或结算已观察候选
      - 超时 → 结算已观察候选；没有候选则取消
```

关键实现约束：

- **不阻塞语音**：基线快照和轮询都在专用 UIA 工作线程上执行；BLE 线程只发消息，不等待 UIA 结果。注入路径和音频路径不能因为捕获变慢。
- **基线优先**：尽量在 `MicrophoneOpenRequested` 就启动快照，利用 `StreamStarted` 前的注入等待窗口。如果快照在文字出现后才回来，本次放弃（记录 `baseline_late`），不猜。
- **控制文本范围**：对 Chromium/Electron，`DocumentRange` 可能覆盖整页而不是输入框本身。优先使用焦点元素的 `ValuePattern`，或 `TextPattern` 的局部范围；单次读取上限 1 MB，超过上限放弃，避免读整页文档造成卡顿或误判。
- **有界等待**：每次 UIA 读取有超时；工作线程卡住时放弃本次捕获，不挂住应用。不能使用无超时的 `reply_receiver.recv()`。
- **session/generation**：每次会话有唯一 `session_id` 和递增 `generation`；迟到回调、旧轮询、旧删除不能污染新会话。
- **窗口失焦/切应用**：读取时同时校验目标 PID 和 UIA RuntimeId；不一致立即取消。
- **敏感目标**：`IsPassword`、只读、禁用、地址栏、搜索框、设置类输入区域一律拒绝。复用 `focus_windows` 的语义排除词，并补 Windows 常见英文/中文关键词。
- **文字差异**：
  - 优先使用选区/光标位置，要求只接受原位置附近的一段连续新增。
  - 读不到选区时，用“原文前缀 + 新增 + 原文后缀”的单一连续差异；如果存在多个可能切分、或前后缀之外还有改动，放弃。
  - 不能只比较长度；必须比较内容。
  - 组合文本中间态、文字被替换、目标清空等路径都要有测试。
- **隐私**：快照里的完整文字只在内存中用于差异计算，不写日志、不进 IPC、不进错误信息。日志只写字符数和稳定原因。

### 5.5 前台应用与图标

- 前台窗口 → PID；排除本进程。
- 应用身份优先级：
  1. 窗口 AUMID（打包应用，避免 `ApplicationFrameHost` 误归属）。
  2. 进程 AUMID。
  3. 可执行文件完整路径（只用于计算 `applicationKey` 和取图标，不落盘）。
- 显示名优先级：版本信息 `ProductName` → `FileDescription` → 可执行文件主名 → AUMID 末段 → 「未知应用」。
- 图标：`PrivateExtractIconsW` 取 64×64 HICON，经 `GetDIBits` 转 RGBA 后编码 PNG（按可执行路径缓存）；写入图标缓存；失败用通用图标。
- 启动器/AUMID 口径复用 `app_launcher`、`registered_apps` 的既有实现，不另写一套。

### 5.6 设置

- `AppSettings` 新增 `local_transcript_history_enabled: bool`，默认 `false`。
- `CURRENT_SCHEMA_VERSION` 升到 6；旧配置缺字段时保持 `false`，不迁移成开启。
- `SettingsStore::save_local_transcript_history_enabled` 持久化；保存失败回退开关并给页面错误。
- 启动时读取设置后调用 `platform.set_transcript_capture_enabled(saved.local_transcript_history_enabled)`。
- 开关变化时同时：
  1. 保存设置；
  2. 更新平台原子开关；
  3. 关闭方向调用捕获取消；
  4. 页面立即反映状态；历史记录不重新加载也可以保留。

### 5.7 IPC 与事件

建议命令：

```text
get_transcript_history() -> TranscriptHistorySnapshot
set_transcript_history_enabled(enabled: bool) -> bool
delete_transcript(record_id: String) -> TranscriptHistorySnapshot
delete_transcript_application(application_key: String) -> TranscriptHistorySnapshot
delete_all_transcripts() -> TranscriptHistorySnapshot
```

`TranscriptHistorySnapshot` 建议包含：

```text
enabled: bool
revision: u64
applications: [{ key, name, count, latestEndedAt, iconDataUrl? }]
records: [{ id, sessionId, endedAt, localDateKey, applicationKey, applicationName, text }]
totalCount: u64
recentCount: u64
archiveError: string | null
```

- 图标以 64 px PNG data URL 放在 applications 里；应用数量通常很少，不需要逐条 IPC。
- 记录文字只在页面需要时返回；不放进事件、不放进日志。
- 保存成功后从 Rust 发 `transcript-captured` 事件，只带 `{ applicationKey, sessionId, revision }`；页面收到后重新拉取快照。事件不带文字。
- 页面每次挂载拉一次；删除后直接用返回值更新，不再二次请求。
- 如果页面未打开，事件没有监听者，不影响保存。

### 5.8 前端

新增 `ReflectionsPage.vue`，状态：

- `loading` / `ready` / `error`
- `enabled`
- `selectedApplicationKey: string | null`
- `expandedApplicationSwitcher: boolean`
- `expandedDayKeys: Set<string>`
- `records`、`applications`
- `deleteDialog: { kind: "record" | "application" | "all", ... } | null`
- `copiedRecordId: string | null`
- `archiveError`

交互：

- 侧栏入口加入 `src/navigation.ts`，位置在 `recommendations` 之后、`permissions` 之前；`Sidebar.vue` 增加 `textbubble` 图标。
- 页头开关调用 `setTranscriptHistoryEnabled`；失败回退并显示错误。
- 应用筛选：全部应用视图按最近 7 天过滤；具体应用不过滤日期。
- 展开/收起应用栏；展开时 4 列自适应网格。
- 日期默认展开最新一天；切换应用时重置为最新一天。
- 复制用 `navigator.clipboard.writeText`；失败在页面内显示错误，不弹窗。
- 删除用 `<dialog>`；确认后调用命令；成功更新快照，失败保留原状态并显示错误。
- 所有中文正文不小于 12 px；长文字完整换行；应用名过长尾部截断，悬停显示全名。
- 深色/浅色使用现有 token；强调色跟随系统。
- 空态、关闭态、关闭但有记录态按设计稿。

### 5.9 功能开关

新增 `REFLECTIONS_PAGE_ENABLED`（`src/lib/feature-flags.ts`）：

- 开发分支默认 `true`，方便本地验收。
- 合入 main 前若未完成真机验收，保持 `false`，避免出现空入口。
- 已记录的本地数据不因入口隐藏而删除。
- 真机验收通过后单独提交把开关改为 `true`，再出正式包。

## 6. 分阶段实施

### Phase 0：设计确认（0.5–1 天）

- 交付设计稿与状态截图；逐项确认：
  1. 侧栏位置。
  2. 最近 7 天计数与应用总数并存。
  3. 超长文字完整换行。
  4. 空态页脚。
  5. 首版是否只做文字。
- 设计未确认前不写产品代码。

**完成判据**：你在设计稿上明确回复「按这版做」或列出修改项；修改项回写设计稿后再确认。

### Phase 1：UIA 文本捕获探针（1–2 天）

目标：拿到“能不能读、读得准不准、会不会拖慢语音”的证据。

- 新增一次性探针 `Testing/probe-uia-text-capture.ps1` 或 `crates/sayall-windows/examples/transcript_capture_probe.rs`：
  - 只打印控件类型、AutomationId 长度/是否存在、Class 长度、ValuePattern/TextPattern 可用性、文字长度、哈希、选区数量/长度、读取耗时。
  - **不打印文字内容、窗口标题、路径**。
  - 覆盖：记事本、Chrome、Edge、DimAgent、VS Code、微信聊天窗口、WorkBuddy、密码框、地址栏、搜索框、只读网页。
  - 对 Chromium/Electron 分别测 `ValuePattern`、焦点元素 `TextPattern`、`DocumentRange` 三种读数，确认哪一种范围是输入框而不是整页。
  - 做一个“基线 → 模拟输入固定测试串 → 读取 → 差异”最小实验。优先用专用 fixture 窗口；如果用记事本，必须是临时文件且不得复用用户已打开的记事本窗口。
- 结论写入 `docs/investigations/2026-10-25-windows-transcript-capture.md`，包含正例/反例矩阵和耗时。

**探针本身要先避开的坑**（本机快速试探已经遇到，正式探针必须按这些口径写）：

- Windows 11 记事本是打包应用，可能复用已有 Notepad 进程/窗口；不要把 `Start-Process` 返回的 PID 当作窗口 PID。产品路径只认**前台窗口 PID**，探针也一样。
- Chrome 的 UIA 树里第一个 `Edit` 可能是地址栏，不是测试输入框；**禁止“找不到目标就取第一个 Edit”**。必须按 AutomationId/焦点元素/ControlType 精确定位。
- `SetForegroundWindow` 可能被前台锁拒绝；探针不要依赖强制抢焦点。用真实前台窗口，或改用 `ValuePattern.SetValue` 验证“能读到变化”，再单独验证真实键盘输入。
- 探针不能碰用户正在编辑的文档；所有写操作只在临时 fixture 或临时记事本里做。

**Go 条件**：

- 至少记事本、Chrome/Edge、一个 Electron 正例能稳定读到文字并提取单一连续新增。
- 基线快照能在不阻塞注入的前提下完成，或证明 `MicrophoneOpenRequested` 的提前量足够。
- 敏感目标与无树目标能被稳定拒绝。
- 日志不包含文字内容。

**No-go 处理**：如果只有少数应用可读，先把范围收缩为“只在可读应用记录”，并把限制写进产品文档；不要上 OCR、剪贴板兜底或注入。若连正例都不稳定，停止实现，回到方案评审。

### Phase 2：核心模型、存储与设置（2–3 天）

- `sayall-core/src/transcripts.rs`：
  - 模型、日文件、应用键、校验、排序、7 天策略、分组。
  - 文本连续差异纯函数。
  - 单条 8000 字符、单日/总量上限。
  - 单元测试覆盖正常、空、超长、非法日期、非法应用键、重复记录、乱序、跨时区。
- `src-tauri/src/transcripts.rs`：
  - 归档读写、原子写、坏文件跳过、图标缓存、回收站删除、错误码。
  - 测试用临时目录；删除用可注入的 `trashItem` 回调，避免测试真的动回收站。
  - 真实回收站删除另做手工人验收。
- 设置：
  - `AppSettings.local_transcript_history_enabled` 与 `local_original_audio_recording_enabled` 默认 false；schema 7；schema 6 及更早配置迁移测试。
  - `SettingsStore` 保存/读取。
- 测试：`cargo test -p sayall-core -p sayall-windows-app transcripts`。
- 提交：`feat(reflections): add transcript model and archive store`。

**完成判据**：模型/存储/设置全绿；坏文件和磁盘错误不会丢用户数据；删除只走回收站。

### Phase 3：捕获接入语音生命周期（3–5 天）

- `transcript_capture.rs`：
  - 专用 UIA 工作线程/任务队列，有界等待。
  - 基线快照、轮询、稳定判定、取消、generation、下一段结算。
  - 敏感目标过滤、应用身份、图标提取。
  - 结构化日志。
- `WindowsPlatform`：
  - 新增 `transcript_capture`、`transcript_capture_enabled` 字段。
  - `BleRuntime` 构造时传入捕获运行时。
  - `MicrophoneOpenRequested` → `begin_pending`；`StreamStarted` → `bind_session`；`StreamStopped` → `finish`；断连/停止/退出 → `cancel`。
  - `PlatformRuntime` 增加 `set_transcript_capture_enabled`、`set_transcript_capture_sink`（默认 no-op，仿真平台不受影响）。
- 应用宿主：
  - 创建归档写入线程和捕获 sink；保存成功发 `transcript-captured`。
  - `sayall-windows` 只定义 `TranscriptSink`（接收 `CapturedTranscript`），不依赖 Tauri/归档；app host 实现 sink，内部只往归档工作线程发消息，避免 UIA 线程做磁盘 IO。
  - 启动时按设置开启/关闭。
- 测试：
  - 假时钟 + 假快照的状态机单测：正常保存、稳定等待、超时、下一段结算、目标切换、敏感目标、关闭开关、迟到回调、重复调用、连续会话、8000 字符上限。
  - 日志断言不含文字。
- 提交：`feat(reflections): capture transcripts from voice sessions`。

**完成判据**：本地开发构建里，真实语音输入能在记事本/Chrome 产生一条正确记录；微信/WorkBuddy 静默跳过；语音注入和音频不因捕获变慢。

### Phase 4：IPC 与页面（3–4 天）

- `src-tauri/src/transcripts.rs` 命令与事件。
- `src/lib/bridge.ts` 类型/命令。
- `ReflectionsPage.vue` + 侧栏图标/路由 + scoped 样式。
- 前端测试：
  - 页面头/开关/空态/关闭但有记录。
  - 应用筛选/展开/收起/7 天提示。
  - 日期折叠/最新一天展开/切换应用重置。
  - 复制成功与失败、删除三种确认、取消。
  - 长文字换行、应用名截断、图标回退。
  - 深浅色、窄窗口、无横向滚动。
  - 加载失败/保存失败/回收站失败。
- `pnpm test`、`pnpm build`。
- 提交：`feat(reflections): add reflections page`。

**完成判据**：设计稿的所有状态在浏览器/开发构建中可复现；页面无技术词、无未实现入口（用功能开关控制）。

### Phase 5：验证、出包与真机验收（2–4 天）

- 自动化：
  - `cargo fmt --all -- --check`
  - `cargo test --workspace`
  - `cargo check -p sayall-windows-app --features runtime-simulation`
  - `pnpm test`
  - `pnpm build`
  - `scripts/ci-preflight.ps1`
- Runtime simulation：
  - 在隔离状态目录预置回眸数据，走页面旅程：导航 → 筛选 → 折叠 → 复制 → 删除确认 → 空态。
  - 断言不出现设备地址、路径、文字内容日志。
- 本地测试包：`scripts/build-local-test-package.ps1`；按仓库规则先出本地包，不自动发布。
- 真机手册 `Testing/WindowsReflections.md`：
  - RC001、RC003 分别执行。
  - 冷态首用、连续会话、快速连按、断连、睡眠恢复、切应用、目标关闭、开关中途关闭。
  - 正例：记事本、Chrome/Edge、DimAgent、VS Code。
  - 反例：微信聊天窗口、WorkBuddy、密码框、地址栏、搜索框、只读网页。
  - 复制、删除单条/当前应用/全部删除，回收站恢复。
  - 长文字、8000 字符边界、浅/深色、100%/150% 缩放、最小窗口。
  - 卸载/升级后 `%LOCALAPPDATA%\SayAll\transcripts` 保留；重装仍能读到原记录。
  - 日志抽查：一次报障 + 一次日志拉取能定位到环节。
- 记录结果：passed / failed / deferred，不把自动化当硬件验收。
- TODO、产品文档、ATTRIBUTION 同步。
- 验收通过后把 `REFLECTIONS_PAGE_ENABLED` 改为 true；再按仓库流程开 PR。

## 7. 测试矩阵

### 7.1 自动化（必须）

| 层 | 用例 |
| --- | --- |
| core | 日文件 schema、应用键安全、非法日期/重复/乱序、7 天策略、单条 8000 上限、总量上限、连续差异（插入/替换/清空/多处修改/前后缀相同） |
| storage | 写入/读取、原子写、坏文件跳过、权限/磁盘错误、删除单条/应用/全部、回收站失败不删、图标缓存 |
| capture | 基线重试、稳定 900 ms、8 s 超时、下一段结算、目标切换、敏感目标、无树目标、关闭开关、发送后清空/占位文案保留最后候选、generation、迟到回调、重复调用 |
| settings | 默认关闭、持久化、旧配置保持关闭、schema 7、音频开关默认关闭、保存失败回退 |
| recording | WAV 头/时长/字节数、空录音丢弃、关闭不写、取消清目录、队列溢出丢弃、manifest 校验、路径安全 |
| frontend | 入口、两个开关、空态、7 天提示、应用筛选/网格、日期折叠、复制、删除三种确认（单条/应用/全部）、播放/停止、导出、静默刷新（首次快速读取与后续刷新都不闪整页加载卡）、错误、长文字、图标回退、深浅色、导航顺序 |
| simulation | 页面旅程与隔离数据；日志脱敏断言 |

### 7.2 真机（deferred 直到本地包）

- RC001 / RC003 分别跑完整用例。
- 正例与反例应用按 6.5 节。
- 冷态首用、连续、连按、断连、睡眠、切应用。
- 回收站恢复。
- 100% / 150% DPI、浅/深色、最小窗口。
- 日志无文字、无路径、无窗口标题。

## 8. 风险与应对

| 风险 | 影响 | 应对 |
| --- | --- | --- |
| UIA 读不到文字或选区 | 功能价值下降 | Phase 1 先做探针；只对可读应用记录；不引入 OCR/剪贴板兜底 |
| 基线快照拖慢注入 | 用户感觉语音变慢 | 捕获与注入解耦；专用线程 + 有界等待；基线晚到就放弃本次 |
| 用户手动编辑被误记 | 记录不准确 | 焦点身份 + 连续差异 + 稳定窗口；不连续就放弃 |
| 输入法 composition 中间态 | 记录半截或重复 | 900 ms 稳定 + 只接受单一连续新增；不连续放弃 |
| UIA 跨进程卡死 | 捕获线程挂住 | 专用工作线程 + 超时；失败不影响语音；日志可见 |
| 打包应用归属错误 | 图标/应用名错误 | 优先窗口 AUMID；回退进程 AUMID/exe；测试 ApplicationFrameHost |
| 回收站删除失败 | 用户以为删了 | 失败保留原文件并显示错误；不永久删除 |
| 卸载/升级误删历史 | 用户数据丢失 | 归档放在 `%LOCALAPPDATA%\SayAll\transcripts`；安装生命周期测试断言卸载保留；安装器不得新增删除该目录的规则 |
| 隐私泄漏 | 用户不信任 | 不存路径/标题/URL/全文；日志只写字符数；测试断言 |
| 大历史加载慢 | 页面卡顿 | 单日/总量上限；首版加载有上限；后续再分页 |
| 功能开关未验收就露出 | 用户看到半成品 | `REFLECTIONS_PAGE_ENABLED` 默认 false；验收后单独提交打开 |
| 范围蔓延到 MCP | 首版拖长 | 明确非目标；录音已完成第二阶段，MCP 另立阶段 |

## 9. 工作量估算

文字版 + 原始录音；不含 MCP：

| 阶段 | 估算 |
| --- | --- |
| Phase 0 设计确认 | 0.5–1 天 |
| Phase 1 捕获探针 | 1–2 天 |
| Phase 2 模型/存储/设置 | 2–3 天 |
| Phase 3 捕获接入 | 3–5 天 |
| Phase 4 IPC/页面 | 3–4 天 |
| Phase 5 自动化/出包/真机 | 2–4 天 |
| **合计** | **11.5–19 天** |

估算不含等待设计确认、等待真机、CI 排期和发布。录音另估 2–4 天；MCP/Agent 访问另估 5–10 天，且需要独立安全评审。

## 10. 完成定义

- 设计稿获得明确确认。
- 文字版实现完成，修改范围与任务直接相关，没有无关重构。
- 自动化测试覆盖正常、失败、重复调用、超时、取消、迟到回调。
- 真实 Windows 上验证 RC001/RC003；未完成的部分明确标 `deferred`。
- 日志能区分成功、失败、取消、超时、重试、恢复和迟到回调，且不含文字/路径/标题。
- 删除只走回收站；关闭开关不删除历史。
- `TODO.md`、`docs/product-copy.md`、`Testing/WindowsReflections.md`、`ATTRIBUTION.md` 已同步。
- 本地测试包路径、SHA-256、验证命令和未覆盖边界写进交付说明。
- 真机验收通过前不把入口默认打开，不把自动化结果说成真机通过。

## 11. 需要你确认的点

1. 首版范围：本轮按文字版开始；录音与 MCP 留到后续阶段。
2. 侧栏位置：已定「推荐」之后、「权限」之前。
3. 最近 7 天计数 + 应用总数并存（本稿）。
4. 超长文字完整换行（本稿）。
5. 空态页脚保留（本稿）。
6. 设计稿按当前版本开发（2026-10-25 用户已说“开始做”）。

确认后从 Phase 1 开始；Phase 1 的结论会决定是否继续 Phase 3 的捕获实现，而不是先写页面再补逻辑。
