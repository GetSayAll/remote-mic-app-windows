# 语音优先：请求成功但键鼠实际 2.4–4 s 后才切到 Balanced，短会话丢首字

- 发现日期：2026-11-23
- 状态：已修复（1.9.11 先测后调：按非遥控器 BLE HID 数量持久化 observe/hold；资源够的配置不碰鼠标，降级配置才常驻 15 ms；1.9.5 的 60 ms 空闲预热已否决）
- 影响范围：1.9.4（含自动「语音优先」）之后、1.9.5 之前的本地包；Windows 25H2，RC003 + BLE 键盘 + 鼠标；本机 MediaTek 适配器
- 功能点：语音优先 / BLE HID 连接参数让出
- 现象：语音输入偶尔没有文字，或首字被吞；同一版本内时好时坏。
- 复现条件：遥控器、BLE 键盘、鼠标同时连接；做 2–4 s 的短语音会话。
- 正常预期：语音期间键鼠让出蓝牙连接事件，遥控器语音从第一帧就是满速。

## 证据

### 1.9.4 日志：短会话速率呈双峰

2026-11-22 16:18–16:20（UTC）连续会话：

| session | 时长 | `voice_audio_rate` | 文字插入 | `voice_priority` |
| --- | --- | --- | --- | --- |
| 36 | 2.532 s | degraded 4,454 /s | 无 | active / yielded=2 |
| 37 | 2.822 s | full 13,267 /s | 有 | active / yielded=2 |
| 38 | 2.739 s | degraded 9,550 /s | 有 | active / yielded=2 |
| 39 | 3.957 s | degraded 8,248 /s | 有 | active / yielded=2 |
| 40 | 4.142 s | degraded 10,661 /s | 有 | active / yielded=2 |
| 42 | 3.032 s | degraded 6,332 /s | 0，随后 rejected | active / yielded=2 |
| 43 | 3.126 s | degraded 6,449 /s | 无 | active / yielded=2 |
| 44 | 2.416 s | degraded 6,059 /s | 无 | active / yielded=2 |

同一时段 19 次会话中 8 次 degraded；排除 1 次 SayAll 自身前台后，18 次实际语音里
5 次没有插入文字（约 28%）。所有会话都记录 `state=active yielded_devices=2`，
说明该状态只表示请求被接受，不表示键鼠真的切到了 Balanced。

### 2. 独立只读探针：切换实际延迟 2.4–4.1 s

2026-11-22 16:29（UTC）用 `hid_params_read_probe` 每 500 ms 读回：

| session | 语音开始 | `apply` 成功 | 实际读到 48/0/400 | 延迟 | 音频 |
| --- | --- | --- | --- | --- | --- |
| 46 | 16:29:27.214 | 16:29:27.325 | 16:29:30.374 | 3.05 s | full 12,698 /s |
| 47 | 16:29:33.286 | 16:29:33.607 | 16:29:36.026 | 2.42 s | full 13,866 /s |
| 48 | 16:29:38.141 | 16:29:38.372 | 16:29:42.196 | 3.82 s | degraded 10,969 /s |
| 49 | 16:29:45.714 | 16:29:46.012 | 探针结束前未读到 | ≥2.6 s | degraded 10,607 /s |

在切换前，两只 HID 都保持 `6 / 44 / 216`（7.5 ms）；切换后为 `48 / 0 / 400`
（60 ms），遥控器始终 `12 / 0 / 200`（15 ms）。恢复通常在会话结束后的
0.25–0.5 s 内回到 `6 / 44 / 216`。

结论：Windows 的 `RequestPreferredConnectionParameters` 会先返回 `Success`，
实际连接参数在 2.4–4.1 s 后才会生效。当前实现只在 `Streaming`（按下语音键后）
才请求，短会话的整个开头都处于 6/44/216；首字正好落在这一段。

### 3. 1.9.5 空闲预热与读回验证

修复后（1.9.5 本地测试包，未提交构建）：

- 应用启动、遥控器 Ready 且桌面空闲约 1 s 后，未按语音键即读到键鼠
  `48 / 0 / 400`；遥控器保持 `12 / 0 / 200`。
