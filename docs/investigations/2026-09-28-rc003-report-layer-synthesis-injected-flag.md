# RC003 报告层合成按键 = 物理按键（LLKHF_INJECTED 实证）

- 日期：2026-09-28
- worktree：`.worktree/doubao-hid-synth`（分支 `doubao-hid-synth`，探针提交 a6424fa + 本轮修复）
- 证据：`hardware/RC003/evidence/synth-doubao-2026-09-28-run{1,4,5}.log`、`elev-diag-2026-09-28.log`
- 探针：`hardware/RC003/probes/wudf_ioctl_synth.{js,py}`、`ll_flag_logger.py`、`elev_open_ladder.py`

## 问题

豆包 ImeService 的全局 LL 钩子首查 `LLKHF_INJECTED(0x10)`，命中即丢弃
（`Bugs/2026-09-04-doubao-hold-hotkey.md` 四层闭环）⇒ SendInput/keybd_event 全灭。
唯一成本可控的候选路径：在 `WUDFHost.exe` 报告层改写 HID 报告（helper 增强轨已有 tap）。
要回答的问题：**报告层改出来的键，到达 Windows 输入流时带不带 LLKHF_INJECTED？**

## 方法（双阳性对照，一次运行内）

载体 = 主页键 0x004A（已知必产报告、有可见后果、释放报告存在），
B 阶段把它的报告改成 0x00E2（RightAlt）。同一观测器（WH_KEYBOARD_LL 读 flags 位）：

    A  物理主页键            → VK_HOME     injected=0   （判"真实"）
    S  SendInput 右 Alt      → VK_RMENU    injected=1   （判"合成"）
    B  报告层 0x004A→0x00E2  → 合成键      injected=?   （被测）

A 与 S 同轮成立才采信 B。宿主为共享宿主（另有两台 BLE 键鼠），
B 阶段按 A 阶段 FileHandle 绑定做设备维度过滤。

## 结果（run5，宿主 42020，18:22）

**9/9 PASS，判定 `synth_physical_equivalent`。**

- B 阶段报告层实际改写 3 次（`0100004a0000000000→010000e20000000000`），
  TAP 改写与 LL 事件时差 2ms——LL 读到的就是被改写的报告本身。
- **合成键 31/31 事件 `injected=0`**（flags=0x0020 仅 LLKHF_ALTDOWN，
  无 0x10 无 0x02）。全日志唯一 injected=1 的事件 = S 阶段 SendInput 对照。
- 安全性：3 次按压经边沿去重后 3 pairs / 0 孤立释放 / 无粘键
  （28 DOWN 中 25 个为 autorepeat，`count_pairs` 按边沿跳变去重）。
  释放报告（全零）不经改写，天然成对——释放沿丢失风险在设计上不存在。
- 主页键被完全替换（B 窗口零 VK_HOME），解除武装后 C 阶段完整恢复。

**结论：报告层合成的按键在 Windows 眼里与物理按键不可区分（至少在
LLKHF_INJECTED 位上），豆包的注入标志门槛对这条路径无效。**

## 过程中的三个发现（比结论本身还值钱）

1. **usage 0x00E2 的实际映射是 VK_LMENU(0xA4)，不是 VK_RMENU(0xA5)**。
   kbdhid 翻译槽位里的 0xE2 时**不带扩展前缀**，win32k 按普通 0x38 扫描码
   映射成左 Alt。产品若采用此路线，合成热键是**左 Alt**；想要右 Alt/其它键
   需逐 usage 实测（`SYNTH_VK_ON_STACK` 常量记录该事实）。
2. **WUDFHost 的 DACL 已收紧**：管理员令牌下 OpenProcess 连
   `PROCESS_QUERY_LIMITED_INFORMATION` 都 err=5（全权限阶梯含 ALL_ACCESS 同拒），
   但 frida.attach 仍成功——frida helper 自己启用了 SeDebugPrivilege
   （管理员令牌默认携带但 disabled）。显式 `AdjustTokenPrivileges` 后全档放行。
   09-23 写入 tap 能过旧守卫，说明收紧发生在这 5 天内（怀疑系统更新）。
   修复：`wudf_host_probe.enable_se_debug_privilege()`，探针守卫先开特权再探测。
3. **产品运行时会污染宿主实验（run4 教训）**：SayAll app+helper 在跑时，
   产品 agent 持有所有权租约会清空 RC003 报告（全按键支持的工作方式），
   且我们注入的右 Alt 恰是产品「按住说话」默认热键——run4 出现
   LL 全程零事件、B 阶段 330 次 IOCTL 命中零 0x4A 报告的假象。
   **任何宿主探针实验前必须退出产品 app+helper。**

