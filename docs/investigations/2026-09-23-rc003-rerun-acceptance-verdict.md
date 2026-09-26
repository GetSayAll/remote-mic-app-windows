# RC003 增强捕获轨：反复运行验收判决（2026-09-23 16:06–16:11）

**结论：两条路径都成立。** 上一次「第二次运行必崩」的修复在真机上生效（`action=reuse`，
一个字节都没写）；并且**接管路径（`[ATTACH]`）首次在真机端到端跑通**——不复制、不注入，
常驻 agent 用持久令牌自己连回来，补发的 `arm` 生效，三键边沿照常上报。

同一轮里还暴露出 **6 处"证据/鲁棒性"缺陷**：它们没有让本次运行失败，
但会让**下一次排查时读到错误的结论**。已全部修复并配自检回归（含阳性对照）。

- 原始证据：[`2026-09-23-acceptance-run-raw.log`](../../hardware/RC003/evidence/2026-09-23-acceptance-run-raw.log)
  （helper-run.log 第 728–1149 行，未改动）
- 整理版：[`2026-09-23-acceptance-run.log`](../../hardware/RC003/evidence/2026-09-23-acceptance-run.log)
  （只删掉 `[HB]`/`[RENEW]` 两类周期行，其余一字未改）
- 前置：Andy 已"断开并重新配对 RC003"清除旧世代 tap（宿主 `32684` → `31284`，
  占用者探针 `users=(none)` 且阳性对照通过）

## 1. 判决：两轮四次启动，谁成了什么

| # | 时刻 | 计划 | 结果 | 判据 |
| --- | --- | --- | --- | --- |
| 1 | 16:06:37 | `Canonical`（全新注入） | **通过** | `[PREP] action=reuse`（不写盘）、`[INJECT]` 后 `[VERIFY-MODULE] gadget_modules_in_host=1`、`[HELLO] auth=true`、`[CONFIG] arm=true`、三键各一组 `[EDGE]`、`how=injected` |
| 2 | 16:10:08 | — | **端口冲突，退出码 8** | 上一轮仍在 `--duration 300` 窗口内（其 `[SUMMARY] uptime_s=228` 出现在 16:10:25），`os error 10048` |
| 3 | 16:10:37 | `Attach`（接管上代 tap） | **通过** | `[TOKEN] source=file`、`[TAP] resident=1`、`[PREP] action=skip_copy_attaching_existing_tap`、`[ATTACH] skip_injection`、`[HELLO] auth=true`、三键各一组 `[EDGE]`、`how=attached_existing_tap` |

第 3 行的意义要说清楚：**"注入了"和"接管了"是两条不同的代码路径**，
而这次真正证明的是后者——包括最容易漏的一环：收尾时发过 `disarm`，
所以接管后**必须补发 `arm`**，否则心跳、续约、握手全都正常，但按键会把缓冲区原样留给
`kbdhid`（三键继续完全不可见）。

三键 edge 明细（两轮一致，各 5 次按下 → 6 条 edge 含释放）：

```
[EDGE] buttons=back       usages=0x00F1 n=1
[EDGE] buttons=volume_up  usages=0x0080 n=1
[EDGE] buttons=volume_down usages=0x0081 n=1
```

`edges=6` 与 `distinct_buttons=|back|volume_down|volume_up` 出现在两轮的 `[SUMMARY]` 里。

**两轮都是 Andy 手动结束的**（uptime 228s / 40s，都不是 300s，且没有 `[TIMEUP]`），
所以 `--duration` 到期自动收尾这条路径**在真机上仍未验证**（见 §4）。

## 2. 本轮暴露的 6 处缺陷

共同点：**它们不影响功能，只污染证据**——而排查代码时我们唯一依赖的就是证据。

| # | 缺陷 | 现场表现 | 后果 | 修法 |
| --- | --- | --- | --- | --- |
| 1 | "新一轮运行"分隔线由 `Logger::new` 写 | 每轮武装前多一条分隔线（16:06:38.031 / 16:10:37.414） | `grep '新一轮运行'` 的**运行起点计数是错的**：真实 6 轮数成 11 条。这条分隔线当初加进来就是为了让起点可辨，它自己把它破坏了 | 分隔线只在 `Logger::open_round` 写；内部线程用 `Logger::new` |
| 2 | agent 的 `connect` 可以叠加 | 接管轮 3 条连接、2 条被 RST（`10054`/`10053`） | `sock` 被后到的 `connect` 覆盖，被覆盖的自缢于 GC 并发出 RST；**哪个赢取决于完成顺序**，接管路径变成看运气 | `attemptSeq` + `connecting` 并发守卫；输家主动 `close`（发 FIN 而非 RST） |
| 3 | `pump` 的读失败分支绕过 `dropConnection` | 断线重连确实发生了，但 `rx_timeouts=0`、`auth_rejected=0` | **"连接死过"在统计里完全看不见**，会被读成"看门狗从未触发"；且 `sock`/`retryNotBefore` 不复位 | 改走 `dropConnection('read_error')`，新增 `read_errors` 计数；并用 `mine` 核对回调属于哪条连接 |
| 4 | `discarded` 一个计数管两件事 | 接管轮 `discarded=11`，无从判断含义 | "未连接时丢掉的心跳" vs "令牌不符的命令"安全含义相反（前者正常、后者=有人冒充助手），却无法区分 | 拆成 `send_dropped` 与 `cmd_rejected` |
| 5 | `[DLL]` 汇报照搬另一条路径的字段 | 接管轮打出 `reused_existing=false sha256_verified=false copied=false` | 读起来像"没校验就用了"，而事实是这轮**一个文件字节都没碰** | 按 plan 分别汇报（`action=attach_existing_tap` + `touched_files=false`） |
| 6 | `[CONFIG]` 的 `arm` 恒为 `true` | `sent=false arm=true` 自相矛盾的一行 | 实际是那条连接的 arm 根本没送到（socket 已被对端重置），但日志说它武装了 | 逐条记录 `arm_sent`/`mode_sent`/`restore_sent`，并注明"本协议无 ack" |

