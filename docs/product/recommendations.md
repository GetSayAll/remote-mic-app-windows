# 推荐页产品规范（Windows）

状态：2026-10-18 按用户要求实现。Windows 版只承载官网推荐页，不复制推荐内容，
也不自建推荐 UI。参考 Mac 产品规范（私有平台仓库 `docs/product/recommendations.md`，
提交 `9845c01`）与 Mac 通用网页容器（`725061f`）；未复制其代码。

## 目标与范围

- 设置侧栏新增「推荐」入口，位置对齐 Mac：`按键 → 推荐 → 连接 → 权限 → 设置`。
- 页面内容、分类、筛选、商品顺序、详情与运营更新全部来自官网
  `https://sayall.app/recommendations/`。客户端不保存推荐目录，不内嵌 HTML/脚本，
  不读取会员、支付、录音或遥控器数据。
- 首次接入需要发布包含本页面的 Windows 客户端；之后官网内容与布局更新不需要
  重新发布 App。
- 推荐页不阻塞按键、语音、连接或托盘驻留等基础能力。

## 页面承载

- 官网返回 `X-Frame-Options: DENY`，不能用 `<iframe>`；Windows 使用 Tauri 主窗口
  内的子 WebView（WebView2）。子 WebView 的边界固定为侧栏 108px 之外的整个客户区，
  窗口缩放与跨屏 DPI 变化时重新贴边。
- 加载地址：`https://sayall.app/recommendations/?embed=app&appPlatform=windows&theme=light|dark`。
  - `embed=app` 让官网隐藏自身导航和页脚，只保留推荐内容；
  - `appPlatform=windows` 是平台标记；当前官网按 `website` 内容面渲染；
  - `theme` 跟随 App 当前浅色/深色外观；切换外观时重新加载同一页面。
- 页面内部的列表/详情导航由官网自己的 `?item=<slug>` 与浏览器历史处理；
  容器不重复实现推荐详情。
- 子 WebView 不向官网暴露 Tauri 原生调用或文件访问能力。

## 加载、失败与跳转

| 状态 | 行为 |
| --- | --- |
| 加载中 | 子 WebView 保持隐藏，页面显示「正在加载网页…」；不显示空白页冒充成功 |
| 成功 | 子 WebView 显示；官网自己负责推荐数据的加载、缓存、离线和重试提示 |
| 主文档失败/超时 | 子 WebView 隐藏，显示「暂时无法加载此页面，你可以前往官网查看，或稍后重试。」+「打开官网」+「重试」；超时阈值为 20 秒 |
| 重试 | 重新加载当前推荐页；旧导航的迟到完成事件按 generation 丢弃 |
| 离开页面 | 隐藏子 WebView，并让后续迟到回调失效 |

- 「打开官网」交给系统浏览器，保留当前详情参数（`item`），移除 `embed`、
  `appPlatform`、`theme` 等 App 专用参数。
- 允许在子 WebView 内导航的地址仅限 `sayall.app` 的 `/recommendations/`、`/en/recommendations/`
  及其子路径；其他 HTTPS 链接（商品官网、App Store、官网其他页面）和新窗口链接
  交给系统浏览器。非 HTTPS 或带用户信息的地址一律拦截。
- 退出页面、切换页面、进入向导或退出应用时不留下可见的网页层。

## 文案

| 场景 | 文案 |
| --- | --- |
| 侧栏 / 页面标题 | 推荐 |
| 加载 | 正在加载网页… |
| 失败 | 暂时无法加载此页面，你可以前往官网查看，或稍后重试。 |
| 失败主入口 | 打开官网 |
| 失败次入口 | 重试 |
| 浏览器预览 | 浏览器预览中无法内嵌官网推荐页面，请前往官网查看。 |

官网正文、分类名、商品名和推荐理由不在本仓库维护；以官网发布内容为准。

## 隐私与边界

- 不记录推荐浏览行为、不新增分析事件；结构化日志只记录阶段、结果、
  generation、主题和导航结果，不记录完整 URL 查询串、设备身份或个人信息。
- 不读取或修改第三方 App 的私有数据；站外链接只通过系统默认浏览器打开。
- 不把子 WebView 当作通用浏览器：地址白名单只覆盖推荐页，导航和弹窗均受限制。

