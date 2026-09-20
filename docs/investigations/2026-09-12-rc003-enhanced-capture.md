# RC003 增强采集实现方案

> 范围：当前已实现的本地实验版本，不是未落地的设计提案。
> 实现基线：`ec81f301d2357e762492876e92735a34aa764070`，安装版本 `0.2.7`。
> 实证日期：2026-09-12 UTC / 2026-09-13 Asia/Shanghai。
> 本次只写文档，不修改、重装或重启正在运行的 SayAll。

## 1. 目标与结论

本方案让 SayAll 获取 RC003 的返回、音量加、音量减三个实体键的状态，接入已有单击、双击、长按映射。

实现方式是：**普通权限主程序显式启动独立管理员 Helper；Helper 在目标 Windows 蓝牙 HID 宿主中加载 Frida Gadget，读取有限的原始按键状态，再交回主程序执行映射。**

它不是修改遥控器固件，也不替换遥控器驱动。不安装自制内核过滤驱动，不把三个键转换为 F13/F14/F15。

| 项目 | 当前边界 |
| --- | --- |
| 支持目标 | 本机 RC003；硬件身份限定当前实现支持的 REV 00A4 |
| 新增输入 | `back`、`volume_up`、`volume_down` |
| 基础语音 | 沿用 BLE/ATVV、音频及语音快捷键，不依赖 Helper |
| 权限 | 主程序普通权限，只有显式开启的 Helper 提权 |
| 启动 | 默认关闭，不随登录或蓝牙重连自动开启增强采集 |
| 来源 | 本机实际为 `proxy_unverified`，不宣称完整逐设备归因 |
| 已证实 | 安装、启动、建立采集会话、三键逐一确认与设置解锁 |
| 未验收 | 配置后的实际动作、闲置首按、增强采集下的完整语音回归等 |

“实验信号已确认”只证明本次会话观察到该键完整按下、松开，不等于动作已经配置或所有场景通过。

## 2. 路线选择

原有 Windows 按键路径在当前环境中不能为三个键提供可用事件，但独立宿主采集已经观察到它们的报文。因此在宿主完成 I/O 时补充读取，而不是继续猜普通键盘虚拟键码。

| 路线 | 做法 | 主要代价 |
| --- | --- | --- |
| 设备专属内核过滤驱动 | 转换前改写 HID usage，再从 Raw Input 获取 F13/F14/F15 | 内核驱动部署、兼容性、签名 |
| 本文用户态增强采集 | Gadget 在用户态 HID 宿主观察完成态报文 | 管理员权限、进程注入、反作弊与宿主稳定性风险 |

避开的是自制内核驱动签名问题，不是所有 Windows 安全要求。安全策略仍可能阻止程序运行，本地构建也不等于正式代码签名。

宿主内部 I/O 形态不是可承诺跨 Windows 版本稳定的输入接口。本机通过不能推广为所有 Windows、固件或 RC001 均可用。

## 3. 架构与职责

```text
RC003 遥控器
  |
  +--> Windows 原有路径 --> SayAll 普通按键 / 基础语音
  |
  +--> 目标 WUDFHost.exe
         Frida Gadget + hid_tap.js
         观察已完成 I/O，只提取三个目标键的当前状态
               |
               | 通道 B：认证后的本机回环 TCP
               v
         sayall-rc003-helper.exe [管理员权限]
         验证设备、宿主、策略、存活状态
               |
               | 通道 A：认证后的本机回环 TCP
               v
         UserHidRuntime [SayAll 普通权限]
         验证会话、协议、租约、输入边界
               |
               v
         UserHidSession：逐键确认、去重、生成 DOWN/UP
               |
               v
         原有映射引擎：单击 / 双击 / 长按 / 连发
               |
               v
         原有动作执行器：快捷键、鼠标、应用等
```

Helper 只采集，不执行用户动作；宿主脚本不改写报文、参数或返回值；主程序才负责手势与动作。

两个通道有各自的每次运行随机口令，只监听 `127.0.0.1`。Gadget 主动连接 Helper，不在宿主内监听端口，不自动重连。

## 4. 启动与权限流程

