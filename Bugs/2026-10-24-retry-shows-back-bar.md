# 重试后推荐页顶部出现「返回上一页」

- 发现日期：2026-10-24
- 状态：已修复（Windows 本机调试构建与安装版实测；与 RC001/RC003 无关）
- 影响范围：无线麦 SayAll Windows 1.0.0，推荐页；重试及 App 内重新加载路径
- 功能点：推荐页子 WebView / 历史栈 / 顶部返回条
- 现象：推荐页失败后点「重试」，页面恢复成功，但顶部出现「返回上一页」；
  它指向失败页或旧页面，不是用户真正浏览过的上一页。
- 复现条件：推荐页发生超时或网络失败（phase=failed）后点「重试」；日志在
  `show ... mode=retry` 后出现 `history_changed can_go_back=true`。
  安装版 2026-10-24T11:39–11:40Z 实测：`mode=language_replace` 超时 → 失败页 →
  `mode=retry` → `history_changed can_go_back=true`。
- 正常预期：重试和 App 自己发起的重新加载不产生新的浏览历史；只有网页内真实
  导航（列表 → 详情、站内 pushState）才允许顶部返回条。
- 证据（修复前）：
  - 安装版 pid 8728，2026-10-24T11:39:35Z：`reason=timeout nav_id=7`。
  - 2026-10-24T11:40:32Z：`show mode=retry generation=5` →
    `history_changed can_go_back=true changed=true`。
  - 调试构建同类复现：页内 reload 超时后重试，失败页/旧页留在后退栈。
- 根因：`present` 的重试和非 ready 状态下的 App 内导航走 `webview.navigate`；
  WebView2 把当前（失败/旧）页面压入后退栈，新页面加载成功后 `CanGoBack=true`，
  前端按 `can_go_back` 显示返回条。`location.replace` 只覆盖了已 ready 文档的
  主题/语言切换，没有覆盖重试、失败页换外观、首次 about:blank 等路径。
- 修复（最小改动，均在 `src-tauri/src/recommendations.rs`）：
  - 新增 `reset_history_after_navigation`：重试、reuse 导航、首次创建成功后
    置位；`location.replace` 路径不置位。
  - `NavigationCompleted` 成功后，如果是 App 发起的导航，调用 WebView2 CDP
    `Page.resetNavigationHistory` 清空后退/前进历史；记录
    `event=history_reset result=passed can_go_back=false`。
  - 新增 `history_reset_pending`：重置回调完成前，`HistoryChanged` 一律按
    `can_go_back=false` 处理，ready 事件也不会闪出返回条；回调结束后读回真实值。
  - 失败、20s 超时、离开页面、渲染进程致命失败都会清掉待重置标记，避免它
    泄漏到下一次网页内导航。
- 覆盖的其他触发路径（逐项核对）：
  - 重试（页内 reload 超时、语言切换网络失败、详情 + 主题切换失败）：修复后
    均 `history_reset passed can_go_back=false`，无返回条。
  - 非 ready 状态换主题/语言、失败后换外观再进入：同一 reuse 路径，导航成功后重置。
  - 首次创建：about:blank 不留在历史里；启动安装版即 `history_reset passed`。
  - 已 ready 换主题/语言：`location.replace` 不新增条目（前一轮修复）。
  - 页内普通 reload / 同 URL 导航：实测 `can_go_back=false`，不触发返回条。
  - 站内 `history.pushState`：仍触发 `history_changed can_go_back=true`；这是
    网页真实历史，保留返回条（由网页决定，不归 App 内重新加载）。
  - 用户点返回条：`go_back` 后历史变化正常更新显隐。
- 验证：
  - 调试构建：上述三类重试均 `history_reset passed can_go_back=false`，DOM 无
    `.recommendations-back`；页内 reload/同 URL 无返回条。
  - 前端相关测试（bridge / language / recommendations / settings）71 passed；
    完整 487 passed 在本轮语言改动完成时通过；`vue-tsc --noEmit` 通过。
  - Rust 推荐页 11 个单测通过；`cargo fmt --all -- --check` 通过。
  - Rust 推荐页 11 个单测、`cargo test --workspace`、`cargo fmt --all -- --check`
    通过。安装版（12,985,755 字节，SHA-256
    `377F510D35A9B69ACA62754E997E87E059C5015049497D76AB9DB9029489911B`，按当时
    本地工作区构建）启动即 `history_reset passed can_go_back=false`，UIA 无
    「返回上一页」。
- 隐私检查：日志与本文档只含 generation、布尔值与固定路径，不含设备身份、
  语音内容、个人路径或凭据。
