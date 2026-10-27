# 助手固定端口被系统级保留（WSL2 mirrored）导致报告层永不生效

- 发现日期：2026-10-24（issue #296，@jihtsan）
- 状态：已修复，等待真机验证（mirrored 机器 + 安装版助手）
- 影响范围：1.0.0（`a4c1311`）与 main（`70e9e99`）；Windows 11 26H2 x64；RC003；WSL2 `networkingMode=mirrored`；豆包输入法
- 功能点：RC003 增强捕获（报告层）与语音键路径
- 现象：按遥控器语音键后键盘出现 F5 KeyUp（部分场景还有 F5 KeyDown 泄漏）；豆包不开麦；诊断日志
  `rc003_bridge event=closed reason=voice_synth_write_error` 每 ~20 s 一次，`synth_confirmed` 从未变 true
- 复现条件：WSL2 `.wslconfig` 启用 `networkingMode=mirrored`，且 Linux 侧 `ip_local_port_range` 覆盖
  47831。该区间每次启动随机，因此同一台机器上表现为**时好时坏**
- 正常预期：助手一次启动即常驻，`[LISTEN] 127.0.0.1:47831` → `[HELLO] auth=true`，语音键走报告层合成
- 证据：issue #296 的根因链与"占用者不可见"四组对照（`netstat -ano` / `-anoq`、`Get-NetTCPConnection`
  （含管理员）、TCP 实连、`show excludedportrange` 全部看不到占用者）；PR #297 的真机日志；
  `Testing/WindowsRC003EnhancedCapture.md`「2026-10-25 增量」
- 根因（已确认）：WSL2 mirrored 把 Linux 侧 `ip_local_port_range` 整段镜像成**宿主内核级保留区** ⇒
  `bind 47831` 恒得 `WSAEADDRINUSE(10048)`，而保留发生在 HNS/vfp 层、不体现为任何 socket ⇒ 有界重试
  （3 s / 150 ms）耗尽后 `[STOP]` 退出码 8 ⇒ 主程序 auto_trigger 每 20 s 再拉起，形成崩循环 ⇒ 报告层
  永不生效 ⇒ 语音键回落 `SendInput`（注入右 Alt 带 `LLKHF_INJECTED`，豆包按设计丢弃）+ 原生 F5 按下沿泄漏。
  仍未知：Windows 侧保留区与 HNS 规则的精确生命周期（重启 WSL 是否立即释放未逐次验证）
- 修复：
  - 首版（PR #297，提交 `668e5a5`）：固定端口预算耗尽且错误为 10048 时回退 `port 0` 由系统分配，
    实际端口经 gadget `parameters.port` 下发，agent 连接机制不变
  - 收口（PR #304）：单实例改用命名互斥体 `SayAll.RC003Helper.SingleInstance`（端口不再兼锁）；
    回退前先做 `connect` 探针（只有 `refused` 才回退）；回退端口写 `session.port` 跨重启复用
    （agent 端口在注入时写死，端口一变接管核验必空等 8 s 后重建宿主）；`BindError` 结构化；
    `args.port` 一律取 `listener.local_addr()`（顺带修好 `--port 0` 把 0 写进配置）
  - **第三段（2026-10-27，issue #291 用户追问"能不能换个端口"）**：#304 的"只有 `refused` 才回退"
    把"端口被**真实进程**占着"的机器仍然锁在崩溃循环里——而单实例早已由互斥体保证，
    `refused` 这个门禁失去依据（回退是让系统另给一个端口，并不会顶掉占用者）。改为查占用者
    （`GetExtendedTcpTable` + `QueryFullProcessImageNameW`）后分类：`holder=own_helper`
    （另一支 sayall-helper，多为旧版本没退干净）才 `[STOP]` 退出码 8，`other_program` /
    `unknown` / `nobody` 一律回退。日志新增 `[PORT] event=holder … decision=fallback|stop`。
    同时 `startup_failing` 的用户文案不再要求用户"去找是哪个程序"（他无从定位）
- 验证：自动化 `passed`（助手 `cargo test` **82 passed**，含本轮新增 3 项——回退真值表、
  占用者分类与 token、以及**真调用 Win32** 的自监听→查 PID→读映像路径用例；
  `cargo fmt --check` 通过）；**真机 `deferred`**——mirrored 机器上的首次回退与"第二次启动"
  回归，判据见 Testing 文档「2026-10-25 增量」（重启后应见 `[FALLBACK] … reused_remembered=true`
  且数秒内 `[HELLO] auth=true`，不得出现 `[ATTACH] verify=failed`）
- 隐私检查：本文只引用 usage、端口号与错误码，未包含设备身份、个人路径、语音内容或令牌

## 归因

- **问题定位与首版修复**：@jihtsan（joo00der）——issue #296 提交了完整根因链与诊断路径，PR #297
  （提交 `668e5a5`）给出端口回退方案并完成 mirrored 机器上的真机验证。本 Bug 最难的部分（把
  "端口绑不上" 从 "上一轮还在跑" 里区分出来，并追到 WSL2 mirrored 的内核保留区）是这条工作线完成的。
- **收口补强**：PR #304 在 `668e5a5` 之上补单实例互斥体、探针门禁与端口复用；`668e5a5` 原样保留在
  #304 的历史里，合入时随提交一起进入 main。