1. 用户开启“RC003 增强采集”，阅读风险提示并同意。
2. 前端调用 `start_user_hid`，传入 `acknowledgeRisk: true`，后端再次检查确认。
3. 主程序从固定资源目录定位 `rc003-helper/sayall-rc003-helper.exe`，不接受前端指定任意程序。
4. 检查 Raw Input 已就绪且匹配设备数为 1；所选 BLE 对端必须是已就绪 RC003。
5. 创建带连接纪元的 `UserHidPermit`、临时回环监听器及 256 位随机口令。
6. 使用 `ShellExecuteExW` 的 `runas` 显式启动隐藏 Helper，由 Windows UAC 确认权限。
7. 验证 Helper 的 `hello`、协议版本和口令，再发送当前选中的 BLE 对端标识。
8. Helper 检查管理员状态、实验内核过滤驱动冲突、唯一活动 RC003 和宿主共享情况。
9. HID 服务须与所选对端完全匹配，不接受任意 PID 或只按型号猜测。
10. 获取单实例锁，只启用自身令牌已有的 `SeDebugPrivilege`，验证系统 WUDFHost 微软签名和保护策略。
11. 确认主程序仍在线且未要求停止，再准备受保护运行目录、校验 Gadget 哈希和已有模块。
12. 加载或显式复用本方案的 Gadget；通道 B 验证独立口令与当前目标宿主 PID。
13. 脚本就绪后 Helper 发出 `ready`，主程序建立映射会话。
14. 用户将三个键分别按下、松开一次，逐键解锁设置。

UAC 未完成时主程序仍可撤销许可，但不会强行关闭系统 UAC 窗口。后续连接仍需通过有效会话检查。

```text
stopped -> starting -> ready
                 |       |
                 v       v
               failed  stopping -> stopped / failed
```

- `available`：资源目录存在 Helper，不代表启用或获权。
- `ready`：采集通道就绪，不代表三键都已确认。
- `confirmedUserHidButtons`：本次会话已确认的目标键列表。
- `cleanupConfirmed`：清理确认，不代表 DLL 已从内存卸载。
- 日志里的 `confirming`、`confirmed` 是映射会话事件，不是新增前端生命周期枚举。

## 5. 原始状态识别

`hid_tap.js` 在宿主中观察 `ntdll!NtDeviceIoControlFile`。

| 校验项 | 当前实现 |
| --- | --- |
| I/O 控制编号 | `0x80018483` |
| 声明输出长度 | 必须恰好为 9 字节 |
| 成功状态 | 函数返回成功且 `IO_STATUS_BLOCK.Status` 为成功 |
| Pending I/O | `STATUS_PENDING` 不读缓冲区，也不延后持有读取 |
| 完成长度 | 只接受 `Information=0` 或 `9` |
| 报文头 | 必须为 `01 00 00` |
| 目标键 | `0x00F1 -> back`；`0x0080 -> volume_up`；`0x0081 -> volume_down` |
| usage 字段 | 偏移 3、5、7 的小端 16 位值，只保留三种目标键 |

接受 `Information=0` 是本机实测后的兼容处理：宿主返回成功、声明长度为 9、报告头有效。不是不检查长度就读取，也不是跨版本保证。

每次 I/O 都重新查询句柄当前的 NT 对象，避免句柄复用后继续使用过期归因。

- 对象名与选中设备的直接名称匹配，标为 `rc003`。
- 只匹配 UMDF 控制代理，标为 `proxy_unverified`，不升级为直接来源。
- 本机实际为第二种。唯一活动 HID 服务、实体测试和普通键盘对照仍不能证明所有代理对象、设备组合均隔离。

同一流比较三键集合，相同集合不重复发送。其他 usage、原始缓冲区、语音数据和设备路径不作为按键事件转发。

## 6. 进程间协议与时限

### 6.1 通道 A：主程序与 Helper

ASCII 编码，每行一个 JSON。Helper 发出的所有帧共用递增序号，包括诊断、心跳，主程序检查连续性。

| 方向 | `kind` | 关键字段与用途 |
| --- | --- | --- |
| Helper -> 主程序 | `hello` | `seq`、`token`、`protocol: 1`；认证 |
| 主程序 -> Helper | `accepted` | `peer`；认证后才发所选对端 |
| 主程序 -> Helper | `lease` | 允许本会话继续运行 |
| 主程序 -> Helper | `stop` | 协作停止，不是强杀 |
| Helper -> 主程序 | `ready` | `seq`、`scope`；通道就绪 |
| Helper -> 主程序 | `state` | `seq`、`stream`、`scope`、`active` |
| Helper -> 主程序 | `heartbeat` | `seq`；健康消息 |
| Helper -> 主程序 | `diagnostic` | `seq`、`event`、`result`；受限分类 |
| Helper -> 主程序 | `stopped` | `seq`、`reason`、`cleanup` |

