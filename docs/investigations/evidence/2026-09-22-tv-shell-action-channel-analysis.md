# TV 键 Shell 协议动作的通道归属分析（2026-09-22，静态取证 + 真机采集）

> 目的：回答选型文档（`docs/investigations/2026-09-22-per-device-key-interception-route-selection.md`）附录 C.2 第 1 项所列的**最大不确定点**——"TV 的独立 Shell 协议动作来自哪个 report"。
> 方法：**只读**本仓库既有代码与本机设备枚举，不执行真机按键实验、不改产品代码。所有结论标注证据来源。
> 状态词汇：`passed` = 实际执行并观察通过；`failed` = 实际执行不满足预期；`deferred` = 依赖当前不可得条件。

> ⚠️ **2026-09-22 17:54 追加真机采集结论（见 §7），修正本文 §3 与 §5 的推论。**
> 真机实测表明：该设备在 Raw Input 里 **100% 走键盘通道**，**HID 通道一条报文都没到**。
> → 本文 §3 声称"Raw Input 的 HID 通道仍在报告层收到原始字节、这些键在管道里可见"为 **`failed`**。
> → §5 的"报告层清空覆盖全部按键（含 TV）"**前提不成立**，为 **`failed`**。
> **§7.6 已完成复核**（设备实例拓扑 + Raw Input 类型枚举 + 跨枚举树排除）：该设备在系统内
> 只存在 1 个 HID 实例且被 `kbdhid` 归入 Keyboard 类，**不存在可被用法页过滤的 HID 通道** →
> "报告层清空 `failed`" 已从观测推断升级为**设备枚举层面的确定性结论**。
> 保留原段落不删，以便对照推理为何出错。

> **脱敏说明（2026-09-22 22:1x，入库前处理）**：文中蓝牙 LE **设备地址**与完整 HID 设备接口路径中的地址段统一替换为 `<BT-ADDR>`；保留的 `ClassGUID` / 接口类 GUID / `VID&PID&REV` 均为**公开常量或产品标识**，实例 ID（`9&1748ac9e&0`）为 Windows PnP 生成的相对标识，不含设备地址、不指向个人身份。脱敏不改变任何结论。

---

## 1. 结论（先说结论）

**该设备在 Windows 上只枚举出 1 个 HID TLC，且仅有键盘页（`UP:0001_U:0006`）。因此 TV 的 Shell 协议动作必然经由这唯一的一条 HID 报告通道触发——报告层清空对 TV 键同样有效。**

> ⚠️ **本文初稿的结论与此相反**（曾写"清空大概率无法消除 TV 动作"）。该初稿结论基于"键盘边沿被吞下后动作仍发生"这一观察所作的推理；本轮补做设备枚举后，推理方向被证据推翻，已按证据更正。保留此说明以符合"证据优先于推理"的复盘规则。

> ⚠️⚠️ **第二节更正（真机采集后）**：上面这句"报告层清空对 TV 键同样有效"**不成立**。设备枚举事实（只有 1 个键盘页 TLC、无消费页）得到真机支持，但由此**推不出**"报告层能拿到这份报告"——真机显示 HID 通道**完全收不到**该设备的报文。详见 §7。

**关键证据（本机实测枚举，2026-09-22）**：

| # | 事实 | 证据 |
| --- | --- | --- |
| 1 | RC003（`VID_2717/PID_32B8/REV_00A4`）在系统内**只有 1 个 HID 设备实例** | `HKLM\SYSTEM\CurrentControlSet\Enum\HID\{00001812-...}_Dev_VID&012717_PID&32b8_REV&00a4_<BT-ADDR>\9&1748ac9e&0&0000` 是唯一子键 |
| 2 | 该实例的 `HardwareID` 只声明**键盘页** | 原始值含 `HID\VID_2717&UP:0001_U:0006 HID_DEVICE_SYSTEM_KEYBOARD HID_DEVICE_UP:0001_U:0006` |
| 3 | 驱动服务为 `kbdhid`，类为 Keyboard | `Service = kbdhid`；`ClassGUID = {4d36e96b-e325-11ce-bfc1-08002be10318}`；`DeviceDesc = HID Keyboard Device` |
| 4 | **没有消费页（`0x0C`）TLC** | 同一设备键下无其它 HID 实例；全系统 `HID\VID_2717&UP:000C*` 不存在 |