- 读回日志：`voice_priority action=apply ... status=success` 之后 3.4 s 出现
  `voice_priority action=observe slot=0 interval_units=48 latency_events=0
  timeout_units=400 effective=true`，两只设备随后 `state=active yielded_devices=2`。
- 最终构建（PID 24552）：17:03:02 active（两只 48）；17:03:15 桌面活动触发
  `restore` → `off`；17:03:21 空闲后重新 `applying`，17:03:23.8 两只读回 48 → active。
- 这证明：只有实际读到 48/0/400 才报 active；预热、撤销和重新预热都生效。
- 用户实机复测（2026-11-23 17:06 UTC，sessions 62–67）：
  - session 62：`apply` 17:06:17.946 → 17:06:20.281 才读到 48；语音 17:06:18.528
    开始，前约 1.75 s 仍是 `6/44/216`；速率 15,245 /s，文字 10/11 字符。
  - session 63–67：语音开始时 48 已生效；速率 15,238 / 14,903 / 14,008 /
    14,637 / 15,552 /s（全部 full），均有文字插入；65 的 `voice_verify` 为
    `reacted`。
  - 这证明：预热完成后的 5 次短语音全部 full；session 62 是切换尚未完成就按键的
    残余边界。
- 语音首字主观效果：session 62 仍是唯一可能丢首字的会话；其余 5 次已由用户实测。
  彻底消除需缓冲首段音频或更早的预测信号，标记 `deferred`。

## 根因

公开 WinRT API 只能“请求”首选参数，Windows BLE/HID 栈实际切换需要约
2.4–4.1 s；原来的按会话（`Streaming`）请求无法在短语音开始前完成切换。
`apply status=success` 和 `active yielded=2` 都不能作为“已经让出”的证据。

## 修复

- 遥控器 Ready/Streaming/Draining 且**桌面 1 s 无普通键鼠活动**时，提前请求
  Balanced；语音流中仍无条件保持让出。
- 普通键盘活动、鼠标移动或滚轮立即恢复；单纯鼠标点击不撤销预热，避免
  “点输入框 → 马上按语音”再次丢首字。
- 读回实际 `GetConnectionParameters`，只有 interval≥24 才计入
  `yieldedDevices`/`active`；日志新增 `action=observe` 的 interval/latency/timeout，
  不含地址或设备名。
- 请求被接受但 5 s 内仍未生效时，状态置 `failed`，日志保留观察结果。

## 验证

- `cargo test --workspace --tests`、`cargo fmt --all -- --check`、
  `cargo check -p sayall-windows-app --features runtime-simulation` 通过；
  `scripts/ci-preflight.ps1` 7/7 通过。
- 前端 45 个测试文件、719 passed / 10 skipped；`pnpm build` 通过。
- 1.9.5 安装校验通过；嵌入式前端 18/18；空闲预热与读回日志实测通过。
- 用户 6 次短语音复测：5 次在语音开始前 48 已生效，全部 full；1 次（session 62）
  在 apply 后 0.58 s 就按下，切换尚未完成。
- **deferred**：session 62 这类“移动/点击后 3 s 内立刻说”的首字彻底保护；
  RC001、其它适配器、键鼠耗电；鼠标点击-only 不撤销预热的体感。
- 已知残余边界：先移动鼠标、3 s 内立刻按语音键时，预热尚未生效，仍可能丢首字；
  该场景需要缓冲首段音频或更早的预测信号才能彻底解决。

## 2026-11-24 方案调整（1.9.6）

- 1.9.5 的 1 s 空闲预热被用户否决：日志里 apply/restore 每隔几秒反复发生，
  鼠标回到桌面时先吃 60 ms 间隔，体感是频繁卡顿。空闲预热不应作为默认。
- 1.9.6：**不再按空闲预热**。只在 `Streaming` 时让出；若本次会话
  `voice_audio_health=degraded`，结束后保留 **5 s 重试窗口**，用户重说时
  链路通常已在 48/0/400；普通键盘活动或鼠标移动/滚轮立即取消窗口。
  鼠标平时保持 7.5 ms，只有语音期间（用户已接受）和降级后的重试窗口可能变慢。
