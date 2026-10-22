# 组合动作录入无反馈、Enter 显示不一致、Ctrl+标点无法录入

- 发现日期：2026-10-22
- 状态：已修复，等待安装包真机复核
- 影响范围：组合动作页快捷键录入；按键页自定义快捷键；Windows 10/11
- 功能点：`MacrosPage.vue` 快捷键录入、`ButtonsPage.vue` 自定义快捷键、`key_gate` 低级键盘钩子、组合动作 `MacroChord`
- 现象：
  1. 组合动作步骤里的「开始录入」点击后没有任何可见变化，看起来像按钮失效；
  2. 常用快捷键点「Enter」后，「已录入」显示 `Return`（实际渲染为大写 `RETURN`）；
  3. 按键页录入自定义快捷键时，Ctrl+标点（先报 Ctrl+[，随后 Ctrl+]）在普通与安全录入模式下都无法完成录入。
- 复现条件：
  1. 编辑组合动作 → 编排步骤 → 选择「单键或快捷键」→ 点击「开始录入」；
  2. 同一位置点击「常用快捷键 · Enter」；
  3. 按键页任意按键 → 录入自定义快捷键 → 按 Ctrl+[ 或 Ctrl+]。
- 正常预期：
  1. 点击后立即显示录入中状态，并可用再次点击 / Esc / 失焦取消；
  2. Enter 相关文案统一显示为 `Enter`；
  3. 主键盘所有 OEM 标点都能记录为 `Ctrl + 标点`，并能保存与执行。
- 证据：
  - `sayall-diagnostic.log` 2026-10-22T12:51:43Z 组合动作页 `shortcut_capture action=start ... passed`，12:51:48Z `stop ... passed`，期间 `keys_seen` 未增加；同一时段界面无任何录入态提示。说明点击已到达后端，用户看到的是“无反馈”。
  - 同日志 12:44–12:52Z 多次出现 `last_unmapped_vk=0xDB` / `0xDD`、`unmapped_seen` 持续增长，同时只收到 LeftControl 边沿。0xDB = `VK_OEM_4`（`[`）、0xDD = `VK_OEM_6`（`]`），钩子已看到按键但 `capture_key_code` 没有映射，因此被成对吞下却不投递。
  - `key_code_to_macro_key(Enter) = "return"`，前端 `keyLabel` 没有 `return` 条目，回退 `toUpperCase()` 得到 `RETURN`。
- 根因：
  1. 组合动作录入只在页面底部写一条状态文案，按钮本身没有录入态；没有超时/失焦/卸载清理，一旦用户没注意提示就会认为按钮失效，且离开页面会把后端留在全局吞键状态；
  2. `KeyCode` 原先只有 `Oem3`，没有完整的 `VK_OEM_*` 标点族；钩子映射、宏执行映射、前端语义映射三处同时缺失，所以每补一个键都会再暴露下一个；
  3. Enter 的稳定存储名是 `return`，但显示表只覆盖了 `enter`。
- 修复：
  - `src/pages/MacrosPage.vue`：按钮切换「录入中…（点击取消）」并显示内联提示；再次点击/Esc/窗口失焦/15 秒超时/页面卸载统一走 `stopCapture`；用 `captureGeneration` 防止迟到的启动回调重新武装。
  - `src/lib/bridge.ts`：`return → Enter`；补齐 OEM 标点的显示名。
  - `crates/sayall-windows/src/send_input.rs`：新增完整 OEM 标点族 `Oem1 / OemPlus / OemComma / OemMinus / OemPeriod / Oem2..Oem7 / Oem102`，含各自的 VK 与 Set-1 扫描码；同时新增小键盘 `Numpad0..9 / NumpadMultiply / NumpadAdd / NumpadSubtract / NumpadDecimal / NumpadDivide / NumLock`。
  - `crates/sayall-windows/src/key_gate.rs`：`0xBA/0xBB/0xBC/0xBD/0xBE/0xBF/0xC0/0xDB/0xDC/0xDD/0xDE/0xE2 → Oem*`；`0x60-0x6F / 0x90 → Numpad*/NumLock`。
  - `crates/sayall-windows/src/macro_runtime.rs`：`;` `=` `,` `-` `.` `/` 反引号 `[` `\` `]` `'` 与 `Oem*` 双向映射；ISO 额外键 `Oem102` 归一到 `\`。
  - `src/pages/ButtonsPage.vue` / `src/pages/MacrosPage.vue`：前端 `Semicolon/Equal/Comma/Minus/Period/Slash/Backquote/BracketLeft/Backslash/BracketRight/Quote/IntlBackslash` 与语义键名映射。
  - `Bugs/` 记录补充 Ctrl+] 回归，测试按“整族”覆盖，不再只测单个键。
- 验证：
  - 前端全量 `vitest`：28 个文件、464 passed / 10 skipped，`passed`；新增 Ctrl+]、Ctrl+小键盘 +、录入态、Enter 文案、仅修饰键提示用例后 127 passed（两个页面）。
  - `vue-tsc --noEmit`：`passed`。
  - Rust `cargo test -p sayall-windows --lib`：425 passed / 14 ignored，`passed`；OEM/小键盘虚拟键、扫描码、钩子映射、宏双向映射四张表均有用例。
  - 用户给出的常用快捷键清单新增矩阵用例 `common_shortcut_list_is_fully_representable`：Ctrl+K / Ctrl+, / Ctrl+F / Enter / Shift+Enter / Ctrl+N / Esc / Ctrl+[ / Ctrl+] / Ctrl+Shift+E / Ctrl+B / Ctrl+Shift+B / F11 / Shift+Alt+W / Ctrl+Shift+=（Ctrl++）/ Ctrl+小键盘 +（Ctrl++）/ Ctrl+- / Ctrl+0 / Shift+2（@）/ `/` / Ctrl+T / Ctrl+W / Ctrl+G / Ctrl+L / Ctrl+J / Ctrl+Shift+Enter 全部可回落到可注入的 `KeyChord`，`passed`。
  - 「长按 Alt」「按住 Ctrl 点击」是保持修饰键语义，不是组合键；当前模型按“按下-松开”注入，不能复现按住不放。录入时只按修饰键现在会明确提示「仅按住 Alt/Ctrl 等修饰键暂不支持」，`passed`。
  - `cargo check -p sayall-windows-app --features runtime-simulation`：`passed`。
  - 浏览器预览（前端真实渲染）：点「Enter」后显示 `Enter`；点「开始录入」后按钮变为「录入中…（点击取消）」并出现内联提示，再次点击显示「已取消录入」，`passed`。
  - 本地 NSIS 测试包已生成（`target/release/bundle/nsis/无线麦 SayAll_1.0.0_x64-setup.exe`），前端 15 个资源嵌入校验 `passed`；SHA-256 `a3a760b97d19642932bc595096ca6081b101c5934f4c520a2835409727b897a2`。
  - 真实键盘逐键按 Ctrl+标点、安装包按钮观感：`deferred`（需安装新本地包后人工复核）。
- 隐私检查：未包含个人路径、设备身份、语音内容或凭据。
