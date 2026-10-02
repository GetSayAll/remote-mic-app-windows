# Windows「聚焦输入框」UIA 路线可行性调查（2026-10-01）

- 目的：为「聚焦输入框」功能（计划：`TODO.md:14`；实施计划见会话计划稿）确定候选识别规则、重试窗口与降级策略提供实测依据。
- 结论用途：计划 §5.4「识别与打分」、§5.7「Chromium/Electron 建树」、决策点 D1/D2（自绘应用降级）。
- 工具：`Testing/probe-uia-focus.ps1`（一次性诊断脚本，只读，不随产品发布）；原始输出见 `docs/investigations/evidence/2026-10-01-input-focus-uia-probe.md`。
- 边界：本调查只覆盖公开 UI Automation 的读取能力，**不覆盖**真实遥控器、产品内聚焦动作与最终文字上屏（属真机验收，见计划 §8.2）。

## 结论速览

| 目标 | 判定 | 关键数据 |
| --- | --- | --- |
| 记事本（WinUI，最小化/非前台） | **可用** | 1 个候选：`Document` / `RichEditD2DPT`，非只读；首扫 203 ms，后续 34–37 ms |
| Chrome（运行中，非前台） | **可用** | 2 个候选：`Edit`/`OmniboxViewViews`（地址栏，须排除）、`Document`/`RootWebArea`；首扫 81 ms，后续 47–53 ms |
| Edge（冷启动新实例） | **可用** | 首次 `FindAll` 即返回 4 个候选（70 ms）；HTML `<textarea id="ta">` 暴露为 `Edit` 且 **AutomationId = HTML id** |
| SayAll 自身（Tauri / WebView2） | **可用** | `Document` / `RootWebArea`；19–45 ms |
| 微信 4.0（Weixin.exe，登录窗口） | **未通过** | 整棵树 28 个元素，仅 Button/Text/Group/Pane/ToolBar/Custom；**TextPattern 可用数 = 0，Edit/Document = 0** |

本机目标应用安装情况：Edge、Chrome、微信 4.0（`D:\Apps\Weixin\Weixin.exe`）已安装；**VS Code、ChatGPT、Claude、豆包、Cursor 未安装**。

## 对实现有直接影响的设计结论

1. **跨进程焦点读取可用**：`AutomationElement.FocusedElement` 返回真实系统焦点（实测返回了另一个进程的元素），「焦点元素是否属于目标进程」是有效判据；这正是「学习输入框」捕获与「聚焦成功」读回判定的基础。
2. **扫描不需要目标在前台**：`FromHandle` + `FindAll` 在目标为后台窗口（Chrome/记事本/SayAll）时均成功且耗时稳定。⇒ 实现上「先扫描、后激活、再 SetFocus」的顺序可行；但产品语义仍要求「目标在前台」才允许发送聚焦（避免把焦点设到用户没在看的窗口）。
3. **候选口径需要三条修正**（相对计划初稿）：
   - `Document` **不能**用 `ValuePattern.IsReadOnly` 当排除条件：网页根 `RootWebArea` 读到的就是只读；
   - 需要过滤**合成元素**：Edge 树里存在 `Edit`/`Textfield`、不可聚焦、矩形非有限的占位元素，必须排除（至少要求 `IsKeyboardFocusable` 为真，且矩形有限且为正尺寸）；
   - **地址栏必须靠语义排除**：Chrome/Edge 的 Omnibox 是 `Edit`、非只读、可聚焦，只在名称/类名上可区分（`OmniboxViewViews`、`view_*`、名称长度 6 的「地址和搜索栏」）。
