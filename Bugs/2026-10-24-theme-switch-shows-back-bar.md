# 深色模式下推荐页出现不该有的「返回上一页」条

- 发现日期：2026-10-24
- 状态：已修复（Windows 本机调试构建与安装版实测；与 RC001/RC003 无关）
- 影响范围：无线麦 SayAll Windows 1.0.0，Windows 推荐页；浅色/深色外观切换与深色启动
- 功能点：推荐页子 WebView / 外观主题 / 顶部返回条
- 现象：切到深色（或启动时持久化为深色）后，推荐页顶部出现「返回上一页」条；
  该条指向的只是同一页的浅色版本，不是用户实际浏览过的上一页。
- 复现条件：应用已在浅色外观下打开推荐页并 ready；把外观切到深色。或应用以深色
  偏好启动、推荐页首次加载。日志在 `show ... theme=dark` 之后立即出现
  `history_changed can_go_back=true`。
- 正常预期：主题切换只是同一页换一套外观，不应产生新的后退条目；只有页内实际
  导航（例如列表 → 详情）才应显示返回条。
- 证据：
  - 修复前（安装版 pid 8164，2026-10-24T10:52:47Z）：`show mode=reuse theme=dark`
    → `history_changed can_go_back=true changed=true`。
  - 修复前（调试构建，2026-10-24T10:57:15Z）：启动时先
    `show mode=create theme=light`，随后 `show mode=reuse theme=dark`，导航完成后
    `can_go_back=true`；顶部出现返回条。
  - 修复后（调试构建，2026-10-24T11:00:40Z）：启动仅
    `show mode=create theme=dark`，`history_changed can_go_back=false`。
  - 修复后（2026-10-24T11:00:54Z / 11:01:09Z）：浅→深与深→浅均为
    `show mode=theme_replace`，`history_changed can_go_back=false`，无返回条。
  - 修复后（2026-10-24T11:02:12Z）：进入 `?item=vokie` 详情
    `can_go_back=true`，返回条出现；点返回 `go_back passed` 后
    `can_go_back=false`，返回条消失。
- 根因：
  1. `present` 在主题不一致时调用 `webview.navigate(target)`；WebView2 把浅色页
     压入后退栈，`CanGoBack` 变为 true，返回条按 `can_go_back` 显示。
  2. 前端 `RecommendationsPage.onMounted` 在外观设置异步初始化完成前就用初始
     `effectiveTheme` 调用了首次 `load`；深色偏好到达后 `watch(effectiveTheme)`
     再加载一次，启动阶段也会产生两条历史。
- 修复：
  - `src-tauri/src/recommendations.rs`：已就绪文档且主题变化时改用
    `window.location.replace(<新主题地址>)`（`mode=theme_replace`），替换当前
    历史条目；`eval` 失败回退 `navigate` 并记录 `reason=eval_failed`。
  - `src/lib/theme.ts`：`initializeTheme` 返回同一个进行中的初始化 Promise，
    `disposeTheme` 重置它，调用方可以真正等待初始化完成。
  - `src/pages/RecommendationsPage.vue`：首次加载先 `await initializeTheme()`，
    初始化期间的 `effectiveTheme` 变化由首次加载消化（`themeReady` 门），
    避免深色偏好造成浅色 + 深色两次 `show`。
  - `src/pages/RecommendationsPage.test.ts`：新增「等待外观初始化后再首次加载：
    深色不会先加载一版浅色」用例，并补齐 `initializeTheme` mock。
- 验证：
  - 前端 `vitest run`：28 个文件、480 passed | 10 skipped；`vue-tsc --noEmit` 通过。
  - Rust 推荐页 11 个单测通过；`cargo fmt --all -- --check` 通过。
  - 调试构建逐项实测如上；安装版
    `target/release/bundle/nsis/无线麦 SayAll_1.0.0_x64-setup.exe`
    （13,008,260 字节，SHA-256
    `0BD9020CD1AD69BC42F9FC734840438F455F7B50C9363619E623F4BCE4185048`）
    静默安装退出码 0，深色启动仅 `show mode=create theme=dark`、`can_go_back=false`，
    UIA 按钮树无「返回上一页」；设置里浅↔深切换后进入推荐页均为
    `mode=theme_replace`、`can_go_back=false`、无返回条；离开后重进
    `show mode=retained phase=ready`（`can_go_back=false`）。
- 隐私检查：日志与本文档只含主题名、布尔值、路径名与时间，不含设备身份、
  语音内容、个人路径或凭据。
