# 最终调查报告：避开 Windows 驱动签名实现 RC001/RC003 语音输入与自定义按键

- 日期：2026-09-04
- 协议：deep-investigate（5 轮 × 15 探索代理 + 5 验证者，每轮独立验证门，R5 终审 PASS）
- 核心问题：在不使用需要 Microsoft 驱动签名的虚拟 HID 驱动的前提下，Windows 上还有哪些可行方案，能让 RC001/RC003 遥控器实现（1）唤醒输入法语音输入、（2）无重复输入的自定义按键映射？各方案的可行性边界与推荐顺序是什么？
- 范围声明：用户明确"忽略 AGENTS.md 所有限制"（管理员、Frida、进程注入、读第三方私有配置/内存、驱动含测试签名全部纳入调查范围）；最终报告标注各路线与原边界的关系，供修宪决策。
- 工作文档（全部轮次细节）：`2026-09-04-avoid-driver-signing-input-paths.md`；证据库：`evidence/{e,f,g,h,i,j,k,l,n}/`

---

## 1. 执行摘要

**答案：可以完全避开驱动签名实现语音输入（针对微信输入法与 Windows 系统听写）与自定义按键映射；豆包输入法是唯一注入死角，其唯一路径是虚拟键盘驱动增强轨——而该驱动的签名成本经查证远低于预期（普通 OV 证书，无需 EV/硬件开发者计划）。**

### 推荐路线图（按优先级）

| 优先级 | 路线 | 状态 | 前提 | 工作量 |
|---|---|---|---|---|
| **1** | **微信输入法 WeType 注入**（免驱动免管理员纯 SendInput）：遥控器语音键按住 → 注入 Ctrl+Win → WeType 语音 → ATVV 音频经 CABLE → 云端 ASR → 文字上屏 | **本机 E2E 终验通过**（19 字精确转写 ×2；六项复测全 passed） | WeType 为目标文本框活动输入法；按住 ≥500ms（快按无文本，UX 披露） | **天级**（注入功能已实现，剩 UI 引导+文档+真机遥控器验收） |
| **2** | **Win+H 系统语音输入**（免驱动）：注入唤醒/生命周期（Esc 停止）/麦克风路由设备层全部实证 | 唤醒 passed；**语音服务层需健康主机终验**（本机 Windows 未激活） | 已激活+受支持 OS+语音功能完整；焦点在文本域 | **天级，依赖主机**（五步终验序列已备） |
| **3** | **自定义按键免驱动吞键**：Raw Input 武装 + LL 钩子时序窗吞原键 + SendInput 注入映射键 | 公式+机理（本机六组对照）+双先例（Voice_VibeCoding 全套工程细节）齐备 | 真机遥控器报文参数（采集器常驻待用户按键） | **2-4 周** |
| **4** | **自带 STT**（whisper-rs）：完全绕开 IME 战场的独立路线 | 三档实测：small 0% 错误率（慢）、base 可用（繁体）、tiny 判死 | 流式/分块解码或更快中文模型（延迟头号风险）；目标机 CPU 基线 | **2-6 周** |
| **5** | **豆包 WinUHid 增强轨**（唯一死角唯一路径）：虚拟键盘驱动注入（事件=物理，无注入标志） | 注入规格静态还原齐备（E0 38 按住 ≥150ms 成对）；签名政策闭合 | ADR 0002 修宪决策；OV catalog 签名（~$70-180/年档）；装机实测 | **4-8 周 + 证书成本** |

### 对用户最直接的结论

之前"微信输入法 左Ctrl+左Win 无效"的原因查明：**微信输入法当时不是目标文本框的活动输入法**（TSF 激活陷阱，调查中 F 代理踩了同一个坑被 J 翻案）。现在可用的完整路径：目标应用文本框内切到微信输入法（任务栏指示器确认）→ 按住遥控器语音键 ≥0.5 秒 → 文字上屏（本机已用等价音频端到端验证）。

---

## 2. 调查方法

deep-investigate 多代理循环协议：每轮并行探索代理 → 协调者合并 → 新验证者审计（审计者不规划），GAPS 则继续、PASS 则出循环（min_rounds=5）。全程实际执行观察（命令+输出+截图 OCR+注册表+反汇编落盘），遵循仓库验证词汇纪律（passed/failed/deferred），[UNVERIFIED] 惯例贯穿。关键方法论事件：Round 1 探针 32 字节 INPUT 结构 bug 自我勘误、Round 3 WeType 判决翻案（F 的假象被 J 复现并推翻）、Round 4-5 两起测量争议经字节级仲裁（enableGlobalVoiceShortcut 悬案=时区换算误报；"234 行"=GBK 吞行假象）。

## 3. 主要发现

### 3.1 目标注入行为矩阵（本机实证）

