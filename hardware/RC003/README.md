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
├── helper/            产品化 spike：提权助手 + Gadget agent + 自检台（2026-09-23）
└── probes/            生成上述输出的脚本
```

> `probes/` 与 `helper/` 的分工：`probes/` 是**一次性取证脚本**（Python/JS，各解决一个待验证问题，
> 可独立重跑）；`helper/` 是**待产品化的交付物**（Rust 助手 + agent，按产品级要求写：
> 零第三方依赖、载体固定版本 + SHA-256 校验、失败关闭、内建自检）。

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

### 3.4 宿主报告层 tap（2026-09-23 新增）

| 文件 | 作用 |
| --- | --- |
| `wudf-host-probe.log` | ★ **前提证据**。只读探测承载 RC003 的 WUDF 宿主：`HostPid=32684`、`exe=WUDFHost.exe`、**独占 RC003**、普通权限 `OpenProcess` 被拒（`err=5`，宿主在 session 0） |
| `wudf-ioctl-tap.log` | ★ **关键证据**。提权**只读** tap 的完整日志：60 条转储（`enter`/`leave` 各 30）+ 逐条报告解码 + 判定。**直接证明本机 RC003 的报告里有 `0x00F1`/`0x0080`/`0x0081`** |
| `frida_msg_selftest`（脚本，无独立输出文件） | Frida 消息通道自检：`send` 与 `post/recv` 在 spawn 与 attach 两种模式下均可用（用于定位 session 0 上反向通道只送达第一条的异常） |

### 3.5 产品化 spike 自检与免提权检查输出（2026-09-23 新增）

| 文件 | 作用 |
| --- | --- |
| `helper_selftest.out` | 助手 `--selftest` 的完整输出：**50 项全 PASS，退出码 0**。覆盖自实现的 SHA-256（3 个 FIPS 180-4 向量）、`mask_token` 脱敏、手写 JSON 取值器（7 例，含空数组）、内嵌 agent 内容指纹、Gadget 配置生成、锁定文件一致性、**`HostPid` 读取宽度契约**（5 例）、**实机 `Enum` 扫描**（凡存在的诊断节点必须读得出 `HostPid`）、本机时间戳形态、**会话循环必须遵守 `--duration`**（静默客户端占住连接）、**控制台处理器可注册**、**模块枚举（阳性对照=本进程自身 / 阴性对照=无 Gadget）**、Gadget 模块名判据、**DLL 放置判定（摘要一致即复用）**、**放置策略决策表 5 例**（接管 / 分代 / `[STALE-TAP]` / `--attach-only`）、分代目录命名、**令牌跨运行稳定与分代回收**、**日志分隔线只在开新一轮时写**（阴性+阳性成对）、**端口 10048 文案可操作**、**接管轮的 `[DLL]` 汇报不得报 `sha256_verified`**（配复用路径的阳性对照）、**哨兵键解析与清空集合**（重叠必须报错）、**usage 渲染格式**、**内嵌 agent 与 helper 的三键常量一致且含 `targets` 命令**、**桥接描述文件解析三例**（含"版本不符即整份拒绝"的阳性对照）、**桥接边沿编码**（空集合必须编成 `-`）、**与主程序侧的桥接路径约定一致**（只改一边的现象是"双方都在跑却永远连不上"，且两边日志都不报错）、**桥接描述文件只读探测的 absent/ok/invalid 三态**（`--dry-run` 免提权即可看到）、**桥接连接错误码能分辨「路径未知」与「文件不存在（带路径）」**、**续约写失败能立刻打断当前连接**（死亡螺旋回归项，配阳性对照 `conn_stale_control.out`）。**无需提权、无需设备、不注入** |
| `agent_selftest.out` | agent 自检台输出：**5 项全 PASS，退出码 0**。A 无助手在监听时 `init` 必须由 `CONNECT_TIMEOUT_MS` 兜底（实测 2.20s 落地，禁止立刻返回）；B 正常协议（hello → hb → 续约 → 停止续约后租约过期）；C 显式解除 `disarm`；**D 助手消失后重新上线只允许连一次**（同时要求断线可见：`rx_timeouts + read_errors > 0`）；**E `targets` 护栏**（只许追加哨兵键；被拒的命令不得改动清空范围）。用 frida dev binding 注入哑进程 + 假助手，**无需提权、无需设备** |
| `agent_selftest_noguard_control.out` | **上面 D 与 E 的阳性对照**：`agent/control_make_noguard.py` 生成四处回退的副本（去并发守卫 / 去竞争检查 / `pump` 退回旧行为 / 去 `targets` 两条护栏），同一批用例**必须 FAIL**。实测对照版 D 与 E 双双 FAIL（D：被连 **3 次**、3 条 hello、断线可见=0，与真机接管轮现象一致；E：无拒绝日志且清空范围被改坏）。**没有这份对照，PASS 可能只是判据没分辨力** |
| `canary-argcheck.out` | `--canary-usage` 的解析与拒绝（免提权、只读 `--dry-run`）：`0x4A` 与 `0x4a,0x4B` 通过并算出 `clear_usages`；`0x00F1` 因与三键重叠被 `[STOP]` 拒绝；`0` 与非十六进制在**参数解析阶段**即拒。四种取值退出码 0/2/2/2 |
| `helper_dryrun_canary.out` | 带哨兵键的完整 `--dry-run`：`[CANARY] report_usages=… clear_usages=0x00f1,0x0080,0x0081,0x004a` + `[DRY-RUN-CANARY]`，**免提权**即可看到「真跑起来会清哪些 usage」 |
| `helper_dryrun.out` | 助手 `--dry-run` 的完整输出（**免提权**）：`diag_keys=6` / `HostPid读出=6` / `read_failures=0` / `rc003_instances=1` → 定位到 `pid=32684 WUDFHost.exe` → `exclusive_rc003_host` → Gadget 尺寸与 SHA-256 校验通过。与探针（独立实现）给出同一 PID。另有 `[PREP-SIM]`：只读报告**真跑起来会怎么处理运行时目录**（实测 `dll_exists=true sha256_matches=true would_do=reuse_existing`），把"第二次运行会撞上被锁文件"这类事提前暴露在免提权阶段 |
| `2026-09-23-acceptance-run.log` | **反复运行验收（注入轮 + 接管轮）的整理版**：只删掉 `[HB]`/`[RENEW]` 周期行，其余一字未改。覆盖 16:06:37 全新注入（`action=reuse` + `how=injected`）、16:10:08 端口冲突（`os error 10048`，上一轮还在跑）、16:10:37 **接管**（`[ATTACH]` + `how=attached_existing_tap`），两轮三键各一组 `[EDGE]`。**同一次运行暴露出 6 处「证据/鲁棒性」缺陷**（多余日志分隔线、3 条并发连接 2 条被 RST、断线重连不计数、`discarded` 一物二用、接管轮误报 `sha256_verified=false`、`sent=false arm=true` 自相矛盾）——详见 [`docs/investigations/2026-09-23-rc003-rerun-acceptance-verdict.md`](../../docs/investigations/2026-09-23-rc003-rerun-acceptance-verdict.md) |
| `2026-09-23-acceptance-run-raw.log` | 上面那份的**未改动原文**（helper-run.log 第 728–1149 行，422 行 / 87 161 字节），保留 `[HB]` 心跳以便核对时间线。运行日志本身是追加语义且不入库，所以每次有价值的运行都按此方式归档一次 |
| `helper-rerun-blocked.log` | **2026-09-23 第二次运行失败的原始日志**（用户实跑 3 次，全部在注入之前退出）：`[STOP] 复制 Gadget 失败 ...: 另一个程序正在使用此文件，进程无法访问。 (os error 32)`。同目录 `windows-restart-manager-probe.log` 给出占用者 `pid=32684 WUDFHost.exe`（阳性对照通过）。详见 [`docs/investigations/2026-09-23-rc003-helper-rerun-blocked-by-locked-gadget.md`](../../docs/investigations/2026-09-23-rc003-helper-rerun-blocked-by-locked-gadget.md) |
| `2026-09-23-observe-run.log` | **首次真机 `observe`（提权）的一次性归档**（140 492 字节，完整一次运行）：30 条 `[EDGE]`（返回 `0x00F1` ×5 / 音量+ `0x0080` ×5 / 音量− `0x0081` ×5，各带释放边沿），`clears_ok=0`（observe 从不清报告）。**同一次运行暴露了两处安全上限失效**——可直接验证：`[TIMEUP]`/`[DISCONNECT]`/`[SUMMARY]`/disarm 在该日志中**出现 0 次**，即收尾代码一行都没执行过。详见 [`docs/investigations/2026-09-23-rc003-observe-first-real-run.md`](../../docs/investigations/2026-09-23-rc003-observe-first-real-run.md) |

> 这几份输出的价值在于**把"悄悄错却不会报错"的地方变成可判定项**：自实现的 SHA-256、
> 极简 JSON 取值器、注册表值的读取宽度、没有存活原语的 Socket 语义、以及**收尾路径**
> （`--duration` / Ctrl+C），一旦有偏差，现场现象会是"助手看起来正常但什么都没匹配到"
> 或者"该停的时候没停"，极难反查。
>
> 后两类不是假想——2026-09-23 真机两次运行各栽了一处：
> 首跑把 `REG_QWORD` 按 4 字节读（[`hostpid-read-bug`](../../docs/investigations/2026-09-23-rc003-helper-hostpid-read-bug.md)）；
> 首跑 `observe` 暴露出 `--duration` 在有 agent 连接时**永不到期**、Ctrl+C **不发 `disarm`**
> （[`observe-first-real-run`](../../docs/investigations/2026-09-23-rc003-observe-first-real-run.md)）。

> **`helper-run.log` 不在上表**：它是启动器每次运行都**追加**的可变运行时日志（含本地时间戳
> 分隔头），已单独 gitignore。真机验收后要归档时，复制成带日期的文件名（如
> `2026-09-23-observe-run.log`）——那才是"有意归档的一次性采集输出"。

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

### 4.3 免驱动通道探针（2026-09-23 新增）

这三个探针与 4.1/4.2 的旧脚本不同：**不硬编码任何路径**，输出位置由 `--out` 传入，
因此可直接重跑，无需做占位符替换。

| 脚本 | 作用 |
| --- | --- |
| `hid-direct-read.py` | HID 接口枚举 → `CreateFile` 直读尝试 → 对全部 HID 接口做**阳性对照**（Shared/Exclusive 分布）→ **零权限打开**后用 `HidP_GetButtonCaps` 取出设备声明的 usage 与报告结构。只读，不做任何写入 |
| `raw-input-dump.py` | Raw Input 全量事件转储。一次注册 5 个 TLC 组合（含厂商页 `0xFF00`），**不做任何 VK 过滤**，逐条打印 `VKey`/`MakeCode`/`Flags`/`Message` 与设备路径，并标注是否来自 RC003。用法：`python raw-input-dump.py --seconds 150 --out <文件>` |
| `wudf_host_probe.py` | **2026-09-23 新增**：只读探测承载 RC003 的 WUDF 宿主——枚举全部 `WUDFDiagnosticInfo`、取出 `HostPid`、用 Toolhelp 快照取进程名（不用 `OpenProcess`，因为宿主在 session 0）、并判定宿主是否**独占 RC003**。用法：`python wudf_host_probe.py --out <文件>`。用于判定"报告层捕获"路线的物理前提是否成立 |
| `windows-file-lock-probe.py` | **2026-09-23 新增（保留一次失败的判据）**：用 `(access, share)` 组合试开目标文件，判断它是否被别的进程占用。**它的价值过半在于记录错误判据**：`READ + share=NONE` **不能**检出被映射的 DLL（阳性对照 `kernel32.dll` 同样 OK），而 `WRITE + share=NONE` 的 `err=5`(ACL) 会掩盖 `err=32`(占用)。**带阳性对照开关**（默认开），并且自检式地告诉你能不能信它的结论 |
| `windows-restart-manager-probe.py` | **2026-09-23 新增**：用 Restart Manager API（`RmRegisterResources` + `RmGetList`）直接问系统"哪些进程占着这个文件"。绕开了 ACL 与"读共享"两个坑，**不需要提权、不需要写权限**。默认拿自己的 exe 做阳性对照（本进程必然映射着它 → 占用者列表里必须出现自己），对照组不过就明确声明结论不可信 |

另有一个 Rust 侧探针：`crates/sayall-windows/examples/gatt_probe.rs`
（`cargo run -p sayall-windows --example gatt_probe -- enum|listen [--seconds N] [--out 文件]`），
用于 GATT 服务/特征/描述符枚举与全量订阅采集。

**探针自检要求（本轮教训）**：`raw-input-dump.py` 的正确性用 `SendInput` 注入一个已知键来验证
（见 `raw-input-selftest.log`：注入 `A` 应得到 `vk=0x41` 的按下/抬起）。新写事件类探针时，
一律先做这种自检，避免"零事件"被误读成设备行为。本轮曾因 `GetRawInputData` 尺寸查询语义
与 `WM_INPUT` 的 `HRAWINPUT` 参数位置（本机实测在 `lParam`）写出全零读数，靠自检才发现。

**"查不到"类判据更需要阳性对照（2026-09-23 补充）**：`windows-file-lock-probe.py` 的读侧判据
就是靠阳性对照被证伪的——否则"这个文件没被占用"会被当成事实，而真相相反。
同样的教训当天又出现一次：助手的模块枚举因为结构体尺寸写错（`szModule` 是 256 不是 260）
返回**空列表**，`ERROR_BAD_LENGTH(24)`；**只有阴性断言（"没找到 Gadget"）时它会静默通过**，
于是 `[TAP]` 永远打印 0、所有"没有常驻 tap"的结论都是假的。规则：
**凡是"没找到/为 0/未发生"的结论，都必须附一条能返回"找到了"的阳性对照。**

### 4.4 宿主报告层 tap 探针（2026-09-23 新增，需提权）

| 脚本 | 作用 |
| --- | --- |
| `wudf_ioctl_tap.js` | **Frida agent**：挂 `ntdll!NtDeviceIoControlFile`，命中 `IOCTL 0x80018483` 时在 `onEnter`/`onLeave` 各转储一次输入/输出缓冲区。**只读**——不写任何目标缓冲区（参考实现在 `onEnter` 清零 usage 槽，本探针一字节都不动）。**只使用单向 `send()`**：阶段归属与统计不依赖反向通道（见下） |
| `wudf_ioctl_tap.py` | 驱动与判定：定位独占宿主 → 提权自检 → attach → 分阶段引导按键 → 按**本地时间轴**把记录归到阶段 → 从原始记录重算判定。三种模式：`--dry-run`（只定位+自检）/ 真机运行 / `--analyze <log>`（**离线复核既有日志，不需提权、不需设备在线**） |
| `run-wudf-ioctl-tap.ps1`、`run-wudf-ioctl-tap.cmd` | 提权启动器（ASCII-only）。`.cmd` 可直接右键「以管理员身份运行」 |
| `frida_msg_selftest.py` | 消息通道自检（无需提权）：spawn 与 attach 两种模式下分别验证 `send` 与 `post/recv` |

**通道设计坑（本轮实测）**：真实运行中 Python→JS 的 `post()/recv()` **只送达第一条消息**
（阶段标签停在 `pre`、收尾汇总为空），而同一次运行里 JS→Python 的 `send()` 全程正常。
自检显示 `post/recv` 在 spawn 与 attach 下对**普通进程**都可用，故该异常归因于 session 0
目标环境、根因未定位。最终设计：**每条记录自带 `t`（`Date.now()`），由 Python 按本地时间轴
归属阶段；累计统计随心跳用 `send()` 上行；判定从原始记录重算**——彻底不依赖反向通道。

**提权边界**：宿主在 session 0，普通权限 `OpenProcess` 被拒，注入必须提权；且 agent 工具链
不允许代用户提权，因此该探针由**用户本人**以管理员身份启动。

### 4.5 宿主报告层写入 / 拦截探针（2026-09-23 新增，需提权）

| 脚本 | 作用 |
| --- | --- |
| `wudf_ioctl_write.js` | **写入版 Frida agent**：同样命中 `IOCTL 0x80018483`，但按阶段在 `onEnter` **改写 / 擦除**报告的 usage 槽（只动偏移 3..8），`onLeave` **回写复原**。六道写入闸门 + 写入次数上限；计划表由 Python 注入源码、阶段用本地时钟推进（无反向通道） |
| `wudf_ioctl_write.py` | 驱动与判定：分阶段提示 → 双通道记录（宿主侧写入 + Windows 侧按键）→ 从原始记录重算判定。`--dry-run` / `--selftest`（六例合成日志走同一 parse+verdict 路径）/ 真机运行（`--phases A,B2` 可只跑指定阶段）/ `--analyze <log>…`（**可多份日志合并判定**，不需提权、不需设备在线）。判定只针对**本次实际执行过的阶段**，未执行标 `SKIP` |
| `raw_input_sink.py` | 可复用的 **Windows 侧独立观测点**：后台 Raw Input（`RIDEV_INPUTSINK` + `HWND_MESSAGE`）+ 按 `hDevice` 做设备归属。直接运行即用 `SendInput` 注入 F13 做**零可见副作用的阳性对照**；`--manual N` 追加真机手动窗口 |
| `run-wudf-ioctl-write.ps1`、`run-wudf-ioctl-write.cmd` | 全程提权启动器（ASCII-only）。`.cmd` 可直接右键「以管理员身份运行」 |
| `run-wudf-ioctl-write-b2.ps1`、`run-wudf-ioctl-write-b2.cmd` | **补采短跑**启动器：只跑 `A,B2`（约 22 秒），用于补回全程运行中漏采的阶段。`A` 必须一起跑，否则没有观测基准 |

**实测结果（2026-09-23，全程 + 补采合并判定）**：**7 项断言全部 PASS，判定 `interception_effective`（退出码 0）**。

- 首轮全程（11:33）6 项中 5 项 PASS（`A` / `B1` / `C`×2 / `D` / `E`）；`B2` 窗口内宿主侧命中 0 次
  ⇒ **采集缺失**（非机制失败），退判 `phase_not_observed`（12）。
- 补采 `B2`（11:51，`run-wudf-ioctl-write-b2.cmd`）PASS：4 次改写（返回/音量± → `0x0068`）
  → Windows 侧 `VK_F13`×4；该窗口 8 次命中全部在改写之下，**无**未被改写的目标键报告
  ⇒ 结论不依赖"操作者是否按对键"。
- **指纹级旁证**：Raw Input `MakeCode` 显示扫描码亦由改写后的 usage 重新推导
  （原生/改写 确定 `vk=0x0D make=0x1C`；改写 F13 `vk=0x7C make=0x64`；`SendInput` 注入 `make=0x00`）
  ⇒ 写入在翻译链**最上游**被消费，且替换键带正常扫描码。

合并判定：`python wudf_ioctl_write.py --analyze evidence/wudf-ioctl-write.log evidence/wudf-ioctl-write-b2.log`。

设计与判据见 §5.4，完整结果见
[`docs/investigations/2026-09-23-rc003-hid-host-write-tap-result.md`](../../docs/investigations/2026-09-23-rc003-hid-host-write-tap-result.md)。

### 4.6 `helper/` —— 产品化 spike（**不在 `probes/` 下**，2026-09-23 新增）

把 §4.5 的探针推进成"能真正跑起来"的交付形态。提交 `76e3855`。**未并入产品工作区**
（crate 自带 `[workspace]` 表且不在根 workspace `members` 里，不影响 `cargo build --workspace`）。

| 文件 | 作用 |
| --- | --- |
| `src/main.rs` | 提权助手。**零第三方依赖**（全走 kernel32/advapi32/shell32 原生 FFI），release 约 400 KB。流程：注册表定位承载 RC003 的 `WUDFHost` + **独占性判定** → 校验 Gadget SHA-256 → **枚举宿主模块判断是否已有我们的 tap** → **先绑定端口**（单实例锁：失败要早，且失败的那一轮不碰磁盘）→ 按策略（接管 / 复用已有 DLL / 复制并注入）准备运行时目录 → 注入（**监听在注入之前**，避免 agent 的 `init` 白等）→ 环路 TCP 收 agent 上报。`--selftest` 内建 42 项自检 |
| `agent/rc003_agent.js` | Gadget 侧 agent（540 行）。按 usage **选择性**清空（只动 `0xF1`/`0x80`/`0x81`，确定/主页/方向键保持走 Windows 原生路径，零回归面）。把 HidUsages 当**绝对状态**做 diff 后发 `edge`。含**下行静默看门狗**（§3.6） |
| `agent/agent_selftest.py` | agent 自检台（frida dev binding 注入哑进程 + 自带假助手），无提权无设备验协议与重连四例。可接一个参数指定别的 agent 文件（阳性对照用） |
| `agent/control_make_noguard.py` | 生成去掉并发守卫的对照副本。**改不到三处回退就报错退出**——静默产出一个看着像对照其实没改过的文件，会让阳性对照变成假证据 |
| `vendor/frida-gadget.lock.json` | 载体**版本 / SHA-256 / 许可的唯一事实来源**（17.18.0，wxWindows Library Licence v3.1） |
| `vendor/fetch_frida_gadget.py` | 下载并**逐层校验**：压缩包摘要 → 解压产物摘要 → PE 架构。篡改任一环退出码 3；锁定文件缺失退出码 2 |
| `Cargo.toml` / `Cargo.lock` | 独立 crate（`rc003-helper`）。DLL 本体**不入库**（23 MB，见仓库 `.gitignore`） |
| `run-helper.ps1` + `run-helper*.cmd` | **右键可用的启动器**（纯 ASCII，与 `probes/` 的 `run-wudf-ioctl-write*.cmd` 同约定）。助手是 Rust 输出 UTF-8，启动器会 `chcp 65001` 并把控制台切到 UTF-8，否则中文提示在 GBK 控制台里是乱码 |

**两个设计事实（都由实测倒推，不是假设）**：

1. **失败关闭必须用租约，不能用连接状态**。实测 Frida 17 的 Socket API 没有可用的存活原语：
   对端关闭后**写入不抛错**（3/3）、pending 的 `read` **不以 EOF 收尾**、连不上要**约 2.2s**
   才 reject（太慢，不适合当存活判据）。所以判据只能是
   `Date.now() - lastRenewAt <= LEASE_MS`。6 条反直觉语义全部记在 agent 文件头。
2. **agent 脚本用 `include_str!` 随二进制内嵌**，运行时目录被替换也不生效；
   锁定文件同样内联——缺失直接**编译失败**，内容漂移由自检**确定性**断言。

**运行入口**（四个右键/双击即可，无需记命令行参数）：

| 入口 | 是否需要管理员 | 作用 |
| --- | --- | --- |
| `run-helper-selftest.cmd` | 否（可直接双击） | 内置 42 项自检。**第一步先跑这个** |
| `run-helper-dryrun.cmd` | 否（可直接双击） | 定位 RC003 宿主 + 独占性核对 + 校验 Gadget + **只读预告运行时目录会怎么被处理**（`[PREP-SIM]`）；**不注入、不监听**。全是只读检查，所以**不再要求提权** |
| `run-helper-observe.cmd` | **是** | **零回归风险的第一步真机**：注入并上报按键边沿，但**从不清报告**。用它确认三键真的到得了我们这里。可带一个数字改时长（如 `run-helper-observe.cmd 60`）——短时长是观察 `--duration` 自动收尾最省事的办法 |
| `run-helper-canary.cmd` | **是** | **验收专用**：拦截三键 **+** 额外清掉哨兵键（主页 `0x4A`）。理由见下节——RC003 的三键在 Windows 侧本来就零事件，所以「清掉」与「不清」外部看不出差别；清一个**本来能用**的键，才能让「清空生效」和「fail-open」变成肉眼可见。默认 120 s |
| `run-helper.cmd` | **是** | 正式拦截：只清 `0xF1`/`0x80`/`0x81`；OK/主页/方向键走 Windows 原生路径 |

两个提权入口默认 `--duration 300`（5 分钟后自动收尾并撤钩），避免把拦截留在系统上。
结束方式：**Ctrl+C 或直接关掉窗口**（会先给 agent 发 `disarm` 再退出）。
`--observe` 与 `--duration` 也可直接传给助手（见 `--help`）。

> **`--duration` 与 Ctrl+C 曾经都是假的**（2026-09-23 真机实测，已修）：
> ① 会话读取原本阻塞在 `BufReader::lines()`，主循环被同步阻塞 ⇒ 只要有 agent 连着，
> `--duration` **永不到期**（实测 300 s 的跑到 518 s 仍在运行）；
> ② 从未安装 `SetConsoleCtrlHandler` ⇒ Ctrl+C / 关窗是**硬终止**，`disarm` 永远发不出去。
> 唯一真正兜住的是 **agent 侧租约**。详见
> [`docs/investigations/2026-09-23-rc003-observe-first-real-run.md`](../../docs/investigations/2026-09-23-rc003-observe-first-real-run.md)。

> **为什么 `--dry-run` 刻意不要求提权**：它全是只读检查，卡在 UAC 后面只会把失败藏起来。
> 2026-09-23 那次"`HostPid` 读不出"的 bug 本该在免 UAC 的情况下就暴露，却因为 dry-run
> 也被提权门挡住而白耗了一轮真机运行——之后把这道门去掉了。注入模式照旧必须提权。

> **日志是追加的**：启动器把 `--log` 指向 `evidence/helper-run.log`，该文件**不做截断**，
> 每轮运行之间插一条带本地时间戳的分隔头（`---------- 新一轮运行 … ----------`），
> 运行标题行也带时间戳。这样"这一轮"与"上一轮"可以并排对比——排错时这正是主要依据。
> （此前每轮启动即清空，一次 dry-run 就把前一轮真机失败的原始记录抹掉了。）

> **`run-helper.cmd` 的 `.cmd` 包装层尚未端到端执行过**：`run-helper.ps1` 的四种模式
> 已实测（`selftest` 退出 0；`dryrun` 免提权退出 0 且全链通过；`observe` 未提权退出 3 且指引正确）；
> `.cmd` 本身因当前 shell 无法非交互调用 `cmd.exe` 而仅做了静态检查。若右键运行异常，可直接调用
> `powershell -NoProfile -ExecutionPolicy Bypass -File run-helper.ps1 -Mode selftest`。

> **状态边界**：自检全绿**不等于**三键可用。
> 截至 2026-09-23，真机上已验证到：**定位链**（`--dry-run`）与 **`observe` 捕获链**
> （30 条 `[EDGE]`，三键 usage 与预测一致）。
> **仍未做**：`run` 模式的拦截生效、确定/主页/方向键无回归（`observe` 什么都没碰，
> 所以"无回归"在那次运行里是构造上的必然，不构成证据）、杀掉助手后的 fail-open 现场验收。

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

### 5.1 2026-09-23 补充：免驱动通道调研（四条通道全部 `failed`）

新增一批证据与探针，用于穷尽"不装驱动能不能拿到三键"。完整结论文档：
[`docs/investigations/2026-09-23-rc003-driverless-capture-investigation.md`](../../docs/investigations/2026-09-23-rc003-driverless-capture-investigation.md)。

| 结论 | 依据文件 |
| --- | --- |
| 设备**确实在报告里声明了三键**：`0x80`/`0x81`/`0xF1` 落在键盘页 `report_id=0x01` 的 `0x0000–0x00FE` 内（与可用的 Home `0x4A`、确定 `0x28` 同报告） | `hid-direct-control.log`（零权限 `HidP_GetButtonCaps` 取证） |
| 用户态**无法**直读该 TLC：`CreateFile(GENERIC_READ)` → `err=5`；阳性对照 8/17 可打开且全为 Shared 模式 | `hid-direct-control.log` |
| 唯一 TLC = `Generic Desktop / Keyboard`，输入报告长度 121，3 个 LinkCollection，4 个 ButtonCaps；含厂商页 `0xFF00` 的 `report_id` 6/7/8（嵌套集合，不产生第二个 HID 子节点） | 同上 |
| 厂商 GATT 服务 `8A7A0001` 的 3 个通知特征**可订阅但零通知**；电池特征同窗口有通知（回调链路通） | `gatt-probe-enum.log`、`gatt-probe-listen1.log` |
| GATT HID 服务 `0x1812` 特征枚举**全部 AccessDenied**（`0x2A4B` 报告描述符因此不可得） | `gatt-probe-enum.log` |
| Raw Input 注册 5 个组合（含厂商页 `0xFF00`）后，**返回/音量± 零事件**；同采集内 确定 → `VK_RETURN`×2、主页 → `VK_HOME`×1 按次数精确到达（自证对照） | `raw-input-dump2.log`、探针自检 `raw-input-selftest.log` |

> **归因修正**：三键"不可见"的根因**不是设备不上报**，而是 Windows 的 HID→VK
> 映射表里没有 `0x80`/`0x81`/`0xF1` 三个 usage，`kbdhid` 在映射阶段丢弃；
> 而想绕过映射去读原始报告，又会被 RIM 的独占打开挡住（微软 HID 架构文档的访问模式表）。

### 5.2 2026-09-23 复核补充：报告层捕获路线的前提**成立**

上节四条通道只证明"**不提权、不注入**拿不到三键"，**不等于"拿不到"**——报告的生产端位于
`WUDFHost.exe`（用户态）内、在 RIM 之前。本机只读实测确认：

| 结论 | 依据文件 |
| --- | --- |
| RC003 的 HID 设备由**用户态**驱动宿主承载：`Service = mshidumdf`、`LowerFilters = WUDFRd`、`WUDF\DriverList = HidOverGatt`、`DeviceDesc = Bluetooth Low Energy GATT compliant HID device` | `wudf-host-probe.log` |
| 宿主 PID 可从 `Device Parameters\WUDFDiagnosticInfo\HostPid` 读取；本机命中 `WUDFHost.exe` | 同上 |
| 该宿主**独占 RC003**（整棵 `Enum` 树中只有 1 个成员且为 RC003） | 同上 |
| 普通权限 `OpenProcess` 对该宿主返回 `err=5`（宿主在 session 0）→ **注入必须提权** | 同上 |

结论：**路线 A（HID 宿主内报告层捕获）未被证伪**，与路线 B（`kbdhid` 之下 KMDF lower filter）
并列在方案空间内，且不需要内核驱动 / `TESTSIGNING` / 关 Secure Boot / 重启。
完整复核（含参考实现机理、代价、与本仓库 R1–R6 / ADR 0002 对照）：
[`docs/investigations/2026-09-23-zstdjan-hid-host-tap-implementation-review.md`](../../docs/investigations/2026-09-23-zstdjan-hid-host-tap-implementation-review.md)。

> ~~**仍未执行**：我们自己的注入与拦截（`deferred`）~~ → **2026-09-23 已执行只读版并 `passed`**，见 5.3。

### 5.3 2026-09-23 实证补充：报告层**确实存在**目标三键（`passed`）

5.2 只证明"前提成立"。本轮按 5.2 末的下一步做了**只读**最小注入实验（一次交互式 UAC 提权，
**未重启、未改任何系统设置、未装计划任务**；全程不写目标缓冲区、不 `SendInput`），
结论从 `structural` 升级为 **`passed`**：

| 结论 | 依据文件 |
| --- | --- |
| 本机 RC003 报告里**确实存在** `0x00F1`（返回）×3、`0x0080`（音量+）×3、`0x0081`（音量−）×3 | `wudf-ioctl-tap.log` |
| 阳性对照 `0x0028`（确定）×4、`0x004A`（主页）×2 如期到达（证明"探针确实读到了通道"） | 同上 |
| 与参考实现判据**逐条吻合**：`IOCTL 0x80018483`、`in_len=8`、`out_len=9`、第 5/6 字节 `02/01`、`report_id=0x01`（30/30） | 同上 |
| 报告在 `onEnter` 已完整就位：30 次命中中 `onLeave` 输出缓冲区变化 **0/30** → 参考实现"清空必须在 `onEnter`"在本机成立 | 同上 |
| 调用方唯一：`KERNELBASE.dll+0x3f723`（宿主经 `DeviceIoControl` 进入 `NtDeviceIoControlFile`） | 同上 |
| 报告结构 = `report_id(0x01)` + `modifiers` + `reserved` + **3 × uint16 usage**（小端）；按键 = 按下报告 + 抬起中性报告 | 同上 |

> **因果链闭合**：设备上报（本节）→ 报告到达 `WUDFHost.exe` 层（本节）→ Windows 事件层零事件
> （`raw-input-dump2.log`）⇒ 丢弃发生在 **HID→VK 键码映射（`kbdhid`）**。5.1 的归因修正
> 至此由推断升级为实测。
>
> ~~**仍未验（`deferred`）**：写入路径（`onEnter` 改写 / 擦除 + 应用侧重映射）、~~ → 2026-09-23 已实测通过
> （7/7 PASS，`interception_effective`）。**仍未验**：边沿配对 / 租约 /
> fail-safe 失效语义、异机复跑、共享宿主路径、更严格 EDR/HVCI 策略、RC001（不可外推）。
> ~~**当前唯一阻塞项是产品决策**（ADR 0002 §3 是否重开；R5 是否容纳"提权助手 + 计划任务"）。~~
> → **2026-09-23 已拍板**：ADR 0002 §3 的「不使用 Frida」废止、R5 口径收窄，两项阻塞均解除。
> 写入版最小实验装置与结果见 §5.4。
>
> 完整报告（含复现步骤、环境版本、探针工程坑）：
> [`docs/investigations/2026-09-23-rc003-hid-host-readonly-tap-result.md`](../../docs/investigations/2026-09-23-rc003-hid-host-readonly-tap-result.md)。

### 5.4 2026-09-23 写入（拦截）最小实验

只读实验证明"报告里有三键"，本节验证下一问：**我们在 `onEnter` 改写的字节是否被
Windows 的键码翻译采纳**（即拦截是否真的生效）。

**装置**（均在 `probes/`，需一次交互式 UAC 提权，**不装内核驱动、不改任何系统设置、不重启**）：

| 文件 | 作用 |
| --- | --- |
| `wudf_ioctl_write.js` | 写入版 agent：命中 `0x80018483` 后只在偏移 3..8 改写 / 擦除 usage，`onLeave` 回写复原；计划表随源码注入，无反向通道 |
| `wudf_ioctl_write.py` | 驱动 + 判定：分阶段提示、双通道记录；`--analyze <log>…` 离线复核（可多份合并判定）、`--selftest` 分析层自检（六例）、`--phases A,B2` 只跑指定阶段 |
| `raw_input_sink.py` | Windows 侧独立观测点：后台 Raw Input（`RIDEV_INPUTSINK`）+ 按 `hDevice` 做设备归属；自带 `SendInput` 阳性对照 |
| `run-wudf-ioctl-write.ps1` / `.cmd` | 全程提权启动器（纯 ASCII），日志落 `evidence/wudf-ioctl-write.log` |
| `run-wudf-ioctl-write-b2.ps1` / `.cmd` | **补采短跑**启动器：只跑 `A,B2`（约 22 秒），日志落 `evidence/wudf-ioctl-write-b2.log` |

**阶段设计（本轮的关键是"对照"）**：

| 阶段 | 报告层动作 | 你按的键 | Windows 侧期望（仅 RC003 设备） |
| --- | --- | --- | --- |
| `pre` | 只观察 | 唤醒遥控器 | — |
| `A` | 只观察 | 确定 ×2 | `VK_RETURN`（阳性对照） |
| `B1` | 目标 usage → `0x0028`(确定) | 返回 / 音量± | 变成 `VK_RETURN` |
| `B2` | 目标 usage → `0x0068`(F13) | 返回 / 音量± | 变成 `VK_F13`（本机无任何来源会产生它，无歧义） |
| `C` | 仅擦除目标 usage | 音量± + 确定 ×1 | 目标键 0 事件；确定 仍 `VK_RETURN`（无波及） |
| `D` | 三个 usage 槽全清 | 确定 ×2 | **0 事件**（与 `A` 同键对照） |
| `E` | 停止写入（回到 observe） | 确定 ×2 | `VK_RETURN` 恢复（无残留） |

`A` 与 `D` 是**同一个键**的对照：A 有事件、D 没有，两者唯一差别是我们在报告层写了字节。
`B1`/`B2` 是反向证据：改写前该键在 Windows 侧零事件，改写后出现按键 ⇒ 写入发生在翻译之前。

> **实测结果（2026-09-23，全程 + 补采合并判定）**：**7 项断言全部 PASS → `interception_effective`**。
> `A` 阳性对照、`B1` 改写生效（4 次改写 → `VK_RETURN`×4，一一对应）、
> `B2` 补采 PASS（4 次改写 → `VK_F13`×4，无歧义）、`C` 目标键在报告层已擦除且非目标键未波及、
> `D` 擦除生效（同键对照成立）、`E` 停止写入后恢复。
> 首轮 `B2` 窗口内宿主侧命中 0 次 ⇒ **采集缺失**（非机制失败），退判 `phase_not_observed`（12），
> 由补采闭合。原始计数、扫描码旁证与完整判定见
> [`docs/investigations/2026-09-23-rc003-hid-host-write-tap-result.md`](../../docs/investigations/2026-09-23-rc003-hid-host-write-tap-result.md)。

### 5.5 2026-09-23 产品化 spike（机制已验证 → 形态可运行）

§5.3 与 §5.4 回答的是**机制**问题（报告里有三键吗 / 改写会被采纳吗），答案都是"是"。
§4.6 的 `helper/` 回答的是**形态**问题：这套机制能否造成一个可交付、可自检、会自己
失败关闭的组件。两者都做完了，但**证据强度不同**：

| 问题 | 证据 | 强度 |
| --- | --- | --- |
| 报告层有三键 | `wudf-ioctl-tap.log`（真机，提权只读） | 真机实测 `passed` |
| 改写会被采纳 | `wudf-ioctl-write*.log`（真机，提权写入，7/7） | 真机实测 `passed` |
| 助手/agent 形态可运行、可自检 | `helper_selftest.out`（42/42）、`agent_selftest.out`（5/5）、`agent_selftest_noguard_control.out`（用例 D 与 E 必须 FAIL） | **自检台**（无提权、无设备） |
| 宿主定位链在真机上成立 | `helper_dryrun.out`（免提权，`pid=32684` / `exclusive_rc003_host`）+ 探针 `wudf-host-probe.log` **两个独立实现给出同一 PID** | 真机实测，但**只覆盖定位，不含注入** |
| **三键边沿真的送达我们的代码**（`observe`） | `2026-09-23-observe-run.log`：30 条 `[EDGE]`，返回/音量+/音量− 各 5 次，usage `0x00F1`/`0x0080`/`0x0081`，`target_hits=15` 与按下数 1:1 | **真机实测 `passed`**（首次；但见下条边界） |
| **反复运行不再崩**（复用而非覆盖被占用的 DLL） | `2026-09-23-acceptance-run.log` 第一轮 16:06:37：`[PREP] action=reuse`（`note=一个字节都没写`）、`[VERIFY-MODULE] gadget_modules_in_host=1`、`how=injected` | **真机实测 `passed`** |
| **接管路径端到端**（不复制、不注入，常驻 agent 自己连回 + 补发 `arm`） | `2026-09-23-acceptance-run.log` 第二轮 16:10:37：`[TOKEN] source=file` → `[TAP] resident=1` → `[ATTACH] skip_injection` → `[HELLO] auth=true` → 三键各一组 `[EDGE]` → `how=attached_existing_tap` | **真机实测 `passed`**（首次） |
| `--duration` 到期自动收尾（真机） | — | **未验证**：两次真机运行都是手动结束（228 s / 40 s，无 `[TIMEUP]`） |
| 三键**拦截生效** + 无回归 + fail-open | — | **未做，仍待真机 `run` 模式** |

**这两行不可省略**：自检台跑的是**我们自己的代码与协议**（SHA-256、JSON 取值器、
租约状态机、`init` 超时兜底），它**不**接触真实 HID 宿主，也**不**证明按键能送达应用。
把它当成"机制已通过"的重复证据，或当成"三键可用"的证据，都是层错位。

**`observe` 那一行的边界同样要说清**：它证明了"三键到得了我们这里"，但 `observe`
**一个字节都不清**（`clears_ok=0`），所以：
① 它**不**证明拦截生效——真正的判据在 `run` 模式；
② 它**不**证明"确定/主页/方向键无回归"——那次运行里什么都没碰，"无回归"是构造上的必然。
"能定位"≠"能注入"，"捕获送达"≠"拦截生效"，"自检全绿"≠"三键可用"。

**定位链那一行**：它确实跑在真机上，也确实由两个互不认识的实现交叉印证，
但它止于"能找对宿主、Gadget 校验通过"。2026-09-23 的首跑就是栽在这里（把 `REG_QWORD`
按 4 字节读，见 [`docs/investigations/2026-09-23-rc003-helper-hostpid-read-bug.md`](../../docs/investigations/2026-09-23-rc003-helper-hostpid-read-bug.md)）。

**而 `observe` 这次又暴露出第三类问题：功能路径通了，收尾路径没通。**
`--duration` 在有 agent 连接时永不到期、Ctrl+C 不发 `disarm`——两条都是"看起来没问题、
真跑才显形"。完整记录见
[`docs/investigations/2026-09-23-rc003-observe-first-real-run.md`](../../docs/investigations/2026-09-23-rc003-observe-first-real-run.md)。

### 5.6 2026-09-23 第二次运行被上一代 Gadget 挡住（已修，含一次性迁移）

**现象**：`observe` 首跑成功（15:10:33，`[INJECT] loaded=true`）之后，用户再跑三次
（15:32 / 15:33 / 15:35），全部**在注入之前**就退出：

```
[STOP] 复制 Gadget 失败 ...\vendor\frida-gadget.dll -> C:\ProgramData\SayAll\rc003-helper\frida-gadget.dll:
       另一个程序正在使用此文件，进程无法访问。 (os error 32)