**推论**：
1. 消费页 Raw Input 监听（`crates/sayall-windows/src/raw_input_windows.rs:381` 注册的 `0x0C/0x01`）**对该设备永远收不到报文**——该设备不发消费页报告。
2. 设备全部按键（返回 / 音量± / TV / 主页 / 方向 / OK / 菜单 / 电源）**共用同一份 9 字节键盘报告**，仅 usage 值不同。
3. 因此 TV 的 Shell 动作**不由独立通道触发**，而是由该报告内 TV 的 usage 经 Windows 翻译后产生 → **在报告层清空 usage，会同时阻断键盘翻译与 Shell 动作**。

## 2. 与本仓库既有实测记录的一致性

既有的真机记录（`Bugs/2026-09-10-win-l-mapping-and-capture.md:29-36`）称：

> TV DOWN/UP 已被门控成对吞下，泄漏计数不增长；随后即使等待 15 秒再锁屏仍会弹窗。……Windows 保留了 TV 的独立 Shell 协议动作。

**这并不与本文结论矛盾，而是互补**：

- `WH_KEYBOARD_LL` 钩子工作**在 VK 翻译之后**。把 VK 边沿从 OS 队列里摘掉，**并不能撤销已经产生或已经排队的 Shell 动作**——这正是 `lock_open_with_guard.rs:1-9` 记录的"Windows can retain a **separate shell protocol action**"，且延迟约 4 秒由 DCOM 落地。
- 而报告层清空工作**在 VK 翻译之前**（ZSTDJan `frida_hid_tap_runtime.py:498` 逐字注释 "clears the usage payload before Windows translates it"）→ 翻译从未发生，Shell 动作也就无从产生。
- 两者**层次不同**：LL 钩子是"事后摘除已翻译的边沿"，报告层清空是"从不产生边沿"。前者无法解决 TV，后者可以。

**这条区分是本文最有价值的结论**：它解释了为什么现行方案（LL 钩子 + 常驻抑制）对 TV 结构性无效，也解释了为什么报告层清空原则上能解决。

## 3. 为什么"返回 / 音量± 在 Windows 侧零事件"

> ⚠️ **本节推论已被真机采集推翻（2026-09-22 17:54，见 §7）。** 保留原文以便对照。
> 真机结果：HID 通道对该设备**完全收不到报文**（`hid_report_seen = 0`、`hid_usage_seen = 0`、零解码错误）。
> 因此下文"Raw Input 的 HID 通道仍在报告层收到原始字节 / 这些键在管道里是可见的"**为 `failed`**。
> 返回 / 音量± 在 **Raw Input 的两条通道（键盘 + HID）上都不存在**，不是"键盘孪生事件被 kbdhid 丢弃、HID 仍可见"。

同一份 report 里的 usage 若超出 Windows 键盘页的已知集合，`kbdhid` 会**丢弃**而不产生 VK——这就是"零事件"的机制。但 Raw Input 的 HID 通道**仍在报告层收到原始字节**，所以：

- `crates/sayall-windows/src/raw_input.rs:212` 只接受 `[0x01, 0x00, 0x00]` 前缀的 9 字节 report；
- `crates/sayall-windows/src/raw_input.rs:254-272` 已为 `0x00F1`（返回）、`0x0080/0x0081`（音量±）定义了 usage → 按键 映射。

即：**这些键在本仓库的 Raw Input 管道里是可见的**，只是它们的键盘孪生事件被 `kbdhid` 丢弃了。这与路线 A "从报告直接读、不依赖键盘栈" 的优势一致（选型文档 4.2 节第 3 点）。

## 4. 仍待真机确认的部分（`deferred`）

本文把"通道归属"从**未知**推进到**已知**，但下列事项仍需真机数据，不能由本文替代：

| # | 待确认 | 为什么静态取证不足 | 判据 |
| --- | --- | --- | --- |
| 1 | TV 按压对应的**具体 usage 值**是否为 `0x35` | 代码表是推测映射，未经真机报文验证 | 采集到 TV 报告后逐字节核对 |
| 2 | 报告层清空后 **Shell 动作是否真的消失** | 需要真实清空一次并观察锁屏/解锁后是否仍弹窗 | 无 `OpenWith.exe`、无协议选择器 |
| 3 | 清空对**物理键盘与其它应用**的影响 | 需在真实注入条件下回归 | 物理键盘全键照常 |
| 4 | 是否存在**第二个 report 形状**（如 7 字节 / 6 字节） | `raw_input.rs:211-218` 支持 3 种长度，实际用哪种未知 | 采集全部到达报文 |

