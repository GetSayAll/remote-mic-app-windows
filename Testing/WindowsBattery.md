# 遥控器缓存电量验收

本手册覆盖 Windows 缓存电量读取和界面显示。步骤是验收方法，不代表所有场景均已通过。

## 自动化检查

```powershell
.\scripts\ci-preflight.ps1
node Testing/battery-ui-check.cjs
```

## 真机验收

- 验证 0%、100% 和缺失/错误属性，未知值不能显示成 0%。
- 切换设备、断连、睡眠时拒绝迟到缓存，不显示另一设备的电量。
- 验证缓存查询不新增 GATT 会话，不影响语音；RC001 与 RC003 分别验收。
- 使用 `passed`、`failed`、`deferred` 记录实际执行结果，不上传设备身份。

当前持续更新、断连、睡眠以及 RC001/RC003 真机验证仍为 `deferred`。
