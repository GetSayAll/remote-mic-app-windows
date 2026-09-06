# 应用内更新（updater）本机端到端验证记录（2026-09-05）

验证目标：`tauri-plugin-updater` + GitHub Releases 静态 latest.json 方案（方案一）的完整更新链路在本机真实生效——启动静默检查 → 手动确认 → 下载 → minisign 验签 → NSIS passive 安装（/P /UPDATE /R）→ 安装前 BLE 清理回调 → 注册表升级 0.1.0 → 0.2.0 → 安装器自动重启应用。取证日志（仅 updater 标记行，不含语音/设备数据）：`Testing/sayall-updater-e2e-20260905.log`。

## 环境

- 本机：Windows 10 Pro x64（build 19041），已装无线麦 SayAll 0.1.0（currentUser，AppData\Local\无线麦 SayAll），已装 VB-CABLE、WebView2 运行时 152.x。
- 被更新端：`pnpm tauri dev` 调试实例（0.1.0，debug 构建——debug 构建允许 http 更新端点，插件源码 `config.rs::validate_endpoints` 行为）。
- 更新源：本地 HTTP 静态服务器（127.0.0.1:8787）提供 latest.json + `SayAll-Windows-0.2.0-x64-setup.exe`（本地构建 0.2.0 NSIS 安装器 + minisign 签名，密钥为正式密钥对，公钥已入 tauri.conf.json）。端点经 `SAYALL_UPDATER_ENDPOINT` 环境变量覆盖（正式配置始终指向 GitHub Releases，无任何 dangerous 开关）。
- UI 驱动：WebView2 `--remote-debugging-port=9222` + CDP Runtime.evaluate 点击真实按钮（真实 WebView → Tauri IPC → Rust command 路径）。
- 诊断：`SAYALL_GATT_LOG` 全程开启。

## 结果（按验证词汇）

- 启动静默检查横幅（"发现新版本 0.2.0"）出现：**passed**（CDP 驱动断言；`check.ok available=true current=0.1.0 latest=0.2.0 took_ms=40/166`）。
- 横幅"查看"→ 关于页面板显示版本/说明/按钮：**passed**。
- 手动"下载并安装"：下载完整 6166058 字节（服务器日志 200 + 字节数一致）：**passed**。
- minisign 验签 + on_before_exit 清理回调（`install.before_exit disconnect=ok`）+ 进程 exit(0)：**passed**。
- NSIS passive 安装：注册表 DisplayVersion 0.1.0 → 0.2.0：**passed**。
- 安装器 /R 自动重启：进程从安装目录启动（pid 15264，01:08:53），重启后的 0.2.0 自行完成启动静默检查初始化（日志 17:09:20 行）：**passed**。重启后 0.2.0（release 构建）对 http 端点覆盖按设计 fail-closed 拒绝（该早期失败路径当时漏打点，已修复并补单测 `updater_notes_land_in_diagnostic_log`）。
- 验签安全性负例：第一轮 staging 误配（0.1.0 exe + 0.2.0 sig 混合对）被 `Minisign(InvalidSignature)` 正确拦截且应用无恙：**passed**（负例）。
- manifest 生成脚本 `generate-updater-manifest.ps1`：tag/版本不一致拒绝、多安装器拒绝、成功路径（latest.json/ASCII 资产重命名/SHA256SUMS/JSON round-trip 自检）：**passed**（本机 PS 5.1 + BOM 副本实测；CI 中以 pwsh 7 运行，仓库脚本按约定无 BOM）。

## 未覆盖（deferred）

- **GitHub 生产端点**（`releases/latest/download/latest.json`）：仓库尚无已发布稳定 Release，端点现为 404；首个 Release Publish 后才可端到端验证（检查失败静默，不影响主功能）。
- **windows-release.yml / windows-ci.yml 的 CI 运行**：需 push 到 GitHub 后首个 tag/PR 触发；本地无法验证 Actions 环境。
- **无 VB-CABLE 机器的 passive 更新提示行为**：本机已装 VB-CABLE，POSTINSTALL 提示未触发；未装 VB-CABLE 的用户在 `/P` 更新时的弹窗行为待真机复验。
- **大陆访问 GitHub 的更新下载成功率**：未量化（插件默认带系统代理与多端点能力，配置已留扩展位）。
- RC001/RC003 遥控器连接态下的更新退出清理：本次断开回调在无连接态执行（disconnect=ok）；"更新前正常断开连接中的遥控器"待真机复验。

## 附注（同日并行事件）

- 用户并行构建的 0.1.0 安装器在 tauri.conf.json 加入 updater pubkey 后因缺 `TAURI_SIGNING_PRIVATE_KEY` 构建失败（只留 exe 无 sig）——这正是 CI 兜底脚本 `ensure-updater-signing-key.ps1` 与 release workflow 强校验 Secret 所防的场景。
- E2E 临时脚本（serve_updates.js / drive_update_ui.js / click_vbcable_no.ps1 等）位于 `%TEMP%\sayall-e2e\`，未入库；如需固化为本机回归脚本另行立项。
