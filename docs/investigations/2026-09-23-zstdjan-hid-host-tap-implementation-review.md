# ZSTDJan/windows-remote-mic-app 实现复核：HID 宿主内报告层捕获（2026-09-23）

> **目的**：按用户要求复核参考实现 `ZSTDJan/windows-remote-mic-app` 的按键捕获实现，判定
> 本仓库 2026-09-22 路线选型文档 §0 给出的"路线 A 已被真机 E1 判定 `failed`"是否成立。
> **方法**：浅克隆只读复核源码与项目文档（未安装、未构建、未运行其程序）；另在本机做
> 只读取证（注册表 + 进程快照），不注入、不提权、不修改任何系统状态。
> **参考版本**：`ZSTDJan/windows-remote-mic-app` 提交
> `1e6b1d285f9cd50f30c5bc92ac7787a693fc993d`（`main`，v1.0.44，2026-09-14）。
> **核验状态词汇**：`passed` = 实际执行并观察通过；`failed` = 实际执行不满足预期；
> `deferred` = 依赖当前不可得条件；`structural` = 静态取证或只读实测支持，未做端到端执行。

---

## 1. 结论摘要

**路线 A 没有被证伪。** 2026-09-22 的 E1 判定建立在一个**层错位**的测量上：E1 测的是
**Raw Input 的 HID 通道**，而路线 A 根本不经过 Raw Input——它在**承载该设备的
`WUDFHost.exe` 进程内部**、于 `NtDeviceIoControlFile` 的 UMDF 输出复制入口读取并清空报告。

本机只读实测（`structural`）已确认该路线所需的全部物理前提成立：

| 前提 | 本机结果 | 证据 |
| --- | --- | --- |
| RC003 的 HID 设备由**用户态**驱动宿主承载 | `Service = mshidumdf`、`LowerFilters = WUDFRd`、`WUDF\DriverList = HidOverGatt`、`DeviceDesc = Bluetooth Low Energy GATT compliant HID device` | 注册表 `Enum\BTHLEDevice\{00001812-…}_Dev_VID&012717_PID&32b8…\9&3aacf7b9&0&0055` |
| 可从注册表定位该宿主 PID | `Device Parameters\WUDFDiagnosticInfo\HostPid = 32684 (0x7fac)` | 同上 |
| 该 PID 确为 `WUDFHost.exe` 且**独占 RC003** | 快照命中 `WUDFHost.exe`；整棵 Enum 树中该 PID 只有 1 个成员且为 RC003 | `hardware/RC003/evidence/wudf-host-probe.log` |
| 普通权限无法打开该宿主 | `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` → `err=5` | 同上（说明注入必须由提权组件执行，与参考实现一致） |

**代价更新（关键）**：路线 A **不需要内核驱动、不需要 `TESTSIGNING`、不需要关闭 Secure Boot、
不需要重启、不需要 WHQL/attestation 签名**。它的代价是：一个**提权助手**（参考实现用固定
计划任务）+ 注入一个第三方注入框架（Frida Gadget，SHA256 固定）。

因此本仓库此前"第二轮推荐的 `kbdhid` 之下 KMDF lower filter 是**唯一**可行形态"的结论
不再成立；路线 A 与路线 B 都在方案空间内，且路线 A 的落地代价显著更低。

**2026-09-23 后续更新（只读实验已完成）**：按本文 §6 建议做的**只读最小注入实验**已执行并
判定 **`passed`**——本机 RC003 的报告里确实存在 `0x00F1`（返回）/ `0x0080`（音量+）/
`0x0081`（音量−），且 `0x80018483` / in 8 / out 9 / `02 01` 等判据逐条吻合。
报告：[`2026-09-23-rc003-hid-host-readonly-tap-result.md`](2026-09-23-rc003-hid-host-readonly-tap-result.md)。
路线 A 的**技术不确定性已基本消除**，剩余的是产品决策（ADR 0002 §3 与 R5）。

---

## 2. 参考实现的完整机制链

### 2.1 拓扑（其台账 `WINDOWS-ARCHITECTURE-LEDGER.md` §7 原文结构）

