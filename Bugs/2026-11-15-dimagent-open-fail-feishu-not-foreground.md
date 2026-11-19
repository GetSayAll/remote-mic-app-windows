# DimAgent 收进托盘后有时打不开；飞书打开后偶尔不在最前台

- 发现日期：2026-11-15
- 状态：已修复（本机真实应用验证 passed）；用户端遥控器复测 `deferred`
- 影响范围：Windows 版 `1.9.3`；按键映射 `volume_down → dimagent`（预设）与 `tv → 飞书.lnk`（自定义快捷方式）；与 RC001/RC003 无关
- 功能点：按键映射「打开应用」——已运行应用的托盘恢复与启动后置前
- 现象：
  1. DimAgent 收进托盘后，有时按打开键没有反应（应用仍在运行，但窗口不出现）。
  2. 飞书已运行并收在托盘（或窗口在后台）时，按打开键有时能打开，但窗口没有到最前台。
- 正常预期：打开键只恢复目标应用已有主窗口并置前，不新起实例；窗口出现在最前台。
- 复现条件：
  1. DimAgent 主窗口 `Chrome_WidgetWin_1`（1078×843）通过关闭按钮收进托盘；
  2. 调用生产路径 `activate_or_launch("dimagent")`；
  3. 或飞书主窗口隐藏/在后台，调用生产路径 `activate_or_launch(<飞书 .lnk>)`。
