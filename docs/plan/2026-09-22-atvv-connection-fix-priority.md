# ATVV 连接故障：必须做的优化（优先级排序）

- 日期：2026-09-22
- 触发：用户报 `发现 ATVV 服务返回状态 1`（本机日志 724 次）与
  `发现 ATVV 服务返回状态 3`（本机 0 次，来自其他用户）
- 证据来源：`%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`（88719 行，
  2026-09-09T02:24 → 2026-09-22T05:20，ver 0.2.3 → 0.2.14）、
  `crates/sayall-windows/src/ble.rs`、`bluetooth_radio.rs`
- 本文件只列**必须做**的项，不含可选优化

---

## P0-1 结构化日志必须能区分 GATT 状态码（不改行为）

**问题**：`ble_error_code`（`ble.rs:1110`）把所有 `PlatformError::Gatt` 一律归成
`gatt_status_failed`，且不落状态码。状态 1（Unreachable）、2（ProtocolError）、
3（AccessDenied）在结构化日志里**长得完全一样**。用户报"状态 3"时，
我们手上只有 `error_code=gatt_status_failed`，无法反查。

这直接违反 AGENTS.md「功能点必须自带日志」的判据：*一次用户报障 + 一次日志拉取 =
定位到具体环节*。当前做不到——连"是哪个环节的哪一种失败"都分不出来。

**做法**：
1. `find_service` / `find_characteristic` / `require_success` 的失败路径带上
   `status.0` 原始数值，落 `gatt_status=N` 字段（与 09-17 补 HRESULT 保真同一手法）。
2. `ble_error_code` 按状态码细分：`gatt_unreachable` / `gatt_protocol_error` /
   `gatt_access_denied`，替代单一的 `gatt_status_failed`。
3. 保留 `raw_error` 原文（已存在），确保状态码之外的信息不丢。

**约束**：状态码是数值枚举，不含设备身份/地址/语音内容，符合 LOGGING.md 隐私红线。
不改变任何连接行为，纯打点。

**验收**：单元测试覆盖四个状态值的映射；`gatt_status=3` 可被 grep 到。
**状态**：deferred（本机 13 天日志里没有状态 3 样本，需用户侧下次报障才能实测）。

---

## P0-2 把"遥控器不在线"和"GATT 真故障"分成两条 UI 文案

**问题**：`ble.rs:1254` 把原始异常直接拼进用户可见文案：

> `Xiaomi voice remote GATT operation failed: 发现 ATVV 服务返回状态 1；将在 4 秒后进行第 2 次重连`

这句话对用户毫无可操作性：它把"遥控器睡着了、链路还没起来"说成了"GATT 操作失败"。
用户既不知道要不要动手，也不知道该动什么手。状态 3 更糟——那是权限问题，
文案却和链路问题一模一样。

**做法**：按错误语义给**用户可见文案**分流（不动底层错误类型）：
| 情形 | 用户文案方向 |
| --- | --- |
| Unreachable（1）且上一条是无 `device_arrived` 的 resume | "遥控器暂时不在线，正在等待它唤醒…"（说明会自动恢复） |
| AccessDenied（3） | "无法访问蓝牙设备，请检查遥控器是否在 Windows 蓝牙设置中被禁用"（给可操作指引） |
| 其他 GATT 状态 | 保留技术描述，但同时给"正在自动重试"的安定信息 |

**注意**：AGENTS.md 明确禁止把"重启电脑/重开蓝牙"作为常规解法推给用户；
只有公开 API 全失效时才允许提示人工介入，且须说明原因与预期效果。
`ble.rs:437-439` 已经有一条正确的范例（"蓝牙链路暂时不可用，正在持续重试…"），
应把这套口径推广到所有连接失败路径。

**验收**：前端文案断言测试；每种错误类别一条。
**状态**：可立即执行（不依赖硬件复现）。

---

## P0-3 唤醒恢复优化：等遥控器上线再发起 uncached 服务发现

**问题**（本机实测，最影响体验的一条）：
6 个故障 episode **全部**紧跟在 `system_resume` 之后（睡眠 1.6–17.8 小时）。
其中 `device_from_address` 与 `device_properties` 都 passed——因为它们只读
Windows 本地缓存；**只有 `service_discovery` 失败**，因为
`ble.rs:2526` 用 `GetGattServicesForUuidWithCacheModeAsync(uuid, Uncached)`
强制走空口，而遥控器还没回到无线电上。

实测代价：
| 唤醒 | 失败次数 | 恢复耗时 |
| --- | --- | --- |
| 09-19 05:06 | 422 | **2.5 小时**（醒着期间一次没成，直到 09-20 10:34） |
| 09-21 01:20 | ~177 | 2.5 小时 |
| 09-21 15:53 | 70 | 26 分钟 |
| 09-22 04:56 | 50 | 18 分钟 |
| 09-22 01:19 | 1 | 20 秒 |
| 09-20 10:32 | 4 | 2 分钟 |

