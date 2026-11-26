# 鼠标连续移动 PoC（issue #407）

- 日期：2026-11-26
- 来源：<https://github.com/GetSayAll/remote-mic-app-windows/issues/407>
- 分支：`feat/pointer-motion-poc-20261126`
- 范围：**PoC**。只接方向键的 `mouse_move` + 已开启「按住时连续执行」；
  不改 UI、不改 schema、不改默认配置；默认关闭。

## 结论

issue 的诊断成立：当前按住移动不是速度模型，而是「固定位移 + 固定节拍」——

- 每次 `mouse_move` 固定跳 `distance` 像素，直接 `SetPhysicalCursorPos`
  （`button_mapping.rs` 的 `MouseMove` 分支、`send_input_windows.rs::mouse_move`）。
- 按住只是每 100ms 再触发一次，起拍还要等 350ms
  （`raw_input.rs::repeat_interval`、`button_gestures.rs::REPEAT_START_DELAY`）。

因此典型手感是「按下先跳 20px → 停 350ms → 每 100ms 跳 20px」。调大
`distance` 只会更跳，不会更顺。

PoC 把按住过程改为**速度积分**：8ms 节拍、二次缓入曲线、亚像素余量、
松手立即停止；会话期间成对 `timeBeginPeriod(1)`/`timeEndPeriod(1)`。

## 启用方式

```powershell
$env:SAYALL_POINTER_MOTION = "1"
$env:SAYALL_GATT_LOG = "C:\path\to\gatt.log"
& "$env:LOCALAPPDATA\无线麦 SayAll\sayall-windows-app.exe"
```

- 必须是方向键（上/下/左/右）的 `mouse_move`。
- 必须已开启「按住时连续执行」（重复单击动作或重复长按动作，视配置槽位）。
- 关闭环境变量即完全回到旧行为；不写配置、不改 schema。
- 引擎启动时落一行
  `mouse_motion config source=poc enabled=... tick_ms=... ...`，日志里可直接确认
  本次是否启用。

## 参数（固定常量，PoC 阶段只按真机调）

| 项 | 值 | 含义 |
| --- | --- | --- |
| tick | 8ms | 目标节拍；实际间隔写进会话日志 |
| 初速 | `distance × 10 px/s` | `distance=20` → 200 px/s；100ms 短按约 20px |
| 精细窗口 | 500ms | 这段时间保持初速，给短按住留出可控的小幅定位 |
| 最大速度倍率 | 5× | 只在精细窗口之后加速；20px 默认档略快后从 6× 下调 |
| 最大速度上限 | 2000 px/s | 防止 `distance=100` 时飞出去 |
| 加速时长 | 700ms | 精细窗口结束后二次缓入到最大速度；总计 1.2s 达到最大 |
| 单次积分上限 | 32ms | 引擎被阻塞后不允许一次大跳补回来 |
| 单次按住上限 | 15s | 释放沿丢失时兜底停止 |
| 进度日志间隔 | 1s | 长按期间可定位 |

## 生命周期与收口

`PointerMotion` 只在引擎线程内使用，不持锁、不跨线程：

- 开始：`fire_gesture` 的 `MouseMove` 分支，仅当
  `continuous && is_direction_button(button)` 且 `begin` 成功；后续重复拍
  （同一键已有会话）直接忽略，不再走旧的 100ms 单步。
- 结束：释放沿在 `handle_edges` **任何 continue 之前**配对 `release`，
  先补最后一小段位移再停止；被切换器/方向键按住/常用语面板消费也不会漏。
- 统一收口：断连、监听停止、设备移除、退出、reset/reconfigure、
  睡眠/暂挂、语音键开始、门控线程消失、锁屏 terminal action 都会
  `release_all` 并落结束日志。
- 错误：注入失败立即结束会话，并在该键释放前抑制新会话，避免 100ms 一次的
  错误重试；释放后清除抑制。
