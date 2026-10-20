# 按键页「锁定当前按键」切页后自动回开（关不掉）

- 发现日期：2026-10-18（用户报障）
- 状态：已修复（自动化 + 仿真真机 + 本地测试包安装/安装版复验 passed）
- 影响范围：1.0.0 及此前所有带该开关的构建；Windows 10/11（与 CPU 架构无关——纯前端状态问题）；
  RC001 / RC003 无差异（本功能与遥控器型号无关）；与第三方工具无关。
- 功能点：按键页页脚「锁定当前按键」开关（映射编辑选择策略，Mac `MappingSelectionPolicy` 同源）
- 现象：关掉开关后切到其他页面再切回，开关又变回「开启」。用户侧表现即"这个按钮关不了"。
- 复现条件：① 打开「按键」页；② 关闭「锁定当前按键」；③ 切到「连接」页；④ 切回「按键」页
  → 开关重新为「开启」。每次必现（与遥控器、连接状态无关）。
- 正常预期：用户的选择被记住并生效。2026-10-18 用户定稿：**跨页面切换与跨重启都保持**；
  首次使用（从未设置过）默认「开启」。
- 证据：
  - 代码路径（根因直接证据）：`src/App.vue` 以 `<component :is="activeComponent">` 切换页面、
    无 `KeepAlive` → 切页卸载并重建 `ButtonsPage`；`src/pages/ButtonsPage.vue` 中该开关
    只是页面局部状态，全仓库无任何读写点。
  - 自动化复现（红→绿）：`src/pages/ButtonsPage.test.ts` 新增
    `persists the lock toggle so it stays off across page switches`（卸载后重挂载 =
    切页往返）；修复前失败于「关闭动作未落盘」，修复后通过。
  - 真机证据见下方「验证记录」。
- 根因：`lockSelection` 仅为页面局部 `ref(true)`，没有持久化；页面切换（组件重建）即回默认值。
  Mac 版同款开关是设置窗口内 `@State`（本就不持久化），移植到 Windows 时未纳入本仓库的
  设置存储——而 Windows 版页面是可切换的，于是"切页回开"成为本仓库特有缺陷。
- 修复（最小改动）：
  - `crates/sayall-core/src/settings.rs`：新增 `lock_mapping_selection`（默认 `true`；
    老配置无此字段时按容器级 `#[serde(default)]` 回落默认）。
  - `src-tauri/src/settings.rs`：`load_lock_mapping_selection` / `save_lock_mapping_selection`。
  - `src-tauri/src/lib.rs`：命令 `get_lock_mapping_selection` / `set_lock_mapping_selection`
    （结构化日志 `mapping_settings feature=selection_lock`，加载/保存各自记录结果、错误域与耗时），
    双 invoke_handler（含 runtime-simulation 构建）同步注册。
  - `src/lib/bridge.ts`：`getLockMappingSelection` / `saveLockMappingSelection`
    （浏览器预览回落"开启"且不落盘，与主题等设置类一致）。
  - `src/pages/ButtonsPage.vue`：挂载时读回（迟到读取不覆盖用户刚做出的选择）、变更时保存
    （串行队列防过期值落盘；保存失败回退到最近落盘值并在页脚提示）。

## 验证记录

### 自动化（本机，2026-10-18）

| 检查 | 命令 | 结果 |
| --- | --- | --- |
| 核心设置模型 | `cargo test -p sayall-core` | passed（含新增默认值/往返用例） |
| 设置存储 | `cargo test -p sayall-windows-app lock_mapping_selection` | passed（新增用例，修复前 E0599 红） |
| 全工作区 | `cargo test --workspace` / `cargo check --workspace` | passed（exit 0） |
| 平台层（仿真构建） | `cargo check -p sayall-windows-app --features runtime-simulation` | passed |
| 格式 | `cargo fmt --all -- --check` | passed |
| 前端全量 | `vitest run`（23 文件） | 371 passed / 11 skipped |
| 类型/构建 | `vue-tsc --noEmit`、`vite build` | passed |

按键页新增 4 条用例（先红后绿）：挂载读回持久值（含解锁语义随读回值生效）、
切页往返保持关闭（本 Bug 原始路径）、保存失败回退并提示、读取失败回落默认并落前端事件。

### 真机·仿真构建（runtime-simulation + 隔离状态目录，真实 WebView / IPC / 设置文件）