对照组证明代码没问题：09-22 05:15:05 链路一通，**同一路径 173 ms 通过**，
全链路 2.2 s。失败耗时 7689/7697/7698 ms 高度一致，是 **OS 侧放弃**的时长
（`ble.rs` 里没有 7.7 s 常量，只有 30 s `REQUEST_TIMEOUT`）。

**做法**：唤醒后不要立刻无条件重连，先等一个"遥控器上线"信号再发起
uncached 发现。可用信号：
- `raw_input device_change action=device_arrived`（本机实测：09-22 05:14:26/27/28/43
  连续 4 次 arrived 之后，连接立刻成功）；
- `BluetoothLEDevice.ConnectionStatus == Connected`；
- 遥控器 HID 设备节点在 PnP 中重新出现。

具体形态建议：唤醒后给一个短的有界等待窗口（如最多 30–60 s），窗口内
出现上述信号就立即发起连接；窗口内没有则照常退避重连。
**不要**把等待做成永久门控——没有信号时仍必须自动重连（用户侧零介入）。

**约束**：
- 延迟优化不得降低成功率（AGENTS.md 2026-09-05 晚要求）；
  验证必须覆盖**冷/闲置后首用**，不能只测热态。
- 必须持锁实测，不得凭推理改常量（AGENTS.md 延迟/时序类改动规则）。

**验收**：真机走一次真实过夜睡眠 → 唤醒 → 测量从唤醒到 connected 的耗时；
对比本基线（最坏 2.5 h）。RC001 / RC003 分别验收。
**状态**：deferred（需真实过夜睡眠 + 真机）。

---

## P0-4 状态 3 归因能力：设备节点访问失败必须能自愈

**问题**：状态 3 = `AccessDenied`（枚举值来自 windows-0.62.2 绑定：
`Success=0 / Unreachable=1 / ProtocolError=2 / AccessDenied=3`）。这是**权限层**
故障，与状态 1 完全不同。

已排除（全仓 grep 无命中）：
- 我们没有调用任何 `GattProtectionLevel` / `ProtectionLevel` → 不是特征保护级别不匹配；
- 所有 GATT 操作都在 MTA 套间内（`ble.rs:2665-2671` `WinRtApartment`）→ 不是缺 COM 套间；
- 只用公开 API，不碰任何私有配置。

已确认的先例（同一台机器上真的出现过 AccessDenied 类故障）：
- `Bugs/2026-09-07-ble-unreachable-both-remotes.md:10`：普通用户
  `pnputil /restart-device` 得「拒绝访问」；`/disable-device` 被拒。
- 09-12 A/B 对照（PR #94→#97 之间）：配对的 AssociationEndpoint selector →
  `FromIdAsync` 在非 UI 线程返回 `E_ABORT(0x80004004)`；仓库因此改用
  `FromBluetoothAddressAsync`（`ble.rs:1803`）。同一类"入口权限/套间不对 → 被拒"。
- 本机 0.2.6 上 `stage=device_from_id` 失败 75 次（160 行），
  时间窗 2026-09-12T04:53 → 08:52，`windows_api_failed`。**HRESULT 未落盘**
  （保真打点 09-17 才加），需翻当时安装版日志才能坐实。

**做法**：
1. 先做 P0-1（否则状态 3 进了日志也认不出来）。
2. 拿到状态 3 样本后，按上述候选逐条排除，优先级：
   ① BLE 设备节点/服务句柄被拒（bthserv 拒绝、句柄跨会话、节点半僵死）；
   ② 遥控器在 Windows 蓝牙设置里被禁用 / 配对不完整；
   ③ 应用缺蓝牙权限或组策略限制。
3. 按 AGENTS.md「用户侧零介入」，**能自愈的必须自愈**：设备节点访问被拒属于
   "公开 API 仍可用"的场景（`BluetoothLEDevice` 能创建、只是 GATT 访问被拒），
   应补一条"重新打开设备句柄 / 重建设备对象"的恢复路径，而不是弹提示让用户处理。

**验收**：需要用户侧 JSON 诊断摘要或新包日志中的 `gatt_status=3` 样本。
**状态**：deferred（本机无样本，卡在 P0-1 上线 + 用户复现）。

---

## P1-1 `conn_params` 从未生效——吞吐优化实际没落地

**问题**：`conn_params result=unavailable reason=connection_parameter_api_failed`
出现在**每一次**连接尝试里，包括 09-22 05:15:05 那次成功连接。

这条 2026-09-07 引入的吞吐优化（`ble.rs:1808-1836`）是**真的没生效**——
注释里写着"RC001 送达率实测仅 ~52%，ThroughputOptimized 收紧连接间隔"，
而日志显示 `RequestPreferredConnectionParameters` 从来就没成功过。

