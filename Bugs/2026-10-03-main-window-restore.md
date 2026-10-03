# 主窗口最小化后：托盘左键 / 二次启动（双击快捷方式）无法把窗口打开（2026-10-03）

- 发现日期：2026-10-03
- 状态：已修复（本机真机验证 passed；待用户安装本地测试包后复验）
- 影响范围：Windows 版（用户现场为 15:57 启动的安装版）；托盘左键、"显示主界面"菜单项、双击快捷方式（二次启动）；与 RC001/RC003、BLE、语音链路无关（纯窗口层）
- 功能点：托盘驻留与主窗口显示（`src-tauri/src/lib.rs` 托盘事件、单实例守卫；`crates/sayall-windows/src/instance_signal.rs`）
- 现象：主窗口最小化到任务栏后，点击托盘图标没有可见反应（托盘菜单"显示主界面"同样）；应用已在运行时双击桌面/开始菜单快捷方式也没有任何反应（第二个实例被单实例守卫静默拦下后直接退出）。
- 复现条件：
  1. 启动 SayAll（修复前版本），把主窗口最小化（标题栏最小化按钮/任务栏"最小化"）；
  2. 点击通知区域（或溢出区）的托盘图标 → 窗口不恢复（前台/任务栏按钮也无变化）；
  3. 保持应用运行、窗口最小化，双击快捷方式 → 同样无反应。
- 正常预期：两条入口都应把主窗口从最小化（或收进托盘）恢复为可见并尽力置前。

## 证据（根因：源码级 + 最小实验 + 真机复现）

**tao 0.35.3 源码（`%USERPROFILE%\.cargo\registry\...\tao-0.35.3`）**：

- `platform_impl/windows/window_state.rs::apply_diff`：仅当 flag 有差异时才调用 Win32，`diff == empty` 时**直接返回**。最小化窗口仍处于"可见"状态（`VISIBLE` 未变），因此 `window.show()`（`set_visible(true)`）**整个是空操作**——连 `ShowWindow` 都不会被调用；
- `platform_impl/windows/window.rs::set_focus`：`is_visible && !is_minimized && !is_foreground` 才调用 `force_window_active`；缓存 `MINIMIZED=true` 时**整个跳过**；
- `window_state.rs` 的 `MINIMIZED` 差异处理：清除时调用 `ShowWindow(SW_RESTORE)`（即 `unminimize()` 才是恢复最小化的正确调用）。

**最小实验（本机 PowerShell + Win32，记事本对照太不稳，改用自建 WinForms 窗口）**：对最小化窗口依次调用

| 调用 | `IsIconic` 结果 |
| --- | --- |
| `SW_MINIMIZE`（进入最小化） | true |
| `SW_SHOW`（= tao `show()` 落到的 Win32 调用） | **true（未恢复）** |
| `SW_RESTORE`（= tao `unminimize()` 落到的调用） | false（恢复） |

**单实例守卫**：`ERROR_ALREADY_EXISTS` 分支此前只写日志并 `return`，没有任何通知已运行实例的通道——二次启动必然无反应。

**真机复现（修复前，安装版 pid 27224，`Testing/probe-main-window-restore.ps1`）**：

| 用例 | 结果 | 读回 |
| --- | --- | --- |
| 用例1 最小化(SC_MINIMIZE) → 托盘左键 | failed | iconic=true visible=true |
| 用例2 最小化(SW_MINIMIZE) → 托盘左键 | failed | iconic=true visible=true |
| 用例3 关闭到托盘 → 托盘左键 | passed | iconic=false visible=true（隐藏路径本来就正常，与源码分析一致） |
| 用例4 最小化 → 二次启动 | failed | 第二实例退出（`second_exited=true`），窗口仍 iconic=true |

## 修复

1. `src-tauri/src/lib.rs` 新增 `show_main_window(app, trigger)`：
   `unminimize()`（tao 缓存 `MINIMIZED=true` 时 → `SW_RESTORE`）→ Win32 `IsIconic` 兜底恢复（覆盖 tao 缓存与实际状态漂移的路径）→ `show()` → `set_focus()`；
   结束时用 Win32 读回（6 × 100 ms 有界复核；已恢复可见后再给前台约 2 × 100 ms 观察窗口）落
   `app_lifecycle event=show_main_window trigger=... terminal_result=... minimized_before/iconic_before/visible_before/iconic_after/visible_after/foreground_after`。
   托盘左键与"显示主界面"菜单项统一走它。
2. 新增 `crates/sayall-windows/src/instance_signal.rs`：会话内命名事件 `Local\SayAll-ShowMainWindow`（**自动重置**：一次置位对应一次显示请求；置位时若无等待者也不会丢）。第二个实例在单实例守卫命中时置位该事件再退出（日志追加 `show_request_result=passed|failed`）；主实例的 `spawn_second_instance_listener` 线程收到后调用 `show_main_window(trigger=second_instance)`。
3. 新增真机探针 `Testing/probe-main-window-restore.ps1`（五个用例：最小化×2 路径 / 关闭到托盘 / 二次启动 / 任务栏图标 UIA 激活；托盘用例走 shell 同款回调消息，判据全部为 Win32 读回；含收尾恢复，不会把应用留在不可用状态）。

