# 调查：避开 Windows 驱动签名实现 RC001 语音输入与自定义按键

- 状态：进行中（deep-investigate 协议）
- 日期：2026-09-04
- min_rounds：5
- 范围变更：用户明确"忽略所有限制"——管理员权限、Frida、进程注入、读取第三方私有配置/内存、驱动（含测试签名）全部纳入调查范围。最终报告标注各路线与原 AGENTS.md 边界的关系，供修宪决策。

## 核心问题

在不使用需要 Microsoft 驱动签名的虚拟 HID 驱动的前提下，Windows 上还有哪些可行方案，能让 RC001/RC003 遥控器实现（1）唤醒输入法语音输入、（2）无重复输入的自定义按键映射？各方案的可行性边界与推荐顺序是什么？

## 已确证事实（本会话实证；Round 1 勘误 + Round 3 翻案/K/L/I 新增吸收）

> 阅读说明：条目 1-10 为 Round 1 版事实（保留原文与当时勘误标注）；条目 11-16 为 Round 3 起吸收的现行结论（J 翻案、K 豆包启动条件、L whisper 实测、I 环境修正、physicalize 判死、ZSTDJan Frida 定位）。两者冲突时以 11-16 与各轮交付节为准。

1. SendInput（扫描码 0x38+扩展键，VK_RMENU）注入在系统层真实到达：GetAsyncKeyState 探针 7ms 内观察到 DOWN、保持、UP（Testing/probe-right-alt.ps1）。⚠️ 勘误：本会话部分早期探针（probe-doubao-voice/round2/round3/driverless）有 32B INPUT 结构 bug（正确为 40B），其"豆包无反应"截图当时未注入任何事件；A 在验证豆包激活态下用正确结构复测，结论不变。
2. 豆包输入法：物理按键可唤起语音（用户实证）；SendInput/InputInjector 注入不唤起（A 激活态复测 + D 逆向 LLKHF_INJECTED 检查闭环）。**勘误：设置页有快捷键配置项**（LongPressShortcutBox/HandsFreeShortcutBox，默认"未设置"=出厂右 Alt），且可 UIA 程序化配置（B 实证配置成功但注入触发仍被过滤）。
3. 豆包 0.8.2.7（C:\Program Files\DoubaoIME\）：进程 ImeService（uiAccess 提权）/ImeWatchdog；麦克风已设 CABLE Output；私有配置 %APPDATA%\DoubaoIme\conf\config.json 含 enableGlobalVoiceShortcut=false。**勘误：微信输入法 WeType 2.1.3.18 实际已安装**（B 实证：四进程常驻、含 voice_engine）。
4. InputInjector 注入带 LLKHF_INJECTED（丢 scancode、raw 层有伪设备句柄）且对豆包无效（A ground truth + 激活态复测）。
5. **Win+H sent=0 已解决**（结构体 bug）；正确 per-event 注入（LWin↓→80ms→H↓→60ms→H↑→60ms→LWin↑）可唤起 Windows 语音输入条（OCR 实证"正在聆听"），豆包激活态也有效；~~但停止序列与麦克风路由未验证（路线三件套未闭合）~~→【R2 F 更新】生命周期已闭合（UP=Esc tap、断连/睡眠兜底=Esc tap 幂等）、麦克风路由设备+链路层已打通（IPolicyConfig 切 CABLE Output + VB-CABLE 16kHz 直通）；语音服务层因本机未激活 blocked-on-env（终验规格见"Win+H 终验主机规格与流程"节）。
6. WinUHid（cgutman，MIT）：无预编译 Release，需自建+签名；"UMDF 只需 OV Authenticode"结论待证据链闭合（外部核查方向支持但官方口径含混，Round 2 落盘）。
7. 参考实现：ZSTDJan（管理员仅用于 Frida WUDFHost tap=HID 采集旁路；~~含 physicalize 技巧=自家钩子清注入标记转发~~→该技巧【R2 E 机制级 + R3 J 语义复查】**结构性无效**，见条目 15）、Voice_VibeCoding（LL 钩子吞键免管理员 + WinUHid 注入优先 SendInput 降级；已踩坑清单完整）。
8. 本仓库现有能力：Raw Input 设备路径过滤观察、SendInput 批量注入（含和弦 DOWN/UP 边沿）、ATVV 语音会话管线完整。
9. 豆包 UI 锚点：OimeDirectUIWindow（激活判据）、**OimeVoiceWaveWindow（语音会话 Win32 级探测器）**；TSF 程序化切换输入法可用（ITfInputProcessorProfileMgr，GUID 已验证）；活动输入法判定必须用 TSF 查询而非 HKL。
10. JOURNALPLAYBACK 本机封死（ACCESS_DENIED，提权+原生 DLL 均试）；PostMessage 不入系统输入流；豆包运行时 UI 无 UIA provider（任何完整性级别，提权也读不到）。
11. **WeType（微信输入法 2.1.3.18）注入有效（Round 3 J 翻案，passed）**：前提=WeType 为目标文本框的**会话级**活动输入法（TSF `ActivateProfile` 必须带 `TF_IPPMF_FORSESSION=0x20000000`；dwFlags=0 仅线程级=F 假象根因）。该前提下 Ctrl+Win 按住两配方（纯 VK 80ms 间隔 / 标准扫描码）均触发语音会话——ConsentStore mic start（06:45:35/06:45:40）+ WeType 吞掉注入的 LWin + 自注入 0xFC break key（extra="WTYP"）三证；WeType **不检查 LLKHF_INJECTED**（与豆包决定性对照）；语音功能已启用、无需登录；释放后 mic 干净关闭（~0.5s 停止尾）。自带 >500ms 长按门限（快按取消预录音，二进制字符串 `kLongPressEnd: pre-recording cancelled (< 500ms)`）。遗留复测（R4 N 进行中）：快速连按、释放停止延迟精确值、HF 和弦默认值、麦克风路由 CABLE Output、ATVV 端到端。
12. **豆包语音键完整启动条件（Round 3 K 静态还原，9 项 [UNVERIFIED]）**：右 Alt（VK 级 VK_RMENU、必须独按）DOWN **当即预启动音频**（不等阈值）；**150ms** hold 阈值只管波形窗出现与轻点/长按分类——释放 <150ms → PRESS_CANCEL 丢弃、≥150ms → PRESS_STOP 提交；环境前置=豆包为焦点进程的活动输入法（IsPidImeActive）+ **网络在线**（云端 Sami ASR 硬前置，断连直接拒绝）+ 无快捷键冲突 + 设置页不聚焦（聚焦=钩子整体旁路）+ 麦克风选定 CABLE Output；文本域焦点不是启动条件。WinUHid 注入规格：E0 38 ↓→保持 ≥150ms（建议 ≥200ms）→↑ 成对、期间不得有任何其他按键（evidence/k/ §7）。
13. **whisper-rs 三档实测（Round 3 L，本机 i3-8130U 2C/4T，6.8s 干净 TTS 中文音频）**：tiny-q5_1 CER 33.3%（同音字灾难，中文判死）/ base-q5_1 CER 9.5%（输出繁体，t2s 归一化后=最低可用线）/ small-q5_1 CER 0%（质量完美但 RTF 22.43 本机太慢）。延迟=RTF×时长（base 6.8s≈44s 出字）——产品化必须流式/分块解码或更快中文模型（SenseVoice-small/sherpa-onnx 为 R4 头号候选）；语言自动检测误判英语（tiny/base auto 各一次，runs/tiny-auto.txt、base-auto.txt），必须 set_language("zh")；真实 ATVV 远场麦克风音频未测（deferred）。
14. **本机实为 Windows 10 Pro 19041.207（2004 RTM 未打补丁）+ 未激活（LicenseStatus=5）**（Round 3 I 实测，evidence/i/env-facts-check.txt）——此前多份报告误称"Windows 11"，一切 Win11-only 行为（Windows AI Speech 等）本机不可测；Win+H 失败根因拆分（未激活 vs 语音包形态：OneCore 下实有 zh-CN token MS-2052-110-WINMO-DNN 而 Win+H 仍失败，"语音包缺失"归因存疑）留给健康主机终验（见环境备忘）。
15. **physicalize 结构性无效（Round 2 E 机制级 + Round 3 J 语义复查，定论）**：LL 钩子事件数据不跨钩子传播（E 三层实证：每钩子私有结构副本 / CallNextHookEx 转发通道不存在 / 应用层收原始键），清注入标志只影响自家钩子副本——ZSTDJan 该技巧（legacy_key_suppressor_windows.py L142-155）对下游钩子/应用为 no-op；该候选已从路线图删除。
16. **ZSTDJan 真正能影响豆包的方案=Frida 版**（doubao_rpc.py attach ImeService 清 LLKHF_INJECTED，未接线进生产）——A2/A4/A5 三重触碰，仅调查定位、不作产品路径（路线表 #8）；豆包的产品化候选仅剩 WinUHid HID 设备层（无 INJECTED 标志，K §7 注入规格；装机实测 deferred）。

## 维度

1. 用户态注入 API 全谱系 × IME 过滤矩阵（SendInput 变体、InputInjector、PostMessage、JournalHooks；豆包/微信/搜狗/Win+H/游戏 PTT）
2. UIA/辅助功能路线（点击 IME 语音按钮；暴露度与按住语义映射）
3. 免驱动吞键（LL hook + Raw Input 时序窗）+ 参考实现拆解（哪些能力实际不需要驱动/管理员）
4. 替代语音管线（Win+H 接受注入与否；其他 IME；自带 STT 边界）
5. 驱动签名成本复核（2026 attestation 政策）+ 测试签名自用路径
6. 逆向/注入路线（Frida/进程内直接调用 IME 语音触发/读私有配置与内存理解过滤逻辑）
7. 各路线与原 AGENTS.md 边界的关系标注（信息性）

## 轮次记录

### Round 1（完成）

四个探索代理（A 注入实证 / B UIA / C 参考实现拆解 / D 逆向+签名）全部交付。

**验证者 R1 判定：GAPS**（11 项问题，按严重度）：
- 严重①：真机硬件维度整体缺失——Round 2 候选无真机项；吞键公式未经 RC001/RC003 真实报文验证；usage 表差异、时序窗参数对齐未验；
- 严重②：Win+H"已确认可行"撑不住——三件套标准（唤醒+音频路由+生命周期）未统一；toggle vs 按住语义、麦克风能否路由 CABLE Output 未验；
- 严重③：D 的核心结论（RVA/反汇编/config 键名/UMDF+OV）零证据工件零来源链接；B 的 UIA/TSF 同样基本无独立工件——不可复查；
- 严重④：微信过滤结论矛盾未被识别（Bugs 文档断言"两家 IME 均过滤" vs D 二手证据"微信不过滤"）；
- 中等⑤：IME 过滤矩阵实际只有豆包一列（搜狗/讯飞/游戏 PTT 零接触）；
- 中等⑥：physicalize 链序前提未验证（豆包 watchdog 重挂、链头竞争、提权盲区波及未评估）；
- 中等⑦：基线污染未恢复（豆包快捷键残留 Ctrl+Shift+D、InputMethodOverride 被删、遗留记事本）；
- 中等⑧：工作文档事实区 5+ 处过时（本轮已修正）；
- 较轻⑨：libvirtualhid（UMDF2+VHF，经 Store 分发）未入方案空间；
- 较轻⑩：维度 7 边界对照表未建立，ATTRIBUTION 未同步 Round 1 新结论；
- 较轻⑪：自带 STT 零覆盖。

