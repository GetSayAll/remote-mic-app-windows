# 无线麦 SayAll Windows 日志规范

本规范把参考仓库中已验证的日志经验适配到 Windows。它适用于 BLE、HID、按键注入、WASAPI、输入法、Tauri IPC、更新器、安装器和诊断摘要。

## 基本原则

- 日志是功能的一部分。新增、修改或修复功能时，同时设计、实现并验证从入口到用户可见结果的完整链路。
- `received`、`decoded`、`enqueued`、`submitted` 只表示中间事实，不能冒充最终成功。
- 事实、未知状态和推测分开记录；无法观察第三方 App 内部状态时写 `unknown` 或 `diagnostic_boundary`，不得猜测。
- 同一次操作使用进程内短生命周期的 `operation_id`、`attempt_id`、`generation` 关联；一次操作只能有一个终态。
- 时间戳使用 UTC ISO 8601 毫秒精度；耗时使用单调时钟并输出整数毫秒。

## 行格式与诊断头

每行至少包含：

```text
2026-09-08T03:19:26.123Z pid=1234 ver=0.2.1 build=42 BLE CONNECT phase=completed result=passed
```

字段使用小写 `snake_case`，枚举使用稳定英文值。诊断摘要至少包含：

```text
diagnostic_schema=1
app_version=0.2.1
app_build=42
source_revision=<完整 SHA 或 unknown>
build_channel=stable|pr_preview|local|unknown
release_tag=v0.2.1|none|unknown
process_id=1234
process_architecture=x86_64|unknown
windows_version=<版本或 unknown>
windows_build=<build 或 unknown>
```

构建时写入 `source_revision`，运行时不得读取 Git 工作区。无法可靠取得的值写 `unknown`，不能省略字段。

