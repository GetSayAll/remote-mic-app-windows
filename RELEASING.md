# 无线麦 SayAll Windows 发布流程

本流程将参考仓库的发布不变量改写为 Windows 版本，适用于公开 Preview、Stable 及应用内更新资产。

## 发布不变量

- **出包/编译前置**：`src-tauri/tauri.conf.json` 的 `bundle.resources` 声明了
  `sayall-helper.exe` 与 `frida-gadget.dll`，而这两个文件**不入库**（见 `.gitignore`）。
  tauri-build 在**编译期**就校验资源路径存在，所以任何会编译 `src-tauri` 的环境
  （本机出包、CI verify、发布流程）都必须先执行 `node scripts/stage-bundle-inputs.cjs`
  ——它构建助手、按锁文件获取并校验 Gadget、再复用 `stage-bundle-resources.cjs` 落地。
  漏掉这一步的报错是 `resource path 'sayall-helper.exe' doesn't exist`（2026-09-27 实测）。
- **前端资源必须嵌入主程序**：出包后必须运行 `scripts/verify-frontend-embedded.ps1`
  （本机出包用 `-Executable` / `-FrontendDirectory` 指向实际产物与 `dist-build-*`；
  CI verify / installer / release 三个 workflow 已各自接入同一脚本）。补丁
  `build.frontendDist` 必须写**相对路径**（相对 `src-tauri`）；Windows 绝对路径
  （`D:/...`）会被 tauri 按 URL 解析成单字母 scheme（`d:`），资源被静默跳过嵌入，
  应用界面会变成目录列表，而哈希、签名、内嵌修订号全部正常（2026-10-03 本机实测）。
  该检查有阳性对照：对未嵌入的 exe 必须失败。
- 默认交付物是本地测试包。先在本机生成测试安装器，报告其路径和校验值，并完成与改动风险相称的本地安装、升级、启动和功能验证。
- 本地验证通过后必须停在“可发布”状态；不得自行创建发布 Tag、GitHub Release、发布草稿或上传发布资产。只有用户在收到验证结果后明确要求“发布预览版”，才获得本次发布授权。“继续”“做完”“合入 main”等指令本身不构成发布授权。
- 发布源必须是已合入远端 `main` 的精确 SHA；开始前 `git fetch origin main`，发布 worktree 必须干净且与该 SHA 一致。
- `main` 不直接开发或 push，版本、Release Notes、脚本和文档均通过普通 PR 合入。
- Preview 的 CI 未签名 NSIS artifact 只能用于受限验收，不能宣称为公开可信安装包；公开分发需同时满足 Authenticode（若流程已启用）、updater minisign 签名、SHA-256 和来源元数据要求。
- Tag、Release Notes、资产和 `latest.json` 建立后视为不可变。内容变化回到普通 PR，使用新版本/Build；不得覆盖旧 Tag 或资产。

## 双载荷：x64 + arm64（2026-10-07 issue206）

- **交付形态不变**：主程序、安装器和更新通道仍是 **x64 单包单通道**；变的是安装包
  同时携带**两份助手与两份 Gadget**，运行时按系统原生架构选一份：
  - x64：`sayall-helper.exe`、`frida-gadget.dll`
  - arm64：`sayall-helper-arm64.exe`、`frida-gadget-arm64.dll`
  原因：Windows 11 ARM64 上"全按键支持"只能注入原生 ARM64 的 `WUDFHost.exe`，
  x64 助手进不去（见 `docs/decisions/0003-arm64-enhanced-capture-scope.md`）。
- **默认 `src-tauri/tauri.conf.json` 的 `bundle.resources` 必须保持只有 x64 两件**。
  tauri-build 在**编译期**校验资源路径存在（见 `scripts/stage-bundle-inputs.cjs` 顶部
  2026-09-27 实测记录），而 arm64 助手要链接成 `aarch64-pc-windows-msvc` 就需要 VS 的
  `Microsoft.VisualStudio.Component.VC.Tools.ARM64` —— 本机与部分环境没有它。写进默认
  配置会让任何 `cargo check` / `cargo test` 直接失败，破坏全仓库日常验证回路。
- **arm64 走覆盖式声明**：只有 arm64 两件真的暂存到 `src-tauri/` 之后，出包/CI 才追加
  `--config src-tauri/tauri.arm64-payload.conf.json`（把 `bundle.resources` 覆盖成四件）。
  tauri 的 `--config` **可重复**，按 RFC 7396 顺序合并、数组整体覆盖（2026-10-07 用
  tauri-cli 2.11.4 实测：两个 `--config` 都被接受，且其 `helpers/config.rs` 用
  `json_patch::merge`），所以它可以与 `tauri.local-build.conf.json` 叠加。
- **强制变量 `SAYALL_REQUIRE_ARM64_PAYLOAD=1`**：arm64 载荷缺失时让 staging 与落地
  步骤**非零退出**（退出码 3），而不是打印警告后继续。发布路径（CI `installer` job）
  必须设置它——发布不允许静默退化成单载荷包。默认不带该变量时允许跳过并打印明确
  警告（本机缺工具集是正常情况）。
- **工具链判据**（两条都要满足，否则视为"缺 arm64"）：`rustup target list --installed`
  含 `aarch64-pc-windows-msvc`，且 vswhere 能查到 `Microsoft.VisualStudio.Component.VC.Tools.ARM64`
  （注意 vswhere 查不到时**仍以 0 退出**、输出为空，判据是 stdout 非空而不是退出码）。
  诊断命令：`node scripts/stage-bundle-inputs.cjs --check-arm64`（不构建任何东西）。
