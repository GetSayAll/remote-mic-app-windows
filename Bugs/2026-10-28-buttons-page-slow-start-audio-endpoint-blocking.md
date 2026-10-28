# 按键映射页开关与配置内容延迟数秒（启动时音频端点恢复阻塞主线程）

## 现象

用户反馈（1.8.0）：按键映射页好几个开关显示明显延迟，按键配置内容要等几秒才出现；
主观判断"之前没有出现过"。

## 现场数据

同一台机器，诊断日志里按键页初始化耗时（`buttons_page_resource_setup elapsed_ms`）的时间序列：

| 时间（UTC） | 耗时 |
| --- | --- |
| 02:10–03:27 | 105–146 ms（正常，中位数 127 ms） |
| 03:36–03:44 | 1.0–1.5 s |
| 03:54–04:13 | 3.9–6.9 s |

首个 IPC 同样被拖住：`runtime_snapshot reason=initial_ipc_ready elapsed_ms=6382`。

## 根因

`tauri_setup phase=started` 04:13:30.134 → `phase=completed` 04:13:36.324，耗时 6.2 s，
中间是这一段：

```
audio_endpoint action=open phase=retry attempt=3 hresult=0x8889000A reason=endpoint_in_use next_delay_ms=900
audio_endpoint action=open phase=retry attempt=4 … next_delay_ms=1600
audio_endpoint action=open phase=retry attempt=5 … next_delay_ms=0
audio_endpoint action=restore phase=fallback result=attempted endpoint_kind=virtual_cable
audio_endpoint action=restore phase=completed terminal_result=passed reason=fallback_sibling elapsed_ms=3451
```

- `0x8889000A` = `AUDCLNT_E_DEVICE_IN_USE`：CABLE 输出端点被别的程序占着
  （2026-10-27 用户把输入法麦克风设为 `CABLE Output` 之后常驻占用）。
- 打开端点按 `OPEN_RETRY_DELAYS_MS = [0, 150, 400, 900, 1600]` 退避重试，等待合计 3.05 s，
  实测 `elapsed_ms=3451`，最后回退到同一台声卡的另一个端点才成功。
- 这一步（`restore_audio_endpoint`）同步跑在 `tauri::Builder::setup` 闭包里，占住主线程：
  webview 的脚本求值等到 36.333 才开始，于是 `runtime_snapshot` 6.4 s、按键页 6.9 s。

用户看到的正是"窗口骨架先出来，开关和按键配置几秒后才到"。

**为什么"之前没有"**：端点被占用是 10-27 改输入法麦克风之后才稳定的；重试期间主线程被占，
以前端点能一次打开，这条路径几毫秒就过去了。

## 修复（2026-10-28）

`restore_audio_endpoint` 只为"下一次语音"服务，没有任何理由挡住窗口——移到独立线程
（与 `rc003_auto_trigger_reconcile` 同一模式）。恢复本身照做，失败仍只记日志、不阻断启动。

新增判据：`audio_endpoint action=restore phase=background_completed elapsed_ms=<n>`。

## 判据

- **正常**：`tauri_setup phase=completed` 与 `phase=started` 的间隔回到 1 s 级；
  按键页 `buttons_page_resource_setup elapsed_ms` 回到 100–200 ms；
  `runtime_snapshot … initial_ipc_ready` 不再出现 4 s 以上的值。
- **audio 恢复仍会发生**：即使端点被占，日志里应出现
  `audio_endpoint action=restore phase=background_completed`（在 `tauri_setup` 完成之后），
  值可以是 3000 ms 量级——**这是允许的**，它不再影响界面。
- 反例：`tauri_setup` 再次超过 3 s，且这行 `background_completed` 出现在它之前
  ⇒ 又把阻塞调用放回主线程了。

## 顺带修掉的两处资源浪费

同一批（2026-10-28）：

| 资源 | 原 | 现 |
| --- | --- | --- |
| `RC003-remote-photo@2x`（按键页遥控器照片） | PNG 320,484 B | WebP 41,406 B |
| `app-logo`（侧栏应用标识） | PNG 278,773 B | WebP 35,064 B |
| `app-icon-faceted-duck`（设置页预览） | PNG 65,930 B | WebP 11,776 B |

三张图都是"实际显示尺寸远小于像素尺寸"的照片类 PNG，转 WebP 后显示效果不变
（WebView2 原生支持）。遥控器照片另加了 `index.html` 的 `preload`，避免"切到按键页才开始加载"。

## 验证

- 自动化 `passed`：前端 36 文件 / 560 passed / 10 skipped，`vue-tsc --noEmit`、`cargo check` 通过；
- 真机 `deferred`：需要在端点被占用的机器上确认上面三条判据。

## 隐私检查

本文只引用日志字段、端点类型与耗时，未包含语音内容、输入框文本、个人路径或令牌。
