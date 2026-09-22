# 按设备按键源头替换：路线重选（2026-09-22 第二轮研究）

> **触发原因**：首轮选型的推荐路线（ZSTDJan 式"HID 宿主内清空报告"）经真机采集判定 **`failed`**——RC003 在 Raw Input 的 HID 通道上零报文，报告层拿不到它的报告。详见 `evidence/2026-09-22-tv-shell-action-channel-analysis.md` §7。
> **本轮目标**：重新回答"在 `kbdhid` 之下，哪一层能同时拿到**设备身份**与**原始 HID 报告**，并在源头完成替换"。
> **状态词汇**：`passed` = 本机实测通过 / `failed` = 实测不成立 / `deferred` = 依赖当前不可得条件 / `cited` = 引官方文档或第三方源码（带出处）。
> **纪律**：本文为研究文档，未授权开发。第 7 节的实验 E2-1 ~ E2-4 全部未执行。

---

## 1. 结论（先说结论）

**推荐路线改为：RC003 设备专属 KMDF lower filter，挂在 `kbdhid` 之下，在 `IRP_MJ_READ` 完成后原地改写 HID 报告的 usage 字节。**

这条路线同时满足用户提出的四个硬条件：

| 用户条件 | 本路线是否满足 | 依据 |
| --- | --- | --- |
| 解决**所有**按键副作用 | **是**——在 `kbdhid` 把 usage 翻译成 VK **之前**替换，原生语义（TV 的 Shell 动作、主页键）从不产生 | §3.2、§4 |
| 纯自定义 | **是**——用户态只接收被改写成 F13–F24 的设备专属源键，自由映射 | §3.3 |
| 不引入其他副作用 | **是**——经 Extension INF 绑定 RC003 的 HardwareID，**物理键盘/鼠标完全不受影响** | §3.1 |
| 返回 / 音量± 零事件 | **是**——这三个键在 `kbdhid` 层本被丢弃，filter 在其**下方**，因此可先一步改写 | §3.2 |

**与前一轮的关键差异**：首轮推荐路线假设"设备走 HID 通道、在 HID 宿主内清空"。真机证明该假设不成立。本轮路线**不依赖 HID 通道**——它挂在**设备栈本身**上，因此绕开了"HID 通道对键盘 TLC 不可见"这个障碍。

**代价（必须让用户知情，见 §5）**：
- 内核驱动：Win10 1607+ 要求 Dev Portal 签名才可在**正常代码完整性模式**下加载；自用需 `TESTSIGNING`（须关 Secure Boot）。
- 安装需管理员权限 + 至少一次重启。
- 存在一个**可逆的**系统级改动（测试模式 + 受信任证书）。

**替代方案（免驱动的现状档）**：无管理员权限的用户维持现状（含 TV/主页 的既有行为）——与用户 2026-09-22 的两级能力决策一致，本文不改变该决策。

---

## 2. 上一轮为何失败（一句话）

RC003 只有一个 TLC（键盘页 `0x0001/0x0006`），**该 TLC 被系统独占**：

> "The system opens all keyboard and mouse collections for its exclusive use."
> "Raw Input Manager (RIM) opens all such devices exclusively."
> — [keyboard-and-mouse-hid-client-drivers](https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/keyboard-and-mouse-hid-client-drivers)（`cited`）

**独占发生在 `hidclass.sys` 之上**。Raw Input 从 TLC 的 PDO 打开，因此 HID 通道（`RIM_TYPEHID`）永远看不到这份报告。首轮路线要在 HID 宿主内清空，前提就不存在。

**推论**：要到"源头"，必须走到**独占层之下**，即 `kbdhid` 的下方。

---

## 3. 硬件事实（本机注册表逐条核对，`passed`）

### 3.1 RC003 是两级 PnP 设备树（**修正上一轮的表述**）

上一轮只查了 `Enum\HID`，漏掉了父节点。实际结构（`driverstack.out`，2026-09-22 18:2x）：

