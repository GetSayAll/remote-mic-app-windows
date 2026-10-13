# 两支遥控器在 Windows 里同名：按错一支时界面毫无反应（②步按键行不亮）

- 发现日期：2026-10-13（Andy 真机截图 + 本机诊断日志；本机现场）
- 状态：已修复（自动化 passed；真机复核 `deferred`）
- 影响范围：0.8.6 起、本机两支遥控器（同为 RC 系列的 `…D9DB` / `…B76B`）同时配对时；
  Raw Input 侧"按「应用正在用的那支」过滤"（#249）之后表现最明显
- 功能点：Onboarding 第②步的按键检测 + Raw Input 路径过滤 + 连接后名称显示

## 现象与时间线（本机日志，UTC）

- `10:12:39` 用户进入②步；`10:13:05` 重新订阅按键边沿（`button_observation passed`）。
- `10:13:23` Raw Input 监听器启动：`matched_device_count=1`，绑定 `…c05d39c2d9db…`。
- `10:13:30` `stale_remote_event first_seen … bound_path=…c05d39c2d9db… event_path=…e85540b1b76b…`
  —— 另一支在发按键，按"非目标"丢弃。
- `10:13:45` 用户点设备行「连接」：`remote_target action=published` +
  **`ble_switch action=disconnect_previous terminal_result=passed`**（换机先断开旧的那支，本轮新逻辑）
  → 连上另一支（目标文件 `rc003-target.ini` 写入 `…e85540b1b76b`）。
- `10:13:53` 绑定审计按目标地址改绑到 `…e85540b1b76b…`（`action=rebound kept_previous=false`）。
- 用户"两支都按了"：`10:14:23` 起心跳仍为 `remote.button_not_ready detail=obs_0_n2`，
  整段没有 `map_edges`、`semantic_edges` 不涨。

## 根因

1. **两支遥控器的 Windows 名字完全相同**（截图里两行都叫「小米蓝牙语音遥控器」，没有 2 / 2 Pro），
   用户无法判断该点哪一行；他连上的那支（`…B76B`）不是手里按的那支（`…D9DB`）。
2. #249 之后按键按「应用正在用的那支」过滤是**有意的**（避免按住 A、程序响应 B），
   但被过滤掉的报文只进 `stale_remote_event_count`、界面上**没有任何解释** ⇒ 用户看到的就是
   "两条遥控器都按了，都没反应"。
3. 顺带暴露的一个 Raw Input 脆弱点：同一支遥控器的 HID 接口换过实例后，旧实例可能仍留在枚举里，
   绑定审计因此不重绑，而报文从新实例到达 —— 这类"同地址、不同实例路径"的报文此前同样被丢。

## 修复

- **② 步显示具体型号**（`RemoteStep.vue`）：已连接那一行的标题、以及下方「… · 已连接」一行，
  优先显示运行时按 GATT 读回的型号（小米蓝牙语音遥控器 2 / 2 Pro），型号未知才退回系统名字。
- **按到另一支时给一句解释**（`OnboardingPage.vue` + `RemoteStep.vue`）：等待本步普通按键期间，
  Raw Input 的 `stale_remote_event_count` **在本步内增长** = 另一支在发按键 ⇒ 就地提示
  「刚收到的是另一支遥控器的按键。当前连接的是「…」。请按这一支的普通按键；要用另一支，
  就点它那一行的「连接」。」（未连接时提示"先点一行「连接」选中你手上这一支"）；
  整步只提示一次，并落日志 `reportStepAction("remote", "other_remote_button", "passed")`。
- **同地址的其他实例路径也接受**（`raw_input_windows.rs`）：报文路径等于绑定路径，**或**属于
  当前目标地址（`remote_target::matches_address`）即处理；首次走这条兜底路径记一条
  `raw_input target_path_accept first_seen path_matches_bound=false`（无设备身份）。

## 验证

- 自动化 passed：OnboardingPage 50 passed（新增 2 例：型号显示、按到另一支的提示与一次性日志）、
  全前端 331 passed | 11 skipped、`vue-tsc` + `vite build`、`cargo fmt --all -- --check`、
  `cargo test -p sayall-windows --lib` 337 passed。
- **deferred（真机）**：两支同型号遥控器同时在线时——连接甲支后按甲支应正常亮灯；按乙支时应出现
  「另一支遥控器」提示；点乙支那行「连接」后（`ble_switch` 日志）改按乙支应正常。需 RC001/RC003
  分别确认。

## 隐私

- 未把任何蓝牙地址写进日志、文案、诊断块或仓库；本记录中的地址片段来自用户本机排查时的日志摘录，
  入库时只保留可区分的前后缀（`…D9DB` / `…B76B`）。