## 验证

自动化：

- `pnpm test`：导航顺序、侧栏图标、推荐页状态壳、bridge 命令名与 generation。
- `pnpm build`（`vue-tsc --noEmit` + `vite build`）。
- `cargo fmt --all -- --check`、`cargo test --workspace`、`cargo check --workspace`。
- Rust 单测：嵌入 URL、主题归一化、地址白名单、详情参数保留、`about:blank` 回退。

真机/目视（本地验收包执行，不能由单元测试替代）：

1. 侧栏「推荐」位于「按键」之后；进入后官网页面填满右侧内容区，官网导航/页脚隐藏。
2. 浅色、深色各看一次；切换 App 外观后网页主题跟随。
3. 窗口缩放、最大化、跨不同 DPI 显示器后，网页仍贴合侧栏右侧，无重叠或空白条。
4. 列表内进入详情、网页内返回；「打开官网」打开系统浏览器并保留 `item` 参数。
5. 断开网络后重进或点重试：出现原生失败状态；恢复网络后重试成功。
6. 站外商品/官网链接在系统浏览器打开；子 WebView 不跳到站外页面。
7. 离开推荐页后网页层消失；点击侧栏其他页面、隐藏到托盘再恢复均无残留。

## 2026-10-18 验证记录

自动化（本机 Windows）：

- `pnpm test`：24 个测试文件、378 passed | 11 skipped（含导航顺序、侧栏图标、
  推荐页状态壳与 bridge 命令契约）。
- `vue-tsc --noEmit` + `vite build`：通过。
- `cargo fmt --all -- --check`：通过。
- `cargo test --workspace`：`sayall-core` 33、`rc001_scenario_replay` 4、
  `sayall-windows` 353 passed | 11 ignored、`diagnostic_log` 1、`ipc_contract` 1、
  `sayall-windows-app` 118 passed | 1 ignored；推荐页 4 个单测全通过。
- `cargo check -p sayall-windows-app --features runtime-simulation`：通过。

本机调试构建目视（`target/debug/sayall-windows-app.exe`，Vite 提供前端）：

- 进入推荐页：日志 `event=show mode=create` → `event=observer result=passed` →
  `event=navigation result=passed http_status=200`；页面显示官网内容
  （推荐 / 遥控器推荐 / 搭配软件 / vokie / 豆包输入法 / 微信输入法）。
- 窗口从 1081×837 调整到 1400×900（150% DPI，物理 2100×1350）：
  `WRY_WEBVIEW` 子视图重贴到侧栏右侧（物理 113→2087，窗口右缘 2096），
  无重叠、无空白条，网页按两列重新排版。
- 离开推荐页切到「按键」：日志 `event=hide generation=2`，网页层完全消失。
- 深色外观下重新进入：日志 `event=show mode=reuse generation=3 theme=dark` →
  `navigation result=passed`，官网页面切换为深色配色。
- 浏览器预览（无 Tauri 运行时）：显示「浏览器预览中无法内嵌官网推荐页面」+
  「打开官网」「重试」，不伪造推荐内容。

未覆盖（deferred）：

- 断网 / 主文档 5xx 的失败状态：本机未断开用户网络，未在真机触发；仅由单元测试
  与状态壳测试覆盖，需在验收包中按上面第 5 项执行。
- 网页内详情页与「打开官网」保留 `item` 参数：`browser_destination` 已有单测，
  但未在真机点击验证（官网卡片当前没有「查看详情」按钮，只有外链）。
- 跨显示器 DPI 变化、最大化/还原、托盘隐藏恢复：未逐项执行。
- 系统浏览器外链跳转：未在真机点击（需确认不会抢占用户当前浏览器会话）。

## 2026-10-19 本地测试安装包验证

产物：`target/release/bundle/nsis/无线麦 SayAll_1.0.0_x64-setup.exe`
（12,603,200 字节，SHA-256 `72CE71FAD662A70E88D8A039D5A7689C211285B6169CB9774AD8371EC7E83920`）。
构建源：`3bf542ff18793c59a371e43b3e4ada32e7284730` + 未提交的推荐页改动；
前端目录 `dist-build-20261019-1418`。

