# 无线麦 SayAll Windows 日志规范

本规范把参考仓库中已验证的日志经验适配到 Windows。它适用于 BLE、HID、按键注入、WASAPI、输入法、Tauri IPC、更新器、安装器和诊断摘要。

## 基本原则

- 日志是功能的一部分。新增、修改或修复功能时，同时设计、实现并验证从入口到用户可见结果的完整链路。
- `received`、`decoded`、`enqueued`、`submitted` 只表示中间事实，不能冒充最终成功。
- 事实、未知状态和推测分开记录；无法观察第三方 App 内部状态时写 `unknown` 或 `diagnostic_boundary`，不得猜测。
- 同一次操作使用进程内短生命周期的 `operation_id`、`attempt_id`、`generation` 关联；一次操作只能有一个终态。
- 时间戳使用 UTC ISO 8601 毫秒精度；耗时使用单调时钟并输出整数毫秒。

## 行格式与诊断头

每行至少包含：

```text
2026-09-08T03:19:26.123Z pid=1234 ver=0.2.1 build=42 BLE CONNECT phase=completed result=passed
```

字段使用小写 `snake_case`，枚举使用稳定英文值。诊断摘要至少包含：

```text
diagnostic_schema=1
app_version=0.2.1
app_build=42
source_revision=<完整 SHA 或 unknown>
build_channel=stable|pr_preview|local|unknown
release_tag=v0.2.1|none|unknown
process_id=1234
process_architecture=x86_64|unknown
windows_version=<版本或 unknown>
windows_build=<build 或 unknown>
```

构建时写入 `source_revision`，运行时不得读取 Git 工作区。无法可靠取得的值写 `unknown`，不能省略字段。

## 必需事件链

每项功能按实际存在的边界记录：

1. 请求进入及来源分类；
2. 前置条件、权限和设备/端点选择；
3. 状态机接受或拒绝；
4. WinRT、WASAPI、SendInput 或 IPC 调用开始与返回；
5. 数据到达、解析、验证和路由；
6. 下游消费、提交和真正用户可见结果；
7. 取消、断连、超时、重试、恢复和迟到回调；
8. 唯一 `terminal_result` 及总耗时。

失败应带 `error_domain`、`error_code`、`reason` 和 `retryable=true|false`。状态未变化时不得重复刷相同日志；高频 HID、音频包和轮询只记录首尾、聚合计数、字节/帧数、丢弃数量和耗时。

## 隐私红线

不得记录语音或转写正文、用户输入、剪贴板、用户名/邮箱/手机号、完整路径、窗口标题、蓝牙 MAC/UUID/序列号、HID 路径、IP、Token/API Key/密码/私钥/证书、第三方 App 私有状态或原始音频字节。不要用敏感值的稳定哈希替代脱敏。允许记录稳定产品分类、布尔结果、错误 domain/code、采样率、帧数、耗时和脱敏计数。

## 评审清单

- [ ] 入口、前置检查、跨组件交接和最终结果均有日志。
- [ ] 成功、失败、取消、超时、重试、恢复和 stale callback 可区分。
- [ ] 异步提交与真实完成分开记录，且一次操作只有一个终态。
- [ ] 日志不包含个人信息、设备身份、路径、凭据、用户内容或第三方私有数据。
- [ ] 已覆盖自动化、Windows 真机和第三方工具的验证边界。