顺带：`[EDGE]` 的释放行原本打出空的 `reason=`，已补 `reason:'state'`——
`reason` 存在的意义就是区分"正常状态变化"与"被强制释放"，空值等于没用。

### 2.1 关于缺陷 1 的一个方法论注记

这次是我**自己**被它误导的：读日志时把 11 条分隔线当成 11 次运行，
直到发现"有分隔线却没有 `=== rc003-helper ===` 行"才起疑。
一条为可观测性而加的机制，在没有自检的情况下变成了**反可观测**的。

## 3. 阳性对照（证明判据有分辨力）

| 判据 | 对照做法 | 结果 |
| --- | --- | --- |
| agent 用例 D「重新上线只允许连一次」 | `agent/control_make_noguard.py` 生成三处回退的副本（去并发守卫、去竞争检查、pump 退回旧行为），再跑同一用例 | 当前版本 **4/4 PASS**（被连 1 次，断线可见=1）；对照版本 **用例 D FAIL**（**被连 3 次 / 3 条 hello**，与真机的 3 条连接一致，断线可见=0），退出码 1 |
| helper 用例「同轮内再建 Logger 不写分隔线」 | 与「开新轮必须写」成对出现（阴性 + 阳性） | 同一检查项内同时断言两个方向；缺陷 1 的现场证据即 `helper-run.log` 里"分隔线紧跟着武装横幅" |
| helper 用例「端口 10048 文案可操作」 | 自检内真绑一个端口再绑第二次 | 第二次必然失败且文案含"已经有另一次运行在跑" |
| helper 用例「接管轮不得报 sha256_verified」 | 配一条阳性对照：复用路径**必须**报 `sha256_verified=true` | 否则"该字段永远不出现"会让上一条假通过 |

证据：`evidence/agent_selftest.out`（4/4）、`evidence/agent_selftest_noguard_control.out`（用例 D FAIL）、
`evidence/helper_selftest.out`（38/38）。

自检项数：helper **34 → 38**；agent 自检台 **3 → 4**。

## 4. 仍未验证（需要真机，且需要 Andy 本人提权）

1. **`--duration` 到期自动收尾**：两轮都是手动结束（228s / 40s），没有 `[TIMEUP]`。
   这条路径目前只有合成自检（静默客户端占住连接 + 绝对 deadline）覆盖。
   建议：起一轮后什么都不做、让它自己到点。
2. **`run` 模式的拦截生效 + 无回归 + fail-open 现场验收**：目前所有真机证据都来自 `observe`，
   而 `observe` 一个字节都不清——它证明不了"三键被拦下来了"，也证明不了
   "确定/主页/方向键无回归"。
3. `[STALE-TAP]` 分流（旧世代 tap 的归因与清理指引）：本次因重配对清干净而**没有走到**。
4. `--new-generation`（同宿主双 Gadget 实例）：未验证。
5. 接管路径在**多轮反复**下的稳定性：本次只做了 1 次接管（16:10:37）。

## 5. 复现方法（无需设备）

```bash
# helper：38 项（含本轮 4 项新增）
hardware/RC003/helper/target/release/rc003-helper.exe --selftest

# agent：4 项协议/生命周期用例（需 frida 17，受管 venv 里有）
cd hardware/RC003/helper/agent
python agent_selftest.py
# 阳性对照：去掉修复的副本必须让用例 D FAIL
python control_make_noguard.py
python agent_selftest.py ../target/tmp/rc003_agent_noguard.js   # 期望退出码 1
```

## 6. 相关文档

- 上一轮（第二次运行必崩）的根因与修复：[`2026-09-23-rc003-helper-rerun-blocked-by-locked-gadget.md`](2026-09-23-rc003-helper-rerun-blocked-by-locked-gadget.md)
- 首次真机 observe 与两处"安全上限失效"：[`2026-09-23-rc003-observe-first-real-run.md`](2026-09-23-rc003-observe-first-real-run.md)
- 路线总览与代价：[`2026-09-22-per-device-key-interception-route-selection.md`](2026-09-22-per-device-key-interception-route-selection.md)
- 硬件取证与运行手册：[`../../hardware/RC003/README.md`](../../hardware/RC003/README.md)