- OK 上下文点击：每次成功 tick 和释放补位移都会续期 `last_remote_move_at`；
  断连/暂挂清空窗口。

## 计时器精度的实测

探针 `examples/pointer_motion_probe.rs` 的 `timer` 模式用与引擎相同的
`mpsc::recv_timeout` 量 8ms 节拍：

```text
timer mode=default samples=120 min_ms=10.99 avg_ms=15.60 p95_ms=16.46 max_ms=17.17
timer timeBeginPeriod result=0
timer mode=1ms samples=120 min_ms=8.01 avg_ms=8.56 p95_ms=9.37 max_ms=9.51
```

结论：默认节拍约 15.6ms（64Hz）且抖动；`timeBeginPeriod(1)` 后约 8.5ms
（117Hz）。因此会话期间必须提升计时器精度，最后一帧结束立即恢复；否则
「8ms 积分」实际会变成 15.6ms 一次的大步。

## 真实光标探针（两段曲线）

`move right 20 600`（真实移动光标，结束后恢复原位置）：

```text
motion start button=Right direction=Right distance=20 base_px_s=200.0 max_px_s=1000.0 fine_ms=500 accel_ms=700 tick_ms=8
motion end reason=probe duration_ms=603 ticks=73 moved_ticks=71 requested_px=121.0 observed_px=120.0 peak_px_s=216.1 tick_avg_ms=8.26 tick_p95_ms=8.64 tick_max_ms=8.72 dropped_ms=0 errors=0
cursor restored matches=true
```

`move right 20 1900`：

```text
motion start button=Right direction=Right distance=20 base_px_s=200.0 max_px_s=1000.0 fine_ms=500 accel_ms=700 tick_ms=8
motion progress duration_ms=1001 ticks=117 speed_px_s=603.0 requested_px=268.0 observed_px=263.0
motion end reason=probe duration_ms=1902 ticks=224 moved_ticks=222 requested_px=1129.0 observed_px=1124.0 peak_px_s=1000.0 tick_avg_ms=8.49 tick_p95_ms=8.67 tick_max_ms=26.76 dropped_ms=0 errors=0
cursor restored matches=true
```

`move right 30 1300`：

```text
motion start button=Right direction=Right distance=30 base_px_s=300.0 max_px_s=1500.0 fine_ms=500 accel_ms=700 tick_ms=8
motion progress duration_ms=1008 ticks=121 speed_px_s=922.1 requested_px=409.0 observed_px=409.0
motion end reason=probe duration_ms=1301 ticks=156 moved_ticks=154 requested_px=792.0 observed_px=792.0 peak_px_s=1500.0 tick_avg_ms=8.34 tick_p95_ms=8.65 tick_max_ms=9.09 dropped_ms=0 errors=0
cursor restored matches=true
```

`requested_px` 与 `observed_px` 基本一致，说明真实光标按积分位移移动；`tick_avg_ms`
约 8.3ms，说明 `timeBeginPeriod(1)` 生效。600ms 的 20px 档只移动 120px；
1900ms 的 20px 档移动 1129px（旧的单段曲线约 1328px）；1300ms 的 30px 档
移动 792px（旧的单段曲线约 1857px）。

## 2026-11-26 用户试按：单段曲线偏快

用户在安装版上实测旧单段曲线（`distance=30`，长按 600ms）：

- 1.309s 移动段：请求 1857px，实际 1857px，已接近 1920px 屏宽；
- 1.867s 移动段（`distance=20`）：请求 1907px，实际 1907px；
- 短按仍能小幅移动，但长按很容易过冲，精细定位不如旧行为。

结论：单段「500ms 加速到 6×」对精细控制太激进。先改为「500ms 精细窗口 +
700ms 加速」的两段曲线，长按 1.3s 从约 1857px 降到约 900px。用户复测两段
曲线后反馈「体验还行」，但随后指出默认 20px 档仍略快；最大倍率再从 6× 降到
5×，同一 1.3s 的 30px 档约 792px，20px 档 1.9s 约 1129px。该 5× 版本待用户
再次复测。是否默认开启、是否暴露设置仍待产品决定。

