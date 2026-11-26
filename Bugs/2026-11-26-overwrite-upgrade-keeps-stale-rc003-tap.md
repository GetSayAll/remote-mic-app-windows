# 覆盖安装后「全按键支持」永久失败：旧 RC003 运行时与旧 Gadget 被保留

- 发现日期：2026-11-26
- 状态：等待真机验证（覆盖安装路径）
- 影响范围：Windows 1.9.8 用户现场；RC003；全按键支持（增强捕获）。基础 BLE 语音、语音键、基础按键路径不受影响。
- 功能点：安装/升级 → 全按键支持 → 计划任务 `SayAll RC003 Helper` → `WUDFHost.exe` 内 Gadget 注入
- 现象：用户不卸载、直接覆盖安装 1.9.8 后，全按键支持停在「监听失败」；同一版本先卸载再安装则恢复正常。
- 复现条件：用户现场（覆盖安装保留 `%ProgramData%\SayAll\rc003-helper` 与旧 `WUDFHost` Gadget）；开发机未复现。
- 正常预期：覆盖安装完成后，新助手第一次正常启动时使用新 Gadget 完成 `[TAP] resident=0` → `[PREP] copied` → `[HELLO] auth=true`，全按键支持在数秒内可用。

## 证据

用户提供 `sayall-diagnostic(17).log`（时间戳 UTC；本地为 +8）：

- 覆盖安装后的应用 `pid=19864`（`source_revision=89c57ed…`）：
  - 助手 `pid=21388`：`[ATTACH] verify=failed reason=no_authenticated_agent action=restart_host_clean_inject members=1 rc003_members=1 gadget_maps=1 note=常驻 tap 握的是我们接不上的旧令牌（旧世代）`；
  - 应用：`rc003 feature=enhanced-capture action=capture_inactive phase=completed terminal_result=failed reason=agent_never_acked retryable=true`；
  - 助手摘要：`authenticated_ever=false app_bridge=connects=1 … how=restart_host_clean_inject`；
  - 之后再次出现 `verify=failed reason=no_authenticated_agent action=restart_host_clean_inject`，说明宿主重启后仍未能恢复。
- 同一日志里，全新卸载后安装到另一个安装路径的应用 `pid=21584`：
  - `key_host action=start result=passed`；
  - 助手 `[TAP] resident=0` → `[PREP] action=copied` → `[HELLO] auth=true pid=18720`；
  - 全按键支持正常。
- 安装器契约：`NSIS_HOOK_PREUNINSTALL` 会写 `%LOCALAPPDATA%\SayAll\rc003-uninstall-cleanup`（先于停助手），并清理运行时目录；`NSIS_HOOK_PREINSTALL` 原本没有对应动作。覆盖安装不走完整卸载清理，旧运行时目录与旧宿主 Gadget 被保留。

## 根因

已确认：

- 覆盖安装保留了上一代 `%ProgramData%\SayAll\rc003-helper` 运行时（含 `session.token`、`frida-gadget.dll`、`rc003_agent.js`）。
- 新助手看到 `WUDFHost.exe` 里已有旧 Gadget，走 `[ATTACH] skip_injection` / `resident=1`，但旧 Gadget 的 agent 不认识新令牌或已无法回连，于是 `no_authenticated_agent`；这不是令牌文件本身损坏，而是宿主内驻留的旧世代无法被新助手接管。
- 卸载重装从 `[TAP] resident=0` 开始，重新 `[PREP] copied` 并注入新 Gadget，所以恢复。
- 现有 `--uninstall-cleanup` 已包含「释放独占 RC003 宿主 → 恢复设备节点 → 删除运行时目录」；缺的是覆盖安装路径没有触发它。

未确认：

- 同一现场的应用 `pid=19864` 在启动时还出现了 key host 6 次 `accept_timeout`；该现象与全按键支持同场出现，但当前证据不足以证明二者由同一根因导致。用户随后改到另一个安装路径全新安装，安装路径/安全软件/旧进程状态都发生变化，需用带 key host 子进程诊断的包在同路径覆盖升级中复测，见 `2026-11-26-key-host-startup-timeout-gates-all-buttons.md`。

## 修复

1. `src-tauri/windows/installer-hooks.nsh`：在 `NSIS_HOOK_PREINSTALL` 停止旧助手之后写 `%LOCALAPPDATA%\SayAll\rc003-upgrade-cleanup`。路径与助手侧 `upgrade_cleanup_signal_path()` 逐字符一致；标记存在性不依赖安装路径。
2. `hardware/RC003/helper/src/main.rs`：
   - 把 `--uninstall-cleanup` 的清理主体抽成 `perform_runtime_cleanup`，卸载与升级共用；
   - 正常启动（非 dry-run）检测 `rc003-upgrade-cleanup`：提权助手先释放旧宿主/设备环、删除旧运行时目录，成功后删除标记并继续全新注入；清理不完整则保留标记、退出码 3，由应用按桥接快照重试，不带着旧 tap 继续注入；
   - 非提权时保留标记并记录 `deferred_not_elevated`，等计划任务的提权实例处理。
3. `src-tauri/src/lib.rs`：增加安装器契约测试，钉住 `rc003-upgrade-cleanup` 与 `SayAllStopHelper install 0` 的顺序关系。

## 验证

- `cargo test --manifest-path hardware/RC003/helper/Cargo.toml`：92 passed / 0 failed（2026-11-26，`passed`）。
- `cargo test -p sayall-windows-app --lib installer_marks_upgrade_for_a_clean_helper_runtime`：1 passed（`passed`）。
- `cargo fmt --all -- --check`、`cargo fmt --manifest-path hardware/RC003/helper/Cargo.toml -- --check`：`passed`。
- 开发机真实覆盖升级（安装包 `SayAll_1.9.8_x64-setup_upgrade-cleanup-r3.exe`，SHA-256 `0bfbcf10…`，内嵌修订号 `c9bd6d25`，覆盖到已有旧运行时的 `%LOCALAPPDATA%\无线麦 SayAll`）：`passed`。
  - 安装器退出码 0，并写出 `%LOCALAPPDATA%\SayAll\rc003-upgrade-cleanup`；
  - 应用下一次启动后日志依次出现：
    `[CLEANUP] event=upgrade_cleanup elevated=true` →
    `files_deleted=4 files_pending_reboot=1` →
    `host_release=terminating gadget_maps=1` →
    `host_release=released` →
    `files_deleted=1 files_pending_reboot=0 dirs_deleted=2` →
    `device_recover=ok` →
    `[TAP] resident=0` →
    `[PREP] copied` →
    `[HELLO] auth=true`；
  - 标记在成功后删除；第二次覆盖安装再启动，同样消费一次并恢复，证明幂等。
  - 同轮应用日志 `key_host action=start result=passed`；但这是开发机，**不能代替**用户现场或 RC003 按键验收。
- 用户现场真实覆盖升级 + RC003 按键：`deferred`（需在受影响机器用本包覆盖安装，确认 `[CLEANUP]`、`[TAP] resident=0`、`[HELLO] auth=true` 和实际按键映射）。

## 隐私检查

本文不含个人路径（统一写 `%LOCALAPPDATA%` / `%ProgramData%`）、设备身份、蓝牙地址、语音内容或凭据；用户原始日志不提交仓库。