#### Explorer D 关键发现（Round 1）

**1. 豆包过滤机制已静态实锤（逆向证据）**：
- `ImeService.exe`（uiAccess=true，High IL）安装全局 `WH_KEYBOARD_LL` 钩子（hookproc RVA 0x7426C0）；
- 回调第一件事：`test byte ptr [lParam+8], 0x10`（LLKHF_INJECTED），命中即 `CallNextHookEx` 纯透传——注入事件在进入任何按键逻辑（含语音状态机）之前被丢弃；
- **只测 0x10，不测 0x02（LOWER_IL）——提权注入也无效**；
- 全部 14 个二进制无 Raw Input（RegisterRawInputDevices=0）；tsf-oime-core.dll 的 SetWindowsHookExW 是 WH_GETMESSAGE（内部消息），非键盘钩子。

**2. 豆包存在两条候选非注入路径**：
- 状态栏 `btn_voice` 语音按钮（窗口类 `OimeDirectUIWindow`，`window_status_bar`）——UIA 点击候选；
- 配置 `%APPDATA%\DoubaoIme\conf\config.json`：`voice.enableVoiceShortcut=true`、**`enableGlobalVoiceShortcut=false`**、`voiceShortcut/voiceLongPressShortcut {keyCode:0=默认右Alt}`——语音快捷键其实可配置；`enableGlobalVoiceShortcut=true` 时可能走 `RegisterHotKey(id=0x2E55)`（RegisterHotKey 理论上响应注入事件！）——一行配置的差别可能直接解决注入问题。

**3. ZSTDJan 的 Frida 真相**：注入 WUDFHost 做 HID 按键采集（补 Raw Input 丢失的返回/音量键），不是注入过滤旁路；它同样没解决豆包。

**4. 微信输入法**：官方入口=状态栏语音按钮（ZSTDJan 用 UIA 点击 `wetype.statusbar.window`）+ SendInput 兜底（二手证据：微信不过滤注入，无公开反证）[UNVERIFIED：本机未装]。

**5. 签名成本修正（路线图级）**：
- 微软门户签名强制**仅内核态**（1607+ 官方原文）；**UMDF 用户态驱动只需普通 Authenticode（OV 证书 ~$70-180/年，开源项目可用 Certum 更低）**——WinUHid 是 UMDF → 自建 WinUHid 分发可能完全绕开硬件计划与 EV；
- 硬件计划注册免费；SSL.com 有个人（sole-proprietor）EV 证书 $200-349/年；attestation 签名不需 HLK；
- 测试签名自用：bcdedit /set testsigning on（需关 Secure Boot）可逆，迁移路径干净。

**6. Frida 对豆包的技术可行性（研究用，非产品路径）**：attach ImeService 后在 hookproc onEnter 清 flags（`args[2].add(8).writeU8(0)`）——单点改写，可行性高；无 anti-Frida 痕迹；风险=回调延迟→LowLevelHooksTimeout 摘钩→豆包语音整体失效；自动更新换版本 RVA 失效。

**7. D 推荐的后续实验（Round 2 候选）**：
① UIA/坐标点击豆包状态栏 btn_voice（验证点击语义 toggle vs hold）；
② 改 config.json `enableGlobalVoiceShortcut=true` 后重试 SendInput（若走 RegisterHotKey 则注入直接通过）；
③ Frida 一次性验证（闭环证明机制）；
④ WinUHid 自建+测试签名验证虚拟 HID 右 Alt 必过（kbdclass 无 INJECTED 标志）。

#### Explorer C 关键发现（Round 1）

**1. 免驱动免管理员的"自定义按键无重复输入"可行，公式确认**：
`Raw Input（设备识别/武装）+ WH_KEYBOARD_LL（时序窗吞原键）+ SendInput（注入映射键）`
两个真机验证先例：Voice_VibeCoding（v1.6.7 已发布）与 ZSTDJan（真机候选版）。

**2. 本机 6 组对照实验（实证机理）**：
- LL 钩子先于 Raw Input 调用（钩子先决策）；被钩子吞掉的事件 Raw Input 收不到；
- 同进程 SendInput 到自己窗口走快速路径跳过钩子；
- **提权窗口为前台时，中完整性钩子完全不被调用（观测+吞键双失效）**；中完整性 SendInput 不达提权窗口（UIPI）；
- 钩子回调慢（350ms×5 次）→ 被系统静默卸钩（LowLevelHooksTimeout 默认 300ms）；
- 本机有后台软件注入杂散键（F13/RAlt/A）→ 吞键判定必须排除 INJECTED/自家 EXTRA_INFO 标记。

**3. ZSTDJan 管理员依赖的唯一来源**：Frida 注入 WUDFHost 的 HID tap——只为返回键（usage 0x00F1，Windows kbdhid 不翻译、Raw Input 拿不到）。其余全部免管理员。RC003 音量路径未真机验证。

**4. Voice_VibeCoding 完整拆解（吞键工程细节）**：
- 自家注入放行：EXTRA_INFO('XMIR') 或 INJECTED 标志 → CallNextHookEx；
- 时序窗吞键：音量 recent 200ms、back/home/menu/tv/power 250ms、方向/OK 200ms 或 tap_ready+自定义位图（代价：软件运行期真键盘该键暂不可用，需披露）；
- **钩子链头 bump（重叠安装：先挂新再卸旧，消除空窗）**——LL return 1 无法撤回链头应用已看到的键；
- F5 语音键全套状态机：sticky/correlate 120ms/tail 3s/UP 配对语义（DOWN 漏进 OS 则 UP 必放行，防粘键）；
- 音量防双格：Tap 转发 + SendInput VK_VOLUME_* + 200ms 吞固件残留；
- Alt 和弦用 SendMessageTimeoutW 直发前台（避免系统菜单）；方向/OK 自定义映射强制 SendInput（WinUHid 事件会被自家钩子误吞）；
- **已踩坑清单**（bump 空窗、sticky 粘键、60ms 去抖门连点、WinUHid 分步松开等 10+ 条，docs/ 有完整记录）。

**5. ZSTDJan 的 physicalize 技巧（对豆包的免驱动候选！）**：`win32_input.py` L142-155——自家 LL 钩子对"带自家标记的注入 RAlt"**清除 INJECTED 标志后转发**，让下游应用钩子当物理键。**依赖自家钩子在目标应用钩子之前（链头）**。注意：ZSTDJan 仍把豆包列为未验证——此技巧对豆包实际有效性需 Round 2 实证（D 的逆向证明豆包只查 flags 0x10，机制上应可穿透）。

**6. 失败模式清单（免驱动重映射边界）**：提权/安全桌面窗口、钩子超时静默卸钩、链头竞争、DirectInput/反作弊游戏、时序竞态（DOWN/UP 配对）、typematic、多键盘 VK 碰撞、60ms 有界等待延迟。

**7. 对 SayAll 的落地方案草图**（C 给出四层分层 + 现有代码落点）：识别/武装层（raw_input_windows.rs 补 arm 出口）→ 吞键层（新增，专职线程+有界等待+bump+存活自检）→ 注入层（send_input_windows.rs 增强：extraInfo 标记、修饰键扫描码、Alt 和弦 SendMessage、physicalize）→ 产品边界声明（提权窗口/安全桌面/DirectInput 游戏为盲区；RC003 返回键免管理员拿不到）。

**8. Round 2 候选实验**（C 提出）：
- **physicalize 实证**：装自家 LL 钩子清 INJECTED 标志 + SendInput 右 Alt → 豆包是否唤起（免驱动免管理员的豆包路线！）；
- HID 报告特征（0x2A4D）WinRT 订阅作为免 Frida 的"钩子前"直接信号源（30 分钟实验，若可行可消除 60ms 等待）；
- RC001/RC003 usage 表差异真机对照。

#### Explorer A 关键发现（Round 1）

**1. 重大利好：Win+H 免驱动系统语音路线成立（passed，本机实证）**：
- per-event SendInput（40 字节正确结构）：`LWin↓→80ms→H↓→60ms→H↑→60ms→LWin↑`，全部 sent=1；
- OCR 证据：语音输入条"正在聆听..."出现（mx-06-winh-per.png）；**豆包为活动输入法时同样有效**（mx5-01-doubao-after.png）；
- 前提：焦点在文本域（否则提示"请选择文本域"）；批量单次调用 vs per-event 的差异需复核（建议 per-event 带间隔）；
- 待验证：Win+H 的停止序列（Esc vs 再按）、它用哪个麦克风（能否喂 CABLE Output 路由 ATVV 音频）。

**2. 勘误：此前探针的 INPUT 结构体 bug（32B≠40B）**：
- probe-doubao-voice/round2/round3/driverless 的 C# union 只含 KEYBDINPUT → sizeof(INPUT)=32 → SendInput 静默返回 0，**那几轮"豆包无反应"的截图证据无效（当时没注入任何事件）**；hookmon 日志为空同因；
- 但"豆包过滤注入"结论不受影响：A 在**已验证豆包激活**（OimeDirectUIWindow 可见判据）状态下用正确结构复测：纯 wVk / scan+ext / InputInjector 三机制 RightAlt 按住均无反应（OimeVoiceWaveWindow 始终隐藏）；
- 仓库 Rust 端结构一直正确（19 次会话 + 7ms 探针）。

**3. 全机制注入标记 ground truth（自建 LL+Raw 监视器）**：
- SendInput（wVk 或 scan+ext）、keybd_event、KEYEVENTF_UNICODE：LL flags 均带 LLKHF_INJECTED(0x10)；SendInput raw hDevice=0；
- **InputInjector：同样带 0x10 + 丢 scancode，但 raw 层有非 NULL 伪设备句柄**（对豆包仍无效——豆包不看 raw 句柄）；
- PostMessage 直发：LL/Raw 均不可见，前台弱反应；
- **WH_JOURNALPLAYBACK 本机封死**：ACCESS_DENIED（原生 EXE/DLL/提权全试过），判死；
- dwExtraInfo 不能清除注入标记。

**4. 豆包 UI 锚点（给 UIA 路线）**：窗口类 `OimeDirectUIWindow`（悬浮条，激活判据）、**`OimeVoiceWaveWindow`（语音波形窗 = 语音会话检测器）**、OmeMessageWindow、OmeTrayWindow。

**5. 运维注意（Round 2 必须处理）**：
- **并行代理互相污染**：多代理同机注入实验互相干扰（llmon 捕获到别家代理注入的 E/D/F/G 键；前台被豆包设置窗口抢占）——后续轮次实验需错峰或声明焦点占用；
- **系统状态披露**：A 的脚本曾写坏 `HKCU\...\InputMethodOverride` 后删除该值（现=语言列表默认，第一项=豆包）；用户原默认输入法设置可能需手动恢复；
- 遗留：2 个记事本窗口、Testing\ 下新增探针脚本/截图。

**6. Round 2 候选（A 提出）**：Win+H 停止序列与麦克风路由实验；batch vs per-event 复核；TSF COM `ITfInputProcessorProfileMgr::GetActiveProfile` 确定性识别活动输入法；InputInjector 伪设备句柄深挖。

#### Explorer B 关键发现（Round 1）

