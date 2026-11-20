# 代码评审：快捷键录入期间暂停增强捕获

- 评审日期：2026-11-15
- 评审对象：`fix/shortcut-capture-device-20261115`（基于 `origin/main` `25bd512`）
- 变更范围：4 个源文件 + 2 个单元/契约测试 + 1 个 Bug 记录
- 结论：**复核后无阻塞性缺陷；P1/P2 已处理，逐项状态见文末「复核后状态」**

## 变更摘要

| 文件 | 变更 | 评价 |
| --- | --- | --- |
| `crates/sayall-windows/src/lib.rs` | 新增 `SHORTCUT_CAPTURE_SUSPENDED`、`effective_enhanced_capture`、`set_shortcut_capture_suspension`；暂停时清空报告层目标并关闭语音键合成 | 方向正确；见 P1-1、P1-2 |
| `src-tauri/src/platform.rs` | trait 新增暂停方法，Windows 纯委托 | 低风险 |
| `src-tauri/src/lib.rs` | 录入开始暂停、门控失败回滚、录入结束恢复；契约测试 | 主路径成对；见 P1-1 |
| `src/pages/ButtonsPage.vue` | 录入/启动期间隐藏「开着但没生效」提示 | 避免暂停造成误报 |
| 测试 | `shortcut_capture_suspension_only_changes_effective_targets`、`..._also_mutes_voice_synth`、`shortcut_capture_suspends_and_restores_enhanced_capture` | 覆盖纯函数/源码契约；见 P2-2 |

## P1 残留

### P1-1 暂停没有后端 watchdog，前端失联时会一直保持

- 位置：`src-tauri/src/lib.rs:3777`（暂停）、`src-tauri/src/lib.rs:3843`（恢复）
- 事实：暂停/恢复完全依赖 `start_shortcut_capture` 与 `stop_shortcut_capture`
  成对调用。正常取消、15 秒超时、页面卸载都会走 `stop`；但如果 WebView 崩溃、
  `stop` 的 IPC 丢失，或页面在 `start` 期间被强制销毁，后端没有计时器把
  `SHORTCUT_CAPTURE_SUSPENDED` 复位。RC003 的额外按键会一直走原生路径，
  直到应用退出/重启。
- 同源风险：此时 `key_gate` 的全局录入吞键也会保持，属于既有问题；暂停不是
  新引入的唯一失效点，但它扩大了失效面。
- 建议：增加后端 generation + 30 秒 watchdog，超时走与 `stop` 同一收尾函数
  （清 `capture_active`、恢复目标、恢复语音合成、恢复 IME），日志
  `reason=watchdog`。若暂不做，至少把「WebView 崩溃需重启应用」写入已知边界。

### P1-2 暂停成功的日志早于 helper/agent 的 `targets_ack`

- 位置：`crates/sayall-windows/src/lib.rs:1286`、`src-tauri/src/lib.rs:3777`
- 事实：`set_capture_targets(false, 0)` 只更新主程序侧目标并把 `T` 行交给
  连接线程；agent 实际应用空目标后才会回 `targets_ack`/`O` 行。当前日志写
  `phase=applied / terminal_result=passed`，但这是**本地状态已提交**，不是
  “报告层已经停止清键”。用户点击后立刻按键的极小窗口里，第一个键仍可能被
  agent 清掉。
- 影响：不是本次“完全无反应”的主因（agent 的 `activeSource` 持续占用），但
  与仓库的「外部调用成功判据」冲突：不能用本地布尔值宣布外部副作用完成。
- 建议：二选一：
  1. 增加带代次的 `wait_for_targets_ack(generation, 100ms)`，失败只降级为
     `requested` 并继续录入（fail-open）；
  2. 至少把日志改为 `phase=requested`，在 `capture_diagnostics_summary` 或
     桥接快照里补 `target_generation`，避免把“本地已提交”写成“已生效”。

## P2 建议

### P2-1 `preheld` 仍在按键页/组合动作页被丢弃

- 位置：`src/pages/ButtonsPage.vue:1395`、`src/pages/MacrosPage.vue` 的
  `beginCapture`（`startShortcutCapture()` 返回值未使用）
