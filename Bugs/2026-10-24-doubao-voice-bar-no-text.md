# 2026-10-24 豆包语音条出现但无文字上屏：ATVV 音频只有满速的约 1/3

- 发现日期：2026-10-24
- 状态：已修复提示（应用侧检测 + 连接页/向导提示）；规避验证 passed；真机提示复现 deferred
- 影响范围：1.0.0、`source_revision=c0d79f6…`、Windows 11、RC003 2 Pro、豆包输入法
- 功能点：RC003 语音键 → 报告层右 Alt 合成 → ATVV 音频 → CABLE Output → 豆包转写 → 文字上屏
- 现象：按住遥控器语音键后豆包语音条出现，松开后没有任何文字上屏
- 复现条件：报告层合成生效（`voice_attempt … path=report_layer synth=0x00E6 synth_confirmed=true`）；
  2026-10-24 19:00（本地）之后的每一次语音会话均复现；与目标应用无关
- 正常预期：松手后豆包转写出的文字进入当前输入框

## 证据

### 1. 语音键与豆包唤起链路正常

2026-10-24 19:30:31（本地）会话 105：

```text
voice_attempt session=105 tool=doubao path=report_layer synth=0x00E6 synth_confirmed=true
voice_effect session=105 result=replaced window_ms=500
```

helper 同时记录 `synth:frame replace ok to=0xe6`。说明语音键已在报告层替换为右 Alt，
豆包语音条被唤起；问题不在按键、焦点或「全按键支持」开关。

### 2. 音频样本率只有 16 kHz 满速的约 1/3

`audio_session generation=N action=finish … submitted_samples=` 与同一会话
`action=begin` 的时间差可算出实际送入 WASAPI 的样本率。当天会话对照：

| 本地时段 | 会话 | 样本率 |
| --- | --- | --- |
| 16:36–18:54 | 63–96 | 14,700–15,800 /s（满速） |
| 19:01–19:34 | 1–3、105–108 | 4,700–5,500 /s（约 1/3） |

例：会话 105 `begin 19:30:31.783` → `finish 19:30:34.209`，2.426 s，
`submitted_samples=13200`，即 **5,441 /s**；同期 `audio_pump` 持续
`queued_samples=0`，说明不是应用侧队列积压，而是送入队列的音频本身只有约 1/3。

### 3. 已排除的链路层与构建变量

- ATT MTU：只读探针 `GattSession.MaxPduSize()` 读回 **247**（不是 43/23），
  排除了「单包只能 40 字节」。
- `GattSession.MaintainConnection=true` 保持 120 s：样本率仍为 ~5 k/s。
- 连接参数：`conn_params … interval_units=12 latency_events=0 timeout_units=200`，
  与 16:36 满速时段相同。
- 应用重启、重装（`c0d79f6` 安装版）、helper 重启（pid 30100，新 HELLO）均不恢复。
- 同一时段微信输入法会话（`path=send_input`）也约 5 k/s，证明与豆包或报告层无关。

### 4. 同一时刻新增一支已连接的 BLE 键盘

Windows System 日志：2026-10-24 18:59:47，`BTHUSB` 事件 8 记录一支远程设备
完成本地配对。该设备（键盘型号 OKR250 BT2）用只读探针读取
`BluetoothLEDevice.ConnectionStatus` 为 **Connected**；遥控器同时为 Connected。
音频速率从 19:00 起下降，时间点与该设备配对/连接重合。

### 5. 单变量 A/B 已确认因果关系（2026-10-24 19:48–19:49）

用户删除/断开 OKR250 BT2 后，保持同一支遥控器、同一应用、同一输入框，连续两次会话：

```text
会话 109：2.32 s，submitted_samples=35040 → 15,116 /s
会话 110：1.61 s，submitted_samples=24000 → 14,925 /s
```

样本率从约 5 k/s 回到约 15 k/s（满速），用户确认**文字已上屏**。
只读探针同时确认：键盘 `ConnectionStatus=Disconnected`，遥控器仍为 `Connected`。
这是单变量对照，根因确认：**第二支 BLE 键盘抢占了适配器的连接调度/带宽，
遥控器音频通知只剩约 1/3**；豆包转写失败是下游结果。

### 6. `voice_verify mic=not_observed` 在本机已失效，不能作为「豆包没开麦」的证据

