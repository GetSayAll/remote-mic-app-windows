# 2026-10-23 松手后豆包语音条不消失：报告层合成的右 Alt 释放报告丢失

- 发现日期：2026-10-23
- 状态：已修复（真机 passed：尾字不再丢，记事本字母提示不再出现；
  「自然卡住」路径与 RC001 仍 deferred）
- 影响范围：Windows 10/11；RC003 + 豆包输入法 + 报告层合成（`path=report_layer`）；
  RC001 未涉及
- 功能点：RC003 报告层语音键合成、ATVV 语音会话收尾、豆包语音条生命周期
- 现象：松开遥控器语音键后，文字已经上屏，但豆包语音条（波形条）继续显示；
  有时立即消失，有时持续数十秒，最长实测 66 秒才消失
- 复现条件：RC003 按住语音键约 1–2 秒后松开；豆包为当前输入法；
  报告层合成生效（`voice_attempt ... path=report_layer synth=0x00E6`）
- 正常预期：松手后 ATVV 会话结束，合成右 Alt 的 UP 到达 Windows，豆包语音条消失

## 证据（`%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`，UTC）

2026-10-23 卡住现场（本地 +08，括号内为 UTC）：

- `11:50:29.737`（`03:50:29.737Z`）`voice_attempt session=124 tool=doubao
  path=report_layer synth=0x00E6 synth_confirmed=true`；helper 同时
  `synth:frame replace ok to=0xe6`。
- `11:50:31.313`（`03:50:31.313Z`）收到 ATVV 停止帧
  `direction=C byte_count=2 preview=[00 02]`。
- `11:50:31.341` `audio_session generation=26 action=finish phase=completed
  terminal_result=passed submitted_samples=23280 queued_samples=0`。
- 独立键态探针（每 50 ms 读 `GetAsyncKeyState(VK_RMENU)`）：
  - `11:50:29.727` `down=True`（按下）
  - `11:50:31.313` ATVV 停止到达
  - **`VK_RMENU` 持续 `down=True` 直到 `11:51:37.220`**（停止后约 66 秒）
  - `11:51:38.472` 豆包 `OimeVoiceWaveWindow` 变为不可见
- 同期 helper 统计 `synth_hits` 每次按下只 +1，停止帧没有产生第二次
  `synth:frame`；说明语音键的 HID 释放报告没有到 Windows，而不是 helper
  拒绝了释放帧。

正常对照（同日 `12:00:02` / `12:00:05` / `12:00:07`，本地时间）：

- `VK_RMENU` 在 ATVV 停止前 0.5–0.9 秒抬起（HID 释放先到）；
- 豆包语音条在 `audio_session finish` 后约 1 秒消失；
- `voice_synth_guard` 预期会记录 `key_down=false action=none`（修复后）。

补充现场（同日 `11:48` 会话）：用户确认**文字已上屏，语音条仍留着**——
说明语音/转写链路与会话收尾正常，卡住的是报告层合成的按键状态。

窗口归属：卡住的条是 `ImeService.exe` 的 `OimeVoiceWaveWindow`
（豆包输入法自己的界面），不是 SayAll 的窗口。

## 根因

已确认：

1. 报告层合成把遥控器语音键 `0x003E` 槽内替换成右 Alt `0x00E6`，Windows
   的右 Alt 键态完全由后续 HID 报告驱动（状态语义）。
2. 遥控器松手时，ATVV 停止帧可靠到达，但**语音键的 HID 释放报告偶发没有
   到达 Windows**（是固件未发还是链路丢失，现有日志无法区分；需要 WUDF
   tap 才能进一步区分）。
3. 没有释放报告时，合成的右 Alt 停留在按下状态；豆包语音条跟随该键态，
   因此不消失。文字上屏走的是 ATVV 音频/云端转写链路，不受影响。
4. 豆包自身有时会在转写完成后清理该键态（正常样本约 1–5 秒消失）；
   清理没有发生时，遥控器固件的约 60 秒会话上限最终结束会话并释放键态
   （本次实测 66 秒，与 60 秒上限吻合）。

## 修复（应用侧，最小范围）

文件：`crates/sayall-windows/src/ble.rs`、
`crates/sayall-windows/src/send_input_windows.rs`

1. `PipelineOutput::StreamStopped` 收尾后启动 `spawn_voice_synth_guard`
   短线程：
   - 仅在报告层合成生效且目标工具为豆包时启用；
   - 等待 `VOICE_SYNTH_RELEASE_GRACE_MS = 250 ms`，让真实 HID 释放报告先到；
   - 读取 `GetAsyncKeyState(VK_RMENU)`；仍按下则用 `SendInput` 补发一次
     右 Alt UP；
   - 用 `voice_session_epoch` 检查：250 ms 内若已开始新会话则跳过，
     迟到补发不得打断新会话；
   - 结构化日志：
     - 正常：`voice_synth_guard generation=… epoch=… report_layer=true
       key_down=false action=none`
     - 卡住：`voice_synth_guard generation=… epoch=… key_down=true
       action=release terminal_result=passed|failed`
     - 换代：`action=skipped reason=session_replaced`
2. 纯判定 `should_release_stuck_voice_synth(report_layer_active, tool, key_down)`
   与单测，保证只在「报告层 + 豆包 + 右 Alt 仍按下」三条同时成立时补发。
3. `send_input_windows::right_alt_down()` 只读键态，不改系统设置。

## 验证

- `cargo fmt --all -- --check`：passed。
- `cargo test -p sayall-windows --lib`：**429 passed / 0 failed / 16 ignored**，
  含新增 `releases_only_when_report_layer_doubao_and_key_is_down`：passed。
