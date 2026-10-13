# 换遥控器后报告层不跟随：新遥控器的按键不被改写（语音键"没反应"）

- 发现日期：2026-10-13（Andy 提出："断一支、连另一支，是不是应该重选宿主？"——当场从代码确认成立）
- 状态：已修复（助手侧自愈 + 两侧日志）；**真机复验 deferred**（需要换机 + 提权助手）
- 影响范围：开「全按键支持」时换遥控器；对豆包尤其致命（见下）
- 功能点：助手宿主定位（`hardware/RC003/helper`：`resolve_target` / `Enum` 宿主选择）、
  报告层改写、应用侧 `remote_target` 目标发布

## 现象

断开甲支、连上乙支之后，"按住语音键没反应"；用户把「全按键支持」关掉再打开（助手重启）就恢复。

## 根因

- 助手**只在启动时**按 `rc003-target.ini` 里的目标地址选一次宿主（`resolve_target` 在
  `main()` 里调用一次，主循环与 `serve_connection` 里没有任何复查）。
- 换一支遥控器后，新遥控器由**另一个 `WUDFHost.exe` 进程**承载：旧宿主上继续改写报告
  没有任何作用。
- 应用侧那条兜底（`send_input` 注入右 Alt）对**豆包无效**：豆包的 IME 服务在全局 LL 钩子里
  首查 `LLKHF_INJECTED`，命中即透传丢弃（见
  `docs/investigations/2026-09-04-avoid-driver-signing-input-paths.md`，字节级逆向 + 独立复现）。
  ⇒ 报告层一失效，豆包必不出字；微信输入法不检查 `LLKHF_INJECTED`，所以它不受这条影响。

## 修复

1. **助手侧自动重选宿主（自愈）**：新增目标 watchdog 线程（每 2 秒读一次目标文件）：
   目标地址变成**另一个已能解析出宿主**的值、且该宿主与当前服务的不一样 ⇒ 记一条
   `[RETARGET] event=restart reason=target_host_changed candidates=N` 并**重启自己**——
   新实例走已验证的启动路径重新定位与注入，不在运行中另写一条注入路径。
   纯判定抽出（`target_address_requires_recheck` / `host_pid_for_address`）并单测覆盖：
   断开（清空目标）不动手、重复值不动手、目录写入交错去抖（连续两次看到同一新地址才动）。
2. **两侧日志**：
   - 助手：`[RETARGET] event=restart|restarted|restart_failed`；
   - 应用：报告层没生效且工具是豆包时单列一行
     `voice_path fallback session=N tool=doubao reason=report_layer_inactive note=tool_ignores_injected_keys`
     ——"按住没反应"的报障可一眼归因到路径而不是输入法。
3. 应用侧不需要新协议：目标文件本来就是每次连接/恢复时发布的（日志 `remote_target action=published`），
   助手读同一个文件（同源格式）。
4. **即时路径（同日晚追加，Andy 要求"换完立刻按也要中"）**：主程序发布新目标的同一刻向助手
   发一行裸 `R`（无载荷，与 `T`/`S`/`W` 同族的文本协议），助手置位后**立刻**复查目标并重选宿主
   ——把"换机后约 6 秒内按下赶不上"的窗口压到毫秒级；2 秒轮询兜底保留（请求行丢失/别的原因换机
   仍能自愈），请求路径跳过去抖（请求本身即"目标刚变"的证据）。

## 验证

- 助手：`cargo test --manifest-path hardware/RC003/helper/Cargo.toml` **53 passed**
  （含新增 3 例：目标变化判定、按地址选宿主、`R` 行解析）。
- **真机 passed（2026-10-13 22:55，构建 `5cc5d12`）**：换机后
  `14:55:15.6 remote_target action=published` → `14:55:21.3 [RETARGET] event=restart reason=target_host_changed candidates=10`
  → `14:55:21.4 event=restarted child_spawned=true` → 新实例 `[REG] … 按主程序目标地址消歧
  candidates=2 target_matched=1` → `[HOST] pid=12544 WUDFHost.exe` → `[EXCLUSIVE] verdict=exclusive_rc003_host`
  → `[APP-BRIDGE] connected` → `synth:frame replace ok`；随后两次语音 `path=report_layer`、
  `mic=observed`（豆包出字），全程**未**关开「全按键支持」。轮询路径的这组证据即上表；
  即时路径（`R` 行）为当日晚追加，待下一轮真机复核（预期换机后 1 秒内出现
  `[RETARGET] event=request` + `trigger=app` 的 restart 行）。
