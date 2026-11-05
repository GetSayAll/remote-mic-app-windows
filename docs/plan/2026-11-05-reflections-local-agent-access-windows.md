# Windows「回眸 · 本地 Agent 访问」实施计划（对齐 Mac）

- 日期：2026-11-05
- 状态：**方案已确认，按 M1 → M4 实施中**（M1 已落地：core + `sayall-mcp` + 打包暂存）
- 参考实现：Mac 仓库 `D:\SayAll\src\GetSayAll\remote-mic-app`（只参考行为与数据边界，不复制 Swift 源码）
  - 界面：`Sources/RemoteMic/TranscriptAgentAccessSection.swift`（358 行）
  - 状态：`Sources/RemoteMic/TranscriptAgentAccessModel.swift`（364 行）
  - 客户端接入：`Sources/RemoteMic/MCPClientIntegrationService.swift`（416 行）
  - 授权库 / 历史 / 审计 / 配置：`Sources/SayAllMCPKit/*.swift`
  - MCP 进程：`Sources/SayAllMCP/main.swift`（342 行，stdio JSON-RPC）
  - 面向 Agent 的说明：`AI_SETUP.md` / `AI_SETUP.en.md`
- Windows 现状：`docs/product/reflections.md` 明确「暂不做本地 Agent 或 MCP 访问」；`docs/product-copy.md` 有一条「应用内不出现 Agent 访问入口」。本期同时修订这两条边界。
- 相关：`TODO.md` 回眸条目、`Testing/WindowsReflections.md`、ADR 0002（可选能力必须独立、默认关闭）

## 0. 一句话方案

在回眸页底部（「全部删除」之上）复刻 Mac 的「本地 Agent 访问」卡片：一个默认关闭的总开关；开启后可以给 Codex / Claude Code / Cursor / OpenCode 四条 Quick Connect 或手动创建只读授权，授权以「本机只读 MCP」形态让 AI 工具读取回眸记录；Windows 侧用**独立进程 `sayall-mcp.exe`（stdio MCP）**+ `%LOCALAPPDATA%\SayAll\mcp\v1\access.json`（只存令牌哈希）实现，令牌只在生成配置的那一次显示。

## 0.1 已确认的决策（2026-11-05 用户）

| 项 | 决定 | 说明 |
| --- | --- | --- |
| 标题 | **与 Mac 一致**：本地 Agent 访问（英文 Local Agent Access） | 不做去术语化改写 |
| 配置里的可执行路径 | **与 Mac 一致**：直接写安装目录里的绝对路径（`%LOCALAPPDATA%\无线麦 SayAll\sayall-mcp.exe`） | 不做 ASCII 副本；Windows 特有的只是路径本身（中文 + 空格），TOML 转义必须正确 |
| 「令牌」措辞 | **与 Mac 一致**：用户可见文案使用「访问令牌」 | `docs/product-copy.md` §3 的禁用词表要为这张卡片加一条例外说明（用户必须知道这个字符串要保管、只显示一次） |
| 本期范围 | 手动配置 **与** 四客户端快速连接**都做** | 不拆期 |
| Codex 是否改名 ChatGPT | **不改**：Windows 上就叫 Codex | 本机实测：OpenAI 的本机工具装在 `%LOCALAPPDATA%\OpenAI\Codex\bin\<hash>\codex.exe`，配置在 `%USERPROFILE%\.codex\config.toml`（已存在）。ChatGPT 桌面版是另一个应用，Windows 侧本地 MCP 配置没有稳定的公开口径（第三方文档给出 `%APPDATA%\OpenAI\ChatGPT\…` 与 `%APPDATA%\ChatGPT\mcp.json` 两种互斥说法），因此不纳入本期；需要时再单独调研 |


## 1. 目标（逐条复刻 Mac 行为）

