# APP 适配（Windows）界面设计与功能计划

- 日期：2026-11-09
- 状态：聚焦、滚动区域与预设可用（2026-11-11）已实现，自动化验证通过；逐应用预置滚动规则与部分真实应用验收仍为 `deferred`；未发布
- 参考实现：`HD838A/remote-mic-app`，`origin/main`
- 设计稿：[2026-11-09-app-adaptation.html](../artifacts/design/2026-11-09-app-adaptation.html)
- 设计板截图：[2026-11-09-app-adaptation.png](../artifacts/design/2026-11-09-app-adaptation.png)
- 窗口单屏截图：[2026-11-09-app-adaptation-frame.png](../artifacts/design/2026-11-09-app-adaptation-frame.png)
- 实现态有内容截图：[2026-11-09-app-adaptation-populated.png](../artifacts/design/2026-11-09-app-adaptation-populated.png)

## 1. 目标与边界

### 1.1 目标

在 Windows 版新增独立侧栏页「APP 适配」，复刻 macOS 的 APP 级作用域：

- 输入框聚焦按 APP 保存，APP 在前台即可使用。
- 每个 APP 拥有独立的聚焦方式、学习记录与快捷键。
- 按键页只选择「打开哪个 APP」，不复制可写的聚焦控件。
- 从 APP 详情反查绑定到它的按键，并可跳回按键页。
- 可删除某个 APP 的适配；有绑定时阻止删除。

### 1.2 本轮交付

- 界面设计稿（HTML + PNG）。
- 本功能计划（信息架构、数据契约、实现顺序、验证、风险）。
- 不修改 `src/`、`crates/`、`src-tauri/` 下的生产代码。
- 不创建发布 Tag、不发布安装包。

### 1.3 已确认决策

1. 首版范围：只做界面与计划；用户在查看设计后要求「开始做吧」，因此继续实现聚焦首版。
2. APP 列表：只列用户主动添加的 APP。M0 审计发现本机已有 `netease_music` 显式档案（`open_only`）；为避免配置被隐藏，带显式档案的内置预设也会出现在列表，但使用探测到的真实可执行文件路径展示，不显示「内置」标签；识别仍复用预设 target，避免改写既有按键与组合动作引用（2026-11-09 用户反馈后调整）。2026-11-11 用户要求：APP 适配左列只列用户手动添加的 APP；手动添加的路径若匹配预设，标注「预置可用」并直接使用预设聚焦/翻页能力，未手动添加的预设不显示。用户添加的 APP 集中到按键页「目标 APP」分组。

### 1.5 实现结果（2026-11-09）

已实现：

