# 豆包输入法未响应按住说话快捷键（右 Alt）

## 复现

- RC001/RC003 真机连接，ATVV 就绪；输出端点 CABLE Input（VB-CABLE）已选择。
- 连接页按住说话快捷键预设选择"右 Alt"（voice-hold-hotkey.json = right_alt）。
- 按住语音键说话：语音会话计数增加，但豆包输入法语音输入未被唤起。

## 本机排查证据（2026-09-04，全部为实际执行观察）

- 应用侧：settings.json 显示当日 19 次语音会话完成、endpoint 为 CABLE Input、快捷键为 right_alt。会话完成意味着注入 DOWN 成功（SendInput 失败会中止会话且不计数）。
- 注入机制：独立探针以与应用完全相同的 INPUT 构造（scan 0x38 + EXTENDEDKEY）注入右 Alt，GetAsyncKeyState 在 7ms 内观察到 DOWN、松开后观察到 UP，按住期间状态保持——注入在系统层真实到达。
- 豆包侧配置（其可见设置界面读取）：语音输入为长按模式（按住说话，松手结束）、麦克风已选 CABLE Output、语音页无快捷键配置项。**勘误（Round 1 B 实证）：豆包设置页实际存在快捷键配置项（长按/免提快捷键，默认"未设置"=出厂右 Alt），且可 UIA 程序化配置——下条"固定不可自定义"表述有误，保留原文仅作当时记录。**
- 公开资料：豆包输入法 Windows 版语音快捷键为固定的"长按右边 Alt，或按一次右 Alt+空格键"，不可自定义。（已被上条勘误修正：可配置。）
- 合成注入实验：向记事本注入"按住右 Alt 1.5 秒"及"右 Alt+空格"变体，按住期间与释放后的整屏截图对比与 OCR 均无豆包语音界面出现（diff=0 或仅光标级噪声）；受限项：合成条件下无法确认焦点窗口的活动输入法是否为豆包。
- 对照实验：注入 Win+H 未唤起 Windows 语音输入条；该测试受合成焦点限制，不能作为注入链路失效的结论。**勘误（Round 1 A 结构复测）：本轮探针存在 32 字节 INPUT 结构 bug（正确为 40 字节），上述"注入 Win+H 无反应"当时实际未注入任何事件、证据无效；改用正确 per-event 结构（LWin↓→80ms→H↓→60ms→H↑→60ms→LWin↑）复测已可唤起 Windows 语音输入条（OCR 实证"正在聆听"），豆包为活动输入法时同样有效——见 `docs\investigations\2026-09-04-avoid-driver-signing-input-paths.md` 事实区。**

## 根因判定