**1. 豆包运行时 UI 的 UIA 路线判死**：候选框/状态栏/语音窗（OimeDirectUIWindow 等）在任何完整性级别下 UIA 均为空（Name 空、子树 0、无 pattern）——不是 UIPI 问题，是 DirectUI 不实现 UIA provider。0.8.2.7 无可点击语音按钮（桌面工具栏开关 On 也不渲染像素）。

**2. 勘误：豆包设置页有快捷键配置项**（LongPressShortcutBox / HandsFreeShortcutBox，默认"未设置"=出厂右 Alt）——此前"固定不可配置"的结论有误。B 实测 **UIA 点击 + 注入组合键成功把长按快捷键程序化配置为 左Ctrl+左Shift+D 并持久化**——配置面可全自动，但注入触发仍被过滤（与 D 逆向闭环）。

**3. 微信输入法（WeType）实际已安装 2.1.3.18**（C:\Program Files\Tencent\WeType\，四进程常驻，含 voice_engine）——与早前"未装"结论冲突（见交叉检查）。Flutter 渲染、UIA 零暴露；存在隐藏的"语音输入"设置窗与状态栏窗，强制显示后鼠标穿透无法外部操作；具体语音触发交互 [UNVERIFIED]。

**4. 高价值附带产出**：
- **`OimeVoiceWaveWindow`（156x32）= 语音会话状态的 Win32 级探测器**（vis 翻转 = 语音起止），与注入解耦，可用于项目状态机；
- **TSF 程序化切换输入法可用**（ITfInputProcessorProfileMgr::ActivateProfile，GUID 已验证）；活动输入法判定必须用 TSF profile 查询而非 HKL（纯 TSF IME 不改 HKL）；
- 讯飞 PC 版是唯一有公开"状态栏语音按钮"入口的主流 IME（备选目标）。

**5. 清理残留（必须处理）**：豆包长按快捷键被 B 实验改为 左Ctrl+左Shift+D 且程序化重置失败——**需人工在豆包设置→语音输入→长按模式→重置**；A 删除了 InputMethodOverride 注册表值（用户默认输入法设置可能需恢复）；遗留 2 个记事本窗口。

#### Round 1 跨代理交叉检查

- **冲突 1**：A 称"WeType 未装（仅 TIP 残留）"vs B 实测"已安装 2.1.3.18 四进程常驻"——B 证据更细（文件/进程/候选框实测），A 检查可能过早或仅查卸载表；**Round 2 需一锤定音**（也影响"微信是否过滤注入"的实证可行性）。
- **冲突 2（已解决）**："请选择文本域"toast 归属——B 中途误判为豆包反应，A 证明属于 TextInputHost（Win+H 系统听写）。
- **互补闭环**：注入被豆包过滤 = A（行为层，激活态复测）+ D（逆向层，LLKHF_INJECTED 检查）+ B（配置面成功但触发失败）三方一致；C 的 physicalize 技巧与 D 的逆向共同指向 Round 2 头号实验。
- **方法论修正**：A 证明本会话此前的 32B INPUT 结构 bug 使部分旧探针无效；活动输入法判定需 TSF 查询（B）；并行代理实验互相污染（A/B 都观察到）——Round 2 实验需错峰。

#### Round 1 综合图景（合并后；部分结论已被 Round 2 更新，见 Round 2 综合判定）

**已确认可行的免驱动路线**：
1. Win+H 系统语音输入（per-event 注入，豆包激活态也有效）——待验证停止序列与麦克风路由；
2. Raw Input + LL 钩子时序窗吞键 + SendInput（自定义按键无重复输入，免管理员，双先例）；
3. TSF 程序化切换输入法；UIA 自动化豆包配置面；
4. `OimeVoiceWaveWindow` 语音状态探测（监控用）。

**已判死**：豆包 UIA 点击（无 provider）、JOURNALPLAYBACK（ACCESS_DENIED）、PostMessage（不入输入流）、InputInjector（带标志）。

**待 Round 2 验证的候选**：
1. **physicalize**（自家 LL 钩子清 INJECTED 标志转发——ZSTDJan 有实现但对豆包未验证；D 逆向表明机制上应穿透）；
2. **enableGlobalVoiceShortcut=true**（豆包私有配置开关，若走 RegisterHotKey 则注入可过）；
3. WeType 实际安装状态与语音触发机制（冲突 1 + 交互未明）；
4. Win+H 停止序列 + 麦克风路由（VB-CABLE 能否喂系统听写）；
5. 讯飞 PC 版状态栏按钮（备选 IME 目标）。

### Round 2（完成）

针对验证者 R1 问题清单组建：
- Explorer E：physicalize 决定性实验（含链序前提独立验证）+ enableGlobalVoiceShortcut 实验——豆包路线生死判定（对应严重②⑥部分、中等⑥）——**已完成，交付见下节**
- Explorer F：Win+H 三件套闭合（停止序列 + 麦克风路由端到端实验）+ WeType 过滤矛盾实证裁决（对应严重②④）——**已完成，交付见下节**
- Explorer G：D 证据链落盘复现（反汇编/配置/导入表工件入库）+ 签名结论官方口径闭合 + libvirtualhid 调研 + 自带 STT 覆盖（对应严重③、较轻⑨⑪）——**已完成，交付见下节**
- Explorer H：系统基线恢复 + 真机捕获工具链 + 边界对照表 + ATTRIBUTION/Bugs 文档修正（对应严重①、中等⑦、较轻⑩）——**已完成，交付见下节**

#### Explorer E 交付（Round 2，140+ 证据工件落盘 docs/investigations/evidence/e/REPORT.md）

**总判定：豆包注入路线三路全灭。**
1. **步骤 0 基线恢复（passed）**：Round 1 重置失败原因=可见的是 ClearButton（ResetButton 离屏）；点击后快捷键回"未设置"（=出厂右 Alt），config 持久化 {0,0}；负对照复验注入右 Alt 无语音。
2. **步骤 1 enableGlobalVoiceShortcut/RegisterHotKey 路线（failed）**：7 状态矩阵（开关×显式快捷键×服务重启×激活态）全部无语音、热键探针全部 FREE（豆包从未注册任何系统热键）。前提独立验证：注入键确实能触发 RegisterHotKey（F13 热键实证 WM_HOTKEY）——是豆包没用它，不是机制不行。附带发现：裸修饰键作热键时注入不触发；记录框拒单修饰键。
3. **步骤 2 physicalize（failed，机制级死刑）**：本地清标志成功（0x31→0x21 保留 EXTENDED；另清 extraInfo）但两种链序（我方链头/豆包链头）均无语音。**三层实证证明 LL 钩子事件数据不跨钩子传播**：①双钩子改写 A→B，写后读回 0x42 但下游钩子看到原始 0x41（每钩子私有结构副本）；②CallNextHookEx 传修改后指针下游仍见原始值（转发通道不存在）；③应用层收到原始 'a'（候选条"啊/阿/A"OCR）非改写 'b'。**结论：LL 钩子只能放行或吞掉，不存在修改下游观察内容的通道——Round 1 对 ZSTDJan physicalize 的跨进程解读错误（其技巧只能影响自家进程内逻辑），路线图中该候选删除。** 链序规则实证 LIFO（后装先调用）。
4. **恢复声明**：config 已还原并验证（快捷键 {0,0}、enableGlobalVoiceShortcut=false、无 BOM 字节干净）；ImeService 正常；测试窗口/钩子已清理。遗留说明：enableGlobalVoiceShortcut 的"原值"按 E 备份恢复为 false，但 G 曾观察到 true（updatedAtUtc=2026-09-03，改动者未定）——功能上无差异（两种值均无 RegisterHotKey 行为）。**悬案定论（R2 验证者，Round 3 I 回填）：G 读到的 true 系 E 步骤 1 实验中间态，E 恢复 false 正确，无真实基线漂移——详见 Round 3 悬案定论节。**
5. **新问题**：RegisterHotKey 0x2E55 调用点用途（G 已定位：失败检查 0x581→上报 conflict_other_app，支持"设置页冲突检测"推断）；豆包语音键除 flags 外的完整启动条件（对 WinUHid 路线有参考价值）。

#### Round 2 综合判定

- **豆包（0.8.2.7）注入触发判死三连**：SendInput/InputInjector（Round 1）+ RegisterHotKey 路线（E）+ physicalize（E，机制级）。唯一存活路径=HID 设备层（虚拟键盘驱动，#6/#7）。
- ~~**WeType 注入判死**（F，三配方+对照实验），从目标 IME 降级。~~ **【已被翻案，Round 3 J】**F 阴性根因=ActivateProfile dwFlags=0 为线程级激活、WeType 从未成为活动 IME（全程实为豆包）；会话级激活（TF_IPPMF_FORSESSION）下 Ctrl+Win 纯 VK/扫描码两配方均触发语音会话（mic ConsentStore + 吞 LWin + 0xFC break key 三证）——WeType 不过滤注入热键，**升为第一优先注入目标**（免驱动免管理员纯 SendInput）。现行结论与前提见 Round 3 Explorer J 交付节与"最终路线图前提与约束"节；原文保留作历史记录。
- **Win+H 免驱动路线三件套**：唤醒✅/生命周期✅（DOWN=Win+H、UP=Esc tap、兜底=Esc tap 幂等）/麦克风路由设备+链路层✅（IPolicyConfig 切 CABLE Output + VB-CABLE 16kHz 直通）——**语音服务层需已激活+语音包完整主机终验**（本机 Windows 未激活；Round 3 I 勘误："语音包缺失"归因存疑——OneCore 下实有 zh-CN token 而 Win+H 仍失败，见环境备忘与 Win+H 终验节）。
- **免驱动自定义按键**（Raw Input+LL 吞键+SendInput）：Round 1 双先例+机理实证成立，真机报文数据待采集（捕获器常驻中）。
- **自带 STT**（whisper-rs）：成立，独立绕开 IME 战场，2-4 周量级。
- **驱动路线成本修正**：UMDF+OV catalog（~$70-180/年或 Trusted Signing）即最低门槛，无需硬件计划/EV；libvirtualhid 为活先例（许可阻断产品使用）。

#### Explorer G 交付（Round 2，证据全部落盘 docs/investigations/evidence/g/）

**1. D 逆向证据复现：全部成立（字节级）**——安装点/hookproc 0x7426C0/检查点 0x742D9C（`41 f6 46 08 10`，全 .text 唯一）/RegisterHotKey 0x2E55/WH_GETMESSAGE/29 个 PE 零 Raw Input，全部与 D 报告吻合；hookproc 内部日志字符串实锤函数名 **`VoiceKeyHookProc`**（voice_key_hook.cpp，专用线程）。293 行反汇编已归档。
**新情报**：豆包还有 VoiceMouseHookProc（语音中点击即停）；清理路径自己注入 RAlt-up；**设置页聚焦时钩子整体旁路**（实验环境敏感点）；ASR=云端 Sami WebSocket；VHK 状态机字符串簇完整（hold/fallback/latched 等，可对标语音键生命周期）。