## 5. 对路线选型的影响（更新）

> ⚠️ **本节第 1 行与第 3 行已被真机采集推翻，见 §7。** 下表保留原文以对照。

| 原判断（选型文档附录 C.2 第 1 项） | 更新后 |
| --- | --- |
| "TV 的独立 Shell 协议动作来源未知" | **来源已定位到该设备唯一的 HID 报告通道**（消费页通道不存在） |
| "这是推荐路线的最大不确定点" | 通道层面不再是阻塞项；剩余不确定项收窄为**清空动作本身在真机上的效果**（上表 1–4） |
| 可能"必须改走驱动路线" | **无此必要**：报告层清空覆盖该设备全部按键，包括 TV |

**对两级能力方案（选型文档第 9 节）的影响**：有权限档的 TV 键预期可从"禁用"变为"可映射、零原生"；无权限档按 9.4 决策维持现状不变。

## 6. 下一步

| 编号 | 动作 | 前置条件 | 判据 |
| --- | --- | --- | --- |
| E1-a' | 在现有 Raw Input 的 HID 分支补"未归因 usage"结构化日志，按压 TV / 返回 / 音量± 采集真实 usage 值 | 改产品代码（需授权）；无需额外环境 | 得到逐键 usage 值，核对 `0x35 / 0xF1 / 0x80 / 0x81` |
| E1-b | 在 HID 宿主内采全部 report（含长度分布） | 需 Frida Gadget 环境（本机 `frida` 未安装） | 得到完整 report 清单与长度 |
| E2 | 实施一次真实清空，观察 TV 的 Shell 动作与全部原生残留 | E1-a' / E1-b 完成 + 真机按键 | 无原生字符/光标/音量；锁屏无协议选择器 |
| E3–E6 | 见选型文档第 6 节 | — | — |

> 注：原选型文档第 6 节的 E1（"记录该设备全部 report ID 与原始字节"）目标可由 **E1-a' 低成本达成**——因为该设备只有一条通道、一种主要报告形状，无需 Frida 即可覆盖 TV / 返回 / 音量± 的 usage 采集。
>
> ⚠️ **该注已被真机推翻**：E1-a' **未采到任何数据**，因为该设备根本不在 HID 通道发报文。见 §7。

## 7. 真机采集结果（2026-09-22 17:54，**修正本文核心推论**）

### 7.1 实验条件

| 项 | 值 |
| --- | --- |
| 构建 | 工作树 `2d949f417b7f54cd79248097a0ab687299fbdb7d` 的 debug 构建，`ver=0.2.6`，`target\debug\sayall-windows-app.exe`（25,403,392 B） |
| 采集代码 | 在 Raw Input 的 **HID 分支**加"全量逐键采集"：`hid_report_seen`（每个 (报告长度, report id) 一次）与 `hid_usage_seen`（每个 usage 一次，不论是否已映射）；另保留原 `hid_usage_unmapped` / `hid_report_shape_unmapped` |
| 日志 | 独立文件 `e1-capture3.log`（`SAYALL_GATT_LOG` 覆盖，避免与其他实例混写） |
| 就绪证据 | `raw_input_listener action=start phase=completed terminal_result=passed matched_device_count=1 awaiting_remote_hid_interface=false` |
| 设备 | RC003，`VID_2717 / PID_32b8`，绑定的 Raw Input 路径含 `dev_vid&012717_pid&32b8_rev&00a4` |
| 操作 | 用户逐个按压实体键 |

### 7.2 观测结果

| 观测项 | 值 |
| --- | --- |
| `map_edges` 识别到的按键 | **Tv / Home / Menu / Ok / Up / Down / Left / Right / Power = 9 种**（每个 DOWN+UP，共 18 条边沿） |
| `map_fire` | 1（`button=Tv trigger=Single action=open_app`） |
| **`hid_report_seen`** | **0** |
| **`hid_usage_seen`** | **0** |
| `hid_usage_unmapped` | 0 |
| `hid_report_shape_unmapped` | 0 |
| HID 解码错误（decode / unsupported / invalid / truncated） | **0** |

**会话终审（采集实例退出后对整份日志全文件审计，`passed`）**：

采集实例 `pid=3124` 最终 **exit code = 0**（优雅退出，非强制终止）→ 未留下不干净的 BLE 会话。
对 `e1-capture3.log`（30,879 B / 121 行）做全文件终审，除上表四项外另加三条**更宽的兜底模式**：