passed：

- 前端资源嵌入校验 `scripts/verify-frontend-embedded.ps1`：10 个前端资源全部嵌入。
- 静默安装 `/S`：退出码 0；连续执行两次（第一次时有旧实例占用，第二次补齐）。
- 安装目录 `%LOCALAPPDATA%\无线麦 SayAll`：`sayall-windows-app.exe` 内嵌修订号
  `3bf542ff…`、`recommendations-page-state`、`feature=recommendations` 均可检索到。
- 启动安装版 → 侧栏点「推荐」：日志 `mode=create`、`observer passed`、
  `navigation passed http_status=200`；截图确认官网页面正常显示（深色）。
- 窗口缩回默认 1081×837 后重新进入：页面仍贴合侧栏右侧，两列 + 右侧软件栏完整。
- 切到「按键」：`event=hide generation=2`，网页层消失。

deferred（沿用上面的未覆盖项）：断网失败页、真机「打开官网」与 `item` 参数、
跨屏 DPI、托盘恢复、站外链接点击。

## 2026-10-19 修复：推荐页按钮无法点击

用户现场反馈「推荐页里面的按钮无法点击、无法跳转」。根因与修复见
[Bugs/2026-10-19-recommendations-links-not-clickable.md](../../Bugs/2026-10-19-recommendations-links-not-clickable.md)：
`tauri_plugin_opener` 默认注入的「自动接管 target=_blank」脚本在推荐页子 WebView
里调用 IPC，但该 WebView 不在 ACL 授权范围，被拒绝后 `preventDefault()` 已生效，
导致点击完全无反应。修复 = 关闭该注入（`open_js_links_on_click(false)`），站外
链接由推荐页自己的 `on_new_window` / `on_navigation` 交给系统浏览器。

复验 passed：CDP 真实点击得到 `Page.windowOpen https://vokie.com/?from=sayall.app`，
诊断日志 `event=external_link action=browser result=passed`，系统浏览器同秒启动；
本地测试安装包（SHA-256 `D24FCEC8D9E7293441B3023DB684A64FCA065E4E8D5E47AFD7BC92342A55A965`）
重复点击两次同样 passed。

## 2026-10-24 修复：一直加载不出来 / 没有失败重试页

用户现场反馈：进入「推荐」后右侧一直显示「正在加载网页…」，网页内容不出现；
而且没有 Mac 端那样的失败/重试页面。完整记录见
[Bugs/2026-10-24-recommendations-stuck-loading.md](../../Bugs/2026-10-24-recommendations-stuck-loading.md)。

- 根因 1：导航完成回调先 `swap(0)` 再比对导航 ID。不匹配的迟到完成事件（离开后
  上一代导航才完成、被取消的 `about:blank`）会清空当前等待中的导航 ID，随后真正
  的完成事件被当成迟到事件忽略。窗口标题已变成「推荐 · 无线麦 SayAll」证明网页
  实际已加载，只是子 WebView 一直没有被显示。
- 根因 2：前端没有主文档加载超时，`failed` 状态只有 Rust 主动上报时才可达；
  导航完成事件被吞后永远停在加载中，重试入口也就永远不出现。
- 修复：Rust 改为 `compare_exchange` **只在导航 ID 匹配时**消费等待状态，并补齐
  `started` / `completed result=observed` / `result=ignored` 结构化日志；前端增加
  20 秒超时，超时后显示失败页并撤下子 WebView（迟到成功事件作废）。
- 失败文案严格对齐 Mac（2026-10-24 追加）：显示命令本身失败时也只显示
  「暂时无法加载此页面，你可以前往官网查看，或稍后重试。」，错误原因只进
  `frontend event=recommendations_page phase=show reason=show_command_failed` 日志，
  不把底层错误文本拼给用户。

验证 passed：

- 前端 `vitest run`：28 个测试文件、478 passed | 10 skipped；推荐页 4 项，超时用例
  先做阳性对照（去掉计时器即失败）、命令失败用例断言固定文案且不泄漏错误文本。
  `vue-tsc --noEmit` 通过。
- Rust `cargo test --workspace` 全通过（含新增「迟到完成不得清掉等待导航」单测）；
  `cargo fmt --all -- --check` 通过。
