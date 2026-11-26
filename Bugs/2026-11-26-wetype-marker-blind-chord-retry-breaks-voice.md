# 微信输入法 3.x 的存活标记与 ConsentStore 同时失明，自动重发和弦把按住说话反复掐断

- 发现日期：2026-11-26
- 状态：已修复；自动化与本机探针 `passed`，原报障机器（WeType 3.0.3.x + RC001）端到端复测 `deferred`
- 影响范围：SayAll 1.9.3 / 1.9.7（1.9.8 同源代码）；微信输入法 3.0.3.x；Windows 11；RC001
- 功能点：语音键自动唤醒（`wetype_check` → `chord_retry`）
- 现象：按住遥控器语音键，微信输入法浮窗出现并开始转写，约 2～5 秒后自动结束、上屏，
  随即又自动重新开始，循环；松手前句子永远说不完整。用键盘按住左 Ctrl + 左 Win
  触发同一输入法则可以一直录音到松手。
- 复现条件：微信输入法 3.0.3.x（2026-11-24 升级内核后出现）；SayAll 使用默认
  左 Ctrl + 左 Win 按住说话快捷键；按住语音键说话。
- 正常预期：一次物理按住只触发一次听写，直到松手才结束；观测不可用不得导致重发按键。

## 证据

原报障机器的诊断日志（`%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`，2026-11-26 10:26）：

```text
10:26:12.117  chord_press result=ok session=9 gap_ms=80
10:26:12.118  wetype_check armed attempt=0 baseline_available=true marker_baseline=0
10:26:12.819  wetype_check reacted=false attempt=0 epoch=11 reviving
10:26:12.919  voice_verify session=9 marker=wetype at_ms=800 mic=not_observed verdict=not_reacted
10:26:14.620  voice_verify session=9 marker=wetype at_ms=2501 mic=not_observed verdict=not_reacted
10:26:15.043  chord_retry result=ok attempt=1 epoch=11
10:26:15.744  wetype_check reacted=false attempt=1 epoch=11 reviving
```

- `marker_baseline=0`：整个进程生命周期内一次 `0xFC`/`WTYP` 标记都没看到。
- `mic=not_observed`：ConsentStore 的微信条目也没有更新。
- 两条判据同时失明 → `NotReacted` → 每约 2.5 秒释放并重按一次和弦；微信输入法是
  “按住说话”，重按等于结束当前听写后立刻重开。

同机 2026-11-25 的正常日志在同一位置是：

```text
wetype_check reacted=true attempt=0 epoch=12 evidence=marker mic=not_observed marker_extra=0x57545950
```

即旧版本仍注入存活标记；升级到 3.0.3.x 后该标记消失，旧逻辑无法把“看不到”与
“确实没响应”区分开。

## 根因

`reaction_verdict` 原先按：

```text
marker 前进              → Reacted
ConsentStore Observed    → Reacted
ConsentStore NotObserved → NotReacted（执行恢复阶梯）
```

但微信输入法 3.x **既不注入存活标记，也不更新 ConsentStore**。`NotObserved` 只代表
“这条判据看不到”，不代表输入法没在录音。恢复阶梯因此在每次按住约 2.5 秒时把
正在进行的语音会话拆掉。

## 修复

新增第三条**与输入法版本解耦**的正面判据：枚举录音端点上的 WASAPI 会话，若任一
**Active** 会话属于微信输入法进程（进程在 WeType 安装目录下，或可执行文件名以
`wetype` 开头），即判 `Reacted`。

- 观测：`crates/sayall-windows/src/wetype_revive.rs` 的
  `wetype_capture_evidence()`（只读公开音频会话状态与进程可执行文件名；不含音频内容、
  路径或第三方私有数据）。
- 裁决：`reaction_verdict` 新增 `capture` / `capture_settled` 参数：
  - `Active` → `Reacted`，否决一切破坏性恢复；
  - 稳定采样点（2.5 s）的 `Inactive` → 允许 `NotReacted`，保留真休眠的自动恢复；
  - `Unavailable` → `Unknown`，**不重发按键、不切输入法**；
  - 其他输入工具仍走 `NotApplicable`，保持原有语义。
- 时序：`spawn_wetype_check` 在 700 ms 只记录（录音会话从按下到 Active 实测约 1 s，
  本机 1.02 s），2.5 s 稳定点才允许判未响应；`RetryVoiceChord` 执行前再取一次
  `capture`，若已 Active 则跳过。
- `voice_verify` 同步记录 `capture=`，盲判时 800 ms 为 `verdict=unknown`、2500 ms
  出现 Active 后为 `verdict=reacted`。

相关文件：

- `crates/sayall-windows/src/wetype_revive.rs`
- `crates/sayall-windows/src/ble.rs`
- `crates/sayall-windows/examples/capture_session_probe.rs`（本机只读探针）

## 验证

- 自动化 `passed`：`cargo test -p sayall-windows --lib` **639 passed / 22 ignored**；
  新增用例覆盖：录音会话正面证据、稳定点前 `Inactive` 不判未响应、`Unavailable`
  不许判未响应、录音会话不可用但 ConsentStore 可读时仍可恢复、非微信工具保持旧语义、
  进程名匹配大小写与路径边界。
- 本机探针 `passed`（WeType 3.0.2.44，Windows 11，2026-11-26）：
  - 注入并按住左 Ctrl + 左 Win 后，默认录音端点出现
    `active pid=… process=wetype_update.exe`，释放后回到 `inactive`；
  - 实际调用产品函数 `wetype_capture_evidence()` 在录音期间连续 11 次返回 `Active`；
    探针在释放和弦后观察到同一会话回到 `inactive`。
- 错误路径 `passed`（单测）：`CaptureEvidence::Unavailable` + `mic=not_observed`
  → `Unknown`，不会触发 `chord_retry`。
- 原报障机器真实 15 秒按住、冷/闲置首用、快速连按：`deferred`——需要在
  微信输入法 3.0.3.x + RC001 上复测；本机版本（3.0.2.44）无法复现标记失明。
- 日志判据：
  - 健康：`wetype_check pending ... capture=inactive`，随后
    `wetype_check reacted=true ... evidence=capture`，无 `chord_retry`；
  - 观测不可用：`wetype_check skipped reason=observation_unavailable ... capture=unavailable`，
    无 `chord_retry`；
  - 真休眠：2.5 s 稳定点 `capture=inactive` → `reviving` → 重试后恢复。

## 隐私检查

未记录蓝牙地址、HID 路径、语音内容、用户名或个人路径；诊断日志只新增
`capture=active|inactive|unavailable|not_applicable` 标记，不记录进程路径或音频内容。