1. 回眸页底部一张卡片：标题 + 一句说明 + 右侧「允许访问」开关（默认关闭）。
2. 开关下方常驻一句隐私说明：回眸记录不上传；被授权的 AI 工具可能把读到的文字发给自己的服务商；关掉开关后所有工具立即读不到。
3. 关闭时：只显示隐私说明 + 「已授权客户端」列表（空态一句）。
4. 开启时追加三段：连接 AI 工具（2×2 客户端网格）、其他客户端或手动配置、生成出来的配置（可复制两种格式）。
5. 每个客户端卡片显示：真实应用图标、名称、状态（可连接 / 已连接 · 请重新打开 X / 未检测到安装 / 位置已变化）、右侧按钮（连接 / 移除 / 进行中转圈）。
6. 手动配置：输入名称（1–100 字）→「创建只读授权」→ 展示配置 + 两个复制按钮（标准 MCP JSON / Codex TOML），复制成功 1.5 秒内显示「已复制」。
7. 「已授权客户端」列出全部有效授权：名称、创建时间、撤销按钮（撤销后从列表移除；撤销记录保留在状态文件里但不展示）。
8. 所有失败都在卡片底部给一句人话（不弹窗），且能区分：客户端没装 / 命令行入口起不来 / 配置同名冲突 / 配置不是合法 JSON / 配置路径不安全 / 写配置失败 / 客户端拒绝安装 / 自动连接失败 / 撤销成功但配置没删掉 / 重复授权。
9. 快速连接是事务：**先预检、后建授权**；写配置失败必须丢弃这次临时授权；同一客户端最多一条有效授权。
10. 关闭开关立即断掉所有客户端（不是延迟生效），不需要重启无线麦；连接后要求用户重开 AI 工具。
11. 只有用户点「连接」时才会写第三方配置；改名、改路径、卸载都不静默改动第三方文件。
12. 全程不需要管理员权限。

## 2. 非目标

- 不写回眸记录：MCP 只读，不提供写入 / 修改 / 删除工具。
- 不做网络传输、云同步、远程访问；MCP 只在 stdio（本机父进程管道）上跑。
- 不做「让 Agent 直接操作遥控器 / 按键 / 语音」。
- 不读取第三方应用的私有配置或内部数据库；只按公开文档写它们的 MCP 配置文件。
- 不复刻 Mac 的 `path_changed` 以外的跨端迁移能力（迁移、iCloud 等都不做）。
- 不在本期支持 VS Code / Windsurf / Cline 等更多客户端（结构留扩展位）。

## 3. Mac 现状基线（要复刻的判定逻辑）

### 3.1 状态模型（`TranscriptAgentAccessModel`）

| 状态 | 含义 | 触发 |
| --- | --- | --- |
| `isEnabled` | 总开关 | `setEnabled` 写 store；失败回读真值 + 报错 |
| `authorizations` | 有效授权（`revokedAt == nil`） | `refresh()` / 增删后重读 |
| `clientName` | 手动配置输入 | 输入框 v-model |
| `generatedConfiguration` | 刚生成、还没复制完的配置（含明文令牌，只在内存） | 创建授权 / 连接时产生 |
| `copiedConfiguration` | 哪个格式已复制（1.5 s 后自清） | 复制按钮 |
| `integrationInProgress` | 正在连接的客户端（按钮转圈，全局互斥） | Quick Connect 进行中 |
| `failedClient` | 哪个客户端失败了（用于把 `%@` 填进错误句） | 失败时 |
| `authorizationsNeedingReconnect` | 可执行文件路径哈希与当前不一致的授权 | `refresh()` 时比对 |

### 3.2 快速连接事务（含 Mac 已修的坑）

Mac `Bugs/mcp-quick-connect-failure-duplicates.md` 的结论必须原样继承：

1. `preflight(client)`：检测安装 → 检查配置可写 / 无同名 `sayall_history` / JSON 合法 / CLI `--version` 返回 0。
2. `createAuthorization(displayName, integrationIdentifier, helperPath)`：同一 `integrationIdentifier` 有有效授权时报 `.duplicateClient`；同名有效授权同样拒绝。
3. 生成配置 → `install(client, config)`。
4. 安装失败 → `discardAuthorization(clientId)`（真删；丢弃失败再尝试 revoke），错误上报为「本次授权没有保留」。
5. 状态文件里保留历史撤销记录，界面只显示有效授权。