- 本地调试构建目视：进入推荐页 `navigation started nav_id=11` →
  `completed result=observed success=true` → `result=passed http_status=200`；
  切到「按键」再进入（`mode=reuse`）同样 passed。截图见
  `artifacts/local-test/2026-10-24-recommendations-load/recommendations-page.png`。
- 本地测试安装包 passed：`target/release/bundle/nsis/无线麦 SayAll_1.0.0_x64-setup.exe`
  （12,974,303 字节，SHA-256
  `F6594E5630635E2C8224CEEB481A2E8251CD900477123B69DDFE23C709FC48D1`，构建源
  `c0d79f6` + 未提交的本次修复）；静默安装退出码 0，安装版进入推荐页
  `navigation passed http_status=200`、离开后重进（`mode=reuse`）同样 passed；
  截图 `installed-recommendations-page.png`。
- 超时/重试链路 passed（安装版，CDP 仅暂停主文档请求）：20.0s 时出现
  `frontend event=recommendations_page ... reason=page_load_timeout` 与
  `event=hide`；之后被暂停请求的成功完成被记为
  `result=ignored reason=no_pending_navigation`，不会盖回失败页；点击「重试」后
  `navigation passed http_status=200`。截图 `timeout-failure-page-webview.png`、
  `retry-loaded-page.png`。

deferred（沿用上方未覆盖项）：真实断网 / 5xx / DNS 挂起、跨屏 DPI、托盘恢复。
（后续 P1+P2 段已实测真实断网 / 5xx / DNS 失败页、离开即停止与托盘恢复；
跨屏 DPI 仍 deferred。）

## 2026-10-24 Mac 推荐页逻辑对齐（P1+P2）

按 Mac 私有实现（`sayall-private-platform/packages/macos-membership`）把推荐页容器
的 P1/P2 逻辑对齐到 Windows。内容面仍是 `appPlatform=windows`（官网按 website
渲染），不改网站。

- **已就绪文档保留**：切走再切回不重新导航，保留筛选、详情和滚动位置；只有切换
  主题、上次加载被中断或上次已失败时才重新加载。失败页在返回时保留，等用户点
  「重试」。
- **离开即停止**：离开推荐页时停止本次加载并作废 20s 导航看门狗，不再让挂起的
  导航在后台继续跑。
- **重试当前页**：重试重新加载当前地址（保留 `?item=` 等站内参数），不再回到列表；
  新地址仍会套用当前主题。
- **外链只认用户点击**：`NavigationStarting.IsUserInitiated` 为真且是 HTTPS 时才交给
  系统浏览器；脚本重定向、自动跳转一律拦截（日志
  `result=blocked reason=non_user_initiated`），不会自动弹出浏览器。
- **子 frame 一律拦截**：`FrameNavigationStarting` 对同站点/站外 iframe 都调用
  `SetCancel(true)`（对齐 Mac 容器 `!targetFrame.isMainFrame` 的取消规则）；
  日志只记 `result=blocked same_host=true|false`，不写完整地址。
- **网页就绪信号**：主文档完成后读取 `window.sayallWebPageState`；只有
  `ready` 才显示子 WebView，`failed` 显示失败页，缺失则交给 20s 超时兜底。
- **原生 20s 看门狗**：每次允许的导航（包括网页自己触发的整页导航/刷新）都启动
  超时；超时停止加载并显示失败页（`reason=timeout`），不依赖前端计时器。
- **渲染进程失败**：WebView2 `ProcessFailed` 的渲染进程退出/无响应/浏览器进程
  退出都转成失败页（`reason=process_failed`）。
- **返回上一页**：ready 且 WebView 可后退时，顶部显示 44px 返回条；子 WebView
  同步下移，点击调用原生 `GoBack`，历史变化通过 `HistoryChanged` 更新显隐。
- **主题切换不产生后退条目**：已就绪文档切换浅/深色时用 `location.replace`
  替换当前历史条目（`mode=theme_replace`），不会把旧主题页压进后退栈；首次加载
  等待外观设置初始化完成，避免深色启动时先加载浅色再切深色。

验证 passed（调试构建 + 安装版，CDP 仅用于制造挂起/读状态）：

