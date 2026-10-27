# 语音识别结果与内容出入较大（输入法麦克风设为「自动检测」选错录音设备）

## 现象

- 遥控器语音键正常触发，豆包（`tool=doubao`）能出字，但**识别内容与用户实际说的出入较大**；
- 用户主观感受是"准确率比以前低"，第一反应怀疑新版本回归。

## 现场证据（2026-10-27 18:37，`source_revision=f991d49`）

链路侧全部正常，没有任何失败：

```
voice_attempt session=127 tool=doubao path=report_layer synth=0x00E6 synth_confirmed=true
voice_effect session=127 result=replaced window_ms=500
transcript_recording action=finish phase=completed result=passed duration_ms=1935 bytes=61964
```

- 4 次会话全部 `path=report_layer` + `result=replaced`：报告层合成生效，按键链路没有问题；
- 录音时长 3.2–17.7 s，没有截断；
- `voice_verify … mic=not_observed` **不能作为"没开麦"的证据**：该判据读
  `CapabilityAccessManager\ConsentStore\microphone\NonPackaged`，在本机 10-23 / 10-24 / 10-25
  三份历史日志里 `mic=observed` 计数**都是 0**，即它一直不工作。

## 根因

输入法的麦克风选的是**「自动检测」**，它跟随系统默认录音设备；本机默认录音设备不是
`CABLE Output`。系统里同时活跃三个录音端点：

| 端点 | 来源 |
| --- | --- |
| `CABLE Output` | VB-Audio Virtual Cable（SayAll 写进去的那一路） |
| `立体声混音` | Realtek，录到的是系统播放的声音 |
| `麦克风阵列` | Realtek 内置麦克风 |

于是输入法实际录到的是环境声/系统声，而不是遥控器送进虚拟线的那份音频——表现为
"能出字但内容对不上"，而不是"完全没有结果"。

**用户把输入法麦克风从「自动检测」改成 `CABLE Output` 后，准确率立刻恢复正常**（2026-10-27 验证）。

## 处置

这不是应用代码缺陷，是配置问题。代码侧只补"下次一眼定位"的证据（2026-10-27）：

1. `voice_capture_input default="<默认录音设备名>" is_cable_output=<bool>`（每次语音键按下，
   结果缓存 30 s）——默认设备不是 CABLE Output 时，这一行直接指出问题；
2. `voice_level session=… reason=stopped frames=… samples=… peak_dbfs=… rms_dbfs=… clipped=…`
   （每次会话收尾）——用来区分"声音太小 / 削波 / 音频根本没进来"，
   只记数字，不碰音频内容。

产品侧仍待定（需要 Andy 决定，本次未改）：连接页在「语音设备」卡片旁边提示
「把输入法的麦克风选成 CABLE Output，不要用自动检测」，或检测到默认录音设备不是
CABLE Output 时给一句提示。

## 验证

- 自动化 `passed`：`cargo test -p sayall-windows --lib`（新增 4 条电平边界用例 + 1 条端点名判定用例）；
- 真机 `deferred`：需要一次真实语音会话确认两行日志都出现，且 `is_cable_output=true`。

## 隐私检查

本文只引用端点名、日志字段与时长，未包含语音内容、输入框文本、个人路径或令牌。
