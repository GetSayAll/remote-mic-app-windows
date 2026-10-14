# 另一支遥控器宿主里的残留 agent 抢占唯一连接位：界面全绿、语音与报告层合成静默失效

- 发现日期：2026-10-14
- 状态：已修复（自动化 passed；真机复验 passed，见文末「验证记录」）
- 影响范围：0.8.6 及更早（加入宿主 pid 不变量之前的所有版本）；Windows 11、RC003、
  **两支同型号遥控器同时在线**且两个宿主里都留有我们的 Gadget/agent；RC001 与只有一支
  遥控器的用户不涉及
- 功能点：`hardware/RC003/helper/src/main.rs`——agent 准入（`handle_line` 的 `hello` 分支）
  与"结束旧宿主换新宿主"的选择（`refresh_agent_in_host`）
- 现象：用户报「豆包语音输入不行了」——按住语音键无反应（向导⑤步等 `voice_verify … mic=not_observed`），
  而所有界面与检查全绿：`voice_attempt … synth_confirmed=true`、助手心跳 `lease_ok=true`、
  IME 检查通过、`capture_resumed`。备用注入因 `chord_press skipped reason=report_layer_synth_active`
  不执行，自动救回 `voice_heal action=focus_bounce` 连做 4 次后到上限，全程无改善。

## 复现条件

- 本机曾对**两支 RC003 的宿主**都做过注入（每支遥控器一个 `WUDFHost`，两个宿主里各有一份
  我们的 Gadget/agent；幂等的接管路径不会清理另一支的残留）；
- 助手（单实例）同时只服务**一条** agent 连接，连接位被"另一支遥控器宿主"里的残留 agent 取得
  （只要发生一次连接抖动/助手重启，两个 agent 就会重新抢位，谁先连上谁占——今天两次重掷都输）。

## 正常预期

- 只有 `resolve_target` 解析出的**目标宿主**里的 agent 能成为已鉴权会话；
- 外来宿主的握手一律拒绝（折叠日志），连接位留给目标宿主的 agent；
- 任一"结束宿主换新宿主"的自愈路径成功后，新宿主的 agent 必须仍被接纳。

## 证据（`%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`，UTC；本机 +08）

1. 正常窗口 11:32–12:20：按下 15 次 ↔ `[AGENT] msg=synth:frame seen` + `replace ok to=0xe6`
   15 条（session 82–96 全部 `mic=observed`；`clears_ok` 从 55 递增到 70）。
2. 12:23:25–32 连接抖动：`ownership_released/timeout` →（12:23:29）`[WARN] 读取连接失败：
   …os error 10053` → `[DISCONNECT] alive_s=3138 agent=rc003_agent/1 agent_pid=31728`
   → 随后 100+ 条 `[HELLO] auth=true pid=8220` 重连风暴，连接位被 **8220** 的 agent 占住。
   全日志 `[HELLO] … pid=8220` 计 129 次，`pid=31728` 只有 2 次（11:31:10 与 12:53:00）。
3. 12:20:38 → 12:53:08 之间 `synth:frame` **零条**；心跳 `stat` 冻结在
   `synth_hits=1 / target_hits=0 / edges_sent=0 / clears_ok=1`（agent 侧计数器已换成新实例）。
4. 该窗口内 22 次豆包按下（session 97–99、102–110、112–118、119–121）全部：
   `synth_confirmed=true`（`voice_synth_ack … confirm=agent`，**只是配置回执**）→
   `chord_press skipped`（不走注入兜底）→ `mic=not_observed`。
   （session 100/101 是切到微信输入法的对照轮：`path=send_input` 注入路径，正常。）
5. 注册表 `…\WUDFDiagnosticInfo\HostPid` 对账：在用那支 → `WUDFHost` **31728**；
   另一支 → `WUDFHost` **8220**。8220 的注入见上一份日志 `10-13 17:16:44Z [INJECT] pid=8220`
   （当时目标解析落在那一支上，属历史遗留残留）。
6. 恢复时刻：12:52:59 助手重启 → 12:53:00 `[HELLO] … pid=31728` → 12:53:08 `replace ok to=0xe6`
   → 豆包 ConsentStore 记录 12:53:08 开始录音 → session 122/123 `mic=observed`。
7. 交叉证明「不是构建/升级问题」：新构建 `0e26bd8`（PR #253）上 session 119–121（12:50–12:51）
   **仍然全部失败**；恢复只与"哪个 agent 抢到连接位"同时发生。

## 根因