- 事实：连接页会读取 `preheld` 并提示“请先松开”；这两个页面直接丢弃。若
  笔记本键盘有键被 `GetAsyncKeyState` 判为常按，后端会一直不投递边沿，界面
  只显示「录入中」，与本次现场症状无法区分。
- 建议：复用连接页的提示：`const preheld = await startShortcutCapture();`，
  非空时显示「检测到仍有按住的按键，请先松开所有按键」，并在 `stop` 时清理。
  这是诊断性改动，不改变录入协议。

### P2-2 缺少真实目标切换的集成测试

- 事实：新增测试一是纯函数，二是源码契约；`set_capture_targets(false,0)` /
  恢复、`set_voice_synth(None)` / 恢复三条链路只靠既有桥接测试间接覆盖。
  测试没有直接证明 `WindowsPlatform::set_shortcut_capture_suspension(true/false)`
  会让 `BridgeSnapshot.target_usages` 与 `owned_usages` 按预期变化。
- 建议：在既有 `Rc003Bridge::start_in` 测试夹具上加一条串行用例：
  1. 开启增强捕获并等目标非空；
  2. 调暂停 → 断言 `target_usages` 为空、语音合成配置为 `None`；
  3. 调恢复 → 断言目标回到 `mapped_mask()`、语音合成回到当前热键。
  若暂时不做，代码评审记录里明确“以纯函数 + 契约覆盖，真机验收兜底”。

### P2-3 `SHORTCUT_CAPTURE_SUSPENDED` 是进程级全局，测试隔离未定义

- 位置：`crates/sayall-windows/src/lib.rs:820`
- 事实：当前测试只读纯函数，没有测试写这个原子量；未来若加 P2-2 的集成测试，
  并行测试可能把它置 true 并污染其他用例（与 `GATE_ACTIVE` 的 2026-09-28
  flake 同型）。
- 建议：加测试专用 reset 或串行锁，并在文档注释里写明“测试必须先复位”。

## P3 / 已接受

- **P3-1（v1 已修，v2 生效）**：语音键 F5 的合成没有暂停时，录入 F5 会看到
  右 Alt 而不是 F5。v2 已在暂停时 `set_voice_synth(None)`，恢复时按当前热键
  重配，并加 `shortcut_capture_suspension_also_mutes_voice_synth` 契约测试。
- **P3-2**：`captureInactiveNote` 只在按键页隐藏；连接页/组合动作页在极端的
  跨页并发录入下可能闪一句“没生效”。连接页自定义录入当前隐藏，风险低。
- **P3-3**：暂停期间遥控器返回/音量±会暂时走原生路径（最长一次录入会话），
  属有意取舍；建议在用户可见说明或发布说明里写一句。
- **P3-4**：`shortcut_capture_suspend` 平台日志与命令日志各一条，重复但便于
  关联；保留。
- **P3-5**：暂停是布尔量，不是引用计数；两个页面同时录入时先结束的一方会
  提前恢复。当前单窗口单页面录入，接受；若未来支持多窗口录入，改为代次/计数。

## 正向确认

- `start` 失败（gate 不可用）会回滚暂停；`stop` 无条件恢复，重复调用幂等。
- 用户在录入期间切换「全按键支持」或保存映射：`apply_enhanced_capture_state`
  读 `SHORTCUT_CAPTURE_SUSPENDED`，不会把清键重新打开；`stop` 用当时的开关
  与 `mapped_mask()` 恢复。
- 用户在录入期间改「按住说话快捷键」：`set_voice_hold_hotkey` 在暂停期间不
  重配合成，`stop` 用当时的热键恢复，不会覆盖新设置。
- helper 重连：`set_capture_targets` 是绝对状态，连接线程会在鉴权后补发当前
  目标；不会停留在旧目标。
- 不改用户设置、不改 helper/agent、不改安装器；应用退出后进程状态消失。
- 日志只含布尔值、代次/掩码是否存在与固定 token，不含设备身份、HID 路径或
  语音内容。