- 侧栏「APP 适配」页、左列列表、右侧详情、空态。
- 用户添加 APP + 有显式档案的内置预设列表；添加 APP 复用现有扫描对话框。
- 预设应用探测真实可执行文件路径（App Paths / System32 / 安装候选 / 开始菜单 / AppsFolder），列表显示真实路径而不是「内置预设」。
- 应用图标改为 Shell 提取真实 logo（EXE / Store Shell 项）；图标不再包圆角背景框，提取失败才显示通用应用图标，不显示首字母色块。
- 左列状态摘要对齐 Mac：聚焦短名 + 翻页状态（例如「只打开 · 需学习」「已记录输入框 · 需学习」）。选中项保存在窗口级会话，切换侧边栏回来不重置到第一项。
- 详情头部不再显示大图标或 APP 路径；绑定区不再显示「0 处」计数和删除说明；绑定与删除同卡、横线分隔，删除按钮使用文字链接样式并收紧上下间距。
- 回眸、组合动作、APP 适配的删除与撤销文字统一使用 `--delete-text`（对齐 macOS SwiftUI `Color.red` 系统红）；改这一组主题变量即可全局调整。
- 聚焦主选项收敛为 Mac 同款两个：只打开 APP / 打开后自动聚焦；自动选择输入框、记录点击位置不再提供入口，旧档案保留执行并显示旧版提示。
- 输入框学习窗口改为 3 秒，文案、按钮、成功/失败提示与 Mac 对齐；未记录时「测试输入框」禁用，不再提供清除记录按钮。
- 聚焦方式五种策略中保留快捷键兜底；成功修改不再弹顶部提示，只保留内联状态与错误提示。
- 按键页和组合动作页改为只读摘要 + 「去配置」；可写控件统一在 APP 适配页。
- 绑定反查（基础按键配置）、删除/恢复默认适配、跨页导航。
- 滚动区域首版：APP 详情内学习（3 秒内点击内容区，超时用当前光标）、已学习/需学习状态、测试向上/向下翻页与到两端、清除记录；档案按 APP 保存到 `scrollRegions`，schema 升到 5。
- 翻页执行优先 UIA `ScrollPattern`；失败时向区域内命中的子窗口发送 `WM_MOUSEWHEEL`，不移动光标，并在可读回时用滚动百分比验证。新增 `page_up` / `page_down` / `scroll_to_top` / `scroll_to_bottom` 动作。
- 窗口默认与最小内容尺寸保持 1066.67 × 800（不缩小用户已合入的最小高度）；启动时显式按逻辑像素重设默认/最小尺寸并居中，避免 Tauri 初始窗口在高 DPI 会话里落到错误尺寸。
- 2026-11-11 预设与自定义入口对齐 Mac：按键页「打开应用」改为「打开 APP」只列预设，末尾「打开自定义 APP」展开「目标 APP」分组；自定义 APP 才显示「聚焦：… · 翻页：…」摘要与「去配置」。
- 2026-11-11 组合动作「聚焦学习过的输入框」步骤复用同一套分组与摘要：预设网格、手动添加的 App 收进「目标 APP」、预置路径显示「预置可用」，自定义路径保留「聚焦：… · 翻页：…」+「去配置」，可写聚焦控件仍只在 APP 适配页。执行侧组合动作与按键页统一按「预设优先」解析聚焦档案，手动添加的预设路径不会退化为「只打开」。（2026-11-13 修订：改为显式档案优先、预设默认兜底。）
- 2026-11-11 聚焦步骤不再要求预先配置聚焦档案：只要选择目标 App 即可进入下一步；档案缺失或为「只打开 APP」时，执行只打开或激活目标 App，不再把步骤判为缺少依赖。
- APP 适配中手动添加的预设详情改为只读「预置能力」卡（聚焦方式 + 翻页，均标注「预置可用」），不显示学习/清除；未手动添加的预设不进入列表。自定义 APP 保留原编辑卡。组合动作页移除「我的组合动作」小节标题，数量移到主标题右侧，数量为 0 时隐藏。
- 2026-11-13 修订（1.9 回归修复）：显式聚焦档案重新优先于预设默认（含预设 id / 真实路径两种键）；APP 适配页对微信等 `auto_composer` 预置或已有显式档案的预置恢复「记录点击位置」学习；组合动作页为预置目标显示聚焦摘要与「去配置」；显式导航到未加入应用库的预置时只显示详情、不写入应用库。记录见 `Bugs/2026-11-13-wechat-focus-learning-regression.md`。
- 2026-11-14 用户要求：预置 APP 与普通 APP 一致，移除只读「预置能力」卡；聚焦方式五种策略都可选，预置默认策略显示为选中并标「预置可用」；滚动区域同样显示「预置可用」，允许学习覆盖，清除后恢复预置。
- 中英文文案与 1066×800 空态浏览器走查。
- 收尾：组合动作页删除残留的可写聚焦编辑器死代码；编辑器在改选策略/切换目标时作废进行中的学习与迟到回调，录入启动与卸载均成对释放后端捕获。

未实现 / deferred：

- 预置滚动规则：首版全部 APP 都从「需学习」开始，不显示虚假的「预置可用」。
- 私有键位方案的绑定反查。
- 微信 4.x 等完全不暴露滚动树的 APP 只能走滚轮回退，无法读回验证；真机逐应用验收仍为 `deferred`。

验证命令与结果见 §6；本轮自动化结果以实际执行记录为准。

### 1.4 非目标（首版）

- 不重做按键页画布、手势识别、按住连发。
- 不新增 APP 分组、排序、搜索。
- 不新增 APP 级导入导出。
- 不把「打开应用」拆成多个动作。
- 不在本页编辑按键自定义快捷键或常用语。
- 翻页滚动引擎首版已接入；预置规则仍单列后续阶段，不与聚焦首版捆绑。

## 2. 现状与依据

### 2.1 macOS 参考

- 产品规范：`remote-mic-app@origin/main:feature/mapping-scope-separation/PRODUCT_SPEC.md`
  - 四类作用域：按键 × 手势、按键、APP、遥控器型号。
  - APP 适配页拥有独立页面标题、左侧 APP 列表、右侧详情。
  - 聚焦能力按优先级呈现；快捷键是兜底，不与主选项平级。
  - 详情包含聚焦、滚动区域、绑定反查、删除适配。
- 页面实现：`remote-mic-app@origin/main:Sources/RemoteMic/SettingsView.swift`
  - `SettingsSection.applications` 位于 `.mapping` 之后。
  - `applicationsPage` 使用 `HStack(alignment: .top)`：左列 246，右侧自适应。
  - 空列表不显示左列。
  - `CustomApplicationProfile` 包含 `focusStrategy`、`focusShortcut`、`accessibilityTarget`、`scrollRegionTarget`。