```
BTHLE\Dev_<BT-ADDR>                                     (PDO，蓝牙 LE 枚举)
 └ BTHLEDEVICE\{00001812-...}_Dev_VID&012717_PID&32b8_REV&00a4
     \8&14b2359b&b&0055                                    (父节点)
       ClassGUID    = {745a17a0-74d3-11d0-b6fe-00a0c90f57da}   ← HIDClass
       Service      = mshidumdf                                ← UMDF 用户态
       LowerFilters = WUDFRd                                   ← UMDF 反射器
       Mfg          = @hidbthle.inf → Bluetooth LE GATT HID
       ParentIdPrefix = 9&1748ac9e&0
    └ HID\{00001812-...}\9&1748ac9e&0&0000                 (子节点)
       ClassGUID = {4d36e96b-e325-11ce-bfc1-08002be10318}      ← Keyboard
       Service   = kbdhid
       HardwareID: ... | HID\VID_2717&UP:0001_U:0006 | HID_DEVICE_SYSTEM_KEYBOARD | ...
```

父子的连接证据：父节点 `ParentIdPrefix = 9&1748ac9e&0` 与子节点实例名 `\9&1748ac9e&0&0000` **严丝合缝**。

> **脱敏说明**：蓝牙 LE 设备地址（`BTHLE\Dev_` 后的 12 位十六进制）已替换为 `<BT-ADDR>`。文中保留的 `ClassGUID` / `ExtensionId` 均为**公开系统常量或第三方公开值**；实例 ID（`8&14b2359b&b&0055`、`9&1748ac9e&0`）是 Windows PnP 生成的**相对标识**，不含设备地址，也不指向个人身份。其余证据强度不受影响。

**修正**：上一轮写"该设备只在系统内存在唯一一个 HID 实例、不作为 HIDClass 设备存在"——**前半句对（HID 树下的确只有 1 个），后半句错**。它**有**一个 HIDClass 类的父节点，只是该节点的传输是 UMDF 的 `Microsoft.Bluetooth.Profiles.HidOverGatt.dll`，不是 `hidbth.sys`。
**这个修正很重要**：有 HIDClass 父节点，才有"在设备栈上挂 filter"的落点。

### 3.2 传输驱动不是 `hidbth.sys`（`passed`）

`hidbth.sys` 只服务**经典蓝牙** HID（`BTHENUM\{00001124-...}`，本机那个 `VID&0002046D_PID&B016` 才是）。RC003 走 **BLE HID over GATT**（`{00001812-...}` = HID Service UUID），传输是 UMDF 的 `HidOverGatt.dll`，宿主 `WUDFHost.exe`。

### 3.3 过滤驱动槽位（`passed`）

