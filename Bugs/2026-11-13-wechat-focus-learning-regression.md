# 1.9 回归：微信「聚焦学习」失效，记录点击位置入口消失

- 发现日期：2026-11-13
- 状态：已修复（自动化 passed；真实微信目视验收 deferred）
- 影响范围：1.9.0 / 1.9.1；组合动作「聚焦学习过的输入框」、APP 适配页预置应用、按键页「打开 APP + 聚焦」
- 功能点：聚焦档案解析、预设应用聚焦配置、`recorded_point` 学习
- 现象：用户反馈 1.8 能学会聚焦微信，1.9 怎么学都不成功；想把「打开微信」+「聚焦到某个群」做成组合动作，学不出来，也不知道去哪里操作。
- 复现条件：Windows + 微信 4.x；在组合动作里选择内置预设「微信」作为聚焦目标，或在 APP 适配页手动添加微信。
- 正常预期：微信 4.x 不暴露 UIA 树，应允许用户记录点击位置；已学过的档案应优先于预设默认值执行。

## 证据

- 微信 4.x 主窗口与渲染子窗口的 `OBJID_CLIENT` / `UiaRootObjectId` 都返回 0，UIA 树没有可聚焦输入框；只能走 `recorded_point`（`Bugs/2026-10-15-focus-address-bar-and-recorded-point.md`、`TODO.md` 已知边界）。
- 1.8.3 的 `open_app_effective_profile` 顺序是「显式档案 → 预设默认」；1.9 的 `focus_profile_for_target` 改成「预设 → 显式档案」，微信的预设默认 `auto_composer` 覆盖了用户学过的 `recorded_point`（`crates/sayall-windows/src/button_mapping.rs`）。
- 1.9 的 `AppFocusProfileEditor` 只保留「只打开 APP / 打开后自动聚焦」两个主选项，`recorded_point` 不再提供入口。
- APP 适配页对预置应用只显示只读「预置能力」卡，不渲染编辑器；组合动作页对内置预设不显示摘要和「去配置」。
- 数据未丢失：`migrate_preset_focus_profiles` 会保留 `recorded_point` / `recorded_element` / `app_shortcut`，只是执行时被跳过。

## 根因

1. **执行侧**：预设优先解析导致 1.8 已学档案失效。
2. **界面侧**：记录点击位置入口被移除，预置应用没有可进入的学习界面，组合动作页也没有配置入口。

## 修复

- `focus_profile_for_target` 恢复「显式档案优先，预设默认兜底」，并支持预设 id / 真实路径两种档案键互相查找。
- 前端统一使用 `effectiveProfileForTarget`：按键页、组合动作页、APP 适配页同一口径。
- `AppFocusProfileEditor` 恢复「记录点击位置」选项与 6 秒学习流程；APP 适配页对没有可靠内置聚焦的预置（微信等 `auto_composer`）或已有显式档案的预置显示编辑器。
- 组合动作页为预置目标显示聚焦摘要和「去配置」；显式导航到未加入应用库的预置时，APP 适配页显示该预置的详情，但不写入应用库。
- 列表摘要显示实际聚焦方式；预置聚焦卡区分「预置可用 / 需设置 / 已自定义」。

## 验证