- 本地化：`Resources/zh-Hans.lproj/Localizable.strings`、`Resources/en.lproj/Localizable.strings`
  - 侧栏中文「APP 适配」；英文侧栏短写「Apps」。
  - 状态短标签：只打开 / 快捷键 / 已记录输入框；预置可用 / 已学习 / 需学习。

### 2.2 Windows 现状

以下为实施前的现状快照（2026-11-09）：

| 事实 | 位置 |
| --- | --- |
| 窗口默认与最小 `1066.67 × 800`，侧栏 130，内容宽约 861 | `src-tauri/tauri.conf.json`、`src/styles.css` |
| 当前无「APP 适配」页、无导航项 | `src/navigation.ts`、`src/App.vue` |
| 按键页内嵌完整聚焦编辑器：策略、学习输入框、记录点击位置、APP 快捷键、测试 | `src/pages/ButtonsPage.vue:2744` 起 |
| 组合动作页也复制了一套聚焦编辑器 | `src/pages/MacrosPage.vue:2030` 起 |
| 聚焦档案按 `open_app` 的 target 键控，与 `applications` 解耦 | `src/lib/bridge.ts:430`、`crates/sayall-windows/src/send_input.rs:875` |
| 五种聚焦策略：`open_only`、`auto_composer`、`app_shortcut`、`recorded_element`、`recorded_point` | `src/lib/focus-defaults.ts`、`crates/sayall-windows/src/focus.rs` |
| 预设应用有默认聚焦：Chrome `Ctrl+L`、网易云 `Ctrl+F`，其余 `auto_composer` | `src/lib/focus-defaults.ts` |
| 已有 IPC：打开应用、学习输入框、记录点击位置、测试聚焦、扫描本机应用 | `src/lib/bridge.ts:1618-1680` |
| 没有任何滚动区域 / 翻页动作实现 | 全仓搜索 `ScrollPattern`、`scrollRegion`、`scroll_page` 无产品实现 |
| 键位方案是私有可选组件；公开仓库只有空壳 | `src/private/windows-button-profiles/index.ts`、`crates/sayall-windows-button-profiles/src/lib.rs` |

### 2.3 关键结论

Windows 的 `focusProfiles` 和 `applications` 是两张表：

- `applications`：用户添加的 APP 库（路径或 `registered:` AUMID）。
- `focusProfiles`：按 target 保存的聚焦档案；预设应用也可以有档案。

因此新页面必须明确「列表来源」和「档案来源」不是同一件事。首版按用户决定只列 `applications`；存量预设档案的可见性作为实现前置门禁（见 §8）。

## 3. 界面设计

设计稿：`artifacts/design/2026-11-09-app-adaptation.html`（PNG 同目录）。

### 3.1 信息架构

侧栏顺序：

1. 按键
2. **APP 适配**（新增，位于按键之后）
3. 组合动作
4. 推荐
5. 连接
6. 回眸
7. 设置
8. 个人中心（底部）

图标：四宫格（对齐 Mac `square.grid.2x2`），18 × 18 描边。

页面标题：中文「APP 适配」；英文「Apps」。
页面副标题：中文「输入框聚焦与翻页滚动按 APP 生效，APP 在前台即可使用。」

### 3.2 页面布局

内容布局按 1066 宽设计；窗口默认与最小高度 800，内容超出时内容区滚动：

- 标题栏 32 px。
- 侧栏 130 px。
- 内容区左边距 32 px、右边距 35.5 px，内容宽约 861 px。
- 页头：标题 24 px，副标题 13 px muted；页头下间距 12 px。
- 主体：左侧列表 228 px + 间距 14 px + 右侧详情自适应（约 619 px）。
- 左右两栏顶部对齐；左列高度随 APP 数量增长，右列由内容决定；整页在 `.content` 内滚动。
- 卡片：沿用 `--card`、`--border`、`--shadow-card`、12 px 圆角。
- 详情内分区：`--surface-subtle` 底色、10 px 圆角、1 px 边框、10–12 px 内边距。

### 3.3 左列

- 卡片标题「已添加的 APP」，16 px。
- 每行 26 px 真实图标（不加圆角背景框）+ 名称 + 一行状态；行高约 42 px。
- 状态短标签（对齐 Mac compact status：聚焦短名 · 翻页状态）：
  - 聚焦：`只打开` / `快捷键` / `已记录输入框`；旧版档案可能显示 `自动选择` / `点击位置`
  - 翻页：`预置可用` / `已学习` / `需学习`；Windows 尚无滚动引擎，当前统一显示 `需学习`
  - 组合：`只打开 · 需学习`、`已记录输入框 · 需学习`
