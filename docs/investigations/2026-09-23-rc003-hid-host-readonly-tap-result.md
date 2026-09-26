# RC003 HID 宿主报告层只读 tap 实验结果（2026-09-23）

> **目的**：用一次提权、**只读**的最小注入实验，把
> `2026-09-23-zstdjan-hid-host-tap-implementation-review.md` §5 未验证项 1 从
> `structural` 升级为 `passed` / `failed`——直接回答"本机 RC003 的 9 字节键盘报告里
> 到底有没有 `0x00F1`（返回）/ `0x0080`（音量+）/ `0x0081`（音量−）"。
> **方法**：在已核验的「独占 RC003」`WUDFHost.exe` 内挂 `ntdll!NtDeviceIoControlFile`，
> 命中 `IOCTL 0x80018483` 时转储输入/输出缓冲区。**不清空、不注入按键、不改任何系统设置。**
> **核验状态词汇**：`passed` = 实际执行并观察通过；`failed` = 实际执行不满足预期；
> `deferred` = 依赖当前不可得条件；`structural` = 静态取证或只读实测支持，未做端到端执行。

---

## 1. 结论摘要

**判定：`three_keys_present_in_report`（`passed`，阳性）。**

本机 RC003 的报告层**确实存在**目标三键，且与参考实现的判据**逐条吻合**。三键在
Windows 侧不可见的原因因此**闭合为"键码映射阶段丢弃"**，而不是"设备不上报"：

| 观测项 | 实测值 | 与参考判据 |
| --- | --- | --- |
| 命中 IOCTL | `0x80018483` | 一致 |
| 输入长度 | `8`（唯一取值） | 一致 |
| 输出长度 | `9`（唯一取值） | 一致 |
| 输入第 5/6 字节（operation/selector） | `02/01`（唯一取值） | 一致 |
| 报告前缀 | `report_id = 0x01`，30/30 | 一致 |
| 阳性对照 usage | `0x0028`（确定）×4、`0x004A`（主页）×2 | 通道被读到 |
| **目标 usage** | **`0x00F1`（返回）×3、`0x0080`（音量+）×3、`0x0081`（音量−）×3** | **present** |

`onLeave` 时输出缓冲区发生变化 **0/30** —— 直接证实参考实现台账所称的时序：
**报告在调用前就已由宿主填好，内核在调用期间不改写它**，因此"清空必须发生在
`onEnter`"这条最容易踩空的细节在本机成立。

---

## 2. 实验装置

| 项目 | 内容 |
| --- | --- |
| 目标宿主 | `WUDFHost.exe`，`HostPid=32684`（0x7fac），判据 `exclusive_rc003_host` |
| 宿主定位 | `Device Parameters\WUDFDiagnosticInfo\HostPid`（只读注册表），复用 `wudf_host_probe.py` |
| 注入 | `frida` 17.18.0（Python 侧 `frida.attach`），**一次 UAC 提权** |
| 挂钩点 | `ntdll!NtDeviceIoControlFile`（命中后仅在 `onEnter`/`onLeave` 读取缓冲区） |
| 探针 | `hardware/RC003/probes/wudf_ioctl_tap.js`（Frida agent）+ `wudf_ioctl_tap.py`（驱动与判定） |
| 启动器 | `hardware/RC003/probes/run-wudf-ioctl-tap.ps1` / `.cmd`（ASCII-only，需管理员） |
| 采集窗口 | `pre` 15 s → `A` 阳性对照 12 s → `B` 目标三键 16 s → `post` 4 s |
| 证据 | `hardware/RC003/evidence/wudf-ioctl-tap.log` |

**只读契约（执行时逐条遵守）**：不写入任何目标缓冲区（参考实现在 `onEnter` 把 6 字节
usage 槽清零，本实验**一字节都不动**）；不调用 `DeviceIoControl`；不 `SendInput`；
不写注册表；不装计划任务；不安装/修改任何驱动或系统设置。

**阳性对照（可伪证性设计）**：阶段 A 要求按**主页**与**确定**——这两个键在 Windows 侧
本来就可用，若 tap 正常必然出现它们的 usage。没有这组对照，"零事件"无法区分
"设备没发"与"探针坏了"（2026-09-22 那次层错位判定正是栽在这里）。

---

## 3. 原始结果

### 3.1 逐条记录（节选自 `wudf-ioctl-tap.log`）

