# 遥控器电量显示验收

本手册覆盖 GATT 电量订阅（主路径）、Windows 缓存电量轮询（兜底）和界面显示。步骤是验收方法，不代表所有场景均已通过。

## 自动化检查

```powershell
.\scripts\ci-preflight.ps1
node Testing/battery-ui-check.cjs
```

## 真机验收

- 验证 0%、100% 和缺失/错误属性，未知值不能显示成 0%。
- 连接后日志应出现 `remote_battery phase=gatt_subscribe result=passed` 与 `phase=initial_read`；订阅失败时出现 `result=fallback reason=…` 并回退 `remote_battery phase=read`（60s 缓存轮询）。
- **充电实时性（2026-09-27 用户反馈的主缺陷）**：连接状态下给遥控器充电，页面电量应在设备上报后数秒内更新（诊断日志 `remote_battery phase=notify source=gatt_notify level=…`）；不应再出现"充电后电量长期不变"。
- 切换设备、断连、睡眠时拒绝迟到缓存/通知，不显示另一设备的电量。
- 验证电量订阅不新增 BLE 会话、不影响语音（语音会话与订阅共存，RC001 与 RC003 分别验收）。
- 使用 `passed`、`failed`、`deferred` 记录实际执行结果，不上传设备身份。

当前充电实时性、断连、睡眠以及 RC001/RC003 真机验证仍为 `deferred`（代码与仿真验证通过，真机待验）。