4. **建树延迟在预算内**：Chromium 系（Chrome/Edge/WebView2）首个 `FindAll` 即拿到完整候选，冷启动 Edge 首扫 70 ms；原生 WinUI 记事本首扫 203 ms。⇒ 重试窗口不必按 mac 的 2.75 s 起步，可先用 **8 × 200 ms**（1.6 s）并保留总预算上限；真实 Electron 冷启动仍待补测（见下）。
5. **`BoundingRectangle` 必须先做有限性/正尺寸校验**：最小化的记事本返回离屏坐标（`-31991,-31888`），Chromium 存在非有限矩形（`nonfinite`）的合成元素。⇒ 归一化矩形只作次要匹配项，且转换前必须判 `is_finite` 与 `> 0`。
6. **`AutomationId` 在 Chromium 与微信上都可用**：Chromium 暴露 HTML `id`；微信暴露点分路径 id（如 `login_layout_...current_login_nick_name`）。⇒ 「学习输入框」记录 AutomationId 有实际区分度，应作为最高权重匹配项。
7. **微信 4.0 不支持通用扫描（两轮实测）**：登录窗口暴露 28 个自定义控件但没有文本输入；**登录后的聊天主窗口整棵树只有 2 个 `Pane`**（0 个 Edit/Document、0 个 TextPattern）。⇒ 微信只能走「只打开应用」或 opt-in 的位置点击兜底（详见第二轮实测与决策点 D1/D2）。
8. **约 100 ms 量级的读取成本**：所有成功目标的单次扫描都在 20–200 ms，跨进程调用未见 2 s 超时现象。⇒ 工作线程 + 总超时（3 s）+ `ConnectionTimeout` 下调的预算设计可行。

## 第二轮实测（同日补充：微信登录后 + Electron 目标）

### 微信 4.0（已登录，聊天主窗口）

| 观测 | 结果 |
| --- | --- |
| 窗口类 | `Qt51514QWindowIcon`（Qt 5.15）+ 子窗口 `MMUIRenderSubWindowHW` |
| 整棵树 | **2 个元素（2 × `Pane`）**，`TextPattern` = 0，可聚焦 = 0，无 Edit/Document |
| 补充尝试 | 直接探子窗口 `MMUIRenderSubWindowHW` → 0 个元素；`powershell -Mta`（MTA 线程）→ 结果不变；窗口 `WM_NULL` 有响应（未卡死） |

对照：**登录窗口**曾暴露 28 个元素（Button/Text/Group/Pane/ToolBar/Custom，带点分 AutomationId 如 `login_layout_...current_login_nick_name`）。
⇒ 微信 4.0 具备 UIA provider，但**聊天主窗口不暴露文本输入**：「扫描 Edit/Document 找输入框」在微信上不成立。这是决策点 D1/D2（opt-in 位置点击兜底或仅提示手动点击）的直接实测依据。

### 内容可编辑输入框：TipTap / ProseMirror 暴露为 `Group`（DimAgent，2026-10-02 补充）

用户报告「DimAgent 打开后无法聚焦到输入框」，实测定位到**算法口径问题**（可修）：

| 观测 | 结果 |
| --- | --- |
| 当前焦点元素 | `ControlType.Group`，`class_name = "tiptap ProseMirror outline-none ProseMirror-focused"`（Chromium 把 DOM class 原样暴露），`focusable=true`、`has_focus=true`、矩形 1417×133（窗口底部） |
| 我的既有候选扫描（只认 Edit/Document） | **只找到 1 个** `Document`/`RootWebArea`——真正的输入框被漏掉 |
| 宽口径扫描 | 363 个元素：150 个带 TextPattern、133 个可聚焦；可聚焦元素里既有 `Group`（编辑器容器）也有大量 `Button`（侧栏/工具条） |
| `SetFocus` 到 `RootWebArea` | 调用成功、读回一致（`has_focus=true`）——说明 UIA 设焦点可行，但聚焦网页根 ≠ 聚焦输入框 |

⇒ **硬门槛不能只认 Edit/Document**。需要扩展为「可聚焦 + 具备可编辑/文本特征 + 语义或几何合格」，并把「当前焦点元素（若属于目标进程且通过硬门槛）」作为首选目标。已记入实现待办（`focus.rs` 的 `passes_hard_gate` / 打分口径）。

### Electron 目标