报告只有 `report_id=0x01` 一种；`out` 为 9 字节 = `report_id` + `modifiers` + `reserved`
+ **3 × uint16 usage**（小端）：

| 阶段 | `in`（8 B） | `out`（9 B） | 解出的 usage |
| --- | --- | --- | --- |
| `A` | `802f000002010000` | `0100004a0000000000` | `0x004A` 主页（阳性对照） |
| `A` | `bc24000002010000` | `010000280000000000` | `0x0028` 确定（阳性对照） |
| `B` | `dc22000002010000` | `010000f10000000000` | **`0x00F1` 返回** |
| `B` | `9c3b000002010000` | `010000800000000000` | **`0x0080` 音量+** |
| `B` | `c81f000002010000` | `010000810000000000` | **`0x0081` 音量−** |
| 各键抬起 | 同左 | `010000000000000000` | 中性报告（全部松开） |

- 每次按键由**两条**报告构成：按下（带 usage）+ 抬起/中性（usage 全 0），三键各 3 次
  即 3 次按下边沿，与用户实际按键次数一致。
- `in` 的前 4 字节随调用变化（如 `402f0000`、`a0400000`、`dc220000`），取值均小于
  `0x10000` 且不重复；**本轮不对其语义下结论**（疑似请求/主机端句柄或序号）。
  第 5–8 字节恒为 `02 01 00 00`。

### 3.2 判定输出（`--analyze` 离线重算，无需提权）

```
解析到记录 60 条（enter/leave 各半）
命中 IOCTL 0x80018483 的记录数: enter=30 leave=30
输入长度集合: [8]（参考判据 8）
输出长度集合: [9]（参考判据 9）
输入第 5/6 字节（operation/selector）集合: ['02/01']（参考判据 02/01）
调用方模块: ['KERNELBASE.dll+0x3f723']
与参考实现判据一致: True

report_id=0x01 的报告数: 30
  0x0028 确定/OK (Enter) × 4  ← 阳性对照键
  0x004A 主页/Home × 2  ← 阳性对照键
  0x0080 音量+ (Volume Up) × 3  ← 目标三键
  0x0081 音量- (Volume Down) × 3  ← 目标三键
  0x00F1 返回 (Back) × 3  ← 目标三键

onLeave 时输出缓冲区发生变化: 0/30
判定: three_keys_present_in_report（阳性，路线 A 前提在报告层成立）
```

### 3.3 附带观测

- **调用方唯一**：所有命中都来自 `KERNELBASE.dll+0x3f723`，即宿主经 `DeviceIoControl`
  （kernelbase）进入 `NtDeviceIoControlFile`。这与参考实现"挂 `DeviceIoControl` 导入槽做
  来源归因"的做法互为印证。
- **IOCTL 谱干净**：整个窗口内 `NtDeviceIoControlFile` 总调用 = 30，**全部**是目标 IOCTL，
  没有其它 IOCTL 噪声。说明该宿主在该时段只做报告交付这一件事。
- **无高频心跳**：47 秒内 30 次调用 ≈ 0.64 次/秒，且全部对应用户按键边沿。
  即便真的落地拦截，写入路径的触发频率也是"按次"而非轮询级。

---

## 4. 这证明了什么 / 没证明什么

### 4.1 证明（`passed`）

1. **设备确实上报三键**：`0x00F1` / `0x0080` / `0x0081` 以 16 位 usage 出现在键盘页
   `report_id=0x01` 报告里，与 `0x0028` / `0x004A` 同处一个报告结构。
2. **报告在 `WUDFHost.exe` 内可读**：用户态、无内核驱动即可拿到完整原始报告。
3. **三键不可见的因果链闭合**：设备上报（本节）→ 报告到达宿主层（本节）→
   Windows 事件层零事件（`raw-input-dump2.log`）⇒ 丢弃发生在 **HID→VK 键码映射**
   （`kbdhid`），而非设备或 UWP/HID 传输层。
4. **参考实现的"`onEnter` 清空"时序成立**：`out_changed` 0/30，报告在调用前已就位。
5. **参考实现的判据可移植**：`0x80018483` / in 8 / out 9 / `02 01` 在本机
   （Windows 11 25H2，Build 26200.7171）逐条命中，不依赖其私有实现细节。

### 4.2 未证明（仍是 `deferred`）

1. **tap 可落地**：本轮**只读**。真正的拦截需要在 `onEnter` 写入（清零 usage 槽），
   这会改变系统行为，需要单独授权与失效验收。