- `cargo fmt --all -- --check` passed。
- `cargo test --workspace -j 1` passed：`sayall-windows` 556 passed / 17 ignored；`sayall-windows-app` 220 passed / 2 ignored；其余 workspace 测试 passed。
- `cargo check -p sayall-windows-app --features runtime-simulation` passed。
- `pnpm test` passed：689 passed | 10 skipped。
- `pnpm build`（`vue-tsc --noEmit` + `vite build`）passed。
- 新增回归用例：微信 `recorded_point` 档案优先于预设默认；预置微信可学习点击位置并按预设 id 落盘；预置目标显示摘要与「去配置」；旧 `recorded_point` 档案在编辑器里可见可执行；预置滚动区域可学习覆盖并在清除后恢复预置。
- 本地测试包（基于 `origin/main` `ac2e7e3` + 代码提交 `9fab377`）：`target/release/bundle/nsis/无线麦 SayAll_1.9.1_x64-setup.exe`，SHA-256 `10F9207821EE0F8C372C4F8E32F98F8800A1DCB0130AAAA515F4E6D9E9C42B41`（本地测试密钥签名，不可发布）；前端嵌入校验 18/18 passed。
- 安装校验 passed：helper / gadget / sayall-mcp 哈希对齐，app exe 仅 3 字节 UNK→NSS 差异，内嵌修订号 `9fab3776` count=1；启动截图 `C:\Users\hd838\AppData\Local\Temp\sayall-installed-145713.png` 确认 1.9.1 的 APP 适配页：预置 APP 不再显示只读「预置能力」卡；微信行显示「自动选择 · 预置可用」。
- 布局核对：浏览器实测 1280×800 与 1150×1500 视口下，「添加 APP」按钮底部分别为 742.6 / 1442.6，均在可视区域内且留出约 58px 间距；聚焦方式顺序为 自动选择(0) → 只打开(1) → 打开后自动聚焦(2) → APP 快捷键(3) → 记录点击位置(4)，自定义 APP 不显示自动选择。
- 真实微信目视确认：**deferred**。需要在本机微信 4.x 里记录一个群聊输入框位置，确认「打开微信 → 聚焦到该群」实际点击落点正确。

## 隐私检查

本记录未包含个人路径、设备身份、聊天内容或凭据；只保留代码路径、策略名和测试计数。

## 2026-11-14 追加：预置 APP 与普通 APP 一致

用户要求移除只读「预置能力」卡：预置 APP 也显示聚焦方式与滚动区域，预置默认策略显示为选中并标「预置可用」；用户仍可重新学习覆盖，滚动区域清除后恢复预置。聚焦方式按 0–4 顺序排列，自动选择输入框仅预置应用提供并默认选中，提示改为「预置可用」；删除重复的「改用 APP 快捷键聚焦」入口与聚焦方式右侧「已自定义」提示；左侧「添加 APP」固定在列表滚动区底部。已同步修改 `AppsPage`、`AppFocusProfileEditor`、`focus-defaults` 与滚动区域预设 id/路径别名解析；自动化 passed。

## 2026-11-14 追加：聚焦与翻页、学习记录保留、DimAgent 滚动

- 代码提交：`f4ab08f`（`fix(apps): 聚焦与翻页文案、学习记录保留与 DimAgent 滚动`）。
- 页面命名与文案对齐 Mac PR #1040：侧栏/页面标题改为「聚焦与翻页」，英文页标题 `Focus & Scrolling`、窄侧栏 `Focus/Scroll`；副标题改为「按 App 单独设置输入框聚焦与翻页滚动；只要该 App 在前台就能用，不会改变按键绑定。」；「绑定到它的按键」改为「哪些按键会打开它」；删除入口改为「删除这个 App 的设置」。来源与边界见 `ATTRIBUTION.md`。
- 移除「记录点击位置」右侧的「界面不暴露输入框时」提示，移除快捷键策略下方的「改回打开后自动聚焦」链接。
- 学习记录保留：`AppFocusProfile::normalized` 现在保留所有有效载荷，策略只决定执行哪条；`profileForStrategy` 切换时携带 `recorded` / `point` / `shortcut`。新增前端与后端回归测试；旧版 1.9.x 在降级回读时仍可能按旧规则清掉非当前策略载荷，这是版本兼容边界。
- DimAgent 滚动：日志显示保存的是 `control_type=Window`，旧回退走 `PostMessage(WM_MOUSEWHEEL)`，Chromium/Electron 渲染子窗口忽略该消息（`method=wheel moved=false`）。现改为 `SendInput` 真实滚轮：临时把物理光标移到区域中心，发送后恢复；左键按住或光标定位失败时才回退旧消息路径。日志新增 `input=send_input|post_message` 与 `cursor_restored`。临时探针调用生产 `scroll_foreground` 后，DimAgent UIA 文本位置统计由 `sum=107757` 变为 `129199`，确认内容实际滚动；探针已移入回收站。安装版测试按钮也记录到 `input=send_input` / `cursor_restored=true`；但 UIA 点击测试按钮会把 SayAll 拉回前台，无法用该次内容位移做目视判据，真实遥控器前台触发仍列为 deferred。
- 左侧列表布局：短列表时面板高度随内容收缩，「添加 App」紧跟最后一行并保留 8px 间距；长列表时面板最高 `calc(100vh - 150px)`，列表滚动、按钮固定在面板底部。浏览器实测 1067×800：3 行时面板高 242px、按钮顶部 302.6px；20 行时面板高 650px、列表可视 552px、滚动内容 985px、按钮底部 742.6px，均在可视区内。
- 验证：`cargo fmt --all -- --check` passed；`cargo test --workspace` passed（`sayall-windows` 556 passed / 17 ignored，`sayall-windows-app` 220 passed / 2 ignored，其余 workspace passed）；`cargo check -p sayall-windows-app --features runtime-simulation` passed；`pnpm test` 690 passed / 10 skipped；`pnpm build` passed。
- 本地测试包：`target/release/bundle/nsis/无线麦 SayAll_1.9.1_x64-setup.exe`，SHA-256 `D58CAEB91808797619188B78B6ECB6E14FEE011787B4C105EFA58EF3F6B236FD`，大小 13485486 字节（本地测试密钥签名，不可发布）；app exe SHA-256 `3CFB97EC71A072F07385C85D695FE856A4DD6A5A832F98492B636B2176769C37`，前端嵌入校验 18/18 passed（对 app exe 校验；NSIS 安装包经过压缩，不能直接做资源键阳性判据）。
- 安装校验 passed：helper / gadget / sayall-mcp 哈希对齐；app exe 与构建产物仅 3 字节 UNK→NSS 差异；内嵌修订号 `f4ab08f` count=1；安装版启动截图 `C:\Users\hd838\AppData\Local\Temp\sayall-installed-155725.png` 确认侧栏与页面标题为「聚焦与翻页」、「添加 App」和「删除这个 App 的设置」文案生效。
- 真实遥控器按「向上/向下翻页」触发 DimAgent 滚动、以及微信目视聚焦仍为 **deferred**：需要 RC001/RC003 真机与用户在前台操作目标 App 确认最终用户可见结果。

