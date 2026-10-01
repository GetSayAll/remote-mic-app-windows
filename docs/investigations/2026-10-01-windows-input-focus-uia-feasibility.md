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
7. **微信 4.0 现状不支持通用扫描**：登录窗口没有暴露任何文本输入（无 TextPattern）。这是**首次实测证据**，直接支撑决策点 D1/D2：需要登录后复测聊天输入框；若仍无暴露，则微信只能走「只打开应用」或 opt-in 的位置点击兜底。
8. **约 100 ms 量级的读取成本**：所有成功目标的单次扫描都在 20–200 ms，跨进程调用未见 2 s 超时现象。⇒ 工作线程 + 总超时（3 s）+ `ConnectionTimeout` 下调的预算设计可行。

## 待补项（`deferred`，需要环境或人工参与）

| 项 | 为什么没做 | 复现方式 |
| --- | --- | --- |
| 微信聊天输入框是否暴露 | 当前只有登录窗口；不能替用户完成登录 | 登录微信并把光标点在聊天输入框后运行：`probe-uia-focus.ps1 -ProcessName Weixin -Broad` |
| 真实 Electron 冷启动建树时序（VS Code / ChatGPT / Claude） | 本机未安装这些应用 | 安装后运行 `probe-uia-focus.ps1 -ProcessName Code -Attempts 8 -AttemptGapMs 250` |
| 产品路径的 `SetFocus` + 读回验证 | 探针会抢占前台，本次只做只读探测，避免打断用户 | 计划 Phase 2 真机冒烟（记事本冷启动 + 热路径）执行 |
| 豆包/微信输入法场景 | 输入法运行期 UI 无 UIA provider（仓库既有实证），且本机未装豆包 | 沿用既有结论；实现完成后按计划 §8.2 真机矩阵验收 |
| RC001 / RC003 实体按键链路 | 与本调查无关 | 计划 §8.2 真机验收 |

## 与实施计划的衔接

- 计划 §5.4 的打分口径按本调查修正（Document 只读位、合成元素过滤、地址栏排除）。
- 计划 §5.7 的 Chromium 建树假设得到支持（无需 mac 式属性触发，UIA 调用本身即可），但「冷启动 Electron」仍是开放项。
- 决策点 D1/D2 的输入：微信 4.0 在登录窗口无文本输入暴露；**聊天输入框结论待登录后复测**，在拿到该数据前不启用位置点击兜底。
- D4（预置应用内置扫描）：记事本、Chrome/Edge、WebView2 已实测可用，可作为内置默认；微信待定。
