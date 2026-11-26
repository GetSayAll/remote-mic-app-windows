# 微信输入法录音会话判据（3.x 标记失明）真机测试手册

## 测试对象

- 修复记录：`Bugs/2026-11-26-wetype-marker-blind-chord-retry-breaks-voice.md`
- 适用：微信输入法 3.0.3.x（或其他不再注入 `0xFC`/`WTYP`、不写 ConsentStore 的版本）
- 遥控器：RC001 必测；RC003 另行记录
- 预期：一次物理按住只启动一次听写，直到松手才结束；不得出现约 2.5 秒一次的
  “断 → 自动重开”。

## 安装前准备

1. 记录 Windows 版本、微信输入法版本（`wetype_server.exe` 文件版本）、SayAll 版本、
   遥控器型号。
2. 确认 SayAll 的按住说话快捷键为默认 左 Ctrl + 左 Win，微信输入法语音热键一致。
3. 确认微信输入法麦克风为「自动检测」或与系统默认录音设备一致；系统默认录音设备
   应为 CABLE Output（语音识别准确性前提，不是本修复的判据）。
4. 正常退出旧版 SayAll，安装被测本地包，启动并连接遥控器。

## 用例一：长按不再被自动掐断

1. 在可输入文本的窗口中激活微信输入法。
2. 按住遥控器语音键，连续说话 **15 秒**，然后松开。
3. 重复 3 次，每次间隔至少 5 秒。

预期：

- 每次只出现一次微信输入法浮窗；按住期间不结束、不重新弹出。
- 日志出现 `wetype_check pending ... capture=inactive`（或首次采样直接
  `reacted=true evidence=capture`），随后 `wetype_check reacted=true ... evidence=capture`。
- 同一会话内没有 `wetype_check reacted=false ... reviving`，也没有
  `chord_retry result=ok`。
- `voice_verify` 在 `at_ms=800` 可为 `verdict=unknown capture=inactive`（录音会话
  尚未出现），`at_ms=2500` 应为 `verdict=reacted capture=active`。

## 用例二：闲置后首用

1. SayAll、微信输入法、遥控器保持连接，至少 10 分钟不按语音键。
2. 第一次按住并连续说话 15 秒。

预期：与用例一相同；冷启动不得出现 `chord_retry`。

## 用例三：真休眠仍能自动恢复（回归）

只有自然出现“微信输入法热键休眠”（按住无浮窗、无转写）时才执行本用例，不得为了
制造休眠而修改第三方配置或强杀第三方进程。

1. 出现休眠时按住语音键。
2. 观察日志与界面。

预期：

- 2.5 秒稳定点仍为 `capture=inactive`（确认没有录音会话）后，才允许
  `wetype_check reacted=false ... capture=inactive reviving` 与后续 `chord_retry`。
- 恢复后出现浮窗与文字；若三轮仍未响应，显示既有的人工提示。

## 用例四：观测不可用（失败安全）

本机若无法枚举录音会话（COM/端点异常），日志应为：

```text
wetype_check skipped reason=observation_unavailable ... capture=unavailable
```

预期：**不重发按键、不切输入法**；如果本次语音确实没有开始，用户松开后重按即可，
不得出现反复重开。

## 用例五：快速连按与配对

1. 连续完成 20 次约 1 秒的按住、说话、释放。
2. 再完成 3 次 10～15 秒的按住。

预期：DOWN/UP 严格成对，无粘键、无漏释放、无跨会话重试；每次听写只对应一次物理按住。

## 日志判据

日志：`%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`。

健康（3.x 标记失明场景）：

```text
wetype_check armed attempt=0 ... marker_baseline=0
wetype_check pending attempt=0 ... mic=not_observed capture=inactive note=wait_capture_settle
wetype_check reacted=true attempt=0 ... evidence=capture mic=not_observed capture=active marker_extra=0x0
voice_verify session=... at_ms=800 mic=not_observed capture=inactive verdict=unknown
voice_verify session=... at_ms=2501 mic=not_observed capture=active verdict=reacted
```

同一会话内不得出现：

```text
wetype_check reacted=false ... reviving
chord_retry result=ok ...
```

真休眠（允许出现）：

```text
wetype_check reacted=false ... capture=inactive reviving
wetype_revive result=ok ...
chord_retry result=ok ...
```

观测不可用（失败安全，允许出现且不得跟随 `chord_retry`）：

```text
wetype_check skipped reason=observation_unavailable ... capture=unavailable
```

提交日志前删除个人路径、设备身份、语音内容与转写正文。

## 2026-11-26 原报障机器复测结果

环境：微信输入法 3.0.3.x、RC001、SayAll 1.9.8（`source_revision=44c6a33`）。
日志：`sayall-diagnostic(11).log`（UTC 2026-11-26 03:50–04:43）。

| 指标 | 1.9.7 | 1.9.8 |
|---|---:|---:|
| `wetype_check armed` | 30 | 5 |
| `reacted=true` | 0 | 5 |
| `reacted=false` | 27 | 0 |
| `chord_retry result=ok` | 11 | 0 |
| `wetype_revive` | 27 | 0 |
| `voice_heal` | 51 | 0 |
| `voice_verify` | 38 × `not_reacted` | 9 × `reacted` + 1 × `unknown`（会话已结束） |

5 次会话时长 2.3 / 5.4 / 4.5 / 11.3 / 6.4 秒，全部 `voice_audio_rate result=full`；
每次均为 `marker_baseline=0`、`marker_extra=0x0`，`evidence=capture capture=active`。
结论：**长按不再被自动掐断，passed**；15 秒整、闲置 10 分钟后首用、自然真休眠恢复与
RC003 未在该日志覆盖，仍为 `deferred`。

## 结果记录模板

```text
Source commit:
Windows / WeType / SayAll:
Remote: RC001 | RC003
长按 15 秒 × 3: passed | failed
闲置 10 分钟后首用: passed | failed | deferred
真休眠自动恢复（自然出现）: passed | failed | deferred
观测不可用失败安全: passed | failed | deferred
快速连按 20 次: passed | failed
失败发生时间:
对应日志片段:
```

只有实际执行并观察满足预期才记录 `passed`；没有自然出现真休眠或观测不可用场景时，
对应项记录 `deferred`，不得用其他项目的结果替代。