## 2026-11-14 追加：底栏、聚焦方式徽标与 AppsFolder 预设识别

- 代码提交：`1017395`。
- 左侧列表改为 `height: fit-content`：短列表不再被父容器拉伸，`添加 App` 距面板底部为 12px padding；长列表仍由 `max-height: calc(100vh - 150px)` 限制并滚动，按钮固定在列表末尾。浏览器实测 1150×1500：面板底部 694.1px、按钮底部 681.1px（13px）；1067×800 同样为 13px。
- 快捷键策略的 `shortcut-footer` 只有移除「改回打开后自动聚焦」后留下的空行；现改为只在有 `shortcutMessage` 时渲染消息段落，不再保留 30px 空 footer。`测试打开与聚焦` 紧跟内容，聚焦卡片高度由 270px 降到 232px。
- 「预置可用」从「自动选择输入框」选项内部移到「聚焦方式」标题右侧；仅当 `allowAutoComposer && strategy === "auto_composer"` 时显示，样式为 `font-size: 11.5px / weight 600 / var(--success-text)`，与滚动区域的「预置可用」计算色 `rgb(38, 113, 72)` 一致。自动选择以外的策略不显示。
- Chrome / WorkBuddy 等应用库目标使用 `shell:AppsFolder\...`，前端按路径无法匹配预设。新增 `app_launcher::preset_id_for_target`（AppsFolder 目标先解析真实 exe，再与预设探测路径精确比对；Store 目标按已知 AUMID 匹配）与 Tauri 命令 `resolve_preset_target`；APP 适配页加载时把应用库目标解析为预设 id，再传给 `buildAppRows`。安装版已确认 Google Chrome / ChatGPT / Vokie 等行显示「预置可用」，Chrome 详情出现「自动选择输入框」选项；WorkBuddy 选中后同样出现该选项（其已有显式 recorded_element 档案，因此列表摘要仍显示「已记录输入框」）。
- 验证：`cargo fmt --all -- --check` passed；`cargo test --workspace` passed（`sayall-windows` 556 passed / 17 ignored，`sayall-windows-app` 222 passed / 2 ignored）；`cargo check -p sayall-windows-app --features runtime-simulation` passed；`pnpm test` 692 passed / 10 skipped；`pnpm build` passed。
- 本地测试包：`target/release/bundle/nsis/无线麦 SayAll_1.9.1_x64-setup.exe`，SHA-256 `1C0F015D7F2925956AE3939A1FDA703AC0F38A86B566AA42BAA8F9C724053468`，大小 13493599 字节（本地测试密钥签名，不可发布）；app exe SHA-256 `9870F30BAC8D3C0DEB27CCE81607C8C87CEEEA3CB75D40DF9F88E52EC79F8A2C`；前端嵌入校验 18/18 passed。
- 安装校验 passed：helper / gadget / sayall-mcp 哈希对齐；app exe 仅 3 字节 UNK→NSS 差异；内嵌修订号 `1017395` count=1；安装版截图 `C:\Users\hd838\AppData\Local\Temp\sayall-installed-164340.png` 确认 Chrome / ChatGPT / Vokie 的预置状态与「聚焦方式」区域。
- 仍未覆盖：真实遥控器翻页与真实微信目视聚焦（deferred，需真机前台操作）。

