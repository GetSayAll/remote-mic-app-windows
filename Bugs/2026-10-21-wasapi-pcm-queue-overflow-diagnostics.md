# WASAPI PCM 队列溢出：端点停止排空还是 worker 阻塞

- 发现日期：2026-10-21
- 状态：调查中（已加诊断与失效早检；根因待现场日志确认）
- 影响范围：SayAll Windows 当前构建、RC003、VB-CABLE；长语音会话或 CABLE 下游停读时可能触发
- 功能点：WASAPI PCM 队列与 `AudioSink::pump`（`crates/sayall-windows/src/audio.rs`）
- 现象：界面报 `WASAPI PCM queue exceeded its bounded capacity`，日志为 `error_code=queue_overflow`；旧构建随后永久停在 `endpoint_not_selected`
- 复现条件：长语音会话期间 CABLE 渲染端/下游消费端停止排空，或音频客户端已失效但 worker 未及时发现。尚未稳定复现
- 正常预期：失效时立即以 `device_invalidated` 收尾并在下一次按键重开；短暂停读时可从 1 秒进度日志判断是“无可写空间”还是“worker 未运行”

## 证据

- 2026-10-21 12:55:56.318Z（旧构建 `1f42dfd`）：12 秒会话 `queued_samples=31884`、`submitted_samples=32196`，`error_code=queue_overflow`；随后两次 `endpoint_not_selected`（`elapsed_ms=0`）。
- 同一构建 19 秒健康会话提交 295680 个样本，说明 2 秒队列上限本身不是固定不足。
- 2026-10-21 安装版 `66b8821`：重启 Audiosrv 触发 `event=disconnected reason=server_shutdown`，下一次按键 `action=recover ... passed`（66 ms），`begin`/`finish` 均 passed；说明失效恢复路径本身可用。
- 方案 A（切走豆包麦克风）没有复现；方案 B（禁用/启用端点）没有让旧客户端失效，禁用窗口内仍提交 95520 个样本。两者不能作为稳定的队列溢出复现手段。

## 根因

尚未确认，候选按优先级：

1. **端点停止排空**：`get_available_space_in_frames` 持续为 0，队列增长到 2 秒上限；下游 CABLE Output 消费端可能停止读取。
2. **worker 被阻塞**：`pump` 调用间隙变大，`pump_calls` 不增长；可疑点是 `ensure_session_unmuted` 的 COM 枚举/静音检查。
3. **失效事件未及时处理**：旧构建没有 `IAudioSessionEvents`；客户端已失效但 `get_available_space` 仍返回 0 或错误来得太晚。

## 修复

- `AudioSink::pump` 在入口检查 `OnSessionDisconnected` 标记；已失效立即返回 `AudioDeviceInvalidated`，不再等队列涨到 2 秒才报 `queue_overflow`。日志记 `audio_pump event=invalidated reason=...`。
- `AudioSink` 增加 `pump_calls` / `zero_space_polls` / `last_pump_progress`，每 1 秒输出 `audio_pump phase=progress available_frames=... queued_samples=... pump_calls=... zero_space_polls=... stream_started=... draining=...`。
- 不改队列上限和丢弃策略；`queue_overflow` 仍按原逻辑失败，下一次按键由 #276 的自动重开恢复。

## 验证

- `cargo fmt --all -- --check`：passed。
- `cargo test -p sayall-windows`：423 passed / 14 ignored / 0 failed。
- `cargo test -p sayall-windows audio::tests -- --include-ignored`（本机 VB-CABLE）：22 passed / 0 failed，包含新增 `invalidated_sink_pump_fails_before_prebuffer`。
- 现场日志验证（1 秒进度、端点停止排空 vs worker 阻塞）：**deferred**，需要一次真实 `queue_overflow` 或长会话停读场景；当前本机健康会话不触发。

## 隐私检查

未包含用户名、主机名、蓝牙地址、HID 路径、端点 GUID、语音内容或凭据。
