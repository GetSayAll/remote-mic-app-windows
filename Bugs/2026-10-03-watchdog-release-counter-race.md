# 2026-10-03 CI 慢机必现：释放边沿已到、`watchdog_release_total` 仍为 0（状态发布在信号之后）

## 现象

PR #183（纯脚本改动，分支 = main + `scripts/`，Rust 代码零改动）CI `verify` 的
`Test Rust workspace` 失败：

```
test rc003_bridge::tests::silence_watchdog_releases_pressed_buttons ... FAILED
panicked at crates\sayall-windows\src\rc003_bridge.rs:2604
   （断言 bridge.snapshot().watchdog_release_total >= 1）
test result: FAILED. 249 passed; 1 failed
```

第一个断言（`released`，按键确实被释放）**通过**；第二个断言（强制释放必须被计数）失败。

同一份 bridge 代码在 #179 的 CI（run 37101084835）`verify` **通过**；本地压测 15/15 全过。

## 判定与证据

- 本地：`cargo test -p sayall-windows silence_watchdog_releases_pressed_buttons` × 15 → **15/15 PASS**。
- CI（双核慢机）：同一分支可复现（run 37103750293 attempt 1 红）。
- 分支内容 = main + 纯脚本 ⇒ 非本次改动引入，是调度竞争在慢机上暴露。

## 根因

`crates/sayall-windows/src/rc003_bridge.rs` 会话收尾的**发布顺序反了**（修复前 1514-1532）：

```rust
for edge in released { sender.send(EngineMessage::GateEdge(edge)); }   // ① 先投边沿（唤醒观察者）
let mut state = lock(shared);
if drop_reason != "helper_bye" { state.watchdog_release_total += 1; }   // ② 后落计数
state.pressed.clear(); state.last_rx = None; /* phase/helper_pid */      // ③ 再清按下集合
```

边沿是「释放已经发生」的通知。投出后任何观察者（测试断言、诊断读取、引擎侧回查）都可能
**立刻**读快照，而此刻 ②③ 尚未执行 → 读到「已释放但计数为 0、`pressed` 仍非空」的半更新状态。

不是等待余量不足：测试在收到边沿后立即断言，慢机上必然可能抢在 ② 之前；快机上清理线程
总能先跑完 ②③，所以本地看不见。

## 修复（最小）

把状态更新整块移到投边沿**之前**，并让锁的作用域在发送前结束（不持锁发送）：

```rust
{ let mut state = lock(shared); /* 计数 / clear pressed / last_rx / phase / helper_pid */ }
for edge in released { let _ = sender.send(EngineMessage::GateEdge(edge)); }
```

语义不变：边沿仍表示“已被释放”；变化只是**保证任何观察到边沿的一方读到的快照已经一致**
（publish state, then signal）。

## 验证

- 修复后本地：`scripts/verify-rc003-helper.ps1 -Full`（agent 逻辑台 52/52、助手 23/23、
  `cargo test -p sayall-windows` 全量、runtime-simulation 编译检查、fmt）×1 + 该用例 ×15 全过。
- 回归判据：同一用例在 CI 慢机上不再出现「边沿已到、计数为 0」。该用例本身就是回归测试
  （修复前的失效机制正是它在 CI 上抓到的；修复后其断言由构造保证确定性）。
- 同类模式另一处：行处理路径 `edges_applied += 1` 亦在投边沿之后（`rc003_bridge.rs` 约 1310/1314）。
  本次**不改**——没有任何断言以该顺序为前提，扩大范围无收益；记录在此备查。

## 边界

- 仅调整发布顺序，不改释放/计数/相位语义；不涉及首次按下修复
  （见 `Bugs/2026-10-03-first-press-lost-before-ime-switch.md`）。
- 该 flake 只影响 CI 判定与诊断可读性，不影响真机行为（现场释放路径照旧）。

## 隐私检查

仅含代码位置、计数与断言文本；不含设备身份、语音内容或用户路径。