正式应用默认写入 `%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`，不再要求用户
预先设置环境变量。受控测试仍可用 `SAYALL_GATT_LOG` 覆盖写入位置；手动覆盖也默认应放在
`%LOCALAPPDATA%\SayAll\Logs\` 下，只有需要隔离并发进程时才使用临时目录。若覆盖路径落在
`%LOCALAPPDATA%\SayAll` 根目录或它的非 `Logs` 子目录，启动时会归一到
`Logs\<原文件名>`，并在日志里记一条 `diagnostic_log path_normalized
reason=override_outside_logs`；产品目录之外的临时路径原样保留。日志正文不得打印实际文件
路径。`%LOCALAPPDATA%\SayAll` 根目录里的旧诊断文件是历史手动覆盖产物，不是应用默认路径。
应用启动、Tauri setup、前端入口、Vue 挂载和首次 IPC 读取必须
在用户看到主页面前后分别留痕，确保安装后白屏可以区分为宿主、WebView、脚本、
Vue 渲染或 IPC 阶段故障。

### 单一日志文件：主程序 + 提权助手（2026-10-01）

RC003 提权助手的日志**与主程序写进同一个文件**，报障收集只要求用户提供一份日志。
链路：主程序在桥接描述文件里发布 `log=<主程序诊断日志路径>` → 助手读到后把行写进
同一文件（行首为 UTC ISO 8601 毫秒时间戳 + `pid=<助手进程号> component=rc003-helper`）。
助手与该文件之间是"整行单次 `write_all` 追加"，主程序侧按行读取不受影响。

- 描述文件缺 `log=`（旧版主程序）→ 助手按约定回退 `<描述文件目录>\Logs\sayall-diagnostic.log`；
- 两者都不可写 → 回退助手自己的 `%ProgramData%\SayAll\rc003-helper\helper-task.log`，
  并在该文件里记 `[LOG] event=helper_local_fallback`（回退绝不静默）；
- 助手行**落盘前统一脱敏**：`%USERPROFILE%` / `%LOCALAPPDATA%`（含 `\AppData\Local` 形态）/
  `%APPDATA%` / `%ProgramData%`，另抹掉 `C:\Users\<用户名>`。

### 轮转与体积上限：单个文件不超过 10 MB（2026-10-08）

- **硬上限**：任何单个诊断日志文件不得超过 **10 MB**。写入前按「当前大小 + 本次长度 > 10 MB」
  判断；超过就把当前文件改名成 `<主名>-<UTC 时间戳>-p<进程号>.log`，再新建当前文件继续写。
- **保留份数**：轮转文件保留最近 **5 份**（不含当前文件），更旧的按文件名里的时间戳序列删除；
  总量上限约 60 MB。
- **两个写入方共用同一份"当前文件"**（见上一节）。实现上刻意"无长句柄"——每次追加都重新
  打开文件；任一进程轮转之后，另一方下一次写入自然落到新文件，不会继续写进已被改名的旧文件。
  代码在 `crates/sayall-windows/src/diagnostic_log.rs` 与 `hardware/RC003/helper/src/main.rs`
  两处（分属不同 workspace，无法共享代码）——**改上限/份数时两处必须一起改**。
- **心跳节流**：
  - 助手 `[HB]`：只在**状态变化**时或每 **30 s** 落一条（原来逐条落盘、实测约 2 条/秒，是日志膨胀主因）；
  - 主程序诊断心跳：**只在无线麦窗口处于前台时**记录，**10 s** 一条（2026-10-08 用户要求；
    前台判定只比较进程号，不记录窗口标题/类名等窗口身份）。应用在后台时没有周期状态行——
    这是有意取舍：后台的静默停摆改由事件行与 `capture_inactive` 一类判据行体现。
  - 心跳的用途是"证明进程还活着 + 保留趋势计数"，不是逐次留痕；高频重复相同日志本来就违反
    上一节「状态未变化时不得重复刷相同日志」。
- **体积实测（2026-10-08）**：治理前单机累计 768 MB、约 107 MB/天（助手 `[HB]` 逐条为主因）；
  治理后健康态实测约 **3 MB/天**（主程序心跳行 406 B、助手 `[HB]` 行 618 B）。按 10 MB/文件
  轮转，约每 3 天一个文件，保留 5 份 ⇒ 约 50 MB 封顶、约半个月历史。

## 必需事件链

每项功能按实际存在的边界记录：

1. 请求进入及来源分类；
2. 前置条件、权限和设备/端点选择；
3. 状态机接受或拒绝；
4. WinRT、WASAPI、SendInput 或 IPC 调用开始与返回；
5. 数据到达、解析、验证和路由；
6. 下游消费、提交和真正用户可见结果；
7. 取消、断连、超时、重试、恢复和迟到回调；
8. 唯一 `terminal_result` 及总耗时。

失败应带 `error_domain`、`error_code`、`reason` 和 `retryable=true|false`。状态未变化时不得重复刷相同日志；高频 HID、音频包和轮询只记录首尾、聚合计数、字节/帧数、丢弃数量和耗时。

### 向导连接 / 按键卡点快照（2026-11-21）

首次设置向导的连接步骤不能用"看不到设备"或"按键没反应"作为唯一证据。该步骤在进入、
连接阶段跳变、Raw Input 阶段跳变、匹配接口数/重连次数/错误位/型号变化时落
`frontend event=onboarding phase=remote_observed reason=remote_state step=remote detail=<token>`，
30 秒心跳与离开步骤时的 `reason=remote_state_exit` 再带 Raw Input 报文/语义边沿/丢弃计数。token 由
`src/onboarding/remote-state.ts` 生成，只含阶段代码与计数（≤64 字节），不含设备名、
地址、HID 路径或错误原文；前端尚未拿到 runtime 快照时固定写 `snap_0`，不得把默认值
当成真实 idle/stopped；字段组合超过 64 字节时显式写 `token_too_long`，不依赖 Rust 侧
的 `invalid` 兜底。Rust 侧扫描另落 `ble_scan phase=requested|completed`（成功给 `found_count`，另落
`ble_scan filter total_paired_count/supported_count`，用于区分"没有配对设备"与
"有配对设备但名称不在支持列表"），失败只给 `error_domain` + `error_code`；
连接与 Raw Input 的详细 stage 仍由既有
`ble_connect*` / `raw_input*` 行提供。现场只取一份 `sayall-diagnostic.log`，即可区分
「没连上」「连上但无 HID 按键通道」「通道在位但按键没有到达」。
### 向导语音测试慢轮询与撕裂结束帧（2026-11-25）

1.9.7 现场（`Bugs/2026-11-25-onboarding-doubao-voice-bar-report-layer-inactive.md`）：
向导第⑥步的 200 ms 轮询在会话结束后 **23 s** 才看到 idle，日志里只有终态
`detail=d26160_s23280_q0_drain0`，无法区分"主线程排队""快照子系统阻塞"与"撕裂帧"。
本次补三类只读日志：

- 前端轮询往返：`frontend event=onboarding phase=poll result=unknown reason=slow_snapshot step=voice_test detail=rt_<ms>`。
  只在一次 `getRuntimeSnapshot` 往返 ≥ 1 s 时落一行；它回答"主线程/IPC 是否被卡住"。
- Rust 快照分段：`runtime_snapshot phase=completed terminal_result=slow elapsed_ms=<总> connection_ms=<段> audio_ms=<段> raw_input_ms=<段> send_input_ms=<段> button_mapping_ms=<段> voice_priority_ms=<段>`。
  只在一次 `PlatformSnapshot::snapshot()` ≥ 1 s 时落一行；它回答"卡在哪个子系统"。
  前端 `renderer` 读取与 Rust 命令执行是两个事实，必须分两行记录。
- 结束方向撕裂帧：`frontend event=onboarding phase=voice_attempt result=unknown reason=torn_end_frame step=voice_test detail=vs_<voiceState>_ap_<audioPhase>_s<submitted>_q<queued>_d<decoded>`。
  每次 attempt 最多一条；命中判据 = `voiceState=idle` + `audioPhase=streaming|draining` +
  `audioLastError=null` + `queuedSamples=0`（正常结束时后端先让 audio 回到 ready，再让
  voiceState 回到 idle，因此这个组合只可能是撕裂）。相应终态的 `detail` 追加 `_torn1`。
- 捕获生效状态（第④/⑤/⑥步）：`frontend event=onboarding phase=capture result=passed|failed reason=read|changed|read_failed step=<full_key|voice_tool|voice_test> detail=health_<状态>_reason_<原因>`。
  进入步骤时 `read`，5 s 轮询只在变化时 `changed`，读失败只落一次 `read_failed`。
- 助手从未连上：`rc003 feature=enhanced-capture action=capture_unavailable phase=completed terminal_result=failed reason=helper_not_connected accepts=<累计接受> rounds=<连续触发轮> owned_usages=0 …`。
  `accepts=0` 表示助手一次都没握手；`reason=helper_startup_failing`（`accepts>0`）是"连上就断"的另一形态。两者都让界面从 `starting` 进入 `inactive`。

隐私：以上字段只有状态 token、计数与毫秒；不含窗口标题、设备身份、输入文字或路径。

### 向导主按钮可见性（2026-11-24）

- 步骤进入（DOM 更新后）、resize 去抖 250ms、点击主按钮时各落一条
  `frontend event=onboarding phase=action result=passed reason=layout_snapshot`；
  `detail=<step|resize|click>_vp<W>x<H>_dpr<d>_mode_<single|two>_ovf<0|1>_btn<0|1>_hit<0|1>_en<0|1>`，
  全部为稳定 token（≤64 字符）。
- `btn1` = 主按钮完整落在视口内；`hit1` = 按钮中心 `elementFromPoint` 命中按钮；
  `ovf1` = 文档比可视宽度更宽。应用无法知道用户是否“看见”，只记录按钮在视口内且可点。
- 复制诊断的 `Layout:` 行给同样事实的可读版本；不做逐帧或滚动记录。

### BLE 适配器与链路建立诊断（2026-11-24）

issue #104 的 1.9.5 日志能证明“配对记录存在、服务发现失败”，但不能回答
“Windows 在尝试期间是否曾经连上过”。连接路径补充三类只读日志（这三类日志
本身不改变重连或 GATT 决策；后续的服务路径验证见本节末尾）：

- `ble_adapter phase=observed trigger=startup|failure_streak attempt=none|N low_energy=... central_role=... peripheral_role=... classic=... radio_state=... bus_type=... power_state=...`：
  进程启动时，以及连续失败第 5 次（约 60 秒）之后每 10 次失败，由独立短生命周期
  线程查询 `BluetoothAdapter` 能力、Radio 状态和适配器 PnP 事实。`trigger=startup`
  时 `attempt=none`；`trigger=failure_streak` 时 `attempt=N` 与同次 `ble_adv` 的
  `attempt` 一致。`bus_type=usb|internal|bth_stack|sdio|unknown` 来自
  `DEVPKEY_Device_EnumeratorName`；`power_state=d0|d1|d2|d3|unknown` 来自
  `DEVPKEY_Device_PowerData.PD_MostRecentPowerState`（最近一次电源状态，不是实时
  瞬态）。查询失败写 `terminal_result=unavailable ... reason=adapter_unavailable`，
  也带 `trigger`/`attempt`。不记录适配器名称、地址、设备实例 ID、驱动路径。
- `ble_device phase=access ... access_status=... is_paired=... is_present=... is_connected=... is_enabled=...`：
  每次连接尝试在创建设备对象后、服务发现前记录访问/配对/在场快照；只写稳定 token。
  `is_present`/`is_connected` 来自 AEP 属性，读不到时写 `unknown`。
- `ble_link phase=observer terminal_result=registered|released|release_failed ...` 与
  `ble_link phase=observed status=... connection_generation=... attempt=... elapsed_ms=...`：
  在设备对象创建后立即注册只调用 `gatt_note` 的早期 `ConnectionStatusChanged`
  观察器，成功或失败路径都成对释放；它不发送 worker 消息，不触发清理/重连。
  这样失败尝试里也能看到 Windows 是否短暂 `connected`，而不是只在失败瞬间
  读到一次 `link_status`。

这些行只用于区分“适配器不支持 BLE 主机角色 / 配对记录存在但设备不在场 /
权限被拒 / 链路短暂建立后 GATT 失败”，不能替代 `ble_service_path` 的路径结果。

2026-11-24 的 1.9.6 日志进一步证明：`device_uuid_cached` 可以返回
`service_count=1`，但随后的 transmit 特征查询仍是 `gatt_unreachable`。因此
“拿到服务对象”不再算路径成功；每条路径必须再解析 transmit 特征：

- `ble_service_path phase=service_ready ...`：该路径已返回服务对象，记录
  `gatt_status`、`service_count`、`elapsed_ms` 和当时链路状态。
- `ble_service_path phase=characteristic_probe ... terminal_result=passed|failed ...`
  ：对服务对象查询 transmit 特征的结果；失败带 `error_code`、`gatt_status`、
  `link_status_after`，然后继续下一条服务路径（除非超时或链路已断开）。
- `ble_service_path phase=completed ... characteristic_probe=passed|failed`：
  路径最终结果。只有 `characteristic_probe=passed` 的路径才会被选中。
- 若路径失败时链路仍 `connected`，尤其 `gatt_unreachable`，继续尝试剩余路径；
  若超时或链路 `disconnected`，剩余路径落 `skipped`，不拖长每次重连。

对应的 `characteristic_transmit` 查询现在属于服务路径验证；日志看
`ble_service_path ... characteristic_probe`，不再依赖单独的
`ble_connect_stage stage=characteristic_transmit`。

### BLE 空口探测与 Raw Input 枚举统计（2026-11-25）

issue #104 的 1.9.7 现场已能证明「配对记录在、GATT 不可达、HID 接口不在场」，
但仍无法区分「遥控器不在空口」「在空口但不可连接」「在空口且可连接，失败在
Windows 链路/ATT/配对层」。连接失败段补三类只读日志；都不改变重连、GATT 或
按键决策：

- `ble_adv phase=requested|completed ...`：连续失败第 5 次（约 60 秒）做第一次
  被动扫描，之后每 10 次失败一次。窗口 2.5 秒，`scan_mode=passive`。
  - `requested`：`attempt`、`window_ms`、`trigger=failure_streak`。
  - `completed`：`reason=target_seen|target_absent|no_advertisements`、
    `seen=yes|no`、`target_advertisements`、`total_advertisements`、
    `connectable=yes|no|unknown`、`advertisement_type`、`rssi_dbm`、
    `watcher_status`、`elapsed_ms`。
  - 判读：`seen=no total_advertisements=0` = 适配器/扫描没收到任何广告；
    `seen=no total_advertisements>0` = 无线电工作但目标没广播（休眠/没电/
    超出范围/被其它主机占用）；`seen=yes connectable=no` = 目标在广播但不可
    连接（通常已被另一台主机占用）；`seen=yes connectable=yes` = 目标可连接，
    失败在 Windows 链路/ATT/配对层。
  - **不记录**目标地址、名称、厂商数据或服务数据；只比较调用方传入的地址。
    探测器启动/订阅/扫描模式失败落 `terminal_result=failed` 与 `error_code`；
    watcher 被系统中止时即使已看到广告也落 `terminal_result=failed
    error_code=watcher_aborted`，避免把中止当成“目标没广播”。整个探测在独立
    短生命周期线程执行，外层 4 秒上限；超时落 `error_code=probe_timeout` 后
    继续原重连流程，不让僵死的 BLE 栈把工作线程一起拖住。
- `raw_input scan phase=completed ...`：监听器启动时记录一次，之后只在枚举
  签名（总数/键盘数/HID 数/名称读取失败数/匹配数/目标地址是否已知）变化时
  记录，设备稳定时不刷屏。
  - 字段：`total_devices`、`keyboard_devices`、`hid_devices`、`name_failures`、
    `remote_paths`、`matched_device_count`、`target_known`、
    `reason=matched|ambiguous|no_devices|no_remote_hid|target_filtered`。
  - 判读：`reason=no_remote_hid` = 系统有 HID 但遥控器接口不在；
    `reason=target_filtered` = 遥控器接口在，但被“应用正在用的那支”地址过滤掉
    （通常是连错了另一支）。**不记录**设备路径、名称、VID/PID。
- `ble_link phase=changed ...` 追加 `session_ms`（连上到断开的毫秒数，未知写
  `unknown`）与 `phase`（`ready|streaming|draining|...`），用于区分“连上几秒
  就掉”和“稳定一段时间后偶发掉链”，以及掉链是否紧跟在语音结束之后。

这些行只用于分流链路层原因；不能替代 `ble_service_path` 的 GATT 路径结果。

### BLE 适配器 PnP 事实与采集脚本（2026-11-25）

两份 1.9.7 现场日志都来自「台式机 + USB 蓝牙适配器」环境；应用日志能证明链路不在，
但看不到适配器总线类型、最近电源状态、驱动版本、蓝牙事件和系统电源事件。为此：

- `ble_adapter` 追加 `bus_type` 与 `power_state`：`DeviceId` 是设备接口路径，应用只在
  本地把它转换成 PnP 实例 ID 后查询 `DEVPKEY_Device_EnumeratorName` 与
  `DEVPKEY_Device_PowerData`；原始路径和实例 ID 不落日志。`CM_Get_DevNode_PropertyW`
  第一次返回 `CR_BUFFER_SMALL`（26）是正常的两段查询路径，不是失败。
- `scripts/collect-bluetooth-adapter.ps1`：普通用户权限运行，采集物理适配器
  （USB/PCI/ACPI/SDIO 枚举器）、`MSPower_DeviceEnable`（USB 选择性挂起设置）、
  驱动版本、蓝牙服务状态，以及 System 日志中 BTH/Bluetooth 事件和 Kernel-Power/
  Kernel-PnP 事件的**按 provider/id 计数**。不读取/输出设备名称、实例 ID、蓝牙地址、
  驱动路径、用户路径或事件原文。
  - 用法：`powershell -NoProfile -ExecutionPolicy Bypass -File scripts/collect-bluetooth-adapter.ps1`
    （可加 `-OutFile adapter-facts.txt`、`-EventWindowDays 14`）。
  - 判读：`adapter_count=0` = 没有枚举到物理适配器；`selective_suspend=true` = 允许
    Windows 挂起该 USB 设备（不等于已经挂起）；`power_state=d2/d3` 配合
    `bluetooth_event`/`pnp_event` 计数，才支持「适配器被挂起/重置」的方向；
    `driver_version` 用于识别旧驱动；`bth_stack` 表示 WinRT 返回的是 BTH 栈节点，
    不能据此推断内置或外置。
  - **`bluetooth_event`/`pnp_event`/`power_event` 是系统级计数，不是适配器专属**；
    必须与 `sayall-diagnostic.log` 的时间戳对齐，不能单独归因到蓝牙适配器。
  - 不覆盖：USB Hub/端口的供电与链路训练、2.4 GHz 干扰、xHCI 控制器内部状态、
    厂商驱动内部日志。需要这些证据时走 Windows 事件日志、USBView 或厂商工具，
    并仍然只保留分类与计数。

## 隐私红线

不得记录语音或转写正文、用户输入、剪贴板、用户名/邮箱/手机号、完整路径、窗口标题、蓝牙 MAC/UUID/序列号、HID 路径、IP、Token/API Key/密码/私钥/证书、第三方 App 私有状态或原始音频字节。不要用敏感值的稳定哈希替代脱敏。允许记录稳定产品分类、布尔结果、错误 domain/code、采样率、帧数、耗时和脱敏计数。

生产日志不得记录原始音频包；GATT 音频只在会话终态聚合帧数、样本数、丢弃数
和耗时。音频端点只记录 `virtual_cable|bluetooth|other` 分类及计数，不记录名称、
端点 ID 或蓝牙身份。

## 评审清单

- [ ] 入口、前置检查、跨组件交接和最终结果均有日志。
- [ ] 成功、失败、取消、超时、重试、恢复和 stale callback 可区分。
- [ ] 异步提交与真实完成分开记录，且一次操作只有一个终态。
- [ ] 日志不包含个人信息、设备身份、路径、凭据、用户内容或第三方私有数据。
- [ ] 已覆盖自动化、Windows 真机和第三方工具的验证边界。
