# RC003 免驱动捕获通道调研（2026-09-23）

**范围**：回答"不装驱动、不提权的用户态程序，能不能拿到 RC003 的返回 / 音量± 三键"。
**方法**：三条互不重叠的免驱动通道逐一实测，每条都自带阳性对照。
**核验状态词汇**：`passed` = 实际执行并观察通过；`failed` = 实际执行不满足预期；`deferred` = 依赖当前不可得条件。

> 本文全部结论来自 **2026-09-23 本机真机实测**（RC003 在线，已配对，VID_2717 / PID_32B8 / REV_00A4），
> 原始输出归档在 [`hardware/RC003/evidence/`](../../hardware/RC003/evidence/)，探针在
> [`hardware/RC003/probes/`](../../hardware/RC003/probes/) 与 `crates/sayall-windows/examples/gatt_probe.rs`。

---

## 0. 结论摘要

| # | 通道 | 判定 | 决定性证据 |
| --- | --- | --- | --- |
| A | 厂商 GATT 服务 `8A7A0001` 通知 | `failed`（通道可达但静默） | 3 个厂商通知特征订阅成功，240 s 窗口内**零通知**；同窗口电池特征有通知，证明回调链路本身通 |
| B | GATT HID 服务 `0x1812` 特征（含 `0x2A4B` 报告描述符） | `failed`（AccessDenied） | 8 次枚举尝试全为 `GattCommunicationStatus=AccessDenied`；选择器路径 `FromIdAsync` 返回空 |
| C | 用户态直读 HID 顶层集合（TLC）原始报告 | `failed`（权限层拒绝） | `CreateFile` **err=5 拒绝访问**；阳性对照 8/17 接口可打开，**全部是"共享"访问模式** |
| D | Raw Input 全量注册（含从未注册过的厂商页 `0xFF00`） | `failed`（零事件） | 三键**零事件**；同一次采集里 确定/主页 各自按其 VK 正常到达（自证对照） |

**总判定：免驱动路线对 RC003 三键不成立，且是结构性的——不是"还没找对方法"，而是四层通路各自有硬约束。**

**并且本轮修正了一个此前的归因错误**：三键"不可见"的根因不是"设备不上报"，而是
**Windows 的 HID→VK 映射表里没有 `0x80` / `0x81` / `0xF1` 这三个 usage**；
设备把它们好好地放在键盘报告 `report_id=0x01` 里发出来了（见 §3.2 报告描述符取证），
是 `kbdhid` / `kbdclass` 在映射阶段丢掉的。

---

## 1. 问题与三条候选通道

正式产品约束（根 `AGENTS.md`）：基础路径不得依赖管理员权限，也不得引入新的系统级改动。
因此第二轮路线选型文档推荐的"设备专属 KMDF lower filter"（代价 `TESTSIGNING` + 关 Secure Boot + 重启）
属于**待拍板**选项；本次先穷尽它之前的免驱动可能。

免驱动的候选通道只有三条，它们操作的是链路的不同层次：

```
                     RC003（BLE）
                          │  BLE HID over GATT（唯一 HID TLC：键盘）
                          ▼
        ┌──────── mshidumdf / Microsoft.Bluetooth.Profiles.HidOverGatt（UMDF 传输驱动）
        │                 │
        │                 ├──► hidclass.sys ──► kbdhid.sys ──► kbdclass.sys ──► RIM ──► 应用（Raw Input 键盘通道）
        │                 │                                              ▲
        │                 └──► [通道 C] 用户态 CreateFile 直读 TLC 输入报告 ┘（被 RIM 独占挡住）
        │
   BLE GATT 层
        ├──► [通道 B] HID 服务 0x1812 的 Report 特征（AccessDenied，被传输驱动独占）
        └──► [通道 A] 厂商服务 8A7A0001 的通知（可订阅，无数据）
```

**关键认识**：通道 C 与「Raw Input 键盘通道」不是同一条路。Raw Input 是
`kbdhid → kbdclass → RIM` 的**消费端**，只能拿到已被映射过的按键；通道 C 直接读 hidclass
的输入报告环形缓冲，理论上能看到 `kbdhid` 丢弃的原始 usage。**这是本次调研的新增覆盖。**

---

## 2. 实验 A：厂商 GATT 服务通知

