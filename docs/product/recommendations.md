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
| 主文档失败/超时 | 子 WebView 隐藏，显示「暂时无法加载此页面，你可以前往官网查看，或稍后重试。」+「打开官网」+「重试」 |
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