**2. 签名政策闭合（路线图级判定）**：**UMDF 分发不需硬件计划、不需 EV；OV 级 catalog 签名即最低门槛**。三层官方原文：加载层 1607 强制仅 kernel-mode（两处原文）；安装层 catalog 需 WHQL 或第三方 release certificate（PnP 页）+"user-mode drivers don't require digital signing"（tutorial 页）；EV 仅是硬件计划注册要求（"You don't need to sign your driver with it" 原文）。含混的 Q&A 4106592 判为支持代理模板低质量回答；2026-04 cross-signed 信任移除仅限 kernel-mode。**残余含混**：无单句"OV 签 UMDF catalog 可装"原文 → 建议一轮真机实测（Trusted Signing/OV 签 WinUHid catalog 装机）永久闭环。ARM64 需 WHQL。

**3. libvirtualhid**：产品路径**许可阻断**（Windows 键盘创建需 Polar 付费许可），但它是"Azure Trusted Signing（无 EV/无硬件计划）签 x64 UMDF catalog 分发"的业界活先例。WinUHid 仍首选。

**4. 自带 STT 路线成立（独立绕开 IME 战场）**：P2=whisper-rs + tiny-q5/base（MIT/Unlicense、16k mono PCM 与 ATVV 天然对齐、75MiB 模型/~273MB RAM、工作量 2-4 周、风险=中文质量）；P3=Windows AI Speech（Win11 24H2+，FromStream 可直接吃 PCM，但 MSIX 打包与 Tauri NSIS 冲突+实验性 API；**本机 Win10 2004 永不可测——环境备忘，Round 3 I**）；老 WinRT SpeechRecognizer 判死（仅麦克风、dictation 依赖在线）。

**5. 基线警报（交 E 处理）**：config.json 当前 `enableGlobalVoiceShortcut=true`（D Round 1 读到 false；updatedAtUtc=2026-09-03T20:51:44Z，改动者未定——B/E/用户都有可能）；B 的 Ctrl+Shift+D 快捷键残留已不在配置中；`selectedMicrophoneId`=CABLE Output（Active）。**勘误（Round 3 I）：此警报系误报**——G 读取时 E 正持锁执行步骤 1 实验（读到中间态），时区换算与定论见 Round 3 悬案定论节；E 最终恢复 false 正确，无真实基线漂移。

#### Explorer H 交付（Round 2，2026-09-04）

真机维度说明：RC001/RC003 物理按键无法由软件合成，属硬件边界；策略=先建好捕获工具链与协议，用户随时按下遥控器按键即完成采集（无需专门配合），最终报告中真机项如实标注状态。

**1. 系统基线恢复**（详见 `docs\investigations\evidence\h\baseline-restore.md`）

- `HKCU\Control Panel\International\User Profile\InputMethodOverride`：**确认当前已删除**（=使用语言列表默认，第一项为豆包输入法）。**原值未知，不可猜测**——A 在 Round 1 写坏该值后删除，未备份原值；若用户需指定默认输入法，请到 设置 → 时间和语言 → 语言 → "选择始终使用的默认输入法" 手动设置。
- Round 1 遗留记事本 ×2（pid 6872/11032，含探针文本）：**已关闭**（05:10:23 锁内 Stop-Process，探针文本为一次性内容、截图证据已存 Testing\）。
- 杂散状态复核：写字板 pid 1956 存在但启动于 04:57（Round 2），是 F 的 Win+H 实验焦点目标窗，**非 Round 1 残留，未动**（留协调者处理）；WeType 四进程正常常驻、B 曾强制显示的窗口已全部还原隐藏（H 用 EnumWindows 复核 passed）；豆包运行时窗口状态正常（OimeVoiceWaveWindow 隐藏、悬浮条常显=默认输入法正常表现）；豆包设置窗口（DoubaoImeSettings）E/F 实验中反复使用，未动。

**2. 真机捕获工具链（对策：验证者严重①）**

- 常驻捕获器 `Testing\investigation\remote-capture.ps1`（基于已验证的 llmon.ps1 改造）：WH_KEYBOARD_LL 钩子 + Raw Input 三集合注册（键盘 1/6、消费控制 0x0C/1、系统控制 1/0x80，均 RIDEV_INPUTSINK 后台接收）、QPC 高精度时间戳、LL↔RAW 关联标记（`corr=LL<n> dms=<ms>`，250ms 窗）、设备路径归因（hDevice→路径缓存，vid_2717 判 `remote=1`，RC003 的 pid 待真机按键后从 `DEV` 行读出）、窗口可见性快照（OimeVoiceWaveWindow/OimeDirectUIWindow/wetype.statusbar.window + 前台 pid，可见性翻转与心跳均记录）、20MB 滚动日志、单实例互斥、stop 文件停止协议、编译失败硬护栏（失败即写 FAILED 标记退出，不再留僵尸进程——部署首版曾因一个非 public 方法编译失败留下无钩子僵尸，已修复）。
- 部署状态（passed，2026-09-04 05:09 v2）：pid 2528 后台运行，钩子与 Raw Input 注册成功；已实测捕获真实事件流——E 的注入右 Alt（`corr=LL1 dms=1`、`corr=LL2 dms=0`，顺带实测 LL 先于 Raw Input 约 0-1ms=时序窗数据点 #1）与 F 的 Win+H per-event 序列（LWin↓/H↓/H↑/LWin↑ 全链 corr 成对）。v1（pid 9060）暴露两个问题已修复：① C# 一个非 public 方法致编译失败→ErrorActionPreference=Continue 语义下留无钩子僵尸进程（已加编译失败硬护栏+FAILED 标记）；② FindWindow 只查每类第一个窗口，OimeDirectUIWindow 有双实例致 ODUI 误报 0（已改 EnumWindows 任意可见判定）。stop 文件停止协议已实测干净退出（`# stop ll=46 raw=44` 尾行）。
- 采集协议 `Testing\investigation\REMOTE-CAPTURE-PROTOCOL.md`：每键 ×3 短按 + 1 次 2 秒长按，覆盖 OK/方向/返回/首页/菜单/电视/电源/音量三键/语音键；回答四问题：RC001 vs RC003 usage 表差异、返回键 0x00F1 是否出现在 LL/Raw、音量 VK 路径（KB 的 VK_VOLUME_* vs HID 消费页报告）、时序窗对齐（corr dms 实测分布）。
- 既有遥控器活动痕迹复核：Round 1 全部 llmon 日志无任何可归因的遥控器事件（无 0x00F1，所有 RAW 行 dev=0 注入或物理键盘，且旧工具不记设备路径，无法事后归因——新工具已修复该缺陷）；SayAll `settings.json` 统计：2026-09-03 = 23 button_presses / 22 voice_sessions / 12.75s；2026-09-04（最后写入 02:03）= **0 button_presses / 45 voice_sessions / 26.1s**——两计数器口径差异未明（45 会话含 Round 1 实验期间的注入触发？待确认），不据此下结论。SayAll 应用当前未运行；遥控器 "MI RC"（BTHLE，配对于系统层）当前 HID 设备显示 Error=未连接/休眠，任意按键会唤醒连接——捕获器常驻的意义即在用户随时按键时无准备采集。

**3. 路线 × 原 AGENTS.md 边界对照表**：见下节（维度 7）。

**4. 仓库文档修正**：Bugs 豆包文档 3 处指定勘误（32B 结构 bug 无效证据标注、"两家 IME 均过滤"改为豆包实证+微信待 Round 2 裁决、删除与真机物理对照结论矛盾的旧待验项）+ 1 处附带勘误（豆包快捷键实际可配置）；ATTRIBUTION.md：ZSTDJan 条目补 physicalize 技巧与 WeType 配方（纯 VK/80ms/无标记，标注本机未实证）、Voice_VibeCoding 条目补 LL 吞键工程细节（时序窗/链头 bump/F5 状态机/音量防双格/已踩坑清单）、新增 cgutman/WinUHid 条目（MIT、UMDF 虚拟 HID 框架、无预编译 Release、签名结论引用本文档且标注 G 闭合中）。

#### Explorer F 交付（Round 2，2026-09-04，证据全部落盘 docs/investigations/evidence/f/，详见 FINDINGS.md）

**1. Win+H 生命周期闭合（passed）**：Win+H=**toggle**（开↔关，含"请选择文本域"toast 态，a1/a2/p1 三轮状态机自洽）；**Esc tap=停止听写且幂等**（无条时无副作用）。**映射方案：语音键 DOWN=per-event Win+H、UP=Esc tap、断连/睡眠兜底=Esc tap**（不能用 Win+H 兜底——toggle 语义状态不明时会误开听写条）。新发现：①听写条静默自动关闭（约 2 分钟内，精确超时未测，产品状态机需补偿）；②Win+Alt+H 会误触发前台应用 Alt+H 菜单，不可用作听写条菜单入口；③TextInputHost 无 UIA provider（0 子元素，听写条不可 UIA 自动化）；④听写条文本右侧像素级无齿轮按钮（麦克风选择项 [UNVERIFIED]）。**版本限定（Round 3 I）：①②④为 Win10 2004 RTM 观察，Win11 行为待验（环境备忘）。**

**2. Win+H 麦克风路由（设备+链路层 passed；语音服务层 blocked-on-env）**：
- **IPolicyConfig COM**（未公开公知）可切默认录音设备到 CABLE Output（console+comm 双 S_OK，CLSID {870AF99C-171D-4F9E-AF0D-E63DF40C2BC9}）；UIA 路径因**本机设置 App 僵死**（ms-settings:sound 启动 30s+ 无窗口，且延迟弹窗会抢焦点）不可用——H 做基线恢复时会撞上此环境问题。**默认录音设备实验后已恢复 Realtek 麦克风并只读验证**。
- **VB-CABLE 链路 16kHz 直通实证**：16kHz WAV→CABLE Input（wasapi crate，与产品 audio.rs AudioSink 同构）→桌面 SR（System.Speech zh-CN MS-2052-80-DESK）从 CABLE Output 拾到音频（ConsentStore 记录 SpeechRuntime 麦克风使用；识别"因而" conf 0.002=有信号但老引擎 dictation 质量差）；**48kHz passthrough 写入=完全无信号**（VB-CABLE 管道锁 16k）——ATVV 16kHz PCM 天然匹配，其他采样率需对齐缆两端。
- **Win+H 语音服务本机损坏**：听写条报"出现错误，请稍后再试"（OCR 实证）；ConsentStore 四时点（前/听中/播后/Esc 后）零变化=**TextInputHost 从未打开麦克风流**；根因=Windows 未激活（LicenseStatus=5）+ Speech_OneCore\Recognizers 空（新语音栈识别包未安装）**（Round 3 I 勘误：实测该键存在、Tokens 下有 zh-CN token MS-2052-110-WINMO-DNN——"为空"与 R2 勘误的"键不存在"均不成立；Win+H 失败的根因拆分存疑，见环境备忘）**。**端到端闭环需在已激活+中文语音语言包完整的主机终验——是环境缺陷，不是链路失败**（设备路由层与 CABLE 音频链路层均已实证打通）。

