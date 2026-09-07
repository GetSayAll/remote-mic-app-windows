# CABLE Input 端点静音自愈实证（2026-09-07）

## 问题与根因

Windows 音量混合器中有两层互相独立的静音：左侧 CABLE Input 端点主静音，以及右侧“无线麦 SayAll”应用会话静音。任一层静音时，SayAll 的 WASAPI 共享模式写入和电平活动仍可存在，但下游 CABLE Output 收到的是静音结果。旧实现只持有并写入 `IAudioClient`，没有检查端点级 `IAudioEndpointVolume::GetMute`，也没有检查会话级 `ISimpleAudioVolume::GetMute`；写入成功不能证明链路可听。

## 修复边界

- 只管理名称确认的 VB-CABLE 渲染端点，非 CABLE 输出明确跳过。
- 打开/恢复端点时检查一次；每次语音会话开始前再次检查，覆盖应用运行期间端点被外部静音的场景。
- `GetMute` 为真时调用 `SetMute(FALSE)`，随后再次 `GetMute`；读回仍静音则会话失败并显示音频错误，不继续制造“状态正常但全静音”的假象。
- 应用会话通过 `IAudioSessionManager2` 枚举，并以 `IAudioSessionControl2::GetProcessId` 只匹配当前 SayAll 进程；不修改系统声音或其他应用。
- 初始化时让 SayAll 会话退出 Windows 默认通信自动压低机制；这项预防措施不等同于已归因外部静音来源。
- 会话初始化、语音会话开始、流启动后检查；推流期间每 100ms 检查一次，覆盖激活后稍晚恢复持久化静音的现场现象。
- 只解除静音，不修改端点或会话音量标量。
- 日志不含端点 ID，记录检查点、分支结果、匹配会话数、前后静音状态、音量与耗时。

## Windows 真实端点实验

环境中应用保存的输出端点名称为 `CABLE Input (VB-Audio Virtual Cable)`。实验测试通过环境变量接收端点 ID，不把 ID写入仓库或日志。

步骤：

1. 读取基线：`mute=False, level=1.000`。
2. 通过公开 `IAudioEndpointVolume::SetMute(TRUE)` 制造端点静音。
3. 调用产品 `AudioRuntime::select_endpoint`；读回必须为未静音。
4. 在端点保持打开时再次 `SetMute(TRUE)`。
5. 调用产品 `AudioRuntime::begin_session`；读回必须再次为未静音。
6. 中断测试会话并恢复实验前状态；最终只读探针确认 `mute=False, level=1.000`。

会话层追加受控步骤：

1. 初始化真实 CABLE Input 渲染会话，确认只枚举到当前测试进程的一个会话。
2. 在 `begin_session` 前将该会话设为静音，产品读回并解除。
3. 在首批音频触发 `IAudioClient::Start` 前再次静音，产品在 `after_start` 检查点解除。
4. 推流期间第三次静音，100ms 监视检查点解除。
5. 测试退出前恢复会话原始静音状态。

结果：`passed`。

```text
endpoint_unmute checkpoint=open result=unmuted was_muted=true is_muted=false level=1.000 elapsed_ms=1
endpoint_unmute checkpoint=begin_session result=unmuted was_muted=true is_muted=false level=1.000 elapsed_ms=1
session_unmute checkpoint=begin_session result=unmuted sessions=1 muted_before=1 muted_after=0 min_level=1.000 elapsed_ms=0
session_unmute checkpoint=after_start result=unmuted sessions=1 muted_before=1 muted_after=0 min_level=1.000 elapsed_ms=0
session_unmute checkpoint=stream_watch result=unmuted sessions=1 muted_before=1 muted_after=0 min_level=1.000 elapsed_ms=0
```

自动测试：`cargo test -p sayall-windows audio::tests::cable_endpoint_unmutes_on_open_and_each_session -- --ignored --exact --nocapture`，1 passed。

## SayAll + RC001 现场复验

修复版 Tauri 应用连接真实 RC001 后连续触发 9 次语音。每次 `begin_session` 与 `after_start` 读回均为未静音；流启动约半秒后，外部状态变化把 SayAll 会话重新设为静音，100ms 监视在 9/9 会话中捕获 `muted_before=1` 并读回 `muted_after=0`。共收到 2765 个 ATVV 音频包；9 次开始/停止与快捷键按下/释放均严格成对。

```text
session_unmute checkpoint=after_start result=already_ok sessions=1 muted_before=0 muted_after=0 min_level=1.000 elapsed_ms=0
session_unmute checkpoint=stream_watch result=unmuted sessions=1 muted_before=1 muted_after=0 min_level=1.000 elapsed_ms=0
```

结果：RC001 现场“开始时未静音、很快又静音”复现并自愈，9/9 `passed`。

修复版 NSIS 本地测试包随后交由用户复测；2026-09-07 用户确认 RC001
语音功能测试通过。该结果记为 RC001 + CABLE Input 语音路径 `passed`。

边界：这次实验实证的是 Windows 真实 CABLE Input 的端点、当前进程应用
会话静音自愈及 RC001 语音路径；未代替 RC003 的语音真机验收。
