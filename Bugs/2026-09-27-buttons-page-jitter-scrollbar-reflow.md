# 切换页面时整窗水平抖动（按键页滚动条出现/消失引发回流）

- 发现日期：2026-09-27
- 状态：已修复（浏览器预览实测通过；真机验证 deferred）
- 影响范围：Windows 版全部页面切换，默认窗口（1029×732）最明显；WebView2（Chromium 内核，经典滚动条占布局空间）
- 功能点：前端布局（`src/styles.css`，根元素滚动与滚动条）
- 现象：每次从「连接」「权限」等页点进「按键」页，整个页面水平抖动一下；切走时再抖回来。
- 复现条件：视口高度 700px（窗口 1029×732 去标题栏）。按键页自然高度 713px 略超一屏，连接/权限页 700px 不超。
- 正常预期：切换页面时布局零位移。
- 证据：Playwright + 本机 Chrome（视口 1029×700）逐帧测量（rAF 记录 `clientWidth` 与关键元素 `getBoundingClientRect`）：
  - 修复前四页调查：连接/权限 `clientWidth=1029`，按键/关于 `clientWidth=1014`（垂直滚动条占 15px）；
  - 修复前点击「按键」后 t=38ms：`clientWidth 1029→1014`，`.page-header` 宽 859.6→844.3，整窗水平回流；
  - 按键页 713px 仅超视口 13px，属窗口尺寸调整（1080×720 → 1120×800 → 1029×732，见 Bugs/2026-09-03）后的边缘态。
- 根因：滚动发生在根元素（`.content` 随内容增高、自身不滚），正文滚动条随页面高度出现/消失。Chromium 经典滚动条占 15px 布局宽度，出现瞬间视口变窄，全部右对齐内容左移、离开时复原。已实测否证备选方案：`scrollbar-gutter: stable` 对根滚动器在 Chromium 不生效（html 上 overflow visible/auto 均不预留槽位，computed 值正确但 `clientWidth` 不变）。
- 修复：`src/styles.css` 对 `html` 隐藏根滚动条（`scrollbar-width: none` + `::-webkit-scrollbar{display:none}`，宽度归零、滚轮/键盘滚动保留）。四页 `clientWidth` 恒定 1029，切换零回流。与 plan 2026-09-05 T2「页面不显示滚动条、仍可滚动」的产品决策一致；`.content` 上同款规则是既有先例。
- 验证：
  - 修复后逐帧测量：点击「按键」前后 `clientWidth` 恒 1029、`.page-header`/`.mapping-canvas`/`.mapping-footer` 位置与宽度零变化（passed）；
  - 滚轮滚动仍生效（scrollTop 0→12.7，passed）；弹窗为原生 `<dialog>` + `showModal()`（top layer），不受影响（passed）；
  - `pnpm test` 111/111、`pnpm build`（vue-tsc + vite）passed；
  - 真机 WebView2 确认抖动消失：deferred（浏览器预览与 WebView2 同为 Chromium；用户症状本身即经典滚动条占位的证据）。
- 后续（2026-09-27 晚，用户要求）：窗口默认/最小尺寸 1029×732 → **1100×720**（`src-tauri/tauri.conf.json`）；视口相应变为 1100×688。复测：四页 `clientWidth` 恒 1100、滚动条宽度 0（无可见滚动条）、按键页（714px）与关于页（890px）仍可滚轮滚动，切换零回流（passed）。关于页 `.diagnostic-output` 为嵌入式诊断输出框的内部滚动（max-height 180px），不属于页面级滚动条，保留。
## 2026-09-28 追加：第二个水平抖动源（画布宽度测量时序）

- 现象：用户反馈 #138 合入后点进按键页**仍然**抖动，且方向明确是**水平**（左右）。
- 先排除滚动条（#138 那一条）：用户安装的包实测窗口 1116×759 物理像素（去 Windows 边框与标题栏即逻辑 1100×720，说明 #138 的窗口改动已生效）；真机截图右侧像素均匀、内容右边界恒定 1103，**无可见滚动条**，切换时内容区宽度不变。
- 根因：画布几何全部基于 `canvasWidth`（左卡 x = `cardWidth`、右卡 x = `canvasWidth - cardWidth`、遥控器图 x = `(canvasWidth - 202) / 2`、连线路径与 `<svg width>`）。`canvasWidth` 初值是 `CANVAS_MIN_WIDTH = 800`，而 1100 窗口下容器实际宽 **926**。原实现在 `onMounted` 中**先** `await Promise.all([...])`（挂载首批 IPC），**之后**才首次测量宽度 → 首帧以 800 布局渲染，IPC 返回后跳到 926，画布内所有元素整体水平重排。
- 为何此前反复测不到：浏览器预览中这几个 Promise 同帧 resolve（内存快照），首帧即最终值、无中间帧；**真机 IPC 跨进程有延迟，必然跨帧**。此前逐帧测量只记录过垂直方向的 `canvasTop`，水平方向未覆盖。
- 证据：在 `bridge.ts` 浏览器分支注入 300ms 延迟复现（视口 1100×688，rAF 逐帧对比全部元素的 `left/right/width`）：遥控器图与 anchor-dot **+63px**、右列卡片（含标题/映射格）**−30px**、连线箭头 polygon **+96px**、`svg width` **800→926**。
- 修复：把画布宽度的首次测量移到 `onMounted` 的同步段（首个 `await` 之前），首帧即真实宽度；窗口尺寸变化仍由 `ResizeObserver` 接管（原处重复的那次测量删除）。
- 验证：注入 300ms 延迟下修复 —— 首帧 `svg width` 即 926、逐帧全元素**零水平位移**（passed）；移除延迟的正常路径同样零位移（passed）；单测与构建由 CI 验证（本机沙箱反复吞掉 `node_modules`）。
- 真机复验：deferred（需出新包后确认）。

- 隐私检查：本文档与修复不含个人路径、设备身份、语音内容或凭据。