- 冷启动首字仍可能丢：如果语音在第一次 `apply` 后 2–4 s 内开始，切换尚未完成；
  这是平台切换延迟的残余，需手动低延迟模式、缓冲/提示或硬件隔离才能彻底解决。
- 15 ms `ThroughputOptimized` 和 `GattSession.MaintainConnection` 加速切换仍未采用：
  当前本机 BLE 键鼠不在线，无法完成音频/鼠标对照；保留为 `deferred` 实验。
- 1.9.6 构建/安装验证：`无线麦 SayAll_1.9.6_x64-setup.exe` 安装成功，
  启动日志 `ver=1.9.6`；空闲 10 s 内没有任何 `voice_priority apply`，只有遥控器
  自身的 `conn_params 12/0/200`。

## 2026-11-24 定案（1.9.7 实验 → 1.9.8）

- 1.9.6 实机 16 次语音（sessions 46–61）：8 full / 8 degraded；7 次降级重试窗口
  中 3 次保留到下一次语音且全部 full，另外 4 次在 0.5–1.9 s 内被鼠标/键盘活动
  取消，下一次重新经历 2.1–3.4 s 的 6→48 切换。
- 1.9.7 实验：键鼠**常驻 ThroughputOptimized 15 ms**（日志 `12/0/200`），
  语音期间不再切 Balanced。sessions 64–69 共 6 次全部 full，
  12,154 / 12,844 / 12,928 / 12,569 / 12,643 / 12,663 samples/s；用户反馈
  鼠标空闲体感可接受，语音期间因音频共用适配器略慢，文字总体正常。
- 1.9.8 采用该方案：遥控器 Ready/Streaming/Draining 期间保持 15 ms；遥控器断开
  或应用退出才释放。删除 Balanced 切换、降级重试窗口和桌面键鼠活动判定。
- **残余/待观察**：12.1–12.9 k 低于 Balanced 满速 14.5–15.5 k，但高于 12 k full
  阈值；长语音、弱信号、其它适配器、RC001 与键鼠耗电为 `deferred`。

## 2026-11-24 单 HID 验证（1.9.9 测试 → 1.9.10）

- 1.9.9 先按“≥2 个非遥控器 BLE HID 才调整”做配置门控；只有鼠标（键盘关机）时
  策略 `observe hid_count=1`，不动鼠标。
- 实测 session 83（只有鼠标 + 遥控器，未调整）：**4,789 samples/s, degraded**。
  说明“只有鼠标就不受影响”的假设不成立；单 HID 也会显著争用 ATVV 音频。
- 1.9.9 的自动回退随即记录 `hold_single` 并对该鼠标开 15 ms；sessions 84–90
  恢复 full（12,476 / 13,085 / 12,676 / 12,631 / 13,069 / 12,573 等）。
  sessions 91–96 有 4 次在 11.6–11.9 k 的阈值边缘，仍为 `degraded` 但接近满速。
- **1.9.10 定案**：存在 ≥1 个非遥控器 BLE HID 就常驻 15 ms，0 个不操作。
  1.9.5 的 60 ms 空闲预热仍是鼠标卡顿主因，15 ms 是必要折中。
- **1.9.11 改为先测后调 + 持久化**：首次遇到某 HID 数量先不改参数并记录真实
  语音健康；`full` 永久 observe，`degraded` 永久 hold/15 ms。较低数量已 hold 时
  更高数量直接继承（更多 HID 不会减少争用）。决策文件
  `%LOCALAPPDATA%\SayAll\voice-priority-policy.json` 只含数量和决策，不含设备
  名/地址/路径。资源够的用户完全不操作鼠标；资源不够的用户首次语音仍可能丢首字。
- 本机已按 session 83 的实测结果预置 `{"1":"hold"}`，避免再次首测丢字；其它用户
  仍从空策略开始测量。
- 残余：单 HID 15 ms 的速率余量有限；进一步只能 60 ms、硬件隔离或手动低延迟模式，
  分别有鼠标卡顿/操作成本，`deferred`。

## 隐私检查

未包含蓝牙地址、HID 路径、设备名、用户名、语音内容或凭据；只记录序号、连接参数、速率、耗时和布尔结果。