| 位置 | 当前值 | 可否挂 |
| --- | --- | --- |
| `Control\Class\{4d36e96b}`（Keyboard 类） | `UpperFilters = kbdclass`，无 `LowerFilters` | 类级，**全类生效**（不满足"按设备"） |
| `Control\Class\{745a17a0}`（HIDClass 类） | **无 UpperFilters、无 LowerFilters** | 类级，**全类生效** |
| RC003 `kbdhid` 子节点设备键 | 无设备级 filter 值 | **设备级 ← 本路线用这里**（HardwareID 前缀 `HID\`，见 §4.4 要点 4） |

`ConnectMultiplePorts` / `KeyboardDataQueueSize` 在 Keyboard 类键树下**命中 0 处** → 走 Windows 默认行为。

### 3.4 官方对 filter 位置的限制（`cited`）

微软**明确不推荐**在 `hidclass.sys` 与传输 minidriver 之间插 filter：

> "Filter drivers *aren't* recommended as a filter between HIDCLASS and HID transport minidrivers"
> — [keyboard-and-mouse-hid-client-drivers](https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/keyboard-and-mouse-hid-client-drivers)

官方**允许**的位置是"作为 `kbdhid`/`mouhid` 的 upper filter"或"作为 `kbdclass`/`mouclass` 的 upper filter"。
**但**：RemoteMapper 采用的**设备专属 lower filter（挂在 `kbdhid` 之下）**已在真机验收通过（§4.1）。这条路的性质是"官方不推荐"而非"官方禁止"，且**按设备绑定**解决了"全类生效"的问题。**风险须如实记录**（见 §6）。

---

## 4. 参考实现：RemoteMapper `MiRemoteHidFilter`（`cited`，逐字读源码）

参考仓库：`QL-4/RemoteMapper`，本机副本 `ref-repos/RemoteMapper/`（源码文件已逐字核对）。

### 4.1 栈位置与实现（`driver.c` 第 40-41 行）

```c
// Device-specific lower filter below kbdhid.sys.
WdfFdoInitSetFilter(DeviceInit);
```

栈序（其 README 第 58 行）：
```
kbdclass -> kbdhid -> MiRemoteHidFilter -> mshidumdf
```

实现方式：只注册 `EvtIoRead`（`driver.c:57`），转发 `IRP_MJ_READ`，在**下层完成后**（`MiRemoteReadCompletion`，`driver.c:97-142`）原地、**等长**改写 `report[3]`，然后**按原状态与原长度**完成请求（`driver.c:138-141`）。

**关键点**：它**不修改 Report Descriptor、不修改 Report ID、不修改报告长度**——只在 `report[3]` 这一个字节上做替换。这使它对协议是透明的。

### 4.2 实机 preparsed metadata（其 README 第 33-53 行，`cited`）

```
Top-level collection : Generic Desktop / Keyboard (0x0001/0x0006)
InputReportByteLength: 121
Keyboard Report ID   : 0x01
Vendor Report ID     : 0x06 / 0x07 / 0x08（不修改）
报告字节             : 01 00 00 <usage> 00 ...
```

字段语义（其 README 第 48-53 行）：
```
report[0] = Report ID（0x01）
report[1] = modifiers
report[2] = reserved
report[3] = 按下的键 usage
```

**这份 metadata 交叉验证了我们仓库的两处代码**（`passed`，独立来源）：
1. `raw_input.rs:210-228` 的 `decode_report_usages` 只接受 `[0x01,0x00,0x00]` 前缀 —— 与 `01 00 00 <usage>` 完全吻合。
2. `raw_input.rs:254-272` 把 `0x0035` 映射为 `Tv` —— RemoteMapper 把 `0x35` 标为 `HID_USAGE_KEYBOARD_LIVE`（直播/TV），**两处独立得出同一 usage 值**。
3. `raw_input_windows.rs:876` 对 `virtual_key == 0x74`（VK_F5）的特判 —— RemoteMapper `remap.c:17` 注释写"遥控器语音键以键盘页 usage `0x3E`（F5）到达"，**吻合**。

### 4.3 完整改写表（`remap.c:6-27`，`cited`）

| 物理键 | 原始 usage | 改写为 | 结果 VK | 我们侧的对应 |
| --- | --- | --- | --- | --- |
| 音量加 | `0x80` | F13 `0x68` | `0x7C` | `button_for_usage: VolumeUp` |
| 音量减 | `0x81` | F14 `0x69` | `0x7D` | `VolumeDown` |
| 返回 | `0xF1` | F15 `0x6A` | `0x7E` | `Back` |
| 主页 | `0x4A` | F16 `0x6B` | `0x7F` | `Home` |
| 菜单 | `0x65` | F17 `0x6C` | `0x80` | `Menu` |
| 直播/TV | `0x35` | F18 `0x6D` | `0x81` | `Tv` |
| 电源 | `0x66` | F19 `0x6E` | `0x82` | `Power` |
| 语音（F5） | `0x3E` | F20 `0x6F` | `0x83` | `None`（语音，`0x003E`） |

**这张表把我们的七个痛点键全部覆盖了**，包括前一轮判定"无解"的返回/音量±。

其 README 第 27 行解释了为什么主页/菜单/直播/电源也要改写：

> "后四个本可映射为 Home / Apps / OEM_3 / Power，但全局低级键盘钩子没有来源设备 ID，直接映射会误吞物理键盘的同名键。因此把它们改为 F16–F19，仅由此 VID/PID 的遥控器生成。"

**这正是我们 `key_gate.rs` 的原始痛点**（`WH_KEYBOARD_LL` 无设备身份 → 只能"整键盘吞"）。改写为设备专属 F 键后，用户态只需吞 F13–F24，**永远不会碰到物理键盘**。

### 4.4 挂载方式（`MiRemoteHidFilter.inf`，`cited`）

```inf
Class=Extension
ClassGuid={e2f84ce7-8efa-411c-aa69-97454ca4cb57}        ← Extension INF 类
ExtensionId={20c2709e-e4ab-4a6c-a436-8a6bbdbf35aa}
DriverVer=08/11/2026,1.0.1.0
PnpLockdown=1