| 目标 | Electron 版本 | 树规模 | 判定 |
| --- | --- | --- | --- |
| DimAgent | Electron（`Chrome_WidgetWin_1`） | **685 个元素**：120 个带 TextPattern、288 个可聚焦、1 个 `Document`/`RootWebArea`；扫描 20–120 ms | **可用（正例）** |
| WorkBuddy（腾讯） | Electron 37.10.3（安装目录 `version` 文件） | **2 个元素（2 × `Pane`）**，0 候选、0 TextPattern、0 可聚焦 | **不暴露（反例）** |

WorkBuddy 的补充尝试（全部无效）：

- 子窗口 `Chrome_RenderWidgetHostHWND` 存在且可见 → 直接探它：0 个元素；
- 向目标窗口与全部子窗口发送 `WM_GETOBJECT(UiaRootObjectId)`（4/4 消息被处理）→ 树不建立；
- 置前台（`foreground=true`）→ 结果不变；
- `powershell -Mta`（MTA 线程）→ 结果不变；
- `WM_NULL` 有响应 → 窗口未卡死；**操作人确认窗口显示正常**（不是白屏/暂态）；
- 主进程与渲染进程命令行均无无障碍相关开关（`--disable-features` 只含 `ScreenAIOCREnabled,SpareRendererForSitePerProcess,WinDelaySpellcheckServiceInit`）。

⇒ 判定为**应用级行为**（具体原因未定位，可能是应用自身关闭了无障碍支持）。产品不需要知道原因：**Electron 的 UIA 可用性逐应用不同**，不能按「Electron 类」整体假设；必须按应用实测，并把「无树/无候选」当作正常失败路径处理（计划 §5.10 的 `no_candidate` / `not_accessible`），而不是异常。

### 顺带确认

- Chrome 在 MTA 下同样可读（4 个候选），且出现**最小化窗口的地址栏矩形 `-31697,-31923`**、离屏 `Document`、`value_readonly=true` 的网页根——再次印证第一轮的三条矩形/只读口径修正。
- 探针新增能力：`-ListWindows`（枚举进程全部顶层窗口）、`-ListChildren`（子窗口）、`-Wake`（发送 `WM_GETOBJECT(UiaRootObjectId)`）、minimized/cloaked 判定；可用 `powershell -Mta` 复核线程套间差异。

## 2026-10-02 追加：产品路径 `SetFocus` + 读回真机实测（计划 Phase 2）

探针换成产品代码自身：`crates/sayall-windows/examples/focus_probe.rs` 直接调用
`focus_windows` 的扫描 / 聚焦 / 读回；目标窗口保持在前台。

| 场景 | 目标 | 结果 |
| --- | --- | --- |
| 候选扫描与选择 | Chrome（独立 profile + 本地 fixture：顶部 `autofocus` 搜索框、底部 `chat-input` 文本域） | 候选 2 个：搜索框（`Edit`、`focused=true`）与 composer（`Edit`、`rect.y≈0.86`、宽 0.97）；`focus_frontmost` 选 composer 而不是搜索框 |
| 焦点迁移（A/B 对照） | 同上 | `--index 0` 把焦点放到搜索框 → 独立读回确认；再 `focus_frontmost` → 读回的元素身份哈希变化，落在 composer。`attempts=1`、`elapsed_ms=87..116` |
| 幂等 | 同上 | 焦点已在 composer 时重复执行仍为 `Focused`，不会误报迁移 |
| TipTap / ProseMirror | DimAgent | 候选 1 个：`Group`（`text_pattern=true`、`focusable=true`、`focused=true`、宽 0.485）＝用户报障的输入框；只读网页根被过滤 |
| 普通 Win32 | 记事本 | 候选 1 个：`Document`（`readonly=false`、`text_pattern=true`、宽 0.979） |
| 自身/无输入应用 | 本应用前端窗口 | 候选 0 个（只读网页根被过滤）→ 走 `no_candidate` |

**本轮新发现（已回写实现）**

1. **Chromium 网页根节点是只读 `Document`**：`ControlType.Document` +
   `CurrentIsReadOnly=true` + `text_pattern=true`，能通过原硬门槛并在缺少更强候选时被
   选中（聚焦它不能输入、还会抢走真输入框）。硬门槛补 `read_only == Some(true)` 直接
   拒绝，`None`（拿不到 ValuePattern）保持宽松。复测：Chrome fixture 候选 3 → 2，
   DimAgent 侧只剩 TipTap 的 `Group`。
