# ⑤步手动输入误报：截图热键的三个按下沿被判成「键盘打字」

- 发现日期：2026-10-13（Andy 截图现场；同一轮还报告「文字已上屏却提示输入框没有聚焦」，见
  docs/product-copy.md §7 第十三轮条目）
- 状态：已修复（代码 + 单测）；**真机复核 deferred**（等新包在同一入口复测）
- 影响范围：Windows 11（10.0.26200）；RC003 2 Pro；豆包输入法；现场构建 = 分支 `fix/connected-row-token`
  的本地测试包（提交 `ab144e9` 前一版语义，修复在本记录对应的提交）
- 功能点：向导第⑤步「手动输入检测」（物理键观察窗口 / 探针④）；后端
  `crates/sayall-windows/src/key_gate.rs` + `src-tauri` 命令 `begin/end_key_observation`

## 现象

一次**成功上屏**的语音会话被判失败：实时检查里「语音键已按下并松开」「语音声音已收到」为绿、
输入框里已经出现识别文字，但结果卡写「检测到键盘输入。这一步请只用遥控器语音键，不要用键盘打字。」。
用户明确表示没有用键盘往输入框打字。

## 复现条件（现场形态）

⑤步停留期间触发任意"带 Ctrl/Alt/Win 修饰的系统或工具热键"（现场是截图热键 Win+Shift+S：
Win、Shift、S 三个按下沿），随后完成一次正常语音会话。

## 正常预期

正常路径 `input_observation feature=physical_keys action=end … count=0`（验收手册用例三.2 原话）；
只有真实键盘打字才判 `voice.manual_input`（验收手册用例三.4 阴性对照）。

## 证据（本机 `sayall-diagnostic.log`，2026-10-13，UTC）

```
15:53:15.966Z input_observation … action=begin window=1 exclude_count=1
15:53:43.397Z voice_attempt session=59 tool=doubao path=report_layer synth=0x00E6 synth_confirmed=true
15:53:44.863Z (voice key released)  → 15:53:44.891Z audio_session finish passed
15:53:44.971Z input_observation … action=end window=1 count=3 last_vk=0x53
15:53:44.973Z frontend event=onboarding phase=voice_attempt result=failed reason=voice.manual_input
```

- `count=3`、`last_vk=0x53`（VK 'S'）与截图热键 Win+Shift+S 的三个按下沿
  （0x5B Win、0xA0 左 Shift、0x53 S）完全吻合；同一窗口其余按键计数为 0（排除集里的合成右 Alt 生效）。
- 观察窗口从⑤步进入开到会话结算关（约 29 秒），期间只要发生一次系统/工具热键就会污染判定。
- 旧实现只留 `last_vk` 一个值，无法复现"是哪几个键"，本记录的另一项修复即是补上按键列表。

## 根因

观察窗口计入口径过宽：**"非注入 + 未被门控吞下 + 不在排除集"的任意键盘按下沿**都计数，不区分

1. 键本身能不能产生文字（修饰键、功能键、方向键、媒体键都算进去了）；
2. 按下当下有没有 Ctrl/Alt/Win 按住（有 = 系统/工具热键，不是"往输入框打字"）。

于是"用截图热键截个图"这种与打字无关的操作，也会把一次成功的语音会话判死。

## 修复（最小改动）

- `crates/sayall-windows/src/key_gate.rs`：计入判据收窄为两问都过才计数——
  ① `text_capable_vk`：字母 / 数字 / 空格 / 回车 / 退格 / 制表 / 删除 / 小键盘 / 常用符号键；
  ② `HELD_HOTKEY_MODS == 0`：新增"非 Shift 修饰键（Ctrl/Alt/Win）当前按住掩码"，由透传路径上
  每条**非注入**键盘事件成对维护（DOWN 置位 / UP 清位、开窗清零）。Shift 刻意不在掩码里：
  打大写字母要用它，阴性对照不得放宽。
- 诊断增强：`begin/end_key_observation` 增加 `keys=`（窗口内计入的按键码列表，最多 12 个，
  `OBSERVATION_KEYS` 原子槽）——"一次用户报障 + 一次日志拉取 = 定位到具体环节"。
- 影响边界：注入事件（自家 SendInput 映射/和弦注入）照旧在入口提前返回；被吞键照旧不计；
  排除集（`exclude_vks`，报告层合成和弦）机制不变。

## 验证

- `cargo test -p sayall-windows --lib key_gate`：4 条观察窗口用例通过，其中 2 条为新回归——
  「热键组合与修饰键不计数（Win+Shift+S 形态 → count=0；放开 Win 后 S → count=1；Shift+字母 → count=1；
  F5/方向键/裸 PrintScreen → 0；掩码随窗清零）」与「计入键列表按顺序记录」。
- `cargo test --workspace` passed（sayall-windows 348 passed | 13 ignored；app 110 passed）。
- `cargo check -p sayall-windows-app --features runtime-simulation` passed；`cargo fmt --all` 已跑。
- **deferred（真机）**：装新包在⑤步先按一次截图热键、再走一次正常语音会话——应通过且日志
  `action=end count=0 keys=none`；阴性对照（直接键盘打字）仍应判失败并可重试。

## 隐私检查

- 本记录与新增日志只含事件名、计数与虚拟键码；不含设备地址、HID 路径、音频端点、个人路径、
  任何语音内容或识别文字。