- 进入推荐页：`navigation started` → `ready_signal result=passed reason=document_ready`
  → `http_status=200`。
- 已就绪文档：切到「按键」再回来 `hide phase=ready stopped=false` →
  `show mode=retained phase=ready`，无重新导航；页面内 JS 全局值与
  `window.sayallWebPageState=ready` 均保留。
- 整页导航到 `?item=vokie`：`history_changed can_go_back=true` → 顶部返回条出现；
  点返回条 `event=go_back result=passed` → `can_go_back=false`，回到列表。
- 网页触发的整页刷新被 Fetch 挂起：20.0s 时 `reason=timeout nav_id=...`，随后迟到的
  完成事件被记为 `result=ignored reason=no_pending_navigation`；点「重试」
  `mode=retry` 后仍加载 `?item=vokie`，`ready_signal passed`。
- 站外自动跳转：`result=blocked reason=non_user_initiated user_initiated=false`，
  页面 URL 不变，没有打开浏览器。
- 渲染进程崩溃（CDP `Page.crash`）：`event=process_failed kind=1 fatal=true`；
  回到推荐页 `mode=retained phase=failed`，显示固定失败文案与「打开官网 / 重试」。
- 子 frame：注入 `https://sayall.app/` 与 `https://example.com/` 两个 iframe，
  两条 `event=subframe_navigation result=blocked`（`same_host=true/false`），
  子 frame 导航被取消。
- 深色启动/主题切换：启动仅 `show mode=create theme=dark`（没有先浅色再深色），
  `history_changed can_go_back=false`；浅→深与深→浅均为
  `show mode=theme_replace`、`can_go_back=false`，顶部无返回条。进入
  `?item=vokie` 详情后 `can_go_back=true` 返回条出现，点返回后恢复 false。
  安装版深色启动同样只有 `mode=create theme=dark`，UIA 按钮树无「返回上一页」；
  安装版浅↔深切换均为 `mode=theme_replace`、`can_go_back=false`，同样无返回条。
- 真实 5xx：CDP `Fetch.fulfillRequest` 返回 500 → `http_status=500
  reason=http_error`；失败页显示固定文案与「打开官网 / 重试」。
- 真实断网：死代理 `--proxy-server=127.0.0.1:9` → `success=false
  http_status=0 reason=navigation_failed`；DNS 失败（`direct://` +
  `host-resolver-rules MAP sayall.app 127.0.0.1`）同样落到
  `reason=navigation_failed`，失败页出现。
- 离开即停止：Fetch 挂起整页刷新后点「按键」→
  `hide phase=loading stopped=true`；22s 后无 `reason=timeout`，迟到的完成事件
  记为 `result=ignored reason=no_pending_navigation`（`web_error_status=9`）。
- 窗口缩放：1081×837 → 900×700 时子 WebView 由 959×800 变为 777×663，
  恢复后回到 959×800（`relayout` 的 Resized 路径）。
- 托盘/二次启动恢复：`window_close source=ctrl_w` 隐藏后置位
  `Local\SayAll-ShowMainWindow` → `show_main_window trigger=second_instance
  terminal_result=passed visible_after=true`；子 WebView 重新可见，推荐页
  `ready_signal` 时间戳不变（保留文档，无重新导航）。
- 前端 `vitest run`：28 个文件、480 passed | 10 skipped；`vue-tsc --noEmit` 通过。
  Rust 推荐页 11 个单测通过；`cargo test --workspace` 本轮全通过（436 + 152 +
  67 + 4 等，0 failed；此前一次并行负载下 `rc003_bridge` 用例抖动，单独重跑与
  既有记录均通过）。
- 本地测试安装包 passed：`target/release/bundle/nsis/无线麦 SayAll_1.0.0_x64-setup.exe`
  （13,008,260 字节，SHA-256
  `0BD9020CD1AD69BC42F9FC734840438F455F7B50C9363619E623F4BCE4185048`，构建源
  `c0d79f6` + 未提交的本次改动，含子 frame 拦截与主题切换历史修复）；静默安装
  退出码 0，安装版深色启动仅 `show mode=create theme=dark`、`can_go_back=false`、
  UIA 无「返回上一页」；安装版在设置里浅↔深切换后进入推荐页均为
  `mode=theme_replace`、`can_go_back=false`、无返回条；进入推荐页
  `ready_signal passed http_status=200`，离开后重进 `hide phase=ready stopped=false`
  → `show mode=retained phase=ready`（`can_go_back=false`）。

