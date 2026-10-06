# ADR 0003：ARM64 上「全按键支持」的支持范围与分发方式

- 状态：Accepted（2026-10-07，Andy 拍板；实施中，待 ARM64 真机验证）
- 日期：2026-10-07
- 关联：Issue #206、ADR 0002（双轨注入 / 可选 Helper）、
  [Bugs/2026-10-07-issue206-arm64-unsupported.md](../../Bugs/2026-10-07-issue206-arm64-unsupported.md)

## 背景

Issue #206（2026-10-06，Windows 11 ARM64 内部版本 26200，Snapdragon X Elite，RC003）：
开启「全按键支持」后界面永久停在「全按键支持已开启，正在启动」，「返回 / 音量+ / 音量−」
三键零事件，其它按键正常。

核验结论（2026-10-07，基于 `origin/main` = `613dac9` 与发布 tag `v0.5.0` = `e5374a9`；
两者在注入链路上相同）：

1. 报障人日志中的 `[STOP] CreateRemoteThread 失败，GetLastError=6（安全软件拦截注入时常见）`
   逐字出自助手 `inject_gadget()`（`hardware/RC003/helper/src/main.rs:2261`，v0.5.0 为 `:2174`）。
   按该函数的分支顺序可断定：助手已提权、已定位宿主、Gadget 已通过 SHA-256 校验，
   `OpenProcess` / `VirtualAllocEx` / `WriteProcessMemory` 均成功，**只有 `CreateRemoteThread`
   被拒**；随后助手以退出码 9 结束（`main.rs:4407`），从未向主程序鉴权，故主程序侧
   `helper_pid=0`、桥一直是 `listening`。
2. 产品全链路只产出 x64 二进制，且是**主动断言**而非默认配置：
   `hardware/RC003/helper/vendor/fetch_frida_gadget.py:153`（PE machine ≠ 0x8664 直接失败）、
   `hardware/RC003/helper/src/main.rs:6404`（`verify_gadget` 只认单一 SHA-256）、
   `scripts/generate-updater-manifest.ps1:44`（断言安装器名必须是 `*_x64-setup.exe`）、
   `:57`（资产名写死 x64）、`:67`（平台键写死 `windows-x86_64`）；
   两个 CI workflow 无构建矩阵、无 `--target`。
3. 运行时没有任何架构门禁或提示：唯一的启动期环境门是 Windows 版本号
   （`crates/sayall-windows/src/compatibility.rs:3`），助手定位只检查文件是否存在
   （`src-tauri/src/rc003_task.rs:54`）；唯一带「架构」字样的字段是**编译期常量**
   `std::env::consts::ARCH`（`src-tauri/src/lib.rs:1992`）——在 ARM64 上照样打印 `x86_64`。
4. 注入目标在 ARM64 上只能是原生 ARM64 的 `WUDFHost.exe`。ARM64 进程不能加载普通 x64 镜像
   （微软 Arm64X 文档：同一 DLL 要同时服务 Arm64 与 x64/Arm64EC 进程必须构造成 Arm64X/Arm64EC）。
   因此即便 `CreateRemoteThread` 成功，`LoadLibraryW` 也必然失败——**该路径在 ARM64 上
   结构性不可用，与安全软件无关**。
5. 参考实现侧条件已具备：Frida 自 16.5.0 起支持 Windows on ARM（arm64 版按目标架构注入
   原生 arm64 进程与仿真的 x86/x64 进程）；17.18.0 提供 `frida-gadget-17.18.0-windows-arm64.dll.xz`。

## 决策

1. **范围：只做增强轨的 ARM64 支持。** 主程序、安装器与更新通道保持 x64 单包；不新增
   `windows-aarch64` 更新通道，也不出原生 arm64 主程序（Andy，2026-10-07）。
2. **分发：单安装包内同时携带 x64 与 arm64 两份助手与 Gadget**，运行时按**系统原生架构**选择。
   选择依据用 `IsWow64Process2` 的 `nativeMachine`；**不得**使用 `PROCESSOR_ARCHITECTURE`
   环境变量或编译期常量——x64 进程在 ARM64 上两者都会给出误导性的 `AMD64` / `x86_64`。
3. **架构一致性成为注入前置校验**：注入前校验目标宿主架构与待注入 Gadget 架构一致，不一致
   立即给出明确原因。现有文案把跨架构失败归因成「安全软件拦截注入时常见」，属误导，必须修正。
4. **不可用时说清结果与下一步，不空转**：架构不受支持、或助手 / Gadget 缺失时，界面明确说明
   原因与恢复方式，并停止约 25 秒的自动重试空转（`src-tauri/src/lib.rs:591`、`:680`）。
   用户可见文案按 `docs/product-copy.md` 定稿（说结果与下一步，不出现内部术语）。