2. **DPI 感知影响坐标**：UIA 工作线程固定 Per-Monitor V2（物理像素），元素矩形与窗口
   矩形才在同一坐标系；沿用 `send_input_windows` 的 Get/Set + 成对恢复写法。复测归一化
   矩形与改动前一致。
3. **读回判据两段式**：先看 `CurrentHasKeyboardFocus`，再用 `GetFocusedElement` +
   `CompareRuntimeIds` 交叉确认（SAFEARRAY 由调用方释放）。实测两者一致；只凭
   `CurrentHasKeyboardFocus` 不足以排除「窗口未激活但元素自报焦点」的应用。
4. **无树应用仍走失败路径**：WorkBuddy 本轮未在运行，沿用第一轮结论（0 元素）；无
   provider 的应用在 `ElementFromHandle`/`FindAll` 失败时立即 `not_accessible`，不做长重试。

## 待补项（`deferred`，需要环境或人工参与）

| 项 | 为什么没做 | 复现方式 |
| --- | --- | --- |
| 微信聊天输入框的 opt-in 兜底路线 | 已确认聊天主窗口不暴露文本输入（第二轮） | 决策点 D1/D2 待定；实现后按 `Testing/WindowsInputFocus.md` 真机验收 |
| 真实 Electron 目标的冷启动建树时序（VS Code / ChatGPT / Claude） | 本机未安装这些应用；已用 DimAgent（正例）与 WorkBuddy（反例）替代验证 | 安装后运行 `probe-uia-focus.ps1 -ProcessName Code -Attempts 8 -AttemptGapMs 250` |
| 产品路径的 `SetFocus` + 读回验证 | ~~未做~~ **2026-10-02 已做**（Chrome fixture / DimAgent / 记事本，见上一节） | `cargo run -p sayall-windows --example focus_probe -- --frontmost` |
| 属性缓存请求（`FindAllBuildCache`）降低跨进程调用 | 当前整条路径 87–116 ms 已达标；缓存对部分 provider 可能返回空值，需按应用实测 | 有性能需求时再改，改前先复核成功判据 |
| 豆包/微信输入法场景 | 输入法运行期 UI 无 UIA provider（仓库既有实证），且本机未装豆包 | 沿用既有结论；实现完成后按计划 §8.2 真机矩阵验收 |
| RC001 / RC003 实体按键链路 | 与本调查无关 | 计划 §8.2 真机验收 |

## 与实施计划的衔接

- 计划 §5.4 的打分口径按本调查修正（Document 只读位、合成元素过滤、地址栏排除）。
- 计划 §5.7 的 Chromium 建树假设得到支持（无需 mac 式属性触发，UIA 调用本身即可；冷启动 Edge 首扫即得），但「无树应用」（WorkBuddy、微信聊天窗口）必须走失败路径而非重试到超时。
- 决策点 D1/D2 的输入：**微信 4.0 聊天主窗口不暴露任何文本输入**（第二轮实测），位置点击兜底或「只提示手动点击」需要拍板；在拍板前不启用兜底。
- D3（失败提示）得到强化：目标应用可能根本不提供 UIA（WorkBuddy 为实例），「无法读取该应用的界面」提示必须有，且不能表现为重试卡死。
- D4（预置应用内置扫描）：记事本、Chrome/Edge、WebView2、DimAgent 已实测可用，可作为内置默认；微信不适用。
- Phase 2 的验证要覆盖两类目标：有树（记事本/浏览器/WebView2）与无树（WorkBuddy/微信聊天窗口），确保无树时快速失败并给出原因，而不是耗满 3 s 预算。
- **Phase 2 状态（2026-10-02）**：纯逻辑层、重试编排与 UIA 后端已完成并真机验证（见上节）；属性缓存与「日志事件」并入 Phase 3 的接入工作，避免后端先行落日志造成重复。
