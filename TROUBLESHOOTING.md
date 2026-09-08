# 无线麦 SayAll Windows 排障指南

先收集去标识化诊断摘要和对应时间段日志，再按“复现 → 日志 → 代码 → 最小修复 → 重新验证”处理。不要把重新配对、重启蓝牙或重启电脑作为常规首选；应用应先执行自动重试和公开 API 的无线电恢复。

## 找不到或连不上遥控器

确认设备已在 Windows 设置中配对，查看扫描、型号识别、GATT 服务发现、通知订阅和重试结果。若连续失败达到恢复阈值，应用会尝试公开无线电恢复；只有公开 API 全部失败时才提示人工介入。不要记录或粘贴蓝牙地址、设备 UUID 或 HID 路径。

## 按住语音键没有声音或没有文字

分别检查语音键 DOWN/UP、ATVV `STREAM_START → AUDIO → STREAM_STOP`、PCM 解码、CABLE 端点/应用会话静音状态、输入法热键提交和最终文字结果。收到音频或完成入队不等于第三方已提交文字；第三方内部状态不可观察时应标记 `diagnostic_boundary=external_tool_internal_state_unavailable`。

## 普通按键重复、粘住或没有动作

检查 Raw Input 与 LL 钩子是否发生双源、抑制器是否满足 DOWN/UP 边沿配对、SendInput 是否真实返回成功。若 DOWN 泄漏进系统，UP 必须放行；应用退出、断连、睡眠和重启都必须清理按键状态。

## 安装、升级或更新失败

确认系统版本门禁、安装器返回码、当前安装身份、设置保留、updater `latest.json` HTTPS、`.sig` 验签和 SHA-256。升级前应用应正常退出并释放 BLE/音频资源；不要强杀进程或删除旧 Release 资产。

## 日志与报告

日志收集必须遵守 [LOGGING.md](LOGGING.md)，发布问题按 [Bugs/README.md](Bugs/README.md) 建立独立记录。报告中区分 `passed`、`failed`、`deferred`，不提交语音内容、个人路径、设备身份或凭据。
