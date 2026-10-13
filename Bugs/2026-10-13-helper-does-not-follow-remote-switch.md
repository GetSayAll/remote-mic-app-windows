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

## 验证

- 助手：`cargo test --manifest-path hardware/RC003/helper/Cargo.toml` **52 passed**
  （含新增 2 例：目标变化判定、按地址选宿主）。
- 应用：`cargo fmt --all -- --check`、`cargo test -p sayall-windows --lib`、`cargo check --workspace`、
  `cargo check -p sayall-windows-app --features runtime-simulation` passed。
- **deferred（真机）**：甲→乙换机后，助手日志应出现 `[RETARGET] event=restart`，新实例的 `[REG]`
  应按新目标选到新宿主；随后按语音键应 `path=report_layer`（豆包能出字），且**不需要**关开开关。
  还需要覆盖：换回甲支、连续换机、只断开不换（不应重启）。

## 隐私

- 日志只记事件、原因与计数；目标地址、宿主 pid、实例路径一律不进日志（沿用两侧既有口径）。
