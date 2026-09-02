# Windows 启动时额外出现终端窗口

## 复现

- 构建并安装 Windows NSIS Preview。
- 从开始菜单启动无线麦 SayAll。
- 预期：只出现应用窗口。
- 实际：同时出现标题类似 `sayall-windows-app.exe` 的终端窗口。

## 日志与证据

- 从安装包提取的 `sayall-windows-app.exe` PE Subsystem 为 `3`（Windows CUI）。
- 这与 Tauri GUI 应用预期的 Subsystem `2` 不符。

## 根因

`src-tauri/src/main.rs` 未声明 Windows GUI 子系统，发布构建因此使用控制台子系统。

## 修复

为 `main.rs` 增加 `windows_subsystem = "windows"` 条件属性，并在 Windows CI 的最终发布可执行文件上校验 PE Subsystem 必须为 `2`。

## 验证

- 自动化：`scripts/verify-windows-bundle.ps1` 在 Windows Runner 解析最终 PE 头并拒绝 Subsystem 非 `2` 的构建。
- 真机：待下一版 Draft 安装包在 Windows 上重新安装并启动确认。
