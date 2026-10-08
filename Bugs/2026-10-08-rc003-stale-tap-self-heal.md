# RC003「全按键支持」旧世代 tap 死锁：开关显示已开启、按键却全断

- 发现日期：2026-10-08
- 状态：已修复（自动化 passed；RC003 真机复现 + 自愈 passed）
- 影响范围：0.8.1 及更早版本；Windows 10/11 x64；RC003 增强捕获（「全按键支持」）——表现为按键映射不触发、语音键唤不起输入法语音；RC001 路径不涉及
- 功能点：RC003 增强捕获（提权助手 + 遥控器宿主进程内的 Gadget 接线）
- 现象：升级或重新安装后，「全按键支持」开关显示已开启，但按键没有任何反应
- 复现条件：宿主进程里已有上一轮注入的 Gadget（其 agent 在加载时记下当时的令牌），随后 `session.token` 被重新生成（重装/清理后由助手重建）→ 助手重启后按「令牌文件预先存在」判为可复用、选择接管并跳过注入 → agent 的握手永远被 `[REJECT] reason=token_mismatch`
- 正常预期：拿不到已鉴权握手时应自动升级为「另起一代注入」，并从日志上把「桥已连」与「捕获已生效」分开
- 证据：`%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`（日志时间为 UTC，本机 +08）
  - 修复前同型现场：`2026-10-08T05:36:43Z` / `05:40:57Z` 两次 `[NO-HELLO]` 退出（助手每轮 30 秒后被应用自动拉起）+ 持续的 `[REJECT] reason=token_mismatch` 循环
  - 受控复现（把令牌换成新值后重启助手）：`06:31:50.915Z` `[ATTACH] action=skip_injection`（此时日志写的是"令牌一致"，实际不一致）→ `06:31:51.399Z`–`06:31:54.891Z` `[REJECT] reason=token_mismatch`
  - 自愈（本次修复）：`06:31:58.984Z` `[ATTACH] verify=failed reason=no_authenticated_agent action=inject_new_generation`（距接管 8.07 秒）→ `06:31:59.134Z` `[PREP] … gen-… action=copied sha256_verified=true` → `06:31:59.219Z` `[INJECT] hmodule_return=…` → `06:31:59.220Z` `[VERIFY-MODULE] module_present=true gadget_modules_in_host=2` → `06:31:59.343Z` `[AGENT] msg=targets:applied generation=…`（12 个目标）
  - 应用侧新判据：`06:31:51.092Z` `action=capture_inactive terminal_result=failed reason=agent_never_acked retryable=true` → `06:32:06.093Z` `action=capture_resumed terminal_result=passed owned_usages=12`
- 根因：`decide_plan` 把「令牌文件在助手启动前存在」当成"令牌可复用"（`token_assumed_reusable`），据此选择接管并静默跳过注入；同时应用侧只按桥阶段判定成功（`terminal_result=passed reason=helper_connected`），于是界面显示正常、链路却死着。两处叠加 = 开关亮、按键全断
- 修复：助手侧新增 8 秒接管核验（`ATTACH_VERIFY_MS` + 纯判定 `attach_verification_expired`），核验不过即另起一代复制并注入（注入成功后为新 agent 延长握手窗口；仍失败保留原有 `[NO-HELLO]` 可操作指引）；应用侧新增「已下发目标但 20 秒无任何所有权」的失败日志与恢复日志，并把恢复判据收紧为「确实拿回所有权」。文件：`hardware/RC003/helper/src/main.rs`、`src-tauri/src/lib.rs`（PR #223，合入 `82a1ada`）
- 验证：自动化 passed——助手单测 34、助手 `--selftest` 63 项（含新增「接管核验」判定 6 例）、应用 crate 106、`cargo fmt --all -- --check`、`scripts/ci-preflight.ps1` 7/7；**RC003 真机 passed（2026-10-08，操作人配合）**——受控复现后 8.07 秒完成自愈，操作人确认普通按键与语音键均正常；证据见上文时间戳
- 遗留（已登记）：`[ATTACH]` 原措辞"（令牌一致）"只证明文件存在、不证明一致，已在后续 PR 修正为「按可复用令牌接管；一致性由握手核验」；诊断日志体积问题（实测 768 MB、约 107 MB/天）记入 `TODO.md`
- 隐私检查：全文未包含个人路径（仅环境变量形式）、设备身份、语音内容或凭据；令牌只以指纹形式出现