- 证据（修复前，本机真实应用）：
  - DimAgent 收进托盘后：`phase=tray_icon matched=true submitted=true method=electron_tray_message terminal_result=failed` → `phase=relaunch_skipped reason=running_app_window_hidden`；`application_readiness` 可见窗口仍为 false。直接 UIA Invoke 可见托盘图标同样不显示窗口；只有 `WM_APP + 1` + `WM_LBUTTONDBLCLK` 才恢复主窗口。
  - 飞书 `.lnk` 解析到根目录启动器，真正窗口进程在同目录 `app\` 子目录；`application_readiness` 使用精确路径口径报 `running_process=false visible_window=false`。窗口隐藏时，自定义路径在 `launch_explicit` 后直接返回，没有预设路径的 `foreground_after_launch` 等待，因此窗口出现后可能仍在后台。
- 根因：
  1. Electron 直接托盘消息只发 `WM_LBUTTONDOWN`。QQ/WorkBuddy 的单击回调会显示主窗口，DimAgent 的单击回调不显示、双击才显示；`submitted=true` 只代表消息已投递，不能当成功。随后只等已知隐藏窗口，等不到就返回错误。
  2. 自定义路径（`.lnk` / `.exe`）启动后缺少有界置前：预设路径有 `foreground_after_launch`，自定义路径直接 `return launch_explicit`。
  3. `application_readiness` 的自定义 `.lnk` 口径只按解析出的精确 exe 路径查窗口；飞书的根启动器与 `app\Feishu.exe` 不同路径，导致窗口明明可见却报 false，后续等待/聚焦步骤可能超时。
  4. 已知隐藏窗口偶尔被 DWM cloak，不在 `window_visibility` 候选里；托盘唤回的是另一个主窗口句柄时，只盯旧句柄会把成功误判为失败（QQ 复测时观察到一次）。
- 修复：
  - `crates/sayall-windows/src/tray_icons.rs`：
    - 把 Electron 托盘消息抽成 `try_electron_tray_gesture`，支持 `WM_LBUTTONDOWN` 与 `WM_LBUTTONDBLCLK`；
    - 新增 `invoke_electron_tray_double_click`，只对 `Electron_NotifyIconHostWindow` 发送。
  - `crates/sayall-windows/src/app_launcher.rs`：
    - 新增 `tray_restore_with_double_click`：先单击；Electron 单击未唤回时补一次双击；只对 `method=electron_*` 重试，不对已成功的 QQ/WorkBuddy 重复发送。
    - `tray_restored_window` 增加面积兜底：已知隐藏窗口没变可见时，接受面积不小于隐藏最大窗口一半的可见窗口（`visible_area_matches_hidden`）。QQ 主窗口 595k vs 隐藏辅助窗 480k 接受；微信登录小窗 114k vs 隐藏主窗口 942k 拒绝。
    - 新增 `foreground_after_custom_launch`：自定义 `.exe/.lnk` 启动后按 10×150 ms 有界等待可见窗口并置前；`.lnk` 合并同安装目录同族进程（飞书根启动器 vs `app\Feishu.exe`）。
    - `application_readiness` 的自定义 `.lnk` 分支改用同一同族口径，`running_process` 也检查同族进程。
  - `ATTRIBUTION.md` 补充 Electron `WM_LBUTTONDBLCLK` → `NotifyDoubleClicked` 来源。
- 验证：
  - 新增单测 `tray_area_fallback_accepts_main_window_but_rejects_login_popup` passed；`electron_tray_host_class_is_case_insensitive` passed。
  - 本机真实 DimAgent（预设 `dimagent`）passed：收进托盘后连续 3 次 `activate_or_launch("dimagent")`，日志出现 `tray_retry gesture=double_click method=electron_tray_double_click_message terminal_result=passed`，主窗口可见且位于前台；未再走重新拉起。
  - 本机真实飞书（自定义 `.lnk`）passed：隐藏后 `launch_explicit` → `phase=foreground_after_launch terminal_result=passed`，主窗口 `Chrome_WidgetWin_1` 可见且前台；可见在后台与最小化两种状态也分别验证为前台/还原成功；`application_readiness` 从 `running_process=false visible_window=false` 变为 `true/true`。
  - QQ 回归 passed：收进托盘后连续 3 次打开均恢复主窗口、进程数 8 → 8，无登录小窗；日志 `method=electron_tray_message terminal_result=passed`。
  - 微信回归：直显区 UIA `method=invoke_pattern` 路径未改动，此前 5 → 5 passed。
  - `cargo test --workspace --quiet` passed（`sayall-windows` 562 passed / 17 ignored）；`cargo check -p sayall-windows-app --features runtime-simulation`、`cargo check --workspace`、`cargo fmt --all -- --check` passed。
  - **`deferred`**：RC001/RC003 物理遥控器端到端与用户端安装包复测。
- 已知边界：
  1. 只对 Electron 宿主支持单击/双击直接消息；非 Electron 应用仍走 UIA 托盘路径，若其单击回调不显示窗口且没有可用的双击 UIA 等价路径，会返回可诊断错误，不会强制显示隐藏窗口。
  2. 面积兜底只用于“已知隐藏窗口没变可见”的托盘恢复；启动路径仍以窗口出现为准。
- 隐私检查：未记录个人路径、窗口标题、设备身份、语音内容或凭据；只保留公开窗口类名、布局尺寸、进程计数与策略名。

## 2026-11-15 追加：DimAgent 打开延迟与飞书 open_app 误判

- 用户反馈：DimAgent 从托盘打开仍有很大延时；飞书有时仍不能到最前台，且不如 QQ/微信即时。
- 证据（用户诊断日志，真实安装包）：
  - DimAgent：`tray_single_click_wait phase=single_click_wait elapsed_ms=1200 unresolved=true window_visible=false` → `tray_restored_window phase=area_fallback_wait elapsed_ms=1238` → 随后才补双击成功；单次打开约 2.76 s。DimAgent 的 Electron 托盘忽略单击，前两段固定等待是纯浪费。
  - 飞书：`foreground_after_launch terminal_result=passed` 已成功，但同一动作末尾 `map_launch result=err ... reason=target_unavailable`。配置中 `focusProfiles[飞书.lnk]` 只有 `strategy=recorded_element`，没有 `recorded` 目标；`launch_app_and_focus` 因此返回“该应用还没有学习过输入框”，把已经成功的打开动作判为失败。
- 根因：
  1. 单/双击统一路径先单击，再在已知隐藏窗口和面积兜底上各等一轮固定 1.2 s；DimAgent 单击无效，白白等待约 2.4 s。
  2. “打开应用”把**聚焦档案不完整**也当作打开失败；宿主因此不记录 `target_opened`，后续按键仍按旧目标/旧前景计划处理，表现为“没到最前台/不像 QQ 微信”。
- 修复：
  - `app_launcher.rs`：`tray_restored_window` 改为单轮 `checks` 轮询，同一轮同时检查已知隐藏窗口与面积兜底；新增按 exe 的 `TrayGesturePreference`（Single/Double/Unknown）缓存。Unknown 和已知 Single 必须先等完整单击预算，只有已确认 Double 才直接走双击。2026-11-16 评审移除了 150 ms 快速单击窗口，避免单击有效但恢复较慢的 Electron 应用被误补双击。
  - `button_mapping.rs`：`RecordedElement` 档案缺少 `recorded` 时记录 `open_app_focus_recorded_element phase=skipped reason=not_configured` 并返回 `Ok(())`；聚焦任务提交失败记录 `phase=deferred reason=focus_service_unavailable`，也不再判打开失败；实际聚焦结果仍由异步 `focus_report` 记录。
  - 自定义 `.exe/.lnk` 显式启动不再发送预设 Alt，避免给不需要的前台解锁路径增加一次按键。
- 验证：
  - 临时探针（不提交）`dimagent_gesture_probe`：P2 修改后第一轮 1382 ms（Unknown 先等完整单击预算，避免误发双击），第二轮 72 ms（已学习 Double，直接双击）；比原来的 2.76 s 明显降低，且慢单击 Electron 应用不会再被误补双击。
  - 临时探针 `focus_missing_config_probe`：构造 `recorded_element` 且无 `recorded` 的飞书档案，结果 `result=Ok(())`；日志 `open_app_focus_recorded_element phase=skipped reason=not_configured`，同轮 `show_and_force_foreground terminal_result=passed`。
  - 飞书真实应用隐藏/后台 3/3 次到前台，进程数 8 → 8，未新起实例；单次约 250 ms（探针进程启动另计）。
  - 单测 `tray_gesture_preference_is_learned_per_exe`、`focus_submission_failure_does_not_fail_open_app`、`focus_step_failure_does_not_fail_open_app` passed。
  - `cargo test --workspace --quiet` passed（`sayall-windows` 564 passed / 17 ignored）；`cargo check -p sayall-windows-app --features runtime-simulation`、`cargo check --workspace`、`cargo fmt --all -- --check` passed。
  - 本地测试包（2026-11-15）：`target/release/bundle/nsis/无线麦 SayAll_1.9.3_x64-setup.exe`，SHA-256 `B44E6C07424944E8E5C6B98B39AD4186F6AF942EE9B44C6E8A4C832A334C98AC`；安装后 app exe SHA-256 `8D5EA2DD4FDF278D01EF447A1A0ED0D35A6E5478A525E370706BDF6223E9044E`（与 `target/release` 差 3 字节 bundle 标记），二进制含 `c5442c2`。本包按现有工作树构建，包含未提交的 RC001 HID 变更；这些变更未进入 `c5442c2`。
  - **`deferred`**：RC001/RC003 物理遥控器端到端、用户端安装包复测（包已安装，等待用户启动应用后复测托盘与飞书）。
- 已知边界：
  1. 飞书没有 AUMID（实测 `GetApplicationUserModelId` 返回 15703），托盘图标也没有公开坐标；只能走单实例启动/窗口置前，约 250 ms，无法达到 QQ/微信直接托盘消息的即时性，不改用私有 IPC 或 UIA 托盘飞窗。
  2. 聚焦档案不完整不再阻塞打开；要恢复“聚焦已记录输入框”仍需用户在设置里重新学习。

## 2026-11-16 追加：飞书“偶尔没到最前台”的假阳性

- 用户反馈：安装 `c5442c2` 后仍出现过一次飞书没有到最前台。
- 证据：
  - 用户日志中 13 次飞书打开的 `show_and_force_foreground` 全部是 `class=Chrome_WidgetWin_1 terminal_result=passed target_result=foreground_observed`；失败那次没有留下 failed 记录，说明是**提前误判成功**，不是置前被拒绝。
  - 本地探针反复隐藏飞书主窗口后走生产 `activate_or_launch(<飞书 .lnk>)`：修复前日志出现 `show_and_force_foreground ... class=Tauri Window ... terminal_result=passed`，随后 `phase=foreground_after_launch terminal_result=passed`，但探针读到飞书成为前台是在约 100 ms 之后；即先把**无线麦自己的 Tauri Window** 当成了飞书窗口。
- 根因：自定义 `.lnk` 的 `InstallDirectoryFamily` 兜底传入空 AUMID；`window_visibility` 把空串无条件包成 `Some("")`，`enum_visibility_proc` 的 `by_identity` 便匹配**所有 AUMID 为空的窗口**（包括无线麦自己的 Tauri Window）。飞书窗口隐藏或尚未创建时，`largest_visible` 选中 Tauri Window，`foreground_after_custom_launch` 对它置前并返回成功，飞书只能依赖自身启动时抢前台；抢不到就表现为“偶尔没到最前台”。
- 修复：
  - `window_visibility` 的 `InstallDirectoryFamily` 用 `non_empty_app_user_model_id` 把空 AUMID 转为 `None`，只按 PID/安装目录匹配；
  - `enum_visibility_proc` 再过滤一次空 AUMID，避免任何调用方把空串当身份选择器；
  - 新增单测 `empty_family_aumid_is_not_identity_selector`、`empty_aumid_does_not_match_window_identity`；后者直接覆盖 `enum_visibility_proc` 使用的匹配谓词。
  - 评审 P2：`TrayGesturePreference` 改为 Single/Double/Unknown 三态；Unknown 与已知 Single 都等完整单击预算，只有已确认 Double 才直接双击，移除 150 ms 快速路径；新增 `tray_gesture_preference_is_learned_per_exe` 三态断言。
  - 评审补强：`launch_app` 成功后，`AppShortcut` 缺快捷键/注入失败、`RecordedPoint` 缺位置/解析失败、`AutoComposer` 聚焦服务不可用也统一走 `focus_outcome`，只记录 `open_app_focus...` 并返回 `Ok`；新增 `focus_submission_result`、`focus_outcome` 单测，避免同类“已打开但聚焦失败被判成打开失败”的路径残留。
- 验证：
  - 修复前探针：`class=Tauri Window` 假阳性，飞书在前台检查中为 false。
  - 修复后探针：连续 8/8 轮隐藏飞书后打开，`show_and_force_foreground class=Chrome_WidgetWin_1`，飞书主窗口 `target_foreground`，无 `lost_at`；单次 430–570 ms（等待飞书真实窗口出现，之前的 ~300 ms 是假阳性提前返回）。
  - `focus_missing_config_probe` 实际路径：`recorded_element` 无 `recorded` 时 `result=Ok(())`，日志 `open_app_focus_recorded_element phase=skipped reason=not_configured`。
  - `cargo test --workspace --quiet` passed（`sayall-windows` 568 passed / 17 ignored）；`cargo fmt --all -- --check`、`cargo check --workspace`、`cargo check -p sayall-windows-app --features runtime-simulation` passed。
  - 本地测试包（2026-11-16，含评审补强）：`target/release/bundle/nsis/无线麦 SayAll_1.9.3_x64-setup.exe`，SHA-256 `B41617E4ED08DAA770B47331A7DF446784F38EF5857A924E8CA4974FD129F5BE`；安装后 app exe SHA-256 `894559182AC2823B8D834BE88B53880C4AC678C542D9ED09A152C82FD7C40D7A`，二进制含 `2a0ee83`。本次构建时工作树无 RC001 HID 变更。
  - **`deferred`**：用户端再次复测；如果仍出现，需在失败时刻记录当时 `GetForegroundWindow` 的类名/PID，以区分“错误候选窗口”与“置前后被抢”。