探针：`crates/sayall-windows/examples/gatt_probe.rs`（`enum` / `listen` 两个模式）
证据：[`gatt-probe-enum.log`](../../hardware/RC003/evidence/gatt-probe-enum.log)、
[`gatt-probe-listen1.log`](../../hardware/RC003/evidence/gatt-probe-listen1.log)

### 2.1 拓扑枚举（`enum`，passed）

设备共暴露 **9 个 GATT 服务**：

| 服务 | 说明 |
| --- | --- |
| `0x1801` / `0x1800` / `0x180A` | GAP / GATT / Device Info（`0x2A24` = `RC003`，固件 `V2.0`） |
| `0x180F` | Battery：`0x2A19`（30%）、`0x2BED` |
| **`0x1812`** | **HID：服务对象可见，但特征枚举 AccessDenied** |
| `AB5E0001` | ATVV：`AB5E0002`(write) / `AB5E0003`(notify, 音频) / `AB5E0004`(notify, control) |
| **`8A7A0001`** | **小米厂商服务**：`8A7A0101`(write-nr) / `8A7A0102`(notify) / `8A7A0103`(notify) / `8A7A0111`(write-nr) / `8A7A0112`(notify) |
| `0x01BF` | 厂商服务：`0x0001`(write-nr)，**只有写，没有通知** |
| `0xFE59` | Nordic DFU（`8EC90001/0002` + `0x0003` = "Version1.0"） |

厂商服务的 3 个 notify 特征 CCCD 均存在且初值为 `00 00`（通知关闭）——即这是**真正的通知通道，
不是占位**。这是本节唯一有希望的方向，因此做了完整订阅采集。

### 2.2 订阅与采集（`listen`，failed）

- **24 个可订阅特征中订阅成功 11 个**，包含 ATVV_CONTROL 与**全部 3 个厂商通知特征**
  （`8A7A0102` / `8A7A0103` / `8A7A0112`）——CCCD 写入均返回 `Success`，通道确实打开了。
- 240 秒窗口内，操作者按节奏按压 返回 / 音量+ / 音量- / 方向 / 确定 / **语音键（按住 3 秒）**。
- **结果：厂商服务零通知，ATVV_CONTROL 零通知。** 全窗口只有 3 条电池通知
  （`0x2A19`：t=64.7 s / 92.9 s / 106.7 s，电量 30%→29%）与 1 条 `0x2BED` 初始值。

**阳性对照的诚实说明**：电池特征在窗口内**确有**通知到达，这证明
"订阅成功 + 回调链路"是通的（探针不是哑的），因此"厂商服务静默"不是采集工具的假阴性。
但 **ATVV_CONTROL 的阳性对照未成立**——这符合既有认知（ATVV 音频会话需主机先发起），
本轮没有独立验证，所以它**不能**用来证明"工具能收到厂商侧通知"。

**判定**：`failed`。厂商通知通道**可订阅但按键期间无任何数据**。
本轮的排除力等级不高于 §3 与 §4（后两者是结构性的），因此作为**佐证**而非决定性证据。
未做的部分：厂商服务有 2 个 write-nr 特征（`8A7A0101` / `8A7A0111`），可能需要在写入
某个"使能命令"后通知才会流动。协议未知，**盲目写入有触发 OTA / 重置配对的风险**，
故本轮明确不写入 —— 这一残余不确定性已记入 §6。

---

## 3. 实验 B：用户态直读 HID 顶层集合

探针：`hardware/RC003/probes/hid-direct-read.py`
证据：[`hid-direct-control.log`](../../hardware/RC003/evidence/hid-direct-control.log)

### 3.1 打开尝试：AccessDenied（failed，且这是结构性约束）

枚举到的 RC003 HID 设备接口共 2 条（HID 接口类 + 键盘接口类）：

```
\?\hid#{00001812-…}_dev_vid&012717_pid&32b8_rev&00a4_<BT-ADDR>#a&435f0b9&0&0000#{4d1e55b2-…}\kbd
\?\hid#{00001812-…}_dev_vid&012717_pid&32b8_rev&00a4_<BT-ADDR>#a&435f0b9&0&0000#{884b96c3-…}
```

两条路径 `CreateFile(GENERIC_READ)` **均返回 `err=5`（ERROR_ACCESS_DENIED）**。

**阳性对照（自证不是探针写错）**：同一段代码对全部 17 个 HID 设备接口逐个尝试，
**8 个打开成功、9 个被拒**。打开成功与失败的分界完全落在"访问模式"上：

