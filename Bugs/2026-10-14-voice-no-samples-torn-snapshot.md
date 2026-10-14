# ⑤步「收到了语音会话，但没有声音数据」误判：跨子系统撕裂快照被当成"开始→立即结束"

- 发现日期：2026-10-14 凌晨（Andy 截图："已经上屏了，而且有语音条，怎么还要声音数据"）
- 状态：已修复（跟踪器语义 + 2 条回归用例）；**真机复核 deferred**
- 影响范围：向导第⑤步按住说话验证；任何"会话开始恰好落在一次快照读取的两段之间"的会话都可能中；
  现场构建 `c3b884f`（当日本地测试包）
- 功能点：`src/onboarding/voice-attempt.ts`（attempt 跟踪器）；触发源是
  `crates/sayall-windows/src/lib.rs` `PlatformSnapshot::snapshot()` 的分段读取

## 现象

输入框里已经有识别文字（"12345"）、工具的语音条也出现过，但结果卡是
「收到了语音会话，但没有声音数据。对着遥控器正常说话，再试一次。」——判定码 `voice.no_samples`。

## 证据（`sayall-diagnostic.log`，2026-10-13T16:56，UTC）

```
16:56:39.766 frontend … voice_attempt result=unknown reason=armed step=voice_test detail=id_1
16:56:46.144 gatt packet direction=C preview=[04 03 02 43]        （语音键按下）
16:56:46.168 voice_attempt session=67 … path=report_layer synth_confirmed=true
16:56:46.170 audio_session generation=1 action=begin phase=completed
16:56:46.172 frontend … voice_attempt result=failed reason=voice.no_samples detail=d0_s0_q0_drain0
16:56:46.317 audio_stream phase=started
16:56:46.969 voice_verify session=67 marker=doubao at_ms=800 mic=observed
16:56:47.846 gatt packet preview=[00 02]                            （松开）
16:56:47.847 audio_session generation=1 action=finish submitted_samples=24720
```

- 失败判在**会话开始后 4 ms**（`elapsed_ms=6406` = 自 arm 起算），而该会话实际投递 **24720 采样**
  （≈1.5 秒语音）并成功上屏——判定用的是 0/0 计数。
- `detail=d0_s0_q0_drain0` 反推唯一能产生该终态的帧形态：`voiceState=idle` 且
  `audioPhase=streaming`（drain0 要求 `audioPhase !== "ready"`）。其余组合要么不进入"结束"分支、
  要么落在别的失败码上。

## 根因

1. `PlatformSnapshot::snapshot()` **按子系统依次读取**（先 `connection_snapshot()`、后
   `audio_snapshot()`，各自独立锁）。真实会话开始（BLE 线程同时把 voice 置 Streaming、
   audio 置 Streaming）恰好落在两次读取之间时，产出撕裂帧 `{voiceState: idle, audioPhase: streaming}`。
2. 跟踪器在 `waiting_start` 里用 `!idleFrame`（语音非 idle **或** 音频 streaming/draining）
   作为"会话已开始"，并在同一帧继续走到"回到 idle = 结束"的分类——于是撕裂帧被当成
   "立即开始又立即结束"，用 0/0 计数判 `no_samples`。
3. 会话是否开始的本体是 **`connection.voiceState`**；音频相只是下游证据（文档注释本来就写着
   "开始 = 观测到非 idle 的语音状态"，实现偏离了这条）。

## 修复

- `voice-attempt.ts`：`waiting_start` 中新增守卫——`voiceState === "idle"` 而音频忙的帧**不视为开始**，
  继续等待（解码计数照常累计；会话若真的开始，后续帧的 `voiceState !== "idle"` 会正常推进状态机；
  极短按压仍走"解码差值 → waiting_end"路径）。
  撕裂帧的两种方向都不再有危害：`{idle, streaming}` → 守卫忽略；`{streaming, ready}` → 正常进入 streaming。
- 回归用例 2 条：`撕裂快照（音频忙、语音仍 idle）不当作会话开始`（终态应为通过、startObserved=true）、
  `音频忙帧之后，真实空会话仍判 no_samples`（守卫不过度放宽）。

## 验证

- `vitest`：`voice-attempt.test.ts` + `OnboardingPage.test.ts` **76 passed**；全量 349 passed（含新增）。
- **deferred（真机）**：正常语音会话（尤其"刚打开应用直接落在⑤步后的第一次按下"）应判通过、
  文字上屏；真·空会话（按了键但没说话）仍应给 `voice.no_samples`。

## 隐私检查

- 本文只含时间戳、计数与判定码；不含设备地址、语音内容、个人路径。
