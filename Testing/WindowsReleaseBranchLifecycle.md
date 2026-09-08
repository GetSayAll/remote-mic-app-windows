# Windows 发布分支生命周期测试

## 适用范围

验证从精确 `origin/main` SHA 产生的 Preview/Stable 候选的分支、提交、构建、安装和回滚边界。该手册不替代 RC001/RC003 真机语音验收。

## 准备

- Windows 10 1809 或 Windows 11 x64；
- 可用的 Tauri/NSIS 构建环境；
- 测试用用户数据目录和上一版本安装器；
- 记录 source SHA、版本、Build、Run 和 artifact digest。

## 步骤与预期

1. `git fetch origin main`，确认功能分支从最新 SHA 创建；预期工作区干净且来源可追溯。
2. 运行 `scripts/ci-preflight.ps1` 与 `scripts/verify-windows-bundle.ps1`；预期检查通过，未签名候选明确标记为不可公开分发。
3. 安装候选并启动；预期版本门禁、单一安装身份和应用存活通过。
4. 使用上一版本用户数据升级；预期设置、映射和统计保留，旧进程正常退出，BLE/音频资源释放。
5. 执行 `/S` 卸载并检查保留边界；预期程序移除且用户数据按产品约定保留。

任一步骤返回码、版本、数据或资源状态不符合预期即为 `failed`。SmartScreen、可见 UI、真实硬件或第三方工具未执行时标记 `deferred`。

## 证据

保留命令输出、构建元数据、安装器返回码和 SHA-256；不得保存完整用户路径、蓝牙地址、语音内容或凭据。