```text
RC003 遥控器
  |-- BLE GATT / ATVV --------------------------> RC003App（普通权限主程序）
  |                                                   |
  |-- Windows HID 键盘栈 --> Raw Input -------------+
  |                                                   |
  +-- HidOverGatt read --> WUDFHost + Gadget --------+
                                                      |-- 按键动作 --> SendInput / 启动应用
                                                      +-- ADPCM --> PCM --> 宿主语音输入
```

两条按键来源并行：Raw Input 负责**已经到达 Windows 的**物理边沿；HID tap 负责
**被 Windows 丢掉的那部分**。二者由应用侧的手势门闩合并成一次动作。

### 2.2 宿主定位（只读注册表，与我们的探针同构）

`frida_hid_tap_runtime.find_rc003_hidogatt_host_pid()` 的判据：

1. 枚举 `HKLM\SYSTEM\CurrentControlSet\Enum\BTHLEDevice` 下名字以
   `{00001812-0000-1000-8000-00805f9b34fb}` 开头、且含硬件 token
   `dev_vid&012717_pid&32b8_rev&00a4` 的键；
2. 在该服务节点下逐个实例读取
   `Device Parameters\WUDFDiagnosticInfo` 的 `HostPid`；
3. 配合 `ContainerID` 与用户所选设备摘要核对，避免多遥控器歧义。

`rc003_hidogatt_host_is_exclusive()` 进一步要求：在**整棵** `Enum` 树里，
该 HostPid 只对应 RC003 一个成员，才允许走"简单绑定"路径；否则判 `shared_host`，
需要额外的来源核验路径。

**本机实测结果：`exclusive_rc003_host`**（唯一成员），因此我们若实现，第一版可直接走
简单绑定路径，不必实现其复杂的来源核验状态机。

### 2.3 注入（需要提权，`SeDebugPrivilege`）

`frida_hid_tap_injector.py`：

- 手工声明 `LUID` / `TOKEN_PRIVILEGES`，用 `OpenProcessToken` +
  `AdjustTokenPrivileges` 启用 `SeDebugPrivilege`；`ERROR_NOT_ALL_ASSIGNED` 视为失败；
- 经典三件套注入：`VirtualAllocEx` → `WriteProcessMemory` → `CreateRemoteThread(LoadLibraryW)`
  在目标 `WUDFHost.exe` 内加载 `RemoteMicRC003HidTap.dll`（即 Frida Gadget 17.15.3，
  x64，`.xz` 解压后 SHA256 固定）；
- Gadget 走 config 加载内嵌脚本 `rc003_hid_gadget.js`，并向 `127.0.0.1:30684` 建
  loopback TCP 连接；服务端用 `GetExtendedTcpTable` 反查客户端 PID，**必须等于刚刚
  核验过的 WUDFHost PID**，否则在读取任何消息前关闭连接。

### 2.4 拦截（核心：在 UMDF 输出复制入口读取并清空报告）

内嵌 Gadget 脚本的关键判据（`frida_hid_tap_runtime.GADGET_SCRIPT`）：

```js
const UMDF_COPY_IOCTL = 0x80018483;
const EXPECTED_OUTPUT_LENGTH = 9;
```

挂两个点：

1. **来源归因**：挂 `WUDFHost.exe` 映像导入表里的 `DeviceIoControl` 槽（由 PE 导入表
   逐槽定位并核对到 `kernel32/kernelbase` 的同名导出，**不固定 KernelBase**，也不把
   Frida 推导的地址当槽内地址）。在 `onEnter` 里用调用方寄存器解析设备对象与控件句柄，
   按线程栈关联外层调用与内层复制。
2. **本体拦截**：挂 `ntdll!NtDeviceIoControlFile`。命中条件
   `IoControlCode == 0x80018483 && inputLength == 8 && outputLength == 9 &&
   input[4] == 2（operation）&& input[5] == 1（selector）`。

命中后执行清空（`interceptKeyboardReport`）：

