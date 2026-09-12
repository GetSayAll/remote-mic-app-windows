# Voice key inserts a Notepad timestamp during BLE reconnect

- 发现日期：2026-09-10
- 影响版本：v0.2.6 及更早版本
- 状态：已实现修复；Windows 自动化与 RC003 安装版在线 F5 真机验收通过，
  精确建链窗口按压及 RC001 真机验收待完成

## 现象

应用升级重启后的 BLE 重连窗口内，在记事本按遥控器语音键，光标处偶发插入
`15:00 2026/9/10/周四` 一类当前时间、日期和星期，而不是开始语音输入。

## 证据与根因

1. 小米遥控器语音键在 Windows HID 键盘层表现为 `F5`；记事本收到 `F5`
   会执行“插入当前时间/日期”。
2. 2026-09-10 14:57:53 升级到 v0.2.6 后，应用按 2/4/8/16/30 秒退避
   反复建立 GATT 链路，15:02:29 才首次进入成功的 ATVV 服务发现，约
   15:03:33 收到首个语音控制通知。该会话前抑制器累计
   `seen=74 swallowed=0 leaked=74`，证明原生 F5 在未就绪窗口全部放行。
3. `key_suppressor.rs` 与 `raw_input_windows.rs` 曾分别调用
   `RegisterRawInputDevices` 注册键盘。微软文档规定同一进程、同一 Raw Input
   设备类只有最后注册的窗口能接收；主监听器覆盖语音抑制器窗口后，后者无法
   再用设备路径归因 F5，也无法用遥控器活动打断重连退避。
4. 历史日志只在每次尝试开始前留下 WASAPI interrupt，未记录失败所在的 WinRT
   阶段和耗时；可确认是多次失败/系统超时而非一个调用阻塞四分钟，但无法从旧
   日志进一步区分 `FromIdAsync`、服务发现或 CCCD 订阅。Windows System 日志在
   对应窗口没有可用的逐尝试事件。

## 修复

- 删除语音抑制器的第二套 Raw Input 注册，由进程唯一的主监听器把已归因的
  遥控器语音 F5 转发给抑制器和 BLE 立即重连入口；Raw Input 按物理
  DOWN/UP 配对，每次按住只唤醒一次，typematic 重复 DOWN 仅刷新抑制宽限。
- 在 `Connecting / Discovering / AwaitingCapabilities / Reconnecting` 建链窗口
  临时保护 F5；进入 Ready、失败、挂起或用户主动断开后关闭，稳定在线时仍由
  ATVV `0x04` 前置信号精确武装。DOWN/UP 继续遵守“DOWN 泄漏则 UP 放行”的
  防粘键配对规则。
- 为每次 BLE 尝试及 `device_from_id`、设备属性、服务/特征发现、CCCD 订阅、
  连接回调注册、能力请求增加结构化阶段日志和耗时；下次同类故障一次日志即可
  定位具体 WinRT 环节。

## 参考边界

- 参考仓库 `vibe-flow`（提交 `b47f7cdce8b753fade0c64c97332bebe80f17d2d`）
  对语音 F5 使用 LL 钩子兜底且接受实体键盘 F5 冲突。本项目只在建链窗口临时
  采用该兜底，稳定状态保留实体键盘 F5。
- 无驱动的 LL 钩子本身没有来源设备 ID；要同时做到“重连首沿零泄漏”和“实体
  键盘 F5 永不受影响”需要设备级驱动，本修复不扩展到驱动轨。

## 验证

- 单元测试：建链四相位启用保护；Ready/Streaming/Draining/失败/挂起等相位关闭。
- 单元测试：连接保护、ATVV 会话或 Raw Input 武装任一成立均吞 F5 DOWN；
  DOWN 曾泄漏时 UP 仍放行。
- 真机验收要求：RC003 在记事本前台触发一次重连窗口语音按压，确认无时间戳、
  `leaked` 不增加、`swallowed` 增加，并从新增阶段日志确认恢复耗时归因。
- 2026-09-12，基于最新 `origin/main` 的安装版（source revision
  `17f0ced8`）在 RC003 在线状态、记事本前台实按语音键：用户确认未出现
  日期时间；同次日志收到 ATVV `0x04` 与完整音频 begin/finish，抑制统计由
  `seen=1 swallowed=1 leaked=0` 增至 `seen=2 swallowed=2 leaked=0`，且
  `raw_remote_f5=1`，证明真实 F5 到达并被吞下。passed。
- 关闭整机蓝牙时 HID 通道也随之消失，期间“未出现时间戳”不能作为建链保护
  真机证据；建链四相位保护目前由自动化覆盖，RC003 精确建链窗口按压与 RC001
  仍为 deferred，不扩大通过声明。
