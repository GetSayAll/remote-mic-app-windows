# 快捷键录入期间暂停增强捕获清键（内置键盘录不到，外接键盘正常）

- 发现日期：2026-11-15
- 状态：已实现并完成代码复核，等待本地包真机复验
- 影响范围：1.9.3，Windows 笔记本内置键盘；按键页与组合动作页的快捷键录入；
  开启「全按键支持」时
- 功能点：`start_shortcut_capture` / `stop_shortcut_capture`；
  `WindowsPlatform::set_shortcut_capture_suspension`；RC003 报告层 agent

## 现象

用户升级 1.9.3 后，按键页「录入自定义快捷键」和组合动作页「开始录入」都停在
「录入中」，内置键盘按任何键都没有录入结果。现场规律：

1. 接上外接键盘后，用外接键盘录入正常；
2. 拔掉外接键盘后，内置键盘也能录入；
3. 电脑重启后，内置键盘再次无法录入，必须再插一次外接键盘才恢复。

用户无法提供日志，也拒绝做进一步实验，因此按已确认的代码风险直接修复。

## 根因（已确认的代码风险）

1.9.3 的 RC003 增强捕获 agent 在共享 `WUDFHost.exe` 中按报告来源隔离状态，
但**没有精确设备门禁**：遥控器空闲时，同一宿主里的另一台 HID 设备（笔记本
内置键盘）如果先按到目标 usage，就会成为 `activeSource`，其按键在报告层被清掉。

目标 usage 与笔记本键盘重叠：

| 目标 usage | 笔记本键盘上的对应键 |
| --- | --- |
| `0x0028` | Enter（遥控器 OK 的默认映射） |
| `0x004A` | Home |
| `0x004F`–`0x0052` | 方向键 |
| `0x007F`–`0x0081` | 音量键 |
| `0x003E` | F5 |

低级键盘钩子看不到被清掉的报告，因此录入零边沿。外接键盘走另一个 HID 宿主，
不受影响；拔插外接键盘可能让共享宿主释放来源，所以内置键盘恢复；重启后 agent
重新注入，若用户又先按了目标键，问题复发。

现有 agent 回归
（`node hardware/RC003/helper/agent/agent_logic_test.mjs`，95/95 passed）已复现该
机制：`动态全键：0x28 被清零`、`来源隔离：设备 A 按下 0x00f1 被清零`、
`来源隔离：设备 A 成为活动来源`；而目标集合为空时 `默认：确定键不被改写`。

## 修复

快捷键录入是前台短会话，遥控器此时不参与。录入开始时让报告层**临时停止清键**，
录入结束/失败/取消时按当前「全按键支持」开关与映射恢复：

- `crates/sayall-windows/src/lib.rs`
  - 新增 `SHORTCUT_CAPTURE_SUSPENDED` 与纯函数 `effective_enhanced_capture`；
  - `apply_enhanced_capture_state` 在暂停期间下发空目标集，避免用户在录入中
    切换开关时把清键重新打开；
  - 新增 `WindowsPlatform::set_shortcut_capture_suspension(bool)`：暂停时
    `set_capture_targets(false, 0)` 并 `set_voice_synth(None)`，恢复时按当前
    `mapped_mask()` 与「按住说话快捷键」重新下发；
  - 语音键 F5 的合成也必须暂停：否则录入 F5 时 agent 会把它替换成右 Alt，
    前端只看到修饰键、永远等不到主键；`set_voice_hold_hotkey` 在暂停期间不重配
    合成，录入结束后由 `set_shortcut_capture_suspension(false)` 用当时的快捷键恢复。
- `src-tauri/src/platform.rs`：`PlatformRuntime` 新增同名方法（默认 no-op），
  `WindowsPlatform` 纯委托。
- `src-tauri/src/lib.rs`
  - `start_shortcut_capture` 在武装钩子前调用暂停；门控不可用时回滚；
  - `stop_shortcut_capture` 走唯一收尾函数 `finish_shortcut_capture`；
  - 新增 `SHORTCUT_CAPTURE_GENERATION` 与 30 秒 watchdog（线程
    `sayall-shortcut-capture-watchdog`）：前端超时/WebView 崩溃/`stop` IPC 丢失
    时自动恢复增强捕获，日志 `reason=watchdog`；代次通过 `compare_exchange`
    认领，旧计时器不会停掉新会话；
  - 暂停日志写 `phase=requested / terminal_result=pending`，不把「本地目标已
    提交」冒充「agent 已生效」；真实结果以桥接 `targets_changed` / ownership
    日志为准。
- `src/pages/ButtonsPage.vue`、`src/pages/MacrosPage.vue`：录入/启动期间不显示
  「全按键支持开着但没生效」提示；读取 `startShortcutCapture()` 返回的
  `preheld`，非空时显示「检测到仍有按住的按键，请先松开所有按键；松开后即可按
  新组合，录入将自动开始」，第一条边沿到达后收回。

没有改 helper、agent、安装器或用户设置；暂停只存在于当前进程内，应用退出或
停止录入后自动恢复。

## 验证

- `cargo fmt --all -- --check`：passed。
- `cargo test -p sayall-windows --lib`：560 passed / 0 failed / 17 ignored；
  新增 `shortcut_capture_suspension_only_changes_effective_targets`、
  `shortcut_capture_suspension_also_mutes_voice_synth` passed。
- `cargo test -p sayall-windows-app --lib`：223 passed / 0 failed / 2 ignored；
  新增 `shortcut_capture_suspends_and_restores_enhanced_capture`、
  `shortcut_capture_finish_claim_rejects_stale_generation` passed。
- `cargo check -p sayall-windows-app --features runtime-simulation`：passed。
- 前端 `vitest run src/pages/ButtonsPage.test.ts src/pages/MacrosPage.test.ts`：
  153 passed（含「检测到仍有按住的按键」用例）；`vue-tsc --noEmit`：passed。
- `cargo test --workspace`：passed（`sayall-windows` 560 + app 223 + 其余
  crate/集成测试，共 918 passed / 0 failed / 19 ignored）；
  `pnpm test`：696 passed / 10 skipped；`verify-frontend-embedded.ps1`：18/18 passed。
- 本地测试包 v3：`artifacts/local-test/2026-11-15-shortcut-capture-suspension-v3/`，
  源码提交 `4a245a6`（二进制内嵌同一修订），
  SHA-256 `92b407edc032c41b3545b293ac350f9971ce785d380ddbd2846e99fbeebcedc4`；
  v1（`90750eee…`）与 v2（`dc2a8687…`）已被取代。
- 真机：**deferred**。需要用户安装本地包后在重启后的内置键盘上复测
  Enter / L / 方向键 / F5，并确认外接键盘与遥控器原有功能不受影响。

## 边界

- 本修复覆盖“报告层把内置键盘按键清掉”这一分支；若现场实际是低级键盘钩子
  未收到内置键盘，本修复不会消除，但也不会引入新问题。
- 录入期间遥控器的额外按键（返回/音量±等）会暂时回到原生路径，最长一次录入
  会话（15 秒超时或用户取消）；不影响语音键与基础映射。
- 没有真实日志，不能把“代码路径成立”写成“用户现场已通过”。

## 隐私检查

本文只包含版本、按键语义、代码路径和测试计数；未包含设备地址、HID 路径、
语音内容、用户名或个人目录。