- 选中项使用 `--accent-surface` 背景 + `--accent-border` 边框。
- 列表末尾固定「＋ 添加 APP」；打开现有 `RegisteredAppsDialog`。
- 列表按添加顺序排列；本阶段不提供排序与搜索。
- 列表为空时整页不显示左列，右侧显示空态。

### 3.4 右列详情

固定顺序：

1. **APP 头**
   - 36 px 图标、20 px 名称、11 px 路径（单行省略）。
   - 应用未找到时显示「应用未找到，请重新添加」。
2. **作用域说明**
   - 「输入框聚焦和上下翻页按 APP 单独设置：只要该 APP 在前台就能用，与“打开 App”动作无关。」
3. **聚焦方式卡**
   - 标题「聚焦方式」，不再显示状态 badge（2026-11-09 用户反馈）。
   - 主选项只有两个（对齐 Mac），选项中的图标与文字整体居中：
     - 只打开 APP
     - 打开后自动聚焦
   - 选中「打开后自动聚焦」时显示学习与测试（对齐 Mac）：
     - 说明「点击后会立即打开目标 APP。请在 3 秒内点击需要聚焦的输入框；无线麦 SayAll 只记录辅助功能特征，不读取输入内容。」
     - 主按钮「开始学习输入框」
     - 次按钮「测试输入框」，未记录时禁用
     - 状态「已记录目标输入框 / 尚未记录目标输入框」
     - 成功提示「输入框学习成功，可以直接测试打开与聚焦。」；失败提示「没有识别到可用输入框，请确认点击位置和辅助功能权限后重试。」
     - 未记录时补充「学习只需要在目标 APP 里点一下输入框，比录快捷键更快。」
   - 旧版 auto_composer / recorded_point 档案保留执行，但界面不再提供入口；选中时显示一行旧版策略说明，选择上方任一方式后覆盖。
   - 快捷键兜底行：
     - 「这个 APP 有自带的聚焦快捷键？也可以用它代替学习。」
     - 文字按钮「改用 APP 快捷键聚焦」独占下一行并允许换行。
     - 进入该策略后显示快捷键编辑器；录入区不再出现特殊按键快捷按钮，成功/失败提示放在左下， “改回打开后自动聚焦”靠右。
   - 不把「使用 APP 快捷键聚焦」做成与主选项并列的第三个主卡片。
   - 「测试打开与聚焦」固定在聚焦方式卡片右下角；学习区的「测试输入框」是另一个按钮。
4. **滚动区域卡（目标态）**
   - 标题「滚动区域」+ `预置可用 / 已学习 / 需学习`。
   - 说明「翻页键会滚动这个 APP 的内容区。只读取区域结构，不读取内容，也不移动焦点或鼠标。」
   - 按钮：学习滚动区域 / 测试向上翻页 / 测试向下翻页 / 清除学习结果或恢复预置。
   - 第二行：滚到两端（滚到顶部 / 滚到底部）。
   - 已启用此卡：没有记录时显示「需学习」；学习成功后显示「已学习」并启用测试与清除。首版没有预置规则，因此不会显示「预置可用」。
5. **绑定到它的按键卡**
   - 标题 + 数量 badge。
   - 每行：`上键 · 长按` + `打开应用` + `去按键页`。
   - 只读；点击行跳到按键页并打开对应按键编辑卡。
   - 空态：「还没有按键绑定这个 APP，可在按键页的“打开应用”里选择它。」
   - 绑定列表与删除适配放在同一张卡片内，中间用一条横线分隔（对齐 Mac）。
6. **删除适配**
   - 无绑定时显示红色文字按钮「删除这个 APP 的适配」。
   - 有绑定时改为说明：「还有 N 处绑定在使用它，先解绑才能删除适配。」
   - 二次确认文案说明只删除 SayAll 保存的适配（输入框学习记录、聚焦快捷键），不卸载或改动 APP 本身。

### 3.5 状态与交互

| 状态 | 界面 | 规则 |
| --- | --- | --- |
| 空列表 | 整页空态：标题「还没有添加任何 APP」、说明、主按钮「添加 APP」 | 不显示左列 |
| 有列表未选中 | 默认选中第一项 | 选中项在本次窗口内保持 |
| 应用未找到 | 详情显示原因；学习/测试禁用；允许删除 | 不静默移除档案 |
| 学习输入框 | 打开目标 APP；3 秒窗口；显示等待文案 | 成功后保存结构特征；失败保留原策略 |
| 旧版记录点击位置 | 不再提供入口 | 已有档案保留执行，详情显示旧版策略提示 |
| 快捷键录入 | 进入录入态；Esc 取消；窗口失焦取消 | 离开页面必须停止录入 |
| 绑定阻止删除 | 显示数量说明，不显示删除按钮 | 不静默解绑 |
| 删除确认 | 二次确认；确认后回退到列表第一项 | 只删适配 |