## 日志

只落会话级日志，逐帧 tick 不写（`gatt_note` 每次重开文件，8ms 级会刷爆）：

- `mouse_motion config source=poc enabled=...`：引擎启动一次，含 `fine_ms` /
  `accel_ms`。
- `mouse_motion phase=start source=poc button=... direction=... distance=...
  base_px_s=... max_px_s=... fine_ms=... accel_ms=... tick_ms=...`
- `mouse_motion timer=begin period_ms=1 result=ok|failed ...`
- `mouse_motion phase=progress ...`：每 1s 一行。
- `mouse_motion phase=end ... reason=release|restart|timeout|inject_error|
  suspended|reconfigure|shutdown|engine_stopped|gate_not_alive|terminal_action`
  + `duration_ms / ticks / requested_px / observed_px / peak_px_s /
  dropped_ms / errors / moved`。
- `mouse_motion timer=end period_ms=1 result=ok`：最后一个会话结束时。

## 验证

已在本机执行（2026-11-26）：

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo check -p sayall-windows-app --features runtime-simulation
cargo run -p sayall-windows --example pointer_motion_probe -- timer
cargo run -p sayall-windows --example pointer_motion_probe -- move right 20 600
cargo run -p sayall-windows --example pointer_motion_probe -- move right 30 1300
```

结果：

- `cargo fmt --all -- --check` passed。
- `cargo test --workspace` passed：`sayall-windows` 652 passed / 22 ignored，
  `sayall-windows-app` 234 passed / 2 ignored，其余 crate 全绿。
- `cargo check -p sayall-windows-app --features runtime-simulation` passed。
- 引擎接线用例 `button_mapping::tests::direction_mouse_move_ticks_until_release`
  passed：按住产生多帧 `mouse_move_by`、释放后停止、旧单步 `mouse_move` 未被调用。
- 单元测试覆盖：两段速度曲线、精细窗口、亚像素余量、短按、释放补位移、
  重复拍幂等、注入错误抑制、15s 超时、全部收口、计时器只提升一次并配对恢复。
- 真实光标探针 passed（见上）；光标位置已恢复。
- 用户试按数据已记录；两段曲线仍需 RC001 / RC003 真机复测。
- **RC001 / RC003 真机手感验收 deferred**：需要分别用实体方向键测试短按、
  长按、松手、冷态/闲置首按、断连、睡眠、退出；自动化结果不能替代。

## 未覆盖 / 待拍板

- **开关语义**：PoC 用现有「按住时连续执行」作闸门，不改变产品文档的三态；
  是否让 `mouse_move` 默认「按住即动」需要产品决定。
- **`distance` 语义**：PoC 继续表示「轻推一档」，100ms 短按约等于旧行为一次
  台阶；精细窗口、加速时长和倍率未暴露给用户，仍由固定常量决定。
- **两段曲线调参**：500ms 精细窗口和 700ms 加速是首版折中；若真机仍觉得
  长按过快，可继续延后加速或降低最大倍率，不改变用户配置。
- **双击/长按**：沿用现有触发点（无双击按下即动；有双击 300ms 确认后动；
  长按判定沿用当前设置，默认 550ms，本机用户配置 600ms），不新增时序。
- **非方向键**：其他按键的 `mouse_move` 仍走旧的 100ms 单步；是否扩展到全部
  按键需要另外评估。
- **斜向/双方向**：每个按键独立会话，同时按住两个方向会得到两轴合成；未做
  归一化，也未做专门的斜向手感调参。
- **原生泄漏**：基础路径的方向键首按可能漏进 OS（既有泄漏对冲边界），
  PoC 未改变该行为。
- **计时器影响**：`timeBeginPeriod(1)` 会在按住期间提高系统计时器精度，
  增加少量功耗；已按会话成对恢复，异常路径由 `Drop` 兜底。
