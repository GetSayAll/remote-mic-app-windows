# 语音优先：请求成功但键鼠实际 2.4–4 s 后才切到 Balanced，短会话丢首字

- 发现日期：2026-11-23
- 状态：已修复（1.9.5 空闲预热；用户 6 次复测中 5 次 full，session 62 为切换未完成的残余边界）
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

## 隐私检查

未包含蓝牙地址、HID 路径、设备名、用户名、语音内容或凭据；只记录序号、
连接参数、速率、耗时和布尔结果。
