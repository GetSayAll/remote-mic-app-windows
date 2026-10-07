# ARM64 上「全按键支持」永久停在「正在启动」：x64 助手与 Gadget 无法用于 ARM64 宿主

- 发现日期：2026-10-06（Issue #206 提交）；2026-10-07 完成核验
- 状态：主问题**已修复并经 ARM64 真机实测通过**（2026-10-07，报障人在 RC003 上执行）；
  之后在升级路径上又发现并修复一个后续缺陷（计划任务目标架构对账，见本文「后续修复」）——
  见 [ADR 0003](../docs/decisions/0003-arm64-enhanced-capture-scope.md)「实施记录」
- 影响范围：Windows 11 家庭版 ARM64（内部版本 26200，Snapdragon X Elite）；应用
  SayAll Windows 0.5.0（发布 tag `v0.5.0` = `e5374a9`）；RC003；「全按键支持」（增强捕获）。
  基础语音与其它按键路径不受影响（报障人确认其它按键正常）
- 功能点：按键页 → 全按键支持 → 计划任务助手 → 注入承载遥控器的 `WUDFHost.exe`
  （捕获链第 ① 段）
- 现象：开关打开后界面长期停在「全按键支持已开启，正在启动」；「返回 / 音量+ / 音量−」
  三键自安装以来零事件（诊断日志中一次都没有出现）
- 复现条件：ARM64 系统 + 已注册计划任务 + 开启「全按键支持」；报障人侧 18 次尝试全部失败
  （约等于 18 次应用启动，每次启动最多落一条该失败行）
- 正常预期：开关开启（或启动自动触发）后数秒内完成 `helper_authenticated`，界面转为
  「全按键支持已开启」，三键边沿经桥转发到主程序并触发映射动作

## 证据

### 1. 报障人日志（`%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`）

```
[STOP] CreateRemoteThread 失败，GetLastError=6（安全软件拦截注入时常见）

rc003 feature=enhanced-capture action=auto_trigger_escalate phase=completed
terminal_result=failed reason=helper_still_not_connected retryable=false
bridge_phase=listening helper_pid=0
```

### 2. 代码核验（`origin/main` = `613dac9`；v0.5.0 在注入链路上相同）

| 事实 | 证据 |
| --- | --- |
| 那条 `[STOP]` 只可能从 `inject_gadget()` 抛出；同函数内每一步失败都有各自文案，因此失败点在 `CreateRemoteThread`，其前的 `OpenProcess` / `VirtualAllocEx` / `WriteProcessMemory` 均已成功 | `hardware/RC003/helper/src/main.rs:2183-2263`（v0.5.0 `:2174` 为同一行文案） |
| 注入失败后助手以退出码 9 结束，因此不可能走到鉴权，主程序侧必然是 `helper_pid=0` | `hardware/RC003/helper/src/main.rs:4403-4409` |
| 助手与主程序写同一个诊断日志（报障人能看到该行即证明助手确实运行过） | `hardware/RC003/helper/src/main.rs:1196-1215`（`resolve_shared_log`） |
| Gadget 被钉死为 x64：校验 PE machine 必须为 0x8664 | `hardware/RC003/helper/vendor/fetch_frida_gadget.py:153-156` |
| 运行时只比对单一 SHA-256（即 x64 那份），换架构必须同时改常量与锁定文件 | `hardware/RC003/helper/src/main.rs:6404-6424`、`vendor/frida-gadget.lock.json:8-33` |
| 打包与发布只产出 x64：resources 为固定裸文件名；更新清单平台键与资产名写死 x64 | `src-tauri/tauri.conf.json:63-66`、`scripts/generate-updater-manifest.ps1:44-67`、两个 CI workflow 无矩阵与 `--target` |
| 运行时**没有**任何架构门禁：启动期唯一环境门是 Windows 版本号；助手定位只查文件存在 | `crates/sayall-windows/src/compatibility.rs:3`、`src-tauri/src/rc003_task.rs:54-84` |
| 唯一的「架构」字段是编译期常量，在 ARM64 上仍打印 `x86_64`（误导） | `src-tauri/src/lib.rs:1989-1993`（`std::env::consts::ARCH`） |
| 失败后不再重试：启动时一轮、最多 5 次触发约 25 秒，随后写死 `retryable=false` 并放弃 | `src-tauri/src/lib.rs:591-592`、`:597-683` |
| 界面无超时也无失败提示：`failed` 相位只表示桥自身 bind 失败；助手连不上时桥恒为 `listening`，前端只有「正在启动」一个文案 | `crates/sayall-windows/src/rc003_bridge.rs:639-645`、`src/pages/ButtonsPage.vue:1125-1149` |
| 死掉的恰是增强捕获的默认目标 usage（0xF1 / 0x80 / 0x81 = 返回 / 音量+ / 音量−） | `hardware/RC003/helper/src/main.rs:271` |

