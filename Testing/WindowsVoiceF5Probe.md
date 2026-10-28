# 语音按下「没反应」现场取证手册（F5 边沿 / 目标窗口输入法）

## 测试对象

- **判定目标**：用户报障「SayAll 窗口不在最前端时，按遥控语音键没有语音」的真实断点。
  2026-10-28 的现场日志（`ver=1.8.0 source_revision=f3f5212` 与 `ver=0.8.6 3ec6656`）
  已排除"版本回归"：两版在同一前台（微信 pid 15688）下同时失败，前台切成 SayAll 自己时
  又同时恢复。剩下两个候选断点必须用真机分开：
  1. **目标窗口的输入法会话不是微信输入法**（应用侧 `ime_activation` 全程
     `outcome=already_active`、`last_switch_age_ms=never`，"该切却没切"）；
  2. **遥控器语音键的键盘报告（F5）没有进 OS**（失败会话里抑制器一条边沿都收不到，
     而按键宿主当时是活的）。
- **代码位置**：`crates/sayall-windows/src/ime.rs`（`foreground_probe`）、
  `key_suppressor.rs`（`f5_snapshot` / `f5_key_down_in_os`）、
  `wetype_revive.rs`（`spawn_mic_verify`）、`ble.rs`（`voice_attempt`）。
- **适用环境**：Windows 10 1809+ / Windows 11 x64；RC001 与 RC003 **各自单独记录**。
  任一型号缺环境即记 `deferred`。

## 前置条件

1. 装好本次构建（或 `cargo tauri dev` 跑起来），**增强捕获已接管**（连接页显示已启用）。
2. 微信输入法为默认输入法，SayAll「按住说话快捷键」= 左 Ctrl + 左 Win，
   语音输入工具 = 微信。
3. 打开记事本（或任意可输入窗口）放在旁边，作为"F5 泄漏"的可见阳性对照
   （F5 进系统时记事本会插入时间/日期或触发功能键）。
4. 准备一份干净的诊断日志：记下开始测试的时间点，之后只取这一段。

## 复现步骤（3 轮，每轮 3~5 按）

- **A 组（报障场景）**：焦点放到微信输入框 → 按住遥控语音键 1.5~2 秒 → 松开，重复 5 次。
  预期：微信出现语音条并出字。
- **B 组（对照）**：把 SayAll 主窗口切到前台，同样按 5 次。
  预期：同样出语音条。
- **C 组（反向对照，判钩子死活）**：焦点仍留在微信，**用键盘手按一次 F5**
  （物理键盘，不是遥控器）。这一步不验证功能，只回答"F5 抑制器这条链现在能不能收到键"。

每组之间不要重装、不要重启应用或助教；A/B 交错做（A 一次、B 一次）以排除"时间窗口"这种
混淆变量——10-28 那轮就是 A/B 在 3 秒内交替、结果同样交替，才把"版本"排除掉的。

## 观测点

默认诊断日志 `%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`。每次按下依次落：

```text
voice_attempt session=<n> tool=wechat path=send_input synth=off synth_confirmed=false
  fg_self=<bool> fg_pid=<n> fg_class=<hash> fg_hkl=<0x........> fg_focus=<hash|none>
  since_tool_select_ms=... since_hotkey_change_ms=... since_ime_switch_ms=...
ime_activation tool=wechat scope=voice_press outcome=<already_active|switched|...>
  ... fg_pid=<n> fg_class=<hash> fg_hkl=<0x........> fg_focus=<hash|none> ...
chord_press result=ok session=<n> gap_ms=80
wetype_check armed attempt=0 epoch=<n> baseline_available=true marker_baseline=<n>
voice_verify session=<n> tool=wechat marker=wetype at_ms=800 mic=<observed|not_observed|unknown>
  f5_edges=<n> f5_leaked=<n> f5_key_down=<true|false>
voice_verify session=<n> ... at_ms=2500 mic=... f5_edges=<n> f5_leaked=<n> f5_key_down=<...>
```

一次采样命令（PowerShell，取最近 15 分钟）：

```powershell
$log = "$env:LOCALAPPDATA\SayAll\Logs\sayall-diagnostic.log"
Get-Content $log -Tail 4000 |
  Select-String -Pattern 'voice_attempt|ime_activation|chord_press|wetype_check|voice_verify|suppressor_ask' |
  ForEach-Object { $_.Line -replace '^.*?(build_channel=unknown release_tag=unknown )','' }
```

## 判据口径（2026-10-28 14:36 本机实测后修正，**先读这一节**）

`mic=` **不能单独当失败判据**。本机 10/10 会话出现
`wetype_check reacted=true evidence=marker mic=not_observed`：微信输入法的钩子层存活标记
（0xFC + extra="WTYP"）命中，而 ConsentStore 开麦观测全程失明——这正是 issue #118 的形态
（WeType 2.1.4.6 起录音不再写 ConsentStore）。

因此**唯一的通过判据是合并裁决**，与 `wetype_revive::reaction_verdict` 同源（marker 为正面证据，
先于 mic 否决）：