**3. WeType 过滤矛盾裁决（注入路线 failed，机理 [UNVERIFIED]）——⚠️【已被翻案，Round 3 J】本条阴性结论全部无效**（根因=F 的 TSF 激活为线程级（dwFlags=0）、WeType 从未激活，实际活动 IME 全程为豆包；会话级激活下注入有效，WeType 不检查 LLKHF_INJECTED。现行结论见 Round 3 Explorer J 交付节与"最终路线图前提与约束"节。以下原文保留作历史记录）：TSF 激活成功（S_OK；正确 GUID=CLSID_TF_InputProcessorProfiles {33C53A50-F456-4884-B049-85FD643ECFED} + IID {71C6E74C-0F28-11D8-A82A-00065B84435C}；**常被引用的 coclass {580F4E32-...} 本机未注册**）；激活判据双证（候选框"啊/阿/A/爱/按"+wetype 窗口族）。官方口径 web 确认=按住 Ctrl+Win、免按=Ctrl+Win+Shift。**三配方注入全无反应**：ZSTDJan 纯 VK 80ms 配方 / 标准扫描码 / Ctrl+Win+Shift 免按变体（wetype/voice/record/speech 窗口 diff、进程 diff、OCR 三无变化）。**对照实验：LWin 单键 tap 注入弹出开始菜单**（OCR 实证）=注入真实到达系统层、**WeType 不吞裸 Win 键**。裁决：WeType 语音热键注入不可用（本机 2.1.3.18 行为层），与 Bugs 文档"微信过滤"方向一致但机理（过滤注入 vs 语音功能未启用/登录依赖）未定；**Bugs 文档可按此修正**："WeType 普通键注入可过（LWin 实证），但 Ctrl+Win/扫描码/免按变体三配方语音热键注入均无反应"。隐藏"语音输入"设置窗与状态栏强显后 Flutter surface 不渲染（B 的"强制显示无法操作"复现，UIA 路线对 WeType 运行时 UI 判死）。

**4. 对路线的输入**：Win+H 保留头号推荐但**标注"需在已激活主机终验"**；桌面 SR 从 CABLE 拾音可行（质量差，Grammar/OneCore 待评估）可作自带 STT 的 Windows 原生兜底方向（衔接 G 的 P2/P3）；WeType 建议从目标 IME 降级（待 G 逆向机理）。附带澄清：写字板窗口非 F 所开（F 实验仅用记事本）。

**5. 新问题**：Win+H 语音服务根因拆分（激活 vs 语音包，可在激活主机或修复语音语言后复测）；听写条超时精确值；桌面 SR dictation 质量；VB-CABLE 16k 锁定的配置来源；设置 App 僵死影响所有设置类 UIA；**并行代理锁纪律破坏**（F 04:59 接管 E 过期锁后，约 05:30 锁被删、打印对话框/多记事本/WeType"边写边译"窗污染 F 两轮实验——Round 3 需严格执行 machine.lock 协议）。

### 路线 × 原 AGENTS.md 边界对照表（维度 7，信息性——用户已解除调查限制，此表仅供最终修宪决策）

条款代号：A1=提权须独立显式 Helper 承载、主程序普通权限；A2=基础语音路径不得依赖 Frida/管理员权限/虚拟 HID 驱动；A3=第三方应用只可用公开 API/公开协议/全局快捷键/用户可见辅助功能界面；A4=禁止读写第三方 App 私有配置/内部数据库/内存结构/私有协议；A5=禁止第三方进程注入作为稳定语音主路径。

| # | 路线 | 触碰条款 | 要点 | 失败对基础路径影响 | 需修宪条款 |
|---|------|---------|------|-------------------|-----------|
| 1 | SendInput 注入（触发 IME 语音/全局热键/PTT） | 无 | 公开 API；豆包已实证过滤（failed）；~~接受注入的目标待真机~~→【R3 J 翻案】WeType 已实测接受注入（passed，前提=会话级活动 IME），豆包为仅存注入死角；已装待测=微信客户端（deferred 需用户配合），未装=搜狗/讯飞/Discord（协议见 evidence/j/target-matrix.md） | 无（叠加能力，失败仅退化） | 无 |
| 2 | Raw Input 武装 + LL 钩子时序窗吞键 | 无 | 公开 API；代价：运行期真键盘同键暂不可用（需产品披露），提权/安全桌面窗口为盲区 | 无（独立于语音路径；风险是自身 bug 误吞用户键盘，属工程风险非边界风险） | 无（建议产品边界声明中披露） |
| 3 | ~~physicalize（自家钩子清 INJECTED 后转发）~~ | — | **Round 2 E 判死（机制级）**：LL 钩子事件数据不跨钩子传播（三层实证），清标志无法影响下游钩子所见；ZSTDJan 该技巧仅作用于自家进程内逻辑。本路线从方案空间删除 | — | — |
| 4 | 改豆包私有配置 `enableGlobalVoiceShortcut=true` | **A4 明文触碰** | **Round 2 E 判死（行为级）**：7 状态矩阵全部无热键注册、注入无效；该开关在 v0.8.2.7 无 RegisterHotKey 行为（调用点为设置页冲突检测）。本路线从方案空间删除 | — | — |
| 5 | UIA 点击/配置（豆包设置页） | 无（A3 正面案例） | 用户可见辅助功能界面；豆包运行时 UI 无 provider 判死，配置面可用 | 无 | 无 |
| 6 | WinUHid + 测试签名（bcdedit，自用） | **A1+A2 触碰** | 安装/签名需管理员+虚拟 HID 驱动；需关 Secure Boot，不可分发，仅调查自用 | 无 | 调查期由"用户解除限制"覆盖；产品化必须走 #7 |
| 7 | WinUHid/自研 UMDF + OV Authenticode 分发 | A2 字面触碰（可辩护为增强轨）；A1 可满足 | "基础语音路径"禁虚拟 HID——增强轨 Helper（ADR 0002 双轨）语义可辩护；安装期提权由独立 Helper 承载即合规；OV ~$70-180/年（D 结论，G 闭合中） | 无（Helper 缺席时基础路径不受影响=ADR 0002 设计） | 需修宪澄清"基础路径 vs 增强轨 Helper"的边界定义 |
| 8 | Frida 注入 ImeService（改 hookproc 标志） | **A2+A4+A5 三重触碰** | 进程注入+改第三方内存+主路径依赖全被禁；仅调查一次性闭环验证可辩护 | 无（研究路线；风险=回调延迟→豆包语音整体失效） | 三条款均须修改才能产品化；建议永久定位为调查-only |
| 9 | Win+H 系统听写（per-event 注入） | 无 | 公开 API+系统公开功能；三件套 Round 2 F 已闭合：唤醒✅/生命周期✅（DOWN=Win+H、UP=Esc tap）；麦克风路由设备+链路层✅（IPolicyConfig 切 CABLE Output + VB-CABLE 16kHz 直通实证），**语音服务层需健康主机终验（本机未激活、Win+H 服务初始化失败；"语音包缺失"归因存疑——环境备忘，终验规格见 Win+H 终验节）** | 无 | 无 |
| 10 | 自带 STT（本地语音识别管线） | 无 | 全自有链路无第三方触碰；质量/延迟/成本为主要约束（G 覆盖中） | 无 | 无 |
| 11 | libvirtualhid（UMDF2+VHF） | A2 触碰（同 #7） | **Round 2 G 裁决**：产品路径许可阻断（Windows 键盘创建需 Polar 付费许可），不可用于免费分发；保留价值=Trusted Signing 签 UMDF catalog 分发的活先例。首选仍 WinUHid（MIT，自建） | 无 | 同 #7 |

附注（适用于全部路线）：
- 所有路线均不与"语音键按下开始/释放结束、无双击等待/长按阈值、成对清理"条款冲突；但实现层要求注入 DOWN/UP 严格成对、断连/中止/退出统一释放，吞键与 physicalize 只能叠加在语音生命周期之上、不得延迟或替代之（与 AGENTS.md 语音键节一致）。
- 表中"失败对基础路径影响"均为"无"的原因：基础路径=BLE ATVV 语音会话（音频链路），上述路线全部是触发/按键叠加层，架构上与本仓库基础路径解耦；唯一例外是 #2 吞键层自身缺陷可能影响用户正常键盘输入（非基础路径但影响系统体验，需工程护栏：有界等待+存活自检+INJECTED 排除）。
- 本表为信息性对照，不构成对任何路线的采纳建议；修宪决策权在用户。





### Round 3（完成）

**验证者 R2 判定：GAPS**（8 项问题；R1 十一项解决度：③⑥⑦⑧⑨ 解决、①②④⑤⑩⑪ 部分解决）。核心结论被复核扎实（E 三层实证逐层复核、F 环境定性独立复核、G 字节级复现独立、H 基线实测干净）。新问题：
- 严重① **OS 版本误报**：本机实为 Windows 10 Pro 19041.207（2004 RTM 未打补丁、未激活），非 Windows 11——Win+H 终验主机规格需含"受支持 OS 版本"；F 的三个 UI 观察需版本可迁移性限定；G 的 P3（Win11 24H2+）本机永不可测；"Speech_OneCore\Recognizers"实为不存在而非空（Round 3 I 复核：R2 此条勘误本身也不准确——实测该键存在、Tokens 下有 zh-CN MS-2052-110-WINMO-DNN，见环境备忘）。
- 严重② **判决未回填长期文档**：Bugs 文档仍留"待 F 裁决"且仍推荐微信输入法方案（与降级结论矛盾）；ATTRIBUTION 仍以活技巧口吻描述已判死的 physicalize（前提"链头"已被证伪）。
- 严重③ **真机归因路径未验证**：捕获器 539 行日志 100% 为注入事件（dev=0），hDevice→vid_2717→remote=1 的归因代码从未被真实设备事件执行；若归因有 bug，真机采集会静默失效。console 会话前提（RDP 会话看不到本地 HID）未入协议文档。
- 中等④ **enableGlobalVoiceShortcut 悬案已根因定位**（验证者做时区换算）：G 读到的 true 是 E 实验中间态（E 04:49 持锁，G 04:51:44 读取=步骤 1 进行中）——并行锁违规的直接后果，"基线警报"系误报；E 最终恢复 false 正确。
- 中等⑤ **锁纪律账目不可审计**：F/H/E 三份锁时间线无法同时成立；E 的关键阴性有独立证据支撑站得住，但 **F 的 WeType 三配方阴性发生在受污染窗口，复跑前应保持置信度折扣标注**。
- 中等⑥ **四大候选各缺一个决定性环节**：Win+H 缺健康主机终验（+OS 规格）；STT 质量轴零数据；吞键公式缺真机报文参数；UMDF+OV 缺装机实测（受护栏限制）。
- 较轻⑦ 盲区登记未分配（豆包启动条件分析、ZSTDJan physicalize 语义复查、搜狗/讯飞/游戏 PTT、F/G 新问题清单）。
- 较轻⑧ 证据精度小瑕疵（文件名语义、报告内部小不一致）。

Round 3 组建（对应 R2 问题清单）：
- Explorer I：长期文档回填（Bugs/ATTRIBUTION 判决同步 + OS 版本修正 + Win+H 终验规格 + 悬案根因记录）+ 锁协议硬化为审计日志制——**已完成**
- Explorer J：WeType 干净环境复跑（严格锁纪律）+ ZSTDJan physicalize 语义复查（C 本地快照）+ 搜狗/讯飞/游戏 PTT 可测性协议——**已完成（重大翻案，见下节）**
- Explorer K：豆包语音键完整启动条件分析（G 归档的反汇编 + VHK 状态机字符串）——**已完成（见下节）**
- Explorer L：whisper-rs 质量实测 spike（F 的 16kHz 中文测试 WAV）——**已完成**

#### Explorer J 交付（Round 3，69 件证据落盘 docs/investigations/evidence/j/——件数勘误与字段语义勘误见 evidence/j/ERRATA.md）