状态示例，序号仅为示意：

```json
{"kind":"state","seq":12,"stream":1,"scope":"proxy_unverified","active":["back"]}
{"kind":"state","seq":13,"stream":1,"scope":"proxy_unverified","active":[]}
```

前一条表示返回按住，后一条表示三键集合为空。这是状态，不是执行命令。

Rust 端拒绝未知字段、序号异常和不合约消息。Python 控制端按约定的 `kind` 处理，但目前不是对称的全字段严格校验实现；握手合包还有第 12 节的已复现问题。

口令防止未持有本次口令的连接随意接入，不是传输加密或操作系统级进程身份认证，不抵御同权限恶意程序或管理员。不能将口令和真实对端写入日志、文档。

### 6.2 通道 B：Helper 与 Gadget

Helper 生成独立口令并创建临时回环监听器。Gadget 只连接一次，Helper 同时校验 `hello` 口令与目标宿主 PID。

认证后启动脚本，通过 `state`、`stats`、`ready`、`stopped` 等消息传递有限状态和健康信息。EOF、异常、停止和租约到期触发撤钩，无自动重新附加。

### 6.3 当前参数

| 项目 | 当前值及解释 |
| --- | --- |
| 主程序 -> Helper 续期 | 每 1 秒 |
| Helper 等主程序续期 | 超过 5 秒停止 |
| Helper -> 宿主续期 | 运行时每 1 秒 |
| 宿主独立租约 | 15 秒未续期撤钩；停止后不能用续期复活 |
| 宿主统计心跳 | 每 5 秒 |
| Helper 等宿主健康消息 | 超过 12 秒停止 |
| 主程序运行期超时 | `ready` 后超过 4 秒无消息，撤销许可 |
| 设备、宿主复查 | Helper 每 2 秒重新发现比对 |
| 主程序启动等待 | 启动调用返回后约 40 秒，不含用户停留 UAC 时间 |
| 主程序等停止确认 | 最长约 7 秒，未确认不强杀宿主 |
| Rust 通道 A 缓冲区 | 8192 字节，单次解析最多 64 帧 |
| Python 控制缓冲区 | 4096 字节 |
| 脚本待发送 / Helper 输入队列 | 各最多 128 项 |
| 源流 | 脚本最多编号 16 个；映射会话只接受同一 `stream + scope` |
| 目标状态频率 | 每 1 秒窗口最多 256 个，超限停止 |
| Rust 引擎排队时效 | 收到后排队超过 2 秒拒绝，不是物理采集全链路延迟保证 |
| 目标三键集合连续非空 | 达 10 秒停止，防漏松开后持续执行 |

参数仅作用于实验三键，不改变语音键时序。Rust 沿用已有共享映射消息队列，不能宣称全链路有界。定时器和租约也不是实时系统级截止保证。

## 7. 接入既有映射

### 7.1 连接纪元与许可

`UserHidPermit` 保存会话编号、可撤销原子标志和 BLE 连接纪元。连接失效、对端变化、停止或输入验证失败都会撤销许可。

动作执行前再次检查许可，阻止已排队的双击、长按、连发在断连后继续执行。`Ready`、`Streaming`、`Draining` 均为允许的 BLE 阶段，正常语音开始和排空本身不被当成换设备。

### 7.2 第一组完整按下、松开只确认

每个目标键分别维护“当前按住”和“本会话已确认”集合：

```text
第一次 DOWN -> 只记录按住，不执行映射
第一次 UP   -> 加入已确认集合，开放该键设置
后续 DOWN/UP -> 交给原有手势识别器
重复相同状态 -> 忽略，不重复生成边沿
```

按集合差异生成边沿。换流、换来源、异常序号、无效键或过期输入导致会话失效，不猜测来源后继续执行。

### 7.3 与驱动路径独立

- 用户态使用 `confirmedUserHidButtons`，驱动仍使用 `confirmedFilterButtons`。
- UI 分别显示“实验信号已确认”和“驱动信号已确认”。
- 不构造 `FilteredKeyEdge`，不制造 F13/F14/F15，不扩大普通键盘全局吞键范围。
- 沿用原有单击、双击、长按、连发时序，Helper 不另设动作引擎。
- 已有映射保留在原配置文件中；能力暂时消失时只禁用，不覆盖、删除配置。

