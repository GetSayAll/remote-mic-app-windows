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
