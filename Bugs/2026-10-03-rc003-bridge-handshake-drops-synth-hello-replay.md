# 助手握手读丢弃 `OK` 行后的余量：重启/升级后语音键报告层合成不生效（豆包语音条出不）

- 发现日期：2026-10-03（现场日志段 2026-10-02T16:09Z–16:13Z，本地为 10-03 00:09–00:13）
- 状态：已修复（本机单测 + 助手自检 passed；**真机复测 deferred**——需换含修复的构建）
- 影响范围：Windows 11、0.5.0 本地测试包（`source_revision=6693a53…`）、RC003、豆包输入法；
  触发面 = 「全按键支持」开启时的按住说话快捷方式路径（报告层合成）；应用每次重启/升级后的首次使用。
  RC001、未启用增强捕获的基础路径不受影响
- 功能点：主程序 ↔ 提权助手的应用桥接握手（`OK` 行后紧跟的 `S <usage>` 合成配置）；
  `report_layer_synth_active` 门禁与报告层语音键合成（`0x003E → 0x00E6`）
- 现象：重启/升级应用后，按遥控器语音键多次都拉不起豆包语音条（应用日志写着"已在报告层收到合成，跳过注入"），
  而报告层实际没把语音键改写成右 Alt——那一按什么都没送出去
- 复现条件：应用退出（如安装器 `installer_requested_exit`）→ 助手按 fail-open 把 agent 的合成配置回落为关闭 →
  应用重启并重连助手；此后输入工具=豆包时按语音键（不重新点选输入工具）
- 正常预期：应用把"开启合成"的 `S` 行下发后，助手应交给常驻 agent；
  每次按下语音键都应在报告层替换为右 Alt（`synth:frame replace ok`）并唤起豆包语音条

## 证据

`%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`（UTC；本地 = UTC+8）：

- `16:09:21.238Z` 应用 `app_exit graceful_exit_signal reason=installer_requested_exit`（装包退出）；
  `16:09:22.357Z` 助手 `[APP-BRIDGE] event=disconnected_targets_cleared` →
  `16:09:22.560Z` `[VOICE-SYNTH] event=agent_notified to=off` + agent `synth:off`（合成被回落关闭）
- `16:11:22.987Z` 新应用启动；`16:11:28.950Z` 应用
  `rc003_bridge event=helper_authenticated helper_pid=24580 …` 且同毫秒
  `rc003_bridge event=voice_synth_sent to=0x00E6 scope=hello active=true`
- **助手侧此后没有任何 `[VOICE-SYNTH] event=configured` 记录**；agent 侧计数冻结
  （`synth_hits=7`、`clears_ok=8` 从 15:54Z 一直停到 16:13:43Z）
- 四次按键全部落空：`16:11:39.103Z`(session=20)、`16:11:43.619Z`(21)、`16:12:16.951Z`(22)、
  `16:12:20.339Z`(23)——应用均为 `chord_press result=skipped reason=report_layer_synth_active`
  （以为报告层会送右 Alt，跳过自己的注入），而报告层无任何 `synth:frame`（没有替换、也没有清键增量）
- 对照（同一晚）：`16:13:43.153Z` 用户重新点选输入工具 → `16:13:43.158Z`
  `voice_synth_sent to=0x00E6 scope=update active=true` → `16:13:43.176Z` 助手
  `[VOICE-SYNTH] configured to=0x00E6` → `16:13:43.344Z` agent `synth:applied` →
  `16:13:57.628Z` 起每次按键都有 `synth:frame replace ok`
- 全日志复核：所有 `voice_synth_sent … scope=hello`（最早可追到 10-01）都**没有**对应的
  助手 `[VOICE-SYNTH] configured` 记录——即该重放路径从未真正生效过

## 根因

- 已确认事实：主程序在鉴权后把 `OK <version> <generation> <usages>` 与 `S <usage>`
  **背靠背**写出；助手的握手读函数 `bridge_read_ack` 一次 `read` 常把两行一起拿到，
  却只返回第一行、**丢弃余量**（余量字节已从 socket 读走，主循环的 `read_buffer` 永远见不到）。
  于是"开启合成"只写进主程序自己的状态与门禁，agent 里仍是关闭态；
  应用因门禁为真跳过 SendInput 注入 → 按下语音键没有任何事件送出（豆包语音条不出现）。
  用户重新点选输入工具走的是 update 路径（单独一行、不经过握手读），所以"点一下就好"。
- 此前该失败在助手侧**完全无痕**：无 `[VOICE-SYNTH] configured`、计数无变化、连接正常——
  这也是它长期没被抓住的原因。
