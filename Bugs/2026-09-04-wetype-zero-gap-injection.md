# 微信输入法不触发语音：应用将已验证的逐事件配方合并为零间隔单批注入

## 复现

- RC001 真机连接（用户确认型号；蓝牙广播名"小米蓝牙语音遥控器"，HID 路径 VID_2717/PID_32B8——该 PID 曾按第三方仓库 RemoteMapper 的型号映射误推为 RC003，该映射无实证；**型号双证已闭合：应用连接页显示"小米蓝牙遥控器2（RC001）"（GATT 2A24 Model Number 读数），与用户确认一致**），ATVV 就绪；输出端点 CABLE Input（VB-CABLE）已选择；按住说话快捷键 = 默认"微信输入法（左 Ctrl+左 Win）"。
- 目标文本框（WeType 为活动输入法）按住遥控器语音键说话：应用显示"正在接收遥控器语音"、会话计数增加（2026-09-04 当日 83 次），但微信输入法不变麦克风图标、无文字输出。

## 本机排查证据（2026-09-04，全部为实际执行观察）

- **WeType 从未开麦**：ConsentStore `wetype_update.exe` 最近开麦记录 = 12:02:44（真机物理按键对照测试），用户 76 次会话（12:03–12:45）期间零新增开麦记录——WeType 语音会话从未启动。
- **应用注入在 API 层成功**：76 次会话全部完成计数；注入失败会中止会话且不计数（ble.rs 语义），说明每次 DOWN 的 SendInput 调用都返回了成功。
- **应用注入形态与已验证配方不符**：`send_input_windows.rs` 的 press()/release() 把 [LCtrl↓, LWin↓] 合并在**一次 SendInput 调用**里零间隔发出；而 Round 3 J 在本机实证通过的配方是**逐事件注入、两键间隔 80ms**（VK 与扫描码两配方均如此，n-common-lib.ps1）。Round 4 N 的 E2E"通过"用的是探针脚本注入（ERRATA #4 已声明端到端 deferred），应用真实形态从未对 WeType 做过行为验证——本次用户真机测试即首次 E2E，立即暴露缺陷。
- **P 受控实验（docs/investigations/evidence/p/，两轮，序列 A→B→A→B）**：Notepad 前台 + TSF 会话级激活 WeType（FORSESSION，HRESULT=0）+ wetype.statusbar 全程可见，唯一变量为注入形态：
  - TEST A（应用真实形态：单批零间隔扫描码）两轮均 sent=2/2 全到达但 **mic 时间戳无变化**（WeType 拒绝）；
  - TEST B（逐事件 80ms 同扫描码）两轮均真实开麦并成对关闭（13:26:20.214→13:26:21.707；13:29:21.506→13:29:23.006，1493/1500ms 与 1.5s 按住精确对应）。
  - A 失败→B 通过→A 失败→B 通过的交替序列排除激活时序/状态漂移等解释：WeType 2.1.3.18 不认零间隔单批和弦，两 DOWN 边沿必须在时间上分开（80ms 实证有效）。
- **修复一验证后的第二层缺陷（remote-capture.log 13:46–13:53 取证）**：注入修复版应用产生 7 次间隔注入会话（LCtrl↓→80–144ms→LWin↓，成对释放），但 WeType 对全部和弦无反应（LWin 注入事件对捕获器可见=未被吞、无 0xFC break key）。同时捕获到：**遥控器 F5 物理按下泄漏进和弦窗口**（13:52:32 会话：LCtrl↓=31.968 → F5↓=32.030 → LWin↓=32.112）——WeType 判定三键同按拒绝。对照：13:52:19 用户**物理**按 Ctrl+Win（无额外键）时 WeType 立即吞 LWin 并注入 break key（extra=0x57545950），触发正常。
- **F5 泄漏根因（抑制器模块接线错误）**：仓库同时存在两个抑制器——`key_suppressor.rs`（lib.rs 实际启动）与 `voice_key_suppressor.rs`（旧模块，钩子从未启动）；ble.rs 的会话武装信号误发给未启动的 `voice_key_suppressor::set_session_active`，运行中的 key_suppressor 永远收不到 SESSION_ACTIVE，仅剩的 Raw Input 归因武装路径又因单线程设计在钩子回调内自阻塞（回调内 60ms 等待期间同线程的 WM_INPUT 无法分发）而失效，F5 全部泄漏。

## 根因判定

两层独立缺陷叠加：

