# 推荐页一直显示「正在加载网页…」，失败/重试页永远不出现

- 发现日期：2026-10-24
- 状态：已修复（本地测试包已安装复验）
- 影响范围：Windows 客户端 1.0.0（源修订 `4d01aa1`）安装版；Windows 10.0.26200 x64；与遥控器型号无关
- 功能点：设置侧栏「推荐」页（官网 `https://sayall.app/recommendations/` 子 WebView 宿主）
- 现象：进入「推荐」后右侧一直转圈显示「正在加载网页…」，网页内容始终不出现；等待一分钟以上也没有出现产品规范要求的失败页（「打开官网」/「重试」），因此也没有 Mac 端那样的重试入口。
- 复现条件：安装版进入推荐页 → 等待；离开后再进入仍然如此。
- 正常预期：主文档加载成功后显示官网页面；加载失败或超时显示「暂时无法加载此页面，你可以前往官网查看，或稍后重试。」+「打开官网」+「重试」（`docs/product/recommendations.md`）。
- 证据：
  - 应用诊断日志（2026-10-24 07:38:42Z～07:44Z）只有 `feature=recommendations event=show mode=create`、`event=observer result=passed`，随后 `event=hide`；**没有任何 `event=navigation phase=completed` 行**。
  - 同一时段的 Win32 窗口枚举显示子 WebView 窗口标题已经是「推荐 · 无线麦 SayAll」，即**网页实际已加载**，只是状态事件没有回到界面。
  - 10-23 01:58Z 同一形态；10-22 的多次进入则有 `event=navigation phase=completed result=passed http_status=200`，说明是时序相关而非网站不可达。
  - 2026-10-24 07:41:11Z 重进（`mode=reuse`）时，`on_page_load` 记录到上一代导航在 07:41:12.245Z 完成、新一代在 07:41:13.247Z 完成，但两个完成事件都被丢弃。
- 根因：
  - Rust 导航完成回调先用 `expected_navigation.swap(0)` 取出等待中的导航 ID 再比对。任何一次**不匹配**的完成事件（用户离开后上一代导航才完成、被新导航取消的 `about:blank`）都会把等待中的 ID 清成 0；随后真正属于当前页面的完成事件被当成迟到事件忽略（`expected == 0` 分支），不显示子 WebView、也不发 `ready`。
  - 前端没有加载超时兜底：产品规范里的「主文档失败/超时」状态只有在 Rust 主动发 `failed` 时才会出现，导航完成事件被吞后永远停在 loading。
- 修复：
  - `src-tauri/src/recommendations.rs`：新增 `consume_matching_navigation()`，用 `compare_exchange` **只在导航 ID 匹配时**消费等待中的 ID；不匹配的完成事件只记录、不改状态。主文档 HTTP 状态也改为接受本次导航后才消费。
  - 同文件补齐结构化日志：`phase=started`（nav_id/allowed/path/generation）、`phase=completed result=observed`（nav_id/expected/success/web_error_status）、`result=ignored reason=no_pending_navigation|stale_navigation`。
  - `src/pages/RecommendationsPage.vue`：新增 20s 主文档加载超时；超时后显示失败页（打开官网 / 重试），同时隐藏原生子 WebView 并作废该代事件，避免迟到成功把网页层盖回失败页。
  - 同文件（2026-10-24 追加）：失败文案与 Mac 严格对齐——`show_recommendations` 命令本身失败时也只显示固定失败句，底层错误只记 `frontend event=recommendations_page phase=show reason=show_command_failed`，不拼给用户。
- 验证：
  - 前端 `vitest run src/pages/RecommendationsPage.test.ts`：3 passed；其中超时用例先做阳性对照（去掉 `startLoadTimer` 后失败，恢复后通过）。
  - 前端全量 `vitest run`：28 files / 477 passed | 10 skipped；`vue-tsc --noEmit` 通过。
  - Rust：`cargo test --workspace` 全通过（含新增 `stale_completion_does_not_clear_pending_navigation`）；`cargo fmt --all -- --check` 通过。
  - 本地调试构建目视：进入推荐页日志 `phase=started nav_id=... allowed=true` → `phase=completed result=observed ... success=true` → `result=passed http_status=200`，截图确认官网推荐页完整显示；切到「按键」再进入（`mode=reuse`）同样 passed。
  - 本地测试安装包（构建源 `c0d79f6` + 本次未提交修复）：
    `target/release/bundle/nsis/无线麦 SayAll_1.0.0_x64-setup.exe`，
    12,974,303 字节，SHA-256 `F6594E5630635E2C8224CEEB481A2E8251CD900477123B69DDFE23C709FC48D1`；
    前端嵌入校验 15 个资源全部通过；静默安装退出码 0。安装版进入推荐页
    `navigation passed http_status=200`，离开后重进（`mode=reuse`）同样 passed；
    截图 `artifacts/local-test/2026-10-24-recommendations-load/installed-recommendations-page.png`。
  - 超时与重试真机链路（安装版 + CDP 只暂停主文档请求，不修改网络）：
    08:11:44.862Z `navigation started nav_id=7` → 08:12:04.863Z（正好 20s）
    `frontend event=recommendations_page phase=load result=failed reason=page_load_timeout elapsed_ms=20000`
    + `event=hide generation=4`；被暂停的请求随后成功完成时记录
    `result=observed ... success=true` 但 `result=ignored reason=no_pending_navigation`，
    没有把网页层盖回失败页。点击「重试」后 `show mode=reuse generation=5` →
    `navigation passed http_status=200`，页面完整显示。截图
    `timeout-failure-page-webview.png`（失败页）与 `retry-loaded-page.png`（重试成功）。
  - 首轮全量测试曾有一次 `rc003_bridge::tests::end_to_end_loopback_delivers_edges` 失败（收到 `S -` 行早于 `T` 行），单独重跑与第二次全量均 passed，判定为并行负载下的既有时序抖动，与本次改动无关。
- 隐私检查：日志与截图只含公开网址、导航阶段、HTTP 状态和界面内容；未包含设备身份、语音内容、个人路径或凭据。

## 2026-10-24 追加：Mac 推荐页 P1/P2 对齐

同一改动把 Mac 容器的保留文档、离开停止加载、重试当前页、外链用户手势、
网页就绪信号、20s 原生看门狗、渲染进程失败页和「返回上一页」条对齐到 Windows。
完整逻辑、证据和边界见 `docs/product/recommendations.md` 同日 P1+P2 节。

最终本地测试包（含本 Bug 修复与 P1/P2）：13,000,802 字节，SHA-256
`506CF58FAA3D9C75A4A0D5D741CE35259677DDEB3EFEF9F501087F158AE86712`；
安装版进入推荐页 `ready_signal passed http_status=200`，离开后重进
`mode=retained phase=ready`。