```js
const raw = hex(pointer, 9);
if (!raw.startsWith("010000")) return null;   // 只处理 report_id=0x01、modifiers/reserved 为 0
pointer.add(3).writeByteArray([0, 0, 0, 0, 0, 0]);   // 清空 6 字节 usage 槽
return raw;                                    // 原始报文交回 Python 做手势与映射
```

报告布局与我们的独立取证一致：9 字节 = `report_id(0x01) + modifiers + reserved +
3 × uint16 usage`。这也解释了为什么三键能出现在同一个报告里——HID-over-GATT 用
**16 位 usage**，`0x00F1`/`0x0080`/`0x0081` 才有位置放。

**必须在 `onEnter` 清空**：其台账明确记录，内核在调用返回前已经复制了源缓冲区，
旧的 `onLeave` 清空太晚、挡不住原键。这是该方案最容易踩空的一处时序细节。

> **2026-09-23 本机独立证实**：只读 tap 实验在 30 次命中中观测到
> `onLeave` 时输出缓冲区发生变化 **0/30** —— 报告在 `onEnter` 就已完整就位，
> 内核在调用期间不改写它。参考实现的这条时序结论在本机成立。
> 见 [`2026-09-23-rc003-hid-host-readonly-tap-result.md`](2026-09-23-rc003-hid-host-readonly-tap-result.md) §1。

### 2.5 边沿配对与降级合同（工程细节，与我们的约束高度同类）

- **接管就绪门**：只有在观察到一次中性报告（`000000000000`）后才置
  `interceptionReady`，避免"某键的按下已被 Windows 收走、而它的抬起被我们拦下"造成粘键。
  这与本仓库"DOWN 漏进 OS 则 UP 必放行"的规则同构。
- **租约**：`MAX_INTERCEPT_LEASE_MS = 5000`，客户端 `0.5s` 续租、`2.0s` 租期、
  `0.5s` 停用确认、再多留 `0.15s` 安全余量；失联/异常时先取消活动手势并补齐已认领
  按键的松键，再撤销所有权。
- **fail-safe 边界（值得注意的严格取舍）**：只有 HID tap 确认接管原始报告时，才执行
  自定义单击/双击/长按；**tap 未接管时全部自定义按键映射停用**，只保留 Windows 原始
  行为——宁可不映射，也不接受"原生 + 映射"双执行。

### 2.6 提权组件的形态

- 主程序与桥接 worker 保持**普通权限**；提权能力由一个
  `RemoteMicRC003HidHelper.exe` 承载，经 UAC 显式安装为**无触发器的固定计划任务**，
  由普通权限的桥接 worker 通过 `schtasks /Run` 按需唤起；
- 助手**不接受调用方传入的 PID、路径或配置**，自行重新定位 RC003 的 WUDFHost、启用
  调试权限、复核进程名与 Gadget 哈希后才注入；任一项不符即明确失败，不回退为未经核验的注入；
- 旧世代 Gadget 的迁移通过只重启"独占 RC003 节点"完成（`hid_host_reload_windows.py`），
  **不原地卸载、不强杀宿主**。

### 2.7 无驱动、无系统安全设置改动

全仓检索确认：**无 `.inf` / `.sys` / `.cat` / `.vcxproj` / `.sln`**，无
`TESTSIGNING` / `BCDEdit` / Secure Boot 相关代码或文档。其台账原文：
"此方案沿用既有管理员助手与 Frida，**不安装新过滤驱动，不修改 Windows 蓝牙配置或宿主池化设置**"。

其自陈的风险边界：**该方案依赖已验证的 Windows 内部复制语义，不是微软承诺稳定的公开接口**；
且其 README 明确列出"游戏反作弊识别风险"。

---

## 3. 对本仓库 2026-09-22 路线选型判定的更正

### 3.1 原判定

`2026-09-22-per-device-key-interception-route-selection.md` 头部原文：

> 真机 E1 采集（…）：RC003 在 Raw Input 上 **100% 走键盘通道，HID 通道零报文**
> （`hid_report_seen=0`）；键盘 TLC 被系统 / RIM 独占，因此"报告层拿不到该设备的报告"，
> 路线 A 的前提不成立。

