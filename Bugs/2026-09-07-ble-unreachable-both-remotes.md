# 双遥控器 BLE Unreachable（GATT 状态 1）：多手段自愈矩阵实测

## 2026-09-14：`0x80070008` 的内核栈兜底恢复

- 在已稳定复现 `0x80070008` 的主机上，参考仓库的“配对 selector →
  `BluetoothLEDevice.FromIdAsync`”路径返回 `0x80004004 (E_ABORT)`；直接 GATT
  service selector 仍返回 `0x80070008`；SetupAPI 能看到一个 BLE 接口，但 Win32
  GATT `CreateFile` 返回 `0x80070079`（信号量超时）。三条互相独立的用户态入口
  同时失败，定位到 Windows 蓝牙设备节点/内核栈，而不是 SayAll 选错 WinRT 构造入口。
- 普通用户执行 `pnputil /restart-device` 得到“拒绝访问”；`/disable-device` 因关键
  系统设备被拒绝，而且该工具在这类逻辑失败时仍可能返回退出码 0。因此新恢复路径
  不以工具退出码单独判成功，而是操作后重新枚举 WinRT Bluetooth Radio。
- 修复仅在 WinRT 返回 `0x80070008` 或 `0x80004004`、常规 Radio 恢复也无法取得
  对象时启用。SetupAPI 只接受当前存在且服务名为 `BTHUSB` 的唯一蓝牙类设备；随后
  通过 Windows 自带 `pnputil.exe /restart-device` 显示 UAC 请求，用户无需进入设置
  手工关开蓝牙。真实设备实例 ID 只在内存中传递，不写日志；多适配器时拒绝猜测。
- 现场显式运行恢复测试：UAC 同意后 1.95 秒内完成设备节点重启并通过 WinRT Radio
  重新枚举，测试 passed；随后运行 SayAll，连接日志从即时
  `windows_resource_exhausted` 前进到 `conn_params`，证明
  `BluetoothLEDevice` 创建能力已经恢复。当前选中遥控器完整能力协商与 RC001、
  RC003 分别制造僵死后的无人值守恢复仍 deferred。
- 连接资源释放另修一处确定缺陷：旧 `BleSession::close` 在第一次 Close 前就永久标记
  `closed=true`；service/device 任一 Close 失败后，后续调用只重复返回已保存错误，
  与“保留所有者并重试”的设计注释相反。现在分别记录两个所有者的关闭状态，后续
  Close 只重试未成功项；本地事件、CCCD 和连接参数释放仍只执行一次，并新增不含
  身份信息的 `ble_session_cleanup` 阶段日志。

## 2026-09-13：重连失败路径泄漏 WinRT BLE 资源

- 代码复核发现 `BluetoothLEDevice` 创建成功后，设备属性、服务发现或任一特征发现
  使用 `?` 提前返回时，只释放 Rust/COM 引用，没有显式调用
  `GattDeviceService.Close()` / `BluetoothLEDevice.Close()`；订阅函数在注册
  `ValueChanged` 后若读取属性或写 CCCD 的异步调用失败，也存在处理器未退订路径。
  高频指数退避会重复走这些分支，可能逐步占满 Windows BLE 栈资源，最终让设备和
  Radio 的所有 WinRT 创建入口统一返回 `0x80070008`。这是基于代码所有权与现场
  错误演进得到的根因判断；健康栈压力复现仍需真机验收。
- 修复新增 `PendingBleConnection` 连接期所有权守卫：设备创建后任何阶段失败，均按
  已取得资源自动执行事件退订、通知关闭、连接参数请求关闭、GATT service 关闭和
  设备关闭；完整建链后才把所有权一次性交给 `BleSession`。`subscribe` 内部也对
  注册成功但 CCCD 配置失败做局部回滚，外层守卫无需猜测不可见的 token。
- 新增 `ble_partial_cleanup` 与 `ble_subscription_rollback` 结构化日志，只记录尝试数、
  失败数和原因，不含设备地址、ID 或名称。该修复阻止重连循环制造新的资源泄漏；
  Radio 预热缓存和周期性恢复窗口继续处理应用启动前已存在的系统栈僵死。
- 自动化与安装包验证见本次提交交付记录；RC001/RC003 分别制造中途失败并确认随后
  自动恢复仍 deferred。

## 2026-09-13：恢复预算永久耗尽与 Radio 事后枚举失败

- 最新 `main` 安装版 `1086c19` 启动后，`device_from_address` 持续在 0–1ms
  返回 `windows_resource_exhausted`。两轮恢复均取得 `RadioAccessStatus::Allowed`，
  但 `GetRadiosAsync` 与设备查询兜底都返回 `0x80070008`，因此没有执行到
  Off/On；两轮之后只剩普通指数退避，恢复预算在本进程生命周期内永久耗尽。