影响：遥控器语音的 15 ms/120 B 音频帧送达率可能仍处于未优化状态。
但它**不是**本次 ATVV 故障的原因（它失败后降级为默认参数，连接照常建立）。

**做法**：
1. 区分两层失败：`BluetoothLEPreferredConnectionParameters::ThroughputOptimized()`
   构造失败（运行时版本门禁，Win11 22000+）vs
   `RequestPreferredConnectionParameters` 调用失败（API 返回 Err）。
   当前两处都写同一行日志（`ble.rs:1823-1828` 与 `1830-1835`），**分不出是哪层**。
2. 确认本机 Windows 版本是否满足 22000+；若满足，说明是这个 API 在
   MTA 线程 / 这个设备上的真实限制，需换路径（如连接建立后延迟申请、
   或在 UI 线程申请）。
3. 若确认 API 不可用，则 09-07 那次的"送达率 52%→98%"结论需要重新归因——
   本机日志显示该优化从未生效，那次的改善必定另有原因。

**验收**：日志能区分两层失败原因；连接后能读到实际生效的连接参数。
**状态**：可部分立即执行（日志分层）；生效验证需真机。

---

## P1-2 `service_discovery` 偶发长挂起（max 54.98 s）

**问题**：1257 次服务发现失败中，耗时 max 54980 ms、avg 7743 ms。
绝大多数是 7.7 s（OS 放弃），但存在接近 55 s 的长挂起样本。
长挂起会让整个重连循环停摆——而 `ble.rs` 的 `REQUEST_TIMEOUT` 是 30 s，
说明有些 WinRT 操作**超出了应用侧超时**（官方声明 GATT 连接过程不可取消，
所以应用侧超时只能让 IPC 调用方返回，无法终止内部操作）。

**做法**：
1. 先把长挂起样本单独捞出来（`elapsed_ms > 20000` 的 service_discovery 失败），
   看它们的 `error_code` 与相邻事件（是否有 resume、是否与 radio recovery 重叠）。
2. 若长挂起集中在唤醒后首轮，可能被 P0-3 一并解决；若分散，需要独立调查
   （Bugs/2026-09-12 那次 MTA/UI 线程问题就是同类）。
3. 不得为不可取消的 GATT 过程伪造超时线程（`ble.rs:83` 注释已有的决策）——
   要保持这个约束。

**验收**：长挂起样本的归因结论；不得无证据改常量。
**状态**：deferred（需捞样本分析）。

---

## P1-3 幂等地缩小恢复动作范围（避免下一次"空转"）

**问题**：09-22 那次故障跑了 50 次失败、5 个恢复窗口、`window` 涨到 5，
其中第 2 次 radio Off/On 执行 passed（05:14:58–05:15:01）但链路并未因此恢复——
真正恢复的是 05:14:26 起遥控器重新上线。
也就是说这 18 分钟里的 2 次 radio 循环**都是无效动作**。

已有正确机制：`is_stack_exhausted`（`bluetooth_radio.rs:192`）对
`windows_resource_exhausted` / `winrt_operation_aborted` 跳过恢复动作。
但 **Unreachable（状态 1）没被纳入**——而它同样是"遥控器不在线、开关无线电无益"
的形态。本机 6 个 episode 里累计执行了大量无效 radio 循环。

**做法**：
1. 把"遥控器不在线"类失败（Unreachable，以及 P0-1 新增的 `gatt_unreachable`）
   纳入跳过策略：链路没建立时，开关无线电不解决"对端不在广播"。
2. 但**不能**直接照抄 `is_stack_exhausted` 的"完全跳过"——Unreachable 有时确实
   需要无线电恢复（如 09-07 那次的僵死）。建议改为"降低频次/延长冷却"，
   或加一个前置判据（遥控器是否在 PnP 中 present）。
3. 决策必须持锁实测支撑，不得凭推理。

**验收**：A/B 对照脚本（`scripts/analyze-radio-recovery-ab.py` 已有）证明
新策略不降低成功率、且减少无效循环次数。
**状态**：deferred（需真机 A/B）。

---

## 执行顺序建议

```
P0-1 (日志区分状态码)  ─┐
                       ├─→ 一次构建，可立即做，纯打点不改行为
P0-2 (UI 文案分流)     ─┘

P0-1 上线 → 等用户复现状态 3 → P0-4 (状态 3 归因)

P0-3 (唤醒等待上线)    ── 独立，需真机过夜睡眠验证
P1-1 (conn_params)     ── 日志分层可立即做
P1-2 (长挂起)          ── 依赖捞样本
P1-3 (恢复范围)        ── 需 A/B 实测
```

**P0-1 与 P0-2 合起来是一个独立工作项**：都只碰打点与文案，不碰连接逻辑，
风险最低，且立刻提升下一次报障的可定位性。建议作为一个 commit 落地。