5. **能力与提示同版本交付**：不单独发「诚实降级」版本（Andy，2026-10-07）。在此版本发布前，
   ARM64 用户仍会看到空转；对 Issue #206 的回复需说明当前不支持与预期交付。
6. **第三方二进制的登记**：新增第二份 Gadget 后，`hardware/RC003/helper/vendor/frida-gadget.lock.json`
   与 `ATTRIBUTION.md` 按架构各登记一条（版本、URL、压缩包大小与 SHA-256、解压产物 SHA-256、
   官方 digest 来源），`verify_gadget` 的单一常量改为按架构两份。

## 结果

- 正面：ARM64 用户可获得与 x64 等价的「全按键支持」；不改基础路径承诺——ADR 0002 的边界不变
  （Helper 仍是可选增强轨，未安装 / 失败 / 被禁用时基础语音与基础按键不受影响）；不需要驱动、
  不提权范围变化、更新通道与包名不变。
- 代价：安装包增加一份 Gadget（x64 解压后 23254016 字节，arm64 同量级）；需要维护两份锁定值、
  两套构建产物；`verify_gadget`、`fetch_frida_gadget.py`、打包脚本、安装器与发射器选择逻辑
  都要按架构分化；诊断日志需要新增架构选择与前置校验结果两个字段。
- 风险：跨架构「x64 主程序启动 arm64 助手」这条链（含 `ShellExecuteEx(runas)` 提权与计划任务路径）
  尚未验证；arm64 Gadget 在 ARM64 WUDFHost 内的实际行为（IOCTL hook、agent 脚本、租约与清键）
  尚无真机证据。见下节。

## 待验证（不得凭推理处理）

1. x64（仿真）主程序能否启动 arm64 助手，并完成提权与计划任务注册 / 触发；在 ARM64 上先跑通
   `sayall-helper.exe --selftest` 与 `--dry-run`（后者不提权、不注入）。
2. ARM64 工具链：本机 VS BuildTools 2022 缺少 `Microsoft.VisualStudio.Component.VC.Tools.ARM64`
   （`vswhere -requires` 无返回），`rustup target add aarch64-pc-windows-msvc` 亦未安装；
   CI 侧本仓库为 public，GitHub 托管 `windows-11-arm`（arm64，4 vCPU / 16 GB）可用。
3. arm64 Gadget 在 ARM64 WUDFHost 中的报告层拦截是否与 x64 行为一致（`rc003_agent.js` 的
   IOCTL 路径与 usage 门禁）。
4. 端到端逐项复现：边沿锁存、清键、租约、桥协议 v2，按 `Testing/WindowsRC003EnhancedCapture.md`
   全 13 键走一遍。
5. RC001 是否受同一问题影响（RC001 的增强轨机制不同，不能由 RC003 外推）。

## 实施记录（2026-10-07，分支 `feat/arm64-enhanced-capture`）

- **助手与 Gadget 的架构化**：`hardware/RC003/helper/src/main.rs` 的 Gadget 摘要、锁定文件
  架构 token、随包文件名、注入器 machine 全部按编译架构取两份；`verify_gadget` 增加
  **PE machine 校验**并把 `machine` / `arch_expected` 打进 `[VERIFY]` 日志；
  `vendor/frida-gadget.lock.json` 增 arm64 条目（含 `runtimeName` 字段，说明"注入时统一叫
  `frida-gadget.dll`"，世代目录与常驻 tap 识别都按它工作）；`vendor/fetch_frida_gadget.py`
  支持 `--arch` / `--all`，按架构校验 PE machine，产物已通过校验时跳过重复下载与解压。
- **注入前的架构闸门**：新增纯函数 `pe_machine_from_bytes` / `arch_gate` 与 `arch_preflight`，
  在 `inject_gadget` 的**第一步**（任何 `OpenProcess` 之前）比对助手 / 宿主 / Gadget 三方架构，
  不一致直接拒绝并给出可操作说明；`CreateRemoteThread` 的失败文案不再首先归因"安全软件拦截"。
- **应用侧的架构判定与选择**：新增 `crates/sayall-windows/src/os_arch.rs`（`native_arch()` 取
  `IsWow64Process2` 的 `nativeMachine`，分类是纯函数）；`src-tauri/src/rc003_task.rs` 新增
  `CaptureSupport` / `capture_support()` 与按架构的 `locate_helper_exe_for()`；新 IPC
  `get_capture_support`；启动对账与自动拉起前置"架构 + 载荷"检查——不可用时**回落开关、
  落一条可分流日志、不进入 25 秒重试**；启动日志新增 `native_arch` 字段（与既有的编译期
  `process_architecture` 并存，只有前者反映系统架构）。