```

**根因**（两条独立证据）：

| 事实 | 证据 |
| --- | --- |
| 注入成功的代价是宿主**长期映射**那份 DLL。宿主 `WUDFHost.exe` 启动于 01:23:26，一直活着（采样时已 14.47 h） | `windows-restart-manager-probe.log` + 探针给出的进程启动时刻 |
| 该 DLL 被 `pid=32684 WUDFHost.exe` 占用，所以 `fs::copy` 必然 `ERROR_SHARING_VIOLATION(32)` | `helper-rerun-blocked.log`（三次实跑的原始日志）+ Restart Manager 探针（**阳性对照通过**：拿探针自己的 exe 去问能认出自己） |

而 `prepare_runtime` 当时是**无条件覆盖复制**——它把"宿主还映射着上一代 DLL"
这个**正常状态**当成了致命错误。

> **取证纪律**：先试的 `READ + share=NONE` 判据是**坏的**——阳性对照（同一个探针去问
> 它自己映射着的 `kernel32.dll`）同样返回 OK，所以"能打开"推不出"没被占用"。
> 换成 Restart Manager API 才拿到确定答案。见 `probes/windows-file-lock-probe.py`
> （保留那次失败的判据与对照组，供下次不再走弯路）。

**修法**（不是"绕过"，是把状态显式化）：

1. 启动时**枚举宿主模块**，判断里面是否已有我们的 Gadget（`[TAP]`）；
2. 令牌改为**跨运行稳定**（运行时目录 `session.token`），于是常驻 agent 能重连回来被接管
   ——这条路径 agent 侧本来就写了（`ensureConnected`），但此前每次运行都换令牌，
   常驻 agent 的 hello 必然被当冒充者拒掉，**那条重连路径实际上永远走不通**；
3. 能接管就**复制和注入都不做**（`[ATTACH]`），并且鉴权后补发 `arm` / `mode` / `restore`
   ——上一轮收尾发过 `disarm`，**不发 `arm` 接管后永远不会出现 `[EDGE]`**；
4. 接管不了（本机制引入之前的旧世代）→ `[STALE-TAP]` + 明确清理步骤 + 退出码 13，
   **不再去撞那个必然失败的文件**；实验性退路 `--new-generation`；
5. 复制前先按 SHA-256 复用已存在的同名文件（`[PREP] action=reuse`，一个字节都不写）；
6. 注入后**用模块枚举核实**，不再用 `LoadLibraryW` 的返回值充当"已加载"的证据
   （模块已存在时它同样返回非 0）；
7. `--await-hello`（默认 30 s）：注入了却没有会话 → `[NO-HELLO]` + 退出码 12，不许静默干等。

**自检 34 → 38 项**（同一轮里另一次是 23 → 34），新增项里有一次就抓到真 bug 的：Toolhelp 的 `szModule` 是 **256**
不是 260，写错会让结构体大 8 字节、`Module32FirstW` 直接 `ERROR_BAD_LENGTH(24)` 且列表为空。
**只有"查不到 Gadget"的阴性断言时这个 bug 会静默通过**——阳性对照（枚举本进程必须看见自己）
是强制项。

**本机仍留一次性迁移**：15:10 那次注入的 token 未持久化，属旧世代，接不上。
清法（任选其一，都需提权或人工）：断开并重新配对 RC003 / 设备管理器禁用再启用 /
重启系统；用 `windows-restart-manager-probe.py` 确认 `users=(none)` 即已释放。

### 5.7 2026-09-23 可观测性补充：RC003 上「拦截生效」本身无法观测 → 哨兵键（canary）

**发现**：RC003 的三键在 Windows 侧本来就零事件（`kbdhid` 丢弃这三个 usage）。因此
「清掉」与「不清」在外部**完全看不出差别**——音量不会变、也不会出现任何字符。于是传统判据
「按了之后没有原生动作」在 RC003 上**恒为真**：它无法区分「清空生效」与「清空根本没跑到」。
换言之，`observe` 与 `run` 两种模式在用户可感知层面**无法分辨**——这是一处判据缺口，
不是实现缺陷；但它会让「拦截生效」永远拿不到可复核的证据。

**处置**：新增 `--canary-usage <U16>` 与入口 `run-helper-canary.cmd`（默认清主页 `0x4A`）。
主页是 Windows 本来能处理的键，清掉它之后：

| 时刻 | 现象 | 证明了什么 |
| --- | --- | --- |
| 运行前按主页（记事本） | 光标跳到行首 | 基线：该键确实可达（**阳性对照，不可省**） |
| 运行中按主页 | 什么都不发生 | **清空真的作用到了报告上** |
| 结束 / 租约到期 / Ctrl+C 之后 | 光标又能跳 | **fail-open 生效，不残留清空、不需重启** |

安全边界：只清该 usage 的 2 个字节（`report_id` / `modifiers` / `reserved` 一律不碰）；
受 `--duration` 上限与 agent 侧 2 s 租约双重兜底；默认关闭。哨兵键**只进清空集合、不进上报集合**
——`[EDGE]` 永远只有三键，心跳里的 `report_usages` 恒为三键，`clear_usages` 才是实际清空范围。

**协议护栏**（`targets` 命令）：`report` 必须**恰好**等于三键（否则等于允许远端关掉上报，
那样「看不到按键」会被误读成「按键没到」）；`clear` 必须是 `report` 的**超集**
（否则会出现「报了但不清」的隐形配置）。越界命令一律拒绝，且**不改动现有清空范围**——
由 agent 自检台用例 E 覆盖，并配了「去掉护栏必须 FAIL」的阳性对照。

**验收手册**：`Testing/WindowsRC003EnhancedCapture.md`（E1–E6 的命令、判据、日志字段、状态表）。

**证据**：`evidence/canary-argcheck.out`（四种取值：`0x4A` 通过、`0x00F1` 重叠被拒、
`0` 与非十六进制被拒）、`evidence/helper_dryrun_canary.out`。

### 5.8 2026-09-23 捕获链第 ② 段（传输）：主程序桥接

**问题**：清空只做到"让 Windows 看不到这三个键"，**不等于**"主程序知道了"。主程序侧的
Raw Input 与键盘钩子同样拿不到它们（那正是 `kbdhid` 丢弃的直接后果）。在桥接接线之前，
助手收到 `{"type":"edge"}` 只做 `session.edges.push()` 与写日志，全库**没有任何**
向主程序转发的通道——所以三键一直是"能配置但按下去没反应"。

**三段链条**（本段是第 ② 段）：

```
设备 → WUDFHost 报告层 ──[① 捕获]──> 提权助手 ──[② 传输]──> 主程序 ──[③ 重映射]──> 动作
                                          已真机验证      本节新增        无需改动