1. **零间隔单批注入被 WeType 拒绝**（首层）：WeType 的语音热键监听要求和弦按键在时间上分离地到达；单次 SendInput 批量提交的两个 DOWN 边沿零间隔，被 WeType 视为无效和弦（无吞键、无 break key、无开麦）。与注入标记无关（Round 3 J 已证其不过滤 LLKHF_INJECTED）。
2. **遥控器 F5 泄漏破坏和弦（次层，修复一暴露）**：注入间隔修复后和弦形态正确，但遥控器语音键的 HID F5 按下未被抑制器吞掉，落在 LCtrl↓ 与 LWin↓ 之间，WeType 以"额外按键"拒绝整个和弦。泄漏原因为抑制器模块接线错误（会话信号发给未启动的旧模块）。

均属本仓库实现缺陷，非第三方边界。

## 处置

- 修复一（注入形态）：`send_input.rs` 新增 `send_key_edges_spaced_with(events, gap, sender)`——逐事件提交、事件间 `HOLD_CHORD_EVENT_GAP`（80ms，实证值）间隔、保持失败回滚语义（单事件零送达即回滚已送达键）；移除单批 `send_key_edges_with`。`send_input_windows.rs` 的 press()/release()（按住说话快捷键专用路径）改用逐事件间隔提交；按键映射 tap 路径保持单批不变。
- 修复二（抑制器接线）：ble.rs 两处会话武装改接 `crate::key_suppressor::set_session_active`（lib.rs 实际启动的抑制器）；删除未启动的旧模块 `voice_key_suppressor.rs`（已移入系统回收站），消除同名双模块接线陷阱。
- 残余风险（记录；2026-09-04 加固版已处理前两条）：① key_suppressor 单线程 Raw Input 归因在钩子回调内自阻塞，"首个 F5 早于 ATVV 会话激活"窗口依赖会话信号兜底——**已修复**（Raw Input 拆独立线程）；② LL 钩子链序依赖：应用钩子必须晚于 WeType 钩子安装才能在其之前吞 F5——**已加固**（每次会话开始 + 每 10 秒定时把钩子 bump 回链头，先挂新钩再卸旧钩，VVC 技巧）。

## 验证

- 自动化（passed，2026-09-04）：修复一后 `cargo test -p sayall-windows` 39+1 通过（新增 `spaced_edge_submission_sends_one_event_per_call_and_rolls_back_on_failure`）；修复二后工作区全量通过（sayall-windows 35+1，旧模块 4 测试随模块移除）。
- 配方级（passed，evidence/p）：逐事件 80ms 配方两轮触发 WeType 开麦并成对关闭。
- 修复一真机（failed→定位修复二）：注入修复版 7 次会话和弦形态正确但 F5 泄漏致 WeType 拒绝（remote-capture.log 取证）。
- 修复二真机（passed，14:24–14:25，remote-capture.log LL3446–3477）：7 次会话 F5 全吞（日志零 F5 泄漏）、和弦全部逐事件间隔注入并成对释放；微信内语音输入触发（Weixin.exe 开麦 14:25:48–50 与按住/松开精确成对，ConsentStore）。
- **完整链路真机验收（passed，2026-09-04，RC001 真机 + 用户实听确认）**：按住遥控器语音键对遥控器说话 → 微信输入法语音面板出现 → 松开后**文字上屏**。链路前提两条均为用户侧配置：① 输出端点选 CABLE Input（应用内设置，已持久化）；② **系统默认录音设备 = CABLE Output**（应用按测试契约不修改系统默认设备，需用户在声音设置切换或用 evidence/f 脚本切换；本机 2026-09-04 由 P 以 IPolicyConfig 切换并复核，Realtek 可随时切回）。
- RC003 真机验收：**deferred**（本机仅有 RC001 真机；按仓库规则双型号须分别真机验收后才可宣称通过）。
- 加固实现（2026-09-04，同日第二版部署）：钩子链头 bump（每次会话开始 + 每 10 秒定时，先挂新钩再卸旧钩消除空窗，VVC 技巧）+ Raw Input 归因拆独立线程（修复单线程回调内自阻塞，"首个 F5 早于会话激活"窗口恢复生效）。残余风险 ②（钩子链序）就此闭合；残余风险 ①（归因自阻塞）修复。
- 加固版真机回归（passed，2026-09-04 14:52 部署版，用户确认）：按住遥控器语音键 → 松开 → 文字上屏功能正常，加固未引入回归。
- 型号复核（passed）：应用连接页显示"小米蓝牙遥控器2（RC001）"（GATT 2A24 读数），与用户硬件确认一致。
- 未尽项（记录）：快按 <500ms 无 ATVV 会话的行为、断连/睡眠恢复后的成对清理复验、RC003 真机。