## 8. 操作、停止与退出

| 操作 | 影响 |
| --- | --- |
| 开启、同意风险、确认 UAC | 创建新的采集会话 |
| 三键各按下、松开一次 | 逐键确认，解锁设置 |
| 点“未设置”绑定并保存 | 保存用户动作；确认本身不替用户选动作 |
| 切换应用内页面 | 不停止全局采集 |
| 主窗口右上角 X | 当前代码隐藏到托盘，保留后台采集 |
| 关闭增强开关 | 立即撤销许可，请求停止，清除本会话确认 |
| 停止 Raw Input 监听 | 同时停止增强采集 |
| 托盘“退出” | 真正退出，正常退出回调请求停止 Helper |
| 应用内更新退出 | `on_before_exit` 先请求停止 Helper，再断开 BLE |
| 断连、宿主变化、异常、超时 | 撤销许可并清理，不自动重开采集 |
| 重新启动或重新连接后 | 用户重新明确开启，并重新确认三键 |

先撤销映射许可、清理手势与按住状态，再等待协作停止确认，不等清理结束才禁止动作。

**停止采集不等于卸载 Gadget DLL。** 钩子可撤除，但 DLL、文件监视器可能留在宿主中，直到 Windows 自行回收进程。不能用 `cleanup=true` 宣称内存已无注入组件，也不能强杀 WUDFHost 当日常清理。

## 9. 安全、兼容性与来源

### 9.1 已有措施

- 不安装自制内核驱动，不修改测试签名、Secure Boot 或 Defender 排除项。
- 不新增开机服务或定时任务，不把基础语音变为管理员程序。
- 只使用所发现、所选中的 RC003 及系统宿主，拒绝任意目标进程参数。
- 调试权限仅在 Helper 自身令牌启用，结束时恢复，不修改账户权利分配。
- 官方 Gadget 压缩包与解压后 DLL 均使用固定 SHA-256 校验。
- 运行文件位于 `%PROGRAMDATA%\SayAllRc003HidDiagnostic\host-<pid>-<creation-time>\`，按宿主启动身份隔离，拒绝相关路径的重解析点。
- 运行目录仅管理员/SYSTEM 可写，LocalService 只有读取、执行权限。
- 先检查宿主已有模块，再改写会触发重新加载的脚本，避免先加载后检查冲突。
- 日志使用分类与聚合状态，不记录语音内容、原始输入缓冲区、真实设备身份或认证口令。

### 9.2 不能作出的承诺

自己编译不等于没有风险。Frida 在 Windows 宿主内执行代码，缺陷可能影响蓝牙；安全软件和反作弊可能拦截或发生冲突。不要为使用本功能关闭保护，不要与游戏反作弊环境并用。

当前没有正式 Authenticode 签名。安装时做过资源哈希比对，但启动 Helper 时并未对整个目录执行完整签名/防篡改认证；Gadget 固定哈希不等于所有脚本和 Python 运行时均不可被替换。

受保护运行目录降低非管理员篡改风险，不是抵御管理员的边界。`proxy_unverified` 必须保持可见，不能改名为“已通过设备隔离认证”。

### 9.3 来源与许可

协议识别、句柄归因、DLL 加载方式参考 `leowzz/axonkey` 固定提交：

```text
a0451ec59cbcb48063f8dc1d7104f817d3cbaff6
tools/keycode-demo/rc003_hid/
frida_hid_tap_runtime.py / frida_hid_tap_injector.py / frida_compat.py
```

没有安装 Axonkey 整套软件或 Interception。Helper 目录保留 GPL-3.0-only 来源、完整许可，使用未修改的官方 Gadget，打包保留对应源码及 Frida/Python/PyInstaller 许可。完整归属见仓库 `ATTRIBUTION.md`。

依赖官方文档地址：

```text
https://frida.re/docs/gadget/
https://frida.re/docs/javascript-api/
https://pyinstaller.org/en/v6.16.0/usage.html
```

## 10. 代码位置与构建

以下路径相对于仓库根目录。

| 文件 | 职责 |
| --- | --- |
| `crates/sayall-windows/src/rc003_user_hid.rs` | 快照、连接纪元许可、校验、逐键确认与去重 |
| `crates/sayall-windows/src/rc003_user_hid_windows.rs` | 提权启动、通道 A、超时、输入限制、清理 |
| `crates/sayall-windows/src/button_mapping.rs` | 新引擎消息、复用手势与动作执行 |
| `crates/sayall-windows/src/ble.rs` | 对端与连接阶段传递给会话许可 |
| `crates/sayall-windows/src/raw_input.rs` | 用户态三键确认列表 |
| `crates/sayall-windows/src/lib.rs` | 平台持有运行时，监听停止时联动清理 |
| `src-tauri/src/lib.rs` | 三个 IPC 命令、资源路径、退出与隐藏窗口 |
| `src-tauri/src/platform.rs` | 平台运行时接口、仿真边界 |
| `src-tauri/src/updater.rs` | 更新器退出前停止 |
| `src/lib/bridge.ts` | 类型、IPC 封装、逐键能力判断 |
| `src/pages/ButtonsPage.vue` | 开关、风险确认、状态、设置格子 |
| `Testing/rc003-user-hid/sayall_bridge.py` | 提权入口、通道 A 客户端、健康复查与转发 |
| `Testing/rc003-user-hid/capture.py` | 设备发现、权限、宿主验证、独立诊断 |
| `Testing/rc003-user-hid/gadget_backend.py` | 保护文件、校验/加载 Gadget、通道 B 服务端 |
| `Testing/rc003-user-hid/hid_tap.js` | 只读 I/O 观察、三键解析、去重、宿主租约 |
| `Testing/rc003-user-hid/gadget_adapter.js` | Gadget 一次性连接、消息传输、退出清理 |
| `Testing/rc003-user-hid/build-helper.ps1` | 独立环境及 PyInstaller onedir 打包 |
| `Testing/rc003-local-bundle.json` | 本地 0.2.7 包含 Helper 的 Tauri 覆盖配置 |

需要 Windows x64、Python 3.12、项目 Rust/MSVC 和 Node/pnpm 工具链。固定依赖不等于要求升级到“最新版”。

```powershell
# 仓库根目录；准备环境和下载，不附加宿主。
.\Testing\rc003-user-hid\prepare.ps1 -Gadget
.\Testing\rc003-user-hid\build-helper.ps1 -Prepare