- 仍未知边界："一次 read 合并两行"的具体时序占比没有直接观测（本机日志不能给出合并/未合并的判据）；
  但"全日志无一条 hello 重放生效"与代码路径一致，且单测已能确定性复现该丢弃。

## 修复（最小）

`hardware/RC003/helper/src/main.rs`：

1. 新增纯函数 `split_first_line`：返回第一行（含换行）与**其余全部字节**；
   `bridge_read_ack` 改为返回 `(String, Vec<u8>)`；
2. `bridge_connect` 把余量一并返回；`app_bridge_worker` 连接建立时以余量作为
   `read_buffer` 起点（取代原来的 `clear()`），并记
   `[APP-BRIDGE] event=handshake_surplus bytes=N`（补上此前缺失的观测面）；
3. 行解析抽成 `drain_bridge_lines`，与"本次读是否拿到新数据"解耦——
   握手余量在连接建立那一刻即可生效，不再等下一次读（否则仍要等下一次数据到达才解析）。

## 验证

- TDD 红灯（余量修复）：先加 3 例测试，因 `split_first_line` / `drain_bridge_lines` 不存在、
  `bridge_read_ack` 不返回余量而编译失败（缺陷即 API 形状）：`failed`（按预期原因）
- TDD 红灯（门禁加固）：新用例 `voice_synth_gate_follows_agent_ack` 在旧实现上按预期失败
  （"未收到回执前门禁必须保持 false"不成立）：`failed`
- `cargo test --manifest-path hardware/RC003/helper/Cargo.toml`：**21 passed / 0 failed**（新增
  `ack_read_keeps_lines_that_arrive_in_the_same_read`、`drain_applies_synth_line_left_in_the_read_buffer`、
  `split_first_line_waits_for_the_newline`、`parses_agent_synth_ack_states`、`ack_line_encodes_on_and_off`）
- `cargo test -p sayall-windows --lib rc003_bridge`：**22 passed / 0 failed**（新增
  `voice_synth_gate_follows_agent_ack`（无回执不置真 / 未确认重发 / 旧值回执不置真 / 一致回执置真）、
  `parses_synth_ack_lines`、`synth_resend_schedule_is_fast_then_steady`；既有
  `end_to_end_loopback_delivers_edges` 按新协议补 `A -` 确认）
- `cargo fmt --all -- --check` 与 `cargo fmt --manifest-path hardware/RC003/helper/Cargo.toml -- --check`：`passed`
- `cargo run --manifest-path hardware/RC003/helper/Cargo.toml -- --selftest`：全部通过（退出码 0）
- **真机复测 deferred**：换含修复的构建后重跑——安装/重启应用 → 直接按语音键（不重新点选输入工具）→
  豆包语音条出现；日志预期出现 `[APP-BRIDGE] event=handshake_surplus`（多数情况下）、
  `rc003_bridge event=voice_synth_ack … confirm=agent` 与 `synth:frame replace ok`
- **加固（2026-10-03，同日实施）**：门禁不再认"S 行写进 socket"，改为认 agent 回执——
  1. 助手把 agent 的 `synth_ack`（开/关两态）经桥转发给主程序（新增 `A <usage|->` 行）；
  2. 主程序以"回执 == 当前期望值"置/清 `voice_synth_active` 门禁（绝对状态语义；
     旧值/迟到回执只记日志，不改门禁）；未确认期间按 0.5s/1.5s/3s、之后每 5s
     重发 S 行（幂等，确认后停止）——丢行不再变成"静默黑洞"，而是可见失败 + 自愈；
  3. 相关用例按 key_gate 既有约定持 `lock_gate_tests()` 串行（桥的生命周期会改写全局门控）。
- **真机链路实测（2026-10-03 01:39，装包后启动实例）**：`[APP-BRIDGE] event=handshake_surplus bytes=7`
  → `[VOICE-SYNTH] event=configured to=0x00E6` → `[SYNTH-ACK] state=0x003e->0x00e6 to_app=queued`
  → agent `synth:applied from=0x003e to=0x00e6` → 主程序
  `rc003_bridge event=voice_synth_ack to=0x00E6 active=true confirm=agent`。丢行修复与回执门禁在
  真实命名管道 + 在驻 agent（当日 23:45 注入，无需重启 WUDFHost）上均生效；
  **豆包用户可见行为（按键后语音条出现）仍需人工按键确认**（deferred）。

## 隐私检查

本文与新增日志仅含虚拟键码、会话编号与相对时间；不含个人路径（一律 `%LOCALAPPDATA%` / `%ProgramData%`）、
设备身份、蓝牙地址、语音内容或凭据。