### 3.6 视觉与可访问性

- 字号：标题 24 / 分区标题 14 / 正文 12–13 / 辅助 11–11.5。
- 所有可点击项有 `aria-label` 或可见文本；列表使用 `aria-current`。
- 选中态不只靠颜色：使用边框 + 背景 + 单选框。
- 焦点环沿用全局 `:focus-visible` 规则。
- 中英文都要在 1066 × 800 下无横向滚动、无截断。
- 图标：首版用名称首字/首字母色块占位；真实文件图标作为后续增强，不阻塞页面。

### 3.7 与 Mac 的刻意差异

| 项目 | Mac | Windows 设计 | 原因 |
| --- | --- | --- | --- |
| 聚焦策略 | 两个主选项 + 快捷键兜底 | 两个主选项 + 快捷键兜底 | 2026-11-09 用户要求暂时移除 Windows 独有的自动选择 / 点击位置入口；旧档案仍可执行 |
| 列表 | 用户添加的 APP | 用户添加的 APP | 用户明确要求与 Mac 一致 |
| 滚动区域 | 已有（含预置） | 首版已实现学习 / 测试 / 清除；预置规则后续 | Windows UIA `ScrollPattern` 可用性与预置需逐应用验证 |
| 输入框学习时限 | 3 秒 | 3 秒 | 2026-11-09 用户要求与 Mac 一致，Windows 学习窗口同步改为 3 秒 |
| 权限 | 辅助功能权限 | 无单独权限提示 | Windows UIA 走现有进程权限，不伪造 Mac 文案 |
| 图标 | 系统真实图标 | 首版字母/首字占位 | 现有扫描 IPC 不返回图标 |

## 4. 数据与契约

### 4.1 首版（聚焦）

复用现有 `ButtonMappings`，不升 schema：

```ts
interface ButtonMappings {
  enabled: boolean;
  actions: Partial<Record<RemoteButton, ButtonActions>>;
  applications?: CustomAppPick[];
  focusProfiles?: Record<string, AppFocusProfile>;
}
```

- 列表来源：`applications`。
- 行 target：自定义应用路径或 `registered:` AUMID。
- 档案读取：`focusProfiles[target]`；没有显式档案 = `open_only`（自定义应用没有预设默认）。
- 添加 APP：`saveButtonMappings({ ...mappings, applications: [...uniqueApps] })`。
- 保存聚焦：只改 `focusProfiles[target]`，保留其它 target 与 `actions`。
- Rust `AppFocusProfile::normalized()` 保持现有语义：策略保留，缺失载荷清空，执行期给明确失败原因。
- 删除适配：
  - 先收集按键绑定与组合动作引用。
  - 有引用：阻止并显示数量。
  - 无引用：从 `applications` 移除，并删除 `focusProfiles[target]`。
  - 一次 `save_button_mappings` 完成，避免两次写盘造成半删除状态。

### 4.2 第二阶段（滚动区域）

不建议把滚动字段塞进 `focusProfiles` 的现有类型后直接复用：

- 新增 `scrollRegions: Record<string, ScrollRegionTarget>`，schema 升到 5。
- 与 `focusProfiles` 同一次保存、同一次删除。
- `ScrollRegionTarget` 只存结构描述：控件类型、AutomationId、ClassName、窗口标题、归一化矩形、匹配策略；不存文本内容。
- 翻页动作需要新增动作类型：`page_up`、`page_down`、`scroll_to_top`、`scroll_to_bottom`；`scroll`（滚轮格数）保持现状。
- 执行优先 UIA `ScrollPattern`：
  - 翻页：`LargeIncrement` / `LargeDecrement`。
  - 到两端：`NoAmount` + `Minimum` / `Maximum`。
  - 失败时回退为在区域中心注入滚轮；不移动键盘焦点。
- 内置预置只按真实应用逐项验证后再加；首版全部「需学习」，已在 schema 5 中预留
  `scrollRegions`（当前实现）与后续预置扩展。

### 4.3 跨页导航

新增一个明确的导航请求，不用全局单例或通知：

```ts
interface NavigationRequest {
  page: string;
  params?: {
    appTarget?: string;
    button?: RemoteButton;
    trigger?: ButtonTrigger;
  };
}
```