- 新增一次性探针使用系统当前 PnP Radio ID 直接调用 `Radio::FromIdAsync`，仍
  返回 `0x80070008`。这排除了“只需绕过枚举”的假设：对象必须在系统栈健康时
  预先取得并保留，不能等资源耗尽后再创建。
- 修复一：Tauri setup 的可交互 UI 上下文在 BLE 线程启动前调用官方 Radio API，
  预先缓存 Bluetooth Radio 对象并请求控制权限；恢复路径优先复用缓存，不再依赖
  已经失败的枚举。若启动预热失败但 BLE 后续恢复连接，会立即补建缓存。
- 修复二：恢复策略改为“每窗口最多 2 次 + 60 秒冷却后自动重开窗口”；连接成功、
  主动断开和系统恢复会重置窗口。任何一次恢复失败都只影响当前窗口，不再要求用户
  手动开关蓝牙，也不会永久退化成“正在等待遥控器重连”。
- 日志新增 `radio_recovery_prepare`、恢复 `window`、`window_reopened` 和
  `cooldown_ms`，不记录 Radio ID、蓝牙地址或设备名称。
- 验证：恢复预算阈值/上限/冷却重开/连接后重置单元测试 passed；Windows 全工作区
  `cargo check --workspace --all-targets --all-features` passed。安装包 `34537f9` 在启动前
  已资源耗尽的现场完成窗口 1 两次尝试，并在第二次完成 62.06 秒后自动记录
  `window_reopened window=2 cooldown_ms=60000`，证明恢复预算不会永久耗尽（passed）。
  该现场因预热前已经 `0x80070008`，缓存为 unavailable；系统健康启动时缓存成功、
  随后再复现僵死并自动 Off/On/重连仍 deferred。

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

### 2026-09-12 21:37 统一包复验补充

- 提交 `69b1893` 的安装版把设备创建换成 `FromBluetoothAddressAsync` 后，
  仍在 0-27ms 内返回 `windows_resource_exhausted`。因此“仅因 MTA 调用了
  UI-only `FromIdAsync`”的归因被真机证伪；直接故障是 Windows 蓝牙栈资源
  耗尽，地址入口只能避免潜在 UI 授权问题，不能清除已耗尽的系统状态。
- 两次 `ble_radio_recovery` 都在 0-5ms 内报告失败。代码复核发现旧实现忽略
  `SetStateAsync` 返回的 `RadioAccessStatus`，并在请求返回后立即读取 `State`；
  微软文档明确说明状态异步转换，应该先 `RequestAccessAsync`、检查 Allowed，
  再等待并复读最终状态。修复改为缓存 Allowed 权限、Off/On 各等待最多 5 秒，
  并记录权限、拒绝、API 失败和状态超时的结构化阶段日志。
- 修复后的独立真机探针先后尝试 `GetRadiosAsync` 与官方设备查询兜底
  （`GetDeviceSelector` → `DeviceInformation::FindAllAsync` → `FromIdAsync`），
  两条路径均返回 `0x80070008`。这证明当前故障是系统 WinRT 无线电枚举整体
  资源耗尽；在所有公开 API 都无法取得 Radio 对象后，才允许提示用户手动
  关开蓝牙，并明确说明其作用是清空系统蓝牙栈状态。
- 一次语音按住产生多个 F5 typematic DOWN，曾在约 1 秒内连续重置并触发四次
  attempt=0。Raw Input 现按物理 DOWN/UP 配对，每次按住只唤醒一次重连；所有
  重复沿仍刷新 F5 抑制宽限，不改变防粘键规则。

### 2026-09-12 22:12 恢复闭环

- 基于最新 `origin/main` 的安装版（source revision `17f0ced8`）仍稳定复现
  `device_from_address` 在 0-18ms 返回 `windows_resource_exhausted`；两轮应用内
  恢复均成功取得 `RadioAccessStatus::Allowed`，但主枚举与官方设备查询兜底都
  返回 `0x80070008`，因此无法取得 Radio 对象，符合人工介入边界。
- 用户手动关开蓝牙后，下一次设备创建在 1.786s 内成功；首次服务发现瞬时失败，
  再一轮 30s 退避后完整完成服务、特征、CCCD 与能力协商，连接阶段耗时 2.315s。
  这验证“约 4 分钟”并非单次挂起，而是资源耗尽期间的多轮指数退避。
- 系统栈恢复后显式运行 `live_radio_cycle_restores_the_radio_to_on`：自动
  Off→等待确认→保持 2s→On→等待确认在 3.48s 内 passed；运行中的安装版随后
  检测断连并约 6s 自动完成 GATT 重连。Radio 状态等待修复真机 passed。
- 本次只验证当前已选遥控器链路与主机无线电恢复；RC001、RC003 各自的僵死态
  自动恢复复现仍为 deferred，不扩大为双型号通过。

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