# 只有该本地覆盖配置包含增强组件。
pnpm tauri build --bundles nsis --config Testing/rc003-local-bundle.json --ci --ignore-version-mismatches
```

`prepare.ps1` 需要可解析的 x64 `python.exe`，也可用 `-Python` 指定。构建脚本使用 `py.exe -3.12` 建立独立环境，按 `requirements-build.txt` 的固定版本与 SHA-256 安装依赖。

Helper 输出到 `src-tauri/binaries/rc003-helper/`，安装包位于 `target/release/bundle/nsis/`。打包后运行不要求用户另外安装系统 Python。

本地覆盖配置不生成 updater 发布资产，不修改正式 updater 公钥与验签规则。普通构建未包含 Helper 时应显示该能力不可用，不能自动将本地测试包发布出去。

安装前正常退出 SayAll、备份设置，不强杀活动 BLE 会话。此次升级保留了旧程序和配置备份，升级后的两份 JSON 与升级前逐字节相同。

## 11. 验证与证据

`passed` 指实际执行并观察通过；`deferred` 指相应场景尚未验证，不可用编译成功替代。

| 验证项 | 结果与边界 |
| --- | --- |
| Rust 工作区 | 175 项通过、4 项忽略，含会话和映射夹具 |
| 前端单元测试 | 87 项通过，含风险确认、逐键能力、驱动/用户态隔离 |
| Python/脚本离线测试 | 34 项通过，不附加宿主、不执行真实动作 |
| 前端构建和截图 | 构建通过；4 张截图检查，桌面 1280/1029 宽和 390 宽确认弹窗，使用模拟 IPC |
| 独立实体三键采集 | 用户确认每键 3 组完整按下/松开，共 18 个边沿 |
| 普通键盘输入/删除对照 | 受控测试未误收目标键，媒体键未单独确认 |
| 真实 Helper 生命周期 | 20.14 秒续期与停止确认通过；另一次 5.03 秒后关闭主程序连接，Helper 退出 |
| 本地安装 | NSIS 返回 0、版本 0.2.7，74 个 Helper 资源文件逐个哈希一致 |
| 启动 | 主程序正常响应，RC003 重连、单设备 Raw Input 就绪、缓存电量 99% |
| 生产/仿真隔离 | 不含测试仿真命令及前端 |
| 安装态三键确认 | `passed`：日志依次确认 1、2、3 个键，用户截图三键均确认 |
| 安装态三键动作 | `deferred`：截图三键动作仍为“未设置” |
| 闲置首按、保持/连发、增强下语音 | `deferred` |
| 多设备、真实断连/睡眠恢复、RC001 | `deferred`；当前不向 RC001 开放此功能 |

安装态日志时间：

```text
2026-09-12T23:17:08.791Z  ready
2026-09-12T23:17:11.030Z  confirmed key_count=1
2026-09-12T23:17:17.921Z  confirmed key_count=2
2026-09-12T23:17:22.494Z  confirmed key_count=3
```

对应北京时间 2026-09-13 07:17。证据为安装态脱敏日志及用户“确认好了”的截图，不把截图中的个人应用绑定复制进仓库。

20 秒 Helper 生命周期测试对应较早打包产物；安装态三键确认对应随后补充源码与许可的最终包。不能混用两个包的哈希，详见下列记录。

- [独立采集、普通键盘对照](../../Testing/rc003-user-hid/RESULTS.md)
- [集成、安装与校验](../../Testing/rc003-user-hid/INTEGRATION.md)
- [独立硬件聚合数据](../../Testing/rc003-user-hid/hardware-result.json)
- [安装哈希和聚合结果](../../Testing/rc003-user-hid/installation-result.json)

验证命令：

```powershell
cargo fmt --all --check
cargo test --workspace --features runtime-simulation -- --test-threads=1
pnpm test
pnpm build
python -I Testing/rc003-user-hid/test_capture.py
python -I Testing/rc003-user-hid/test_bridge.py
node Testing/rc003-user-hid/test_hid_tap.cjs
node Testing/rc003-user-hid/test_gadget_adapter.cjs
.\scripts\verify-runtime-simulation-isolation.ps1
```

最后一项需已构建的生产二进制。真实 Helper 诊断须由知情用户显式启动，不要与活动增强会话同时运行独立注入测试。

## 12. 已知问题与后续工作

### 12.1 握手合包缺陷：已离线复现，未修复

`0.2.7` 的 `sayall_bridge.py::Channel.__init__` 在同次 `commands()` 返回多条消息时，接受第一条 `accepted` 后，会把紧随的 `lease` 判断为 `app_handshake_invalid`。

主程序发送 `accepted` 后可能立即发送首个 `lease`，当前状态机没有正确处理两条完整消息一次接收同时到达。

本次文档核对使用完全模拟的 Socket，不连接网络、不提权、不访问宿主，直接调用当前构造逻辑：

```text
分开：recv() -> accepted；下一次 -> lease
结果：握手通过