### 3.2 更正

E1 采集的原始记录（`hardware/RC003/evidence/e1-final-audit.out`）显示其数据来源是
本仓库 `raw_input_listener` 的 HID 采集行——即**我们自己的 Raw Input 监听**。逐条核对：

| 原判定分句 | 复核结论 |
| --- | --- |
| "Raw Input 上 100% 走键盘通道、HID 通道零报文" | **成立，但与路线 A 无关**。RC003 只有一个键盘页 TLC，Raw Input 自然只投递 `RIM_TYPEKEYBOARD`；路线 A 不读 Raw Input。 |
| "键盘 TLC 被系统 / RIM 独占，因此报告层拿不到该设备的报告" | **不成立（层混淆）**。RIM 独占的是**用户态 HID 接口的读权限**；报告的**生产端**在 `WUDFHost.exe`（session 0 用户态进程）内，位于 RIM 之前。 |
| "路线 A 的前提不成立" | **不成立**。前提（存在可读且可清空的报告层）已由本机注册表实测 + 参考实现已发布产品双向支持。 |

另需复核的第二轮论证："TV 的 Shell 动作来自该设备唯一的键盘页 report，与'清空即可消除'
互斥（清空发生在拿不到的那一层）"——该括注同样依赖上述层混淆。清空发生在 **UMDF 输出复制处**，
位于 Windows 键码翻译与 Shell 动作生成**之前**；参考实现正是对全部 `010000` 前缀报告统一
清空、再由应用侧按 usage 重新映射（其 CHANGELOG 记录方向键双移动问题即以此方式修复）。

### 3.3 仍然有效的部分

- 2026-09-23 的**免驱动、不提权**调研结论**仍然成立**：厂商 GATT 通知静默、GATT HID 特征
  `AccessDenied`、用户态直读 TLC `err=5`、Raw Input 含厂商页注册零事件——这四条是被正确测量的，
  它们证明的是"**不提权**的普通用户态程序拿不到三键"。
- `HidP_GetButtonCaps` 的**归因更正仍然成立**，且与本轮发现互补：设备确实把
  `0x80`/`0x81`/`0xF1` 声明在键盘页 `report_id=0x01` 中，`kbdhid` 的 HID→VK 映射表缺这三个
  usage 才导致不可见。**绕过映射需要读原始报告，而读原始报告需要提权注入宿主**——两条独立
  证据在这里收口。

---

## 4. 与本仓库既有约束的对照

| 约束 | 路线 A（HID 宿主 tap） | 说明 |
| --- | --- | --- |
| R1 按设备来源 | 满足 | 绑定到核验过的 RC003 宿主与设备对象，不依赖全局钩子 |
| R2 全局可吞 | 满足（源头） | 在报告层清空，Windows 侧根本不产生按键事件 |
| R3 纯替换语义 | 满足 | 清空全部报告后由应用侧重新映射，无双执行 |
| R4 物理键盘零影响 | 满足（结构上优于现行钩子） | 不挂全局 LL 钩子，不碰物理键盘链路 |
| R5 不引入不可逆或全局性的系统级改动 | **满足**（2026-09-23 收窄口径后） | 无内核驱动、不改 Secure Boot/测试签名/驱动签名策略、不写注册表过滤项、不重启、可卸载；提权助手 + 固定计划任务已按产品决策移出禁止范围 |
| R6 不降低可靠性 | 需验收 | 有租约/fail-safe/松键补齐；但依赖未公开的 UMDF 复制语义，需按异机、断连、睡眠分别验收 |
| ADR 0002 §3「不使用 Frida」 | **已解除**（2026-09-23） | 原条文废止，改为"允许注入框架（含 Frida Gadget），但须固定版本 + 校验 SHA256 + 登记来源与许可"；见 ADR 0002 修订记录 |
| ADR 0002 增强轨形态 | 满足 | 普通权限主程序 + 显式安装的提权 Helper，与 ADR 0002 §1 的增强轨描述一致 |
| 许可 | 兼容 | 本仓库与参考实现同为 GPL-3.0-only；Frida Gadget 为独立许可，须按 `ATTRIBUTION.md` 登记并固定哈希 |