| 兜底模式 | 命中 |
| --- | --- |
| 任何 `hid_` 前缀的 note | **0** |
| `decode_report` / `UnsupportedReportShape` | **0** |
| `RIM_TYPEHID` / `type=hid` / `dwType=2` 字样 | **0** |

即：**日志里连一处 HID 通道的痕迹都没有**，同时键盘通道有 18 条边沿 + 1 条 `map_fire`。
`map_edges` 按键逐项（字段格式 `detail=<按钮>=true\|false`，`true`=DOWN / `false`=UP）：

| 按钮 | Tv | Home | Menu | Ok | Up | Down | Left | Right | Power |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 出现次数 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 |

### 7.3 判别依据（字节级对应，非推理）

1. **实际触发的 9 个键与 `raw_input.rs:274-301` 的 `button_for_keyboard` VK 表逐项吻合**：
   `Right 0x27` / `Left 0x25` / `Down 0x28` / `Up 0x26` / `Ok 0x0D` / `Home 0x24` / `Menu 0x5D` / **`Tv 0xC0`** / `Power 0x5E,0x5F`。
2. **未触发的 4 个键全在 `virtual_key == 0xFF`（消费控制）分支**：
   `Back 0x6A` / `VolumeUp 0x30` / `VolumeDown 0x2E` / `VolumeMute 0x20`。该分支**从不触发**，与"返回 / 音量± 零事件"的既有债务一致。
3. **注册了但从未收到报文**：`raw_input_windows.rs:399-412` 注册 `0x01/0x06`（键盘）与 `0x0C/0x01`（消费控制）两个**用法页**。设备是 `kbdhid` 且 `HardwareID` 只声明 `UP:0001_U:0006` → 事件被 Windows 归为 **`RIM_TYPEKEYBOARD`**，HID 分支（`:888`）**不可达**。
4. **零解码错误**：HID 分支若执行过，即使报文异常也会留下 `*_unmapped` 或错误行。全 0 说明该分支**一次都没进**。

### 7.4 结论

- **`passed`**：该设备在 Raw Input 里 **100% 走键盘通道**；HID 通道**完全收不到**它的任何报文。
- **`failed`**：本文 §3「HID 通道仍在报告层收到原始字节、返回/音量± 在管道里可见」——**不成立**。返回 / 音量± 在 **两条通道上都不存在**。
- **`failed`**：本文 §1/§5「报告层清空覆盖该设备全部按键（含 TV）」——**前提不成立**。报告层**拿不到**这份报告，清空无从下手。
- **`passed`**：§1 的设备枚举部分（只有 1 个键盘页 TLC、无消费页 TLC）**得到真机支持**——`0x0C/0x01` 注册确实从未收到报文。

### 7.5 对选型路线的实质影响

推荐路线（ZSTDJan 式"在 HID 宿主内清空报告"）的**前提是该设备走 HID 通道**。真机显示它**不走**。
因此：

- 对 **TV / 主页 / 方向 / OK / 菜单 / 电源**：全部落在键盘通道。**"按设备在源头替换"的目标只能作用于键盘通道**；而 `RIM_TYPEKEYBOARD` **不含设备身份**——这正是本仓库 `key_gate.rs` 只能"整键盘吞"的根因，也是选型文档要解决的原始问题。
- 对 **返回 / 音量±**：Raw Input 两条通道均不可见（`kbdhid` 丢弃 + 无消费页），**与既有债务一致**。
- **"报告层清空"应下调为 `failed`**（§7.6-1 复核后已确认为确定性 `failed`，非推测）。

### 7.6 复核 §7.6-1：该设备是否真的从不产生 `RIM_TYPEHID`？

**问题**：`RIDEV_INPUTSINK` 只收注册过的用法页；若设备实际以其它用法页发出 HID 报告，会被静默丢弃——**观测结果与"从不产生 HID"相同，但结论相反**。故必须排除。

**复核方法（三步，全部只读、不提权、不编译）**：

#### 7.6-1-a Raw Input 设备枚举（`GetRawInputDeviceList` + `RIDI_DEVICEINFO`）

枚举本机 13 个 Raw Input 设备，目标设备（名字含 `VID&012717_PID&32b8`）的结果：

