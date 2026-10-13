# 返回/音量± 在「全按键支持」开启但没有配置动作时不产生任何事件（无法高亮）

- 发现日期：2026-10-13
- 状态：已修复（自动化 passed；真机按键复验 deferred，待安装本地测试包后由 Andy 按压确认）
- 影响范围：0.8.6（现场构建 `b2f0f5e`，onboarding 分支本地测试包），Windows 11 x64，RC003
  （RC001 走同一条捕获与目标集路径，预期同样受影响；未实测）
- 功能点：按键页「全按键支持」→ 报告层捕获目标集（`Rc003Bridge::set_capture_targets`）
- 现象：开着「全按键支持」、但没给 返回 / 音量+ / 音量− 配置任何动作时，按下这三个键
  **零事件**——按键页对应卡片不高亮（向导「按键检测」同样收不到）；而其他按键（电源 /
  主页 / 菜单 / TV / 确定 / 方向…）即使没有配置，按下也会亮。
- 复现条件：
  1. 开启「全按键支持」（助手已连接、agent 持有所有权）；
  2. 不给 返回 / 音量+ / 音量− 配置任何动作（或撤掉已配置的动作）；
  3. 按下这三个键 → 无高亮、无任何边沿；同时按下其他未配置的按键 → 正常高亮。
- 正常预期：与其余按键一致——按下即有高亮（配置动作之前的必要观察反馈）；配置动作后
  照常执行映射。
- 证据：诊断日志（RC003 助手侧）：三键的 `[EDGE]` 只出现在"这三个键被配置了动作"的时间段
  （如返回键 `0x00F1` 的边沿只在其映射存在时出现）；未配置期间按下三键无任何 `[EDGE]`、
  主程序侧无 `map_edges`。同期对照：未配置的左方向键（`0x0050`）按下仍有 `map_edges`
  （原生路径），即"其他按键能亮、三键不能亮"的可复现差异。相关文档
  [RC003 返回/音量± 三键在 Windows 上不可见](../docs/investigations/2026-09-05-rc003-back-volume-buttons-invisible.md)
  （2026-09-23 归因更正：设备确实上报，是 Windows 的 HID→VK 映射表缺这三个 usage）。
- 根因：捕获目标集按"已配置映射的按键"计算
  （`set_capture_targets(enabled, mapped_mask)`，来源 `ButtonMappings::mapped_mask()`）。
  三键在 Windows 输入栈里本来就不可见（`kbdhid` 映射阶段丢弃 `0x00F1`/`0x0080`/`0x0081`），
  只有报告层接管才有事件；零配置时目标集为空 → 主程序按目标集把三键边沿整批丢弃
  （`apply_usages` 的 `usages_dropped`），于是"没有任何配置"的按键连观察都观察不到。
  其余按键不受影响：它们走键盘 / Raw Input 原生路径，与目标集无关。
- 修复：`Rc003Bridge::set_capture_targets` 的目标集改为
  **`mapped_mask | ALWAYS_CAPTURED_BUTTONS`（开启时）**——三键恒被接管
  （`crates/sayall-windows/src/rc003_bridge.rs`）。代价为零：清空这三个 usage 不改变任何
  既有的系统可见行为（它们到不了操作系统与任何应用）；其余按键依旧"只在配置了映射时
  才接管"，最小接管语义不变。
- 验证：
  - 自动化（本机 Windows，`cargo test -p sayall-windows --lib`）：**332 passed / 0 failed**。
    新增/改写用例：
    `rc003_bridge::tests::three_keys_are_captured_without_any_mapping`（零配置时目标集恰为
    `0x0080,0x0081,0x00F1`）、`capture_targets_are_three_keys_plus_mapped_only`
    （已配置 Home + 三键；未配置的确定/方向不进入）、
    `three_key_edges_pass_the_minimal_target_set`（最小目标集下三键边沿放行、未配置 Home
    仍被丢弃）、`end_to_end_loopback_delivers_edges` 与
    `enhanced_capture_enable_pushes_targets_and_disable_clears`（协议行 / 开关语义断言
    同步为含三键）。
  - 阳性对照（修复前先复现）：上述用例在未改实现时分别红在"目标集为空（`[]` vs 三键）"、
    "只有 Home（`[74]` vs `[74,128,129,241]`）"、"OK 行为 `80,f1`（缺 `81`）"。
  - 真机：**deferred**——需安装本地测试包后在 RC003（与 RC001）上按压三键确认卡片高亮，
    并确认已配置动作的三键行为无回归。
- 隐私检查：本记录不含设备身份（无蓝牙地址 / 设备路径）、无个人路径、无语音内容与凭据；
  引用的日志字段均为脱敏后的语义标记。