构建：`cargo build -p sayall-windows-app --features runtime-simulation`；
运行环境：`SAYALL_WINDOWS_RUNTIME_SIMULATION=1`、`SAYALL_RUNTIME_SIMULATION_STATE_DIR=<临时隔离目录>`
（隔离目录只影响状态文件位置；日志与其余行为走生产路径）。界面操作经 UIA（Toggle/Invoke）。

| 步骤 | 观察 | 结果 |
| --- | --- | --- |
| 首启（隔离目录内预置不含新字段的老配置） | 开关显示为**开**（老配置回落默认值） | passed |
| 点击关闭开关 | 隔离目录 `settings.json` 写入 `"lock_mapping_selection": false` | passed |
| 切到「连接」页再切回「按键」页（本 Bug 原始路径） | 开关保持**关** | passed |
| 优雅退出后重启同一构建（同一隔离目录） | 开关仍为**关** | passed |
| 反向：再打开开关 | 文件翻回 `"lock_mapping_selection": true` | passed |

诊断日志摘录（`%LOCALAPPDATA%\SayAll\logs\sayall-diagnostic.log`，UTC 时间）：

```
08:00:21 pid=22288 mapping_settings feature=selection_lock action=load phase=completed terminal_result=passed locked=true elapsed_ms=0
08:00:41 pid=22288 mapping_settings feature=selection_lock action=save phase=requested locked=false
08:00:41 pid=22288 mapping_settings feature=selection_lock action=save phase=completed terminal_result=passed locked=false elapsed_ms=1
08:00:47 pid=22288 mapping_settings feature=selection_lock action=load phase=completed terminal_result=passed locked=false elapsed_ms=0
08:01:03 pid=32576 mapping_settings feature=selection_lock action=load phase=completed terminal_result=passed locked=false elapsed_ms=0
```

截图（本地取证，未入库）：`artifacts/local-test/2026-10-18-lock-selection-verify/lock-off-after-page-switch.png`
——切页往返后页脚开关处于关闭态。

### 本地测试包与安装版复验（2026-10-18）

- 本地测试包（NSIS；本机测试密钥签名，**绝不可公开发布**）：
  - 构建：`scripts/build-local-test-package.ps1 -CargoTargetDir <主仓库 target>`（复用热缓存，
    release 编译约 1.5 分钟；exit 0）；`verify-frontend-embedded.ps1` passed（10 个前端资源
    全部嵌入）；**阳性对照**：同一脚本对未嵌入的 debug exe 按预期失败（exit 1，报缺失清单）。
  - 包：`target/release/bundle/nsis/无线麦 SayAll_1.0.0_x64-setup.exe`；
    SHA-256 `1586e28b785ac8d9a0ebcd95d9036ab75db9181494c36315cc42b1c60f3fc6e3`；含 `.sig`。
- 安装校验：`scripts/verify-local-test-install.ps1 -Setup <包> -ExpectRevision` → PASS：
  静默安装（RC=0）；helper / gadget 哈希对齐；app exe 与裸构建差 3 字节（UNK→NSS，预期）；
  内嵌修订号 `dbfb521` count=2；安装后启动 + 窗口截图 OK。
- 安装版复验（真实用户设置目录）：开关默认「开」→ 关闭（设置文件写入
  `"lock_mapping_selection": false`）→ 切「连接」页再切回保持关 → 重启应用仍为「关」→
  反向开回写盘 `true`（复验后恢复默认）。日志（revision=dbfb521）：

```
09:23:19 pid=10332 selection_lock action=load locked=true
09:23:50 pid=10332 selection_lock action=save locked=false
09:24:01 pid=10332 selection_lock action=load locked=false    ← 切页往返后读回
09:24:07 pid=17528 selection_lock action=load locked=false    ← 重启后读回关闭
09:24:24 pid=17528 selection_lock action=save locked=true     ← 复验后恢复默认
```

- 截图与安装校验日志（本地取证）：
  `artifacts/local-test/2026-10-18-lock-selection-verify/`（含安装版重启后关闭态截图）。

### 未覆盖边界（deferred）

- RC001 / RC003：本功能与遥控器无关，不适用；本次未触及按键映射本身的链路。
- 本包为本机测试密钥签名、单载荷（本机缺 VS ARM64 工具集，不含 arm64 两件）；不可作为
  公开发布物。

- 隐私检查：本文档与新增日志均不含个人路径、设备身份、语音内容或凭据；新增日志字段只有
  布尔偏好值与稳定 token。