## 验证

**修复版本机真机，2026-10-03**（源码分支 `fix/main-window-restore`，提交 `2e981cff8be16cbe153a2e50dc4a1cdaef1a6347`；安装版本机测试包内嵌同一修订号）：

| 用例 | 结果 | 读回 |
| --- | --- | --- |
| 用例1 最小化(SC_MINIMIZE) → 托盘左键 | **passed** | iconic=false visible=true foreground=true |
| 用例2 最小化(SW_MINIMIZE，覆盖缓存漂移兜底) → 托盘左键 | **passed** | iconic=false visible=true foreground=true |
| 用例3 关闭到托盘 → 托盘左键 | **passed** | iconic=false visible=true foreground=true |
| 用例4 最小化 → 二次启动 | **passed** | 第二实例退出；iconic=false visible=true foreground=true |
| 用例5 真实任务栏图标 UIA 激活（shell 官方路径） | **passed** | iconic=false visible=true foreground=true |

开发构建与**本地测试安装包安装后的构建**均 5/5 passed（安装包与安装校验见下）。

- **真实 shell 路径激活**（非消息模拟，安装版 5/5 之一）：任务栏通知区会**过滤注入的鼠标点击**（2026-10-03 实测：`SendInput` / `mouse_event` 点击托盘图标均无回调，右键连菜单都不弹——软件无法产生"硬件点击"）。因此"真实点击"改用 UI Automation 的 `InvokePattern` 激活任务栏上的真实图标按钮（`SystemTray.NormalButton`，name=`无线麦 SayAll`，与用户点击同一个系统入口）：最小化后 Invoke → 窗口恢复并置前，并产生完整日志 `show_main_window trigger=tray_click ... foreground_after=true`。该用例已纳入探针（用例5）。
- 早期一轮"经折叠弹窗点击图标"的测试曾观察到窗口恢复，但**当时没有 `show_main_window` 日志**——恢复来自 shell 自身对最小化窗口的激活（`SetForegroundWindow` 会恢复最小化窗口），不是托盘回调链路的证据。记录在此，避免后来者把它当作托盘路径通过记录。
- 日志链（一次日志拉取即可定位全链）：
  - `app_lifecycle event=second_instance_listener ... reason=listening`；
  - `... event=show_main_window trigger=tray_click ... iconic_before=true iconic_after=false foreground_after=true`；
  - `... event=second_instance_listener phase=observed reason=show_requested` → `event=show_main_window trigger=second_instance ... foreground_after=true`；
  - 第二实例侧：`app_lifecycle event=single_instance ... error_code=already_running ... show_request_result=passed show_request_error=none`。
- 自动化：`cargo fmt --all -- --check` passed；`cargo test -p sayall-windows` 253 passed（含 `instance_signal` 3 例：事件名契约、无等待端时请求报错、置位被消费且不重复唤醒）；`cargo check -p sayall-windows-app`、`cargo check -p sayall-windows-app --features runtime-simulation` passed。
- 未覆盖边界（`deferred`）：
  - 托盘菜单项"显示主界面"的**点击**未做自动化（与已测路径共用同一 `show_main_window`，差异仅在事件来源）——由用户安装后手动确认；
  - 托盘双击：tray-icon 把双击拆成多个 Click 事件，处理逻辑取其中一次 `Click/Left/Up`，与已测路径相同；未单独自动化；
  - 折叠区（overflow）图标：dev 构建图标在折叠区时以 shell 同款回调消息覆盖（用例1-4）；任务栏直显（promoted）图标以 UIA 激活覆盖（用例5）。两种位置的回调机制相同，均已有真机证据。

## 本地测试包（本机，2026-10-03）

- 安装包：`target\release\bundle\nsis\无线麦 SayAll_0.5.0_x64-setup.exe`（基于最新 `origin/main` + 本修复，提交 `2e981cff`），SHA-256 `c501725d26ef45dfad60f5b48a8072a2ec206492b6fe823c1b30d3767d2de155`；同目录有本机测试签名 `.sig`（与 CI pubkey 不匹配的告警为预期，**不可发布**）。
- 安装校验 `scripts/verify-local-test-install.ps1 -ExpectRevision -ExpectAppMarker Local\SayAll-ShowMainWindow,event=show_main_window`：helper/gadget 哈希对齐、app exe 3 字节（UNK→NSS）差异、内嵌修订号 `2e981cff`、两个行为标记均命中，窗口截图与启动日志正常——全部 **PASS**；安装后对安装版再跑探针 **5/5 passed**。
- 说明：本包随后安装在本机 `%LOCALAPPDATA%\无线麦 SayAll`，替换了用户先前从旧工作树构建的安装版；用户配置（`%APPDATA%\app.getsayall.remote-mic.windows`）随升级保留。

## 隐私检查

日志只含布尔状态与触发来源（`tray_click` / `tray_menu` / `second_instance`），不含路径、设备身份、语音内容或凭据；探针输出含 exe 路径与 pid，仅本机使用，不入库。
