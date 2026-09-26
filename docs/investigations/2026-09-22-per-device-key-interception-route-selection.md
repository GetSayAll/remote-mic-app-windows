# 按键拦截路线选型研究：追求"按设备源头替换、零残留副作用"（2026-09-22）

> 状态：**研究结论已获产品策略变更授权，正在分阶段实现**。2026-09-23 已先取消返回/音量±在配置、持久化和映射引擎中的策略性剥离，并恢复 Vue 配置入口；这只解决“策略/配置层禁用”，不等于 RC003 已能产生三键事件。按设备源头捕获、完成态协议、Helper 握手与 RC001/RC003 真机 E1–E6 仍未完成，不能宣称端到端支持。
> 方法：只读取证。逐行阅读五个参考仓库的拦截实现与文档，另加本仓库既有调查档案对照。未安装任何驱动、未运行任何安装/构建脚本、未做真机测试。
> **可核验性**：所有引用的外部仓库均已固定到 commit SHA（见[附录 D](#附录-d版本固定核对时的-commit-sha)）；逐条证据索引见[附录 B](#附录-b证据索引逐条可核验)；供外部 AI 复审的核对清单与已知缺口见[附录 C](#附录-c供外部复审的核对清单)。
> **本文件内的决策记录**：无权限档的 TV / 主页取"维持现状"（2026-09-22 用户确认，见 9.4）；返回/音量±的产品禁用策略已于 2026-09-23 撤销，但两型号的源头捕获实现和验收仍按本文门槛推进。
> **核验状态词汇**：`passed` = 实际执行并观察通过；`failed` = 实际执行不满足预期；`deferred` = 依赖当前不可得条件。本文所有结论**均为静态代码/文档取证**，不构成真机验证；真机项一律标注 `deferred`。

> ⚠️ **勘误（2026-09-23 复核，取代此前的 `failed` 判定）：本节原称路线 A 已被真机 E1 判定为 `failed`，该判定不成立。**
> - **原因：层错位。** E1 采集的数据源是本仓库 `raw_input_listener` 的 HID 采集行（见 [`e1-final-audit.out`](../../hardware/RC003/evidence/e1-final-audit.out) 第 2 节的 `hid_*` 计数），测的是 **Raw Input 的 HID 通道**；而路线 A 不读 Raw Input——它在承载该设备的 `WUDFHost.exe` 内部、于 `NtDeviceIoControlFile` 的 UMDF 输出复制入口读取并清空报告，**位于 RIM 之前**。"Raw Input HID 通道零报文"是**预期现象**（RC003 只有一个键盘页 TLC，Raw Input 自然只投递 `RIM_TYPEKEYBOARD`），不能推出"报告层拿不到该设备的报告"。
> - **本机只读实测（`structural`）已确认路线 A 的物理前提成立**：RC003 的 HID 设备由 `mshidumdf` + `WUDFRd`（`WUDF\DriverList = HidOverGatt`）承载在 `WUDFHost.exe`，宿主 PID 可从 `Device Parameters\WUDFDiagnosticInfo\HostPid` 读取，且在本机为**独占 RC003** 宿主。证据：[`hardware/RC003/evidence/wudf-host-probe.log`](../../hardware/RC003/evidence/wudf-host-probe.log)。
> - **代价更正**：路线 A **不需要内核驱动、`TESTSIGNING`、关闭 Secure Boot、重启或 WHQL 签名**；代价是提权助手（参考实现用固定计划任务）+ 注入一个第三方注入框架（Frida Gadget，SHA256 固定）。
> - 完整机制复核、与本仓库 R1–R6 / ADR 0002 的逐条对照、以及未验证项见 [`2026-09-23-zstdjan-hid-host-tap-implementation-review.md`](2026-09-23-zstdjan-hid-host-tap-implementation-review.md)。~~**我们自己的注入与拦截尚未执行**（`deferred`，需一次提权，不需重启）~~ → **2026-09-23 已执行只读版并 `passed`**（见下方第三次更新）；**写入路径亦已实测并通过**
（7 项断言全 PASS、`interception_effective`，见下方第四次更新）。
> - TV 键通道归属专项、路线重选（第二轮）两份文档**尚未合入 `main`**，位于分支 `feat-rc003-e1-capture-and-route-research`：前者证明 TV 的 Shell 动作来自该设备**唯一**的键盘页 report（其"与『清空即可消除』互斥"的推论**同样依赖上述层混淆，需一并复核**——tap 的清空位于 Windows 键码翻译与 Shell 动作生成之前，会同时消除该 Shell 动作；参考实现正是对全部 `010000` 前缀报告统一清空后再由应用侧按 usage 重新映射）；后者曾改推 **RC003 设备专属 KMDF lower filter（挂 `kbdhid` 之下）**并称其为唯一形态——**该"唯一"结论同样不成立**（见下一条补充更正）。
> - 本文 **§9（两级能力）与 §9.4 决策仍然有效**，第二轮未作修改。
> - 本文 §6 的 E1–E6 已被上述第二轮 §7 的 E2-1 ~ E2-4 取代；其中 E2-1 唯一阻塞 = 需管理员权限 + 一次重启。

> ⚠️ **2026-09-23 补充（真机实测，本仓库已归档）**：**不提权**的普通用户态路径已被穷尽并判定 `failed` ——
> 厂商 GATT 通知静默、GATT HID 特征 AccessDenied、用户态直读 HID 顶层集合被 RIM 独占拒绝、Raw Input 含厂商页注册仍零事件；
> 同时用零权限 `HidP_GetButtonCaps` 证明**设备确实把 `0x80`/`0x81`/`0xF1` 声明在键盘页 `report_id=0x01` 里**，
> 是 `kbdhid` 的 HID→VK 映射表缺这三个 usage 才导致不可见。
> 完整证据、阳性对照与残余不确定项见 [`2026-09-23-rc003-driverless-capture-investigation.md`](2026-09-23-rc003-driverless-capture-investigation.md)。
>
> ⚠️ **同日第二次更正（取代上段原结论）**：上段原写"因此第二轮推荐的『按设备源头捕获（`kbdhid` 之下）』
> 重新成为**唯一**可行形态"——**该"唯一"不成立，且"免驱动路线已穷尽"的范围被误述**。
> 上述四条只证明"**不提权**的普通用户态程序拿不到三键"；**提权注入 HID 宿主**这条路径从未被测，
> 而它同样**不装内核驱动**。本轮对参考实现的复核 + 本机只读实测已确认该路径的物理前提成立
> （`mshidumdf`/`HidOverGatt` 承载于 `WUDFHost.exe`、宿主 PID 可定位、本机为独占 RC003 宿主）。
> 因此当前方案空间内**并列**两条可行路线：**路线 A（HID 宿主内报告层捕获，提权助手 + 注入框架，
> 无内核驱动、无需重启）** 与 **路线 B（`kbdhid` 之下 KMDF lower filter，需 `TESTSIGNING` + 关 Secure Boot + 重启）**。
> 详见 [`2026-09-23-zstdjan-hid-host-tap-implementation-review.md`](2026-09-23-zstdjan-hid-host-tap-implementation-review.md)。
>
> ✅ **同日第三次更新（只读实验已完成，`passed`）**：在据上文定位到的**独占 RC003** 宿主
> （`HostPid=32684`，`WUDFHost.exe`）内挂 `ntdll!NtDeviceIoControlFile`，命中
> `IOCTL 0x80018483`（in 8 / out 9 / operation-selector `02 01`）时**只读**转储缓冲区
> （**不清空、不注入按键、不改任何系统设置**，一次交互式 UAC 提权，未重启）。
> 结果：本机 RC003 的报告里**确实存在** `0x00F1`（返回）×3、`0x0080`（音量+）×3、
> `0x0081`（音量−）×3；阳性对照 `0x0028`（确定）×4 / `0x004A`（主页）×2 如期到达；
> 30 次命中中 `onLeave` 输出缓冲区变化 **0/30**，独立证实"报告在 `onEnter` 已就位"。
> **判定 `three_keys_present_in_report`（`passed`）。**
> 因此"三键在 Windows 侧不可见"的因果链闭合为：**设备上报 → 报告到达宿主层 → 键码映射
> （`kbdhid`）丢弃**；不再是推断，而是实测。
> 报告与复现步骤：[`2026-09-23-rc003-hid-host-readonly-tap-result.md`](2026-09-23-rc003-hid-host-readonly-tap-result.md)。
> **写入路径已实测验证（2026-09-23）**：擦除与改写**均被 Windows 键码翻译采纳**、停止写入后恢复
> ——**7 项断言全部 PASS，判定 `interception_effective`**（首轮 5 项 + 补采 `B2` 2 项）。
> 旁证：扫描码也是由改写后的 usage 重新推导（`0x0068` → `vk=0x7C make=0x64`），
> ⇒ 写入在翻译链**最上游**被消费。**仍未执行**：边沿配对 / 接管就绪门 / 租约 / fail-safe 的失效验收。
> ✅ **同日第四次更新（阻塞项已解除，写入版实验已就位）**：产品决策已下——
> ADR 0002 §3 的「不使用 Frida」已废止（改为"允许注入框架，但须固定版本 + 校验哈希 + 登记许可"），
> 本文件 §1 的 R5 口径同时收窄（显式安装、可逆卸载的提权 Helper 与固定计划任务不再被禁）。
> 写入版最小实验装置：`hardware/RC003/probes/wudf_ioctl_write.py`（+ `wudf_ioctl_write.js`、
> `raw_input_sink.py`、全程 / 补采提权启动器），**实测 7 项断言全部 PASS**，结果见
> [`2026-09-23-rc003-hid-host-write-tap-result.md`](2026-09-23-rc003-hid-host-write-tap-result.md)。
> 剩余阻塞项只剩 R6 类失效验收（边沿配对 / 租约 / fail-safe）与 RC001 适配。

---

## 0. 结论摘要

**条件性推荐路线：在 HID 宿主内"清空报告"（源头按设备替换），即 ZSTDJan `windows-remote-mic-app` 已在其 RC003 场景中实现过的形态。**
它是目前唯一有希望同时满足"按设备 + 不装内核驱动 + 不改系统安全设置 + 不重启 + 不产生合成键"的候选；但当前证据只覆盖 RC003 的已观察 `report ID 0x01`，TV 的独立 Shell 动作、其它 report、RC001 以及我们自己的完成态写回均未证明，因此不能再表述为“一条通道覆盖全部实体键”。本仓库 PR #66 仍是未合入 `main` 的开放 PR，且其当前 `hid_tap.js` 明确不读取 `STATUS_PENDING` 的完成缓冲区；这不是“现有管线只差从只读改成写回”的已交付增量。

**次选（长期/备选）：QL-4/RemoteMapper 的设备专属下层 HID 过滤驱动（MIT，142+71 行，已实机 8 键 PASS）。**
技术最干净（驱动在 `kbdhid` 之下、精确 HWID 绑定），许可也最适合参考移植；但它必须开 TESTSIGNING、关 Secure Boot、两次重启，正式发布要 Microsoft attestation/WHPC 签名——对个人项目是难以闭环的工程门槛。

**否掉：vibe-flow 驱动（未编译未验证、只覆盖键盘类层）、axonkey 的 Interception（闭源驱动无签名、商用需授权、已知"重连后输入整体失效且只能重启"未修）、Suk-ldev 的 Detours 只读钩（只读，不解决副作用）、继续现行 LL 钩子（已知不达标）。**

**必读的诚实边界**：严格意义的"零副作用"在任何路线下都不成立——每条路线都需要一个提权组件或内核组件。可达成的最优形态是：**设备级源头替换（遥控器键在进入 Windows 之前就被替换掉）+ 用户可一键停用 + 停用/失败后回到 Windows 原始行为**。对“拦截/替换”能力而言，后半段是 `fail-open`（恢复原生输入）；对“自定义动作”而言则应 `fail-closed`（不执行未知或过期动作）。本文第 4 节列出推荐路线自身的残余副作用，第 6 节列出审核后必须先跑的判别实验。

**已确定的两个前提**：① 推荐路线按"两级能力"落地——**有管理员权限的用户走新路，没有的用户维持现状**（第 9 节；无权限档的 TV / 主页维持现状已由用户于 2026-09-22 确认）；② 路线选型本身仍待审核，**本文不构成开发授权**。

**给复审者的提示**：本文所有外部结论都固定在 commit SHA 上（附录 D），逐条证据带 `文件:行号` 与复核级别（附录 B），已知缺口与可证伪路径列在附录 C。若发现与代码不符，请以代码为准并指出具体行号。

### 0.1 本轮代码复审结论（2026-09-22）

**结论：方案在原理上可行，但当前文档与本仓库状态不足以授权实现；应标记为“有条件可行 / implementation blocked”，先完成下列门槛。**

| 严重度 | 复审发现 | 为什么阻塞 | 修复方案与验收门槛 |
| --- | --- | --- | --- |
| P0 | 文档把 PR #66 当作 `main` 已有的注入管线。实际 PR #66 仍为开放 PR，未进入当前 `main`；其自身还记录了握手合包缺陷（`docs/architecture/rc003-enhanced-capture.md:362-384`）。 | 推荐路线没有可直接修改的基线，且已知 IPC 缺陷会在启用阶段中止会话。 | 先独立修复并合入 PR #66 的握手状态机（拆包、合包、握手后立即 `lease/stop`、EOF/取消/超时），再以合入后的精确 SHA 作为本路线基线；在此之前不得声称“增量最小”或开始写回实现。 |
| P0 | PR #66 的 `hid_tap.js` 对 `STATUS_PENDING` 明确“只计数、不读取、不延后持有”（其 README/实现同样如此）。推荐文档却把写回描述为在同一点加一条清空语句。 | `NtDeviceIoControlFile` 的输出缓冲区可能在返回前尚未完成；在 `onEnter` 写回可能被后续 I/O 覆盖，在 `onLeave` 盲写则可能修改未完成或非目标报告，造成丢键/蓝牙宿主不稳定。 | 先定义并实现“完成态拦截”协议：覆盖同步成功、`STATUS_PENDING`、失败、部分长度、句柄复用和宿主重启；只在确认 `IO_STATUS_BLOCK` 完成成功且来源/长度/report ID 全匹配后清空，并为每条路径建立离线测试和 Windows 真机证据。无法证明完成态安全时，否决用户态写回路线，退回驱动或维持现状。 |
| P1 | 推荐结论把“同一 report 可覆盖全部实体键”写得过满；当前证据只覆盖 RC003 `0x01`，TV Shell 动作和 vendor report `0x06/0x07/0x08` 仍是 `deferred`。 | 可能出现“键被清空但 Shell 动作仍发生”或清空范围过宽；R1/R2 尚未成立。 | 将推荐范围收窄为“RC003 `0x01` 的候选路径”；E1 先采集全部 report，E2 再按 report 验证 TV/主页/锁屏；在 E1/E2 `passed` 前，能力矩阵不得标记为覆盖全部实体键。 |
| P1 | 文档范围实际只讨论 RC003，却面向本仓库同时支持的 RC001/RC003 给出路线结论。 | RC001 的 report 描述、宿主归因和按键集合没有任何本路线证据，不能把 RC003 的静态研究外推到另一型号。 | 新增 RC001 独立勘测与验收（report/宿主归因、完成态、逐键 R1–R6、断连/睡眠/闲置首按）；两型号分别 `passed` 后才能宣称产品路线可行。 |
| P1 | `fail-closed` 在多处被用于“失败后恢复 Windows 原始行为”。 | 术语会把故障时序设计反向写错：拦截失效应放行原生边沿，而未知映射动作才应拒绝执行。 | 统一写成“拦截能力 `fail-open`、动作执行 `fail-closed`”，并把 E4 的通过条件改为“撤销清空、恢复原生行为、不执行迟到映射”。 |

因此，本次复审不是对推荐路线的无条件通过：**可行性结论为条件性通过，当前实现授权为不通过**。只有 P0 门槛和 RC001/RC003 的 E1–E6（及 RC001 对应补充项）完成并留下可复核证据后，才可进入代码实现 PR。

---

## 1. 目标拆解（把"解决所有副作用、纯自定义、无其他副作用"变成可判定条目）

| 编号 | 要求 | 判定方式 |
| --- | --- | --- |
| R1 | **全部实体键都可自定义**，含当前在 Windows 侧"看不见"的返回 / 音量± | 逐键绑定动作后真机生效 |
| R2 | **零原生残留**：自定义后按键不产生任何原生动作（字符、方向、媒体/音量、Shell 协议动作） | 逐键观察：无原生字符、无光标移动、无音量变化、锁屏场景无 `microsoft-edge:` 协议选择器 |
| R3 | **纯替换语义**：遥控器键只执行我配置的动作，没有第二份行为 | 单次响应、无"原生+映射"双执行 |
| R4 | **物理键盘与原系统行为零影响**：` 、Home、方向、Enter、音量、F5 全部照常 | 遥控器在线期间用物理键盘做高频输入回归 |
| R5 | **不引入不可逆或全局性的系统级改动**：不装内核驱动、不改 Secure Boot / 测试签名 / 驱动签名策略、不写注册表过滤项、不需要重启。**显式安装、可逆卸载、经用户一次性授权的提权 Helper（含固定计划任务）不属于被禁范围**（2026-09-23 按产品决策收窄口径，见 [ADR 0002 修订记录](../decisions/0002-dual-track-injection-optional-helper.md)） | 安装/卸载全流程审计；失败或拒绝时不留残余 |
| R6 | **不降低可靠性**：不新增"输入整体失效、必须重启"这类风险；不可用时回到原始行为 | 断连 / 睡眠 / 组件被杀 / 权限被拒 的场景回归 |

**现状对照**（本仓库既有证据）：现行 `WH_KEYBOARD_LL` 钩子 + 常驻抑制在 R2 与 R4 上结构性不达标——Home/TV 只能整键盘吞（`crates/sayall-windows/src/key_gate.rs:17-20`），物理键盘 ` 与 Home 被接管；TV 的 Shell 协议动作在键盘边沿全部吞下后仍出现（`Bugs/2026-09-10-win-l-mapping-and-capture.md` §8）。返回/音量± 则在 Windows 侧零事件（`docs/investigations/2026-09-05-rc003-back-volume-buttons-invisible.md`）。

---

## 2. 五个参考仓库的取证结果

### 2.1 ZSTDJan/windows-remote-mic-app —— HID 宿主内"复制并清空"报告（推荐路线）

- 许可 / 形态：GPL-3.0-only，Python + 官方 Frida Gadget，`1.0.44` 正式发布源码。
- **拦截层**：管理员 HID 助手把 Gadget 注入**已核验的 RC003 `WUDFHost.exe`**，脚本 hook `NtDeviceIoControlFile`，只处理 IOCTL `0x80018483`、9 字节、前缀 `010000` 的报告（`apps/windows/rc003/src/ovb_rc003/frida_hid_tap_runtime.py:485-499,511-527`）。
- **是否真吞**：**是，且在 Windows 翻译之前**。逐字：
  - `frida_hid_tap_runtime.py:50-51`：「It copies the selected RC003 report to the loopback client and **clears the usage payload** before Windows translates it into a second keyboard event.」
  - `:498`：`pointer.add(3).writeByteArray([0, 0, 0, 0, 0, 0]);` —— 把三个 usage 槽（byte 3..8）全部清零。
  - `README.md:655`：「协议 4 在 **Windows 复制报告之前保存原报告并清空 usage**，复制成功后才把原始边沿送入映射层；**不再依赖下游全局方向钩子抢先吞原键**。」
- **覆盖面**：报告层清空，所以**同一 report ID 内的所有键**都受影响；其文档明确 tap「上报遥控器的**全部键盘 usage**（返回 `0xF1`、音量 `0x80/0x81`、方向/OK/Home/Menu/TV/Power 等）」（`apps/windows/rc003/README.md:627`）。这只能证明同一 `report ID 0x01` 的覆盖能力，不能推出 vendor report 或独立 Shell 协议也会消失；Home / TV / 电源与返回/音量± 是否能在本项目中由**同一条通道**一起解决，仍由 E1/E2 判定。
- **按设备**：`hid_identity.py:64-88` 按 HID 接口路径的 VID/PID（`vid_2717` / `pid_32b8`）匹配，发现 0 个或多个就不接管；脚本侧只用 `boundCopyHandle` / `selectedSourceKey` 接管目标句柄与来源（`frida_hid_tap_runtime.py:511-527`）。
- **防粘键 / 防半截拦截**：接管前必须看到"中性报告"才生效（`:491-497`，注释原文「so its key-up can never be intercepted without the matching key-down」）；接管受租约约束（`:476-483`），无租约即不接管。
- **旧钩子被降级**：`WINDOWS-ARCHITECTURE-LEDGER.md:306-309` 记录旧实现曾用全局 F5 钩子、"不能再把桥接运行期间所有实体 F5 都被吞掉"，原键抑制以宿主内提前拦截为准；LL 钩子只保留用于语音边沿与"偶发泄漏的同一次原始方向键"（`apps/windows/rc003/README.md:582-583`）。
- **代价与残余风险**（`apps/windows/rc003/README.md:126-160,280-283,812-831`）：产物未签名（SmartScreen 提示）；首次启用自定义映射时一次 UAC，把 HID 助手写入 Program Files 并登记固定按需计划任务，此后日常启动/自启/桥接不再弹 UAC；**要求当前登录账号本身属于管理员组**（标准账号借他人凭据不受支持）；**UAC 被拒 → 拦截能力不启用，保留 Windows 原始按键**（拦截为 `fail-open`；参考代码实际还存在按键级降级，见第 10 节）；Frida 注入系统宿主与游戏反作弊共存未验证（`README.md:181`）。
- **未验证（关键）**：在其文档中**未找到** `microsoft-edge:` / `OpenWith` / 协议选择器相关记录。因此"TV 的独立 Shell 协议动作是否随清空一起消失"**无证据**，列为第 6 节 E1/E2 的判别实验；我的判断（推断）：若该动作由同一 report ID `0x01` 的 usage 引发，清空即可移除；若来自其它 report（RemoteMapper 记录该设备另有 vendor report `0x06/0x07/0x08`），则当前 `010000` 过滤会漏掉，需要把清空扩展到对应 report。

### 2.2 QL-4/RemoteMapper —— 设备专属下层 HID 过滤驱动，重映射为设备专属 F 键（次选）

- 许可 / 形态：MIT，C# 应用 + KMDF 过滤驱动（`driver.c` 142 行、`remap.c` 71 行）。
- **拦截层**：extension INF，`Class=Extension` / `FilterPosition=Lower`，精确绑定 `HID\{00001812-...}_Dev_VID&012717_PID&32b8_REV&00a4`；栈为 `kbdclass -> kbdhid -> MiRemoteHidFilter -> mshidumdf`（`driver/MiRemoteHidFilter/README.md`）。转发 `IRP_MJ_READ`，在下层完成后**原地等长改写 `report[3]`**，不动描述符、report ID 与长度；只碰键盘 report `0x01`，vendor report `0x06/0x07/0x08` 不改。
- **映射表**（`remap.c` + 驱动 README + `keymap.txt`）：`0x80→F13`、`0x81→F14`、`0xF1→F15`、`0x4A(Home)→F16`、`0x65(菜单)→F17`、**`0x35(TV/直播)→F18`**、`0x66(电源)→F19`、`0x3E(F5 语音)→F20`。
- **它对"物理键盘劫持"的解法逐字**（驱动 README）：「后四个本可映射为 Home / Apps / OEM_3 / Power，但**全局低级键盘钩子没有来源设备 ID**，直接映射会误吞物理键盘的同名键。因此把它们改为 F16–F19，**仅由此 VID/PID 的遥控器生成**。」——即：把"按设备"下沉到驱动，用户态钩子只需吞遥控器专属 F 键，物理键盘不再受影响（满足 R4 的机制）。
- **实测**：Windows 11 x64、HVCI 开启，8 键逐一 `PASS`（方向/音量±/返回/主页/菜单/直播/电源），有 `verify-keys.bat` 与 `tests/remap_test.c`。
- **代价**：必须 `TESTSIGNING` + **关闭 Secure Boot** + 两次重启；正式发布需 Microsoft Hardware Dev Center attestation/WHPC 签名；卸载要先卸载驱动再关测试模式（否则内核拒绝加载测试签名驱动），若启动异常需安全模式卸载（驱动 README「正式发布限制」「回滚与退出测试模式」）。
- **残留**：`NOTES.md` 记录 F16 与物理修饰键同时按住会触发 Windows 保留组合（其 `keymap.txt` 用 `TAP`：「源键抬起后原子点按目标组合，避免源 F 键与目标修饰键形成 Windows 保留组合」规避）——即"设备专属 F 键"路线仍有需逐项对冲的边缘行为。
- **与我们的关系**：本仓库 PR #66 的 `crates/sayall-windows/src/rc003_filter.rs` 正是这条路的**消费端**（认 `F13/F14/F15` + 精确设备路径），但驱动本体与签名材料不在该 PR 内。

### 2.3 richlearntodo-debug/vibe-flow —— 键盘类上层过滤驱动，按 MakeCode 真吞

- 许可 / 形态：GPL-3.0，C# + 驱动候选（`driver/rc003-filter/src/rc003_filter.c` 589 行）。
- **拦截层**：INF `Class=Keyboard`/`{4D36E96B-...}` + `HKR,,UpperFilters,...,"VibeFlowRc003Filter"`（MULTI_SZ 附加），精确 HWID 含 `REV&00A4`；劫持 `IOCTL_INTERNAL_KEYBOARD_CONNECT`，位于 **`kbdhid` 之上、`kbdclass` 之下**——拿到的已是扫描码，不是 HID 报告。
- **机制**：按 MakeCode 查 256 项抑制表，命中即 `continue;` 丢弃并修正 `InputDataConsumed` → 这是五个候选里**唯一"真吞键"**（不产生替换键）的实现。但代价是：只看 MakeCode、忽略 `KEY_E0` 标志（`E0 47`=Home 与 `47`=小键盘 7 无法区分）；**无 usage 概念、无报告描述符解析、不覆盖消费页/非键盘 usage**；双同型号设备共用全局单例策略；2 秒心跳 fail-open。
- **状态**：其 README 自述**尚未通过编译、SDV、Driver Verifier、HLK、Secure Boot 与真机安装**；CI 只产出未签名候选，不能产出可分发产物。
- **判断**：方向与我们一致（按设备真吞），但覆盖面（只有键盘类层）不足以解决 R2 中的非键盘路径，且成熟度最低，另需自建签名链。

### 2.4 leowzz/axonkey —— Interception 内核过滤（已产品化，但有硬缺陷）

- 许可：仓库无 LICENSE 文件；`vendor/interception/SOURCE.md:21,29-30` 明确其捆绑的 LGPL-3.0 文本为 **non-commercial**，商用分发需另行授权。
- **产品实际用哪条通道**（事实）：10 个普通键走 **Interception**（`set_device_filter` 只对命中 `VID_2717/PID_32B8` 的槽位给 `FILTER_KEY_ALL`，命中映射后**不回注**=按设备 drop）；返回/音量± 走 Frida Gadget **只读**（`rc003_hid_gadget.js` 只 hook 读取 IOCTL `0x80018483`）。
- **按设备能力**：`interception_get_hardware_id` + 谓词过滤（`src-tauri/src/input_service/windows.rs`），**已产品化验证**——这是它最大的价值：证明了"按设备识别 + 按设备丢弃"在 Windows 上可落地。
- **硬缺陷**（`docs/INTERCEPTION_HOTPLUG_INCIDENT.md`，上游 issue #193，**未修复**）：RC003 断连/休眠/重连后，反复枚举会把设备编号推高到驱动固定范围（`KbdClass0–KbdClass9`）之外，此后**所有按键无响应，且用户态无法修复，只能重启电脑**。
- **安装代价**：`scripts/install-driver.ps1` 要求管理员、调用 `install-interception.exe /install`、**需重启一次**；其自身提示原文包含「Interception v1.0.1 is not signed with an Authenticode publisher signature」「commercial distribution requires a commercial Interception license」。
- **判断**：机制上最接近 R1–R4，但直接违反 R6（输入可能整体失效、必须重启）与许可条件。

### 2.5 Suk-ldev/remote-mic-app-windows —— 本仓库 fork，Detours 只读钩（工程路径参考）

- 许可：GPL-3.0（本仓库 fork）。
- **做了什么**（事实）：`native/rc003-hook/inject.cpp` 把 DLL 注入**系统 `WUDFHost.exe`**（校验宿主映像名与 `microsoft.bluetooth.profiles.hidovergatt.dll`/`mshidumdf.dll`），`hook.cpp:153-154` 用 Detours 内联 hook `ntdll!NtDeviceIoControlFile`。
- **只读**（逐字）：`hook.cpp:47-48`「**Read-only observation**: the real call runs first and its result/last-error are returned unchanged.」；`hook_protocol.h:11-12`「The hook is **read-only**: it never modifies, blocks, or delays the real I/O.」只解 `0x00F1/0x0080/0x0081`。
- **价值**：证明"不装驱动、不签名也能注入宿主并取到按键"这条工程路径可行（其版本 0.2.9 起因内嵌 `requireAdministrator`）；**但不解决我们的目标**——只读意味着没有任何副作用消除能力，其文档自认 Home/TV 原生残留未解，并把"按设备全吞"归给尚未启动的 KMDF Helper 轨。`Testing/probe-driver*.ps1`、`probe-hookmon.ps1` 是 2026-09-04 的**语音注入**探针，与本路线无关。
- 另注：其 `docs/investigations/2026-09-04-avoid-driver-signing-input-paths{,-final}.md` 与本仓库上游**逐字节相同**（无新增结论）。

---

## 3. 能力矩阵

| 候选 | 按设备拦截 | 真消除原生动作 | 覆盖返回/音量± | 覆盖 TV Shell 协议动作 | 需内核驱动 | 需改系统安全设置 / 重启 | 需常驻管理员 | 产生合成键 | 实测状态 | 许可可用性 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **A. ZSTDJan 报告层清空** | ✓ | ✓（翻译前清空，待完成态复核） | ✓（仅已观察的 `report 0x01`） | **未验证** | ✗ | ✗ | ✓（一次 UAC 后日常无感） | ✗ | 正式版 1.0.44 | GPL-3.0（同族） |
| **B. RemoteMapper 下层 HID 驱动** | ✓（精确 HWID） | ✓（替换为 F13–F20，需钩子吞 F 键） | ✓ | **未验证**（只碰键盘 report） | ✓ | ✓✓（TESTSIGNING + 关 Secure Boot + 两次重启） | ✗（驱动常驻） | ✓（F13–F20） | 真机 8 键 PASS | MIT（最适合移植） |
| C. vibe-flow 键盘类驱动 | ✓（精确 HWID） | ✓（真丢弃） | 部分（RC003 三键在键盘页，理论可见） | ✗（拿不到报告层） | ✓ | ✓✓（同 B） | ✗ | ✗ | 未编译、未实机 | GPL-3.0 |
| D. axonkey + Interception | ✓（已产品化） | ✓（drop 不回注） | ✗（Frida 只读） | ✗（键盘类层） | ✓（第三方闭源） | ✓（装驱动 + 重启，无需测试签名） | ✗ | ✗ | 已发布 | ✗ 商用需授权 |
| E. Suk-ldev Detours 钩 | 源级校验 | ✗（只读） | ✓（仅取回） | ✗ | ✗ | ✗ | ✓ | ✗ | 探针 passed | GPL-3.0 |
| F. 现行 LL 钩子（现状） | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | 已验收 | — |

---

## 4. 推荐路线（A）的机制、为什么它能一次解决全部问题，以及它自身的副作用

### 4.1 机制

```
RC003 --BLE HID--> WUDFHost(已核验为 RC003 独占)
                      |  hook NtDeviceIoControlFile（管理员助手注入的 Gadget）
                      |  report[0]==0x01 且长度 9：
                      |    1) 读出原始 usage（返回给本机回环 → 映射引擎）
                      |    2) 把 byte 3..8 清零（三个 usage 槽全部）
                      v
                 Windows 键盘栈看到的是"这个设备什么都没按"
                      v
                 我们的映射引擎按用户配置注入动作
```

### 4.2 它为什么能同时满足 R1–R4

1. **R2/R3 只有在完成态和 report 范围被证明后才成立**：若替换确实发生在 Windows 翻译之前，且 TV 动作由同一 report 引发，遥控器键可不产生原生事件；当前“没有 `` 字符、没有 Home 光标移动、没有音量变化、没有 TV Shell 动作”仍是 E1/E2 的待证命题。
2. **R4 从根上成立**：只对已核验的 RC003 宿主/句柄生效，物理键盘与该设备毫无交集。
3. **R1 顺手解决**：返回/音量± 本来在 Windows 侧零事件（`kbdhid` 丢弃未知 usage），而这条通道**直接从报告里读**，三键与其余键一样可见——不需要 PR #66 那种"两条通道各管一半"的设计。
4. **简化既有复杂度**：报告的源头替换到位后，`key_gate` 里"武装死锁 / 常驻抑制 / 泄漏对冲"三套机制的存在意义消失（它们的全部理由是"看不到设备身份，只能事后猜"），可以整体退场——**物理键盘 Home/` 被劫持这个长期代价随之消失**。
5. **增量目前不能判定为最小**：PR #66 仍未合入 `main`，且其 `hid_tap.js` 将 pending I/O 视为不可读并直接放弃；改造成“写回”至少需要完成态状态机、租约失效时的原子撤销、失败恢复和同步/异步两套测试，不能按“加一条清空路径”估算。

### 4.3 它自身引入的副作用（必须先被知情）

| 项 | 内容 | 可缓解性 |
| --- | --- | --- |
| 常驻管理员助手 | 安装时一次 UAC，写 Program Files + 登记按需计划任务；要求登录账号属管理员组 | 用户可拒绝；拒绝后拦截能力不启用并恢复原生按键（拦截 `fail-open`，基础语音不受影响） |
| 注入系统宿主 | Frida 在系统 `WUDFHost` 内执行代码，缺陷可能影响蓝牙 | 需租约 + 心跳 + 一键停用；DLL 可能驻留至宿主回收（PR66 文档已承认的边界） |
| 未签名 | SmartScreen / 杀软误报 | 长期需 Authenticode；短期在文档与安装说明中明示 |
| 反作弊共存 | 注入路径与竞技类游戏反作弊共存**未验证** | 使用前明示；提供一键停用；不通过关闭反作弊来使用 |
| 全键变为纯注入 | 清空后所有映射键都靠 `SendInput` 注入，**输入法对注入键的过滤问题会从语音键扩散到全部映射键**（本仓库 `Bugs/2026-09-04` 已证实 WeType/豆包会过滤注入和弦） | 需要 E5 回归；必要时保留"某些键走原生"的例外，或对输入法场景做单独处理 |
| 长按/连发语义变化 | 原生"按住重复"消失，连发完全由映射引擎负责 | 现引擎已支持；需逐键回归 |

---

### 4.4 "常驻管理员助手"到底对普通用户有多大影响

先纠正一个前提：**最好的实现里没有"常驻管理员进程"。** 需要把三种"持续性"分开看，它们的用户可见程度差别很大：

| 持续性 | 实际形态（ZSTDJan 正式版做法） | 证据 |
| --- | --- | --- |
| 安装足迹 | Program Files 里的助手 exe + 一个**无触发器的固定计划任务** | `apps/windows/rc003/README.md:126-127`；`WINDOWS-ARCHITECTURE-LEDGER.md:141` |
| 提权进程 | **按需启动、用完即退**：计划任务被显式调用 → 助手注入 → 释放句柄；结果写一次 `hid-helper.log` 后立即关闭 | LEDGER:142「短暂持有已独立定位并核验的 RC003 WUDFHost 句柄」；LEDGER:498,713 |
| 宿主内模块 | 注入的 Gadget DLL **驻留在 `WUDFHost` 里**，直到 Windows 回收宿主；组件升级时可能提示"请重启电脑" | `README.md:163-165`「仅重启无线麦服务不能清除旧版驻留组件」；`:391-392` |

"计划任务没有真实触发器，只能运行"（LEDGER:147-149）——即它**不会自动运行、不是常驻服务、不出现在"服务"列表**。

**逐场景的用户体验**（均以 ZSTDJan 正式版文档为证据）：

| 场景 | 普通用户看到什么 |
| --- | --- |
| 安装/首次启用 | **一次 UAC**（"把 HID 助手写入 Program Files 并登记计划任务"）；拒绝时程序仍可安装，但拦截能力不启用、保留 Windows 原始按键，界面显示"管理员按键组件异常 / 修复权限"（`README.md:126-127,157-159`；LEDGER:756-759；实际按键级降级见第 10 节） |
| 日常使用 | **0 次 UAC**（`README.md:127`「确认后，日常启动、桥接和随 Windows 启动都不再弹 UAC」）；没有常驻托盘进程、没有服务、不重复输密码 |
| 账号类型 | **必须"当前登录账号本身属于管理员组"**；标准账号即使用 UAC 输入另一个管理员账号的密码也不受支持（`README.md:133-134,159-160,830-831`）→ **在公司受管电脑的标准用户账号下，这个功能完全不可用** |
| 升级/修复/卸载 | 每项各需**再确认一次 UAC**；卸载会先自动停止进程、删任务与助手文件，**取消 UAC 则卸载中止**（不留指向失效文件的任务）；**设置与日志默认保留**，需手动删 `%LOCALAPPDATA%`（`README.md:358-369,504-513,826`；LEDGER:768-769） |
| 换账号/多用户 | 助手与计划任务归属**安装时的那个管理员账号**；"移除权限"会移除该账号下**所有无线麦版本共用**的助手（`README.md:504-507`） |
| 安全软件 | 产物**未签名** → 首次运行 SmartScreen 需手动"更多信息→仍要运行"；**杀软/安全软件兼容性在其文档中列为未验证**（`README.md:36,49,62,824`；LEDGER:782,795-796） |
| 前台是提权窗口 | 主程序不提权，映射动作靠 `SendInput` 注入；Windows 标准行为（UIPI）会丢弃低完整性进程发往高完整性窗口的注入输入 → **任务管理器、管理员 cmd、部分安装程序/游戏里映射键可能不生效**（本仓库既有调查档案 `2026-09-06-left-double-response-arm-deadlock.md` 方案 B 已记录同一结论） |
| 游戏 | 与反作弊共存**未验证**（`README.md:181`），注入路径可能被拒 |
| 源码调试 | 需从管理员终端启动（`README.md:715`）——只影响开发者，不影响用户 |

**最可能绊倒普通用户的三件事**，按概率排序：
1. **账号不是管理员** → 功能整体不可用（不是"体验差"，是"没有"）。
2. **杀软/企业策略拦截注入或未签名组件** → 表现为"功能突然不工作"（拦截 `fail-open`，遥控器退回原始按键；未知映射动作不得执行）。
3. **升级助手/协议后需要重启电脑**（`README.md:163-165`）——低频但真实。

**与内核驱动路线对比（同样是"普通用户影响"）**：驱动路线要求 **关闭 Secure Boot**（需先进 BIOS/UEFI，且启用 BitLocker 时可能要先暂停保护）、**常开测试签名**——Windows 会在**桌面右下角持续显示"测试模式"水印**，并在 HVCI 下要求使用自建测试证书签名、安装与卸载各需重启。这些都是"每天都看得见"的代价（Microsoft 官方文档：〈加载测试签署的程序代码〉/〈WHQL 测试签名计划〉）。两相比较，**管理员助手路线对用户的可见干扰明显更小**。

**结论（供拍板）**：对"自己的电脑 + 登录账号属于管理员组"的用户，这条路线在授权之后的日常影响近乎为零（0 次 UAC、无常驻进程、无系统设置改动、可一键停用并恢复原始行为）。对"公司受管电脑 / 标准账号"的用户，它**完全不可用**——所以真正需要先确认的是：**目标用户是不是都能在自己机器上自助提权（UAC 点"是"）**。这也是本节唯一无法靠代码研究回答、只能由产品定位决定的问题。

## 5. 为什么排除其他路线（简述）

- **B（驱动）**：技术最干净、许可最友好，但要求关闭 Secure Boot + 常开 TESTSIGNING + 多次重启，正式发布需 WHPC 签名——这本身违反 R5，且对个人项目难以闭环。**保留为长期/备选**：如果 A 的反作弊或稳定性风险被判定不可接受，B 是唯一还剩下的正经方案；其 `remap.c` 表、TAP 组合键规避经验与 HWID 精确绑定写法可直接参考（MIT）。
- **C**：方向对（真吞）但位置太低（只有扫描码，看不到报告层），无法覆盖非键盘路径，且代码尚未编译/未实机，另需自建签名链——是 B 的严格劣化版。
- **D**：机制已产品化，但"重连后输入整体失效、只能重启"是未修复的上游缺陷，直接违反 R6；加上闭源无签名驱动与商用授权成本。
- **E**：只读，不解决 R2。
- **F（现状）**：R2/R4 结构性不达标，已在真机与文档中记录。

---

## 6. 审核后必须先生跑的判别实验（未授权前不执行）

| 编号 | 目的 | 方法 | 通过判据 |
| --- | --- | --- | --- |
| E1 | 定位 TV 的 Shell 协议动作来自哪个 report | 在诊断用 tap 里记录**该设备全部 report ID 与原始字节**（不止 `010000`），逐键采集（TV / 主页 / 返回 / 音量±） | 得到 TV 按压对应的完整 report 清单；确认是否只有 `0x01` |
| E2 | 清空是否真的消除全部原生动作 | 在 E1 结论基础上，按 report 收窄清空范围后逐键验证，**必须包含锁屏场景**（本机 Edge 已安装/未安装两种状态） | 无原生字符、无光标/音量变化、锁屏不出现协议选择器 |
| E3 | 物理键盘零影响 | 遥控器在线时用物理键盘高频输入 `` ` ~ / Home / 方向 / Enter / 音量`` | 全部键照常，无吞键、无延迟异常 |
| E4 | 失效与恢复 | 助手被杀 / 记录租约超时 / BLE 断连 / 睡眠唤醒 / UAC 拒绝 / 卸载 | 拦截能力 `fail-open`、动作执行 `fail-closed`：立即撤钩、遥控器键回到 Windows 原始行为、**不残留清空**、不执行迟到映射、不需重启 |
| E5 | 全键纯注入可用性 | 逐键在记事本、浏览器、微信输入法、管理员窗口（UIPI）下验证映射动作 | 逐项 `passed` / `failed`，失败项单独列出对策 |
| E6 | 共存与安全软件 | 反作弊/杀软共存、注入被拦时的表现 | 至少记录一次实测；无法验证的（具体游戏）保持 `deferred` 并写进用户文档 |

**判定口径**：`passed` = 实际执行并观察通过；`failed` = 实际执行不满足预期；`deferred` = 依赖当前不可得的条件。**编译通过、单元测试通过、模拟宿主机通过，都不算这三项。**

**E1–E6 的当前状态（2026-09-23 实时）**：逐项状态表在 [`Testing/WindowsRC003EnhancedCapture.md`](../../Testing/WindowsRC003EnhancedCapture.md)。
要点：三键边沿送达与「反复运行 / 接管」已 `passed`；E1（TV 的 report 归属）、E2 的锁屏场景、E4d、E5、E6 为 `deferred` 并已写明依赖；E2 主判据、E3、E4a/b/c 为「未执行，可直接做」。

> ⚠️ **E2 在 RC003 上必须用哨兵键才可判。** 三键在 Windows 侧本来就零事件（`kbdhid` 丢弃
> `0xF1`/`0x80`/`0x81`），所以「清掉」与「不清」外部完全看不出差别——判据「没有原生动作」
> **恒为真、没有分辨力**，`observe` 与 `run` 在现象上无法分辨。助手因此新增
> `--canary-usage <U16>` 与入口 `run-helper-canary.cmd`（默认额外清主页 `0x4A`，Windows
> 本来能处理的键），使「清空生效」与「fail-open」变成肉眼可见的三步对照。
> 详见 [`../../hardware/RC003/README.md`](../../hardware/RC003/README.md) §5.7。

---

## 7. 审核通过后需要改动的文件（**现在不动**）

- `TODO.md`：新增该路线条目，并调整"单独评估返回键、音量键等完整 HID 实验能力"与"按 ADR 0002 立项可选 Helper"两条的定位。
- `docs/decisions/`：新增 ADR（替代/修订 ADR 0002 中"虚拟键盘驱动 + 按设备吞键"的 Helper 轨定义）。
- `ATTRIBUTION.md`：记录 ZSTDJan `windows-remote-mic-app`（协议与"复制并清空"做法）、内容边界；若参考 RemoteMapper 的 remap 表或 TAP 语义，一并记录（MIT）。
- `docs/architecture/rc003-enhanced-capture.md`：把既有的只读实验方案与本次路线合并或明确分工。
- `Testing/`：新增验收手册（E1–E6 的命令、判据、日志字段）。**✅ 2026-09-23 已落盘**：[`Testing/WindowsRC003EnhancedCapture.md`](../../Testing/WindowsRC003EnhancedCapture.md)（含退出码表、日志字段速查、常见失败处置与逐项状态表）。
- 两级能力（第 9 节）落地时另需：`src/lib/bridge.ts` 的 `ShortcutCapability` 输入增加"能力档位"、`src/pages/ButtonsPage.vue` 的 `capabilityNote` 逐格说明按档位变化、`src-tauri/src/platform.rs` 暴露档位状态；以及两档切换的回归用例。

**📌 2026-09-23 状态回填**（本节写于 2026-09-22 的"现在不动"，逐项对照）：

| 项 | 状态 | 说明 |
| --- | --- | --- |
| `TODO.md` 条目 | ✅ 已做 | 见 TODO 的"为返回键、音量加、音量减完成按设备源头捕获"条目及其四次更新 |
| `docs/decisions/` ADR | ✅ 已做 | ADR 0002 §3 已修订（「不使用 Frida」废止 → 改为"固定版本 + 校验 SHA256 + 登记来源与许可"），R5 口径同步收窄 |
| `ATTRIBUTION.md` | ✅ 已做 | 已记录 ZSTDJan / `remote-bridge-hub`，并新增"产品注入载体登记：Frida Gadget 17.18.0"一节 |
| `docs/architecture/rc003-enhanced-capture.md` | ❌ **未做** | 只读/写入实验已分别成文于 `docs/investigations/2026-09-23-rc003-hid-host-*.md`；架构文档尚未合并 |
| `Testing/` 验收手册（E1–E6） | ❌ **未做** | 阻塞于真机验收；spike 的 `--help` 已给出运行入口，但 E1–E6 的命令/判据/日志字段手册未写 |
| 两级能力前端/后端改动 | ❌ **未做** | `bridge.ts` / `ButtonsPage.vue` / `platform.rs` 的档位化未动 |

新增（本节未预见）：`hardware/RC003/helper/` 产品化 spike —— 把机制造成可运行组件（提权助手 + Gadget agent + 自检台），**未并入产品工作区**，提交 `76e3855`。

**📌 2026-09-23 二次状态回填（真机首跑后的修正）**：

| 项 | 状态 | 说明 |
| --- | --- | --- |
| 宿主定位链真机成立 | ✅ 已验（**免提权**） | `helper_dryrun.out`：`diag_keys=6` / `pid=32684 WUDFHost.exe` / `exclusive_rc003_host` / Gadget 校验通过；与只读探针**两个独立实现给出同一 PID** |
| 真机注入（`observe` / 正式） | 🟡 **`observe` 已验；`run` 未做** | `observe`：30 条 `[EDGE]`，三键 usage 与预测一致，`clears_ok=0`。**但 `observe` 一个字节都不清 ⇒ 证明不了"拦截生效"，也证明不了"确定/主页/方向键无回归"**。`run` 模式仍需 Andy 提权运行 |
| `Testing/` 验收手册（E1–E6） | ❌ **未做** | **不阻塞于真机**——命令、判据、日志字段可以也应该在跑之前先写定，否则现场没有可对照的验收标准 |

> **首跑教训**（详见 [`2026-09-23-rc003-helper-hostpid-read-bug.md`](2026-09-23-rc003-helper-hostpid-read-bug.md)）：
> 助手把 `HostPid`（本机为 `REG_QWORD` / 8 字节）按 4 字节读，全部返回 `ERROR_MORE_DATA`
> 又被静默丢弃，于是**误报"设备未连接"**。两点可迁移的结论：
> ① **探针能跑通 ≠ 我的原语正确**——Python 的 `winreg` 按真实类型自动转换，从不暴露"宽度"这个契约；
> ② **错误信息不得断言未被检验的原因**——"设备未连接"是个结论，而当时只观测到"计数为 0"。

**📌 2026-09-23 三次状态回填（首次真机 `observe` 之后）**：

| 项 | 状态 | 说明 |
| --- | --- | --- |
| 三键边沿送达我们的代码 | ✅ **已验（真机、提权）** | 30 条 `[EDGE]`（返回 `0x00F1` / 音量+ `0x0080` / 音量− `0x0081` 各 5 次 + 释放），`target_hits=15` 与按下数 1:1、`edges_sent=30`；`evidence/2026-09-23-observe-run.log` |
| `--duration` 上限 | ✅ 已修 + 自检回归 | 原先**有 agent 连接时永不到期**（`BufReader::lines()` 阻塞，收尾检查点到不了；实测 300 s → 518 s 仍在跑）。改为读超时轮询 + 把绝对到期时刻传进会话循环 |
| Ctrl+C / 关窗发 `disarm` | ✅ 已修 + 自检回归 | 原先**从未安装 `SetConsoleCtrlHandler`**，硬终止，收尾代码永远到不了 |
| `run` 模式的拦截生效 / 无回归 / fail-open | ❌ **未做** | 需 Andy 本人提权运行 `run-helper.cmd` |

> **二次真机教训**（详见 [`2026-09-23-rc003-observe-first-real-run.md`](2026-09-23-rc003-observe-first-real-run.md)）：
> **功能路径通过 ≠ 收尾路径通过**。`--duration` 与 Ctrl+C 两条"不把钩子留在系统上"的保障，
> 自检与静态检查都抓不到，只有真机跑够久才显形；而唯一真正兜住的是 agent 侧租约——
> 它**设计上只是最后一道防线，不该成为唯一一道**。
> 顺带：`--dry-run` 原先也要求提权，**正是那道无谓的 UAC 门**让这个 bug 没能被提前发现，已去掉。

---

## 8. 未决问题（诚实列出）

1. TV 的 Shell 协议动作来源**未知**（E1 解决）。
2. 清空后"输入法是否能接受注入键"在**全部**映射键上的表现未验证（E5）。
3. 反作弊共存**无实测**（E6），且可能永远无法给出通用结论。
4. 该路线要求登录账号属于管理员组；标准账号场景不支持（ZSTDJan 实测同结论）。
5. 若"清空+注入"在某个应用里被证明不可用，是否存在"仅对该键保留原生"的降级形态，需要单独设计（当前未设计）。
6. **产品定位问题（需要用户拍板）**：目标用户是否都能在自己机器上自助提权（UAC 点"是"）。若是（个人自用电脑），推荐路线可行且日常近乎无感；若目标包含公司受管电脑或标准账号用户，则这条路线对他们**完全不可用**，需要重新评估取舍（见 4.4）。
7. ~~两级方案里"无权限档的 TV / 主页"取 ①维持现状 还是 ②保守收缩~~ → **已决策：取 ①维持现状（2026-09-22 用户确认，见 9.4）**。
8. 若将来要在"清空"之上支持**单键保留原生**（例如某键不想映射、想保持 Windows 原行为），参考实现里没有这项能力，需要自研 usage 白名单（见第 10 节第 5 条）。

---

## 9. 两级能力方案：有管理员权限的用户走新路，没有的用户维持现状

### 9.1 可行性结论：**可以，而且不是"两个版本"，是一条"按键级能力降级链"**

三条依据（全部来自代码，不依赖 README）：

1. **参考实现本身就是这么做的——能力缺失是按键级降级，不是功能整体停用。** `app.py:1367-1372` 注释逐字：「A missing or unverified Gadget is a **button-only degradation**」；`app.py:2184-2231` 只在失联时取消手势，Raw Input 监听器独立存在并继续作为兜底。
   ⚠️ 该仓库 README 第 158 行"取消或失败时**全部**自定义按键映射停用"与代码**不一致**，以代码为准（见第 10 节）。
2. **本仓库已经有"按键 × 触发 × 型号 → 能力"的现成机制**：`src/lib/bridge.ts:868-909` 的 `ShortcutCapability = "all" | "identity" | "none"`，配套 `src/pages/ButtonsPage.vue:460-466`（`capabilityNote` 接管提示）与 `:1055-1064`（格子级禁用）。做两级只需把**能力档位**加进这个函数的输入，不需要新造开关。
3. **架构边界本来就允许**：`AGENTS.md` 规定"需要提权的能力必须由独立、显式启动的 Helper 承载；主程序保持普通用户权限"，ADR 0002 的双轨也是同一形状——**无权限档 = 默认轨（现状），有权限档 = 增强轨（新路）**。

### 9.2 两档下逐键差异

| 按键 | 有权限（新路：报告层清空） | 无权限（维持现状：LL 钩子 + 常驻抑制） |
| --- | --- | --- |
| 返回 / 音量± | 可映射，零原生动作 | 输入栈不可见 → 格子禁用（无功能） |
| TV | 可映射，零原生（含 Shell 协议动作，待 E2 验证） | 格子禁用（现行矩阵为 `none`） |
| 主页 | 可映射，零原生 | 可映射，但**接管物理键盘 Home**（方案 C 常驻抑制） |
| 确定 / 方向 | 可映射，零原生 | 可映射；同键映射靠泄漏对冲，非同键**冷首按附带一次原生动作** |
| 菜单 / 电源 | 可映射，零原生 | 可映射（直接归因族，无泄漏） |
| 语音键 | 不依赖该 Helper（两档一致） | 同 |

### 9.3 落地必须做到的四件事

1. **运行时能力探测**（不是安装期判一次）：`helper_unavailable`（账号不能自提权 / UAC 被拒 / 注入被安全策略拦 / 未安装）→ 采用 legacy 矩阵；`helper_ready` → 采用新矩阵。账号切换、组件被杀、UAC 取消都会让档位变化。
2. **按键级降级、不静默改语义**：同一份配置在两档下只能是"该键不可用（格子禁用）"或"可用且行为已在界面注明"，**绝不能出现"用户以为映射了 A、实际执行了原生 B"**。UI 复用现有 `capabilityNote` 机制逐格说明。
3. **配置不因档位变化丢失**：与现行"能力消失只禁用、不删除"一致（PR #66 文档 §7.3 同款纪律）。
4. **拦截 `fail-open` + 动作 `fail-closed` + 自动恢复**：Helper 消失必须立即回到 legacy 行为，且不得继续执行已排队动作。参考实现给的量级可直接借用：Python 侧拦截租约 `HID_INTERCEPT_LEASE_SECONDS = 2.0`、续期 0.5s（`frida_compat.py:94-97`）；Gadget 侧上限 `MAX_INTERCEPT_LEASE_MS = 5000`、心跳 5s（`frida_hid_tap_runtime.py:59-61`）→ 即**"报告被清了但没人转发"的最坏窗口约 2 秒**，超过即自动撤钩；这个窗口仍须在 E4 真机测量，不得仅由常量推断。

   **📌 2026-09-23 实现状态**：spike 已按 `LEASE_MS = 2000` / 续期 `RENEW_MS = 500` 落地（与上述量级一致），见 `hardware/RC003/helper/agent/rc003_agent.js`。

   **一个重要更正——判据不能是"连接状态"**：实测 Frida 17 的 Socket API **没有可用的存活原语**：对端关闭后写入**不抛错**（3/3）、pending 的 `read` **不以 EOF 收尾**、连不上要**约 2.2s** 才以 ECONNREFUSED reject（太慢，不适合当存活判据）。因此 `fail-open` 的判据**只能是**租约式 `Date.now() - lastRenewAt <= LEASE_MS`。若照"连接断了就恢复"的直觉写，得到的是一个**永不触发**的 fail-open——这正是本项落地时最容易犯的错。

   **📌 2026-09-23 真机补充：租约之外的两道"收尾"保证曾经都是空的。** 首次真机 `observe` 跑完发现：① `--duration`（本项之外、但同属"不把钩子留在系统上"的保障）在有 agent 连接时**永不到期**——会话读取阻塞在 `BufReader::lines()`，主循环的收尾检查点被同步调用挡死（实测 300 s 的跑到 518 s 仍在运行）；② Ctrl+C / 关窗**不发 `disarm`**——从未安装 `SetConsoleCtrlHandler`，系统硬终止进程，`main()` 末尾的收尾代码永远到不了。
   两处都已修复并纳入自检回归（当时 23 项；现行 **34 项**，含阳性对照证明该回归项在修复前必然 FAIL）。**可迁移的教训**：功能路径通过 ≠ 收尾路径通过；这两类代码在自检与静态检查里都看不出来，只有"真机跑够久"才会显形。详见 [`2026-09-23-rc003-observe-first-real-run.md`](2026-09-23-rc003-observe-first-real-run.md)。

   **📌 2026-09-23 第三次真机教训：功能路径与收尾路径都通之后，还有"第二次运行"这条路。** 注入成功会让宿主**长期映射**运行时目录那份 Gadget DLL，而助手的运行时准备是无条件覆盖复制 → 第二次运行必然 `os error 32`。修法与一次性迁移见 [`2026-09-23-rc003-helper-rerun-blocked-by-locked-gadget.md`](2026-09-23-rc003-helper-rerun-blocked-by-locked-gadget.md)。**教训**：长期驻留的注入物会改变**文件系统的可用性**，这类耦合不会出现在第一轮跑通里。

   **📌 2026-09-23 第四次真机教训：功能全通之后，剩下的问题全在证据层。** 清掉旧世代 tap 后重跑，注入轮与**接管轮**双双通过（`how=injected` / `how=attached_existing_tap`，两轮三键各一组 `[EDGE]`），但同一轮的日志里埋着 **6 处会让人读错结论**的东西：多余的运行分隔线（把 6 轮数成 11 轮）、agent 的 `connect` 叠加导致 3 条连接里 2 条被 RST、读失败断线**在统计里完全不可见**、`discarded` 一物二用、接管轮误报 `sha256_verified=false`、`sent=false arm=true` 自相矛盾。**教训**：当功能验收通过之后，剩下的缺陷往往不再表现为失败，而表现为**证据说谎**——而排查时我们唯一依赖的就是证据。判据是「这个字段能不能被读成别的意思」，而不是「这条日志有没有出现」。验收判决与修法见 [`2026-09-23-rc003-rerun-acceptance-verdict.md`](2026-09-23-rc003-rerun-acceptance-verdict.md)。

   **仍未做**：E4 真机测量（含"杀掉助手后三键自动恢复原生行为"的实测）。

### 9.4 已决策：无权限档的 TV / 主页**维持现状**

**决策（2026-09-22，用户确认）**：取选项 ①——无权限档不缩小现有能力。

| 选项 | 做法 | 代价 | 状态 |
| --- | --- | --- | --- |
| **①维持现状** | 无权限档保持今天的矩阵：TV 禁止映射、主页可映射但接管物理 Home | 这批用户仍带今天的副作用，但行为与他们现在使用的完全一致 | **已采纳** |
| ②保守收缩 | 无权限档把 TV / 主页也标成 `none`（禁用映射） | 无权限用户少两个键可用，但不再劫持物理键盘 | 未采纳（保留为将来可选项） |

**该决策在实现上的确定含义**（避免后期歧义）：
1. 无权限档的能力矩阵**必须逐字等于今天已验收的行为**，不得因为引入两级而改动既有档位行为；今天的矩阵以 `src/lib/bridge.ts:890-909` 的 `shortcutCapability` 与 `crates/sayall-windows/src/key_gate.rs` 的常驻抑制为唯一事实来源。
2. 界面必须如实标注"无管理员权限时该键的行为"（主页=接管物理键盘 Home；TV=不可映射），不允许沉默降级成"看起来可映射、实际执行原生动作"。
3. 两级差异只体现在**有权限档**：返回 / 音量± 从"不可见、禁用"变为"可映射"，TV / 主页从"禁用 / 接管物理键"变为"可映射、零原生"。
4. 这一决策与 4.4 节的结论一致：不接受"为了清零副作用而砍掉无权限用户的功能"。

## 10. 代码级复核结果：README 声称 与 实际实现 的差异

审计对象：ZSTDJan `windows-remote-mic-app`（HEAD）与 QL-4/RemoteMapper（HEAD）。**以下只列 README/文档与代码不一致，或代码无法证实 README 的说法**；未列出的声称（一次性 UAC、日常免提权、共享宿主保护、`restart_required`、失败补齐松键、清空发生在翻译之前）均已在代码中逐行核实成立。

| # | README/文档声称 | 代码实际 | 证据 |
| --- | --- | --- | --- |
| 1 | "取消或失败时**全部**自定义按键映射停用，只保留 Windows 原始按键" | 是**按键级降级**：Gadget 缺失/未验证时只有 Windows 丢弃的 usage（返回/音量±）不可用，其余键继续走 Raw Input 兜底 | `app.py:1367-1372`、`app.py:2184-2231` |
| 2 | "预授权任务要求**当前账号属于管理员组**" | 判据不是组枚举，而是 token 判据：已提权 → 允许；否则 `token_elevation_type() == TOKEN_ELEVATION_TYPE_LIMITED`（当前 token 被 UAC 过滤，说明账号持有可用的完整 token）。**结论与文档一致**（标准账号取不到 LIMITED，会被拒并显示 `current_account_cannot_self_elevate`），差异仅在实现手段。**本节初稿曾写成"该判据在已提权时为假"，经逐行复核为错误，已更正。** | `hid_elevation_windows.py:401-416`（含 `:411-412` 已提权即 True）、`:2838-2841`；测试 `tests/test_hid_elevation_windows.py:3280,3478` |
| 3 | 仅笼统说"复制并**清空被接管的 HID 报告**"，未界定粒度 | 清空**只看 report ID `0x01`、不看 usage**：只要前三字节是 `01 00 00` 就清零三个 usage 槽。若该设备报告里出现字母类 usage，同样被清 | `frida_hid_tap_runtime.py:490-498`；Python 侧 `frida_compat.py:465` |
| 4 | "不根据普通键盘事件猜测设备" | 归属确实靠设备注册表 ContainerID + 调用者必须为 `wudfhost.exe`（成立）；但"清空动作"本身不校验内容——文档未提示该边界 | `frida_hid_tap_runtime.py:324-374,485-500` |
| 5 | 未明说"清空后无法为单个键保留原生行为" | 代码里**没有** usage 白名单/例外机制（白名单只用于"候选探测"阶段） | `frida_hid_tap_runtime.py:400-407`（探测）vs `:485-500`（执行） |

**对 RemoteMapper 的自查结论（我逐行读了 `driver.c` 与 `remap.c`，不是只看 README）**：
- 是标准 KMDF lower filter（`WdfFdoInitSetFilter`，`driver.c:41`），只注册 `EvtIoRead`（`:57`），其余请求类型由框架自动下发；
- 在下层完成后**原地、等长**改写 `Report[3]`（`driver.c:116-141` + `remap.c:35-71`），并原样返回下层的 `lowerStatus` 与 `information`（长度不变）；
- **没有任何"丢弃/抑制"路径**：不在映射表内的键 `default: return FALSE`，缓冲区保持原样（fail-open）；
- 只处理键盘 report `0x01`（`remap.c:35`），vendor report 不动——**这也是它无法触及"非键盘页 Shell 协议动作"的原因**；
- 单键改写假设：远程一次只按一个键，键值落在 `Report[3]`（`remap.c:39-42` 注释）。

## 附录 A：取证方法与本地副本

- 五个参考仓库的完整文件树与关键文件已下载到本机（文件名中的 `/` 以 `__` 代替）：
  - `C:\Users\Administrator\ref-repos\RemoteMapper\`（QL-4/RemoteMapper）
  - `C:\Users\Administrator\ref-repos\vibe-flow\`（richlearntodo-debug/vibe-flow）
  - `C:\Users\Administrator\ref-repos\axonkey\`（leowzz/axonkey）
  - `C:\Users\Administrator\ref-repos\zstdjan\`、`...\zstdjan-code\`、`...\zstdjan-pinned\`（ZSTDJan/windows-remote-mic-app）
  - `C:\Users\Administrator\ref-repos\suk-ldev\`（Suk-ldev/remote-mic-app-windows）
  - 各仓库文件名清单：`C:\Users\Administrator\ref-repos\*.tree.txt`
- 抓取命令（`gh` 需要 git 在 PATH，本机 Git 位于 `…\PortableGit\versions\1.2.0\mingw64\bin`）：
  ```
  gh api repos/<owner>/<repo>/contents/<path>?ref=<SHA> --jq .content   # base64，去换行后解出原文
  gh api repos/<owner>/<repo>/git/trees/<SHA>?recursive=1 --jq '.tree[].path'
  ```
- 本仓库对照档案：`docs/investigations/2026-09-05-rc003-back-volume-buttons-invisible.md`、`docs/investigations/2026-09-06-left-double-response-arm-deadlock.md`、`Bugs/2026-09-10-win-l-mapping-and-capture.md`、`crates/sayall-windows/src/key_gate.rs`、PR #66 的 `docs/architecture/rc003-enhanced-capture.md`。
- 未做：真机测试、驱动安装、脚本执行、对任何参考仓库的写操作。

## 附录 B：证据索引（逐条可核验）

**复核级别**：`本人逐行` = 本会话内由撰写者本人按固定 SHA 逐行读取并核对；`子代理逐行` = 由并行取证子代理按同一仓库读取并给出逐字引用（同一版本范围内，撰写者未二次逐行复核）；`未复核` = 仅由文档描述推导，需复审时补读。

| 论述 | 证据（文件:行号） | 复核级别 | 状态 |
| --- | --- | --- | --- |
| 清空发生在 Windows 翻译之前，清空 3 个 usage 槽 | `frida_hid_tap_runtime.py:485-500`（`:498` 逐字 `pointer.add(3).writeByteArray([0, 0, 0, 0, 0, 0]);`、`:493-495` 注释含 "before taking ownership"） | 本人逐行 | `passed` |
| 清空只认 report ID `0x01`（`010000` 前缀），无 usage 白名单 | `frida_hid_tap_runtime.py:486-490`（长度必须 9；前缀必须 `010000`）；Python 侧 `frida_compat.py:465` | 本人逐行（JS 侧）/ 子代理（Python 侧） | `passed` |
| 接管前必须先见到"中性报告"（防只有 key-up 被拦） | `frida_hid_tap_runtime.py:491-497` | 本人逐行 | `passed` |
| 缺 Gadget 是"按键级降级"而非整体停用 | `app.py:1367-1373`（docstring 逐字 "A missing or unverified Gadget is a button-only degradation"）、`app.py:2184-2188`（失联时只 `_cancel_input_gestures(..., reason="hid_tap_ownership_lost")`） | 本人逐行 | `passed` |
| 拦截租约 2.0s / 续期 0.5s / 关断 ack 0.5s / 安全余量 0.15s | `frida_compat.py:94-97` | 本人逐行 | `passed` |
| Gadget 侧租约上限 5s、心跳 5s | `frida_hid_tap_runtime.py:59-61` | 子代理逐行 | `passed` |
| 自提权判据 = 已提权或 `TOKEN_ELEVATION_TYPE_LIMITED`；不可自提权时返回 `current_account_cannot_self_elevate` | `hid_elevation_windows.py:401-416`、`:2838-2841` | 本人逐行 | `passed` |
| 计划任务 XML **不含** `<Triggers>`，由 COM `RegisterTask` 注册 | `hid_elevation_windows.py:1145,1168-1182,1264-1265,1625` | 子代理逐行 | `passed` |
| 日常注入走"计划任务按需运行"（`task.Run("")`），`runas` 仅用于安装/卸载 | `frida_compat.py:404-435`、`hid_elevation_windows.py:1744-1750`、`:2732,2815` | 子代理逐行 | `passed` |
| 助手进程注入完即退出（单分支、无循环） | `hid_elevation_windows.py:3041-3075`、`src/hid_helper_launcher.py:22` | 子代理逐行 | `passed` |
| 设备归属 = ContainerID + 调用者必须是 `wudfhost.exe`；共享宿主不接管 | `frida_hid_tap_runtime.py:324-374,349-364,1337-1339`、`frida_compat.py:979-985` | 子代理逐行 | `passed` |
| 本仓库没有已合入的用户态 HID 写回管线 | `gh pr view 66`：状态 `OPEN`，head `47118bf1bb1b184cb670f0524e776e9c1779720f`；当前 `main` 不含 `Testing/rc003-user-hid/` | 本人核对 | `passed` |
| PR #66 的同步/异步边界 | `Testing/rc003-user-hid/hid_tap.js:118-123`：`STATUS_PENDING` 只计数；`:124-151` 只解析已完成 `IO_STATUS_BLOCK` | 本人逐行 | `passed` |
| PR #66 的已知 IPC 阻塞项 | `docs/architecture/rc003-enhanced-capture.md:362-384`：握手合包可复现且未修复 | 本人逐行 | `passed` |
| 本轮审查范围 | 当前 `main` `69e56e4`；PR 109 合并提交 `411556c2c7ca09dfeb38e6e1b660c917735b3a95` | 本人核对 | `passed` |
| 旧组件驻留导致 `restart_required` 分支确实存在 | `hid_host_reload_windows.py:138-164`、`frida_compat.py:1120-1122,1298` | 子代理逐行 | `passed` |
| 测试覆盖共享宿主 / 租约过期 / 标准账号被拒 | `tests/test_hid_copy_probe.py:163-172`、`tests/test_hid_copy_interception.py:128,141,172`、`tests/test_hid_elevation_windows.py:175,202,211,2860,3280,3478` | 子代理逐行 | `passed` |
| RemoteMapper 驱动：KMDF lower filter、只 `EvtIoRead`、原地等长改写 `Report[3]`、无丢弃路径、只碰 report `0x01` | `driver.c:41,57,116-141`、`remap.c:35,43-71`；本地副本 `remap.c` 与固定 SHA 的 SHA-256 一致（`56b8719b…3b596a`） | 本人逐行 | `passed` |
| RemoteMapper 映射表含 Home `0x4A`、TV/直播 `0x35`、返回 `0xF1`、音量 `0x80/0x81`、语音 F5 `0x3E` | `remap.c:5-27,43-70`；驱动 `README.md` 表；`keymap.txt` | 本人逐行（代码）/ 子代理（文档） | `passed` |
| vibe-flow 驱动按 MakeCode **真丢弃**（非重映射），并修正 `InputDataConsumed` | `driver/rc003-filter/src/rc003_filter.c:306-320,340` | 本人逐行 | `passed` |
| vibe-flow 心跳超时即 `Rc003DisarmLocked()` 清空策略（fail-open） | `rc003_filter.c:352-373` | 本人逐行 | `passed` |
| vibe-flow 该候选**尚未编译/未实机验证** | 其 `driver/rc003-filter/README.md` 自述 + CI 仅出未签名候选 | 子代理逐行 | `passed`（仅证明"文档如此声称"） |
| axonkey：产品 10 键走 Interception（按 hardware_id 设过滤、命中即不回注），返回/音量± 走 Frida **只读** | `src-tauri/src/input_service/windows.rs`（`get_hardware_id`/`set_filter` 调用点）、`rc003_hid_gadget.js` 只读 IOCTL | 子代理逐行 | `passed` |
| axonkey：上传缺陷"重连后设备槽位耗尽→输入整体失效、只能重启"未修 | `docs/INTERCEPTION_HOTPLUG_INCIDENT.md`（上游 issue #193） | 子代理逐行 | `passed`（仅证明"其文档如此记录"） |
| Interception 商用分发需另行授权 | `vendor/interception/SOURCE.md:21,29-30` | 子代理逐行 | `passed` |
| Suk-ldev 的 `native/rc003-hook` 是 Detours **只读**钩、注入系统 `WUDFHost.exe` | `native/rc003-hook/hook.cpp:47-48,106,153-154`、`hook_protocol.h:11-12`、`inject.cpp:73,96,99,118,126-129` | 子代理逐行 | `passed` |
| 本仓库两级落地所需既有机制存在 | `src/lib/bridge.ts:868-909`、`src/pages/ButtonsPage.vue:460-466,1055-1064`、`crates/sayall-windows/src/key_gate.rs:17-20,210-215,253-257` | 本人逐行 | `passed` |

## 附录 C：供外部复审的核对清单

### C.1 关键结论的证伪路径（每一条都可被推翻）

| 结论 | 怎么证伪 |
| --- | --- |
| "报告层清空能消除全部原生动作" | 在真机上按 TV / 主页，观察是否有任何原生键或 Shell 动作（含锁屏 + Edge 未安装场景）。**实验 E1/E2 尚未执行 → 该结论当前为 `deferred`。** |
| "按设备、物理键盘不受影响" | 同时接入 RC003 与物理键盘，在遥控器在线时做高频输入回归（`` ` ~ / Home / 方向 / Enter ``）。**E3 未执行 → `deferred`。** |
| "失败即回到原始行为、不残留清空" | 杀掉助手 / 让租约超时 / 拔断蓝牙，观察是否在数秒内恢复原始按键且按键本身仍可用。**E4 未执行 → `deferred`。** |
| "不需要内核驱动、不改系统安全设置" | 审计安装/卸载流程与注册表/BCD 变化。参考实现代码支持该结论（无驱动安装路径），但**我们自己的实现尚未存在**。 |
| "无权限用户只是按键级降级，不是功能全灭" | 代码证据见附录 B；但需注意这是**参考实现**的行为，不能直接推断我们实现后的表现。 |

### C.2 已知缺口（外部复审请重点看这些）

1. **TV 的独立 Shell 协议动作来源未知**：本仓库实测它在键盘边沿全部被吞掉后仍出现（`Bugs/2026-09-10-win-l-mapping-and-capture.md` §8）；参考实现的清空只覆盖 report ID `0x01`。这是推荐路线的**最大不确定点**。
2. **清空后所有映射键都变纯注入**：输入法过滤注入键的问题可能从语音键扩散到全部映射键（本仓库 `Bugs/2026-09-04` 已证实 WeType/豆包过滤注入和弦）。**未验证。**
3. **双同型号设备**：ZSTDJan 按宿主/句柄绑定（不匹配即不接管）；RemoteMapper 按硬件 ID 绑定（两台同型号都会命中）；vibe-flow 为全局单例策略。三种行为不一致，需按我们的使用场景确认。
4. **反作弊 / 安全软件共存**：三个项目的文档都标为未验证；`deferred`，可能无法给出通用结论。
5. **所有引用的真机结论均来自他人项目**（RemoteMapper 的 8 键 PASS 是其自述）；我们**没有任何一项自己的真机数据**。E1–E6 全部未执行。
6. **子代理逐行复核的条目未由本人二次复核**（附录 B 已逐条标注）。若复审结果与本文冲突，以代码为准，并请指出具体 `文件:行号`。
7. 本文**不包含**任何许可证结论之外的商业/法律判断；GPL-3.0 与 MIT 的适配性只按仓库 LICENSE 文本陈述。

### C.3 复审时的建议顺序

1. 先用附录 D 的 SHA 取回文件（避免"上游已更新导致行号漂移"）。
2. 抽查附录 B 中标注 `子代理逐行` 的条目——这是最可能存在行号偏差的部分。
3. 再评估附录 C.2 中第 1、2 两项是否足以推翻推荐路线；若会推翻，请给出替代路线与依据。

## 附录 D：版本固定（核对时的 commit SHA）

抓取时间：2026-09-22 00:20–01:00（GMT+8）。**若上游在此之后有新提交，行号可能漂移，请按下列 SHA 取回。**

| 仓库 | 核对时的 commit SHA | 用途 |
| --- | --- | --- |
| `ZSTDJan/windows-remote-mic-app` | `1e6b1d285f9cd50f30c5bc92ac7787a693fc993d` | 推荐路线（报告层清空）的主力证据 |
| `QL-4/RemoteMapper` | `be8b57330c26a70d8b8ec9ff1e60c23251a2fc31` | 次选（下层 HID 过滤驱动）；该 SHA 亦被本仓库 PR #66 的 ATTRIBUTION 引用 |
| `richlearntodo-debug/vibe-flow` | `97fa69cb6831781ebb1dc2ad5f79090a90c1f937` | 键盘类上层过滤驱动（真吞） |
| `leowzz/axonkey` | `823ae4b339a165c8d0b5699d22baf8c564ebd88e` | Interception 路线与 Frida 只读通道 |
| `Suk-ldev/remote-mic-app-windows` | `d01dd61f7f4e91a03422b31b1898c8b11f8e2a50` | Detours 只读钩（工程路径参考） |
| `GetSayAll/remote-mic-app-windows` | `f02c3dc226fbe8540f86c6e4c8166e5b29d8d2b7` | 本文档初次起草时的 main 基线（PR 合入时须重新取最新 main） |
| `GetSayAll/remote-mic-app-windows` PR #66 | `47118bf1bb1b184cb670f0524e776e9c1779720f` | 复审时核对的开放 PR head；未合入当前 `main`，含未修复握手合包问题 |
| `GetSayAll/remote-mic-app-windows` 当前 main | `69e56e4c3e6c6276a8716609a55fdc05828f93c3` | 本轮复审工作分支的基线（PR 109 已合入后的主线） |