| 字段 | 值 |
| --- | --- |
| 列表 `dwType` | **1 = `RIM_TYPEKEYBOARD`** |
| `RIDI_DEVICEINFO` 的 `dwType` | **1 = `RIM_TYPEKEYBOARD`**（交叉验证一致） |
| `cbSize` / 返回长度 | 32 / 32 |
| 设备路径 | `\\?\HID#{00001812-...}_Dev_VID&012717_PID&32b8_REV&00a4_<BT-ADDR>#9&1748ac9e&0&0000#{884b96c3-56ef-11d1-bc8c-00a0c91405dd}` |
| 路径末段 GUID | `{884b96c3-...}` = **Keyboard 设备接口类 GUID**（HID 接口类为 `{4d1e55b2-...}`） |

> **读数更正（重要）**：首版探针在该设备上打印出 `usage_page=0x000C, usage=0x0000`，看似"消费页"。
> 经原始字节核对，这是**读越界**：`RID_DEVICE_INFO_KEYBOARD` 占 24 字节（6 个 DWORD），
> 而 union 里 `RID_DEVICE_INFO_HID` 仅占 16 字节；`usUsagePage` 的偏移落在键盘段的
> `dwNumberOfKeysTotal = 1023 (0x03FF)` 字节上。原始字节：
> `20 00 00 00 | 01 00 00 00 | 51 00 00 00 00 00 00 00 01 00 00 00 0C 00 00 00 03 00 00 00 FF 03 00 00`
> → 正确的读法是 `dwType=1 (KEYBOARD)`，后 24 字节是**键盘段**（`dwType=0x51` 等），
> **不是** usage page。**结论：该设备没有消费页 HID 归类的证据。**

#### 7.6-1-b 注册表 HID 拓扑（`HKLM\SYSTEM\CurrentControlSet\Enum\HID`）

| 项 | 值 |
| --- | --- |
| 匹配 `VID_2717` + `PID_32B8` 的顶层键 | **恰好 1 个**：`{00001812-...}_Dev_VID&012717_PID&32b8_REV&00a4_<BT-ADDR>` |
| 该键下实例数 | **1 个**：`9&1748ac9e&0&0000` |
| `Service` | **`kbdhid`**（键盘类过滤驱动） |
| `ClassGUID` | `{4d36e96b-e325-11ce-bfc1-08002be10318}` = **Keyboard 设备类**（非 HIDClass `{745a17a0-...}`） |
| `DeviceDesc` | `@keyboard.inf,%hid.keyboarddevice%;HID Keyboard Device` |

#### 7.6-1-c 跨枚举树排除第二实例

| 枚举树 | 子键数 | 匹配 `VID_2717`+`PID_32B8` |
| --- | --- | --- |
| `Enum\USB` | 24 | **0** |
| `Enum\BTHENUM` | 4 | **0** |
| `Enum\BTHLE` | 2 | **0** |

#### 7.6-1 结论

- **`passed`**：该设备在系统内**只存在唯一一个 HID 实例**，且被 `kbdhid` 接管、归入 **Keyboard 类**。
  它**不作为 HIDClass 设备存在** → **不存在"以 HID 类型注册、被用法页过滤"这条路径**。
- 因此"Raw Input 的 HID 分支收不到报文"**不是用法页过滤造成的假阴性**，而是**根本不存在 HID 通道**：
  Windows 从设备实例层就把它划给了键盘栈。
- 据此，§7.4 的「报告层清空 `failed`」由"观测推断"升级为**设备枚举层面的确定性结论**（`passed` 级证据），
  **不再是"可能过强"**。

**方法论备注**：`RIDEV_INPUTSINK` 的用法页过滤假阴性风险确实存在，但**排除它的正确手段是查设备实例拓扑，而不是注册通配用法页**——
后者若设备根本无 HIDClass 实例，注册通配也收不到任何东西，仍无法区分两种情况。本节采用的是前者。

### 7.7 剩余待确认（`deferred`）

| # | 问题 | 为什么仍待确认 | 复核方法 |
| --- | --- | --- | --- |
| 1 | TV 的 Shell 动作究竟在**哪一层**产生？ | 已确认不经 HID，则必在 `kbdhid` 的 VK 翻译或其后的 Shell 协议层——这决定还有没有"源头拦截"的可能 | 结合 §7.6-1 结论重做动作归因实验（需真实清空或替换通道） |
| 2 | RC001 是否同样只有键盘页实例？ | 本文全部取证基于 RC003 真机；RC001 须单独验收（`AGENTS.md` 产品范围要求） | 对 RC001 重复 §7.6-1-a/b/c |

---

## 附：证据索引