- **打包与安装器**：默认 `tauri.conf.json` 的 `resources` **仍只有 x64 两件**（tauri-build 在
  编译期校验资源存在，写进去会让没有 ARM64 工具链的机器连 `cargo check` 都跑不了）；新增
  `src-tauri/tauri.arm64-payload.conf.json` 覆盖四件，出包 / CI 在 arm64 载荷就绪时使用；
  `stage-bundle-inputs.cjs` / `stage-bundle-resources.cjs` 支持 arm64 分支与
  `SAYALL_REQUIRE_ARM64_PAYLOAD=1` 强制；`installer-hooks.nsh` 的进程查找、写入锁探测与
  优雅退出同时覆盖 `sayall-helper-arm64.exe`。
- **CI**：`verify` 增加 `cargo check --target aarch64-pc-windows-msvc`（不需要链接器），覆盖
  arm64 助手的编译路径；`installer` 强制包含 arm64 载荷。
- **文档**：`TECHNICAL.md`、`docs/installation-and-configuration.md`、
  `docs/architecture/windows-tauri-roadmap.md`、`DEVELOPMENT.md` 的架构口径按本 ADR 统一；
  `ATTRIBUTION.md` 登记 arm64 载体与参考实现结论。

### 本机证据（x64 开发机）

- `cargo test --manifest-path hardware/RC003/helper/Cargo.toml`：**33 passed**，含新增
  `arch_tests` 8 例（PE 头解析正负样例、四种架构组合的闸门判定、拒绝文案不含"安全软件"、
  按架构常量自洽、两份摘要必须不同、arm64 包名仍被常驻 tap 识别）。
- `sayall-helper.exe --selftest`：**全部通过**，含新增第 6b 项「锁定文件同时登记 x86_64 与 arm64」。
- `sayall-helper.exe --dry-run --target-pid <pid>`：退出码 0，`[VERIFY]` 打出
  `machine=x64 0x8664 arch_expected=x64 0x8664`（新增字段生效）。
- `fetch_frida_gadget.py --all`：两份产物的压缩包与解压产物 SHA-256、PE machine 全部匹配；
  arm64 压缩包摘要与 GitHub Releases API 的 `asset.digest` 逐字符一致。
- `cargo fmt --all -- --check` 与 helper 的 `cargo fmt -- --check`：`passed`。

### 仍未验证（`deferred`）

- **arm64 助手的链接与产物**：本机 VS BuildTools 2022 缺 `Microsoft.VisualStudio.Component.VC.Tools.ARM64`
  （Windows SDK 有 arm64 库、MSVC 没有），`aarch64-pc-windows-msvc` target 亦未安装；本机只能
  做到源码级与 CI 编译级覆盖，出包需在带该组件的机器或 CI 上完成。CI 侧 `windows-2025` 镜像
  自带 ARM64 组件（`actions/runner-images` 的组件表在列）。
- **ARM64 真机全链**：宿主定位、注入、报告层拦截、边沿锁存与清键、桥协议 v2，以及
  「x64 主程序启动 arm64 助手（含提权与计划任务路径）」都要在 ARM64 真机上重跑；
  `x64 → arm64` 的 `CreateProcess` / `ShellExecuteEx(runas)` 组合尚无实证。
- **RC001 是否同受影响**：RC001 的增强轨机制不同，不能由 RC003 外推。

## 依据来源（参考实现与官方资产）

- Frida 发布说明：16.5.0 起支持 Windows on ARM —— arm64 版可注入原生 arm64 进程与仿真的
  x86_64 / x86 进程（引文见 https://github.com/frida/frida/discussions/3350）。
- Frida 17.18.0 资产（GitHub Releases API，2026-10-07 查询）：
  - `frida-gadget-17.18.0-windows-arm64.dll.xz`，size 5555660，
    asset digest `sha256:9362da1d004c5bac3f42e10a6f5ab13a93a6a9de452015737c15b72fcce5ea90`；
  - 同版本 x86_64 资产 digest `sha256:2e549d3b77bc939b83b9a368655e90b7e566bd56b635a10fc3fab68d8dbe173d`
    ——与仓库锁定文件记录（`vendor/frida-gadget.lock.json:23`）逐字符一致，证明「官方 digest
    与锁定值对得上」这条登记方法成立，arm64 条目按同一方法登记。
- 微软 Arm64X 文档：Arm64 与 x64/Arm64EC 进程的镜像加载边界（普通 x64 DLL 不能被 ARM64
  进程加载）。