**1. WeType 复跑——F 结论被推翻（passed，重大翻案）**：
- **F 的"三配方全无反应"根因=WeType 从未被激活**：F 的 ActivateProfile 用 dwFlags=0（线程级；会话级需 TF_IPPMF_FORSESSION=0x20000000），其记事本全程实际活动 IME 是豆包（窗口快照 wetype 全 vis=False 佐证；两家 IME 候选词相同致 OCR 误判）。J 先用 flags=0 复现假象，再会话级激活重测。
- **注入有效（决定性）**：Ctrl+Win 按住（纯 VK 80ms 配方与扫描码配方均触发）→ wetype_update 开麦（ConsentStore 06:45:35/06:45:40 铁证）+ WeType 吞掉注入的 LWin + 自注入 0xFC break key（extra="WTYP"）；**释放后 mic 干净关闭（06:51:10→13，按住说话生命周期天然成对）**。WeType 2.1.3.18 **不检查 LLKHF_INJECTED**（与豆包决定性对照）；语音功能已启用、无需登录。HF 免按和弦默认值 [UNVERIFIED]（+Shift/+Space 均被拒并重放吞键）。
- **WeType 升为第一优先注入目标**（免驱动免管理员纯 SendInput）。遗留复测：快速连按、释放停止延迟精确值、HF 和弦、麦克风路由 CABLE Output。
- **用户实测指引**：用户此前"左Ctrl+左Win 无效"很可能同样踩了激活陷阱——目标文本框内 WeType 必须是当前活动输入法（任务栏指示器确认）后按遥控器语音键。
- ⚠️ **文档回填需求**：I 代理刚回填的"判死"结论需再次修正（Bugs 文档 WeType 段、ATTRIBUTION WeType 配方注、本工作文档 F 交付节与 Round 2 综合判定）。

**2. ZSTDJan physicalize 语义复查（passed，结构性无效技巧定论）**：位置勘误（legacy_key_suppressor_windows.py L142-155，非 win32_input.py）；真实语义=仅在自家钩子私有副本上清标志→转发→恢复，**对下游钩子/应用层不可见且进程内无读者——对声明目标是 no-op**；真正能影响豆包的是 doubao_rpc.py 的 Frida 版（attach ImeService 清标志，未接线进生产）。ATTRIBUTION 精确措辞已交付。

**3. 目标矩阵（target-matrix.md）**：已装实测=WeType（passed）/豆包（failed）/Win+H（F）/记事本（passed）；已装未测=微信客户端 4.1.13.63（Ctrl+Win 同键位，需用户同意后用文件传输助手测）、UU远程（需远控会话）、QQ拼音（无语音非目标）；未装目标附安装协议（搜狗/讯飞/Discord/游戏）。

#### Explorer L 交付（Round 3，whisper 质量实测，证据落盘 docs/investigations/evidence/l/）

| 模型 | 体积 | 中文 CER | RTF（本机 i3-8130U 2C/4T） | 输出 | 判定 |
|---|---|---|---|---|---|
| tiny-q5_1 | 30.7 MiB | **33.3%**（同音字灾难） | 2.30-2.61 | 简体 | 中文判死 |
| base-q5_1 | 56.9 MiB | 9.5%（t2s 归一化） | 6.36-6.51 | **繁体** | 最低可用线 |
| small-q5_1 | 181.2 MiB | **0%** | 22.43 | 简体 | 质量完美/本机太慢 |

- 关键工程事实：短中文音频语言自动检测误判英语（tiny/base auto **各一次**，见 runs/tiny-auto.txt 与 runs/base-auto.txt 的 detect_lang_id=0 与英文转写；必须 set_language("zh")）；ATVV 16k 对齐正向确认（48k 不重采样=幻觉循环负对照）；构建摩擦可行（cmake+libclang，clean 构建 75.8s，静态 exe 2.8MiB）；whisper-rs 上游迁 Codeberg（活跃度打折）。
- **新头号风险=延迟**：弱 CPU 上"松开语音键等全文出字"=RTF×时长（base≈44s@本机）——产品化必须流式/分块解码或更快中文模型（SenseVoice-small/sherpa-onnx 列为 R4 头号 STT 候选）。单句+干净 TTS 音频=质量乐观上界；真实 ATVV 远场麦克风未测。

#### Explorer K 交付（Round 3，主报告 docs/investigations/evidence/k/doubao-voice-start-conditions.md，360 行 + 11 份反汇编工件）

> **交付计量勘定（Round 5 O 复核，回应 R4 验证者问题 1）**：目录实测 12 文件 = 11 份反汇编 txt 工件 + 1 份 md 主报告（另有 _tools 子目录 6 个脚本）——原"6 份反汇编工件"计数失实，已修正。行数实测 **360 行**（原始字节 LF 计数=360、UTF-8 读取=360、read 工具=360；文件 2026-09-03T23:01Z 落盘后未改动）：R4 验证者测得的"234 行"系 PowerShell 5.1 对无 BOM UTF-8 按系统 ANSI/GBK 读取的**吞行假象**（同一文件 GBK 解码后仅剩 233 个 LF；该坑已记录于本文件环境备忘节），非版本漂移——原始"360 行"描述经复核准确。无版本历史可考的顾虑因字节级计数而消除。内容质量经 R3/R4 验证者抽查实锚通过，以现存文件为准。

豆包语音启动完整条件（静态还原，实锚）：
1. **按键匹配**：VK 级 `vkCode==VK_RMENU`（左 Alt 不触发）；方案字符串 `right_alt`（默认）/`right_alt_space`/`left_ctrl_win`/`null`；判定=当前按下集合 ⊆ 方案键集合（右 Alt 必须独按）；单键方案 DOWN 不吃键，UP 被吃掉并自注入 F15+RAlt↑ 防菜单（SynthesizeMetaRelease）。
2. **时序**：hold 阈值=**150ms**（只管波形窗与轻点/长按分类）；**音频在 keydown 当即预启动**（`audio_started_before_ui` 实锚）；释放 <150ms → PRESS_CANCEL 丢弃；免按 fallback 300ms；异步停止重试窗 2s。
3. **上下文**：allowed=IsImeForegroundActive（rpc.dll!IsPidImeActive(焦点pid) → 全局开关回退——这解释了 E 的"enableGlobalVoiceShortcut 无 RegisterHotKey 行为"：它放宽的是**前台激活要求**而非热键注册方式）；每次激活查快捷键冲突；设置页聚焦整体旁路；**文本域焦点不是启动条件**；Controller precheck：**网络断连直接拒绝**（云端 Sami ASR 硬前置）。
4. **状态机**：consumed_/prestarted_/latched_/wave_shown_ 等字段 + 消息协议 0x3EF~0x3F6 + 停止键面（Win/Alt/Tab/CapsLock/PageUp-Down/Ctrl+V 立即停；打字键透传由 TSF 停+300ms 兜底；鼠标点击快停）+ 清理三件套（前台切换/退出：补停+注入 RAlt-up+全复位）。
5. **WinUHid 设计输入**：注入 E0 38 ↓→保持 ≥150ms（建议 200ms）→↑ 成对；期间不得有任何其他按键（会停会话）；环境准备=豆包活动 IME+网络+麦克风 CABLE Output+设置页不聚焦；音频延迟≈DOWN 即起。
6. 9 项 [UNVERIFIED] + 5 个物理键验证实验设计（Exp K-1~K-5）待错峰执行。

#### Round 3 阶段判定（四代理全部交付）

- **免驱动语音路线图巨变**：WeType 注入有效（第一优先，免驱动免管理员）+ Win+H（系统级，终验规格已备）+ 自带 STT（质量轴到位，延迟风险明确）。
- 豆包仍为唯一注入死角，仅剩 WinUHid 虚拟键盘路线（K 的启动条件分析已给出注入规格：E0 38 按住 ≥150ms 成对、环境清单、150ms 阈值/网络前置/前台激活要求）。

### Round 4（进行中）

**验证者 R3 判定：GAPS**（5 项问题；R2 八项：①④⑤ 干净解决、⑦⑧ 大部解决、②⑥ 部分解决、③ 半解决；**J 翻案经逐环独立复核维持成立**——假象复现对照/ConsentStore 时间戳/捕获器 ground truth 全部扎实）。新问题：
- 高① **翻案后工作文档自身不一致**：Round 2 综合判定、F 交付节、路线表 #1、"已确证事实"区四处过期结论未标注"已被翻案"（历史记录 vs 现行结论区分缺失）；
- 高② **真机归因路径零去风险**：捕获器日志 318 条 LL 事件 100% 注入、0 条 DEV 归因行——归因代码从未被真实设备事件执行，真机采集静默失效风险原样开放；
- 中③ **多个决定性环节仍缺**：真机遥控器数据（用户未按键，全 deferred）、Win+H 健康主机终验、WinUHid 装机实测（护栏）、**WeType 本机可闭环复测项全部未做**（快速连按成对性/释放停止延迟/HF 和弦/麦克风路由/E2E）、微信客户端待测、STT 真实音频；**新发现产品级矛盾**：WeType >500ms 长按门限（快按会丢弃预录音）与语音键"快速按下/释放成对清理"条款的产品层对齐未处理；
- 中④ **"WeType 第一优先"的前提（会话级活动输入法）未被路线图显式吸收**；
- 低⑤ 证据精度新瑕疵（J 件数 69≠74、mic_delta 字段语义易误读、target-matrix 与环境备忘的"语音包"表述矛盾残留、K 节标题过时等）。

Round 4 组建：
- Explorer M：工作文档自洽回填（翻案标注四处 + 事实区吸收 K/L/J 头部结论 + WeType 前提与 >500ms 门限入路线图）+ 证据精度修正——**已完成**
- Explorer N：WeType 本机可闭环复测（快速连按/释放延迟/HF 和弦探测/麦克风路由 CABLE Output + TTS WAV 听写 E2E）+ 捕获归因路径去风险（静态审查 + 自检清单）+ 微信客户端语音测试——**已完成**

#### Explorer N 交付（Round 4，40+ 证据落盘 docs/investigations/evidence/n/FINDINGS.md）

**任务 1：WeType 复测 6 项全 passed**（受控环境：FORSESSION 激活 + 窗口可见性+行为双判据）：
- **快速连按成对性**：600ms 按住×10 与 200ms 按住×10 均 **10/10 mic 开/关严格成对**（开 122-181ms、关 107-125ms；快按释放 0-3ms 同步关）；捕获器 120 行 LL ground truth 逐一对应（Win 全被吞、WTYP break key 成对、零泄漏、无粘麦）；
- **释放停止延迟**：2s 按住×3 = **109/115/127ms（≈117ms）**；
- **<500ms 快按机理修正**：设备会话仍开且成对（~120ms），取消发生在 **ASR 预录音层**——与语音键成对清理条款**无冲突**（UX 预期需文档化：快按无文本）；
- **HF 和弦**：四种探测全部被拒（LP中tap Shift 会话持续不中断、3.5s 长按无自动升级）——HF 需 UI 配置 [UNVERIFIED]；
- **麦克风路由 E2E（passed ×2）**：IPolicyConfig 切 CABLE Output → Huihui TTS 16kHz WAV → CABLE Input → WeType Ctrl+Win 按住 → 云 ASR → **记事本转写「今天天气很好，我们一起去公园散步聊天吧」19 字精确**（OCR + WM_GETTEXT 双证）——**ATVV→CABLE→WeType 云 ASR→文本的免驱动听写链路本机终验通过**（**分段实证+拼接论证**：ATVV 段以 WAV 等价模拟；注入段用 PS SendInput 模拟，与产品 Rust SendInput 同 API 同结构——遥控器端到端仍属真机 deferred，精确口径见 evidence/n/FINDINGS.md ERRATA ④）；
- **IME 切换稳定性**：行为级 6/6；判据精化（WTSB 状态栏可见性滞后，须用候选框行为判据）。