## 2026-11-14 追加：移除筛选、组合动作「去配置」与跨页记录同步

- 代码提交：`aceac4a`（`fix(apps): 移除筛选并同步聚焦与翻页状态`）。
- 移除按键页「筛选已添加应用」输入框与对应过滤逻辑；移除组合动作页「搜索组合动作」输入框。两项都是重复入口，保留原生列表滚动。
- 组合动作聚焦步骤摘要始终显示「去配置」链接，不再用「预置可用」徽标替代入口；按键页同样只保留链接。
- 跨页记录同步：`focusTargetPresetId` 之前只匹配精确 id / 预设路径，`shell:AppsFolder\Chrome` 这类目标在按键页和组合动作页解析不到预设 id，导致已经保存到 `focusProfiles[chrome]` / `scrollRegions[chrome]` 的聚焦方式与学习记录回退成预设默认或「需学习」。两页现在都调用 `resolve_preset_target` 解析 AppsFolder 目标，并以 `effectiveFocusPresetId`（精确 id 优先、异步解析兜底）读取显式档案与滚动区域；解析按 generation 防迟到回调覆盖。页面重新挂载时从磁盘重读，因此 Apps → 按键/组合动作切换后能看到同一份记录。
- 列表底栏间距：`.list-panel` 底部 padding 由 12px 收紧到 8px，`.library-pane` 由 14px 收紧到 10px。浏览器实测 1150×1500、应用列表 9 行时，「添加 App」按钮底部 681.1px、面板底部 690.1px，可视间距 9px（8px padding + 1px 边框）；按钮仍在列表滚动区末尾。
- 验证：`pnpm test` passed 693 / 10 skipped；`pnpm build` passed；`cargo fmt --all -- --check` passed；`cargo test --workspace` passed（`sayall-windows` 556 passed / 17 ignored，`sayall-windows-app` 220 passed / 2 ignored，其余 workspace passed）；`cargo check -p sayall-windows-app --features runtime-simulation` passed。新增/调整回归用例覆盖 AppsFolder 预设目标在按键页与组合动作页同步显式聚焦档案和滚动记录、组合动作摘要始终有「去配置」、两处筛选控件不再出现。
- 本地测试包：`target/release/bundle/nsis/无线麦 SayAll_1.9.1_x64-setup.exe`，SHA-256 `03192695F53C1EE73B0B47A271577055C4C1C0B8ECA1BF59B416E815DED6C90B`，大小 13495217 字节（本地测试密钥签名，不可发布）；app exe SHA-256 `5579787E5B6C3137E1828DBB620EDF0379A53E3E5E551CE63AE2D8CAE398968F`。
- 安装校验 passed：helper / gadget / sayall-mcp 哈希对齐；app exe 仅 3 字节 UNK→NSS 差异；内嵌修订号 `aceac4a` count=1；安装版启动截图 `C:\Users\hd838\AppData\Local\Temp\sayall-installed-173347.png` 确认 1.9.1 的「聚焦与翻页」页正常加载。
- 仍未覆盖：真实 RC001/RC003 滚动与真实微信目视聚焦（deferred，需真机前台操作）。