- `App.vue` 保存 pending request，切换页面时传给目标页。
- `AppsPage` 接受 `initialTarget`，用于按键页「去配置」定位。
- `ButtonsPage` 接受 `initialEditor`，挂载并读完 mappings 后自动打开对应按键编辑卡。
- 点击侧栏清除 pending request，回到默认行为。
- 选中 APP 在模块级变量中保留（本次窗口内有效）；删除当前 APP 后回退第一项。

### 4.4 单写入口

当前按键页和组合动作页都有一份可写聚焦编辑器。新页面不能成为第三份。

计划：

- 抽出 `AppFocusProfileEditor`（组件）与 `useAppFocusProfile`（保存/学习状态）：
  - props：`target`、`profile`、`fallback`、`isPreset`、`disabled`。
  - emits：`save(profile | null)`。
  - 内部负责策略选择、学习输入框、快捷键录入、测试；旧版记录点击位置只读保留。
  - 卸载时停止录入、清除定时器、把迟到回调按 generation 丢弃。
- 按键页：删除内嵌可写控件，保留只读状态摘要 + 「去配置」。
- 组合动作页：聚焦步骤只选择已存在的档案，不再提供学习/录入；需要修改时跳 APP 适配页。
- APP 适配页成为聚焦档案的唯一权威编辑位置。

### 4.5 私有键位方案

公开构建没有键位方案宿主：

- 首版绑定反查只覆盖基础 `mappings.actions`。
- 私有键位方案中的绑定反查需要宿主暴露稳定查询接口，不能由公开 UI 解析私有 JSON。
- 在私有组件提供接口前，明确标注为 `deferred`；不伪造「全部绑定」。
- 删除保护也要包含公开层能看到的组合动作引用；私有方案引用在接入后补齐。

## 5. 实现顺序

按 TDD：先写失败测试，再实现，最后补文档与截图。

### M0 设计确认与前置审计

1. 确认设计稿。
2. 增加 `TODO.md` 条目（用户可见功能；实现并验证后才标记完成）。
3. 审计存量 `focusProfiles` 的 key：
   - 是否有预设应用 id（`wechat`、`chrome` 等）的显式档案。
   - 数量与策略。
4. 若存在预设档案，先决定：
   - 方案 A：严格 Mac，列表不显示，档案仍可执行但不可编辑（有隐藏配置风险）。
   - 方案 B：只把这些预设作为「已配置的内置应用」加入列表（小偏差，避免隐藏）。
   - 方案 C：保留按键页只读摘要，不提供编辑。
   - 默认建议 B；需要用户确认后再实现。

**门禁：未完成审计，不进入 P2。**

### P1 页面壳与导航

文件：

- `src/navigation.ts`：新增 `apps` PageId、`NavIcon` 的 `apps`、侧栏项。
- `src/components/Sidebar.vue`：四宫格 SVG。
- `src/App.vue`：注册 `AppsPage`、导航请求。
- `src/pages/AppsPage.vue`：页头、空态、左右布局骨架。
- `src/lib/i18n/en/apps.ts`、`en.ts`、`en/nav.ts`、`en/page.ts`。

测试：

- 侧栏顺序：按键 → APP 适配 → 组合动作。
- 页面可进入、空态显示、无缺失英文 key。
- 1066 × 800 无横向滚动。

### P2 列表、添加与选择

文件：

- `src/lib/app-adaptation.ts`：纯函数。
- `src/pages/AppsPage.vue`：加载 mappings、列表、选择、添加。
- `src/components/RegisteredAppsDialog.vue`：复用；如需要，只加受控关闭/回调，不改扫描逻辑。

测试：

- 添加一个 APP 后列表出现、选中新项、`applications` 落盘。
- 重复路径不重复添加。
- 删除当前项后选中第一项。
- 空列表不渲染左列。
- 已有档案正常回读。

### P3 聚焦编辑器抽取

文件：

- `src/components/AppFocusProfileEditor.vue`
- `src/lib/use-app-focus-profile.ts`（或同目录 composable）
- `src/pages/ButtonsPage.vue`、`src/pages/MacrosPage.vue`：改为只读摘要 + 跳转。

测试：

- 五种策略切换与字段保留规则。
- 学习成功保存 `recorded_element`。
- 学习失败保留原策略与旧记录。
- 记录点击位置成功/失败。
- 快捷键录入、清除、Esc 取消、窗口失焦取消。
- 卸载/切页停止录入与定时器；迟到回调不写入新状态。
- 保存失败回滚并显示错误。

### P4 绑定反查、删除与跨页

文件：

