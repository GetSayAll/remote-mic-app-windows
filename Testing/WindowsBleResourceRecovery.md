# Windows BLE 资源耗尽恢复验证

日期：2026-09-14

## 验证目标

确认 Windows BLE 栈返回 `0x80070008` 或 `0x80004004`、普通 Radio API 无法取得
对象时，SayAll 能以公开 Windows 接口恢复蓝牙设备节点，而不是要求用户进入设置
手工关开蓝牙；同时确认连接会话的 WinRT service/device Close 失败可被再次重试。

## 现场结果

| 项目 | 结果 | 证据边界 |
| --- | --- | --- |
| 参考实现 paired selector → `FromIdAsync` | failed | 僵死现场返回 `0x80004004` |
| 直接 WinRT GATT service selector | failed | 僵死现场返回 `0x80070008` |
| SetupAPI + Win32 GATT `CreateFile` | failed | 接口可枚举，打开返回 `0x80070079` |
| 普通用户 PnP 重启 | failed | Windows 拒绝访问；退出码不能单独代表成功 |
| UAC 明示授权的唯一 BTHUSB 节点重启 | passed | ignored live test 1.95s 完成；WinRT Radio 读回成功 |
| 恢复后的 `BluetoothLEDevice` 创建 | passed | SayAll 日志前进到 `conn_params`，不再即时资源耗尽 |
| 提交 `341721f` 本地 NSIS 安装与启动 | passed | 静默安装退出码 0；日志 source revision 与提交一致 |
| 当前所选遥控器完整 BLE 建链 | passed | 2.806s 内完成设备、服务、三项特征、两路订阅和能力响应 |
| RC001 完整连接与首次语音 | deferred | 本轮未取得完整能力协商和语音证据 |
| RC003 完整连接与首次语音 | deferred | 本轮未取得完整能力协商和语音证据 |

## 安全与产品边界

- PnP 兜底只接受两个已验证的系统栈错误码，常规 Radio Off/On 仍是首选路径。
- SetupAPI 必须恰好找到一个当前存在、服务为 `BTHUSB` 的蓝牙类设备；零个或多个都
  终止恢复，不猜测目标。
- 使用 `%SystemRoot%\System32\pnputil.exe` 固定系统工具；设备实例 ID 不进入命令
  shell、不写日志，并拒绝引号、换行、NUL 与异常长度。
- PnP 重启必须由 Windows UAC 明示授权。主程序始终保持普通用户权限；用户拒绝授权
  时继续普通自动重连，不把失败伪装成成功。
- 工具执行后必须重新枚举 WinRT Bluetooth Radio；只有读回成功才报告恢复完成。

## 自动化与本地包

- `cargo check --workspace --all-targets --all-features`：passed。
- `cargo test --workspace`：176 passed，0 failed，5 ignored（硬件/联网显式测试）。
- `pnpm test -- --run`：83 passed，0 failed。
- `pnpm build`：passed。
- 本地 NSIS：passed；`SayAll-Windows-0.2.6-ble-pnp-recovery-341721f-local-x64-setup.exe`
  的 SHA-256 为
  `701e7870c813de89bfb46eb852486b3717a449a9278d148e0e84995e5d1ac6a5`。
- 安装版进程启动后，诊断日志记录 `source_revision=341721f...`、
  `radio_recovery_prepare ... cache=ready access=allowed`，随后当前所选遥控器完成
  capability response；真实双型号和首次语音结果只按上表标记，不由单型号连接推导。