本机 `ConsentStore\microphone\NonPackaged` 的最后更新时间停在 2026-10-15；
2026-10-24 用一个真实打开 CABLE Output 采集流的探针运行后，该探针没有新增记录，
证明 **Windows 麦克风使用记录本身已不再更新**。因此
`voice_verify … mic=not_observed` 是盲判，不能据此推断豆包未开麦。

## 根因

已确认根因：**蓝牙适配器与第二支 BLE 输入设备（键盘 OKR250 BT2）共存**时，
遥控器的 ATVV 音频通知被调度/丢弃到约 1/3 速率；进入应用的音频只有约 5 k/s，
豆包拿到的是断续音频，转写无法成立，文字自然不上屏。删除/断开该键盘后立即恢复
满速并文字上屏。MTU、连接参数、应用/helper 重启、豆包/微信选择均已排除。

## 修复

**规避（已验证）**：断开/删除同时连接的 OKR250 BT2 键盘后，遥控器语音恢复满速并
文字上屏（passed）。

**应用侧提示（2026-10-24 已实现）**：当前 `voice_verify mic=not_observed` 在本机已失效，
`voice_heal` 的焦点往返对「带宽不足」也无效。实现改为在 `StreamStopped` 按
`active_voice_samples / 会话时长` 判定：

- 判据：样本率 < 12,000 Hz（满速 16,000）且会话 ≥ 800 ms；0 样本/过短会话不判定。
- 结构化日志：
  `voice_audio_rate result=full|degraded rate_hz=… samples=… duration_ms=… expected_hz=16000`。
- `ConnectionSnapshot` 新增 `voiceAudioHealth`（`unknown|full|degraded`）与
  `voiceAudioRateHz`；界面只读，不改变任何语音/断开行为。
- 连接页「语音输入设置」顶部：`degraded` 时显示标题「蓝牙音频不完整」与正文
  「最近一次语音的声音没有完整送到输入工具，文字可能没有出现。请关闭或移开同时连接的
  其他蓝牙设备（键盘、鼠标、耳机等），再试一次。」；
- 向导第⑤步：`voice.no_transcript` 且音频降级时，用同一句蓝牙共存提示替代
  「语音链路正常」；捕获状态（starting/inactive/off）仍优先。
- 不自动断开用户的其它蓝牙设备。

实现文件：
`crates/sayall-windows/src/ble.rs`（判据与日志）、`crates/sayall-windows/src/lib.rs`
（快照字段）、`crates/sayall-windows/tests/ipc_contract.rs` 与
`contracts/ipc/windows-runtime.json`（IPC 契约）、`src/lib/bridge.ts`、
`src/lib/voice-audio-health.ts`、`src/pages/ConnectionPage.vue`、
`src/pages/OnboardingPage.vue`、`src/onboarding/voice-failure-text.ts` 及对应测试。

## 验证

- 断开键盘后的 A/B：会话 109/110 速率 15,116 / 14,925 /s，用户确认文字上屏（passed）。
- 自动化：`cargo test --workspace` 全绿（`sayall-windows` 441 passed / 16 ignored、
  `sayall-windows-app` 153 passed / 1 ignored、IPC 契约 1 passed、诊断日志 1 passed）；
  `cargo fmt --all -- --check` 通过；`cargo check -p sayall-windows-app --features
  runtime-simulation` 通过；前端 `vitest run` 495 passed / 10 skipped、
  `vue-tsc --noEmit` 与 `vite build` 通过。
- 只读探针（未提交，`crates/sayall-windows/examples/gatt_mtu_probe.rs`）：
  MTU 247、遥控器/键盘连接状态；用于排除 MTU 与确认共存设备。
- `voice_verify mic=not_observed` 不可作为开麦判据：本机 ConsentStore 自 2026-10-15
  起不再更新，已用真实采集流探针复核（unknown/盲判）。
- **deferred**：真机上重新连回 OKR250 制造降级，确认连接页与向导第⑤步提示出现；
  本轮为避免再次干扰用户设备，只做了自动化与组件测试。

## 2026-11-22 复测：三链路调度与提示修订

- 现场：SayAll 1.9.3（`source_revision=c01fb4b…`），Windows 25H2 build 26200，
  MediaTek 蓝牙适配器；遥控器、蓝牙键盘、蓝牙鼠标同时 Connected。