---

## 5. 未验证项（deferred，不影响上述判定）

> **2026-09-23 更新**：下面第 1 项已由后续的**只读最小注入实验**完成，判定 `passed`
> （报告层确实存在目标三键）。实验报告：
> [`2026-09-23-rc003-hid-host-readonly-tap-result.md`](2026-09-23-rc003-hid-host-readonly-tap-result.md)。
> 第 3 项（异机与版本相关性）已补记本机全链版本，但异机复跑仍 `deferred`。

1. ~~**我们自己的注入与拦截未执行**~~ → **已执行（只读），`passed`**：2026-09-23 在据 2.2
   定位到的独占宿主内挂 `ntdll!NtDeviceIoControlFile`，命中 `0x80018483`（in 8 / out 9 /
   `02 01`）后转储缓冲区，**未清空**。结果为 `0x00F1`×3、`0x0080`×3、`0x0081`×3，阳性对照
   为 `0x0028`×4 / `0x004A`×2；`out_changed` 0/30，反证 2.4 所述"报告在 `onEnter` 已就位"。
   ~~**仍未执行的是写入路径（清零）**~~ → **2026-09-23 已实测通过**（见第 7 项）；
   **仍未执行的是边沿配对 / 租约 / fail-safe 的失效验收。**
2. **异机与版本相关性**：参考实现自陈"蓝牙版本号本身不能判定兼容，异机须核对
   Windows/驱动链和真实报告"。本机全链版本已补记
   （Windows 11 25H2 / Build 26200.7171，`mshidumdf 10.0.26100.1150`、
   `hidclass 10.0.26100.7019`、`kbdhid 10.0.26100.1882`、`WUDFRd/WUDFHost 10.0.26100.7019`；
   见实验结果文档附录 B），但异机复跑仍 `deferred`。
3. **共享宿主路径**：本机是 `exclusive_rc003_host`，其 `shared_host` 下的来源核验路径
   未涉及（我们第一版可能不需要）。
4. **杀软/EDR 与 HVCI**：本机注入成功（说明该机未被拦截），但未评估启用更严格策略的机器；
   参考实现自己列出游戏反作弊识别风险。
5. **RC001**：参考实现只针对 RC003；RC001 的 VK 0xFF 机制不同，不能由此外推。
6. ~~**两个需要产品拍板的问题**：ADR 0002 §3 是否重开；R5 的意图是否容纳"提权助手 + 计划任务"。~~
   → **2026-09-23 已拍板**：ADR 0002 §3 的「不使用 Frida」废止（改为可用但须固定版本 + 校验哈希 + 登记许可），
   R5 口径收窄为"不引入不可逆或全局性系统级改动"，显式安装且可逆卸载的提权 Helper 不在禁止范围。
   剩余阻塞项为 R6 类失效验收与 RC001 适配（写入路径已由第 7 项的实测验证）。
7. **写入路径已实测验证**（2026-09-23）：`onEnter` 的改写 / 擦除**均被 Windows 键码翻译采纳**，
   停止写入后恢复——**7 项断言全部 PASS，判定 `interception_effective`**
   （首轮 6 项中 5 项 PASS、`B2` 窗口内零命中属**采集缺失**，补采后 PASS）。
   `B2` 用 `0x0068`(F13) 封死了"Windows 恰好也认原 usage"的替代解释；且 `MakeCode` 显示
   扫描码亦由改写后的 usage 重新推导，⇒ 写入在翻译链最上游被消费。
   装置 `hardware/RC003/probes/wudf_ioctl_write.py`，结果见
   [`2026-09-23-rc003-hid-host-write-tap-result.md`](2026-09-23-rc003-hid-host-write-tap-result.md)。
   边沿配对 / 接管就绪门 / 租约 / fail-safe 的失效语义（2.5）仍 `deferred`。

---

## 6. 建议的下一步

