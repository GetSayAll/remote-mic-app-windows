# 全按键支持开关显示已开启但所有按键边沿被主程序丢弃（动态目标集从未下发）

- 发现日期：2026-09-28
- 状态：已定位根因，待修复
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

- 单元：待实施（见上）。
- 真机：`deferred`——需 RC003 实机验收：开启后日志出现 `targets_changed enabled=true usages=…`
  与 `first_edge`，三键与已映射按键动作生效；关闭后 `targets_changed enabled=false` 且旧路径恢复。
- 安装包：`deferred`——从干净 main 重出后安装，核对 `reason=invalid` 消失、关键二进制哈希
  （NSIS 静默安装遇占用文件会静默跳过）。

## 隐私检查

本文仅含日志事件名、计数与提交号，未包含个人路径、设备身份、语音内容或凭据。
