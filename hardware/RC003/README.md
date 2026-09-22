# RC003 硬件取证资料

小米蓝牙遥控器 2 Pro / RC003 的**真机取证原始材料**：按键采集日志、设备枚举输出，以及生成它们的探针脚本。

## 1. 这个目录解决什么问题

项目目标是「用遥控器实体键做自定义映射，且不引入任何副作用」。要做到这点，必须先回答三个问题：

1. **遥控器的按键在 Windows 里走哪条通道？** —— 决定了能不能"按设备"拦截。
2. **设备在 PnP 设备树上长什么样？** —— 决定了驱动/过滤器能挂在哪里。
3. **哪些键根本到不了 Windows？** —— 决定了哪些键只能放弃或换路子。

这些问题**只能用真机实测回答**，无法靠读代码或文档推断。本目录就是当时的实测记录。

配套结论文档：

- [`docs/investigations/2026-09-22-per-device-key-interception-route-selection.md`](../../docs/investigations/2026-09-22-per-device-key-interception-route-selection.md) —— 路线选型（首轮）
- [`docs/investigations/2026-09-22-per-device-key-interception-route-selection-round2.md`](../../docs/investigations/2026-09-22-per-device-key-interception-route-selection-round2.md) —— 路线选型（第二轮，推荐方案）
- [`docs/investigations/evidence/2026-09-22-tv-shell-action-channel-analysis.md`](../../docs/investigations/evidence/2026-09-22-tv-shell-action-channel-analysis.md) —— TV 键通道归属专项

## 2. 目录结构

```
hardware/RC003/
├── README.md          本文件
├── evidence/          采集与枚举的原始输出（不可再生）
└── probes/            生成上述输出的脚本
```

## 3. `evidence/` —— 原始输出

### 3.1 按键采集（E1：按键走哪条通道）

用一版**只加诊断日志、不改按键行为**的 debug 构建，在真机上按遥控器采集。

| 文件 | 作用 |
| --- | --- |
| `e1-capture3.log` | ★ **关键证据**。成功的一次采集会话（pid 3124，121 行）。用户按了 9 个键（TV / 主页 / 菜单 / 确定 / 上 / 下 / 左 / 右 / 电源），日志里能看到 18 条键盘边沿与 1 条 `map_fire`；而 HID 通道相关的采集行**一条都没有** |
| `e1-final-audit.out` | 对 `e1-capture3.log` 的**全文件终审**：逐模式计数（`hid_report_seen` / `hid_usage_seen` / 未归因 usage / 未知报告形状全部为 0），并列出会话边界、按键清单、监听器就绪证据。**这是"HID 通道零报文"结论的直接依据** |
| `e1-capture.log`、`e1-capture2.log` | 前两次采集尝试（一次启动即退出，一次未等到按键）。保留以说明结论不是只跑了一次 |
| `e1-launch.txt` | 采集实例的启动记录：可执行文件路径与大小、构建时间、日志落点、可用磁盘、pid |
| `e1-running.txt` | 采集实例存活标记 |
| `e1-shutdown.out` | 采集结束前的进程与顶层窗口枚举（用于确认实例优雅退出、没有残留 BLE 会话） |
| `e1-read-state2.json` | 日志增量读取器的状态文件（记录上次读到的偏移与日志路径） |

### 3.2 设备枚举（E2：设备栈长什么样）

| 文件 | 作用 |
| --- | --- |
| `driverstack.out` | ★ **两级 PnP 设备树**的完整枚举：`BTHLE\Dev_…` → `BTHLEDEVICE\{…}`（父节点，HIDClass 类，`Service=mshidumdf`）→ `HID\{…}`（子节点，Keyboard 类，`Service=kbdhid`）。父子关系由 `ParentIdPrefix` 与子节点实例名互相印证。**这是"过滤器该挂在哪"的判断依据** |
| `e2-hwid.out` | 两棵枚举树下 `HardwareID` 的**逐条比对**：确认过滤器匹配串（`HID\{…}_Dev_VID&012717_PID&32b8_REV&00a4`）存在于 `kbdhid` **子节点**，而**不在** `mshidumdf` 父节点 |
| `e2-precheck3.out` | E2-1 前置核查的**注册表直读版**（最快、信息最全）：父/子节点归属、类键、系统状态（Secure Boot、TESTSIGNING、驱动与证书残留、OS 版本） |
| `e2-precheck.out`、`e2-precheck2.out` | 同一核查的早两版（用 `Get-PnpDevice` 逐设备查属性，超时后改用注册表直读）。保留可见方法演进与失败原因 |
| `rawinput-types2.out` | ★ Raw Input 设备列表与 `RIDI_DEVICEINFO` 的**双路交叉验证**：目标设备的 `dwType` 两处都是 `1 = RIM_TYPEKEYBOARD`，且其设备接口 GUID 属 **Keyboard 类**（而非 HID 接口类）。**这是排除"假阴性"的关键证据** |
| `rawinput-topology.out` | 注册表 HID 拓扑：匹配 `VID_2717 + PID_32B8` 的顶层键**恰好 1 个**、其下实例**恰好 1 个**，且该实例 `Service=kbdhid`、`ClassGUID` 为 Keyboard 类 |
| `all-hid-tlc.txt` | 匹配 `2717` 的 HIDClass 设备清单 + 注册表里所有带 `UP:` 的 HID 枚举键（用来说明"只有键盘页、没有消费页"） |
| `hid-tlc.txt` | 单个 TLC 的详细字段（`ClassGUID` / `Service` / `Driver` / `Mfg` / `HardwareID`） |
| `wmi-hid.txt` | 用 WMI 查到的蓝牙 HID 设备列表（第三方视角交叉验证同一批设备） |