| 论述 | 证据 | 复核级别 | 状态 |
| --- | --- | --- | --- |
| 该设备只有 1 个 HID 实例、仅键盘页 | `HKLM\SYSTEM\CurrentControlSet\Enum\HID\{00001812-...}_Dev_VID&012717_PID&32b8_REV&00a4_<BT-ADDR>\9&1748ac9e&0&0000` 的 `HardwareID`（本机枚举，2026-09-22） | 本人实测 | `passed` |
| 服务为 `kbdhid`、类为 Keyboard | 同上键的 `Service` / `ClassGUID` / `DeviceDesc` | 本人实测 | `passed` |
| 无消费页 TLC | 同上键下无其它子键；PnP 查询 `VID&012717_PID&32B8` 仅 1 条 HIDClass 条目 | 本人实测 | `passed` |
| 消费页监听已注册 | `crates/sayall-windows/src/raw_input_windows.rs:374-383,485-500` | 本人逐行 | `passed` |
| 解码只认 `010000` 前缀 9 字节（另支持 7/6 字节） | `crates/sayall-windows/src/raw_input.rs:210-228` | 本人逐行 | `passed` |
| 返回/音量±/TV/主页 的 usage 映射 | `crates/sayall-windows/src/raw_input.rs:254-272` | 本人逐行 | `passed` |
| TV 边沿被吞后 Shell 动作仍发生（LL 层局限） | `Bugs/2026-09-10-win-l-mapping-and-capture.md:29-36` | 既有真机记录 | `passed` |
| 清空发生在 Windows 翻译之前 | ZSTDJan `frida_hid_tap_runtime.rs:498` 及其 `:50-51` 注释（选型文档附录 B 已固定 SHA） | 引自已归档文档 | `passed` |
| **该设备全部按键在 Raw Input 里走键盘通道** | `e1-capture3.log`：9 键全部产生 `map_edges`，VK 与 `button_for_keyboard` 逐项吻合 | **本人真机实测** | **`passed`** |
| **HID 通道收不到该设备任何报文** | 同上：`hid_report_seen=0`、`hid_usage_seen=0`、零解码错误 | **本人真机实测** | **`passed`** |
| **HID 通道零报文的会话终审（含三重兜底模式）** | 采集实例 `pid=3124` 退出（exit 0）后对 `e1-capture3.log` 全文件审计：4 项采集行 + `hid_` 前缀 + `decode_report` + `RIM_TYPEHID` 字样**全部 0**；同期 18 条键盘边沿 | **本人实测**（`e1-probe/e1-final-audit.py`） | **`passed`** |
| ~~"HID 通道仍收到原始字节、返回/音量± 可见"~~ | — | — | **`failed`**（§7.4） |
| ~~"报告层清空覆盖含 TV 的全部按键"~~ | — | — | **`failed`**（§7.4；§7.6-1 复核后为确定性） |
| **该设备在 Raw Input 设备列表里是 `RIM_TYPEKEYBOARD`** | `GetRawInputDeviceList` + `RIDI_DEVICEINFO` 双路交叉：`dwType` 均为 1；设备接口 GUID `{884b96c3-...}` = Keyboard 类 | **本人实测**（`e1-probe/rawinput-types2.py`） | **`passed`** |
| **该设备在系统内只有 1 个 HID 实例，`Service=kbdhid`、`ClassGUID` = Keyboard 类** | `HKLM\...\Enum\HID\{00001812-...}_Dev_VID&012717_PID&32b8_REV&00a4_<BT-ADDR>` 唯一子键 `9&1748ac9e&0&0000` 的 `Service`/`ClassGUID`/`DeviceDesc` | **本人实测**（`e1-probe/rawinput-topology.py`） | **`passed`** |
| **USB / BTHENUM / BTHLE 下均无该设备第二实例** | 三个枚举树匹配数均为 0 | **本人实测**（同上） | **`passed`** |
| ~~"该设备可能被 `RIDEV_INPUTSINK` 用法页过滤"~~ | 设备实例层就无 HIDClass 归类，不存在可被过滤的 HID 通道 | — | **`failed`**（§7.6，假阴性风险已排除） |
| TV 的 Shell 动作具体产生层（`kbdhid` 翻译 vs Shell 协议） | — | — | **`deferred`**（§7.7-1） |
| RC001 是否同样只有键盘页实例 | — | — | **`deferred`**（§7.7-2） |
| TV 实际 usage 值 / 清空真机效果 | — | — | **`deferred`**（采集未获数据，因通道不对） |
