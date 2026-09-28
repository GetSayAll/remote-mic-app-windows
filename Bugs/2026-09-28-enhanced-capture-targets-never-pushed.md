# 全按键支持开关显示已开启但所有按键边沿被主程序丢弃（动态目标集从未下发）

- 发现日期：2026-09-28
- 状态：第一轮已修复（0ff8561）；第二轮回归已修复（3afdd01），等待真机验证
- 影响范围：SayAll 0.3.0（source_revision=d5bd0ca，2026-09-28 安装版）；RC003 全按键支持（enhanced capture）整条桥接链路；0.2.6 及更早版本不受影响
- 功能点：RC003 全按键支持（`rc003_bridge` 动态捕获目标下发）
- 现象：安装 0.3.0 后，按键页「全按键支持」开关显示已开启，助手（helper）进程正常拉起并完成桥接鉴权，但遥控器按键在应用内完全无响应。
- 复现条件：安装 0.3.0 安装版 → 开启全按键支持 → 按遥控器任意被映射按键。
- 正常预期：按键边沿经助手桥接进入映射引擎，映射动作执行。

## 证据（`sayall-diagnostic.log`，2026-09-28 00:57–01:22，pid 39800/36680/21288）

- `rc003 feature=enhanced-capture action=enable phase=completed terminal_result=passed` ×4（enable 命令全部「成功」）。
- `rc003_bridge event=helper_authenticated helper_pid=… version=2` ×4（助手连接与鉴权正常）。
- **`enhanced_capture event=targets_changed` 零条**（0.2.6 时代每次 enable 都有，如 2026-09-27T11:26 `generation=2 enabled=true usages=35,4a,65,80,81,f1`；09-28 一次都没有）。
- `rc003_bridge event=closed reason=read_error edges=0 released=0 dropped=10/35/40`——`edges=0`：没有任何边沿进入映射引擎；`dropped` 持续增长：助手确实上报了边沿（tap 在拦、helper 在转），但每一条都因「不在当前动态目标集内」被 `apply_usages` 丢弃。
- `event=first_edge` 最后一次出现在 2026-09-27T12:40（0.2.6），09-28 零条。
- 链路各段单独看全部 passed，合起来功能死亡——「每段证据都诚实，拼起来在说谎」。

## 根因（已确认）

提交 50c74d8（2026-09-28 01:12，「platform.rs 去重 set_enhanced_capture_enabled」）把 trait 实现从
6a4aaf8 的**委托**形态：

```rust
fn set_enhanced_capture_enabled(&self, enabled: bool) {
    WindowsPlatform::set_enhanced_capture_enabled(self, enabled)   // 会 store 原子量 + set_capture_targets
}
```

改成了**内联翻转**形态（src-tauri/src/platform.rs:203）：

```rust
fn set_enhanced_capture_enabled(&self, enabled: bool) {
    let mut mappings = WindowsPlatform::button_mappings(self);
    mappings.enabled = enabled;                                    // 只改 mappings.enabled
    WindowsPlatform::set_button_mappings(self, mappings);
}
```

而 `WindowsPlatform::set_button_mappings`（crates/sayall-windows/src/lib.rs:568）下发目标集时读取的是
`self.enhanced_capture_enabled.load()` 这个**原子量**——该原子量只在 inherent 方法
`WindowsPlatform::set_enhanced_capture_enabled`（lib.rs:579）里 store，而上述 trait 路径**从不经过它**。
于是原子量永远是初始值 `false`：

1. `set_capture_targets(false, mask)` → 动态目标集恒为空（且与初值相同 → 提前 return，连 `targets_changed` 日志都不产生）；
2. 助手鉴权后收到 `OK <ver> 1 -`（空目标）；
3. agent 拦到的边沿照常上报，`apply_usages` 因 `allowed.contains(usage)` 全部为假而逐条丢弃（`usages_dropped` 增长）；
4. 映射引擎收不到任何 `GateEdge` → 全按键支持「打开」但完全不可用。

启动路径（src-tauri/lib.rs:1712 `set_enhanced_capture_enabled(设置值)`）走同一 trait 实现，同样失效；
enable/disable 命令（src-tauri/lib.rs:387/404）同理。50c74d8 属于灾后恢复重建的「去重」提交：
**逐行去重时丢掉了一个语义（原子量 store），两份实现各写了意图的一半，合并后恰好都不完整。**

## 附带发现（独立问题，同批安装包）

安装包 0.3.0 的前端持续上报 `frontend event=rc003_capture_toggle … reason=invalid`，
该字符串**不存在于任何已提交源码**（当前 main 与全部历史 pickaxe 均无）。推断安装包内的前端产物
来自恢复期中间构建或过期 dist，而非 d5bd0ca 源码。重出安装包时必须用干净 main 全量构建
（`pnpm.cmd tauri build`，让 beforeBuildCommand 重新生成 dist），并按惯例核对产物行为
（不应再出现 `reason=invalid`）。

## 修复（建议，未实施）

最小改动一处（src-tauri/src/platform.rs trait impl），同时保住两个语义：