- 豆包的"长按右 Alt"热键未响应 SendInput 注入的按键。参考实现一致指向该限制：ZSTDJan 为按键/语音注入要求管理员权限 + Frida HID 旁路；Voice_VibeCoding 的语音键注入优先使用 WinUHid 虚拟键盘驱动、SendInput 仅作互斥降级——即先例为保证 IME 热键响应而采用驱动级注入。豆包在 ZSTDJan 文档中亦被明确归入"未验证的自定义程序"类。
- 本仓库边界（AGENTS.md）：基础语音路径不得依赖 Frida、管理员权限或虚拟 HID 驱动。因此不做驱动级绕过；该限制记录为第三方兼容性边界，而非本仓库缺陷。
- **2026-09-04 真机物理对照（passed）**：真人按住物理键盘快捷键（右 Alt / 左 Ctrl+左 Win）时，豆包与微信输入法均可唤起语音输入并显示电平图；同一快捷键经本仓库 SendInput 注入对豆包无效。ADR 0002 前置条件 a（物理按键对照）就此完成：两家 IME 物理按键均可唤起。
- **结论定稿（Round 2 实证闭环；Round 3 重大修正）：豆包注入路线判死（failed）；微信 WeType 注入有效（passed，Round 3 J 翻案）。**
  - **豆包（0.8.2.7）**：行为层——激活态下纯 wVk / scan+ext / InputInjector 注入右 Alt 均无反应；逆向层——ImeService 全局 LL 钩子（VoiceKeyHookProc）回调首查 LLKHF_INJECTED 命中即纯透传（Round 2 G 字节级复现）。Round 2 E 两项补充判决：**RegisterHotKey 路线 failed**——enableGlobalVoiceShortcut 开关 × 显式快捷键 × 服务重启 × 激活态共 7 状态矩阵全部无语音，热键探针全部 FREE（豆包从未注册任何系统热键，其 RegisterHotKey 调用点实为设置页冲突检测）；**physicalize 机制级死刑**——E 三层实证：LL 钩子每钩子收到私有结构副本、修改不跨钩子传播（CallNextHookEx 转发通道不存在，应用层收到原始键），"自家钩子清 INJECTED 标志转发"无法影响下游钩子所见，路线从方案空间删除（Round 3 J 语义复查补充：ZSTDJan 该技巧为结构性 no-op，其真正有效的是未接线的 Frida 版）。
  - **微信 WeType（本机 2.1.3.18）——Round 3 J 复跑翻案：注入有效（passed）**。Round 2 F 的"三配方全无反应"根因是 **TSF 激活用了线程级 flags（dwFlags=0），WeType 从未真正成为活动输入法**（全程豆包在响应；F 窗口快照 wetype 全 vis=False 佐证）。J 先复现该假象，再会话级激活（TF_IPPMF_FORSESSION=0x20000000）后重测：**Ctrl+Win 按住（纯 VK 80ms 配方与扫描码配方均触发）成功唤起 WeType 语音**——wetype_update 开麦（ConsentStore 时间戳铁证）+ WeType 吞掉注入的 LWin + 自注入 break key（extra="WTYP"）；**释放后麦克风干净关闭（按住说话生命周期天然成对）**。WeType 不检查 LLKHF_INJECTED（与豆包决定性对照），语音功能已启用、无需登录。HF 免按和弦默认值 [UNVERIFIED]（R4 N 探测：LP 中 tap Shift、Ctrl+Shift+Win tap、3.5s 长按、Ctrl+Win+Alt 四种试法全部被拒——HF 非默认、需 UI 配置，确切默认值因设置面板不渲染无法读出）。遗留复测已于 Round 4 由 N 全部完成（evidence/n/FINDINGS.md）：快速连按成对性 passed（20/20 严格成对，捕获器 ground truth 零泄漏零粘麦）；释放停止延迟 passed（109/115/127ms，≈117ms）；麦克风路由 CABLE Output passed（E2E ×2：TTS WAV→CABLE→WeType 云 ASR→记事本 19 字精确转写）；HF 和弦探测 passed（阴性结论）。真正遗留仅两项：HF 和弦确切默认值 [UNVERIFIED]、微信客户端语音测试 deferred（扫码登录墙，复跑协议已备）。
  - **用户实测指引**：此前"左 Ctrl+左 Win 无效"的测试很可能同样踩了激活陷阱——目标文本框内微信输入法必须是当前活动输入法（任务栏输入指示器确认），再按遥控器语音键。

## 处置

- 保留按住说话快捷键功能：对接受 SendInput 注入的目标可用。**应用注入形态修正（2026-09-04）：WeType 要求和弦逐事件注入、两键间隔 ≥80ms（单批零间隔被拒，真机不出字根因），已修复——见 `Bugs\2026-09-04-wetype-zero-gap-injection.md` 与 `docs\investigations\evidence\p\FINDINGS.md`。**
- 免驱动语音输入目标排序（Round 3 修正）：**微信 WeType 注入有效，升为第一优先注入目标**（免驱动免管理员纯 SendInput，按住语义天然成对；前提=目标文本框内 WeType 为活动输入法）；**Windows 语音输入（Win+H）**为系统级备选（三件套见调查文档，语音服务层需健康主机终验）；豆包语音热键注入判死（四层闭环），如需支持豆包只能走 WinUHid 虚拟键盘增强轨（ADR 0002）。
- 真机待验更新（2026-09-04 Round 3；Round 4 N 复测后更新）：① WeType 注入裁决 passed（J 干净复跑翻案）；② WeType 遗留复测已全部完成（N，evidence/n/FINDINGS.md）：快速连按成对性 passed（20/20）、释放停止延迟 passed（109/115/127ms≈117ms）、麦克风路由 CABLE Output passed（E2E ×2，19 字精确转写）、HF 和弦探测 passed（阴性：非默认/需 UI 配置）——真正遗留仅两项：HF 和弦确切默认值 [UNVERIFIED]、微信客户端语音测试 deferred（扫码登录墙，复跑协议已备）；③ RC001/RC003 遥控器真实按键的 LL/Raw 事件形态采集 deferred（采集协议与常驻捕获器已就绪：`Testing\investigation\REMOTE-CAPTURE-PROTOCOL.md`，用户随时按键即完成采集，注意 console 会话前提）。

## 验证

- 自动化：注入边沿、SendInput 快照、快捷键持久化测试通过（2026-09-04）。
- 真机：豆包**注入**唤起 failed（行为 + 逆向 + RegisterHotKey/physicalize 两项判决，四层闭环，见根因判定）；豆包**物理**唤起 passed（2026-09-04 真机对照）；**微信 WeType 注入唤起 passed（Round 3 J 会话级激活 + 注入 ground truth 捕获器独立记录：开麦/吞键/释放关麦全链）**；WeType **物理**唤起 passed（真机对照）；RC001/RC003 遥控器真实按键事件形态采集 deferred（采集协议与常驻捕获器已就绪，待用户按键会话完成）。其余会话管线用例继续按测试手册执行。