## 自伤记录（不再犯）

- run2/run3 死于同一文件并行 Edit 互相覆盖（import 丢失 → NameError），
  traceback 只在被提权的控制台、窗口一关证据全无。已加护栏：log() 的
  print 失败不中断、main() catch-all 把 traceback 写进 --out 日志、
  stdout 强制 UTF-8。**同文件多 Edit 必须串行——这条早已在备忘里。**

## 对豆包支持路线的含义与下一步

- 门槛已破：合成键不带注入标志。剩余工作全部是工程集成而非机制攻关。
- **豆包行为验证已通过（2026-09-28 晚，`synth-doubao-2026-09-28-doubao-behavior.log`）**：
  - 新增 `--synth-usage` 参数支持 usage 0x00E6（HID RightAlt），
    **实测映射 VK_RMENU(0xA5)**——豆包默认热键「长按右 Alt」零配置直通，
    无需改豆包设置。映射表 `USAGE_TO_VK` 随探针固化（0xE2→0xA4、0xE6→0xA5）。
  - B 阶段按住主页 → 报告层合成 0xA5：LL 侧 123 个 DOWN（含 121 个
    autorepeat）**全部 injected=0**；Andy 目视确认**豆包语音条弹出、
    松开停止聆听**——「按住说话」生命周期天然成对。
  - S 阶段（SendInput 真 0xA5、injected=1）与 B 形成**同 VK、只差
    INJECTED 位**的完美对照：S 被豆包忽略、B 被豆包响应，
    与 bug 文档四层闭环（LL 钩子首查 LLKHF_INJECTED 透传）完全吻合。
  - B 阶段伴随 2 对 VK_NONAME(0xFC) 成对事件 + 2 个孤立 UP——疑似豆包
    ImeService 处理合成键时的自注入/吞键行为（类比 WeType 自注入
    break key），不影响判活；探针结束时无粘键残留。
- 产品集成（下一步）：helper 增强轨加「热键合成」分支——语音键按住期间
  在报告层写入目标 usage（默认 0x00E6→右 Alt），释放写全零；
  与现有三键清空逻辑共享同一 tap，无新增驱动；usage 与产品热键配置联动。
- 边界：本结论来自单机单日；kbdhid 映射随系统版本可能变化，产品验收时
  需按验收手册在真机上重跑本探针确认 `USAGE_TO_VK` 仍成立。

## 2026-09-29 凌晨：helper 死亡排查与产品集成真机诊断（run6/run7）

### 基础设施补盲（已提交 5f666d9 / f12b573）

- **panic 落盘 hook**：Rust panic（unwind）是干净退出（码 101），不触发
  WER/事件日志；`--hide-window` 下 stderr 无人可见 → run4/run5 的
  「日志停在 HB 中间、无收尾、无事件记录」正是这个形状。`set_hook`
  保留 stderr 并把同一份信息写进 `--log`（无 --log 时落
  `runtime_dir/helper-panic.log`）。边界：TerminateProcess/abort 仍无痕迹，
  退出码只能靠启动器 `-PassThru -Wait` 观测。
- **--synth-from/--synth-to 槽位覆盖 bug**（f12b573）：原
  `take().unwrap_or((parsed, 0))` 把单 flag 调用变成清掉另一槽位
  （run3 实测 `synth=0x003e->0x0000`）。
- **事件日志排查记录**：Application 日志 4h 窗口内无 helper 崩溃记录；
  唯一相关事件是 `CodexSandboxService` 21:23 的重启通知——与本轮
  helper 死亡时刻（00:15~00:22）不吻合，codex 嫌疑排除。

### run6（00:44，--duration 900）：helper 死亡未复现 + 新现象

- 带 panic hook 跑满 900s，`EXITCODE=0`、`[TIMEUP]` 正常收尾、PANIC=0。
  **run4/run5 的死亡未复现**——当时的差异是 app 还在跑（PID 5720，
  本轮发现 app 已退出）；死亡是否与 app 桥接交互相关待复现验证。
- **新 bug：agent 静默**。`gadget_modules_in_host=3`（三代并存），
  新 agent HELLO/CONFIG 正常、回了 `mode:clear`，此后 900 秒
  **零上行**（无 HB、无 renew:first、无 targets_ack、无 dropped:*
  重连日志），TCP 单连接保持 ESTABLISHED（netstat 实证，排除重连
  循环），helper 侧 `[TARGETS-RESEND]` 重发 95 次全空。

### run7（01:07，宿主清零后单实例注入）