另一条 Mac 教训必须继承：`Bugs/codex-mcp-invalid-toml-escaping.md` —— **生成 TOML 不能复用 JSON 编码器**（JSON 会把 `/` 写成 `\/`）。Windows 路径全是反斜杠，这条更关键：`\` 必须转成 `\\`，且不能出现 `\/`。

### 3.3 磁盘与认证

- 记录根：`<Support>/RemoteMic/Transcripts/v1`；访问状态：`<Support>/RemoteMic/MCP/v1/access.json`；审计：`.../audit/YYYY-MM-DD.ndjson`。
- `access.json`：`{ schemaVersion: 1, enabled, authorizations: [{ clientId, displayName, integrationIdentifier?, scope: "transcripts.read.all", tokenHash: sha256hex, helperExecutablePathHash?, createdAt, revokedAt? }] }`，目录 0700 / 文件 0600。
- 令牌：32 字节随机 → base64url 无填充；只存 SHA-256；校验时按字节常量时间比较。
- MCP 进程启动即 `requireAuthorized(clientId, token)`：开关关闭 / 授权缺失或撤销 / 令牌不符 → 直接失败退出（stdout 不产出任何数据）。
- 审计：每次 `server_start` / `list_transcript_apps` / `query_transcripts` 写一行 JSON（不写文字，只写条数与筛选条件）。

### 3.4 MCP 契约（stdio，行分隔 JSON-RPC）

- 方法：`initialize`（协议版本协商，支持 2025-11-25 / 2025-06-18 / 2025-03-26 / 2024-11-05 / 2024-10-07）、`ping`、`tools/list`、`tools/call`。
- 工具两个，都是只读（`readOnlyHint: true`）：
  - `list_transcript_apps` → `{ applications: [{ applicationName, bundleIdentifier, recordCount, earliestEndedAt, latestEndedAt }], skippedFileCount }`
  - `query_transcripts`（`startedAtOrAfter` / `endedAtBefore` / `bundleIdentifiers` ≤100 / `order` ascending|descending / `limit` 1–500（默认 100）/ `cursor`）→ `{ records: [...], nextCursor, hasMore, skippedFileCount }`
- 错误以 `tools/call` 的 `isError: true` + `{ code, message }` 文本返回，不用 JSON-RPC 异常。
- 记录字段：`id, startedAt, endedAt, localDateKey, timeZoneIdentifier, applicationName, bundleIdentifier, source, text`。

### 3.5 客户端矩阵（Mac）

| 客户端 | 检测 | 写入方式 | 配置位置 |
| --- | --- | --- | --- |
| Codex | `codex` 或 App bundle | 托管区块（`# BEGIN/END SAYALL MANAGED MCP sayall_history`） | `~/.codex/config.toml` |
| Claude Code | `claude` 可执行 | `claude mcp add-json --scope user sayall_history <json>` | 由 CLI 自己管 |
| Cursor | App bundle | JSON 合并 `mcpServers.sayall_history` | `~/.cursor/mcp.json` |
| OpenCode | App bundle | JSON 合并 `mcp.sayall_history`（`type: local`） | `~/.config/opencode/opencode.json` |

写前一律：拒绝符号链接、拒绝非法 JSON、已存在同名则冲突报错、改动前留私有备份、写后收紧权限。

## 4. Windows 差异与决策

### D1：MCP 服务载体 = 独立 `sayall-mcp.exe`（新增 workspace 成员）

- 新建 `crates/sayall-mcp`（bin），只依赖 `sayall-core` + `serde_json`；不依赖 Tauri、不依赖 `sayall-windows`（保持「core 无平台 API」边界）。
- 理由：主 exe 是 GUI 子系统 + 单实例互斥体 + 托盘 + BLE，`--mcp` 子命令必须先于 Tauri 初始化分流，且升级时被客户端占用会锁住主程序。独立 exe 与 Mac 的 `Contents/Helpers/SayAllMCP` 同构。
- 控制台子系统（`#![windows_subsystem]` 不设），但**不创建窗口**；stdout 只写 JSON-RPC（诊断走 stderr），与 Mac 一致。

### D2：配置里写安装目录的绝对路径（与 Mac 同构）