合并：一次 recv() -> accepted + lease
结果：app_handshake_invalid
```

现场日志还存在 23:17:00 UTC 的一次 `helper_disconnected`，随后用户再次明确开启后成功。日志不足以将那次失败唯一归因此缺陷，只能分别确认现场首轮失败、协议边界离线复现。

修复要求：缓冲区与状态机连续消费消息，正确处理握手之后同批到达的 `lease` 或 `stop`，继续拒绝非法顺序和未知命令。补充拆包、合包、握手后立即停止、EOF、取消、超时测试。不要靠延迟发送、自动反复提权或无限重试掩盖问题。

同时应补齐握手失败的脱敏原因。当前构造期异常可能只显示 `helper_disconnected`，无法直接区分协议拒绝与其他退出原因。

**本次未修复，也未替换运行中的程序。该项是后续稳定性改进，不应在现状描述中略去。**

### 12.2 剩余真机验收

1. 为三个键设置并保存实际需要的动作，验证单击、双击、长按/连发，无重复执行。
2. 闲置后验证首按，不只做连续热态测试。
3. 按住目标键时停止采集，确认无残留长按、连发或迟到动作。
4. 采集开启时验证输入法听写，检查按下开始、释放结束及音频完整性。
5. 单独测试普通键盘媒体键、不同键盘与其他蓝牙 HID 设备组合。
6. 测试真实断连、睡眠、宿主变化和重新开启，不能沿用旧确认或旧动作。

### 12.3 产品化门槛

稳定化前需完成握手修复、上述真机验收、长期运行/开销观察、来源隔离验证和签名/完整性部署评估。

此前保留“实验”标识与风险确认，不让它成为基础语音依赖，不承诺“自己改源码就绝对安全”。
