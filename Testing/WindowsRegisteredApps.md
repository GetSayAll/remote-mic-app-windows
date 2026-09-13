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
- 分别用 RC001、RC003 验证实体按键与已有设置保持不变。

RC001/RC003 实体按键回归仍为 `deferred`。