- NSIS `currentUser` 安装目录：`%LOCALAPPDATA%\无线麦 SayAll\`，`sayall-mcp.exe` 与主程序、助手同目录。
- 配置里直接写 `<安装目录>\sayall-mcp.exe`，与 Mac 写 `Contents/Helpers/SayAllMCP` 的绝对路径是同一种做法。
- 因此 `access.json` 仍保留 `executablePathHash`：安装目录变了（换用户、装到别处）时卡片提示「位置已变化，请先移除这个连接，再重新连接」。
- 已确认不采用「复制到 ASCII 数据目录」的方案：多一份副本就多一处不同步，且与 Mac 行为不一致。
- 必须配套：TOML 里反斜杠要转义成两个反斜杠（见 D5 与 `mcp_config` 的测试），否则含中文/空格的 Windows 路径会让 Codex 拒绝整个配置。

### D3：认证状态文件（Windows 版 `access.json`）

- 路径：`%LOCALAPPDATA%\SayAll\mcp\v1\access.json`；审计：`...\v1\audit\YYYY-MM-DD.ndjson`；二进制：`...\v1\bin\`。
- 结构照 Mac（`schemaVersion: 1`），**不写进 `settings.json`**（安装生命周期测试会对设置文件做哈希断言，见 `launch_state.rs` 的同一理由）。
- 写入：临时文件 + 原子替换；读取时校验 schema 与字段（`displayName` ≤100、`tokenHash` 64 位小写 hex、`revokedAt >= createdAt`、clientId 唯一），任何不合法整体拒绝（不猜测修复）。
- 权限：`%LOCALAPPDATA%` 默认只允许当前用户 / SYSTEM / Administrators。**实现第一步先 `icacls` 实测一次并记进证据**；若发现继承权限过宽，用 `SetFileSecurityW` 收紧到当前用户（需要 `windows` crate 的 security 特性）。
- 令牌：32 字节随机（`BCryptGenRandom` 或 `getrandom`）→ base64url；只存 `sha256`；比较用常量时间。**令牌绝不出现在日志、诊断包、`settings.json`、审计文件**。

### D4：客户端矩阵（Windows 版）

| 客户端 | 检测（按顺序，命中即止） | 写入 |
| --- | --- | --- |
| Codex | `where.exe codex` → `%APPDATA%\npm\codex.cmd` → `%LOCALAPPDATA%\Programs\codex` | 托管区块写入 `%USERPROFILE%\.codex\config.toml`（TOML 正确转义 `\`） |
| Claude Code | `where.exe claude` → `%APPDATA%\npm\claude.cmd` → `%USERPROFILE%\.local\bin\claude.exe` | `claude mcp add-json --scope user sayall_history <json>`；`.cmd` 需经 `cmd /c` 启动 |
| Cursor | `where.exe cursor` → `%LOCALAPPDATA%\Programs\cursor\Cursor.exe` | JSON 合并 `%USERPROFILE%\.cursor\mcp.json` 的 `mcpServers` |
| OpenCode | `where.exe opencode` → `%APPDATA%\npm\opencode.cmd` → `%LOCALAPPDATA%\Programs\opencode` | JSON 合并 `%USERPROFILE%\.config\opencode\opencode.json` 的 `mcp`（**路径待实测确认**） |

- 每条都要「检测不到 → 卡片显示未检测到安装、按钮置灰」；检测结果缓存 30 秒，切进页面刷新一次。
- 图标：Cursor 走既有 `shortcut_icons` / Shell 提取；Codex / Claude Code / OpenCode 多为 CLI，无图标时用中性占位图标（不复刻 Mac 的 SF Symbol）。

### D5：文案以 Mac 原文为准，只有 Windows 说不通的地方才改

已确认：标题、状态句、按钮、错误句都取 Mac 中文原文（含「访问令牌」）。`docs/product-copy.md` §3 的禁用词表要为这张卡片加一条例外：**「令牌」在本卡片里必须出现**（用户要保管一个只显示一次的字符串），但仍不得出现端口、进程、助手 / Helper、注入、提权等机制词。

Mac 原文里有两处 Windows 说不通，必须改：

| Mac 原文 | Windows 写法 | 为什么 |
| --- | --- | --- |
| 当前 App 中缺少本地 MCP Helper，请重新安装完整测试包。 | 当前安装缺少本地读取组件，请重新安装完整安装包。 | 「Helper / 助手」在 Windows 文案红线里禁用 |
| 无线麦SayAll.app 不会上传记录… | 无线麦不会上传记录… | 应用名以 Windows 版为准（`docs/product-copy.md` 名称表） |

其余照抄 Mac（标题「本地 Agent 访问」、开关「允许访问」、连接段、状态「已连接 · 请重新打开 {名称}」「未检测到安装」「位置已变化，请先移除这个连接，再重新连接。」、按钮「连接 / 移除 / 撤销 / 创建只读授权 / 复制标准 MCP JSON / 复制 Codex TOML / 已复制」、列表「已授权客户端 / 还没有客户端获得访问权限。」、16 条错误句）。

英文同步取 Mac `Resources/en.lproj/Localizable.strings` 的同名条目（`statistics.transcripts.agent_access.*`），只替换应用名。

### D6：与既有回眸开关的关系

- 「记录回眸」控制**是否新增记录**；「允许访问」控制**是否可读**。两者独立：关闭记录不影响 Agent 读取已有记录；关闭访问不影响记录继续写入。
- 页面在没有记录时也要显示这张卡片（Mac 在空态时同样渲染，且在「全部删除」之上）。
- 文案必须写清「关闭后所有工具立即无法读取」，避免用户以为关掉记录就等于关掉访问。

### D7：错误码走结构化值，不进用户可见文本

IPC 返回 `Result<T, AgentAccessCommandError>`，错误体 `{ code: "client_unavailable", clientId?: "cursor" }`；前端按 code 选文案并填 `{名称}`。理由：Windows 侧错误分类多（15 条），把中文拼在 Rust 里会同时破坏 i18n 与「不贴内部错误」两条规则。

## 5. 页面设计

### 5.1 位置与容器

回眸页 `ReflectionsPage.vue`：`<footer class="history-footer">` 之前插入 `<section class="card agent-access">`（与 `app-switcher` 同为 `.card`，不引入新容器风格；根不可滚，卡片自身不需要内部滚动）。

### 5.2 线框（开启态）

```
┌─ card ────────────────────────────────────────────────────────────────┐
│ 本地 AI 工具访问                                    [ 允许访问  ● ]    │
│ 允许你授权的 AI 工具通过本机只读连接查看回眸记录。                      │
│                                                                       │
│ 回眸记录不会上传。被授权的 AI 工具可能把读到的文字发给自己的服务商。     │
│ 开关默认关闭，关闭后所有工具立即无法读取。                              │
│ ───────────────────────────────────────────────────────────────────── │
│ 连接 AI 工具                                                          │
│ 无线麦会为所选工具单独创建只读授权，并写入它的本机配置。连接后请关闭      │
│ 并重新打开这个工具；不需要重启无线麦。                                  │
│ ┌───────────────────┐ ┌───────────────────┐                            │
│ │ [icon] Codex      │ │ [icon] Claude Code│                            │
│ │ 已连接 · 请重新打开 │ │ 可连接     [连接] │                            │
│ │            [移除] │ │                   │                            │
│ └───────────────────┘ └───────────────────┘                            │
│ ┌───────────────────┐ ┌───────────────────┐                            │
│ │ [icon] Cursor     │ │ [icon] OpenCode   │                            │
│ │ 未检测到安装       │ │ 可连接     [连接] │                            │
│ └───────────────────┘ └───────────────────┘                            │
│ ───────────────────────────────────────────────────────────────────── │
│ 其他工具或手动配置                                                     │
│ 自动连接不可用时，创建一份授权并复制对应格式。配置里的授权信息只显示一次。│
│ [ 例如 Codex 或 Claude Desktop    ] [ 创建只读授权 ]                   │
│ ┌───────────────────────────────────────────────────────────────────┐ │
│ │ 🔑 已为“Codex”生成配置                                             │ │
│ │ 配置里的授权信息只在这次显示。现在就粘贴到对应工具，不要保存到公开   │ │
│ │ 文件或聊天里。                                                     │ │
│ │ [复制标准配置] [复制 Codex 配置]                                    │ │
│ └───────────────────────────────────────────────────────────────────┘ │
│ ───────────────────────────────────────────────────────────────────── │
│ 已授权工具                                                             │
│  ⌐ Codex        2026-11-05 14:20                              [撤销]  │
│  ⌐ Claude Code  2026-11-05 14:20                              [撤销]  │
│  （空态）还没有工具获得访问权限。                                       │
│                                                                       │
│ 无法写入 Cursor 的配置，检查文件权限后再试。                            │
└───────────────────────────────────────────────────────────────────────┘
```

- 关闭态：只保留标题 / 说明 / 开关 / 隐私句 / 分隔线 / 已授权工具（Mac 同构）。
- 开关写盘失败：回到真值 + 卡片底部一句错误。
- 首次读状态：卡片骨架占位（不整页 loading），避免开关闪一下再变。

### 5.3 交互细节（对齐 Mac + Windows 习惯）

- 开关动画与回眸页现有两个开关同款（`.toggle`，2026-10-24 起统一写法；首次渲染不做动画）。
- 客户端按钮：可用 = 主按钮（醒目底），已连接 = 常规按钮「移除」，进行中 = 置灰 + 转圈（不写文字）。
- 复制按钮：成功后按钮文字变「已复制」+ 对勾图标，1.5 s 后恢复。
- 快速连接进行中，其他客户端的「连接/移除」全部置灰（Mac 的全局互斥），避免并发写同一份配置。
- 键盘可达：卡片内所有按钮可 Tab 聚焦，`aria-label` 含客户端名；错误句 `role="status"`。
- 卸载/离开页面不做特殊处理：状态在 Rust 侧，返回时重新读快照（本页已有模块级浏览状态的先例，但**授权状态不缓存在前端**）。

## 6. 数据与安全模型

| 资产 | 位置 | 内容 | 保护 |
| --- | --- | --- | --- |
| 访问状态 | `%LOCALAPPDATA%\SayAll\mcp\v1\access.json` | 开关 + 授权（只存哈希） | 原子写；校验 schema；ACL 实测并记录；不进 `settings.json` |
| 审计 | `...\v1\audit\YYYY-MM-DD.ndjson` | clientId、工具名、结果、返回条数、筛选条件、错误码 | 不含文字；只追加 |
| MCP 副本 | `...\v1\bin\sayall-mcp.exe` | 与随包二进制同源 | 复制后校验 SHA-256 与随包一致，不一致不写进配置 |
| 明文令牌 | 只在内存 + 剪贴板 | 生成配置一次性展示 | 不落盘、不进日志、不进诊断包、不进审计 |

威胁与对策：

| 场景 | 对策 |
| --- | --- |
| 同机其他用户读 access.json | `%LOCALAPPDATA%` 用户隔离；实现时实测 ACL，异常则显式收紧 |
| 第三方进程伪造 clientId | 必须带令牌；只存哈希 + 常量时间比较 |
| 客户端配置被恶意替换成别的程序 | 只写我们自己的条目；已存在同名条目时冲突报错，不覆盖 |
| 用户撤销后客户端还连着 | 每次 MCP 调用都重新校验开关与授权（不是启动校验一次）→ 撤销/关闭立刻生效 |
| 通过符号链接把配置写到别处 | 写前 `lstat`/`FILE_ATTRIBUTE_REPARSE_POINT` 检查；解析后路径必须在预期根内 |
| 日志泄露内容 | 审计只写条数；应用日志只写事件与结果；令牌只存在于一次性配置字符串里 |

## 7. 后端实现清单

### 7.1 新增 / 修改文件

| 文件 | 内容 |
| --- | --- |
| `crates/sayall-mcp/Cargo.toml`、`src/main.rs` | bin：参数只接受 `serve`；读 `SAYALL_MCP_CLIENT_ID` / `SAYALL_MCP_ACCESS_TOKEN`；启动校验 → stdio 循环 |
| `crates/sayall-mcp/src/protocol.rs` | 行分隔 JSON-RPC：`initialize` / `ping` / `tools/list` / `tools/call`；**按行读 `\n`，去掉行尾 `\r`**；每条响应后 flush |
| `crates/sayall-mcp/src/tools.rs` | 两个工具的 schema 与参数校验（未知字段拒绝、`limit` 1–500、`order` 枚举、时间 ISO-8601） |
| `crates/sayall-core/src/agent_access.rs` | 纯逻辑：access 状态结构 + 校验、令牌生成/哈希/常量时间比较、授权增删（有效期唯一性）、错误码枚举 |
| `crates/sayall-core/src/transcripts.rs` | 抽出 `read_archive(root) -> { records, skipped_files }`（从 `src-tauri::transcripts::load_all_unlocked` 提取，保持跳过规则/大小上限/排序口径不变） |
| `crates/sayall-core/src/mcp_config.rs` | 生成标准 JSON 与 Codex TOML（**TOML 单独转义，不复用 JSON 编码器**） |
| `src-tauri/src/agent_access.rs` | 状态读写（原子写、ACL）、IPC 命令、客户端探测与配置写入（`codex` / `claude` / `cursor` / `opencode` 四路）、可执行副本同步、审计无关（审计由 MCP 侧写） |
| `src-tauri/src/lib.rs` | 注册新命令、`AppState` 增加 `AgentAccess` 句柄 |
| `src/lib/bridge.ts` | 类型 + 6 个 IPC 包装 |
| `src/pages/ReflectionsPage.vue` + 新建 `src/pages/ReflectionsAgentAccess.vue` | 卡片组件（建议拆成独立组件，页面只挂一行，便于单测） |
| `src/lib/i18n/en/reflections.ts` | 全部新文案的英文 |

### 7.2 IPC 契约

```
agent_access_snapshot() -> AgentAccessSnapshot
set_agent_access_enabled(enabled: bool) -> AgentAccessSnapshot
create_agent_authorization(display_name: String) -> CreatedAgentAuthorization
connect_agent_client(client: AgentClientId) -> AgentAccessSnapshot      // spawn_blocking
remove_agent_client(client: AgentClientId) -> AgentAccessSnapshot       // spawn_blocking
revoke_agent_authorization(client_id: String) -> AgentAccessSnapshot
```

```ts
type AgentClientId = "codex" | "claude-code" | "cursor" | "opencode";
interface AgentClientView { id: AgentClientId; displayName: string; available: boolean;
  connected: boolean; needsReconnect: boolean; working: boolean }