> `rawinput-types.py` 在本次调查中**首次发现了"读越界"假象**（把键盘段的字段误读成"消费页 usage"），
> 修正版是 `rawinput-types2.py`。两版都在 `probes/` 里，便于对照这个坑。

### 3.3 构建与会话辅助

| 文件 | 作用 |
| --- | --- |
| `verify-build.out` | 提交前对采集代码做的 `cargo fmt --check` + `cargo check` 验证结果 |
| `live-check.txt` | 采集期间的存活检查（应用进程、RC003 HID 设备是否在、蓝牙连接状态） |
| `live-proc.txt` | 采集期间的进程列表（用于确认没有并发构建/残留进程干扰） |

## 4. `probes/` —— 探针脚本

脚本按用途分组。**都能独立重跑**（见第 6 节注意事项）。

### 4.1 采集会话

| 脚本 | 作用 |
| --- | --- |
| `e1-build.py` | 构建含诊断采集代码的 debug 版应用（含给 `build.rs` 准备 git PATH 的逻辑） |
| `e1-launch.py` | 启动采集实例并记录 pid / 可执行文件 / 日志落点 |
| `e1-shutdown.py` | 通过应用自身退出路径（`WM_CLOSE` 到主窗口）请求退出，**不强杀** |
| `e1-read.py`、`e1-read2.py` | 从诊断日志里抽取采集证据；`e1-read2.py` 支持 `--new` 增量读取 |
| `e1-final-audit.py` | 对整份日志做终审：多种兜底模式计数，确保"0 命中"不是漏检 |
| `verify-build.py` | 跑 `cargo fmt --check` + `cargo check` 并把结果落盘 |

### 4.2 设备枚举与核查

| 脚本 | 作用 |
| --- | --- |
| `driverstack.py` | 枚举两级 PnP 设备树（父/子节点、类 GUID、服务、HardwareID） |
| `e2-hwid.py` | 比对两棵树的 `HardwareID`，定位过滤器匹配串落在哪个节点 |
| `e2-precheck.py`、`e2-precheck2.py` | E2-1 前置核查（PnP 查询版） |
| `e2-precheck3.py` | 同上，**改为注册表直读**（快一个数量级，且绕开 `Get-PnpDevice` 超时） |
| `rawinput-types.py`、`rawinput-types2.py` | Raw Input 设备类型枚举（`RIDI_DEVICEINFO`），确认目标设备是键盘类型 |
| `rawinput-topology.py` | 注册表 HID 拓扑（唯一实例 / Keyboard 类 / 跨枚举树排除第二实例） |
| `enum-rawdev.ps1` | 用 PowerShell 枚举 Raw Input 与 HID 设备（`rawinput-*.out` 的来源之一） |

## 5. 这些证据支持的关键结论

| 结论 | 依据文件 |
| --- | --- |
| RC003 在 Windows 上**只有 1 个 HID 实例、1 个 TLC，且只声明键盘页**（无消费页） | `rawinput-topology.out`、`all-hid-tlc.txt`、`wmi-hid.txt` |
| 该设备在 Raw Input 里**100% 走键盘通道**，HID 通道**零报文** | `e1-capture3.log` + `e1-final-audit.out` |
| "收不到 HID 报文"**不是假阴性**，而是根本没有可被用法页过滤的 HID 通道 | `rawinput-types2.out`、`rawinput-topology.out` |
| 设备栈是**两级**的：父节点（HIDClass / `mshidumdf`）+ 子节点（Keyboard / `kbdhid`） | `driverstack.out` |
| 想按设备拦截，过滤器要挂在 **`kbdhid` 子节点**（不是 `mshidumdf` 父节点） | `e2-hwid.out`、`e2-precheck3.out` |
| **返回 / 音量± 在 Raw Input 两条通道上都收不到** | `e1-capture3.log`（18 条边沿里没有这四个键） |

> 这些结论直接决定了第二轮选型文档推荐的方案（设备专属下层过滤器）。
> **结论以文档为准**，本目录提供原始出处，供复核时回查。

## 6. 脱敏说明与使用注意

**已脱敏**：本目录所有文件在入库前统一做了隐私替换 ——

| 占位符 | 原内容 |
| --- | --- |
| `<BT-ADDR>` | 遥控器自身的蓝牙 LE 设备地址（12 位十六进制） |
| `<BT-ADDR-OTHER>` | 采集机器上另一台蓝牙设备的地址 |
| `<USER-HOME>` | 采集机器的用户主目录绝对路径 |
| `<PROBE-DIR>` | 当时存放本批探针与输出的临时工作目录 |

**保留未改**：各类公开系统常量，例如 `{00001812-0000-1000-8000-00805f9b34fb}`（蓝牙 HID 服务）、
`{745a17a0-…-00a0c90f57da}`（HIDClass 类）、`{4d36e96b-…-08002be10318}`（Keyboard 类）、
接口类 GUID，以及 `VID_2717 / PID_32B8 / REV_00A4` 这类产品标识。
设备实例 ID（如 `9&1748ac9e&0&0000`）是 Windows PnP 生成的**相对标识**，不含设备地址，也保留。

**重跑注意**：`probes/` 里的脚本是当时的现场工具，路径常量已被替换为上面这些占位符。
若要重跑，需先把 `<PROBE-DIR>` 等替换为本机实际路径；部分脚本还依赖当时的开发构建产物。

**不要在真实日志里提交未脱敏的设备地址** —— 本仓库是公开仓库，见根 `AGENTS.md` 的来源与隐私规则。
