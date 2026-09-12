# 双遥控器 BLE Unreachable（GATT 状态 1）：多手段自愈矩阵实测

## 2026-09-12：`Windows API failed: 内存资源不足`

- 安装锁屏修复测试包后，用户连接 RC001/RC003 时收到上述错误，无法继续
  锁屏复验。现场系统仍有约 3.2 GB 可用物理内存、约 9 GB 可用提交内存，
  SayAll 私有内存约 16 MB，排除真实内存耗尽。
- 日志中的音频中断时刻与两轮 2/4/8/16/30 秒退避及无线电恢复时序吻合；
  第二次手动连接后的下一轮尝试停住，之后不再产生重连活动。结合代码可推断：
  BLE MTA 工作线程调用了微软明确要求从 UI 线程调用的 `FromIdAsync`；外层
  30 秒只让 IPC 调用方超时，不能终止内部 WinRT 操作，工作线程一旦卡住，
  后续退避和无线电恢复也无法运行。
- 修复：从 Windows 配对 AssociationEndpoint ID 中解析最后一个蓝牙地址
  （对端），在 MTA 工作线程改用 `FromBluetoothAddressAsync` 重建设备对象；
  不对官方声明“不能取消”的 GATT 连接过程伪造超时线程。连接全过程新增
  `ble_connect`、`ble_connect_stage`、`ble_reconnect`、
  `ble_radio_recovery` 结构化日志，且不记录设备 ID、地址或名称。
- 自动化：地址解析选取对端、错误分类、既有重连/清理测试 passed；新包连接
  与锁屏真机复验 deferred。

## 现象（2026-09-07 用户报障）

- 上午起两只遥控器（RC001 + RC003）均连不上；应用 UI 显示
  "正在等待遥控器重连"，错误文案
  `Xiaomi voice remote GATT operation failed: 发现 ATVV 服务返回状态 1；将在 N 秒后进行第 N 次重连`。
- Windows `GattCommunicationStatus` 状态 1 = **Unreachable（设备不可达）**：
  不是 GATT 表损坏，是蓝牙链路根本建立不起来。
- 前情：同日上午用户报"RC001 连上但语音没声音"（未及单独定位，连接即恶化）。
- 经典蓝牙 HID 层正常：PnP 两只遥控器 Status=OK；用户按键后应用的
  wake_reconnect 退避重置机制可观测（UI 重试计数回退）。

## 系统侧排查（全部实际执行）

| 检查项 | 结果 |
| --- | --- |
| bthserv 蓝牙服务 | RUNNING，正常 |
| Intel 蓝牙适配器（PnP） | OK |
| 两只遥控器 PnP 节点 | 都 OK（配对完好） |
| Windows 事件日志（蓝牙通道 + System，3 小时） | 无蓝牙相关错误 |
| 应用进程 | 运行中，自动重连循环正常（2–30s 指数退避持续爬升到第 33+ 次） |

## 恢复尝试矩阵（时序 + 结果）

1. **PnP 级无线电复位**（Disable→Enable Intel 蓝牙 USB 节点，
   USB\VID_8087&PID_0A2B）：**当场无效**（failed）——遥控器深度睡眠
   不广播时主机侧无从连接；对 BT 外设秒级瞬断。
2. **优雅退出 + 带日志重启**（WM_CLOSE→应用托盘驻留未退出；改用
   Toolhelp32 枚举线程 + PostThreadMessage WM_QUIT 正常退出路径成功，
   Rust drop/BLE 清理正常执行）：重启后仍 Unreachable（failed）。
3. **bthserv 重启**：非提权会话权限不足（无法执行，deferred）。
4. **用户多次按两只遥控器按键**（配合第 2 步重启后的持续重连循环）：
   **约 20 分钟内连接恢复**（passed）——恢复后语音会话连续成功
   （日志 session 32–36：chord_press ok、wetype_check reacted=true、
   MIC_EXTEND 续期 `T 0E` 正常）、按键映射正常（Menu→打开微信 ok）、
   UI 显示"已连接"。

## 结论

- 故障特征：**双遥控器同时 BLE Unreachable + HID 层正常 + 无线电复位
  当场无效 + 按键唤醒后自愈成功**。与 09-05 复盘的"强杀残留链路僵死"
  不同：本次无强杀，且自愈周期远超 60 秒（依赖用户按键唤醒遥控器）。
- 最可能机理：遥控器深度睡眠后 BLE 不广播；无线电复位清不掉"遥控器
  侧不广播"这一状态；多次按键唤醒 + 持续重连循环最终建立链路。为何
  首次唤醒按键未立即恢复（wake_reconnect 触发的立即重连仍 Unreachable）
  待后续复现取证——当前 BLE 链路零日志（见缺陷），无法回放每次尝试。
- 另：bthserv 层是否需要复位未验证（权限限制），留作下次复现时的
  对照项。

## 暴露的缺陷

1. **BLE 连接/重连链路零日志**（违反"功能点必须自带日志"规范，
   2026-09-12 已修复）：
   attempt_connection、find_service 失败、重连调度均无 gatt_note 打点。
   本次定位只能依赖 UI last_error 与恢复后的间接证据；"radio_recovery
   自愈是否运行过"也无法从日志确认。修复：连接路径全分支打点
   （attempt 开始/结果/退避/唤醒/无线电恢复），一次报障 + 一次日志
   拉取即定位。现已覆盖设备对象重建、属性、服务/特征发现、订阅、能力请求、
   退避调度和无线电恢复结果；真机日志待复验。
2. **Raw Input 监听失败提示不区分原因**：UI 显示"监听启动失败（自动
   重试中）"，实际是遥控器不可达时 HID 设备节点缺失的果——应区分
   "等待设备"与"真异常"并落日志。

## 验证

- 事件日志、PnP、服务状态检查：passed（见上表）。
- PnP 无线电复位后 72 秒监测：failed（仍 Unreachable）。
- WM_QUIT 优雅退出 + 带日志重启：passed（进程正常退出、新实例日志
  正常落盘）。
- 按键唤醒 + 重连循环：passed（连接恢复，语音/映射功能正常）。
- 诊断脚本归档：Testing/investigation/conn-status.ps1、graceful-stop-app.ps1、
  radio-cycle.ps1、tray-quit.ps1、find-tray-icon.ps1、tray-quit2.ps1、
  wmquit-app.ps1、start-with-diag.ps1、long-poll.ps1、dump-diag.ps1、
  restart-bthserv*.ps1、show-and-read-ui.ps1、switch-to-conn-page.ps1。