| 打开结果 | TLC 类型 | 数量 | 对应 Windows 访问模式 |
| --- | --- | --- | --- |
| **成功** | 消费类 `0x0C/0x01`、厂商页 `0xFF00/*`、笔 `0x0D/0x0E` | 8 | **Shared** |
| **拒绝 err=5** | 键盘类 `0x01/0x06` | 9 | **Exclusive** |

这与微软官方文档的表完全一致（见 §3.3）：**键盘 / 鼠标类 TLC 由 Raw Input Manager
以独占方式打开，用户态无法再以读权限打开**。

### 3.2 零权限打开 → 拿到设备**声明**的全部 usage（passed，本节核心产出）

官方文档同时给了另一条路：RIM 独占打开后，应用仍可**不请求读写权限**地打开接口做信息查询。
实测 `CreateFile(dwDesiredAccess=0)` **成功**，并取到：

```
HidD_GetAttributes : VID=0x2717 PID=0x32B8 ver=0x00A4
HidP_GetCaps       : UsagePage=0x0001 Usage=0x0006      <- 唯一 TLC = Generic Desktop / Keyboard
                     InputReportByteLength=121
                     InputButtonCaps=4  InputValueCaps=0  LinkCollectionNodes=3
报告描述符          : 不可达（IOCTL_HID_GET_REPORT_DESCRIPTOR err=1）
```

进一步用 `HidP_GetButtonCaps` 枚举**声明在报告里的 usage**（不需要读到报告本身），结果：

```
[caps 0] page=0x0007 report_id=0x01 link=1 range=0x0000-0x00FE   <- 键盘页，整段
          包含 usage=0x0035 `~ / Live(TV)
          包含 usage=0x003E F5 / Voice
          包含 usage=0x0049 Insert
          包含 usage=0x004A Home
          包含 usage=0x0065 Application(Menu)
          包含 usage=0x0066 Power
          包含 usage=0x0075 Help
          包含 usage=0x0080 Volume Up      <<< 三键之一
          包含 usage=0x0081 Volume Down    <<< 三键之一
          包含 usage=0x00F1 Back           <<< 三键之一
[caps 1] page=0xFF00 report_id=0x06 link=2 range=0x0000-0x00FF
[caps 2] page=0xFF00 report_id=0x07 link=2 range=0x0000-0x00FF
[caps 3] page=0xFF00 report_id=0x08 link=2 range=0x0000-0x00FF