- 令牌鉴权只证明"这是我们自己的 agent"，**不证明"它在正在服务的那个宿主里"**。
  `handle_line` 接受任何令牌正确的 hello 成为已鉴权会话；"外来宿主"的 agent 同样能应答
  `arm/mode/targets/synth` 与心跳（应用侧因此一路全绿），但它所在宿主不承载本支遥控器的报告
  ⇒ 报告层合成（语音键 `0x003E` → 按住说话快捷键）**一次都不会发生**，豆包收不到按键。
- 伴随缺陷：`refresh_agent_in_host` 结束旧宿主后只按"任一新 RC003 宿主"选新注入目标，
  两支同型号在线时可能再次挑到**另一支**——同族的静默失效。

## 修复（最小改动，`hardware/RC003/helper/src/main.rs`）

1. 新增纯判定 `agent_host_matches_target(agent_pid, target_pid)`：`0` = 判据缺失 ⇒ 放行
   （退化为原行为，只减不增）；
2. `handle_line` 的 `hello` 分支：令牌通过后校验宿主 pid 必须等于 `SERVED_HOST_PID`，
   不符即拒绝并断开——`[REJECT] reason=host_pid_mismatch total/agent_pid/target_pid`
   （折叠：前 3 条 + 每 30 条一条），连接位留给目标宿主的 agent；
3. `SERVED_HOST_PID`（AtomicU32）在启动解析目标后写入，并在两条"结束宿主换新宿主"的自愈
   路径成功后更新（干净首注 `restart_rc003_host`、旧代刷新 `refresh_agent_in_host`），
   否则自愈回来的新 agent 会被不变量误拒；
4. `refresh_agent_in_host` 的新宿主选择改为 `refresh_host_candidate`：目标地址已知时只认
   这支遥控器的宿主（地址未知退化为原行为）。

## 验证

- **TDD 先红后绿**：新增 `agent_host_guard_tests` 6 例（外来宿主拒绝且不置已鉴权 / 目标宿主接受 /
  agent 侧缺 pid 放行 / 目标侧未知放行 / 令牌不符仍走原折叠路径 / 刷新宿主选择 6 例）；
  实现前因缺符号与签名不符编译失败（预期原因），实现后通过。
- `cargo test --manifest-path hardware/RC003/helper/Cargo.toml`：**59 passed / 0 failed**
  （基线 53 + 新增 6；两条既有 warning 为 origin/main 存量）。
- `cargo run --manifest-path hardware/RC003/helper/Cargo.toml -- --selftest`：退出码 0；
  新增「宿主 pid 不变量」4 例、「刷新宿主选择」6 例均 `[PASS]`。
- `node hardware/RC003/helper/agent/agent_logic_test.mjs`：55/55 通过（本次未改 agent JS）。
- `cargo fmt --manifest-path hardware/RC003/helper/Cargo.toml -- --check`：passed。

## 验证记录（真机，2026-10-14）

- 环境：本地测试包 `无线麦 SayAll_0.8.6_x64-setup.exe`（SHA-256 `a6da1ab1…e0a3d`），
  内嵌修订 `a3a307f9`；安装校验 helper/gadget 哈希对齐、helper 含新标记
  `host_pid_mismatch`；app exe 与构建产物差 3 字节（已知安装期补丁，复核通过）。
- **修复前复发窗口（同一现场，旧构建，作为对照）**：12:57:12–14 旧助手被 8220 的
  `[HELLO] auth=true pid=8220` 风暴接管（数十条；与 12:23 同型）；随后 13:03:49
  （session 128）、13:04:07、13:12:25 三次按下全部 `mic=not_observed`——该故障在旧版上
  是反复发生的，不是一次性偶发。
- 修复后（新助手 pid 42288 + 新应用 pid 39704）：
  1. 外来宿主**再次第一个来连**（13:18:10.436）→ 被拒绝：
     `[REJECT] reason=host_pid_mismatch total=1 agent_pid=8220 target_pid=31728`；
  2. 目标宿主接管：13:18:10.679 `[HELLO] auth=true pid=31728` →
     `targets:applied` / `synth:applied` → `ownership_resumed`；
  3. 语音键**按一次就成**：13:19:22 / 13:22:02 / 13:22:59 三次按下，每次
     `[AGENT] msg=synth:frame replace ok to=0xe6` + `voice_verify … mic=observed`
     （session 5/6/7），`voice_heal` 零触发；豆包 ConsentStore 记录 13:22:59Z 开始录音。
- 仍未验证（deferred）：刷新/干净首注换宿主后新 agent 被接纳的完整路径（需"宿主里是
  旧代 agent"现场）；跨天/睡眠组合。

## 边界与后续

