# 升级 1.9.2 时 sayall-mcp.exe 被占用

- 发现日期：2026-11-14
- 状态：已修复，待进入下一安装包；1.9.2 已发布资产不替换
- 影响范围：Windows 安装器；从 1.9.1（或更早）升级到 1.9.2；Codex / Claude Code 等
  AI 工具正在使用 SayAll 本机记录 MCP 服务时；RC001 / RC003 无关
- 功能点：NSIS 安装/卸载钩子、`sayall-mcp.exe` 文件更新
- 现象：安装程序停在“无法打开要写入的文件：…\sayall-mcp.exe”，用户只能点
  “忽略”或“重试”；点“忽略”后主程序升级到 1.9.2，但 MCP 仍是旧文件
- 复现条件：
  1. 本机已安装 1.9.1（或其他旧版）；
  2. Codex / Claude Code 等客户端已启动 `sayall-mcp.exe serve`；
  3. 运行 1.9.2 安装器并覆盖安装。
- 正常预期：安装器结束当前用户下的 MCP 只读服务，更新 `sayall-mcp.exe`，
  不要求用户手动找进程或点击“忽略”。
- 证据：
  - 现场 `Get-Process` 显示 `sayall-mcp.exe` 正在运行，镜像路径位于安装目录；
  - 安装器询问“无法打开要写入的文件”，文件名正是 `sayall-mcp.exe`；
  - 该 MCP 服务没有接入主程序的命名事件优雅退出链路；
  - 手动结束 MCP 后重新运行 1.9.2 安装器，安装成功，MCP 哈希更新；
  - 用修改后的本地安装包、在真实 `sayall-mcp.exe serve` 进程持有时实测：
    安装器退出码 0，MCP 进程被结束，`sayall-mcp.exe` 被覆盖；重复一次结果相同。
- 根因：`src-tauri/windows/installer-hooks.nsh` 只处理主程序的优雅退出和提权助手，
  没有处理由外部 AI 工具按需启动的 `sayall-mcp.exe`。该进程在覆盖同名文件时
  持有映像锁，Tauri 默认安装段无法写入。
- 修复：
  - 新增 `SayAllStopMcp` NSIS 宏：仅当当前用户存在 `sayall-mcp.exe` 时，
    用 `taskkill /F /IM sayall-mcp.exe` 结束后轮询确认；预算内未退出则中止，
    交互式安装给出明确提示，卸载路径不弹窗；
  - 安装段在覆盖文件前调用 `SayAllStopMcp install 1`；
  - 卸载段同样调用 `SayAllStopMcp uninstall 0`，避免卸载留下被占用的 MCP 文件；
  - 新增契约测试 `installer_stops_mcp_server_before_overwriting_its_image`，
    断言宏内容及安装/卸载两处调用，防止后续改动把该步骤删掉。
- 验证：
  - `cargo test -p sayall-windows-app installer_stops_mcp_server_before_overwriting_its_image -- --nocapture`：`passed`（1 passed）
  - `pnpm tauri build --bundles nsis --ci --config src-tauri/tauri.local-test.json`：`passed`（NSIS 汇编成功）
  - 真实 MCP 进程持有时安装本地测试包：`passed`（安装器退出码 0；MCP 进程消失；文件被覆盖）；重复一次 `passed`
  - 1.9.2 正式安装包本身未改动：发布资产不可替换，修复进入下一版本；本机已恢复 1.9.2 安装态
  - 真实 RC001 / RC003 升级期间语音链路：`deferred`（需要物理遥控器；本次只验证安装器文件锁）
  - 可见安装界面 / SmartScreen：`deferred`（本次用静默安装验证）
- 隐私检查：未记录令牌、个人路径、设备身份或语音内容；MCP 令牌只在测试进程中从本机
  配置读取，未输出。
