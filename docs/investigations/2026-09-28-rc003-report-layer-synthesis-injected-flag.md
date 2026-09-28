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