- 应用：`cargo fmt --all -- --check`、`cargo test -p sayall-windows --lib`、`cargo check --workspace`、
  `cargo check -p sayall-windows-app --features runtime-simulation` passed。
- **deferred（真机）**：甲→乙换机后，助手日志应出现 `[RETARGET] event=restart`，新实例的 `[REG]`
  应按新目标选到新宿主；随后按语音键应 `path=report_layer`（豆包能出字），且**不需要**关开开关。
  还需要覆盖：换回甲支、连续换机、只断开不换（不应重启）。

## 修复引入的回归与再修复（同日 23:10，真机日志实证）

"重启自己"的第一版是**先起新实例、再让旧实例收尾**，与单实例端口锁（47831）冲突：

```
15:10:45.704 [RETARGET] event=restart trigger=poll
15:10:45.720 新实例启动 …
15:10:45.847 [STOP] 绑定 127.0.0.1:47831 失败 (os error 10048)
15:10:46.120 旧实例收尾退出（SUMMARY / helper_bye）
15:10:49.122 voice_attempt path=send_input → voice_path fallback … tool_ignores_injected_keys
```

净效果是换机后**一个助手都不剩**（比原缺陷更糟）——豆包因此拉不起来。修复：
`bind_listener` 改为**有界重试**（3 s / 150 ms，`[WAIT] event=listen_retry … outcome=bound`），
把交接期的端口竞争让过去；真正的"另一个实例在跑"仍会在预算耗尽后按原样 `[STOP]` 退出，
单实例语义不变。应用侧的 `auto_trigger`（`reason=helper_disconnected`）是这条链路的既有兜底，
本次实证它在 1 秒内也能把助手拉回来（15:10:05 一例）。

**教训**：凡"自我交接"（重启/热替换）必须让**新实例的启动条件与旧实例的退出条件互斥消解**——
这里是端口锁，不能靠"时间上应该来得及"。同类改动必须先在真机做一次"交接观察"，
不能只看单侧日志（本次两侧日志都在，一次拉取即定性）。

## 同日晚追加：令牌不匹配时立即走干净首注（提交 `ab144e9`）

上表"换机到可用"的真机读数里有一段明显白等：新令牌识破发生在 +2.3 秒
（`[REJECT] reason=token_mismatch` 已在滚动），但旧逻辑要等满 `ATTACH_VERIFY_MS`（5 秒）
才决定"接管不了、杀宿主重来"（`[ATTACH] verify=failed reason=no_authenticated_agent
action=restart_host_clean_inject`，+8.8 秒才动手），到新宿主就绪共约 11 秒、可用约 18 秒
（另有一例 62 秒）。

改动：连接线程在握手令牌不匹配时登记宿主 pid（`MISMATCH_HOST_PID`）；主循环若发现该 pid
就是当前服务的 `target_pid`、且接管核验尚未通过，则**立即**走"结束宿主 → 新宿主 → 干净首注"，
日志 `[ATTACH] verify=failed reason=no_authenticated_agent trigger=token_mismatch
action=early_restart_host_clean_inject served_pid=N`。判定分工不变：连接线程只登记事实，
主循环动手（它才知道 `target_pid` 与核验状态）；"同一宿主内再注一代"那条路仍不启用
（真机已验证恢复不了）。预期把换机到可用压到约 12 秒（省掉的那一段白等在真机日志里有明确起止）。

- 验证：`cargo test --manifest-path hardware/RC003/helper/Cargo.toml` 53 passed；rustfmt 已跑。
- **deferred（真机）**：换回"持有旧世代 tap"的那一支，确认 `trigger=token_mismatch` 在
  令牌识破后约 1 秒内出现（而不是 +8.8 秒的窗口到期值），并记录新的换机到可用时长。

## 隐私

- 日志只记事件、原因与计数；目标地址、宿主 pid、实例路径一律不进日志（沿用两侧既有口径）。
