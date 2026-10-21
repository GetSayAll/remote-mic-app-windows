# RDP 会话重连后 WASAPI 客户端失效，语音永久停在 endpoint_not_selected

- 发现日期：2026-10-20（issue #276）
- 状态：等待真机验证
- 影响范围：SayAll Windows 1.0.0（`a4c1311`）、RC003、VB-CABLE、Windows WTS 会话断开/重连；Win10/Win11 均可能受影响
- 功能点：WASAPI 输出端点与会话生命周期（`crates/sayall-windows/src/audio.rs`）
- 现象：会话重连后的第一次语音请求报 `0x88890004`；之后每次按键都在 0 ms 内返回 `endpoint_not_selected`，选择仍保存在配置里，只能手动重选设备或重启应用恢复
- 复现条件：正常使用 SayAll → RDP 连接并返回本地原会话 → 再按遥控器语音键。尚未做受控 RDP 重现
- 正常预期：端点仍可用时，释放失效的 `IAudioClient`，在同一端点上重开，保留用户选择；端点消失或改名时才要求重新选择

## 证据

- issue #276 的时间线：19:05:06 会话重连 → 19:05:35 `wasapi_failure`（`queued_samples=480`、`submitted_samples=0`）→ 19:05:37 起 `endpoint_not_selected`（`elapsed_ms=0`）；20:17:56 再次重连后同样复现。
- `elapsed_ms=0` 说明后续失败没有进入任何 WASAPI 调用，是应用状态机直接拒绝。
- 同一现场独立 Python 客户端能重新打开同一端点并持续写入静音，说明端点没有消失，失效的是 SayAll 已持有的旧客户端。
- 微软文档：WTS 会话断开时系统关闭音频会话，已有客户端的方法返回 `AUDCLNT_E_DEVICE_INVALIDATED`；恢复步骤是释放旧 `IAudioClient` 并在同一设备上重新 Activate。
  - <https://learn.microsoft.com/en-us/windows/win32/api/audiopolicy/nf-audiopolicy-iaudiosessionevents-onsessiondisconnected>
  - <https://learn.microsoft.com/en-us/windows/win32/coreaudio/recovering-from-an-invalid-device-error>

## 根因

1. `AudioSink::pump` 的 `get_available_space_in_frames()` 返回 `0x88890004`（`AUDCLNT_E_DEVICE_INVALIDATED`）。
2. `fail_audio` 执行 `sink.take()`，释放旧客户端并把 `phase` 设为 `Failed`，但保留 `selectedEndpointId` / `selectedEndpointName`。
3. 下一次 `begin_session` 只检查 `sink` 是否存在；`sink` 为空就直接返回 `AudioEndpointNotSelected`，从不尝试重开同一端点。
4. 只有用户手动 `select_endpoint` 或启动时的 `restore_endpoint` 会重建客户端，因此故障会一直持续。
5. 旧实现没有注册 `IAudioSessionEvents`，日志拿不到 `OnSessionDisconnected` 的具体原因。

RDP 是强相关触发条件，但尚未受控复现；修复针对 `AUDCLNT_E_DEVICE_INVALIDATED` 本身，不依赖先确认 RDP。

## 修复

- 新增 `PlatformError::AudioDeviceInvalidated` 与 `device_invalidated` 日志错误码；只把 `0x88890004` 归入该分类，`0x8889000A` 等其它错误保持原有行为。
- 打开端点时注册 `IAudioSessionEvents`（best-effort），记录 `audio_session event=disconnect_subscribe result=...` 与 `event=disconnected reason=...`；回调只落稳定英文原因 token，不含端点 ID/名称。
- `begin_session` 在以下两种情况下自动重开：
  - `sink` 不存在但内存里仍有用户选择（上一次流失败后的主路径）；
  - 收到 `OnSessionDisconnected`，sink 已标记失效。
- 重开前先释放旧客户端，再用保存的 id + name 校验端点身份；复用同设备 CABLE 兜底。端点消失或改名时失败关闭，提示用户重新选择，不静默换设备。
- 若事件订阅漏报，`ensure_unmuted` / `reset` 返回 `AUDCLNT_E_DEVICE_INVALIDATED` 时仍会释放旧客户端并重试一次。
- `AudioDeviceInvalidated` 一并纳入 begin / interrupt / finish 的失败收尾：释放 sink、把 phase 置 `Failed`，避免 reset 在 `Draining` 阶段失败后下一次 begin 永久 `AudioBusy`。
- 恢复路径只做一次快速打开（首次尝试 + 同设备兜底各一次），避免拖慢语音起播；完整重试仍保留在用户点选/启动恢复路径。

## 验证

- `cargo fmt --all -- --check`：passed。
- `cargo test -p sayall-windows`：420 passed / 13 ignored / 0 failed。
- `cargo test -p sayall-windows audio::tests -- --include-ignored`（本机 VB-CABLE 端点）：18 passed / 0 failed，包含：
  - `missing_sink_is_reopened_from_the_saved_selection`：模拟 `fail_audio` 后的 `sink=None`，`begin` 自动重开并进入 `Streaming`。
  - `invalidated_sink_is_replaced_before_begin`：模拟 `OnSessionDisconnected` 标记，`begin` 释放旧客户端并重开。
- 日志副作用（真实端点）：`disconnect_subscribe result=passed` → `audio_endpoint action=recover phase=requested trigger=begin_sink_missing|begin_device_invalidated` → `action=recover phase=completed terminal_result=passed`，随后 `begin_session` 完成。
- `cargo check --workspace`：passed。
- `cargo check -p sayall-windows-app --features runtime-simulation`：passed。
- `cargo test --workspace`（rebase 到 `origin/main` `0b9194f` 后）：634 passed / 14 ignored / 0 failed。
- `scripts/ci-preflight.ps1` 七步全部 `PASS`：前端 450 passed / 10 skipped，Rust workspace 测试、Tauri host 与 runtime simulation 编译检查均通过。
- 真机 RDP 受控复现（连接/断开后首次按键、连续两次断连、睡眠唤醒回归）：**deferred**，本机无法受控制造 WTS 重连；需 Andy 在 RC003 + VB-CABLE 现场按复现条件验收。

## 隐私检查

未包含用户名、主机名、蓝牙地址、HID 路径、端点 GUID、语音内容或凭据。