## 验证证据

- `cargo test --workspace`：第一次 **passed**；加入语音键合成暂停后第二次
  只有 `button_mapping::tests::profile_overlay_alt_tab_shortcut_opens_the_app_switcher_session`
  失败（`app_switcher_mask` 期望 0、实际 247）。该用例单独重跑 **2/2 passed**，
  且 `button_mapping.rs` 本次未改动；形状与既有
  `Bugs/2026-09-28-rust-gate-test-parallel-flake.md` 的全局门控时序 flake 一致，
  不归因于本 diff。其余 workspace 用例全绿。
- `cargo fmt --all -- --check`、`cargo check -p sayall-windows-app --features
  runtime-simulation`：passed
- `pnpm test`：695 passed / 10 skipped；`vue-tsc --noEmit`：passed
- 本地包 v2：`artifacts/local-test/2026-11-15-shortcut-capture-suspension-v2/`，
  SHA-256 `dc2a8687411c6490e2a0130179c35484324392dee99ab41f81dbc827d6a47c62`，
  前端嵌入 18/18 passed
- 真机：**deferred**（RC001/RC003、重启后内置键盘、外接键盘、语音键）

## 评审建议

- **可以出本地测试包并让用户复测**：核心路径正确，P1-1/P1-2 是失败场景的
  边界，不阻塞“先验证是否解决现场”。
- **合入 main 前**：建议至少处理 P1-1 的 watchdog 或把边界写清楚；P1-2 至少
  把日志口径从 `passed` 改为 `requested`，避免外部调用成功判据失真。
- **后续**：P2-1 的 preheld 提示是低成本、高诊断价值的改动，建议下一轮补。

## 复核后状态（2026-11-19）

| 项 | 状态 | 落点 / 证据 |
| --- | --- | --- |
| P1-1 无 watchdog | **已修** | `SHORTCUT_CAPTURE_GENERATION` + 30 秒 watchdog；`finish_shortcut_capture` 为唯一收尾，`compare_exchange` 认领代次；测试 `shortcut_capture_finish_claim_rejects_stale_generation` |
| P1-2 日志冒充已生效 | **已修（日志口径）** | 改为 `phase=requested / terminal_result=pending`；未加 ack 等待，真实结果以桥接 `targets_changed` / ownership 为准 |
| P2-1 preheld 被丢弃 | **已修** | 按键页/组合动作页读取返回值并显示先松键提示；ButtonsPage 新增用例 |
| P2-2 缺真实桥接集成测试 | **已修** | 新增 `shortcut_capture_suspension_drives_real_bridge_targets_and_synth`：暂停后 `target_usages` 为空、合成目标 `None`，恢复后回到 `mapped_mask()` 与 `0x00E6`；暂停逻辑抽成 `apply_shortcut_capture_suspension` 供真实桥接夹具驱动 |
| P2-3 全局态测试隔离 | **已收口** | 集成测试与 key_gate 测试共用串行锁，并用 drop guard 复位 `SHORTCUT_CAPTURE_SUSPENDED`；断言失败也不会污染其他用例 |
| P3-1 F5 合成 | **已修（v2）** | `set_voice_synth(None)` + 恢复时按当前热键重配 |
| P3-2 跨页并发提示 | 接受 | 当前单页面录入 |
| P3-3 暂停期间遥控器原生命中 | 接受 | 最长一次录入会话 |
| P3-4 日志重复 | 接受 | 便于关联命令与平台侧 |
| P3-5 布尔而非引用计数 | 接受 | 当前单窗口单页面录入 |

验证：`cargo test -p sayall-windows --lib` 561 passed / 17 ignored；
`cargo test --workspace` 918 passed / 0 failed / 19 ignored（新增用例前）；
`pnpm test` 696 passed / 10 skipped；`vue-tsc --noEmit`、`verify-frontend-embedded.ps1`
passed。本地测试包 v3 源码提交 `4a245a6`，SHA-256
`92b407edc032c41b3545b293ac350f9971ce785d380ddbd2846e99fbeebcedc4`。
真机复验仍为 **deferred**。