```

**方向：主程序监听、助手回连。** 反向不成立——助手以管理员运行、运行时目录在
`%ProgramData%\SayAll\rc003-helper`，普通权限的主程序读不到那里的写入；
而"主程序写在 `%LOCALAPPDATA%\SayAll\`、提权助手去读"没有权限障碍。
主程序用**随机端口**（`127.0.0.1:0`）并写出描述文件 `rc003-bridge.ini`
（端口 + 令牌），助手读取后回连、出示令牌。跨账户提权时助手按固定相对路径
枚举 `C:\Users\*\AppData\Local\SayAll\rc003-bridge.ini` 兜底。

**引擎零改动**：助手送来的边沿投进 `EngineMessage::GateEdge` —— 与 RC001 上
"被 `key_gate` 吞下的厂商键边沿"是**同一条**通道。不用 `HidUsages` 是有意的：
后者会**整体替换**引擎内 HID 来源的按下集合，若同时有 RC001 在跑会把它的状态一起冲掉。

**判据（E7，免设备、只需一次提权）**：主程序与助手同时运行后看助手日志

| 时刻 | 现象 | 证明了什么 |
| --- | --- | --- |
| 主程序未启动 | `[APP-BRIDGE] event=unavailable reason=descriptor_missing(...)`，每 2 s 重试 | 主程序未运行属**正常现象**，不影响捕获与清键 |
| 主程序已启动 | `[APP-BRIDGE] event=connected` | 描述文件被发现、令牌被接受、握手完成 |
| 收尾 | `[SUMMARY] app_bridge=connects=N failed=M edges=K` | `connects>0` 说明第 ② 段通了；**`edges` 才是边沿真的过了桥** |

**`captures 有值而 edges=0`** 正是"键能按、映射不动"的状态——这是排查时最容易
被误读成"注入没成功"的一种。

**安全边界（如实）**：令牌**不是**安全边界，只用于防误连/防混淆（同用户进程本来
就能读描述文件）。真正的纵深防御在**白名单**：桥接只接受 `0x00F1/0x0080/0x0081`，
其余 usage 一律丢弃并计数——即使令牌泄漏，攻击者也只能伪造这三个键，
无法借桥接触发任意按键映射。助手侧**只在鉴权通过后**才转发边沿。

**fail-open 三层**：① 助手正常收尾时先发释放边沿再发 `BYE`；② 助手被强杀时由主程序侧
静默看门狗（3 s 无任何行）释放全部；③ agent 租约（2 s）保证"键恢复原生行为"。
第 ② 层不可省——被任务管理器结束的助手不会发 `BYE`。

**实现与验证**：`crates/sayall-windows/src/rc003_bridge.rs`（单测 9 项，含真 TCP 端到端、
接管、看门狗）、助手自检第 24–26 项（描述文件解析 3 例 / 边沿编码 / 与主程序侧的路径约定）。
设计与威胁模型详见 [docs/investigations/2026-09-23-rc003-app-bridge-transport.md](../../docs/investigations/2026-09-23-rc003-app-bridge-transport.md)。
**已完成（2026-09-23 19:18）**：真机联调与端到端映射均通过 —— 在「按键」页给**返回键**
配置"键盘 b"后按遥控器返回键，**记事本打出 b**。证据
`evidence/rc003-bridge-e2e-2026-09-23.log`（滤掉心跳后 116 行）。

### 5.9 2026-09-23 死亡螺旋：续约写失败 → 主循环卡住 → 按键永久无反应

**现象**：E7 通过（桥接 `event=connected`）之后按三键，助手侧 **一条 `[EDGE]` 都没有**，
且日志停在 `[RENEW] state=write_failed` 之后**不再增长**（心跳、ACCEPT 全无）。

**根因**（既有缺陷，非桥接引入——17:42 的轮次已出现过同款）：续约线程写失败时只做了
`*guard = None`（清空连接句柄），**没有结束当前的 `serve_connection`**。而主循环是
**串行**的（`accept` → `serve_connection` 跑完 → 再 `accept`），于是 agent 重连上来的
新连接只能排在 backlog 里没人 accept ⇒ 续约永远恢复不了：

```
续约写失败 → 续约静默 → agent 2 s 租约过期（lastRenewAt 归零）
→ 再 4 s 判 auth_mismatch → 断开重连 → 新连接排不进 accept → 续约依旧静默 → 循环
```

**为什么写会失败**：不只是"对端已关闭"，还包括**对端不再读**（写缓冲写满）。
后一种情况下 socket 并没有断，`serve_connection` 的读一切正常 —— 所以它**不会自己退出**，
这正是"只清 shared 不够"的原因。

**修法**：续约线程写失败时置位 `conn_stale`；`serve_connection` 每轮检查该标志，
置位即收尾，把主循环还给 `accept`。

**判据**：日志出现 `[CONN-STALE] 续约写入失败 → 本条连接已不可用，立即收尾…` 之后，
**应当立刻**看到新的 `[ACCEPT]` / `[HELLO]`，且 `renew_age_ms` 重新变小、`lease_ok=true`。

**回归**：自检第 29 项（`deadline=None`，因此只可能被 `conn_stale` 打断；3 s 内不返回即 FAIL）。
阳性对照 `helper/target/tmp/conn_stale_control.rs`（不进产品）：有检查 500 ms 收敛、
无检查永不收敛，证据 `evidence/conn_stale_control.out`。

**⚠️ `dropped:auth_mismatch` 这个标签名有误导性**：它的真实判据是
`lastRenewAt === 0 && now - tConnect > AUTH_TIMEOUT_MS`，即"**连上了但一直没收到续约**"，
**与令牌无关**（同一轮 `[HELLO] auth=true` 明明是通的）。看到它不要去查令牌。

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