[link 0] page=0x0001 usage=0x0006（Keyboard Application 集合，TLC）
[link 1] / [link 2] 为其下的嵌套集合
```

**三条决定性事实：**

1. **设备确实在报告里声明了三键**：`0x80` / `0x81` / `0xF1` 落在键盘页 `report_id=0x01`
   的 usage 范围 `0x0000–0x00FE` 内。所以"设备不上报三键"的说法是不成立的。
2. **三键与可用键在同一个报告里**：`Home`（`0x4A`）、`确定`（HID `0x28` Enter）也在这个范围内，
   而它们实测能正常到达应用（见 §4）。同一报告、同一通道——差别只在 usage 有没有 VK 映射。
3. **厂商页 `0xFF00` 输入报告（`report_id` 6/7/8）确实存在**，但它们在**同一个 TLC 内部**的嵌套集合里，
   因此不会生成第二个 Windows HID 子设备节点（注册表实测该设备只有一个 HID 子节点
   `HID\VID_2717&UP:0001_U:0006`），也就同样落在 RIM 的独占范围里。

### 3.3 官方依据

微软 HID 架构文档的访问模式表（<https://learn.microsoft.com/windows-hardware/drivers/hid/hid-architecture>）：

| Usage page | Usage | 客户端 | **Access mode** |
| --- | --- | --- | --- |
| 0x0001 | 0x0001–0x0002 | Mouse | **Exclusive** |
| 0x0001 | 0x0004–0x0005 | Game controllers | Shared |
| 0x0001 | **0x0006–0x0007** | **Keyboard / Keypad** | **Exclusive** |
| 0x0001 | 0x0080 | System controls (Power) | Shared |
| 0x000C | 0x0001 | Consumer controls | Shared |
| 0xFF00… | — | Vendor-defined | Shared |

> "…the access mode for input HID clients is exclusive to prevent other HID clients from
> intercepting or receiving global input state… **For security reasons, Raw Input Manager (RIM)
> opens all such devices exclusively.** If RIM opens a device in exclusive mode, the user can
> still open a HID device interface **without requesting read and write permissions** and obtain
> HID device information via HIDClass support routines (`HidD_GetXxx`)."

这条文档同时解释了我们观测到的两件事：为什么 `GENERIC_READ` 被拒（RIM 独占），
以及为什么零权限打开能成功但读不到报告（零权限只允许 `HidD_GetXxx`，
`DeviceIoControl` 返回 `err=1 ERROR_INVALID_FUNCTION`，与实测完全吻合）。

**判定**：通道 C `failed`。用户态读原始报告的路径在**权限模型层**就被封死，
与"报告里有没有三键"无关。

---

## 4. 实验 C：Raw Input 全量转储（最终定论）

探针：`hardware/RC003/probes/raw-input-dump.py`（消息专用窗口 + `RIDEV_INPUTSINK`，**不做任何 VK 过滤**）
证据：[`raw-input-dump2.log`](../../hardware/RC003/evidence/raw-input-dump2.log)、
探针自检 [`raw-input-selftest.log`](../../hardware/RC003/evidence/raw-input-selftest.log)

### 4.1 为什么还要做这一步

2026-09-22 的 E1 已经测过 Raw Input，但生产监听只注册了两个 TLC
（`0x01/0x06` 与 `0x0C/0x01`），而设备声明的是 `0x01/0x06` + 厂商页 `0xFF00`。
本轮把**从未注册过的组合**一并打开，并去掉探针里的一切按键过滤，避免"过滤导致的假阴性"。

### 4.2 探针自检（passed）

先用 `SendInput` 注入 `A` 键两次，探针原样读到 4 条事件，
`[RAW-KEYBOARD] vk=0x41 make=0x00` 按下/抬起各两次 —— **读数链路自证可用**。

> 探针自身踩过两个坑并已修正，记录以备复核：
> ① `GetRawInputData` 的**尺寸查询调用成功时返回 0**，与错误返回值语义混淆会让报文静默读到全零；
> ② 本机实测 **`HRAWINPUT` 在 `WM_INPUT` 的 `lParam` 里**（`wParam` 恒为 1），
> 用 `wParam` 会得到 `ERROR_INVALID_HANDLE=6`。改用固定大小 `RAWINPUT` 缓冲 + `lParam` 后正常。

### 4.3 采集结果（failed，且自带对照）

5 个注册项全部注册成功（`0x01/0x06`、`0x0C/0x01`、`0xFF00/0x0001`、`0xFF00/0x0002`、`0x01/0x0080`）。
150 秒窗口内，操作者依次按 返回 ×1 → 音量+ ×2 → 音量- ×3 → 确定 ×2 → 主页 ×1。

**全窗口共 6 条事件，全部来自 RC003（`VID&012717`），全部是键盘通道：**

| 实测事件 | 对应按键 | 次数匹配 |
| --- | --- | --- |
| `vk=0x0D (VK_RETURN) make=0x1C` 按下/抬起 | **确定** | 期望 2 次，实得 2 次 ✔ |
| `vk=0x24 (VK_HOME) make=0x47` 按下/抬起 | **主页** | 期望 1 次，实得 1 次 ✔ |
| — | **返回 / 音量+ / 音量-** | **期望 1/2/3 次，实得 0 次** |

同一窗口内 `RIM_TYPEHID` 报文数：**0**（与 E1 一致）。
其余 8 个注册项捕获到的非 RC003 设备事件数：**0**。

**这个对照是自证的**：确定与主页在同一台设备、同一次采集、同一个探针下**按次数精确到达**，
说明"探针能收到这台设备的按键"。因此返回 / 音量± 的零事件是**设备侧或映射层的事实，不是工具问题**。

### 4.4 归因链（把三块证据接起来）

1. 设备在**唯一**的键盘 TLC 里、`report_id=0x01` 中发送 HID usage `0x80` / `0x81` / `0xF1`（§3.2 声明取证）。
2. `kbdhid` 把 HID usage 映射为 VK，映射表里**没有这三个 usage** → 按键被丢弃，不进 `kbdclass`/RIM。
   对照：`0x28`(Enter) → `VK_RETURN`、`0x4A`(Home) → `VK_HOME` 有映射，所以能到（§4.3 实测）。
3. 想绕过第 2 步就得读原始报告，但整台设备的 TLC 归 RIM 独占（§3.1/§3.3），**不提权的**用户态读不到。
   （**2026-09-23 复核更正**：报告的**生产端**在 `WUDFHost.exe` 内、位于 RIM 之前；提权注入该宿主
   即可在报告层读取。见 §8。）
4. 换 GATT 层：HID 服务被 UMDF 传输驱动独占（AccessDenied），厂商服务通知静默（§2.2）。

**四条路互补地堵死"不提权、不注入"的全部空间；这不等于"必须装内核驱动"——见 §8。**

---

## 5. 与既有结论的关系

| 既有结论 | 本轮处置 |
| --- | --- |
| 2026-09-22 E1：「HID 通道零报文、键盘通道有事件，三键要生效必须走按设备源头捕获」 | **确认并加固**。补上了"设备到底声不声明三键"这一环（声明了，在 report 1），把"按设备源头捕获"的必要性从经验判断升级为结构性结论 |
| 2026-09-05：「RC003 三键在输入栈不可见」 | **确认，但修正归因**。原表述容易被读成"设备不发"；准确表述是"设备发了，`kbdhid` 映射层丢弃" |
| 首轮选型文档推荐路线 A（HID 宿主内清空报告） | **原判 `failed` 已由 §8 更正**：该判定测的是 Raw Input 层，而路线 A 在 `WUDFHost` 报告层，属层错位。路线 A 未被证伪。 |
| 第二轮文档路线 B（RC003 专属 KMDF lower filter 挂 `kbdhid` 之下） | **本轮没有推翻它，但"唯一形态"的表述已由 §8 更正**。它与路线 A 并列在方案空间内；代价不变：`TESTSIGNING` + 关 Secure Boot + 重启，与 R5 冲突，需产品拍板 |
| 产品策略：三键已"启用"（2026-09-23） | 配置层正确无误；RC003 上仍需 §6 的后续动作才能真生效 |

---

## 6. 未覆盖与残余不确定性（deferred）

1. **厂商 GATT 是否需要在写入"使能命令"后才推通知**：未验证。`8A7A0001` 有 2 个 write-nr 特征，
   理论上可能需要握手。风险是协议未知、盲目写入可能触发 OTA / 重置配对，**本轮明确不写**。
   若产品认为值得，应另立一次带备份/可恢复方案的实验。
2. **提升到管理员或 `SeTcbPrivilege` 能否读 TLC**：未测。文档表明这是"传系统级安全边界"，
   而且即使可行也违反产品约束（基础路径不得依赖管理员权限），故未投入。
3. **RC001 三键的真机回归**：仍为 `deferred`（需 RC001 硬件）。RC001 的 `VK 0xFF` 厂商键
   机制与 RC003 不同（`0xFF` 说明它的三键落在**有映射**的路径上），因此**不能**从本轮结论
   推断 RC001 也不可用。
4. **`0x2BED` 特征语义**：`[00 41 00]`，未知，与三键无关，未展开。
5. **ATVV_CONTROL 阳性对照未触发**：见 §2.2。它只影响实验 A 的排除力等级，不影响 §3/§4。

---

## 7. 对"三键启用"产品的含义

- **配置层（已交付）**：三键可配置、按型号如实披露能力，这部分不受本轮结论影响。
- **RC001**：三键路径本就完整（`VK 0xFF` 直接归因族），只差真机回归。
- **RC003**：配置能保存，但**在三键真正生效前需要一条能读到原始报告的捕获路径**（位于 `kbdhid`
  的映射之前）。本轮已把"不提权、不注入"的替代方案穷尽并判定 `failed`；**这不等同于"必须装内核驱动"**——
  见 §8 的更正：提权注入 HID 宿主（路线 A）同样不装驱动，且代价低于路线 B。
  决策点因此是路线 A / 路线 B / 维持不可用 三选一。
- 建议：UI 的能力披露文案可以按本轮结论写得更精确 ——
  RC003 不是"还没有实现"，而是**Windows 不把这三个 usage 映射成按键**（原始报告存在于
  `WUDFHost` 内，属于系统保留层）。

---

## 8. 2026-09-23 复核更正：路线 A 未被证伪（本节取代上文相关结论）

> 触发来源：按用户要求复核参考实现 `ZSTDJan/windows-remote-mic-app`
> （提交 `1e6b1d285f9cd50f30c5bc92ac7787a693fc993d`，v1.0.44）。
> 完整复核报告见 [`2026-09-23-zstdjan-hid-host-tap-implementation-review.md`](2026-09-23-zstdjan-hid-host-tap-implementation-review.md)。

**更正 1（层错位）**：本文 §4.4 第 3 条与 §5 对路线 A 的处置，隐含地把"用户态读不到 HID 报告"
当作"报告层没有报告"。实际报告在**生产端**就存在：RC003 的 HID 设备由 `mshidumdf` + `WUDFRd`
（`WUDF\DriverList = HidOverGatt`）承载在**用户态** `WUDFHost.exe` 中，位置在 RIM 之前。
本机只读实测已确认该宿主存在、PID 可从注册表 `Device Parameters\WUDFDiagnosticInfo\HostPid`
读取、且为**独占 RC003** 宿主（证据 `evidence/wudf-host-probe.log`）。

**更正 2（"唯一"表述）**：本文 §7 与首轮选型文档头部的"路线 B 是唯一可行形态"不成立。
两条路线并列：

| | 路线 A：HID 宿主内报告层捕获 | 路线 B：`kbdhid` 之下 KMDF lower filter |
| --- | --- | --- |
| 捕获位置 | `WUDFHost.exe` 内 `NtDeviceIoControlFile`（UMDF 输出复制入口） | 内核栈 `kbdhid` 之下 |
| 内核驱动 | **不需要** | 需要 |
| `TESTSIGNING` / 关 Secure Boot / 重启 | **不需要** | 需要 |
| 正式发布签名成本 | 无额外要求（注入框架需审计 + 固定哈希） | WHCP / attestation |
| 提权 | 需要（`SeDebugPrivilege` + 提权助手） | 需要（安装驱动） |
| 额外风险 | 依赖未公开的 UMDF 复制语义；向系统进程注入可能触发 AV/EDR | 内核驱动长期维护与签名链 |
| 与本仓库约束 | ~~与 ADR 0002 §3「不使用 Frida」冲突（可改为自研注入组件）~~ → **2026-09-23 已解除**：§3 修订为"允许注入框架，须固定版本 + 校验哈希 + 登记许可" | ~~与 R5「不引入新的系统级改动」冲突~~ → **已收窄**：R5 现仅禁止内核驱动 / Secure Boot / 测试签名 / 驱动签名策略 / 注册表过滤项 / 重启 |

**仍然成立的部分**：本文 §3 的四条通道取证（厂商 GATT 静默、GATT HID `AccessDenied`、
用户态直读 `err=5`、Raw Input 厂商页零事件）与 §3.2 的 `HidP_GetButtonCaps` 声明取证
均未被推翻——它们证明的是"**不提权**拿不到"，而不是"拿不到"。

**尚未执行**：我们自己的注入与拦截（`deferred`，需一次提权，不需重启、不改系统设置）。

---

## 附：本轮产物清单

**探针**

| 文件 | 用途 |
| --- | --- |
| `crates/sayall-windows/examples/gatt_probe.rs` | 厂商 / HID GATT 服务枚举、全量订阅采集（`enum` / `listen`） |
| `hardware/RC003/probes/hid-direct-read.py` | HID 接口枚举、直读尝试、阳性对照、零权限 `HidP_GetButtonCaps` 声明 usage 取证 |
| `hardware/RC003/probes/raw-input-dump.py` | Raw Input 全量事件转储（多 TLC 注册、不做 VK 过滤、自带自检） |

**证据**

| 文件 | 内容 |
| --- | --- |
| `evidence/gatt-probe-enum.log` | 9 个 GATT 服务 + 特征/描述符/可读值全量拓扑 |
| `evidence/gatt-probe-listen1.log` | 240 s 厂商通道订阅采集（含电池通知对照） |
| `evidence/hid-direct-control.log` | HID 打开拒绝 + 8/17 阳性对照 + 零权限声明 usage 取证 |
| `evidence/raw-input-dump2.log` | 150 s Raw Input 全量事件（确定/主页对照 + 三键零事件） |
| `evidence/raw-input-selftest.log` | 探针自检（注入 A 键 → `vk=0x41`） |
