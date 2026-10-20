# 2026-10-20 切到微信输入后按下语音键没有语音条（合成门禁被 off 回执重新举起）

- 发现日期：2026-10-20（用户现场，23:59）
- 状态：已修复，等待真机验证安装包
- 影响范围：安装版 `1d85e93a`（分支 `feat/private-membership-boundary`），Windows 11 26200，RC003，
  语音输入工具 = 微信（wetype）；豆包路径不受影响
- 功能点：语音键快捷键注入 / 报告层合成门禁（`rc003_bridge` × `key_gate::VOICE_SYNTH_ACTIVE`）
- 现象：语音输入工具从豆包切到微信后按语音键，微信侧不出现语音条（豆包切回来正常）
- 复现条件：增强捕获已接管（agent 存活、`synth_confirmed=true`）→ 工具豆包按一次 →
  设置里切到微信（app 下发 `S -`、agent 回执 `A -`）→ 再按语音键
- 正常预期：微信走注入路径（`chord_press`），wetype 收到快捷键并弹出语音条
- 证据：`%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`，2026-10-20 15:59:05–15:59:13Z
  - `15:59:05.969 enhanced_capture event=voice_synth_configured to=off`
  - `15:59:06.131 rc003_bridge event=voice_synth_ack to=off active=false confirm=agent owned_usages=8`
  - `15:59:06.385 rc003_bridge event=voice_synth_gate active=true reason=ownership`（254 ms 后被打回 true）
  - `15:59:10.793 voice_attempt session=107 tool=wechat path=report_layer synth=off synth_confirmed=true`
  - `15:59:10.793 chord_press result=skipped reason=report_layer_synth_active`
  - `15:59:11.297 voice_effect session=107 result=missing reason=no_frame`
  - 对照：同进程 15:59:17 后的豆包会话 `voice_effect result=replaced`（normal）正常
- 根因：`BridgeLine::SynthAck` 对 `to == None`（关闭）也把 `synth_confirmed` 置 true（这一步为了让
  重发逻辑停下是正确的）；随后 `BridgeLine::Ownership` 分支用
  `synth_gate_active(synth_confirmed, reported.len())` 重算门禁，于是「off 已回执 + 所有权在手」
  把门禁举回 true。门禁举起后注入路径被整体跳过（含 `chord_press`），而报告层此时没有任何合成
  目标、不改写任何键 ⇒ 微信收不到快捷键。等价说法：`synth_confirmed` 混用了两个语义——
  「期望值已被回执」（对 off 也成立）与「报告层现在会改写」（只对 Some 成立）。
- 修复：`crates/sayall-windows/src/rc003_bridge.rs` 的 Ownership 分支门禁判据补上当前期望目标
  为 `Some`：`synth_gate_active(synth_confirmed && lock(voice_synth_to).is_some(), reported.len())`。
  新增回归测试 `synth_gate_stays_down_after_off_ack_and_ownership`（前置 sanity：合成开启时门禁
  必须能举起；off 回执后所有权续约两次，1 秒观察窗内门禁必须保持 false）。
- 验证：
  - `passed`：新测试在修复前失败、红在目标断言（"合成已关闭…不得把门禁重新举起"），修复后通过
  - `passed`：`cargo test -p sayall-windows rc003_bridge` 30/30，连跑三轮无抖动
  - `passed`：`cargo test -p sayall-windows` 366 passed / 0 failed；`cargo check --workspace` 通过；
    `cargo fmt -p sayall-windows -- --check` 通过
  - 观察到一个未复现的 flaky：某轮全量出现 1 个失败，之后两轮全绿且未捕捉到用例名，待后续留意
  - `deferred`：安装包真机验证（豆包语音条 / 微信语音条 / 按键页语音键状态）
- 隐私检查：日志片段只含会话号、usage 与路径枚举，不含设备身份、语音内容或个人路径