```rust
fn set_enhanced_capture_enabled(&self, enabled: bool) {
    // ① key_gate 的 enabled 位仍走映射同步路径（原内联形态的意图）
    let mut mappings = WindowsPlatform::button_mappings(self);
    mappings.enabled = enabled;
    WindowsPlatform::set_button_mappings(self, mappings);
    // ② 恢复委托：store enhanced_capture_enabled 原子量 + set_capture_targets（6a4aaf8 的意图）
    WindowsPlatform::set_enhanced_capture_enabled(self, enabled);
}
```

（②中的 `set_capture_targets` 与①重复调用无害：目标集相同则提前 return。）

回归测试（可离线）：通过 **trait 对象**调用 `set_enhanced_capture_enabled(true)` 后，
断言 `rc003_bridge.snapshot().target_usages()` 非空（修复前该断言失败，即为阳性对照）。

## 验证

- 单元：`enhanced_capture_enable_pushes_targets_and_disable_clears` passed（0ff8561，
  离线驱动完整链路；enable 后目标集含 0x00F1、disable 清空、原子量与 enabled 位同步）。
  `cargo test -p sayall-windows --lib` 与 `cargo test -p sayall-windows-app --lib`（38 项）passed；
  `cargo fmt --check`、`cargo check -p sayall-windows-app --features runtime-simulation` passed。
  （`silence_watchdog` / `leak_suppression_suite` 偶发计时 flake 在未改动的 main 基线同样复现，
  与本修复无关。）
- 真机：`deferred`——需 RC003 实机验收：开启后日志出现 `targets_changed enabled=true usages=…`
  与 `first_edge`，三键与已映射按键动作生效；关闭后 `targets_changed enabled=false` 且旧路径恢复。
- 安装包：`deferred`——从干净分支重出后安装，核对 `reason=invalid` 消失、关键二进制哈希
  （NSIS 静默安装遇占用文件会静默跳过）。

## 第二轮回归（2026-09-28 上午真机，pid 41708，0ff8561 之后）

第一轮修复出包真机实测后，Andy 报告新行为：

- 全按键支持**关闭**：TV 键（配置"打开无线麦"）不触发动作，原生键入漏成 `·`
  （VK_OEM_3 经 IME）；物理反引号键输出正常。
- 全按键支持**开启**：TV 键能打开无线麦，但物理反引号键无法输出；
  期望 = 与返回键一致（动作单响应 + 不拦截物理按键），关闭 = 回到原有
  「遥控器优先」逻辑。

证据（`sayall-diagnostic.log` pid 41708 + `%ProgramData%\SayAll\rc003-helper\helper-task.log`）：

- `targets_changed enabled=true usages=28,35,4a,…`（第一轮修复生效，目标集已下发）；
- 开启态 `map_edges … gate(sw=24→53 lk=…)`：LL 钩子仍在逐键吞 VK_OEM_3（sw=被吞边沿计数），
  同窗口 `bridge closed edges=0`——报告层**没有**接管，边沿全部来自钩子；
- helper 日志 `[AGENT-STALE] ×21`：宿主内 agent 仍是 `2026-09-26.canary-gate` 旧构建，
  新动态 targets 命令全部被拒（`targets:rejected_report_not_targets` / `rejected_bad_array`），
  `clearUsages` 从未含 0x35 → 报告层未接管 TV，`ENHANCED_OWNED_MASK` 永不置位，
  钩子常驻抑制继续吞 VK_OEM_3（物理反引号被杀）；
- 关闭态（`targets_changed enabled=false` 后）`map_edges … gate(sw=35 lk=15)`：
  sw 不再增长、lk +1，钩子完全不吞——TV 原生键入泄漏成 `·`。

根因 A（关闭态泄漏，代码缺陷，3afdd01 修复）：`apply_enhanced_capture_state`
翻转 `mappings.enabled`，而它是映射功能总开关——`mapped_mask()` 在
`enabled=false` 时恒 0 → `key_gate::configure` 后 `PERSISTENT_MASK` 归零 →
Home/TV「遥控器优先」常驻抑制被一并拆掉。开关语义应为「只切换捕获通道」
（报告层增强 vs 原有 Raw Input + 键盘门控兜底），映射引擎与门控在两种状态下
都必须保持激活。

根因 B（开启态拦截物理键，部署问题，非代码缺陷）：见上 `[AGENT-STALE]`。
宿主换新（让 WUDFHost 重新加载 `2026-09-27.dynamic-all-key` agent）后，
报告层接管全部动态目标 + ownership 握手置位 `ENHANCED_OWNED_MASK`
（rc003_bridge.rs:917）→ 钩子对已接管按键让位（key_gate.rs:393 `!enhanced_owned`）
→ 物理按键透传，兑现"同返回键"语义。

回归测试：`enhanced_capture_enable_pushes_targets_and_disable_clears` 增补
阳性对照二——关闭后 `mappings.enabled` 必须保持 true；旧实现（翻转）下该测试
精确红在此断言（已实测 FAILED → 修复后 passed）。

## 隐私检查

本文仅含日志事件名、计数与提交号，未包含个人路径、设备身份、语音内容或凭据。
