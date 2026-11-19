# QQ 收进托盘后打开：同时出现 QQ 主窗口与登录窗，主窗口点不动

- 发现日期：2026-11-15
- 状态：已修复（自动化 + 本机真实 QQ 复测 passed）；遥控器按键端到端复测 `deferred`
- 影响范围：Windows 版 `1.9.3`；从「打开应用」的 QQ 预设或指向 `QQ.exe` 的自定义目标打开已运行、主窗口收在托盘的 QQ；与 RC001/RC003 无关
- 功能点：按键映射「打开应用」——已运行应用的托盘恢复
- 现象：QQ 主窗口收进托盘后，用遥控器打开 QQ，会同时出现原来的 QQ 主窗口和一个登录/入口小窗；主窗口可见但不能响应鼠标点击。
- 正常预期：只把 QQ 已有的主窗口唤回并置前，不新起进程、不出现登录小窗、窗口可交互。
- 复现条件：
  1. QQ 已登录运行，主窗口通过关闭按钮收进托盘（`Chrome_WidgetWin_1` 960×620 隐藏，进程数 8）；
  2. 触发映射为「打开应用 → QQ」的按键（生产路径 `activate_or_launch("qq")`）。
- 证据（修复前，本机真实 QQ）：
  - `phase=tray_icon matched=false submitted=false method=not_found terminal_result=failed`；
  - `phase=relaunch_entry reason=window_hidden_by_app visible=0 hidden=3` →
    `phase=launch_target source=start_menu_shortcut`；
  - 重新拉起后进程数 8 → 12，新出现一个 640×424 的可见登录/入口窗；
  - `phase=relaunch_entry terminal_result=failed reason=app_window_not_observed` →
    `phase=relaunch_fallback target=largest_hidden_window class=Chrome_WidgetWin_1 size=960x620 terminal_result=passed`；
    `show_and_force_foreground ... visible_before=false took_show_path=true`。
  - 结果与用户描述一致：主窗口是外部 `ShowWindow` 出来的，应用内部仍认为它隐藏，客户端区点击不进入应用；登录小窗则是重新拉起的副作用。
- 根因：
  1. QQ（Electron）的托盘图标**没有 UIA 名称/tooltip**，且默认在「显示隐藏的图标」折叠区。旧 `invoke_tray_icon` 只按 tooltip 搜索直显区，QQ 必然 `matched=false`。
  2. 预设表用 `relaunch_when_hidden: true` 让 QQ 在托盘恢复失败后按入口重新拉起；QQ 与微信 4.x 一样，已运行时再次启动会新起登录/入口小窗。
  3. 重新拉起没有让已知主窗口自己变可见，旧逻辑回落到直接 `ShowWindow` 隐藏主窗口；该窗口因此保留应用内部隐藏态，鼠标点击不生效。
  4. 微信此前用 `relaunch_when_hidden: false` 做了单应用例外，但 QQ、以及任何后续应用仍会走同一条有缺陷的通用分支——这不是应用之间的逻辑不同，而是预设表里的逐应用开关。
- 修复：
  - `crates/sayall-windows/src/tray_icons.rs`：
    - 保留直显区 tooltip 匹配；
    - 新增基于目标进程的托盘图标定位：枚举目标进程窗口 → `Shell_NotifyIconGetRect` 取该进程自己的通知图标矩形 → 在直显区/折叠区 UIA 树中按矩形匹配（不依赖 tooltip）；
    - 折叠区开关优先按本地化名称识别，名称不匹配时用「紧贴最左侧 `NotifyItemIcon` 左侧的 `SystemTrayIcon` 按钮」几何关系；
    - 浮层是分层 XAML 动画，打开后等待 500 ms 再 Invoke（本机实测：立即 Invoke 会被忽略）；
    - 浮层入口不可用/图标找不到时 `matched=true`，调用方不得因此重新拉起。
  - `crates/sayall-windows/src/app_launcher.rs`：
    - 删除预设表的 `relaunch_when_hidden` 逐应用开关；
    - 主窗口隐藏时**只走应用自己的托盘图标**：唤回成功 → 置前；失败 → 返回可诊断错误，不再重新拉起、不再强制 `ShowWindow`；
    - 进程在跑但没有任何合格窗口时：有托盘图标 → 只走托盘；没有托盘图标（资源管理器、Chrome 后台进程等）→ 允许启动新窗口（`tray_policy_launch_allowed`）；
    - 自定义路径复用同一策略与托盘候选名（含预设显示名）；
    - 新日志：`phase=relaunch_skipped reason=running_app_window_hidden`、
      `phase=launch_allowed reason=hidden_window_without_tray_icon`、
      `phase=no_window_tray`、`phase=launch_skipped reason=running_app_tray_not_restored`。