### 3. 报障人自测（PE machine）

- 三个 SayAll 组件均为 x64（0x8664）：主程序、助手、`frida-gadget.dll`；
- 注入目标 `C:\Windows\System32\WUDFHost.exe` 为 ARM64（0xAA64），WinSxS 中也只有 arm64 包。

### 4. 证据缺口

报障人引用的失败行比实际格式少了 5 个对账字段（`bridge_port` / `accepted_total` /
`denied_total` / `replaced_total` / `malformed_total`）；v0.5.0 必然输出全部 7 个字段
（`src-tauri/src/lib.rs:699-707`，v0.5.0 同）。缺的这几个数字是「助手到底有没有连上过」
的唯一分流依据，回复时需请报障人补发完整行。缺失不影响本次判定：`helper_pid=0` 与
`bridge_phase=listening` 已足以说明当时没有已鉴权的助手。

## 根因

**已确认（结构性，与安全软件无关）：**

1. 产品链路只产出 x64 助手与 x64 Gadget（见上表三条硬断言）。
2. ARM64 上的注入目标只能是原生 ARM64 的 `WUDFHost.exe`；ARM64 进程无法加载普通 x64 镜像
   （微软 Arm64X 文档：要同时服务 Arm64 与 x64/Arm64EC 进程必须构造成 Arm64X/Arm64EC）。
   即使 `CreateRemoteThread` 成功，`LoadLibraryW` 也必然失败——该路径在 ARM64 上不可用。
3. 产品没有架构前置校验，也没有「不支持」的提示或降级：照常安装、照常拉起 x64 助手、
   照常在约 25 秒后静默放弃，界面因此永久停在「正在启动」。

**仍未确认（`deferred`）：**`GetLastError=6`（ERROR_INVALID_HANDLE）的精确归属——内核因跨架构
拒绝，还是安全软件特有的拦截返回值，两者都可能给 6。没有 ARM64 真机可复现，外部资料亦未查到
定论。该点不影响结论：即使线程建起来，ARM64 宿主也加载不了 x64 Gadget。

**已知误导项（需在修复中一并处理）：**助手把该失败归因成「安全软件拦截注入时常见」
（`hardware/RC003/helper/src/main.rs:2261`），在跨架构场景下会把排查引向错误方向——
报障人据此排除了安全软件，判断方向反而是对的。

## 修复

按 [ADR 0003](../docs/decisions/0003-arm64-enhanced-capture-scope.md)（2026-10-07，Andy 拍板）
落地，分支 `feat/arm64-enhanced-capture`（实施明细见该 ADR 的「实施记录」节）：

1. **架构化**：助手的 Gadget 摘要、锁定文件架构 token、随包文件名、注入器 machine 按编译架构
   取两份；锁定文件增 arm64 条目（真实 SHA-256 与 PE machine 已核对）；fetch 脚本支持
   `--arch` / `--all` 并校验 PE machine。
2. **注入前闸门**：`inject_gadget` 第一步比对助手 / 宿主 / Gadget 三方架构，不一致立即拒绝并
   给出可操作说明；`CreateRemoteThread` 文案不再首先归因"安全软件拦截"（原误导项已消除）。
3. **应用侧选择与提示**：`os_arch::native_arch()`（`IsWow64Process2` 的 `nativeMachine`）+
   `rc003_task::capture_support()` + 新 IPC `get_capture_support`；启动对账与自动拉起在
   "架构不受支持 / 载荷缺失"时**回落开关、落可分流日志、不进入 25 秒重试**；界面置灰并说明
   原因（前端由同一分支交付）。
4. **打包**：单安装包双载荷（默认 `resources` 仍是 x64 两件，出包时用
   `tauri.arm64-payload.conf.json` 覆盖四件；`SAYALL_REQUIRE_ARM64_PAYLOAD=1` 强制）；
   安装器钩子的进程查找、写锁探测与优雅退出覆盖 `sayall-helper-arm64.exe`。

## 验证

- 本机为 x64（`PROCESSOR_ARCHITECTURE=AMD64`，`native_arch()` 实测返回 `x64`），
  **无法复现 ARM64 环境**：真机部分一律 `deferred`。