**任务 2：捕获归因去风险**：静态审查——仓库已知两种设备路径形态（vid_2717 HID / dev_vid&012717 BLE）**均被覆盖（最可疑的坑不存在）**；5 项次级缺陷登记（新拼写假想形态/512B 截断/usage 页整键缺失/句柄缓存/VID-only）；离线单测 11/11 PASS（含 raw_input.rs 双 fixture）+ 生产 matcher cargo test pass；首事件验证清单（7 项）已写入采集协议。

**任务 3：微信客户端 deferred（登录墙）**：Weixin 4.1.13.63 启动后为 QR 码登录窗（J 的"已登录"前提已失效），扫码需用户手机——登录窗**特意留置前台**供用户随时扫码，复跑协议已备（文件传输助手→Ctrl+Win→ConsentStore+OCR）。

**恢复声明**：录音设备/输入法已恢复并验证；无消息发送；微信登录窗留置（唯一有意残留）。

**新问题**：微信登录态需用户一次扫码；WTSB 判据滞后；f-audio-play 5s 启动延迟（产品勿照抄）；PS 5.1 ushort 绑定坑；记事本 UIA 读偶发（WM_GETTEXT 可靠）；WeType 云 ASR 丢句尾句号。

待决项（进入最终路线图的 deferred 清单）：真机遥控器采集（**用户随时按键即采集，捕获器常驻 console 会话**）、Win+H 健康主机终验、WinUHid 装机实测（护栏限制）、K 的物理键实验 Exp K-1/2/4/5、STT 真实音频 CER 与目标机 RTF。

#### 悬案定论：enableGlobalVoiceShortcut"基线漂移"（R2 中等④，Round 3 I 记录）

R2 验证者做时区换算后定论：**无真实基线漂移，系并行锁违规导致的误报**。G（Round 2）读到的 `enableGlobalVoiceShortcut=true` 是 E 的实验中间态——E 于 04:49（本地时间）获取机器锁开始步骤 1 实验（7 状态矩阵需临时置 true），G 04:51:44 读取 config.json 恰在步骤 1 进行中（G 报告的 updatedAtUtc=2026-09-03T20:51:44Z 换算本地时间即 2026-09-04 04:51:44，与读取时刻吻合）；E 实验完成后按备份恢复 false 并验证。G 的"基线警报"与 E 的"改动者未定"遗留说明均由此解释，E 最终恢复正确。教训已固化进机器锁协议（`Testing\investigation\machine-lock-protocol.md`）：**依赖实验敏感状态的只读检查（如读第三方 config.json）同样必须持锁**，机器敏感实验走 ACQUIRE/RELEASE 审计日志制。

## Win+H 终验主机规格与流程（健康主机一键终验）

本机因 Windows 未激活（LicenseStatus=5）+ Win+H 语音服务初始化失败（"出现错误，请稍后再试"），语音服务层终验 blocked-on-env；设备路由层与 CABLE 音频链路层已实证打通（F）。健康主机按本节规格与序列终验，通过后 Win+H 三件套（唤醒/生命周期/麦克风路由+转写）完全闭合。

### 主机规格（前置条件）

1. **已激活**的 Windows（LicenseStatus=1）。未激活机器 TextInputHost 不打开麦克风流（F 的 ConsentStore 四时点零变化实证），听写条必报"出现错误"。
2. **受支持的 OS 版本**：建议 Windows 10 最新补丁版（22H2/LTSC）或 Windows 11。本调查全部 Win+H 行为数据采集于 Win10 2004 RTM（19041.207，未打补丁），Win11 行为待验（版本限定见环境备忘）。
3. **zh-CN 语音输入功能可用**：以功能判据为准——设置 → 时间和语言 → 语音 → 语音语言=中文（简体），手动 Win+H 能出"正在聆听"并转写。注册表仅供参考（来自本机形态，非充分/必要判据）：`HKLM\SOFTWARE\Microsoft\Speech_OneCore\Recognizers\Tokens` 下的 zh-CN token 形态——本机仅有 MS-2052-110-WINMO-DNN（WinMobile DNN 变体）时 Win+H 仍失败，token 形态与听写可用性的关系留给终验对照。
4. VB-CABLE 已安装（实证其管道锁 16kHz：16k 直通有信号、48k 无信号；ATVV 16kHz PCM 天然匹配）。

### 终验序列（各段均为本调查已验证的步骤，按序拼装）

前置：记事本等文本域窗口置于前台（否则 Win+H 提示"请选择文本域"）；确认当前会话为 console（qwinsta，见 `Testing\investigation\REMOTE-CAPTURE-PROTOCOL.md`）。

1. **唤醒**：per-event 注入 Win+H——`LWin↓ → 80ms → H↓ → 60ms → H↑ → 60ms → LWin↑`（40 字节 INPUT 结构；本机实证 sent=1 且听写条"正在聆听"，豆包激活态同样有效）。
2. **生命周期**：Esc tap 注入停止听写（实证幂等：无条时无副作用）；再 Win+H 重新唤起确认 toggle 语义。语音键映射方案=DOWN 注 Win+H、UP 注 Esc tap、断连/睡眠兜底 Esc tap（不得用 Win+H 兜底——toggle 语义状态不明时会误开听写条）。
3. **麦克风路由**：IPolicyConfig COM（CLSID {870AF99C-171D-4F9E-AF0D-E63DF40C2BC9}）把默认录音设备（eConsole+eCommunications 双角色）切到 CABLE Output，只读查询确认。
4. **转写闭环**：16kHz/16bit/mono 中文 WAV 播入 CABLE Input（wasapi 共享模式渲染，与产品 audio.rs AudioSink 同构）→ 听写条转写文本与 WAV 内容一致。本机止步于此（链路层有信号：桌面 SR 可识别 + ConsentStore 记录 SpeechRuntime 拾音，但 Win+H 服务层初始化失败）。
5. **恢复**：IPolicyConfig 把默认录音设备切回原麦克风，只读验证恢复；Esc tap 确认听写条已关。

全序列通过 = Win+H 路线三件套在该主机 passed，写入路线图终验记录。

## 最终路线图前提与约束（Round 5 终稿素材，待最终报告吸收；Round 4 M 整理）

> 对应 R3 验证者中③（WeType >500ms 门限与语音键条款的产品级矛盾未对齐）与中④（"WeType 第一优先"的前提未被路线图显式吸收）。本节把各路线进入最终路线图前必须成立/必须决策的前提显式化；实证结论本身见各轮交付节，deferred 项见 Round 4 待决项清单。标注"产品决策"的条目不由调查代理代决。R4 验证者已复核（GAPS 以文档精度问题为主）；Round 5 O 已按其问题清单修正——N 复测终值已吸收（前提 B）、证据口径精化（分段实证+拼接论证）、路线×工作量估计表已补（见本节末）。本节现为 Round 5 终稿素材，待最终报告吸收。

### 1. WeType 路线（第一优先：免驱动、免管理员、纯 SendInput）

**前提 A：WeType 必须是目标文本框的会话级活动输入法。**
- 本机默认输入法是豆包（InputMethodOverride 已删除、语言列表第一项=豆包），WeType 不会自发成为活动 IME；程序化切换必须用 TSF `ITfInputProcessorProfileMgr::ActivateProfile` + `TF_IPPMF_FORSESSION=0x20000000`（dwFlags=0 仅线程级、不改变会话活动输入法——F/J 两轮实证教训）。
- 产品二选一（**产品决策**）：
  - a. **自动切换**：语音会话前 FORSESSION 切到 WeType（公开 COM API，A3 合规），但**触碰第三方输入法的用户可见状态**——会话期间用户打字全部经 WeType；会话结束是否恢复原 IME、恢复到哪个状态需定义；与"用户日常使用豆包"的现状冲突。
  - b. **要求用户日常选用 WeType**：产品用 TSF profile 查询（非 HKL）检测活动 IME ≠ WeType 时，UI 引导用户手动切换（任务栏输入法指示器），不改任何状态。
  - **M 建议**：默认 b（无第三方状态触碰、实现简单）；a 作为用户显式 opt-in 的增强开关（"自动切换到微信输入法"，会话后恢复原 IME 并只读验证）。无论 a/b，激活判据用 wetype.statusbar.window/wetype_candidate 可见性（J 教训：双 IME 候选词 OCR 同文会误判激活）。

**前提 B：WeType >500ms 长按门限与语音键条款的对齐。**
- 事实：WeType 长按热键（Ctrl+Win）自带 >500ms 门限——快按（<500ms）取消预录音、无文字产出（二进制字符串 `kLongPressEnd: pre-recording cancelled (< 500ms)`，J 实证）。
- 条款对齐论证：SayAll 的注入**镜像遥控器语音键的物理按住时长**（按下注 DOWN、释放注 UP），500ms 门限是 WeType 自身行为而非 SayAll 添加的阈值——AGENTS.md"不得为语音键增加双击等待或长按阈值"约束的是 SayAll 的按键处理时序，不因目标 IME 的内部分类而违约。
- 产品层方案评估：
  - ①文档披露 + UI 提示"微信输入法需按住半秒以上"（快按=取消、无文字）——零生命周期改动，与条款完全一致；
  - ②快按释放时由 SayAll 补足注入保持到 ≥500ms——**评估后不采纳**：它把语音生命周期延长到用户物理释放之后（用户已松键、录音仍在进行），直接违反"按下开始、释放结束"与"快捷键注入只叠加在 ATVV 语音会话之上，不得替代或延迟语音生命周期"（AGENTS.md 语音键节）；且快按通常是用户取消意图的表达，替用户"续按"改变语义。
  - **M 建议**：采用①；并把"WeType 500ms / 豆包 150ms / Win+H toggle"并列为"目标端语义差异"披露清单。门限精确值与快按/连按行为细节已由 N 代理 R4 复测闭合（evidence/n/FINDINGS.md）：释放停止延迟 **109/115/127ms（≈117ms 均值）**；快速连按成对 **20/20**（600ms 与 200ms 按住各 10/10，捕获器 ground truth 零泄漏零粘麦）；<500ms 快按取消发生在 **ASR 预录音层、设备会话恒成对**（与语音键成对清理条款无冲突）；HF 和弦**非默认、需 UI 配置 [UNVERIFIED]**（四种探测全被拒）；麦克风路由 E2E ×2 passed（19 字精确转写，OCR+WM_GETTEXT 双证）；IME 切换行为级 6/6（判据用候选框行为）。

### 2. 豆包路线（WinUHid 增强轨：豆包唯一存活触发路径）