2. **边沿配对与失效语义**：粘键抑制、接管就绪门、租约、tap 失联时"全部自定义映射停用"
   等 fail-safe 合同，本轮一条都未验证。
3. **异机 / 版本相关性**：本机为 Windows 11 25H2（Build 26200.7171）。参考实现自陈
   "蓝牙版本号本身不能判定兼容，异机须核对 Windows/驱动链和真实报告"。本机版本已记录
   在附录 B；其它机器仍须复跑。
4. **共享宿主路径**：本机是 `exclusive_rc003_host`，`shared_host` 下的来源核验路径未涉及。
5. **杀软 / EDR / HVCI 现场行为**：本轮注入成功（说明本机未被拦截），但未评估
   在启用更严格策略的机器上的表现；参考实现自己列出"游戏反作弊识别风险"。
6. **RC001**：本轮只针对 RC003。RC001 走厂商键 VK `0xFF` 路径，机制不同，不可外推。
7. **一次提权的工程代价**：本轮用交互式 UAC 手动提权完成。产品化需要的"提权助手 +
   固定计划任务"形态~~、以及 ADR 0002 §3「不使用 Frida」的处理，均未决~~ → **2026-09-23 已定**：
   ADR 0002 §3 的「不使用 Frida」废止、R5 口径收窄，"提权助手 + 固定计划任务"不再被禁；
   产品化形态本身（安装/卸载审计、失败不留残余）仍待实现与验收。

---

## 5. 探针工程坑（本轮新增，可复用）

1. **`post()/recv()` 反向通道在 session 0 目标上不可靠——只送达第一条消息。**
   首次真机运行中，Python→JS 的 `post()` 只让阶段停在 `pre`、收尾汇总为空，而同一次运行里
   JS→Python 的 `send()` 全程正常。自检脚本 `probes/frida_msg_selftest.py` 显示
   `post/recv` 在 **spawn 与 attach 两种模式**下对普通进程都可用，故该异常归因于
   session 0 目标环境，**根因未定位**。
   → **设计结论：阶段归属与统计一律不由反向通道承担**。每条记录自带 `t`（`Date.now()`），
   由 Python 侧按本地时间轴归属阶段；累计统计随每次心跳用 `send()` 上行；
   判定从原始记录重算，不依赖任何汇总字段。
2. **并行 agent 会覆盖同一 worktree 的文件。** 本轮实测：探针的 `MAX_RECORDS` 被改成
   1200 后又变回 4000，同时 `codex.exe`（PID 8600）正在同一工作副本运行。
   → 编辑探针后**必须回读校验**；排查异常前先确认是否有其它 agent 在跑。
3. **提权动作只能由用户本人执行。** WorkBuddy 安全策略拦截 agent 代提权（从 Bash 调
   PowerShell 被拒；PowerShell 里带提权动词的 `Start-Process` 也被拒）。
   → 探针准备好可复制执行的入口（`.cmd` 右键"以管理员身份运行"）+ 屏幕分阶段提示，
   agent 负责读日志与判定。
4. **只读探针也要有阳性对照。** 见 §2 末。本轮阶段 A 的 `0x0028`/`0x004A` 就是那道保险。

---

## 6. 对既有判定的影响

| 文档 / 条目 | 原状态 | 新状态 |
| --- | --- | --- |
| `2026-09-23-zstdjan-hid-host-tap-implementation-review.md` §5 未验证项 1 | `deferred` | **`passed`**（本文） |
| 同文档 §6 建议下一步 2"最小注入实验收口" | 建议 | **已完成** |
| `2026-09-05-rc003-back-volume-buttons-invisible.md` 的归因 | 已更正为"设备上报、`kbdhid` 丢弃" | 归因**得到报告层直接证据**（此前是推断链，现在是实测） |
| `2026-09-22-per-device-key-interception-route-selection.md` 路线 A 前提 | 勘误后"未被证伪" | 前提**在报告层实测成立**；2026-09-23 产品决策已解除 ADR §3 与 R5 两项阻塞 |
| 决策点：路线 A / 路线 B / 维持不可用 | **2026-09-23 已拍板走路线 A 的写入版验证** | 约束已解除；剩余为写入路径验证与 R6 类失效验收 |

---

## 7. 下一步与决策点

