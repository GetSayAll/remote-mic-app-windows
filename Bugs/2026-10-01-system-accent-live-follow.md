# 换系统主题色后应用内部不跟随（强调色监听窗口收不到广播）

- 发现日期：2026-10-01
- 状态：已修复（自动化 passed；Windows 真机复验中）
- 影响范围：Windows 版含该功能的全部构建（2026-09-27 `08e94ab` 起）；
  启动时首次读取正常，只有"运行中改系统主题色"受影响；不涉及 RC001/RC003、
  语音链路和第三方工具
- 功能点：界面强调色跟随 Windows 系统主题色（设置 > 个性化 > 颜色），
  `src-tauri/src/accent.rs` + `src/lib/accent.ts`
- 现象：应用运行中在 Windows 设置里换主题色，应用内选中态/主按钮/开关颜色不变；
  重启应用才显示新颜色
- 复现条件：应用运行中改系统主题色（设置 > 个性化 > 颜色 > 选择你的主题色）
- 正常预期：`--accent*` 变量在数秒内按新强调色重算重注入，无需重启

## 证据

现场日志（用户本机 `%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`）：

- `accent_color action=watcher_register ... terminal_result=passed reason=watcher_started`
  共 52 次——监听线程每次都起来、注册成功；
- `accent_color action=watcher_message`（广播到达）**0 次**——监听窗口从未收到广播。
  说明故障在"窗口能不能收到广播"，不在读色、去抖或前端注入。

最小实验（同进程内同时创建 message-only 窗口与顶层窗口，再
`SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, ..., "SayAllAccentProbe")`）：

- 顶层窗口收到 1 次；message-only 窗口收到 **0 次**。
- `HWND_BROADCAST` 只送达顶层窗口，message-only 窗口不在广播名单里。

回归测试在修复前失败（`before=0 after=0`），修复后通过，见下方验证一节。

## 根因

`accent.rs` 的监听窗口以 `parent = Some(HWND_MESSAGE)` 创建，即 **message-only 窗口**。
系统换强调色时用 `SendMessageTimeout(HWND_BROADCAST, WM_SETTINGCHANGE, 0,
"ImmersiveColorSet")` 广播，**广播只送达顶层窗口**，message-only 窗口永远收不到：

广播未到达 → 去抖回调永不触发 → 前端收不到 `system-accent-changed` → 界面不跟随。

窗口注册、WinRT 读色、去抖、前端派生和注入逻辑本身都正确（启动时首读生效即为证）。

## 修复（最小改动）

- `src-tauri/src/accent.rs`：监听窗口改为**隐藏顶层窗口**——`parent = None`、
  `WS_POPUP` + `WS_EX_TOOLWINDOW`，**从不 `ShowWindow`**（不出现、不进任务栏、
  不进 Alt+Tab），线程退出前 `DestroyWindow`，避免句柄泄漏；
- 同文件补结构化日志与可观测判据：每次收到 `ImmersiveColorSet` 广播落
  `accent_color action=watcher_message ... reason=accent_changed|accent_debounced`
  （含 RGB），并新增接收计数 `immersive_color_message_count()` 供回归测试断言
  "窗口确实在广播名单里"；
- `src/lib/accent.ts`：强调色首读结果与变化事件补上报诊断日志
  （`system_accent` / `system_accent_change`）。此前前端只有 `console.info`，
  用户机器上不落盘，导致本次必须额外做探针实验才能定位——现在
  Rust 侧"广播到达"与前端"重注入成功"两条日志合起来覆盖完整链路。
- `src/lib/accent.test.ts`：补首读成功/失败与变化事件的三条上报断言。

## 验证

- 回归测试（修复前 failed / 修复后 passed）：
  `cargo test -p sayall-windows-app --lib watcher_window_receives_immersive_color_broadcast`
  ——修复前 `before=0 after=0`，修复后 `1 passed`；
- `cargo fmt --all -- --check` passed、`cargo check --workspace` passed、
  `cargo test --workspace` passed（core 26 + replay 4 + windows 177 + app 41 等）；
- 前端 `npm test`（vitest）131 passed | 10 skipped，含新增 3 条强调色上报断言；
- Windows 真机复验（运行中改系统主题色，界面是否跟随 + 日志是否落
  `accent_color action=watcher_message ... reason=accent_changed` 与
  `frontend event=system_accent_change ... reason=accent_applied`）：**进行中**。

## 隐私检查

- 只涉及一个系统强调色 RGB 与窗口消息计数，无设备身份、蓝牙地址、HID 路径、
  语音内容、个人路径或凭据；日志沿用既有 `gatt_note` 脱敏约定。