deferred：跨屏 DPI（本机只有一块 3840×2160 / 150% 显示器，`ScaleFactorChanged`
只做了代码接入，需双显示器真机复核）；加载期间显示网页（Windows 产品规范要求
隐藏页面并显示「正在加载网页…」，与 Mac 的 `showsLoadingOverlay: false`
是容器差异，未改）。

## 2026-10-24 应用语言与推荐页英文版

设置页「通用」新增「应用语言」：跟随系统 / 简体中文 / English，持久化在
`AppSettings.application_language`（`system` / `zh-Hans` / `en`，与 Mac
`AppLanguage` 同值域）。`system` 按系统语言归一：`zh*` → 简体中文，其余 → 英文。

- 推荐页按有效语言选择 `/recommendations/` 或 `/en/recommendations/`；首次加载
  等待主题与语言初始化完成，避免先中文后英文双载。
- 英文固定文案对齐 Mac：Picks / Loading page… /
  This page is temporarily unavailable. Open the website or try again later. /
  Open website / Retry / Back。
- 主题/语言切换对已就绪文档用 `location.replace` 保留当前详情（`?item=`）且
  不新增后退条目（`mode=theme_replace` / `language_replace`）。
- **重试 / App 内重新加载不进入历史**：重试、非 ready 状态下的主题/语言切换、
  首次创建成功后通过 `Page.resetNavigationHistory` 清空 WebView 历史；
  `history_reset_pending` 防止重置完成前返回条闪现；失败、超时、离开页面或
  进程失败会清掉待重置标记，避免影响下一次站内导航。
- 「打开官网」按当前页语言与详情参数交给系统浏览器。
- 范围：本轮只有推荐页跟随语言切换；设置行与其他 Windows 界面仍为中文，
  全量界面翻译列为后续。

验证 passed（调试构建 + 安装版）：

- 设置页三档单选：选 English → `application_language ... save passed
  preference=en`；回推荐页 `mode=language_replace language=en`，子 WebView URL
  `/en/recommendations/`；重启后原生读回 `preference=en`，选中状态一致。
- 英文失败页：CDP 挂起主文档 20s → 英文固定文案与 Open website / Retry；点
  Retry `mode=retry language=en`，恢复 ready。
- 英文详情：`can_go_back=true`，顶部按钮文字 `Back`；点后退 `go_back passed`，
  `can_go_back=false`。
- 语言切回简体中文：`mode=language_replace language=zh-Hans`，URL 回到
  `/recommendations/`；再切 English 同样通过。
- 重试与历史：页内 reload 超时、语言切换网络失败、详情+主题切换失败后点「重试」
  均 `history_reset result=passed can_go_back=false`，顶部无返回条；页内普通
  reload / 同 URL 导航不新增历史；站内 `pushState` 仍正常触发
  `HistoryChanged`（真实历史由网页决定）。
- 设置页「通用」不再显示各行副标题（2026-10-24 用户要求）；必要说明只保留在
  控件 `title` 中。
- 前端 29 个文件、487 passed | 10 skipped；`vue-tsc --noEmit` 通过。Rust 推荐页
  11 个单测、`cargo test --workspace`、`cargo fmt --all -- --check` 通过。
- 本地测试安装包 passed：`target/release/bundle/nsis/无线麦 SayAll_1.0.0_x64-setup.exe`
  （12,985,755 字节，SHA-256
  `377F510D35A9B69ACA62754E997E87E059C5015049497D76AB9DB9029489911B`，含重试
  历史清理；该包按当时本地工作区构建，含并行未提交改动，PR 分支本身由 CI
  verify 验证）；静默安装退出码 0；安装版启动即
  `history_reset passed can_go_back=false`，推荐页 `ready_signal passed
  http_status=200`，UIA 无「返回上一页」。英文路径在同一调试构建与上一安装包
  （`CDC3ABD5…`）已验证，`language_replace language=en` → `/en/recommendations/`。