- `src/components/AppBindingsSection.vue`
- `src/pages/AppsPage.vue`
- `src/pages/ButtonsPage.vue`（`initialEditor`）
- `src/App.vue`（导航请求）
- 删除确认对话框组件。

测试：

- 反查只包含当前基础 mappings 的 `open_app`。
- 点击绑定行导航并打开对应编辑卡。
- 有绑定时删除禁用且显示数量。
- 无绑定但有组合动作引用时阻止。
- 无引用时二次确认，确认后同时移除 `applications` 与 `focusProfiles`。
- 删除后按键页不再出现该目标。

### P5 清理与文档

- 按键页移除聚焦编辑器的可写控件、死代码与只被测试引用的函数。
- 组合动作页聚焦步骤改为选择已有档案。
- 更新用户手册相关章节（`docs/installation-and-configuration.md`）。
- 更新 `TODO.md`、测试文档、`design-qa.md` 相关条目。
- 中英文截图与文案审查。

### P6 滚动区域（首版已实现，2026-11-09 第二轮）

1. 先做可行性探针：
   - 对 Chrome、VS Code、微信、记事本、WorkBuddy 检查 UIA `ScrollPattern` 可用性与滚动百分比。
   - 结论写入 `docs/investigations/` 或 `Testing/`。
2. 新 Rust 模块：
   - `crates/sayall-windows/src/scroll_region.rs`：模型、匹配、状态。
   - `crates/sayall-windows/src/scroll_region_windows.rs`：UIA 与滚轮回退。
3. `send_input.rs`：schema 5、`scrollRegions`、四种翻页动作、规范化与迁移测试。
4. `focus_windows.rs` 的 UIA 工作线程复用或新增独立线程；不得阻塞 UI 线程。
5. `AppsPage` 启用滚动卡；未检测到能力时不显示。
6. 验证：UIA 滚动百分比变化或像素位移；不读取内容；不移动焦点。

### P7 增强项

- 真实应用图标：新增 `list_application_icons` IPC（`SHGetFileInfo` / `ExtractIconEx`），带缓存；失败回退字母块。
- 私有键位方案绑定反查：扩展宿主接口，公开层只消费稳定结构。
- 预设应用的聚焦档案兼容迁移（按 M0 结论）。

## 6. 验证

### 6.1 自动化

```powershell
corepack pnpm test
corepack pnpm build
cargo fmt --all -- --check
cargo test --workspace -j 1
cargo check --workspace
cargo check -p sayall-windows-app --features runtime-simulation
```

本轮已执行（2026-11-09，滚动区域首版）：前端 669 passed / 10 skipped、`pnpm build` 与 `vue-tsc --noEmit` 通过；`cargo test --workspace -j 1` 全通过（`sayall-windows` 536 passed / 16 ignored，`sayall-windows-app` 207 passed / 1 ignored）；NSIS 本地测试包内嵌前端校验 18/18。安装版通过 UIA 在 Chrome 上学习 `RootWebArea`，测试向下翻页读回 `moved=true`；独立探针在 Chrome 400 行测试页验证 `LargeIncrement`、滚轮回退、`SetScrollPercent` 到两端均产生可观察位移。默认并行的 `cargo test --workspace` 曾在本地因内存耗尽失败，串行运行即为上述通过结果。

新增/更新的前端测试：

- `AppsPage.test.ts`（含 `AppFocusProfileEditor` 的策略、学习、位置记录、快捷键录入与卸载释放）
- `app-adaptation.test.ts`、`key-codes.test.ts`
- `navigation.test.ts`、`Sidebar.test.ts`、`navigation-seam.test.ts`
- `ButtonsPage.test.ts`、`MacrosPage.test.ts`
- i18n 缺失 key 检查

### 6.2 界面走查

- 1066 × 800 最小窗口，100% 与 125% 缩放。
- 中文 / English 两套文案。
- 浅色 / 深色主题。
- 长 APP 名、长路径、空列表、三项以上列表。
- 键盘 Tab 顺序、焦点环、`aria-current`。
- 无横向滚动、无文字截断、无卡片重叠。

### 6.3 真实应用验证（实现后）

聚焦：

- 微信 4.x：`recorded_point` 路径。
- Chrome：`auto_composer` / 快捷键默认。
- VS Code / 记事本：`recorded_element`。
- 应用未安装、窗口关闭、输入框不存在时的失败路径。

滚动（第二阶段）：

- 至少 Chrome、VS Code、微信、记事本各一次向上/向下翻页与到两端。
- 用 UIA 滚动百分比或像素位移确认内容真的动了，不能只看 IPC 返回成功。
- 确认没有读取输入内容、没有移动键盘焦点。

### 6.4 不做扩大解释