interface AgentAuthorizationView { clientId: string; displayName: string;
  integration?: AgentClientId; createdAtMs: number; needsReconnect: boolean }
interface AgentAccessSnapshot { enabled: boolean; clients: AgentClientView[];
  authorizations: AgentAuthorizationView[] }
interface CreatedAgentAuthorization { snapshot: AgentAccessSnapshot;
  standardJson: string; codexToml: string; displayName: string }
```

- 慢操作（`connect` / `remove`）走 `spawn_blocking`，命令超时 15 s（与 Mac 的 `commandRunner` 上限一致）；超时按 `client_command_unavailable` 或 `command_timeout` 上报。
- 所有命令遵守 AGENTS「外部调用成功判据」：不以退出码为准，要读回 `access.json` 与目标配置文件内容确认落盘。

### 7.3 MCP 输出字段映射（Windows 记录 → Mac 契约）

| Mac 字段 | Windows 来源 | 说明 |
| --- | --- | --- |
| `id` | `record.id` | 保持原样（UUID 字符串） |
| `startedAt` / `endedAt` | `startedAtMs` / `endedAtMs` | ISO-8601 UTC，毫秒截断到秒 |
| `localDateKey` | `localDateKey` | `YYYY-MM-DD` |
| `timeZoneIdentifier` | `utcOffsetMinutes` | 无 IANA 名，输出 `UTC+08:00` 形式并在 tool 描述里写明 |
| `applicationName` | `applicationName` | 已做产品名映射（微信/Chrome 浏览器…） |
| `bundleIdentifier` | 改名 `applicationKey` | Windows 用 exe 主名（`chrome`），不暴露路径；schema 描述同步改名 |
| `source` | `source.as_str()` | `bluetooth_remote` / `unknown` |
| `text` | `originalTranscript` | 唯一暴露的文字字段 |

> 工具 schema 与 Mac 有两处差异（多 `applicationKey`、`timeZoneIdentifier` 语义不同），必须在 tool 描述里写清；不为了「字段一字不差」伪造 bundle id。

## 8. 打包与安装

| 环节 | 改动 |
| --- | --- |
| `src-tauri/tauri.conf.json` | `bundle.resources` 增加 `sayall-mcp.exe` |
| `scripts/stage-bundle-resources.cjs` | 新增 `["target/release/sayall-mcp.exe", "sayall-mcp.exe"]`；arm64 版本按现有规则成对处理（要么都有、要么都没有） |
| `scripts/stage-bundle-inputs.cjs` | 计数与文档（当前「4/4 落地」）同步更新 |
| `scripts/verify-local-test-install.ps1` | 载荷集合增加 `sayall-mcp.exe`；断言安装目录里哈希与 staging 一致；`-ExpectMcpMarker` 可选断言二进制含特征串 |
| 升级 | 安装器替换随包副本；`%LOCALAPPDATA%\SayAll\mcp\v1\` 是数据目录，不随卸载删除（与 transcripts 一致），卸载后残留不影响；重新安装后开关与授权仍在（用户可见行为，需要在验收用例里写明） |
| 卸载 | 不静默改写任何第三方配置；如果用户没先移除连接，客户端会报连接失败——文档与卡片都要求先「移除」 |

## 9. 测试计划

### 9.1 Rust

- `sayall-core::agent_access`：schema 校验（坏版本、坏哈希、重复 clientId、撤销早于创建）、令牌 32 字节/base64url、哈希与常量时间比较、同一集成唯一、同名拒绝、丢弃语义（丢弃后列表里不出现）。
- `sayall-core::mcp_config`：标准 JSON 可被 `serde_json` 解析且结构正确；Codex TOML 断言**包含 `\\` 转义、不含 `\/`**（阳性对照：把 JSON 编码器换回来必须失败）；含中文与空格的路径用例。
- `sayall-core::transcripts::read_archive`：坏文件跳过计数、超过大小上限跳过、非目录/非法应用键跳过、日文件头不匹配跳过；与抽取前 `src-tauri` 行为逐条一致（同用例双跑）。
- `sayall-mcp`：真实子进程 + 临时数据目录（`std::process::Command`）——`initialize` 版本协商、`tools/list` 两项、`list_transcript_apps` 不含正文、`query_transcripts` 过滤/分页/游标/`limit` 越界、开关关闭时进程拒绝启动（退出码非 0 且 stdout 为空）、撤销后调用报 `authorization_unavailable`、审计行写对。
- `src-tauri::agent_access`：原子写与坏文件拒绝、`codex` 托管区块插入/替换/移除（重复插入不叠加）、JSON 合并保留其它条目、同名冲突不覆盖、非法 JSON 不写入、`.cmd` 走 `cmd /c`（用临时 `.cmd` 桩）、超时路径。

### 9.2 前端

- `ReflectionsAgentAccess` 组件：关闭态/开启态渲染、开关保存与失败回退、四个客户端状态矩阵（可连接 / 已连接 / 未检测到 / 位置已变化 / 进行中置灰）、手动创建后展示配置与两个复制按钮（`navigator.clipboard` 桩）、复制成功 1.5 s 自清（`vi.useFakeTimers`）、撤销后列表移除、错误码 → 文案映射逐条（含 `{名称}` 插值）。
- `reflections` 页集成：卡片出现在「全部删除」之上；无记录时也渲染。
- i18n：`src/lib/i18n.test.ts` 的字面量扫描会强制新增中文都有英文（无需额外写测试，但要保证全绿）。

### 9.3 安装版与真机

- `scripts/verify-local-test-install.ps1 -ExpectRevision` 通过（含 `sayall-mcp.exe`）。
- 安装版实测：开启开关 → 授权状态文件出现（结构正确、无明文令牌）→ 复制配置 → 用**真实客户端**（至少 Codex 与 Claude Code 各一次）连接 → `tools/list` 出现 `sayall_history` → 调用 `query_transcripts` 读回一条记录 → 关闭开关后再次调用立即失败。
- 真机（RC001 / RC003 无关，本功能不涉及遥控器）：可不跑；但**升级/卸载保留**与**撤销后立即失效**必须在安装版上各做一次。
- `deferred` 边界：非本机已安装客户端（Cursor / OpenCode）只做配置写入与回滚验证，**不宣称「客户端侧连上」**。

### 9.4 每个里程碑都要有的「判据能失败」对照

- 事务性：故意让写配置失败（只读目录），断言没有留下有效授权，且「已授权工具」不新增行。
- 只读：MCP 无任何写工具（`tools/list` 恰好两项，断言第三个工具不存在）。
- 撤销即时生效：不重启任何进程，撤销后下一次调用失败。
- TOML：把专用转义换回 JSON 编码，断言测试变红。

## 10. 实施顺序（建议四个里程碑）

| 里程碑 | 内容 | 完成判据 |
| --- | --- | --- |
| M1 只读 MCP 通道 | `sayall-core` 抽出 `read_archive` + `agent_access` 状态 + `sayall-mcp` 进程 + 打包暂存 | 真实子进程集成测试 8 条全绿；`cargo test --workspace`、`fmt --check` 通过；手工用 `echo` 管道调通 `initialize` / `query_transcripts` |
| M2 页面与授权管理 | IPC 六个命令 + 卡片组件（开关、手动创建、复制、列表、撤销、错误） | 前端组件测试全绿（含错误码映射）；浏览器预览三态截图；`vue-tsc` / `vite build` |
| M3 快速连接 | 四客户端探测、预检、写配置、回滚、位置哈希 | Rust 单测覆盖四客户端写入/冲突/回滚；真实 Codex + Claude Code 各连一次（本机 passed，另两条 deferred） |
| M4 交付 | 本地测试包、安装/升级/卸载验证、文档同步 | `verify-local-test-install.ps1` 全 PASS；开关/撤销/升级保留在安装版上实测 |

每个里程碑一个 commit，`main` 只经 PR 合入；本地测试包按 `RELEASING.md` 出，不发布。

## 11. 风险与未决问题

已确认（2026-11-05）：标题用「本地 Agent 访问」、路径写安装目录绝对路径、令牌措辞取 Mac 原文、手动配置与快速连接都做、Codex 不改名 ChatGPT。

仍需实测后定稿（写进 `docs/investigations/`）：

- OpenCode / Claude Code 在 Windows 上的真实配置位置与启动方式（含 `.cmd` 包装脚本的调用方式）。本机只有 Codex（`%LOCALAPPDATA%\OpenAI\Codex`），其余三条只能做「写入 + 回滚」验证，客户端侧连接标 `deferred`。
- `%LOCALAPPDATA%` 的实际 ACL 是否符合「仅当前用户可读」。
- 安装目录含中文时，Codex 能否正确加载托管 TOML 区块（用它自己的读回命令验证）。

## 12. 文档同步清单（实现时同批提交）

- `docs/product-copy.md`：删除「应用内不出现 Agent 访问入口」，新增本功能文案与红线说明。
- `docs/product/reflections.md`：把「暂不做本地 Agent 或 MCP 访问」改为「只读访问，见本节」并写清开关关系。
- `TODO.md`：新增条目（用户要求 + 实现 + 验证结论 + deferred 边界）。
- `Testing/WindowsReflections.md`：新增用例十（开关、手动授权、快速连接、撤销、升级保留、关闭即失效）。
- `ATTRIBUTION.md`：记录参考的 Mac 模块与提交（仅行为参考）。
- `docs/decisions/0004-local-agent-access.md`（可选）：把 D1–D3 固化成 ADR（独立进程、只读、默认关闭、状态文件不进 settings）。
