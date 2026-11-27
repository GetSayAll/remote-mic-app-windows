# 拼多多商家版里映射的方向键/回车/字母无效（注入事件在到达全局钩子前消失）

- 发现日期：2026-11-26
- 状态：调查中（已补诊断日志；根因二选一待现场取证，未改注入行为）
- 影响范围：用户现场 1.9.7（`source_revision=98fd2a2…`）、Windows 11 build 22631、
  RC003（报告层接管 + 注入）、第三方 App「拼多多商家版」；RC001 未涉及
- 功能点：按键映射 → `SendInput` 注入（`button_mapping` → `SendInputRuntime::tap`）
- 现象：只有拼多多商家版里方向键/回车/字母无效；音量±、打开微信、语音可用；其他 App 正常
- 复现条件：用户现场；本机没有该客户端，未复现
- 正常预期：映射键注入到当前前台 App，并在该 App 内产生对应动作

## 证据

用户提供的诊断日志（`sayall-diagnostic.log`，2026-11-25T14:17Z–2026-11-26T12:04Z）：

- 遥控器边沿与映射正常：`[EDGE] usages=0x0050/0x004f/0x00f1…` → `map_edges` →
  `map_fire … action=shortcut chord=Left|Right|Enter|Backspace`。
- 每次注入都提交成功：`map_inject result=queued`（`SendInput` 返回数 = 计划数，否则会落
  `result=err`），键值正确：Enter `vk=0x0D scan=0x1C`、Left `vk=0x25 scan=0x4B`（含 E0）、
  Backspace `vk=0x08 scan=0x0E`。排除 Issue #195 那类"缺扫描码"。
- 注入回声按**前台窗口**分叉（按 10 秒内的前台读数归因）：拼多多窗口
  （`fg_class=4aafa526`，pids 28860/23148）120 次注入里 114 次 `echo_delta=0`；微信
  （`Qt51514QWindowIcon`）等其它窗口 150 次里 149 次 `echo_delta=2`。典型时间线：
  `12:01:13` 前台读数为该窗口，`12:01:15–18` 的 Left×7 / Right×2 / Backspace×5 全部
  `echo_delta=0`；`12:00:39` 前台为微信时同样的 Enter 是 `echo_delta=2 last_vk=0x0D
  last_scan=0x1C`。
- 用户人工验证（2026-11-26）：① 映射成字母的键在拼多多输入框里不出现文字；
  ② 遥控器音量±在该窗口能改系统音量；③ 物理键盘的 ↑↓←→ / Enter 在该窗口正常。
- 日志里唯一一次音量键注入测试（`12:01:09–10`）的前一次前台读数是微信
  （`12:00:36`），因此"音量键有效"目前只证明**别的窗口**里注入正常。

## 根因

已确认：

1. 捕获、手势识别、映射查表、`SendInput` 提交全部正常，键值与扫描码正确；
2. 该窗口前台时，注入事件在到达本应用全局 LL 钩子（按键宿主）之前就消失了——
   即被更早的钩子/更下层过滤，或根本没有投递到本会话的输入队列；
3. 事件消失与该窗口一一对应（换窗口立刻恢复回声），不是钩子宿主掉线
   （同分钟内微信窗口回声正常，宿主 `key_host action=start` 之后无重启）。

仍未知（需现场一次取证）：

- 目标进程是否以管理员身份运行（UIPI 会拦掉普通进程发往高完整性窗口的模拟输入；
  但 `SendInput` 会返回 0，而现场是提交成功，故暂不倾向该项）；
- 事件是被目标 App 自带的低级键盘钩子/安全模块吞掉（只拦带注入标记的键），
  还是被内核过滤驱动丢弃；
- "音量键有效"是否真实发生在该窗口内（若注入也被吞，则音量变化只能来自报告层
  未清干净的原始 HID 报告）。

## 本次改动（只加诊断，不改行为）

- `crates/sayall-windows/src/input_target.rs`（新增）：注入前只读采样前台/焦点/
  完整性/可见性；`map_target` 与 `input_env` 两行的纯格式化 + 单测；
  `class_token` 从 `ime` 迁入并 re-export（同一哈希，避免两份实现漂移）。
- `crates/sayall-windows/src/button_mapping.rs`：快捷键动作注入前落 `map_target`。
- `crates/sayall-windows/src/send_input_windows.rs` + `send_input.rs`：每次 `tap`
  落 `send_input action=tap … form/sent/win32_error`，并保留 `GetLastError`。
- `crates/sayall-windows/src/key_host.rs`：按键宿主启动后落 `input_env`
  （本进程完整性、`LowLevelHooksTimeout`、`SAYALL_DIAG_NO_HOOKS`、宿主 pid）。
- `crates/sayall-windows/src/key_gate.rs`：新增只读 `hook_calls_total()`。
- `LOGGING.md`：新增「按键映射注入目标与提交形态（2026-11-26）」判读说明。

## 验证

- `cargo fmt --all -- --check` passed。
- `cargo test -p sayall-windows --lib`：668 passed / 0 failed / 22 ignored。
- `cargo check -p sayall-windows-app --features runtime-simulation` passed。
- 新增单测覆盖：字段齐全、无前台窗口、类名哈希、注入形态三态、部分提交/后端失败、
  `input_env` 未设置超时与诊断开关取值。
- **真机 deferred**：需要在装有拼多多商家版的机器上复测并回传日志；判读口径
  `echo_delta=0` + `fg_integrity` + `send_input form` 三项一起看。

## 隐私检查

本文不含个人路径、蓝牙地址、设备身份、窗口标题明文（只写类名哈希）、输入文字或凭据。