| 读数 | 含义 |
|---|---|
| `wetype_check reacted=true evidence=marker` | **通过**：目标输入法确实消费了本次和弦（mic 说什么都不重要）|
| `wetype_check reacted=true evidence=mic` | 通过：目标输入法在开麦（marker 通道未命中，但有开麦证据）|
| `wetype_check reacted=false evidence=none` | **失败**：两个独立证据源都确认没被触发 |
| `wetype_check skipped reason=observation_unavailable` | 无法判定，不得据此下结论 |

`voice_verify` 的 `mic=` 只作为**辅助**：它 `observed` 时可以佐证；它 `not_observed`
而 `reacted=true` 时，说明这台机器的 WeType 不写 ConsentStore，不是功能失败。

## 字段判读（F5 一支；下表以"目标输入法没被触发"为前提）

| `f5_edges` | `f5_leaked` | `f5_key_down` | 读法 |
|---|---|---|---|
| 0 | 0 | **false** | **语音键的键盘报告根本没进 OS** ⇒ 查报告层/助手侧（agent 是否在改写该 usage），不是注入问题 |
| 0 | 0 | **true** | **F5 在 OS 里是按下的，但本进程钩子没看到** ⇒ 钩子/按键宿主链路被绕过（查 `key_host`、钩子链位） |
| >0 | **>0** | true | F5 泄漏进系统 ⇒ 微信输入法按"额外按键"拒绝和弦（`Bugs/2026-09-04-wetype-zero-gap-injection.md`）|
| >0 | 0 | false | 键盘报告与抑制都正常 ⇒ 断点在**目标窗口的输入法**（这支见下）|

健康基线的读数（2026-10-28 本机，RC003，微信 4.0 在前台）：
`at_ms=800 f5_edges=13 f5_leaked=0 f5_key_down=false`、
`at_ms=2500 f5_edges=46..64 f5_leaked=0 f5_key_down=false`。
`f5_key_down=false` 是**正常**的：F5 的按下沿被抑制器吞掉，OS 侧本来就不该有它的键态；
边沿数只增不减说明遥控器的键盘报告确实在流。

输入法一支的读法（注意 2026-10-28 实测的分辨率限制）：

- `ime_activation outcome=switched` 之后的下一次按下若 `reacted=true` ⇒ 确认"切一次就好"，
  自愈阶梯可以扩到"按下失败后对齐目标输入法"。
- `ime_activation` 全是 `already_active` 且 `last_switch_age_ms=never` ⇒ "该切却没切"成立
  （判据 `sta_ensure_ime` 在新建 STA 线程上读的是本进程会话，看不见目标窗口）。
- **`fg_hkl` 对"同语言的两个 TSF 输入法"没有分辨率**：本机微信 4.0 与 SayAll 都报
  `0x08040804`（= zh-CN 语言 HKL），微软拼音与微信输入法都是这个值。它只在跨语言
  （如中/英切换）或 IMM 型输入法时才可能不同。
- **`fg_focus` 对 Chromium 系应用没有分辨率**：微信（Weixin.exe，类名 `493dd032`）与旧版微信
  （`dc007969` = `Chrome_WidgetWin_1`）都会让 `hwndFocus` 等于顶层窗口本身，
  所以"焦点在输入框 / 在列表"区分不出来。它对 Win32 控件（Edit / RichEditD2DPT 等）有效。
- 结论：判"目标窗口输入法"不要指望这两个字段，**看 `reacted`** —— 它才是目标窗口消费了和弦
  的正面证据；不命中就是没消费。

## 通过 / 失败判据

- **passed**：A 组 5/5 `wetype_check reacted=true`（marker 或 mic 任一），`f5_edges` 随按住
  递增且 `f5_leaked=0`，C 组手按 F5 能在 `suppressor_ask dir=down` 里看到（非会话期应为
  `swallow=0 hold=2`，即放行进 OS——这是正确行为，不是缺陷）。
- **failed**：A 组出现 `reacted=false evidence=none`，并且 F5 一支能落到唯一一行
  （即断点被判据唯一确定）。这种情况下把该行连同 5 行上下文一起归档。
- **deferred**：C 组手按 F5 在抑制器里也看不到任何边沿 ⇒ 说明"钩子死了"，A 组结果不具
  诊断意义，先修链路再重测。

## 边界

- 本手册只做取证，不改语音路径；`f5_edges`/`f5_key_down` 是诊断读数，不参与任何决策。
- 仿真构建（`runtime-simulation`）无法产生真实 HID 报告与真实输入法会话，本手册结论
  **不能**由仿真或单元测试代替。
- `fg_hkl` 是十六进制 HKL，`fg_class`/`fg_focus` 是类名的稳定哈希：日志里不含明文窗口类名、
  窗口标题、输入法名称或用户路径。

## 关联材料

- `Bugs/2026-10-28-voice-no-response-when-target-behind.md`（本次现场与判读）
- `Bugs/2026-10-04-ll-hooks-break-in-app-ime-voice.md`（按键宿主进程的由来）
- `Bugs/2026-09-04-wetype-zero-gap-injection.md`（F5 泄漏 → 和弦被拒）
- `Testing/WindowsF5SuppressorBump.md`（抑制器相关既有实测手册）