1. **先更正文档口径**（本轮已做）：路线 A 未被证伪，两条路线并列在方案空间内。
2. ~~**用一个最小注入实验收口**~~ → **已完成**（2026-09-23，只读，一次提权，未重启、未改系统设置）：
   判定 `three_keys_present_in_report`，`structural` 已升级为 **`passed`**。
   报告：[`2026-09-23-rc003-hid-host-readonly-tap-result.md`](2026-09-23-rc003-hid-host-readonly-tap-result.md)。
   实验同时**独立证实**了 2.4 的时序结论（`out_changed` 0/30，报告在 `onEnter` 已就位），
   并按附录 C 记录了完整复现路径与探针坑（含 session 0 上 `post/recv` 只送达第一条的异常）。
3. 该实验通过后再决定：重开 ADR 0002 §3，还是以自研注入组件替代 Frida。
   **这是当前唯一的阻塞项，且是产品决策而非技术不确定性。**
4. 若选路线 A：下一步做**写入版**最小实验（`onEnter` 清零 + 应用侧重映射），
   再按 §5 的 7 个 `deferred` 项逐条收口；RC001/RC003 必须分别验收。

---

## 附录 A：证据索引

| 证据 | 位置 | 说明 |
| --- | --- | --- |
| 本机宿主探测原始输出 | `hardware/RC003/evidence/wudf-host-probe.log` | 宿主 PID、独占性判定、权限拒绝记录 |
| 宿主探测脚本 | `hardware/RC003/probes/wudf-host-probe.py` | 只读，支持 `--out` 落盘，自动脱敏 |
| E1 终审原始记录 | `hardware/RC003/evidence/e1-final-audit.out` | 层错位判定的来源（其 HID 行数为 0，数据源为 Raw Input 监听） |
| HID 声明 usage 取证 | `hardware/RC003/evidence/hid-direct-control.log` | `HidP_GetButtonCaps` 证明三键声明在 `report_id=0x01` |
| 免驱动四通道取证 | `hardware/RC003/evidence/raw-input-dump2.log` 等 | 不提权路径的零事件证据 |

## 附录 B：参考实现关键文件（逐条可核验）

| 文件 | 作用 |
| --- | --- |
| `apps/windows/rc003/src/ovb_rc003/frida_hid_tap_runtime.py` | 宿主定位、独占性判据、Gadget 准备、内嵌 `rc003_hid_gadget.js`（拦截与清空） |
| `apps/windows/rc003/src/ovb_rc003/frida_hid_tap_injector.py` | `SeDebugPrivilege` + `CreateRemoteThread(LoadLibraryW)` 注入 |
| `apps/windows/rc003/src/ovb_rc003/frida_compat.py` | tap 的 Python 侧客户端、usage→按钮表、租约与降级 |
| `apps/windows/rc003/src/ovb_rc003/hid_host_reload_windows.py` | 旧世代 Gadget 迁移（只重启独占节点，不强杀宿主） |
| `apps/windows/rc003/src/ovb_rc003/device_profile.py` | 按键 usage 表（含 `0x00F1` back / `0x0080` / `0x0081`） |
| `apps/windows/rc003/WINDOWS-ARCHITECTURE-LEDGER.md` | §6 程序角色、§7 运行拓扑、§8 按键链路、§12 资源所有权 |
| `apps/windows/rc003/CHANGELOG.md` | 方向键双移动修复及其验证记录 |

## 附录 C：版本固定

| 来源 | 固定点 |
| --- | --- |
| `ZSTDJan/windows-remote-mic-app` | 提交 `1e6b1d285f9cd50f30c5bc92ac7787a693fc993d`（v1.0.44，2026-09-14） |
| 该方案的技术上游 | `xxb26553663-star/remote-bridge-hub` 提交 `8a93f321ac71a602300c6cd77f7256fa4b63068e`（GPL-3.0-only） |
| 第三方组件 | Frida Gadget 17.15.3（`frida-gadget-17.15.3-windows-x86_64.dll.xz`，SHA256 `b566d701…`） |