- 只读取回的连接参数（公开 `BluetoothLEDevice.GetConnectionParameters`，未写入任何设备）：
  遥控器 `interval_units=12`（15 ms）、`latency_events=0`、`timeout_units=200`（2 s）；
  键盘与鼠标均为 `interval_units=6`（7.5 ms）、`latency_events=44`、`timeout_units=216`。
  三链路共用同一适配器；遥控器语音会话稳定在 5.2–5.6 k samples/s，正好约为满速的 1/3。
- 结论：这是适配器在多条 BLE 链路之间轮流调度造成的时隙不足，不是应用漏收。
  `ThroughputOptimized` 已经提交并生效（15 ms / 延迟 0），MTU 与音频队列均正常。
- **2026-11-22 追加 A/B（passed）**：公开 `BluetoothLEDevice.RequestPreferredConnectionParameters`
  可以临时修改键盘/鼠标的连接参数。只降键盘（180 ms）或只降鼠标（60 ms）都无效
  （4.2–5.0 k/s）；**两只同时降到 `Balanced` 60 ms 或 `PowerOptimized` 180 ms 后，
  遥控器语音恢复到 15.4–15.5 k/s 满速**；结束后 `ThroughputOptimized` + 释放句柄，
  键鼠回到 7.5 ms / 延迟 44。完整数据见
  [2026-11-22-bt-hid-yield-balanced.md](../docs/investigations/2026-11-22-bt-hid-yield-balanced.md)。
- 产品处置（2026-11-22）：不再把“关闭或移开其他蓝牙设备”作为要求。
  连接页标题改为「最近一次语音可能不完整」，正文说明系统会轮流使用蓝牙、
  遥控器只分到一部分声音，并说明语音输入期间会自动让出键盘、鼠标的蓝牙时间、
  结束后恢复；也可以把键盘、鼠标改用 USB 接收器或有线连接。提示增加
  「忽略此提醒」，本次降级期间不再打扰。向导第⑥步复用同一句正文。
- **软件层「语音优先」已实现（2026-11-22，自动、仅语音流期间）**：没有用户开关。
  遥控器 `Streaming` 时，运行时把已连接的 BLE HID 键鼠切到 `Balanced`（60 ms）；
  语音结束、断开或退出立即请求 `ThroughputOptimized` 并释放句柄。运行时探针实测：
  模拟 `Streaming` 时 `state=active yielded=2`，切回 `Ready` 后 `state=off yielded=0`。
- **中间实现的历史证据（已废弃）**：曾做成「默认关闭、降级后一键开启」的常驻设置。
  1.9.3 本地测试包安装后，RC003 + 蓝牙键盘 + 鼠标，`source_revision=2813b07`；
  开启后 `voice_priority action=set enabled=true` → `state=applying` →
  `candidates=2 yielded=2` / `state=active`；键鼠 `48/0/400`；两次语音会话
  `voice_audio_rate result=full rate_hz=15211`（2.761 s）与 `14193`（2.046 s）。
  该包为本地测试构建，不是发布包；机制证据保留，产品行为已被自动实现取代。
- 验证：前端 45 个测试文件、719 passed / 10 skipped、`vue-tsc --noEmit` +
  `vite build` 通过；Rust `cargo test --workspace` 通过（含 `voice_priority`
  2 passed）、`cargo fmt --all -- --check` 通过、
  `cargo check -p sayall-windows-app --features runtime-simulation` 通过；
  `scripts/ci-preflight.ps1` 7/7 通过。
- **1.9.4 本地测试包自动实现实机验证（passed）**：RC003 + 蓝牙键盘 + 鼠标；
  连接页已移除「语音优先」区块（无开关、无一键按钮、无只读说明）。
  两次语音会话 2.658 s / 15.759 s，`voice_audio_rate result=full` 14,898 /s、
  15,777 /s；`state=applying` 在语音开始后 76–118 ms，`apply` 成功在 146–214 ms，
  音频流在 145–195 ms 开始；独立只读探针在会话期间读到键鼠 `48/0/400`，松开后
  `restore` 成功、`state=off`，键鼠恢复 `6/44/216`。短会话整段 full，未观察到
  “前 1–2 s 降级”。该包为本地测试构建，不是发布包。
- **deferred**：RC001、其它适配器、键鼠耗电变化；本轮已安装包验证为 RC003 +
  本机 MediaTek 适配器。

## 隐私检查

未包含蓝牙地址、HID 路径、用户名、语音内容或凭据；只使用相对时间、会话号、
统计值和设备型号。
