# Windows 注册应用扫描与应用库验收

应用使用 Windows 公开 AppsFolder 接口扫描。扫描、搜索或添加到应用库本身不会启动应用，也不会改变任何按键绑定。

## 自动化检查

```powershell
.\scripts\ci-preflight.ps1
cargo test -p sayall-windows registered_apps
```

## 功能验收

- 验证应用库扫描、搜索、多选、全选、扫描失败重试，以及保存、导入和导出。
- 验证扫描与添加不会自动启动应用或绑定按键。
- 从应用库选择一个目标绑定到按键后，验证启动路径。
- 目标未运行时，按一次映射键后窗口应成为前台；目标已运行且被其他窗口遮挡或
  最小化时再次按映射键，窗口也应恢复并成为前台，不能只在任务栏闪烁。诊断日志
  应以 `target_result=foreground_observed` 作为成功终态；`SetForegroundWindow`
  单独返回成功不算通过。
- 物理 Alt 正在按住时触发映射，应用不得注入 Alt UP 破坏用户键态；日志应记录
  `physical_alt_held=true`，若 Windows 因此前台拒绝则明确失败，不得误报成功。
- 分别用 RC001、RC003 验证实体按键与已有设置保持不变。

RC001/RC003 实体按键回归仍为 `deferred`。