- 本修复只保证"外来宿主的 agent 不占位"；宿主进程整体重建（target pid 变化）由既有
  `[RETARGET]`/应用侧 `capture_recover`（60s 兜底）与本修复的 `SERVED_HOST_PID` 更新路径覆盖。
- 仍未验证（deferred）：真机"宿主里是旧代 agent → 自动刷新换宿主 → 新 agent 被接纳"路径
  （需旧代 agent 现场）；RC001 不涉及。
- 后续项（本次不做）：应用侧为每次按下加"报告层确实改写了一次"的生效判据（现在只有配置
  回执，ack ≠ 生效）；残留 agent 的主动清理。**第一条当天已落地，见文末「跟进」。**
- 隐私检查：本文不含个人路径、蓝牙地址、语音内容或凭据；宿主仅以 pid 引用，令牌只见指纹。

## 跟进：按下生效判据（2026-10-14 当天落地）

上文「后续项」的第一条已实现（分支 `fix/rc003-agent-host-guard`，提交 `bae95e8`，
协议为加法、不升 `BRIDGE_PROTOCOL_VERSION`）：

- 助手把 agent 的逐次帧结果转成 `F ok` / `F skip <reason>` 转发给主程序；只发 1000 ms
  内的新鲜结果，断线不补发（过期结果补发会伪装成"刚发生的生效证据"，方向最坏）。
- 应用侧：每次"报告层按下"500 ms 后比对按下前后的帧证据快照，落
  `voice_effect … result=replaced|missing`（`reason=no_frame` / `agent_skip`），不生效计入
  连续缺失；对账在连续缺失 ≥2 且过宽限后判 `capture_inactive
  reason=voice_synth_no_effect`（界面既有提示 + 60 s `capture_recover` 自动重来），
  健康优先级改为「没生效」胜「有所有权」——配置级回执不再能独自把界面刷绿。
- 有意不做注入兜底（v1）：豆包对注入键按设计丢弃（`note=tool_ignores_injected_keys`），
  自动改走注入会与报告层双写互扰（已记录风险）。残留 agent 的主动清理仍未做。

真机验证（本地测试包 `无线麦 SayAll_0.8.6_x64-setup.exe`，SHA-256
`f3e4da10…27cdb0`，内嵌修订 `6ae4e709`；helper/gadget 哈希对齐，包内确认含
`voice_effect session=` 与 `synth:frame replace ok` 标记；日志 UTC）：

1. 正向（新助手 pid 24544，含 F 转发的本包构建）：14:03:30/33/37 三次按下（session 11/12/13）每次
   `[AGENT] msg=synth:frame replace ok to=0xe6` → `voice_effect … result=replaced
   window_ms=500` → `voice_verify mic=observed`；无 inactive、无 heal。
2. 负向（换入**不转发 F** 的旧助手 pid 38820、应用不变，模拟"配置全绿但无生效证据"）：
   14:10:41/44/48 三次按下（session 14/15/16）每次都是 `synth_confirmed=true` +
   `chord_press skipped`，而 `voice_effect … result=missing reason=no_frame`
   （`note=tool_ignores_injected_keys`）→ 14:10:57 `capture_inactive
   reason=voice_synth_no_effect … voice_frames_missing_streak=3`
   （同行 `bridge_phase=connected`、`helper_pid=38820`——所有权全绿不再掩盖按下失效）→
   界面出现既有提示「"支持更多输入工具"还没有生效，正在自动重试；如果一直不恢复，
   请把开关关掉再打开。」（PrintWindow 截图留档）→ 14:11:57（+60 s）
   `capture_recover reason=voice_synth_no_effect attempt=1` →
   `phase=helper_restarted terminal_result=passed`。
3. 恢复：换回新助手并重启应用后 14:13:34/37 两次按下（session 17/18）
   `result=replaced`、`mic=observed`；界面提示消失（截图留档）。

- 自动化（合并前）：helper 62 passed（含帧结果解析 3 例）、`sayall-windows`
  356 passed（含 verdict 6 例 + 桥回环 1 例）、`sayall-windows-app` capture 7 例、
  agent logic 55/55、agent selftest 全过、helper `--selftest` 新增「语音帧结果解析」4 例
  `[PASS]`；合并 `origin/main`（`6ae4e70`）后复跑：helper 65、`sayall-windows` 350、
  app 全套 111、fmt / workspace check / runtime-simulation check 全绿。
- deferred：本次负向以"不转发 F 的旧助手"构造证据缺失；"外来 agent 与目标 agent 再次
  抢位"的完整现场依赖自然发生，未能受控重放。宿主里是旧代 agent 的刷新路径仍 deferred
  （同上文）。RC001 不涉及。
- 隐私检查：同本文；未记录设备地址、令牌或语音内容。