[Manufacturer]
%ProviderName%=Models,NTamd64.10.0...18362              ← Win10 1903+（我们 19041 满足）

[Models.NTamd64.10.0...18362]
%DeviceDesc%=MiRemoteFilter_Install, HID\{00001812-...}_Dev_VID&012717_PID&32b8_REV&00a4
                                     ↑ 绑定 kbdhid 子节点（见下方更正）

[MiRemoteFilter_Install.Filters]
AddFilter=MiRemoteHidFilter,,MiRemoteLowerFilter
[MiRemoteLowerFilter]
FilterPosition=Lower

[MiRemoteFilter_Install.Wdf]
KmdfService=MiRemoteHidFilter,MiRemoteFilter_Wdf
[MiRemoteFilter_Wdf]
KmdfLibraryVersion=1.15
```

**四个要点**：
1. `Class=Extension` + 精确 HardwareID → **只作用 RC003 这一条设备栈**，不误伤物理键盘（其 README 第 13 行："它不会匹配其他物理键盘"）。
2. `AddFilter ... FilterPosition=Lower` → 设备专属 lower filter（Win10 1903+ 官方支持的挂载方式，[installing-a-filter-driver](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/installing-a-filter-driver)）。
3. `KMDF 1.15`、`NTamd64.10.0...18362` —— 与本机 Win10 19041 兼容。
4. **该 HardwareID 属于 `kbdhid` 子节点，不是父节点**（2026-09-22 18:3x 本机实测更正）：

   | 树 | 实例 | Service | HardwareID 前缀 |
   | --- | --- | --- | --- |
   | `BTHLEDEVICE` | `8&14b2359b&b&0055` | `mshidumdf` | **`BTHLEDevice\`** |
   | `HID` | `9&1748ac9e&0&0000` | **`kbdhid`** | **`HID\`** ← INF 匹配的这一族 |

   即：filter **装在 `kbdhid` 的设备栈上**，`FilterPosition=Lower` 使其位于 `kbdhid` **之下**——与 §4.1 的栈序 `kbdclass → kbdhid → MiRemoteHidFilter → mshidumdf` **自洽**。

### 4.5 实机验收记录（其 README 第 63-76 行，`cited`）

> "Windows 11 x64、**HVCI/内存完整性开启**时通过"

```
方向上 = VK_UP 0x26  PASS
音量加 = F13 0x7C   PASS
音量减 = F14 0x7D   PASS
返回键 = F15 0x7E   PASS
主页键 = F16 0x7F   PASS
菜单键 = F17 0x80   PASS
直播键 = F18 0x81   PASS
电源键 = F19 0x82   PASS
```

**八键 PASS，且 HVCI 开启**——说明该实现与内存完整性兼容（不需要关 HVCI）。

> ⚠️ **注意口径差异**：其验收在 **Windows 11 x64** 上完成；**本机是 Windows 10 19041.207**。跨版本可迁移性属本文未决项（§7 E2-4）。

### 4.6 签名与部署代价（其 README 第 117-160 行，`cited`）

```
1. 确认 Secure Boot 已关闭。
2. 管理员运行 prepare-test-mode.bat（信任测试证书 + 启用 TESTSIGNING）
3. 重启 Windows
4. 管理员运行 install-driver.bat
5. 若 Windows 更新了正在运行的驱动映像，再重启一次
6. 运行 verify-keys.bat
```

> "`package/` 是 WDK 测试证书签名的开发包。它需要 Windows TESTSIGNING；关闭测试模式后不能继续加载。"
> "要在关闭 TESTSIGNING 的正常代码完整性模式下继续使用 KMDF 版本，需要 Microsoft Hardware Dev Center attestation/WHCP 签名。UMDF 2 迁移仅作为后续研究方向，目前未实现。"

**回滚路径**（其 README 第 140-156 行，`cited`）：
```
卸载过滤器（uninstall-driver.bat）→ 重启并确认键盘栈恢复
→ restore-normal-mode.bat（关 TESTSIGNING + 移除证书）→ 再重启
```
> "不要在过滤器仍安装时直接关闭 TESTSIGNING，否则 Windows 会拒绝加载测试签名内核驱动。"

**这是干净的、双向可逆的部署路径**——满足用户"系统级动作先讲副作用、可逆"的要求。

### 4.7 对"两级能力"的参考（其 DEPLOY.md 第 63 行，`cited`）

> "权限：普通用户即可（**无需管理员**）"

RemoteMapper 的**主程序**（语音链路 + 按键映射）完全不需要管理员；**驱动是可选增强**（其 DEPLOY.md 第 12 行："若还需要……另行部署可选的 `driver/MiRemoteHidFilter`"）。

**这与用户 2026-09-22 的两级能力决策天然同构**：无管理员 → 不用驱动，维持现状；有管理员 → 装驱动，获得全部自定义能力。

---

## 5. 代价矩阵（必须让用户知情的部分）

| 项 | 内容 | 可逆性 |
| --- | --- | --- |
| 管理员权限 | 安装、卸载各需一次 | — |
| 重启 | 装 1 次 + 可能需要第 2 次 | — |
| **Secure Boot** | 测试签名路径需**关闭** | **可逆**（`restore-normal-mode.bat` 后可再开） |
| **TESTSIGNING** | 开启测试模式 | **可逆**；⚠️ 开启期间桌面右下角**常驻"测试模式"水印**（微软官方已知行为） |
| **受信任证书** | 把 WDK 测试证书装入受信任发布者 | **可逆**（`restore-normal-mode.bat` 移除） |
| HVCI / 内存完整性 | **可保持开启**（其验收即在 HVCI 开启下通过） | — |
| 正式发布 | 要分发则需 **Microsoft Hardware Dev Center attestation / WHCP 签名**（KMDF 为内核驱动） | — |

> **对比首轮推荐路线**：首轮路线（HID 宿主内清空 + Frida Gadget）需要**常驻管理员注入宿主**；本路线只在**安装期**需要管理员，日常运行零提权。**这是本路线相对首轮的实质性优势。**

---

## 6. 风险与不确定（诚实记录）

| # | 风险 | 级别 | 处置 |
| --- | --- | --- | --- |
| 1 | **官方不推荐**在 `hidclass` 与传输 minidriver 之间插 filter | 中 | 本路线挂在 `kbdhid` 之下（`kbdhid` 之上是 `kbdclass`），**严格说属于"HIDClass 之下、kbdhid 之下"**——与官方警告所指的"hidclass 与传输 minidriver 之间"不完全同一位置；但**边界模糊，须实测确认**（§7 E2-1） |
| 2 | 参考实现验收于 **Windows 11**，本机是 **Win10 19041** | 中 | §7 E2-4 跨版本验证 |
| 3 | 报告长度 **121 字节**（`InputReportByteLength`），非我们代码假设的 9/7/6 | 中 | 我们现有 `decode_report_usages` 只认 9/7/6——**若采用本路线，用户态解析需按 121 字节重写**（§7 E2-2） |
| 4 | Vendor Report ID `0x06/0x07/0x08` 未在原实现中处理 | 低 | 其 README 明示"不修改"；我们需确认这些报告是否承载按键外信息 |
| 5 | 驱动崩坏可能导致**键盘整体失效**（内核态） | 高 | 参考实现有 `restore-normal-mode.bat` + 安全模式卸载路径；我们的实现必须有等价的失败保护（只做等长改写、不做分配、不阻塞） |
| 6 | 测试模式水印对用户是可见干扰 | 中 | 必须向用户披露；正式发布走 WHCP 签名可消除 |

---

## 7. 待执行实验（**未授权，全部未执行**）

### 7.0 E2-1 前置核查已完成（只读，2026-09-22 18:2x）

**结论：全部前置条件已满足，唯一阻塞 = 需要管理员权限 + 一次重启。**

| # | 检查项 | 实测值 | 判定 |
| --- | --- | --- | --- |
| 1 | Secure Boot（`UEFISecureBootEnabled`） | **`0`（已关）** | ✅ `prepare-test-mode.ps1:9` 的前置满足 |
| 2 | TESTSIGNING | **未开**（`SystemStartOptions = NOEXECUTE=OPTIN  NOVGA`，无 `TESTSIGNING`） | ✅ 干净起点，需 prepare 开启 |
| 3 | RC003 `kbdhid` 子节点 present | **是**（`OK \| Keyboard \| HID Keyboard Device`） | ✅ `install-driver` 的匹配对象在 |
| 4 | driver store 已有 MiRemoteHidFilter | **否** | ✅ 无残留 |
| 5 | 信任库已有测试证书 | **否** | ✅ 无残留 |
| 6 | 现成 package 完整性 | `.sys 11032 B` / `.inf 1330 B` / `.cat 2596 B` / `.cer 784 B` 齐全（sha256 见下表） | ✅ 可免构建直接用 |
| 7 | 构建工具链 | WDK 目录存在；VS 2022 BuildTools 存在；`msbuild` 不在 PATH | ⚠️ 用现成 package 则不需要 |
| 8 | 本机 OS | **Windows 10 专业版 BuildNumber 19041** | 与 INF 的 `NTamd64.10.0...18362` 兼容 |
| 9 | **当前会话管理员** | **否** | ⛔ **唯一阻塞** |
| 10 | RemoteMapper 驱动是否已装（防重复） | 未装 | ✅ |

**安装/卸载/恢复脚本已逐行审计**（§4.6 已列），确认三个安全守卫：

1. `prepare-test-mode.ps1:9` —— Secure Boot 开着就**直接抛错退出**，不会半途静默改动
2. `install-driver.ps1:19` —— 目标设备不在就直接抛错
3. `restore-normal-mode.ps1:13` —— **驱动还装着就拒绝关 TESTSIGNING**（防住"先关测试模式导致键盘栈崩"这一步）

**改动面清点（全部可逆）**：

| 动作 | 改了什么 | 反向操作 |
| --- | --- | --- |
| `prepare-test-mode.ps1` | ① 装测试证书到 `LocalMachine\Root` + `TrustedPublisher` ② `bcdedit /set testsigning on` | `restore-normal-mode.ps1`（按**证书指纹**精确删除） |
| `install-driver.ps1` | `pnputil /add-driver <inf> /install` | `uninstall-driver.ps1`（`/delete-driver /uninstall /force`） |
| `uninstall-driver.ps1:22` | **故意不动** testsigning 与证书，等恢复确认后才做 | `restore-normal-mode.ps1` |

**现成 package 二进制级复核（2026-09-22 18:5x，本人实测，`passed`）**：

| 文件 | 大小 | sha256（前 16） | 复核要点 |
| --- | --- | --- | --- |
| `MiRemoteHidFilter.sys` | 11,032 B | `bfd5b1f6e59ef429` | `MZ` + `machine=0x0003`（**x64**）、`subsystem=0x0001`（**Native** = 内核驱动，非 UMDF 用户态）；内含 `__KMDF_TYPE_INIT_START` 等 KMDF 运行时符号 → 确为 **KMDF 内核驱动**；调试路径 `F:\Playground\RemoteMapper\driver\MiRemoteHidFilter\x64\Release\MiRemoteHidFilter.pdb` |
| `MiRemoteHidFilter.inf` | 1,330 B | `d0ca4787f3b981d7` | `Class=Extension` / `ClassGuid={e2f84ce7-...}` / `ExtensionId={20c2709e-...}` / `PnpLockdown=1`；`Models.NTamd64.10.0...18362` 匹配串与本机 `kbdhid` 子节点 HardwareID **逐字一致**；`AddFilter=MiRemoteHidFilter,,MiRemoteLowerFilter` + `FilterPosition=Lower`；`KmdfLibraryVersion=1.15` |
| `miremotehidfilter.cat` | 2,596 B | `f8bbde43ba678a52` | 目录签名文件在 |
| `MiRemoteHidFilter.cer` | 784 B | `71d63fbeeb57ecd7` | WDK 测试证书在（即 `prepare-test-mode.ps1` 要装进信任库的那份） |

> 复核方式是"读二进制 + 逐字比对 INF 匹配串"，**不是**读 README 转述。结论：该包在 **x64 / Win10 19041** 上形态匹配、可直接用于 E2-1/E2-2（**`passed`**）。
> 唯一不能在此阶段断言的是"能否被本机 CI 策略接受加载"——那正是 E2-1 要验的东西（`deferred`）。

### 7.1 实验清单

| 编号 | 实验 | 前置 | 判据 |
| --- | --- | --- | --- |
| E2-1 | 在 RC003 `kbdhid` 节点挂一个**只读**设备专属 lower filter，验证它确实位于 `kbdhid` 之下、且能看到原始报告 | 管理员 + TESTSIGNING；WDK | 打印出 `01 00 00 <usage>` 形态的报告；`kbdhid` 仍正常收到 |
| E2-2 | 采集**全部** Report ID（0x01 + 0x06/0x07/0x08）的原始字节与长度分布 | E2-1 | 得到完整报告清单；确认 121 字节填充形态 |
| E2-3 | 实施一次**真实改写**（usage → F13–F24），验证**原生副作用消失** | E2-1 | TV 键不再触发 Shell 动作（无 `OpenWith.exe`、无协议选择器）；主页键不再被系统占用；返回/音量± 用户态可见 |
| E2-4 | 跨版本验证（Win10 19041 vs Win11） | E2-1 | 两版本行为一致 |

> **恢复要求（E2-1 ~ E2-3 共同）**：每个实验结束必须 `uninstall-driver` → 重启确认键盘栈恢复 → `restore-normal-mode`；实验期间的注册表改动须有备份与还原记录。
>
> **一个可直接复用的简化选项**：RemoteMapper 已提供**预编译的测试签名 package**（`package/MiRemoteHidFilter.sys` 11032 B + INF + cat + cer），因此 E2-1/E2-2 可直接用现成包安装验证，**无需先搭 WDK 构建环境**。自研实现才需要 WDK。

---

## 8. 与首轮选型文档的关系

| 首轮判断 | 本轮修正 |
| --- | --- |
| 推荐 ZSTDJan 式"HID 宿主内清空报告" | **`failed`**——RC003 不走 HID 通道，报告层拿不到（§2） |
| 次选 RemoteMapper 式下层 HID 驱动 | **升为推荐**——`passed`（源码 + 实机验收记录），且覆盖面比首轮估计更完整（含返回/音量±，§4.3） |
| "报告层清空覆盖含 TV 的全部按键" | **`failed`**（§2 独占机制） |
| "RC003 只有 1 个 HID 实例、不作为 HIDClass 设备存在" | **部分修正**——HID 树下确只 1 个实例，但**存在 HIDClass 父节点**（`BTHLEDEVICE` 树），这正是可挂 filter 的落点（§3.1） |

首轮文档 `docs/investigations/2026-09-22-per-device-key-interception-route-selection.md` 的需更新位置（**更新前不改代码**）：

| 首轮位置 | 内容 | 处置 |
| --- | --- | --- |
| §0 结论摘要 | 推荐路线 A | 需改为"已失效，改推路线 B" |
| §2.1 | ZSTDJan"HID 宿主内清空报告" | 标注 **`failed`**（真机证明设备不走 HID 通道） |
| §2.2 | RemoteMapper 下层 filter | 由"次选"升为**推荐**；补齐返回/音量± 覆盖 |
| §4 | 推荐路线 A 的机制与代价 | 整节标注失效 |
| §6 | 判别实验 E1–E6 | 改由本文 §7 的 E2-1 ~ E2-4 取代 |
| §9 | 两级能力方案（含 9.4 决策） | **仍然有效，不改**；其"无权限档维持现状"与本轮结论一致 |
| 附录 B/C | 证据索引与证伪路径 | 补入本文 §3 的设备树修正（父节点存在） |

---

## 附：证据索引

| 论述 | 证据 | 复核级别 | 状态 |
| --- | --- | --- | --- |
| RC003 存在 HIDClass 父节点（`BTHLEDEVICE` 树） | 父节点 `\8&14b2359b&b&0055` 的 `ClassGUID={745a17a0-...}`、`Service=mshidumdf`、`ParentIdPrefix=9&1748ac9e&0`；与子节点实例名吻合 | 本人实测（`e1-probe/driverstack.py`） | `passed` |
| 传输驱动是 UMDF `HidOverGatt.dll` 而非 `hidbth.sys` | 父节点 `Mfg=@hidbthle.inf`、`LowerFilters=WUDFRd`、`Service=mshidumdf`；`hidbth` 只服务 `BTHENUM\{00001124-...}` | 本人实测 | `passed` |
| HIDClass 类键 filter 槽位为空 | `Control\Class\{745a17a0}` 无 `UpperFilters`/`LowerFilters` | 本人实测 | `passed` |
| Keyboard 类键 `UpperFilters=kbdclass` | `Control\Class\{4d36e96b}` | 本人实测 | `passed` |
| 键盘 TLC 被系统独占，RIM 也独占打开 | 官方原文（见 §2 链接） | 官方文档 | `cited` |
| 官方不推荐 hidclass↔传输 minidriver 之间插 filter | 官方原文（见 §3.4 链接） | 官方文档 | `cited` |
| 设备专属 lower filter 可行且按设备绑定 | `driver.c:40-41` `WdfFdoInitSetFilter`；INF `Class=Extension` + `AddFilter`/`FilterPosition=Lower` | 第三方源码逐字 | `cited` |
| 报告形态 `01 00 00 <usage>`，121 字节，Report ID 0x01 | 其 README 第 33-53 行实机 preparsed metadata | 第三方实机记录 | `cited` |
| 七个痛点键的 usage 与改写映射 | `remap.c:6-27` | 第三方源码逐字 | `cited` |
| 八键实机验收 PASS（含 HVCI 开启） | 其 README 第 63-76 行 | 第三方实机记录 | `cited` |
| 部署需 TESTSIGNING + 关 Secure Boot，且可逆 | 其 README 第 117-156 行 | 第三方文档 | `cited` |
| 主程序无需管理员，驱动为可选增强 | 其 `DEPLOY.md:12,63` | 第三方文档 | `cited` |
| **filter 装的是 `kbdhid` 子节点（HardwareID 前缀 `HID\`），不是 `mshidumdf` 父节点（前缀 `BTHLEDevice\`）** | 两棵 Enum 树的 HardwareID 逐条比对：`HID\{00001812-...}_Dev_VID&012717_PID&32b8_REV&00a4` 仅出现在 `kbdhid` 子节点 | 本人实测（`e1-probe/e2-hwid.py`） | `passed` |
| E2-1 前置条件（Secure Boot 已关 / TESTSIGNING 未开 / 设备在 / 无残留 / package 齐全） | `e1-probe/e2-precheck.out`、`e2-precheck3.out` | 本人实测 | `passed` |
| **现成 package 是 x64 KMDF 内核驱动，INF 匹配串与本机子节点逐字一致** | `MiRemoteHidFilter.sys` PE 头 + KMDF 符号 + PDB 路径；INF `Models.NTamd64.10.0...18362` 比对 | 本人实测（读二进制） | `passed` |
| 安装/卸载/恢复三脚本的改动面与安全守卫 | 逐行读 `prepare-test-mode.ps1` / `install-driver.ps1` / `uninstall-driver.ps1` / `restore-normal-mode.ps1` | 本人逐行 | `passed` |
| **本路线在 RC003 + 本机 Win10 19041 上可挂载并改写** | — | — | **`deferred`**（§7 E2-1~E2-4；唯一阻塞 = 需管理员 + 一次重启） |
| **改写后原生副作用（TV Shell 动作）确实消失** | — | — | **`deferred`**（§7 E2-3） |
