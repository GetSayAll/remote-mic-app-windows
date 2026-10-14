# 蓝牙鼠标与遥控器共存验收

本手册用于判定 MX Anywhere 3 蓝牙模式卡住是否已随自动关开蓝牙机制的移除而消失。脚本与步骤是验收方法，未执行的用例标为 `deferred`。

## 环境记录

- 记录应用版本、`source_revision`、Windows build、笔记本蓝牙适配器型号、鼠标模式、遥控器型号（RC001 / RC003）。归档时脱敏设备身份和路径。
- 保留 `%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log` 的故障前后时间段。日志中的 UTC 时间与用户报障的本地时间需换算后对齐。
- 启动日志应有 `ble_recovery_policy mode=retry_only radio_cycle=disabled`。缺失时先确认安装的构建，不能把旧包当新版验收。

## 原场景复测

1. 用蓝牙连接 MX Anywhere 3。保持遥控器已在 SayAll 中连接。记录开始时间。
2. 正常移动和点击鼠标，覆盖原报告的空闲与使用节奏。持续时间至少覆盖原先一次卡住的间隔；若间隔未知，先观察 90 分钟。
3. 出现卡住时，立即记录本地时间和持续时间。先观察指针及点击是否同时失效，再按原反馈的晃动加点击动作确认是否恢复。不要以这一步代替事件日志。
4. 退出 SayAll，确认鼠标是否恢复。再以同一鼠标的 USB 接收器模式运行同样时长作为对照。
5. RC001、RC003 分别执行；每种型号至少验证一次闲置后首按、连续语音、快速按放、断连恢复与睡眠恢复。记录语音会话结果及送达率，不能只看连接成功。

## 日志判读

| 日志或事件 | 可以证明 | 不能证明 |
| --- | --- | --- |
| `radio_cycle=disabled` | 本构建的应用不调用旧自动无线电恢复 | Windows 驱动或其他程序不会重置蓝牙 |
| `conn_params terminal_result=submitted` | Windows 接受参数请求 | 遥控器或鼠标的实际参数已经改变 |
| `conn_params terminal_result=observed` | 读到了遥控器链路的实际参数 | 鼠标链路状态或鼠标卡住原因 |
| `ble_link status=Disconnected` | 遥控器连接状态变化 | 鼠标也已断连 |
| `conn_params ... stale_callback` | 旧连接的迟到事件已被隔离 | 当前鼠标链路异常 |
| `conn_params ... no_active_session` | 同代次回调到达时遥控器会话已不存在 | 回调来自旧连接 |

若新版仍复现，先按故障时刻对齐应用日志与 Windows 蓝牙设备事件，再对连接参数做单变量 A/B。`conn_params` 的数值单位：连接间隔单位为 1.25 ms，链路超时单位为 10 ms，延迟单位为连接事件数。日志不记录鼠标身份或输入报告。
