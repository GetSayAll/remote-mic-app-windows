# 2026-10-28 目标窗口不在前台时遥控语音键无反应（取证中）

- 发现日期：2026-10-28（用户现场日志，覆盖 10-27 01:15 ~ 10-28 05:53）
- 状态：**调查中**——已排除版本回归、已加取证探针；本机（Windows 11 26200 / 微信 4.0 / RC003）
  **未复现**，等客户机同构建取数定性
- 影响范围：`1.8.0` 预览包（`f3f5212`）**与** `0.8.6`（`3ec6656`）**同时**出现；
  Windows 10.0.26300 x64；RC003；微信输入法 + 语音工具=微信（左 Ctrl + 左 Win）
- 功能点：语音键按下 → 注入"按住说话快捷键" → 目标窗口的输入法开麦
  （`ble.rs` voice_attempt / `ime.rs` ensure_session_ime / `wetype_revive.rs` spawn_mic_verify）
- 现象：用户反馈"SayAll 窗口不在最前端时，遥控按语音键没有语音"

## 证据（同一台机、同一个遥控器）

判据统一取 `voice_verify ... mic=`（微信输入法开麦观测，`not_observed` 为确认未触发；
与钩子层 0xFC「WTYP」存活标记互为独立证据源，本次两侧同时为负，不是 #118 盲判）。

| 时段 | 版本 | 前台 | 按下 | `mic=observed` |
|---|---|---|---|---|
| 10-27 全天 | 0.8.6 | 微信 pid 21140 | 52 | 51 |
| 10-28 01:24–01:38 | 0.8.6 与 1.8.0 同段 | 微信 pid 15688 | 48 | **1** |
| 10-28 01:24–01:38 | 同上 | SayAll 自己 | 14 | 13 |
| 10-28 03:06–05:44 | 1.8.0 | 微信 pid 29388/22516/10276 | 30 | **30** |

- 01:24:00–01:24:29 的 0.8.6（pid 24032）与 01:24:51 起的 1.8.0（pid 21272）在**同一前台**
  下表现一致 ⇒ **不是 1.8.0 引入的回归**，回退版本不构成对照组。
- 失败会话的完整形态：`voice_attempt path=send_input synth=off` + `chord_press result=ok`
  + `audio_stream phase=started` + `wetype_check reacted=false evidence=none` +
  `voice_verify mic=not_observed`。`send_key_edges_spaced_with` 在 `sent==0` 时返回错误，
  因此也不是被 UIPI 整批拦掉。
- 自愈阶梯跑过（`wetype_revive result=ok`）仍无效；唯一救回的一次（01:26:50）是按住 ≥3 秒
  让阶梯 `RetryVoiceChord` 重放和弦后 `mic=observed`。
- 键宿主当时是活的：`key_host action=start result=passed`、`HOOK kind=f5 installed=1`，
  直到 01:38:06 应用退出才 `disconnected`。
- **F5 边沿与结果几乎一一对应**：`f5=0` 的 41 次按下 38 次 not_observed；
  `f5>3` 的 74 次里 73 次 observed。失败会话里抑制器一条 `suppressor_ask` 都没有，
  `suppressor_stats seen=` 在该时段冻结在 35。

## 根因

**已确认**：断点在"注入的和弦没有被目标窗口的输入法消费"，不在遥控、音频、注入或版本。

**仍未知（两个候选，需现场分开）**：

1. **目标窗口的输入法会话不是微信输入法**。`ime.rs` 模块头已写明平台事实（WeType 语音热键
   只在它是当前会话活动输入法时生效；Windows 按应用记忆输入法），而判据
   `sta_ensure_ime` 在**新建 STA 线程**上调 `ITfInputProcessorProfileMgr::GetActiveProfile`，
   读到的是本线程/本进程会话，看不见目标窗口 ⇒ `already_active` 可能是假阳性，
   于是"该切却没切"（`last_switch_age_ms=never` 就是这个形状）。
2. **遥控器语音键的键盘报告没进 OS**（或进了但被绕过）。failure 会话 `f5_edges=0`
   与结果强共变，而钩子宿主当时是活的。

## 修复

本次只加**取证**，不改语音路径（最小改动、无决策影响）：

- `key_suppressor.rs`：新增 `F5Snapshot` / `f5_snapshot()` / `f5_key_down_in_os()`
  （`GetAsyncKeyState` 高字节最高位），含 2 个单测。
- `ime.rs`：`ForegroundProbe` 增 `hkl`（`GetKeyboardLayout(前台线程)`）与
  `focus_class`（`GetGUIThreadInfo().hwndFocus` 类名哈希），含 1 个单测。
- `ble.rs`：`voice_attempt` 增 `fg_hkl=`、`fg_focus=`；按下前取 F5 基线。
- `wetype_revive.rs`：`voice_verify` 增 `f5_edges=`、`f5_leaked=`、`f5_key_down=`。

## 验证

- `passed`：`cargo fmt --all -- --check`；`cargo test -p sayall-windows --lib`
  **473 passed / 0 failed / 16 ignored**（含新增 3 个用例）。
- **本机复现：未复现**（2026-10-28 14:35–14:40，调试版 `8c878b67`，RC003，
  微信 4.0 `Weixin.exe` pid 29276 在前台）。A 组 4 次（微信前台）+ B 组 6 次（SayAll 前台）
  共 10 次按下，**10/10 `wetype_check reacted=true evidence=marker`**：
  - `f5_edges` 13（800ms）/ 46–64（2500ms），`f5_leaked=0`，`f5_key_down=false`
    ⇒ 遥控器键盘报告在流、全部被吞、OS 侧无残留键态，抑制链健康；
  - `suppressor_ask dir=down swallow=1` 每次按住都有；
  - C 组手按 F5（06:37:54 / 06:37:58 / 06:38:24 / 06:38:28 / 06:38:47）都落在
    `suppressor_ask dir=up swallow=0 hold=2` = 非会话期放行，符合设计 ⇒ 钩子活着。
- **判据修正（本次新发现）**：同一批 10 次按下 `mic=` **全程 `not_observed`**，而 marker 命中
  ⇒ 本机 WeType 不写 ConsentStore（issue #118 形态）。`mic=` 单独不得判失败，
  唯一判据是 `wetype_check reacted=`；`Testing/WindowsVoiceF5Probe.md` 已按此改写。
- **探针分辨率（本次新发现）**：`fg_hkl` 在本机微信 4.0 与 SayAll 上同为 `0x08040804`
  （zh-CN 语言 HKL 不区分 TSF 输入法），`fg_focus` 对 Chromium 系应用恒等于顶层窗口类名。
  因此"目标窗口用的是不是微信输入法"**不能**靠这两个字段，而要靠 `reacted` 是否命中。
- **环境差异（待现场确认）**：客户机前台窗口类 `dc007969` = `Chrome_WidgetWin_1`（微信 3.x），
  本机微信 4.0 = `493dd032`。若现场故障与"按应用记忆输入法"有关，微信大版本本身就可能是变量。
- `deferred`：客户机上用同一构建再跑一轮（`f5_edges` / `f5_key_down` / `reacted` 三处即可定性），
  以及 RC001 同矩阵。

## 隐私检查

日志字段只有会话号、枚举标记、进程号、类名哈希与 HKL 十六进制；不含窗口标题、
输入法明文名称、用户路径、设备身份或语音内容。