- 提权结束 WUDFHost(42020) 让设备重枚举 → 0 Gadget → 单实例注入。
- **agent 完全正常**：HB 每 500ms、lease_ok、renew_age_ms 新鲜
  （agent 在收 renew），且当时 **targets 是空集**（app 不在）——
  **排除「空 targets 杀死 agent」假设**；run6 的静默最大嫌疑收敛到
  **3 Gadget 实例 hook 堆叠**（3 层 NtDeviceIoControlFile 回调链）。
- **未解之谜（保留 instrumentation）**：CONFIG 批次里 agent 只回了
  `mode:clear`，`targets`/`synth` 的 ack 四轮全缺（run4~run7 100% 复现），
  而 renew（批次之后 500ms 周期下发）全部被处理——已给 agent
  `handleCommand` 加命令级日志（非 renew 命令到达即记
  `cmd:<type>`，JSON.parse 失败记 `cmd:parse_failed len= head=`），
  下一轮真机直接区分「字节没到 / 到了但损坏 / 处理了但 ack 没回」。
- 环境：run5（桥 connected + targets 非空）agent 活；run6（桥失败 +
  空集 + 3 实例）agent 死；run7（空集 + 1 实例）agent 活——
  **产品路径（桥连上 → 非空 targets）有 run5 正面实证**。

### 下一步

1. run8：带 cmd 级日志的 agent 重跑（先 app 后 helper 或反之按 UAC
   时序），核对 `[TARGETS-ACK]` / `[SYNTH-ACK]` / HB `stat.synth_applied`。
2. Andy 目视端到端：按住语音键 → 豆包语音条弹出、松开停止。
3. TODO.md 条目仍不可勾：完成定义要求 RC001/RC003 双真机验收。

## 2026-09-29 01:38：端到端验证通过（RC003 + 豆包，Andy 目视）

### 首测失败 → 双写根因（01:32）

首次按语音键：**输入法自动切到微信**，豆包失效。app 诊断日志实锤
双写——语音键按下瞬间 app 从 BLE 层（`gatt packet [00 02]`）走了
「按住说话快捷键」路径注入 **Ctrl+Win 和弦**（`chord_release`，
v1 默认配置适配微信输入法默认语音热键，settings.rs:189），与 helper
的报告层合成（右 Alt）叠加：app 注入的键带 INJECTED 位（豆包忽略，
S 对照实证）且触发微信语音热键 → 输入法被切走。这与「同键双写」
家族同型：**开关语义横跨两层（BLE/ATVV 层 vs 报告层）时只关一层
另一层还在依赖**。

### 修复与通过（01:38）

Andy 在 app 设置清空「按住说话快捷键」→ helper（run8）报告层合成
独占。Andy 目视确认：**豆包语音条弹出、松开停止、输入法保持豆包、
文字出现**。证据：
- `evidence/helper-run8-doubao-e2e.log`：按键窗口 ioctl_calls
  306→1550（+1244 帧报告经过），clears_ok=4/kernel_changed=4
  （4 帧语音键报告被槽内替换为 0x00E6）。
- app 日志 01:38:53 起 `audio_stream phase=started
  endpoint_kind=virtual_cable`（ATVV 音频正常），**无 chord_release**。
- 生命周期判据天然成立：物理释放 → 报告全零 → 合成键消失 →
  豆包停止聆听（HID 状态语义，无补帧无状态跟踪）。

### 结论

报告层合成（injected=0）在产品形态下端到端可用：helper 增强轨 +
报告层 0x003E→0x00E6 替换 = 豆包「长按右 Alt」零配置直通，且
ATVV 音频路径（virtual_cable）与热键合成并行不冲突。产品集成
剩余工作：① helper 正式构建进安装包（当前安装版 helper 是旧构建）；
② heartbeat 上报补 synth 字段（本次顺手已修）；③ RC001 侧验证；
④ 双真机验收后 TODO.md 才可勾。

### 本轮附带发现（记录待查）

- **CONFIG 批次 ack 缺失**：arm/mode/restore/targets(/synth) 连发的
  批次里，mode 之后的命令 ack 100% 缺失（run4~run7），而批次之后
  500ms 周期的 renew 全部被处理、桥单独下发的 targets 有 ack
  （run8 [TARGETS-ACK] gen=1）——疑似 Gadget socket 层批次化问题，
  已加 `cmd:<type>` / `cmd:parse_failed` 命令级日志待下轮定位。
  不阻断产品路径（桥下发的 targets 正常）。
- **3 Gadget 实例 hook 堆叠**：run6（三代并存）agent 静默 900s
  （TCP 假活、零上行、无 drop 日志）；清零后单实例（run7/run8）
  agent 完全正常。多实例注入是实验性路径，产品化必须避免。