| 目标 | 物理键 | SendInput 注入 | 机理 |
|---|---|---|---|
| **豆包 0.8.2.7** | ✅ passed | ❌ **failed（四层闭环）** | ImeService 全局 LL 钩子（VoiceKeyHookProc）首查 LLKHF_INJECTED 命中即透传丢弃（字节级逆向+独立复现）；RegisterHotKey 路线 7 状态矩阵 failed（该开关放宽的是前台激活而非热键注册）；提权注入无效（只查 0x10 不查 0x02） |
| **微信 WeType 2.1.3.18** | ✅ passed | ✅ **passed（翻案后）** | 不检查注入标志（与豆包决定性对照）；Ctrl+Win 按住触发（纯 VK/扫描码均可）；吞掉注入的 Win、自注入 break key（"WTYP"）；释放 ≈117ms 关麦，连按 20/20 成对 |
| **Windows 语音输入 Win+H** | — | ✅ 唤醒 passed（per-event 序列，豆包激活态也有效） | 系统级功能；本机服务层因 Windows 未激活失败（环境缺陷非路线缺陷） |
| 微信客户端 4.1.13.63 | — | deferred | QR 登录墙（登录窗留置，复跑协议已备）；官方热键同为 Ctrl+Win |
| 记事本/通用应用 | — | ✅ passed（Alt+Space 系统菜单等） | — |
| 搜狗/讯飞/Discord/游戏 | — | 未装 | 安装测试协议已备（target-matrix.md） |

### 3.2 免驱动听写链路本机终验（E2E）

Huihui TTS 16kHz WAV → CABLE Input（wasapi，与产品 audio.rs 同构）→ IPolicyConfig 切默认录音设备到 CABLE Output → WeType Ctrl+Win 按住注入 → 云端 ASR → 记事本转写「今天天气很好，我们一起去公园散步聊天吧」19 字精确（OCR + WM_GETTEXT 双证，两次）。**分段实证+拼接论证**：注入段用 PS SendInput 模拟（与产品 Rust SendInput 同 API 同结构），遥控器端到端仍属真机 deferred。VB-CABLE 管道实测锁 16kHz（48k 写入=静音）——ATVV 16kHz PCM 天然匹配。

### 3.3 免驱动自定义按键（无重复输入）

公式：**Raw Input（设备识别/武装）+ WH_KEYBOARD_LL（时序窗吞原键）+ SendInput（注入映射键）**——免管理员。本机六组对照实验证实机理（钩子先于 Raw Input、吞掉事件 Raw 不可见、提权前台窗口=双失效盲区、回调超时静默卸钩）。Voice_VibeCoding 源码拆解提供全套工程细节（时序窗参数、链头 bump 重叠安装、F5 防粘键状态机、音量防双格、已踩坑清单 10+ 条）。真机报文参数（RC001/RC003 usage 表差异、返回键 0x00F1、时序窗实测分布）待采集——常驻捕获器（console 会话）+协议+首事件验证清单（归因路径经静态审查+11/11 离线单测去风险）全部就绪。

### 3.4 豆包增强轨（WinUHid）：规格与成本

- **启动条件（静态还原）**：VK_RMENU 独按匹配（方案字符串 right_alt/right_alt_space/left_ctrl_win）；hold 阈值 150ms（音频 keydown 即预启动，<150ms 释放=取消）；前置=豆包活动 IME（IsImeForegroundActive）+**网络在线（云端 Sami ASR 硬前置）**+快捷键无冲突+设置页不聚焦；
- **注入规格**：E0 38 ↓ → 按住 ≥150ms（建议 200ms）→ ↑ 成对；期间不得有其他按键；
- **签名成本（路线图级修正）**：微软门户强制签名**仅内核态**（官方原文）；**UMDF 用户态驱动只需普通 OV Authenticode catalog 签名（~$70-180/年档，开源项目可用 Certum 更低），无需 EV 证书与硬件开发者计划**；libvirtualhid 为 Trusted Signing 分发 x64 UMDF 的活先例（但其 Windows 键盘许可阻断产品使用）；WinUHid（MIT）为首选来源，需自建+审计。

### 3.5 自带 STT（whisper-rs 实测）

| 模型 | 体积 | 中文 CER | RTF（i3-8130U 参照下限） | 判定 |
|---|---|---|---|---|
| tiny-q5_1 | 31 MiB | 33.3%（同音字灾难） | 2.3× | 判死 |
| base-q5_1 | 57 MiB | 9.5%（输出繁体需转换） | 6.4× | 最低可用线 |
| small-q5_1 | 181 MiB | **0%** | 22.4× | 质量完美/弱机太慢 |

工程事实：必须 set_language("zh")（短音频自动检测误判英语）；ATVV 16k 对齐正向确认（48k 不重采样=幻觉循环）；构建摩擦可行（cmake+libclang，exe 2.8MiB）；**延迟是头号产品风险**（松开语音键等全文=RTF×时长，base@本机≈44s）——需流式/分块解码或 SenseVoice-small 类更快中文模型。

