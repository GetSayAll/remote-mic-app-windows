# CABLE Input 端点静音自愈实证（2026-09-07）

## 问题与根因

Windows 音量混合器中 CABLE Input 端点主静音后，SayAll 的 WASAPI 共享模式写入和电平活动仍可存在，但下游 CABLE Output 收到的是静音结果。旧实现只持有并写入 `IAudioClient`，没有检查端点级 `IAudioEndpointVolume::GetMute`；写入成功不能证明链路可听。

## 修复边界

- 只管理名称确认的 VB-CABLE 渲染端点，非 CABLE 输出明确跳过。
- 打开/恢复端点时检查一次；每次语音会话开始前再次检查，覆盖应用运行期间端点被外部静音的场景。
- `GetMute` 为真时调用 `SetMute(FALSE)`，随后再次 `GetMute`；读回仍静音则会话失败并显示音频错误，不继续制造“状态正常但全静音”的假象。
- 只解除静音，不修改端点音量标量。
- 日志不含端点 ID，记录检查点、分支结果、前后静音状态、音量与耗时。

## Windows 真实端点实验

环境中应用保存的输出端点名称为 `CABLE Input (VB-Audio Virtual Cable)`。实验测试通过环境变量接收端点 ID，不把 ID写入仓库或日志。

步骤：

1. 读取基线：`mute=False, level=1.000`。
2. 通过公开 `IAudioEndpointVolume::SetMute(TRUE)` 制造端点静音。
3. 调用产品 `AudioRuntime::select_endpoint`；读回必须为未静音。
4. 在端点保持打开时再次 `SetMute(TRUE)`。
5. 调用产品 `AudioRuntime::begin_session`；读回必须再次为未静音。
6. 中断测试会话并恢复实验前状态；最终只读探针确认 `mute=False, level=1.000`。

结果：`passed`。

```text
endpoint_unmute checkpoint=open result=unmuted was_muted=true is_muted=false level=1.000 elapsed_ms=1
endpoint_unmute checkpoint=begin_session result=unmuted was_muted=true is_muted=false level=1.000 elapsed_ms=1
```

自动测试：`cargo test -p sayall-windows audio::tests::cable_endpoint_unmutes_on_open_and_each_session -- --ignored --exact --nocapture`，1 passed。

边界：这次实验实证的是 Windows 真实 CABLE Input 的端点静音自愈；未代替 RC001 与 RC003 各自的完整语音真机验收。
