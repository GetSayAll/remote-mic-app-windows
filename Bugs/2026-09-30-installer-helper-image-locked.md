# 覆盖安装时 Helper 映像仍被占用

- 发现日期：2026-09-30
- 状态：已修复，等待最终安装包覆盖安装复测
- 影响范围：已启用 RC003 增强捕获、提权 Helper 正在运行时的覆盖安装/升级
- 现象：安装器弹出“无法打开要写入的文件：`sayall-helper.exe`”；手动停止 Helper 后点击“重试”可继续。
- 正常预期：安装器先请求应用优雅退出，再停止 Helper 并等待其映像写锁释放；若预算内仍未释放，应由产品自己的明确提示中止，而不是落入 NSIS 通用的 Abort/Retry/Ignore 覆盖失败弹窗。

## 根因

`SayAllStopHelper` 已发送计划任务结束请求和停用信号，但最终判据只使用 `FindProcessCurrentUser`。普通权限安装器可能枚举不到提升权限的 Helper，于是把“未发现进程”误当成“映像已可覆盖”，继续执行 File 指令时才由 Windows 以共享冲突拒绝。

## 修复

- 保留原有计划任务结束、停用信号与进程等待。
- 在进程等待后，对已经存在的 `$INSTDIR\sayall-helper.exe` 追加直接写锁探测；锁未释放时按 250ms 间隔在同一 8s 预算内重试。
- 首次安装的目标文件不存在时跳过探测，避免 append 模式创建空文件。
- 超时后显示产品化说明并中止，禁止继续到覆盖写。

## 验证

- TDD 红灯：先加入安装器源码契约，旧钩子因缺少文件存在守卫与写锁探测按预期失败：`passed`（证明覆盖旧缺陷）。
- `installer_waits_for_helper_image_lock_before_overwrite` 的库测试：`passed`；同一 cargo 命令随后执行无关 bin 测试时被本机 Smart App Control 以 OS error 4551 阻止，非断言失败。
- NSIS 编译：`passed`；Tauri 成功产出 0.3.0 x64 NSIS bundle（随后仅因本地未提供 updater 发布私钥而返回非零，不影响已生成安装包）。
- 正在运行提升权限 Helper 时执行最终安装包，安装器自动完成覆盖：`deferred`。

- 隐私检查：本文未包含个人路径、设备身份、语音内容、bridge token 或凭据；原始截图不提交。