- 验证：
  - 新增单测：
    - `tray_icons::tests::tray_icon_rect_matching_tolerates_shell_rounding_but_not_neighbors` passed；
    - `tray_icons::tests::overflow_toggle_name_matching_covers_supported_labels` passed；
    - `app_launcher::tests::running_process_without_tray_icon_may_launch_but_with_tray_icon_must_not` passed；
    - `app_launcher::tests::custom_weixin_target_uses_preset_hidden_policy` 更新后 passed。
  - `cargo test --workspace --quiet` passed：`sayall-windows` 559 passed / 17 ignored；其余 crate 全绿。
  - `cargo check -p sayall-windows-app --features runtime-simulation` passed；`cargo fmt --all -- --check` passed。
  - 本机真实 QQ（生产路径 `preset_apps_probe qq`）passed：主窗口收进托盘后，
    `phase=tray_icon matched=true submitted=true method=overflow_invoke_pattern terminal_result=passed` →
    `phase=tray_foreground terminal_result=passed`；主窗口可见、进程数保持 8、没有登录小窗；
    `show_and_force_foreground ... visible_before=true took_show_path=false`（证明没有外部强制显示）。
  - 本机真实 QQ 自定义路径（生产方法 `activate_or_launch`）passed：`target_kind=custom phase=tray_icon matched=true submitted=true method=overflow_invoke_pattern terminal_result=passed`，进程数保持 8。
  - 微信回归 passed：主窗口收进托盘后 `preset_apps_probe wechat` → `method=invoke_pattern terminal_result=passed`，进程数保持 5，无登录小窗。
  - 资源管理器回归 passed：`app_readiness_probe --activate explorer` → `phase=launch_allowed reason=hidden_window_without_tray_icon`，新窗口出现并置前；没有因“已运行”被卡死。
  - 本地测试包 `target/release/bundle/nsis/无线麦 SayAll_1.9.3_x64-setup.exe` 静默安装 exit 0 passed：`sayall-helper.exe` / `frida-gadget.dll` / `sayall-mcp.exe` 与源产物哈希一致；安装后的 `sayall-windows-app.exe` 与 `target/release` 只差 3 字节（`UNK`→`NSS` bundle 标记）；前端 18/18 资源嵌入校验 passed；启动截图确认是应用界面而不是目录列表。包路径、SHA-256 与内嵌修订号见本次交付消息。
  - **`deferred`**：RC001/RC003 遥控器真实按键端到端、用户端安装包复测，以及“点击主窗口可用”的人工确认。本机验证走的是与按键相同的生产函数，但没有物理遥控器按键。

## 第二轮（2026-11-15 用户复测后追加）：每次打开 QQ 都会先展开托盘浮层

- 现象：第一轮修复后不再出现登录小窗，但每次从托盘打开 QQ，Windows 11 的“显示隐藏的图标”浮层会先展开一下；微信没有这个过程。
- 根因：QQ 的托盘图标在折叠区，第一轮通过 UIA 打开折叠区浮层再 Invoke 图标。浮层是可见的 shell UI，用户必然看到展开动画。微信图标在直显区，按 tooltip 直接 Invoke，所以没有这个动作。
- 修复：增加 Electron 托盘宿主的直接点击消息（`crates/sayall-windows/src/tray_icons.rs`）：
  - 只对窗口类 `Electron_NotifyIconHostWindow` 生效；
  - 用 `Shell_NotifyIconGetRect` 找到目标进程的图标 id，向宿主窗口 `PostMessageW(WM_APP + 1, wParam=icon_id, lParam=WM_LBUTTONDOWN)`；
  - 等价于用户左键点托盘图标，但不打开折叠区浮层；
  - 失败才回落到第一轮的 UIA 折叠区路径；非 Electron 应用行为不变。
  - 来源与边界记录在 `ATTRIBUTION.md`：Electron `notify_icon_host.cc` 的 `kNotifyIconMessage = WM_APP + 1`、`WM_LBUTTONDOWN` → `NotifyClicked`。
- 验证（本机真实应用）：
  - QQ 收进托盘后 `preset_apps_probe qq` → `method=electron_tray_message terminal_result=passed`，耗时约 250 ms，折叠区保持关闭，主窗口可见，进程数 8 → 8，无登录小窗。
  - WorkBuddy 收进托盘后同样 `method=electron_tray_message passed`，无折叠区展开。
  - 微信仍走直显区 `method=invoke_pattern passed`，进程数 5 → 5。
  - 飞书（非 Electron 默认宿主 / 托盘图标不在可扫描矩形内）仍回落为“无托盘图标 → 启动入口”，单实例把它已有的主窗口恢复，进程数没有持续增加。
- **`deferred`**：遥控器物理按键端到端与用户侧肉眼确认“不再展开托盘浮层”。

- 已知边界（未在本次修复范围）：
  1. 扫描本机应用（`shell:AppsFolder\…` 注册目标）仍有自己的激活契约路径；本机 QQ 使用预设 `target=qq`，未走该路径。若后续把 QQ 加入扫描应用并复现同类症状，再对注册路径应用同一“有托盘图标就不重新激活”的策略。
  2. 目标进程登记了托盘图标、但折叠区入口本身不可用时，现在返回错误而不是新起实例；用户需手动点击托盘图标。
  3. 托盘提示文本为空的应用必须依赖 `Shell_NotifyIconGetRect` + 矩形匹配；若第三方应用不使用标准 `NOTIFYICONDATA` 登记图标，仍会落到“没有托盘图标”的启动分支。
- 隐私检查：未记录个人路径、窗口标题、设备身份、语音内容或凭据；只保留公开窗口类名、布局尺寸、进程计数与策略名。
