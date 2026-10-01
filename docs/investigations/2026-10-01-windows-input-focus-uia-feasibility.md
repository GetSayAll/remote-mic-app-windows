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
- `WM_NULL` 有响应 → 窗口未卡死。

⇒ **Electron 的 UIA 可用性是逐应用行为**，不能按「Electron 类」整体假设。产品必须按应用实测，并把「无树/无候选」当作正常失败路径处理（计划 §5.10 的 `no_candidate` / `not_accessible`），而不是异常。

### 顺带确认

- Chrome 在 MTA 下同样可读（4 个候选），且出现**最小化窗口的地址栏矩形 `-31697,-31923`**、离屏 `Document`、`value_readonly=true` 的网页根——再次印证第一轮的三条矩形/只读口径修正。
- 探针新增能力：`-ListWindows`（枚举进程全部顶层窗口）、`-ListChildren`（子窗口）、`-Wake`（发送 `WM_GETOBJECT(UiaRootObjectId)`）、minimized/cloaked 判定；可用 `powershell -Mta` 复核线程套间差异。

## 待补项（`deferred`，需要环境或人工参与）

| 项 | 为什么没做 | 复现方式 |
| --- | --- | --- |
| 微信聊天输入框的 opt-in 兜底路线 | 已确认聊天主窗口不暴露文本输入（第二轮） | 决策点 D1/D2 待定；实现后按 `Testing/WindowsInputFocus.md` 真机验收 |
| 真实 Electron 目标的冷启动建树时序（VS Code / ChatGPT / Claude） | 本机未安装这些应用；已用 DimAgent（正例）与 WorkBuddy（反例）替代验证 | 安装后运行 `probe-uia-focus.ps1 -ProcessName Code -Attempts 8 -AttemptGapMs 250` |
| 产品路径的 `SetFocus` + 读回验证 | 探针会抢占前台，本次只做只读探测，避免打断用户 | 计划 Phase 2 真机冒烟（记事本冷启动 + 热路径）执行 |
| 豆包/微信输入法场景 | 输入法运行期 UI 无 UIA provider（仓库既有实证），且本机未装豆包 | 沿用既有结论；实现完成后按计划 §8.2 真机矩阵验收 |
| RC001 / RC003 实体按键链路 | 与本调查无关 | 计划 §8.2 真机验收 |

## 与实施计划的衔接

- 计划 §5.4 的打分口径按本调查修正（Document 只读位、合成元素过滤、地址栏排除）。
- 计划 §5.7 的 Chromium 建树假设得到支持（无需 mac 式属性触发，UIA 调用本身即可；冷启动 Edge 首扫即得），但「无树应用」（WorkBuddy、微信聊天窗口）必须走失败路径而非重试到超时。
- 决策点 D1/D2 的输入：**微信 4.0 聊天主窗口不暴露任何文本输入**（第二轮实测），位置点击兜底或「只提示手动点击」需要拍板；在拍板前不启用兜底。
- D3（失败提示）得到强化：目标应用可能根本不提供 UIA（WorkBuddy 为实例），「无法读取该应用的界面」提示必须有，且不能表现为重试卡死。
- D4（预置应用内置扫描）：记事本、Chrome/Edge、WebView2、DimAgent 已实测可用，可作为内置默认；微信不适用。
- Phase 2 的验证要覆盖两类目标：有树（记事本/浏览器/WebView2）与无树（WorkBuddy/微信聊天窗口），确保无树时快速失败并给出原因，而不是耗满 3 s 预算。
