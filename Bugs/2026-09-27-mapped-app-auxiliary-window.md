# 按键映射误显示第三方应用的辅助窗口

- 发现日期：2026-09-27
- 状态：代码及本地安装启动已验证；冷态白屏与 RC001/RC003 实体按键待验收
- 影响范围：Windows 上使用多进程或多顶层窗口的应用；ChatGPT、WorkBuddy 为现场样本
- 功能点：按键映射 → 打开应用
- 现象：ChatGPT 被唤起时桌面出现空白窄条；WorkBuddy 偶发白屏。
- 复现条件：目标应用的主窗口暂不可见，但同身份进程存在辅助顶层窗口；触发映射打开应用。
- 正常预期：只恢复目标应用的实际主窗口，并确认该窗口成为前台；没有合格窗口时继续启动/等待，不主动显示辅助窗口。
- 证据：2026-09-27 现场只读枚举确认 ChatGPT 的 `crashpad_SessionEndWatcher` 为可见的 136×39 顶层窗口，WorkBuddy 有 135×37 的同类隐藏窗口和 1920×1019 的隐藏 `Chrome_WidgetWin_0`。这些窗口均无 owner、非 tool window，符合旧筛选条件；历史诊断日志多次出现 `visible_before=false took_show_path=true`，仍记录 `foreground_observed`。用户截图中窄条形状与前者相符。
- 根因：原窗口选择只检查进程/AUMID、owner、tool window 与可见性，把第一个隐藏窗口直接 `ShowWindow(SW_SHOW)`；前台读回只比对 PID，同进程的辅助窗口也会被误报成功。WorkBuddy 白屏具体是否每次都来自其隐藏的 `Chrome_WidgetWin_0`，旧日志未记录 HWND/窗口用途，不能反推每次现场。
- 修复：两个身份匹配入口共用候选检查，排除框架辅助窗口、极小/未布局窗口和 DWM cloaked 窗口；前台读回要求选定的 HWND 本身成为前台；移除注册应用中按 PID 直接判成功的捷径。日志记录候选通过/拒绝原因和尺寸分类，不记录窗口标题、应用身份或个人路径。
- 验证：新增候选分类与精确 HWND 判据回归测试；Windows 上通过公开 AppsFolder 入口分别调用 ChatGPT、WorkBuddy 的生产启动链及独立进程持有 foreground lock 的重试测试，均 `passed`，最终前台 WorkBuddy 窗口类为 `Chrome_WidgetWin_1`。`scripts/ci-preflight.ps1` 的 7 项检查在 CI 模式下全部 `passed`；本地未签名 NSIS 包构建、结构校验、同版本覆盖安装、启动及 BLE 自动重连 `passed`。冷启动白屏观察及 RC001/RC003 实体按键为 `deferred`。
- 隐私检查：日志和文档不含窗口标题、设备身份、个人路径、语音内容或凭据。
