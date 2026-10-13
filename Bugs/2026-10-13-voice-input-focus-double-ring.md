# ⑤步输入框"多一圈不完整的框"：焦点环与边框之间透出 1px 缝

- 发现日期：2026-10-13（Andy 截图现场；同一轮还报告"没碰键盘却判键盘输入"，见
  Bugs/2026-10-13-manual-input-false-positive-hotkeys.md）
- 状态：已修复（CSS 一行 + 契约用例 + 像素级前后对照）；**真机复核 deferred**
- 影响范围：Windows 11（10.0.26200）；向导第⑤步；**重启后第一次进入⑤步**（非指针模态、输入框自动聚焦）时可见；
  点过任意位置后（指针模态）不再出现
- 功能点：向导第⑤步语音测试页输入框焦点样式（`src/pages/OnboardingPage.vue`）

## 现象

重启应用后第一次进入⑤步：输入框外围"多一圈不完整的框"——同色的第二圈细线与边框并行、
中间露出细缝，圆角处两线并拢，看起来像一圈画断了的框。点过任何位置（或此后任何时刻再看）只剩正常的一圈边框。

## 复现与测量

- 现场状态 = 输入框被程序化聚焦（进入⑤步时 `focusVoiceBox()`）+ **无指针交互**（`body.pointer-nav` 未置位）。
  此时页面规则 `.onboarding-voice-input:focus { outline: 2px solid var(--accent-border); outline-offset: 1px }`
  生效（特异性 0-2-0 高于全局 `input:focus-visible` 的 0-1-1）。
- 一旦发生 mousedown，`body.pointer-nav` 置位 → 全局 `body.pointer-nav input:focus-visible { outline: none }`
  （0-2-1）反过来压过页面规则 → 只剩边框一圈。这就是"第一次进入才有"的原因。
- 最小复现（与产品同源的 CSS 片段 + 本机 Chrome，1100×688、DPR 2；脚本与截图见
  `.workbuddy/tmp/ring-mini.cjs`、`ring-mini-*.png`，扫描脚本 `scan-mini.ps1`）：

  修复前（offset 1px，聚焦帧；设备像素游程，四边中点扫描）：

  ```
  左: …背景×36 | D8AFE3×4（外圈） | 背景×2（缝） | D8AFE3×2（边框） | 白…
  右: …白 | D8AFE3×2 | 背景×2 | D8AFE3×4 | 背景…
  上: 背景×36 | D8AFE3×4 | 背景×2 | D8AFE3×2 | 白…
  下: 白 | D8AFE3×2 | 背景×2 | D8AFE3×4 | 背景…
  ```

  = 两圈同色线 + 2 设备像素（1 CSS px）细缝，四边同构。

  修复后（offset 0）：

  ```
  左: 背景×36 | D8AFE3×6 | 白…      右: 白 | D8AFE3×6 | 背景…
  上: 背景×36 | D8AFE3×6 | 白…      下: 白 | D8AFE3×6 | 背景…
  ```

  = 一整圈 3 CSS px 连续细线，无缝隙。

- 真机页对照（同轮预览实测，`ring-measure-dpr2-nav.log`）：经由点击进入⑤步时 `pointer-nav` 已置位、
  计算样式 `outline-style: none`——所以这条只在"无指针交互的首入"场景可见，预览的点击导航路径天然测不到，
  这正是它之前没被自动化拦住的原因。

## 根因

焦点环用 `outline-offset: 1px` 让外圈与边框之间留出 1 CSS px 的页面背景缝；同色（`--accent-border`）
两层细线 + 发丝缝在缩放/圆角处会被感知为"一圈画断的框"。同色的两圈没有信息量，缝只有坏处。

## 修复

- `src/pages/OnboardingPage.vue`：`.onboarding-voice-input:focus` 的 `outline-offset: 1px` → `0`
  （外圈与边框合成一整圈；指针模态下的全局抑制规则不变）。
- `src/pages/OnboardingPage.test.ts`：新增契约用例（读取 SFC 源码断言 `outline-offset: 0`；jsdom 量不了像素，
  该用例防的是"样式值被改回去"）。

## 验证

- 像素级前后对照（上面两组游程）= passed（本机 Chrome、DPR 2、四边中点扫描）。
- `pnpm test`：OnboardingPage 64 passed（含新增契约用例）；全量套件在打包前跑一遍。
- **deferred（真机）**：重启应用后第一次进入⑤步——输入框聚焦时应只有一整圈（无细缝、无第二圈）；
  点过任意位置后再看行为照旧。

## 隐私检查

- 本文与截图脚本只含样式值、像素颜色与几何量；不含设备身份、语音内容、个人路径。

## 2026-10-14 追加：统一为细边框（Andy 定稿）

修掉"断框"之后 Andy 追问："第一次进来和从上一个页面进来，这个输入框的边框为什么不一样？"
像素比对两张现场截图确认：

- 第一次进来（重启后直接落在⑤步、未用鼠标）：聚焦环与边框合成的一整圈，上下边缘各 **7 设备像素**连续线；
- 从上一页进来（点过"继续"）：`body.pointer-nav` 抑制焦点环，只剩 1.5px 边框（**3 设备像素**）。

同一个"输入框聚焦"状态两种外观（根因同前：自动聚焦 + 模态相关抑制）。Andy 选定**统一为细边框**：
`.onboarding-voice-input:focus` 与 `:focus-visible` 都置 `outline: none`——两次进入、点击前后外观一致，
只留 1.5px 细边框。取舍：本框由向导自动聚焦、文案已引导点击，键盘用户不依赖该焦点环；其余控件的全局
焦点环约定不变。契约用例同步改为断言 `outline: none`。