- 助手：`cargo test --manifest-path hardware/RC003/helper/Cargo.toml` **33 passed**（含新增
  `arch_tests` 8 例）；`sayall-helper.exe --selftest` **全部通过**（含新增"锁定文件同时登记
  x86_64 与 arm64"）；`--dry-run` 退出码 0，`[VERIFY]` 打出 `machine=x64 0x8664
  arch_expected=x64 0x8664`。
- 进件：`fetch_frida_gadget.py --all` 两份产物的压缩包 / 解压产物 SHA-256 与 PE machine 全部
  匹配（arm64 压缩包摘要与 GitHub 官方 `asset.digest` 逐字符一致）。
- 工作区：`cargo fmt --all -- --check`、helper `cargo fmt -- --check`、`cargo test --workspace`
  全绿；`cargo check -p sayall-windows-app --features runtime-simulation` 与前端 `pnpm test`
  / `pnpm build` 见 ADR 实施记录与本分支交付说明。
- **ARM64 真机实测——`passed`（2026-10-07，报障人，RC003）**：
  - 只读自检两步通过：`--selftest` 退出码 0（含第 6b 项"同时登记 x86_64 与 arm64"）；
    `--dry-run` 退出码 0，`[VERIFY] … size=21078016 sha256=323a91b3… machine=arm64 0xAA64
    arch_expected=arm64 0xAA64`（与锁定值逐字符一致）。
  - 注入链：`[TASK] … /tr "…\sayall-helper-arm64.exe" --follow-app` →
    `[ARCH] injector=arm64 0xAA64 target=arm64 0xAA64 gadget=arm64 0xAA64 image=…\WUDFHost.exe`
    → `[INJECT] … hmodule_return=0x87550000` → `[VERIFY-MODULE] module_present=true
    gadget_modules_in_host=1` → `rc003_bridge event=helper_authenticated helper_pid=23232
    version=2 transport=named_pipe` → `enhanced_capture event=ownership_resumed
    usages=28,35,4a,4f,50,51,52,65,66,80`。
  - 三键：音量+ / 音量− 各连按 3 次全部命中（`map_fire … steps=3`），返回键 2/2 命中
    （`action=shortcut chord=VolumeMute`）；对照的原有按键（下键）正常，语音与连接无回归。
  - 由此**收窄**：`x64 主程序 → 计划任务 → arm64 助手` 这条此前纯属推断的链已实证。
- 仍 `deferred`：RC001 的同一链路（机制不同，不能外推）；冷态首用、断连、睡眠恢复矩阵；
  安装器生命周期矩阵本机未跑（会改动本机已装应用，交由 CI 的 `installer` job）。

## 后续修复（2026-10-07 报障人实测发现）

**升级路径缺陷——计划任务仍指向旧架构的助手**：覆盖安装保留了旧版注册的计划任务，ARM64
机器上它仍指向 `sayall-helper.exe`，于是升级后第一次自动拉起跑了 x64 助手；架构闸门正确拦下
（`[STOP] 架构不一致…`，文案清楚），但界面要空转约 25 秒重试后才失败。手动关一次开关再开
（强制重装任务）后一切正常。

**修复**：启动对账新增"任务目标架构"判据——读计划任务 XML 的 `<Exec><Command>`（只取文件名，
完整路径不进日志），与按架构选中的助手名比对；不一致即回落开关并落
`terminal_result=revoked reason=task_target_mismatch expected_helper=…`，不进入重试、不在启动
时擅自弹 UAC（重装任务要提权，保持"用户开关一次即重新授权"的语义）。
**顺带修**：`[DLL]` 汇报在 copied 路径原先固定写 `sha256_verified=false`（报障人据此问"是不是
没校验就用了"）——现在复制后对副本重算摘要，copied 路径同样报 `true`，摘要不符则硬失败（不再
只记一条日志接着注入）。

## 对报障人的回复要点

- 确认问题成立：该功能当前只在 x64 系统上可用，ARM64 上不会成功；失败原因与安全软件无关。
- 说明可见影响：基础语音与其它按键不受影响；只有「返回 / 音量+ / 音量−」三键依赖该功能。
- 说明计划：已按 ARM64 支持立项（ADR 0003），能力与提示同版本交付；交付前界面会明确说明
  不支持，而不是长时间停在「正在启动」。
- 索取：完整的 `auto_trigger_escalate` 失败行（含 5 个对账字段）与助手侧的
  `[HOST]` / `[VERIFY]` / `[INJECT]` 行，用于确认宿主定位与 Gadget 校验在 ARM64 上均通过。

## 隐私检查

本文不含个人路径（一律写 `%LOCALAPPDATA%` 或仓库相对路径）、不含设备身份、蓝牙地址、
语音内容或凭据；不提交报障人的原始日志文件。