1. ~~**产品拍板**（无技术阻塞）：
   - ADR 0002 §3「不使用 Frida」是否重开？可改为自研注入组件——机理不依赖 Frida，
     Frida 只是"注入 + 脚本载体"，本轮已证明只读部分可自持。
   - R5 的意图是否容纳"提权助手 + 固定计划任务"这一新增系统级形态？~~
   → **2026-09-23 已拍板**：两项均已解除/收窄（见 [ADR 0002 修订记录](../decisions/0002-dual-track-injection-optional-helper.md)
   与路线选型文档 §1 的 R5 行）。**无剩余产品阻塞**。
2. **走路线 A 的下一步**：先做**写入版**最小实验（`onEnter` 改写 / 擦除 + 应用侧按 usage 重映射），
   装置为 `hardware/RC003/probes/wudf_ioctl_write.py`；通过后再按异机、断连、睡眠、快速连按
   分别验收边沿配对与 fail-safe，并按 §4.2 逐项收口。
3. **若走路线 B**：需 `TESTSIGNING` + 关 Secure Boot + 重启，正式发布要过 WHCP，
   与仓库 R5 冲突；本轮结果对其无影响（两条路线的可行性互相独立）。
4. **无论哪条路**：RC001 与 RC003 必须**分别**真机验收，不可互推。

---

## 附录 A：证据索引

| 证据 | 位置 | 说明 |
| --- | --- | --- |
| 本次实验完整日志（原始转储 + 逐条报告 + 判定） | `hardware/RC003/evidence/wudf-ioctl-tap.log` | 3 键各 3 次 + 阳性对照 6 条 |
| 只读 tap 探针（Frida agent） | `hardware/RC003/probes/wudf_ioctl_tap.js` | 单向 `send()`；`onEnter`/`onLeave` 双转储；**不写入** |
| 只读 tap 驱动与判定 | `hardware/RC003/probes/wudf_ioctl_tap.py` | `--dry-run` / 真机运行 / `--analyze` 离线复核 |
| 提权启动器 | `hardware/RC003/probes/run-wudf-ioctl-tap.ps1`、`.cmd` | ASCII-only，右键"以管理员身份运行" |
| Frida 消息通道自检 | `hardware/RC003/probes/frida_msg_selftest.py` | 证明 `send` 与 `post/recv` 在 spawn/attach 下均可用 |
| 宿主探测（前提） | `hardware/RC003/evidence/wudf-host-probe.log` | 独占性判定 + 权限拒绝记录 |
| 事件层零事件 | `hardware/RC003/evidence/raw-input-dump2.log` | 因果链的"Windows 侧不可见"一端 |
| 声明 usage 取证 | `hardware/RC003/evidence/hid-direct-control.log` | `HidP_GetButtonCaps`：三键声明在 `report_id=0x01` |

## 附录 B：本机环境版本（供异机比对）

| 组件 | 版本 |
| --- | --- |
| Windows | Windows 11 25H2，Build 26200.7171（注册表 `ProductName` 仍报 "Windows 10 Home China"） |
| `mshidumdf.sys` | 10.0.26100.1150 |
| `hidclass.sys` | 10.0.26100.7019 |
| `kbdhid.sys` | 10.0.26100.1882 |
| `WUDFRd.sys` | 10.0.26100.7019 |
| `WUDFHost.exe` | 10.0.26100.7019 |
| `ntdll.dll` | 10.0.26100.7019 |
| `KERNELBASE.dll` | 10.0.26100.7171 |
| frida | 17.18.0（Python 3.13，隔离 venv：`C:\Users\hd838\.workbuddy\binaries\python\envs\default`） |

## 附录 C：复现步骤

```bash
# 1. 只做定位与自检（无需提权，不注入）
python hardware/RC003/probes/wudf_ioctl_tap.py --dry-run

# 2. 真机运行：以管理员身份执行（会弹 UAC），按屏幕提示操作遥控器
#    右键 run-wudf-ioctl-tap.cmd ->「以管理员身份运行」
#    或管理员终端：
powershell -ExecutionPolicy Bypass -File hardware/RC003/probes/run-wudf-ioctl-tap.ps1

# 3. 离线复核既有日志（无需提权、无需设备在线）
python hardware/RC003/probes/wudf_ioctl_tap.py --analyze hardware/RC003/evidence/wudf-ioctl-tap.log
```

退出码语义：`0` 阳性 / `1` 未找到宿主 / `3` 未提权 / `4` 缺依赖 / `5` attach 失败 /
`6` 导出为 null / `7` 该 IOCTL 未出现 / `8` 阳性对照缺失 / `9` 阴性。
