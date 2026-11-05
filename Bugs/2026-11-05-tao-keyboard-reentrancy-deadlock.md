# 按键映射注入 Alt+Tab / Win+D 后主窗口无响应（tao 键盘事件重入自锁）

- 发现日期：2026-11-05
- 状态：已修复（升级 Tauri 2.12.5 / tao 0.37.2），等待本地包与 RC001/RC003 真机验收
- 影响范围：Windows 10/11；1.8.3 预览包及同一依赖树（Tauri 2.11.5 / tao 0.35.3）
  的构建；按键映射把 Home 双击配成 Win+D 或长按配成 Alt+Tab 时；连续按 Home 长按
  （Alt+Tab）约 2–3 次可稳定复现。RC001/RC003 均可能触发（与机型无关，取决于当前
  前台窗口与焦点消息时序）。
- 功能点：按键映射 → 快捷键动作注入（`ButtonAction::Shortcut` / `SendInputRuntime::tap`）。
- 现象：遥控器手势本身已触发、注入也已提交，但无线麦主窗口随后显示“未响应”，
  只能结束进程；期间系统窗口切换/显示桌面是否完成取决于注入时前台窗口。
- 复现条件：
  1. Home 长按配置为 Alt+Tab（或双击配置为 Win+D）；
  2. 无线麦主窗口在前台，连续触发长按 2–3 次（真机 Home 键或等价注入）；
  3. 主窗口停止响应，Windows 记录 Application Hang（WER `AppHangB1`）。
- 正常预期：每次手势都应完成一次窗口切换/显示桌面，无线麦保持可响应；即使注入失败
  也不得让 UI 线程自锁。

- 证据：
  - 结构化日志（`%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`，本地时间
    2026-11-05 21:12:53–21:14:16）：
    - `map_fire button=Home trigger=Double action=shortcut chord=LeftWindows+D`
      后 `map_inject result=queued echo_delta=4`；随后应用自身窗口
      `app_launcher ... minimized=true`，说明 Win+D 注入实际生效。
    - 21:13:09/11/13 连续三次 `map_fire button=Home trigger=Long action=shortcut
      chord=Alt+Tab` + `map_inject result=queued echo_delta=4`；21:13:14 后主进程
      日志停止，窗口 `Responding=False`。
    - 事件日志：`Application Hang` 1002 + WER 1001 `AppHangB1`（EXE 1.8.3.0），
      与上述时间对齐。
  - 挂起现场 cdb 全线程栈（本地 release 构建 + 匹配 PDB）：
    - 主线程最内层为 `ntdll!NtWaitForAlertByThreadId` ← `KERNELBASE!WaitOnAddress`
      ← `parking_lot::raw_mutex::RawMutex::lock_slow`，调用方是窗口子类化链
      （`comctl32!MasterSubclassProc` → `DefSubclassProc` → 原窗口过程）。
    - 外层是 `tao::...keyboard::KeyEventBuilder::process_message` →
      `user32!PeekMessageW`；`PeekMessageW` 分派了另一个线程发来的 sent message
      （`user32!DispatchClientMessage`），重入同一窗口过程后再次申请同一把
      `parking_lot` 锁。
  - 上游同源问题：`tauri-apps/tao#1379`（Windows 事件循环在
    `KeyEventBuilder::process_message` 的 `PeekMessageW` 期间被 sent
    `WM_SETFOCUS`/`WM_KILLFOCUS` 重入，重复申请 `KEY_EVENT_BUILDERS` 自锁）。
    修复 PR：`tauri-apps/tao#1215`（2026-06-10 合并，随 tao 0.36.0 发布）。

- 根因：
  - tao 0.35.3 在 `event_loop.rs` 中先锁住 `KEY_EVENT_BUILDERS`（`parking_lot::Mutex`），
    再把 `KeyEventBuilder::process_message` 的 `PeekMessageW(PM_NOREMOVE)` 放在锁区内
    （`keyboard.rs`）。
  - Alt+Tab / Win+D 会引发窗口焦点变化；另一个线程发来的 `WM_SETFOCUS` /
    `WM_KILLFOCUS` 是 **sent message**，会在 `PeekMessageW` 内被同步分派，重入同一
    窗口过程。重入路径的 `keyboard_callback` 再次请求 `KEY_EVENT_BUILDERS`，而
    `parking_lot` 互斥锁不可重入，于是主线程等待自己，UI 永久卡死。
  - 1.8.3 使用 `tauri 2.11.5` / `tauri-runtime-wry 2.11.4` / `tao 0.35.3`，正好落在
    修复前的版本；不是本仓库按键映射状态机或 `SendInput` 的缺陷（日志已证明手势与
    注入提交均正常）。

- 修复（直接升级依赖，不内置第三方补丁）：
  - `tauri 2.11.5 → 2.12.5`，`tauri-runtime-wry 2.11.4 → 2.12.3`，
    `tao 0.35.3 → 0.37.2`；tao 0.37.2 已含上游 PR #1215 的修复。
  - 升级同时把 `wry 0.55.1 → 0.57.1`、`webview2-com 0.38.2 → 0.39.1`、
    `windows-core 0.61 → 0.62`（与主工程已有的 `windows 0.62` 对齐）。因此需要两处
    最小 API 适配（不改变业务行为）：
    - `src-tauri/Cargo.toml`：`webview2-com = "0.39"`；`windows-core = "0.62"`。
    - `src-tauri/src/recommendations.rs`：`windows_core_061` 导入改为 `windows_core`；
      `CanGoBack` 的参数改为 `*mut BOOL`（`&mut can_go_back as *mut _`）。
  - 未改动按键映射、注入、门控或 UI 行为。

- 验证：
  - `cargo check --workspace`：`passed`（Tauri 2.12.5 / tao 0.37.2）。
  - `cargo test --workspace`：`passed`（见提交说明）。
  - 本地 release 构建 + NSIS 安装包 + 已安装包的注入复现：
    - 旧包：第 3 次 Alt+Tab 注入后 `test_button_mapping` 超时、窗口
      `Responding=False`，WER `AppHangB1`。
    - 新包：连续 6 次 Alt+Tab + 6 次 Win+D 注入全部返回，窗口保持
      `Responding=True`。
  - RC001/RC003 真机手势链路：`deferred`（需要安装本地包后用实体遥控器重复长按验证）。
- 已知边界：
  - 本 bug 只处理 UI 自锁。若主窗口仍带 `WS_EX_TOPMOST`，Alt+Tab / Win+D 还可能
    出现“焦点已切换、但目标窗口在视觉上被自身压住”的**另一类**现象（见
    `Bugs/2026-11-05-own-window-topmost-occludes-target.md`）；两者独立，验收时需
    分别确认。本次升级不会主动清除置顶位。
  - 若其他按键也制造“键盘消息处理期间有 sent 焦点消息”的窗口，理论上仍可走到
    tao 的同一重入点；tao 0.37.2 已按上游修复覆盖该路径，但真实硬件时序仍以真机为准。
- 隐私检查：本文档与日志只含时间、布尔值、窗口类名、进程/线程标识和上游公开链接，
  不含蓝牙地址、HID 路径、语音内容、个人目录或第三方私有数据。