- `cargo check --workspace`：passed。
- `cargo check -p sayall-windows-app --features runtime-simulation`：passed。
- 前端 `vitest run`：**470 passed / 10 skipped**；`vue-tsc --noEmit`：passed。
- 真机补发验证（2026-10-23 `12:09:17`，本地时间）：豆包语音条可见时用
  `SendInput` 补发一次右 Alt UP，`OimeVoiceWaveWindow` 在 `12:09:21.195`
  变为不可见，用户确认“条立即消失了”。这证明补发路径对豆包有效。
- 本地测试包：`target/release/bundle/nsis/无线麦 SayAll_1.0.0_x64-setup.exe`
  - SHA-256 `08C802CD8C7AA19D848A333E03A617D66D0E34545397E840CF43DB5CBF509D34`
  - 大小 `12,997,843` 字节；静默安装 passed；安装版内嵌
    `source_revision=411a0db…`，含新日志标记 `voice_synth_guard`。
  - 安装后 7 次正常语音会话：每次均出现
    `voice_synth_guard generation=… epoch=… report_layer=true key_down=false
    action=none`，豆包语音条均正常消失（用户确认“都正常消失”）。
- **deferred**：等待一次自然卡住的松手，确认日志出现
  `voice_synth_guard ... key_down=true action=release terminal_result=passed`
  且语音条立即消失；若自然卡住长期不复现，以“正常会话 `key_down=false` +
  补发路径真机验证”为通过边界。
- **deferred**：用 WUDF tap 区分“固件未发释放报告”与“BLE 链路丢失”。

## 追加现场：记事本访问键提示与尾字丢失（同日）

- 现象：Windows 11 记事本中松开语音键后，工具栏上方出现一排字母
  （F/E/V/H/L/B/I/G/O/P/S），随后的输入容易丢尾字。
- 字母来源：记事本的 WinUI 访问键提示，由合成的右 Alt 按下触发。
  真机 `GetGUIThreadInfo` 实测只返回 `flags=0x0/0x1`，**不置
  `GUI_INMENUMODE`**，所以不能用菜单标志检测，也不能靠补发 Esc 自动清除。
  用户明确表示字母本身可以先不管。
- 真正严重的是尾字：正常样本里合成右 Alt 的 UP 常常**早于**应用的
  `audio_session finish`（例：`12:44:45.064` UP vs `12:44:45.111`
  finish，早 47ms）。豆包在 UP 时停止收音，应用随后才把队列里的最后
  一段音频排空到虚拟声卡，尾音被截断。
- 修复（helper 侧）：语音键释放帧先保持 `RELEASE_HOLD_MS = 200`
  再放行。期间 OS 仍认为合成右 Alt 按下，豆包继续收音；应用完成尾音
  排空后，零报告才通过，豆包停止。日志
  `synth:frame release hold_ms=200`。
- 应用侧配合：`VOICE_SYNTH_RELEASE_GRACE_MS` 从 250ms 调到 300ms，
  晚于 helper 的 200ms 保持窗，避免应用保险抢先把右 Alt 抬起。
- 已回退的方案：用 `GUI_INMENUMODE` 检测并补发 Esc（`f7c2b82`）。
  真机 Notepad flags 实测 0x0/0x1，未触发；20ms Esc 还有干扰豆包
  转写的风险。
- 自动化：helper `cargo test` **76 passed**；agent 逻辑 **64/64**；
  应用 `cargo test -p sayall-windows --lib` **429 passed / 16 ignored**。
- 本地测试包（尾音保持）：`target/release/bundle/nsis/无线麦 SayAll_1.0.0_x64-setup.exe`
  - SHA-256 `92CD557AD49F3BA714C2A5DB5449F01CFB493BF3F92AB7C0C0081352C4F57A56`
  - 大小 `12,994,951` 字节；静默安装 passed；安装版内嵌
    `source_revision=ecdd49b…`。
- 2026-10-23 `12:58` 只重建 RC003 设备节点：`pnputil /remove-device` 与
  `/scan-devices` 均 passed（未动鼠标和蓝牙适配器）。但 RC003 HID 仍落在
  **共享宿主 `WUDFHost` 37164**（members=4，rc003_members=1）；helper 日志为
  `AGENT-STALE running=2026-10-05.gate-delay-units embedded=2026-10-23.release-hold`
  + `AGENT-REFRESH skipped reason=shared_host`，新 agent 尚未注入。
  helper 设计上不会结束共享宿主，因此**需要重启电脑**才能让 200ms 尾音
  保持生效。
- 2026-10-23 `16:18` 重启后复验（**客观时序 passed**）：
  - helper `[HELLO] ... host_pid=3956 ... build=2026-10-23.release-hold`，
    无 `AGENT-STALE`；新宿主已加载新 agent。
  - 多次输入（session 77–86）每次松手均出现
    `synth:frame release hold_ms=200`。
  - 例：session 80 `audio_session finish` `08:21:30.423`，release hold
    日志 `08:21:30.581`（UP 在 finish 后 158ms）；session 86 finish
    `08:22:03.604`，hold `08:22:03.808`（晚 204ms）。右 Alt UP 已稳定
    晚于音频排空，尾音保持链路生效。
- 2026-10-23 `16:2x` 用户确认（**主观 passed**）：
  - **没有丢字**：多次语音输入（session 77–86 及之后）未再丢尾字。
  - **记事本字母提示也不再出现**。该提示原为记事本 WinUI 访问键，
    本次没有单独用探针归因它为何消失（`GUI_INMENUMODE` + Esc 方案已
    回退，新包不含 Esc），只记录用户可观察结果。
- **deferred**：自然卡住的松手路径、RC001；字母提示消失的独立机理
  未单独归因。

## 隐私检查

未包含蓝牙地址、HID 路径、用户名、语音内容或凭据；仅使用相对日志时间、
稳定 token、窗口类名与计数。