- 前端单测与构建通过不等于 Windows 真机通过。
- 没有真实应用与 Windows 主机时，相关项标记 `deferred`。
- 不把仿真结果表述为 RC001/RC003 真机验收。

## 7. 日志与隐私

新增结构化日志事件（只记结果，不记 APP 路径、窗口标题、输入内容）：

- `app_adaptation action=page_load result=... count=...`
- `app_adaptation action=add_app result=... source=scanned|custom`
- `app_adaptation action=select_app result=... has_profile=...`
- `app_adaptation action=focus_strategy result=... strategy=...`
- `app_adaptation action=learn_focus phase=started|completed|failed reason=...`
- `app_adaptation action=learn_point phase=started|completed|failed reason=...`
- `app_adaptation action=shortcut_capture phase=started|completed|cancelled|failed`
- `app_adaptation action=test_focus result=...`
- `app_adaptation action=delete_blocked reason=bindings|macros count=...`
- `app_adaptation action=delete result=...`
- `app_adaptation action=nav result=... direction=to_buttons|to_apps`

禁止进入日志：自定义应用路径、APP 显示名、窗口标题、AutomationId、输入内容、学习记录详情。

## 8. 风险与待决事项

1. **预设聚焦档案不可见（高）**
   - 用户选择与 Mac 一致的「只列已添加 APP」。
   - 但 Windows 的预设应用也可能有显式 `focusProfiles`。移除按键页编辑器后，这些档案会变成不可编辑。
   - 必须先做 M0 审计；若存在，按 B 方案把「有显式档案的预设」纳入列表，或保留只读摘要。不要静默隐藏。

2. **应用身份不稳定（中）**
   - 自定义 APP 以路径/AUMID 为 key；应用移动或重装后 target 变化。
   - 首版显示「应用未找到」并允许重新添加；旧档案不自动迁移。
   - 后续可做路径变更迁移，但需要用户确认，避免误合并。

3. **重复预设与自定义项（中）**
   - 用户扫描添加微信时，可能出现「微信」预设 id 与微信路径两个目标。
   - 首版不自动合并；在添加对话框中提示同名项，或后续做目标归一。

4. **学习流程的迟到回调（高）**
   - 学习输入框最多阻塞 3 秒；用户可能切页、删除 APP、再次学习。
   - 必须使用 generation / requestId；卸载后丢弃结果；定时器成对清理。

5. **快捷键录入的全局吞键（高）**
   - 录入态必须与页面生命周期绑定：开始/取消/完成/失焦/卸载全部调用 `stopShortcutCapture()`。
   - 不能让页面切走后后端仍处于录入态。

6. **滚动区域第二阶段的不确定性（高）**
   - 不同应用 UIA 树差异大；微信等可能没有 ScrollPattern。
   - 先探针后实现；不要先做 UI 再补后端。
   - 翻页动作是新的按键动作类型，涉及 schema、执行、连发语义、测试，需独立评审。

7. **私有键位方案边界（中）**
   - 公开仓库不得解析私有 JSON 或复制私有 UI。
   - 绑定反查先覆盖基础 mappings；私有方案接入另行设计宿主接口。

8. **最小窗口与英文文案（中）**
   - 861 px 内容宽，右侧详情约 619 px；英文长词容易截断。
   - 设计稿与测试必须同时跑中英文；必要时缩短标签，不缩小到不可读字号。

## 9. 验收清单

- [ ] 设计稿经用户确认。
- [ ] 侧栏出现「APP 适配」，位于按键之后。
- [ ] 空列表显示空态与「添加 APP」。
- [ ] 添加 APP 后出现在列表并自动选中。
- [ ] 聚焦方式可切换，刷新/重启后保持。
- [ ] 学习输入框成功、失败、取消、切页均有明确结果。
- [ ] 旧版记录点击位置档案仍可执行，界面显示旧版策略提示且不再提供新入口。
- [ ] APP 快捷键录入、清除、Esc 取消、失焦取消成对。
- [ ] 按键页「打开应用」只显示只读聚焦摘要与「去配置」。
- [ ] 从按键页「去配置」能定位到对应 APP。
- [ ] 绑定反查列表与当前基础 mappings 一致。
- [ ] 点击绑定行能打开对应按键编辑卡。
- [ ] 有绑定时删除被阻止并说明数量。
- [ ] 无绑定时二次确认后删除适配，不卸载 APP。
- [ ] 中英文、浅色/深色、1066 × 800 无截断与横向滚动。
- [ ] 存量配置升级后不丢失、不改写。
- [ ] 滚动区域卡在能力未实现时不显示（或按第二阶段验收）。