**前提 A：注入规格（K §7，静态还原；装机实测 deferred）**——虚拟键盘发 **E0 38（右 Alt）DOWN → 保持 ≥150ms（建议 ≥200ms 余量）→ UP**，严格成对；期间按下集合必须恰为 {VK_RMENU}（任何其他按键触发 PRESS_STOP/CANCEL，遥控器其他键需改走 SendInput 或延迟到会话结束）；typematic 无害；裸 RAlt 菜单副作用兜底=UP 后 F15 tap（豆包同款技巧）。
**前提 B：环境清单（会话前产品准备，K §7.2）**——豆包为焦点进程的活动输入法（IsPidImeActive；TSF 切换能力已有）+ **网络在线**（云端 Sami ASR 硬前置，断连直接拒绝）+ 无快捷键冲突 + 豆包设置页不聚焦（聚焦=钩子整体旁路）+ 麦克风选定 CABLE Output（selectedMicrophoneId）+ ImeService 存活；文本域焦点非启动条件，但决定识别文字落位。
**前提 C：150ms 阈值的产品语义**——音频在 DOWN 当即预启动（不等阈值）；释放 <150ms → PRESS_CANCEL 丢弃预启动录音（无文字产出）。与 WeType 500ms 同理：注入镜像物理按住时长、门限属目标 IME 自身行为，SayAll 不添加阈值；产品层同口径披露（"豆包需按住 150ms 以上"）。语音键条款对齐逻辑同路线 1 前提 B。
**前提 D：边界与修宪**——虚拟 HID 驱动为 A2 字面触碰（ADR 0002 增强轨语义可辩护；安装期提权走独立 Helper 满足 A1）；UMDF+OV catalog 签名结论待装机实测闭环（G 残余含混）；本机受实验护栏不装机，全链 deferred。

### 3. Win+H 路线（系统级兜底）

**前提 A：健康主机**——已激活 Windows（LicenseStatus=1）+ 受支持 OS 版本（建议 Win10 22H2/LTSC 或 Win11；本机行为数据全部采集于 Win10 2004 RTM 19041.207，Win11 行为待验）+ zh-CN 语音输入功能可用（功能判据=手动 Win+H 出"正在聆听"并转写；注册表 token 形态仅参考——OneCore 下有 zh-CN token 而 Win+H 仍失败，"语音包缺失"归因存疑，见环境备忘）。本机 blocked-on-env；终验序列见"Win+H 终验主机规格与流程"节。
**前提 B：焦点管理**——目标文本域必须前台聚焦，否则 Win+H 提示"请选择文本域"且不拾音；产品需引导/管理焦点。
**前提 C：Esc 兜底与状态机**——语音键 DOWN=per-event Win+H、UP=Esc tap、断连/睡眠/中止兜底=Esc tap（幂等，无条时无副作用；不得用 Win+H 兜底——toggle 语义状态不明时会误开听写条）；听写条静默自动收起（Win10 2004 观察，约 2 分钟内）需状态机补偿；VB-CABLE 管道锁 16kHz（ATVV 天然匹配，其他采样率需对齐缆两端）。

### 4. 自带 STT 路线（全自有链路）

**前提 A：延迟预算**——L 实测 RTF（i3-8130U 2C/4T）：tiny 2.30-2.61 / base 6.36-6.51 / small 22.43；"松开语音键等全文出字"=RTF×时长（base 6.8s 音频≈44s）。产品化必须流式/分块解码或更快中文模型（SenseVoice-small/sherpa-onnx，R4 头号候选），并把"释放→首字/全文可用"定义为产品延迟指标。
**前提 B：目标机 CPU 基线**——需定义最低 CPU 规格；本机 i3-8130U 2C/4T 仅作参照下限，RTF 数据不可外推到更弱机器；真实 ATVV 远场麦克风音频 CER 与目标机 RTF 均未测（deferred）。
**前提 C：质量-体积分档披露**——tiny 中文判死（CER 33.3%）；base 最低可用（CER 9.5%，输出繁体需 t2s 归一化）；small 质量完美（CER 0%）但弱机不可用（181.2MiB 模型）——模型选择按目标机 CPU 分档并向用户披露。

### 路线 × 工作量估计（Round 5 O 汇总，回应 R4 验证者问题 5）

> **估计口径：粗粒度工程判断，非承诺。**"天级"=数个工作日内（含自测）；周级区间按单人专职、含联调自测，不含第三方依赖等待（证书采购、健康主机到位等）。

| 路线 | 已具备（实证/已实现） | 剩余工作 | 估计 |
|---|---|---|---|
| 1. WeType 免驱动语音（第一优先） | 注入已实现、E2E 已过（N：链路 ×2 passed，19 字精确转写；连按 20/20 成对、释放停止 ≈117ms） | 前提引导 UI（活动 IME 检测+切换引导，前提 A 的 b 方案）+ 文档披露（500ms 门限等目标端语义差异清单）+ 真机遥控器验收 | **天级** |
| 2. 吞键层（自定义按键免重复输入） | 公式已实证（Raw Input 武装 + LL 时序窗 + SendInput 注入）；VVC 全套工程细节已拆解（自家注入放行 EXTRA_INFO、时序窗参数、链头 bump 重叠安装） | LL 钩子 + 时序窗 + 链头 bump + 状态机全套实现与真机时序参数对齐 | **2-4 周** |
| 3. Win+H 系统级兜底 | 注入序列已验证（per-event 唤起"正在聆听" + Esc 兜底幂等 + IPolicyConfig 麦克风路由） | 健康主机终验（规格与序列见"Win+H 终验主机规格与流程"节）+ 焦点管理 + 听写条静默收起的状态机补偿 | **天级，依赖主机**（终验需已激活 + zh-CN 语音可用的机器） |
| 4. 自带 STT（全自有链路） | 质量轴实测（L：small CER 0%、base 9.5% 繁体需归一化）；构建摩擦已过（whisper-rs 静态 exe 2.8MiB） | whisper 集成 + 流式/分块解码，或 SenseVoice-small/sherpa-onnx 替代；真实 ATVV 远场音频 CER + 目标机 RTF 实测 | **2-6 周** |
| 5. WinUHid 增强轨（豆包唯一存活路径） | 注入规格静态还原（K §7：E0 38 按住 ≥150ms 成对）；签名政策已闭合（G：OV 级 catalog 签名为最低门槛） | 驱动构建 + OV catalog 签名 + 装机实测 + K 注入规格的产品层实现 | **4-8 周 + 证书成本**（OV 证书价格随时间波动、未复核，D 估值 ~$70-180/年档） |

- 路线 5 边界（ADR 0002）：虚拟 HID 驱动为 A2 字面触碰（增强轨语义可辩护；安装期提权走独立 Helper）——是否启动属修宪/产品决策，不在调查代理权限内；装机实测受调查护栏限制，全链 deferred。
- 真机遥控器验收（LL/Raw 事件形态采集）是路线 1/2 的共用前置——采集协议与常驻捕获器已就绪，用户随时按键即完成（console 会话前提）。

### Round 5（完成，最终轮）

- Explorer O：R4 五项文档问题收尾（K 节计量勘定并推翻 R4 的"234 行"测量——GBK 吞行假象，字节级 LF=360；路线图吸收 N 终值；Bugs/ATTRIBUTION 滞后同步；evidence/n ERRATA；工作量估计表）——**已完成**

**最终验证者 R5 判定：PASS**（min_rounds=5 已满足；完成标准 4/4 达成：五要素/路线齐备、三目标注入接受性本机实证、含 fallback 的可执行路线图、关键结论全部带来源/[UNVERIFIED]；O 五项修正 5/5 落位，"360 vs 234"经字节级独立仲裁成立；六项 deferred 全带规格；已知矛盾全部闭环）。

**调查终态**：5 轮 × 探索代理（R1:4、R2:4、R3:4、R4:2、R5:1，共 15 名）+ 验证者 R1-R5；证据库 evidence/{e,f,g,h,i,j,k,l,n}；最终报告见 `2026-09-04-avoid-driver-signing-input-paths-final.md`，交接文档见 `handoff.md`。

## 环境备忘（给探索代理）

### 本机环境事实（Round 3 I 实测修正，R2 严重①；与任何代理报告冲突时以本条为准）

- **本机 = Windows 10 Pro 19041.207（2004 RTM，未打补丁）+ 未激活（LicenseStatus=5，通知模式）**（实测 2026-09-04 06:20：Win32_OperatingSystem + CurrentVersion.UBR + SoftwareLicensingProduct，见 `evidence\i\env-facts-check.txt`）。此前多份报告误称"Windows 11"，一律以本条为准；一切 Win11-only 行为在本机不可测。
- F 的三个 Win+H UI 观察均为 **Win10 2004 观察，Win11 行为待验**：①听写条静默自动收起（约 2 分钟内）；②Win+Alt+H 误触发前台应用 Alt+H 菜单；③听写条文本右侧像素级无齿轮按钮。已同步在 F 交付节标注。
- G 的 P3（Windows AI Speech，要求 Win11 24H2+）**本机永不可测**；自带 STT 路线本机仅 P2（whisper-rs）可实测。
- **语音识别器注册表实测（Round 3 I，同时修正 F 与 R2 两版表述）**：`HKLM\SOFTWARE\Microsoft\Speech_OneCore\Recognizers` **键存在**（R2 勘误"键不存在"按字面不成立），其 `Tokens` 下有且仅一个 zh-CN token：**MS-2052-110-WINMO-DNN**（WinMobile DNN 变体，非桌面 DESK 级——OneCore 下不存在 MS-2052-80-DESK 子键）；F 的"目录为空"亦不成立。推测两者检查的都是"OneCore 下是否有桌面级 DESK token"（该子键确实不存在）。旧桌面 SAPI 栈 `HKLM\SOFTWARE\Microsoft\Speech\Recognizers\Tokens\MS-2052-80-DESK` 存在（F 的桌面 SR 实测即用它）。**含义：OneCore 下存在 zh-CN token 但 Win+H 仍初始化失败，"语音包未安装"归因存疑；根因拆分（未激活 vs 语音包形态）留给健康主机终验对照。**
- 采集/注入类实验的会话前提：当前 shell 实测处于 **console 会话**（qwinsta：`>console Administrator 1`，2026-09-04 06:20）；若改经 RDP 接入，本地 HID 与注入行为前提全部失效——见 `Testing\investigation\REMOTE-CAPTURE-PROTOCOL.md` console 会话节。

- PowerShell 5.1 无 BOM 的 UTF-8 脚本按 ANSI 读——脚本内避免中文，或用 BOM。
- 视觉模型未配置：`dim image read` 不可用；用 `dim ocr recognize <path> --json`（本地 OCR，可读中文界面）。
- Add-Type C# 嵌套 struct 必须 public；静态方法调用用 `[类名]::方法()`。**跨类调用的 static 方法同样必须 public**（Round 2 H 教训：一个非 public 方法致编译失败，且 ErrorActionPreference=Continue 下脚本不中止、逐条语句失败后继续跑，留下无钩子僵尸进程——长驻工具须加 `'类名' -as [type]` 存在性硬护栏，remote-capture.ps1 已内置）。
- 前台焦点：SetForegroundWindow 可能被焦点窃取保护拒绝；用 SetWindowPos 挪窗口 + 鼠标点击（mouse_event）拿真实焦点。
- 实验护栏：不修改系统启动配置（bcdedit）、不安装任何驱动、不注入可能导致进程崩溃的代码；高风险实验标记给协调者审批。
