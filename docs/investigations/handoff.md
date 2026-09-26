# 交接文档（handoff）——驱动签名规避调查后续

目的：新代理 10 分钟内接手。日期：2026-09-04。调查已闭环（R5 终审 PASS），本文件指引后续执行。

> ## ⚠️ 2026-09-23 状态指针（本节比下文新，先读这里）
>
> 本文件正文停留在 **2026-09-04** 的"驱动签名规避 + WeType 产品化"阶段。此后主线已转到
> **RC003 返回/音量±三键的按设备源头捕获**，与下文的优先级列表已不重合。当前状态：
>
> - **机制问题已闭合**：三键"不可见"的因果链 = 设备上报 → 报告到达 `WUDFHost` 宿主层 → `kbdhid` 键码映射丢弃（真机只读实测 `passed`）；`onEnter` 改写字节**会被 Windows 采纳**（真机写入实测 7/7 PASS）。
> - **形态问题已就位**：`hardware/RC003/helper/` 产品化 spike（提权助手 + Gadget agent + 自检台），自检 **49/49 + 5/5 PASS**（无需提权/设备）；**未并入产品工作区**。
> - **宿主定位链已在真机（免提权）成立**：`--dry-run` 退出码 0，`diag_keys=6` → `pid=32684 WUDFHost.exe` → `exclusive_rc003_host` → Gadget 校验通过；与只读探针**两个独立实现给出同一 PID**（`evidence/wudf-host-probe-crosscheck.log`）。
>   首跑时曾误报"设备未连接"，根因是 `HostPid`（本机为 `REG_QWORD`/8 字节）被按 4 字节读，教训见 [`2026-09-23-rc003-helper-hostpid-read-bug.md`](2026-09-23-rc003-helper-hostpid-read-bug.md)。
> - **三键捕获已在真机成立**（2026-09-23，`observe`，提权）：30 条 `[EDGE]`，返回 `0x00F1` / 音量+ `0x0080` / 音量− `0x0081` 各 5 次，`target_hits=15` 与按下数 1:1，`clears_ok=0`。证据 `evidence/2026-09-23-observe-run.log`。
>   同一次运行还暴露出**两处"安全上限失效"**（`--duration` 在有 agent 连接时永不到期；Ctrl+C 不发 `disarm`），已修并加自检回归，见 [`2026-09-23-rc003-observe-first-real-run.md`](2026-09-23-rc003-observe-first-real-run.md)。
> - **"第二次运行必崩"已定位并修**（2026-09-23）：注入成功后宿主**长期映射**运行时目录那份 `frida-gadget.dll`（本机宿主启动于 01:23、已运行 14.47 h），而无条件覆盖复制必然 `os error 32`。改为：先枚举宿主模块判断是否已有 tap → 令牌跨运行稳定（`session.token`）使**接管**成立 → 能接管就不碰文件并补发 `arm`/`mode`/`restore` → 接不上则 `[STALE-TAP]` + 清理步骤（退出码 13）→ 复制前按 SHA-256 复用 → 注入后用模块枚举核实 → `--await-hello` 超时给 `[NO-HELLO]`（退出码 12）。**本机留一次性迁移**：15:10 那次注入的令牌未持久化，属旧世代接不上，需清一次（断开重配对 RC003 / 设备管理器禁用再启用 / 重启）。见 [`2026-09-23-rc003-helper-rerun-blocked-by-locked-gadget.md`](2026-09-23-rc003-helper-rerun-blocked-by-locked-gadget.md)。
> - **反复运行验收已通过，接管路径首次真机成立**（2026-09-23 16:06–16:11，Andy 重配对 RC003 清掉旧世代后）：注入轮 `[PREP] action=reuse`（一个字节都没写）+ `how=injected`；**接管轮** `[TAP] resident=1` → `[ATTACH] skip_injection` → `[HELLO] auth=true` → 三键各一组 `[EDGE]` → `how=attached_existing_tap`。证据 `evidence/2026-09-23-acceptance-run.log`（未改动原文 `…-raw.log`）。
>   同一轮还暴露出 **6 处证据/鲁棒性缺陷**（每轮多一条运行分隔线、3 条并发连接 2 条被 RST、读失败导致的断线不计数、`discarded` 一物二用、接管轮误报 `sha256_verified=false`、`sent=false arm=true` 自相矛盾），已修 + 自检 34→38、agent 3→4（含「去掉修复必须 FAIL」的阳性对照）。见 [`2026-09-23-rc003-rerun-acceptance-verdict.md`](2026-09-23-rc003-rerun-acceptance-verdict.md)。
> - **发现一处判据缺口并已补上：RC003 上「拦截生效」本身无法观测。** 三键在 Windows 侧本来就零事件，所以「清掉」与「不清」外部完全看不出差别，传统判据「没有原生动作」**恒为真**——它无法区分「清空生效」与「清空根本没跑到」，`observe` 与 `run` 在现象上无法分辨。处置：助手新增 `--canary-usage <U16>`，入口 `run-helper-canary.cmd` 默认额外清**主页 `0x4A`**（Windows 本来能处理的键）。于是「运行前能跳行首 → 运行中毫无反应 → 结束后又能跳」三步就能**直接**证明「清空生效」与「fail-open」，不再靠推断。哨兵键只进清空集合、不进上报集合（`[EDGE]` 恒为三键），并有协议护栏：`report` 必须恰好等于三键、`clear` 必须为其超集，越界拒绝且不改动现有范围。自检 38→**42**，agent 自检台 4→**5**（新增用例 E + 「去掉护栏必须 FAIL」的阳性对照）。见 `hardware/RC003/README.md` §5.7。
> - **捕获链第 ② 段（传输）已接线完成**（2026-09-23）：接线前助手收到 `{"type":"edge"}` 只 `session.edges.push()` + 写日志，**全库没有向主程序转发的通道**——所以三键一直是"能配置、按下去没反应"。现改为**主程序监听随机端口 + 写 `%LOCALAPPDATA%\SayAll\rc003-bridge.ini`（port+token），提权助手读取后回连**（反向在权限上不成立：助手运行时目录在 `%ProgramData%`）。引擎**零改动**：边沿投 `EngineMessage::GateEdge`，与 RC001"被 `key_gate` 吞下的厂商键边沿"是同一条通道；不用 `HidUsages` 是因为它会整体替换引擎内 HID 按下集合、会把 RC001 的状态冲掉。fail-open 三层：助手收尾先释放再 `BYE` / 主程序 3 s 静默看门狗 / agent 2 s 租约。令牌**不是**安全边界（只防误连），纵深防御在**白名单**（只收三键 usage，其余丢弃计数）。验证：`rc003_bridge` 单测 9 项（含真 TCP 端到端、接管、看门狗）、助手自检 42→**47**、`cargo test --workspace` 全绿。见 [`2026-09-23-rc003-app-bridge-transport.md`](2026-09-23-rc003-app-bridge-transport.md)。
> - **验收手册已落盘**：`Testing/WindowsRC003EnhancedCapture.md`——E1–**E7** 的**命令、判据、日志字段、状态表**（含退出码表与常见失败处置）。E1（report 归属）与 E2 的锁屏场景、E4d、E5、E6 明确标为 `deferred` 并写明依赖；E2/E3/E4a/E4b/E4c/**E7** 标为「未执行，可直接做」。
> - **仍需 Andy 本人提权跑**（这是当前唯一阻塞真机进展的事）：**E7 桥接连通性**（主程序 + 助手同时运行 → 看 `[APP-BRIDGE] event=connected` 与 `[SUMMARY] app_bridge=connects=N edges=K`；**不需要设备按键，只需一次提权**，是当前性价比最高的验收点）；`run-helper-canary.cmd`（E2 + E4b/E4c 一起验：清空生效 → 主页无反应 → `[TIMEUP]` → 主页恢复）；`run-helper.cmd 120`（E3 物理键盘零影响）；E4a（运行中强杀助手 → ≤2 s 主页恢复）。
> - 待办与逐项状态以 [`../../TODO.md`](../../TODO.md) 的"为返回键、音量加、音量减完成按设备源头捕获"条目为准（已含十次更新）。
> - 相关文档：`2026-09-23-rc003-driverless-capture-investigation.md`、`2026-09-23-zstdjan-hid-host-tap-implementation-review.md`、`2026-09-23-rc003-hid-host-readonly-tap-result.md`、`2026-09-23-rc003-hid-host-write-tap-result.md`、`2026-09-23-rc003-helper-hostpid-read-bug.md`、`2026-09-23-rc003-observe-first-real-run.md`、`2026-09-23-rc003-helper-rerun-blocked-by-locked-gadget.md`、`2026-09-23-rc003-rerun-acceptance-verdict.md`、`2026-09-23-rc003-interception-unobservable-canary.md`、`2026-09-23-rc003-app-bridge-transport.md`；硬件取证见 [`../../hardware/RC003/README.md`](../../hardware/RC003/README.md)。

## 当前系统状态（接手前必读）

1. **常驻捕获器**在 console 会话运行（`Testing\investigation\remote-capture.ps1`，检查进程+日志 `Testing\investigation\remote-capture.log`）——**用户随时按遥控器按键即采集真机数据**（协议：`Testing\investigation\REMOTE-CAPTURE-PROTOCOL.md`，含首事件 7 项验证清单与 console 会话前提）；
2. **微信登录窗特意留置前台**（Weixin.exe 4.1.13.63，QR 码待用户扫码）——扫码后按 `evidence/n/FINDINGS.md` 任务 3 复跑协议测客户端语音（仅文件传输助手）；
3. 默认录音设备=Realtek 麦克风（已恢复）；活动输入法=豆包（已恢复）；豆包快捷键=出厂右 Alt（config 已还原并验证）；
4. 机器锁协议：`Testing\investigation\machine-lock-protocol.md`（审计日志制，敏感实验必守）。

## 关键文档

| 文档 | 内容 |
|---|---|
| `2026-09-04-avoid-driver-signing-input-paths-final.md` | **最终报告**（先读这个） |
| `2026-09-04-avoid-driver-signing-input-paths.md` | 工作文档（全部轮次细节、路线图前提、边界对照表、Win+H 终验规格、环境事实） |
| `Bugs/2026-09-04-doubao-voice-hold-hotkey.md` | 豆包/WeType 全案记录（含勘误体系） |
| `docs/decisions/0002-dual-track-injection-optional-helper.md` | 双轨架构 ADR（增强轨待修宪决策） |
| `Testing/WindowsRC003Preview.md` | 产品真机测试手册 |

## 后续工作（按优先级）

1. **真机遥控器采集**（用户按键即可，零准备）：数据到手后回答 usage 表差异/0x00F1/时序窗参数 → 直接喂给吞键层实现与真机验收；
2. **WeType 产品化收尾**（天级）：连接页 UI 引导（活动输入法确认+按住 ≥0.5s 提示）、快按无文本的 UX 披露、真机遥控器端到端验收（注入段已实现+音频段已 E2E，只差遥控器本体）；
3. **Win+H 健康主机终验**（五步序列已备）：任何已激活+语音功能完整的机器；
4. **吞键层实现**（2-4 周）：公式+工程细节齐备（工作文档 C 交付节+路线图），真机数据为输入；
5. **修宪决策**（用户）：是否启动 WinUHid 增强轨（豆包唯一路径，4-8 周+OV 证书）；澄清 A2"基础 vs 增强"边界；
6. **微信客户端复测**（用户扫码后，协议已备）；STT 路线若立项先解决延迟（流式/SenseVoice）。

## 环境陷阱速查（历史教训，全部实证）

PS 5.1 无 BOM UTF-8 脚本按 GBK 读（别在脚本写中文/计数用字节级）；INPUT 结构必须 40 字节（32B 会静默失败）；TSF 激活必须 FORSESSION=0x20000000（dwFlags=0 静默无效）；活动 IME 判定用候选框行为（非 S_OK/HKL/WTSB）；OCR 用 `dim ocr recognize`（视觉模型未配置）；焦点用 SetWindowPos+点击；VB-CABLE 锁 16kHz；豆包 ASR 需网络；锁协议必守（并行实验互相污染有实证）。