### 3.6 机制层发现（判死路线集与通用教训）

- **LL 钩子修改不跨钩子传播**（三层实证：私有结构副本/CallNextHookEx 转发通道不存在/应用层收到原始键）——physicalize 机制级死刑；ZSTDJan 该技巧实为结构性 no-op（真正有效的是其未接线的 Frida 版）；
- 注入 API ground truth：SendInput/keybd_event/UNICODE/InputInjector 全带 LLKHF_INJECTED（InputInjector 另有伪设备句柄但对豆包无效）；PostMessage 不入系统输入流；**JOURNALPLAYBACK 本机封死**（ACCESS_DENIED，提权+原生 DLL 均试）；
- 豆包/WeType 运行时 UI 均无 UIA provider（任何完整性级别）——UIA 点击路线判死；TextInputHost 同；
- **TSF 激活陷阱**：ActivateProfile dwFlags=0 是线程级、静默无效——会话级必须 TF_IPPMF_FORSESSION=0x20000000；活动输入法判定须用行为判据（候选框版式）而非 S_OK/HKL/状态栏可见性（WTSB 滞后）；
- Win+H 生命周期：toggle 语义；Esc tap 停止且幂等（兜底安全）；听写条静默自动收起（~2 分钟，状态机需补偿）。

## 4. deferred 清单（六项，全部带规格非裸 deferred）

1. **真机遥控器采集**（路线 1/3 共用前置）：常驻捕获器 console 会话运行中，用户随时按键即完成；协议+首事件 7 项验证清单就绪（`Testing/investigation/REMOTE-CAPTURE-PROTOCOL.md`）；
2. **Win+H 健康主机终验**：主机规格（已激活+受支持 OS+语音功能判据）+五步序列（`工作文档"Win+H 终验主机规格与流程"节`）；
3. **WinUHid 装机实测**：受调查护栏（不装驱动）限制；注入规格+签名政策文档就绪；
4. **K 物理键实验 Exp K-1/2/4/5**：设计在 `evidence/k/doubao-voice-start-conditions.md` §8；
5. **STT 真实音频 CER 与目标机 RTF**：单句干净 TTS=质量乐观上界，真实 ATVV 远场麦克风未测；
6. **微信客户端扫码复测**：登录窗特意留置前台，复跑协议已备（文件传输助手→Ctrl+Win→ConsentStore+OCR）。

## 5. 与原 AGENTS.md 边界的关系（修宪决策点）

| 路线 | 边界状态 |
|---|---|
| WeType/Win+H/吞键/SendInput/自带 STT | **零触碰**（全部公开 API） |
| WinUHid 增强轨 | A2 字面触碰（"基础语音路径不得依赖虚拟 HID"）——ADR 0002 双轨语义可辩护（增强轨 Helper、失败不影响基础路径），需修宪澄清"基础 vs 增强"边界定义 |
| 改豆包私有配置 / Frida 注入 | 已判死或调查-only（Frida 三重触碰 A2/A4/A5，建议永久定位为研究） |

WeType >500ms 门限与语音键条款对齐结论：门限是目标 IME 自身行为（非我们添加的阈值），注入镜像物理按住时长即合规；采用披露+UI 提示方案；否决"快按补最短保持"（会延迟语音生命周期，违反条款）。

## 6. 盲点与教训

- 用户原测试与 F 代理踩了同一个 TSF 激活陷阱——IME 行为测试必须会话级激活+行为判据双确认；
- PS 5.1 无 BOM UTF-8 按 GBK 读会吞行（本轮两起测量争议均源于此）——跨工具计数须用字节级方法；
- 并行代理同机实验互相污染（Round 2 教训）——锁协议已升级为审计日志制；
- 32 字节 INPUT 结构 bug 曾使一轮实验全部无效——注入类实验必须校验 SendInput 返回值；
- 微信输入法云 ASR 丢句尾句号（产品文案需预期）；HF 免按和弦非默认 [UNVERIFIED]。

## 7. 来源与证据索引

- 证据库：`docs/investigations/evidence/{e,f,g,h,i,j,k,l,n}/`（豆包生死实验/Win+H+WeType 初裁/逆向复现+签名+STT 调研/基线恢复/文档+环境/WeType 翻案+目标矩阵/豆包启动条件/whisper 实测/WeType 复测+E2E）
- 关键外部来源：cgutman/WinUHid（MIT）、mwlt/Voice_VibeCoding、ZSTDJan/windows-remote-mic-app（拆解）、微软官方签名政策三页（evidence/g/signing-policy.md 含原文）、libvirtualhid、whisper.cpp/whisper-rs
- 仓库文档同步：Bugs/2026-09-04-doubao-voice-hold-hotkey.md（全案记录+勘误体系）、ATTRIBUTION.md（physicalize 判死+WeType 配方实证+WinUHid 终态）、ADR 0002（双轨架构）、边界对照表（工作文档）