- **CI 分工**：`verify` job 装 aarch64 目标并跑
  `cargo check --target aarch64-pc-windows-msvc --manifest-path hardware/RC003/helper/Cargo.toml`
  （`cargo check` 不调用链接器，因此不依赖 VS ARM64 工具集，只为让 arm64 助手的编译
  路径在 CI 里被真实覆盖）；`installer` job 设 `SAYALL_REQUIRE_ARM64_PAYLOAD=1` 并以
  `--config src-tauri/tauri.arm64-payload.conf.json` 出包。
- **校验判据（集合一致）**：`scripts/verify-windows-bundle.ps1` 断言暂存集合与 arm64
  覆盖配置声明的集合一致，且 `SAYALL_REQUIRE_ARM64_PAYLOAD=1` 时 arm64 必须真的暂存到；
  `scripts/verify-local-test-install.ps1` 对**已暂存**的 arm64 两件做安装目录哈希对齐。
  未暂存时两者都不得误报失败（否则 ARM64 机器上从双载荷包升级到单载荷包时会误判）。
- **安装器钩子覆盖两个名字**：`src-tauri/windows/installer-hooks.nsh` 的进程查找、
  映像写锁探测与升级前等待同时覆盖 `sayall-helper.exe` 与 `sayall-helper-arm64.exe`，
  否则 ARM64 机器上正在运行的助手会被漏掉、覆盖安装撞上"无法打开要写入的文件"。
- **边界（deferred）**：本机没有 ARM64 硬件，"ARM64 上真的能跑"必须由真机验收给出，
  不得用 x64 主机上的构建/校验结果代替。

## 版本号唯一来源（2026-09-30 收敛）

- 应用版本号**只写在** `src-tauri/tauri.conf.json` 的 `version`。安装包文件名、exe 的
  版本资源、关于页显示、更新器比较、诊断日志的 `app_version` 全部由它派生。
- 改版本号 = 改这一行。`Cargo.lock` 里的 crate 版本是占位 `0.0.0`，不随应用版本变化，
  因此发布只需一个文件的一行 diff。
- 不要在两处维护版本号：`Cargo.toml` 的 `[workspace.package] version` 是内部 crate 的
  占位值，`package.json` 不再带 `version`。把版本号写回这两处既不会改变产物，又会让
  安装包/关于页与仓库里的数字重新漂移。
- 守卫：`src-tauri` 的单元测试 `app_version_comes_from_tauri_config` 核对运行期
  `package_info().version` 与 config 一致（删掉 config 的 `version` 会同时丢掉 exe 的
  版本资源并让测试失败）；`generate-updater-manifest.ps1` 继续强制 Tag 与 config 版本一致。

## Preview

1. 先完成本地测试包的构建和验证，并向用户报告结果；没有用户随后给出的明确预览发布指令时，到此停止。
2. 获得明确发布授权后，从最新 `origin/main` 建立发布分支，确认版本、Build 和说明已冻结。
3. 运行 `scripts/ci-preflight.ps1`；需要深度检查时运行 `-Full`。CI 必须记录 source SHA、构建通道和 artifact digest。
4. 运行 `scripts/verify-windows-bundle.ps1`，确认 NSIS、应用和元数据状态；未签名候选明确标记为 `unsigned-ci-preview-not-for-public-release`。
5. 在 Windows 主机按 [Testing/WindowsReleaseBranchLifecycle.md](Testing/WindowsReleaseBranchLifecycle.md) 完成安装、升级、卸载、启动和设置保留验证；按 [Testing/WindowsRC003Preview.md](Testing/WindowsRC003Preview.md) 执行硬件与第三方语音边界。
6. PR 描述分别列出自动化、安装器、真实 RC001/RC003、VB-CABLE 和第三方输入法结果；不可把 Mac 构建或模拟器结果写成 Windows 真机通过。

## Stable 与 updater 资产

- 正式 Release workflow 必须从精确 Tag/Commit 构建，缺少 `TAURI_SIGNING_PRIVATE_KEY` 或 Authenticode 发布凭据时 fail closed；CI 临时 updater key 只允许测试构建。
- `scripts/generate-updater-manifest.ps1` 生成 `latest.json`、ASCII 安装器名、`.sig` 和 `SHA256SUMS.txt`。清单中的签名是 `.sig` 内容，不是路径；下载地址必须为 HTTPS。
- 发布前打印并核对待上传资产清单、大小和 SHA-256；删除或覆盖远端资产不属于正常重试流程。
- 应用内更新退出前必须显式断开 BLE、释放键态和停止音频；不能依赖 `Drop`，也不能强杀正在连接的旧进程。

## 失败与重试

- Runner、GitHub、网络或签名服务失败且尚无公开身份：在同一 SHA、版本、Build 和 artifact 身份上重试，不新建 rerun 分支、不升版本、不重签已成功字节。
- Release 已创建但验证失败：先只读核对 Tag、资产、digest 和 notes，只补缺失验证；发现字节或来源不一致立即停止并保留现场。
- 任何认证、权限、5xx、超时或无法判断的远端结果均 fail closed。

## 发布报告

报告 source SHA、Tag、版本/Build、workflow Run、artifact ID/digest、安装器签名状态、资产 SHA-256、安装生命周期矩阵、真实硬件/第三方工具结果及 `passed`、`failed`、`deferred` 边界。不得输出证书、私钥、密码、Token 或用户数据。
