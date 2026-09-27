# 按键映射误显示第三方应用的辅助窗口

- 发现日期：2026-09-27
- 状态：补充修复及自动化验证完成；本地包安装与关窗后实体按键复测待验收
- 影响范围：Windows 上使用多进程或多顶层窗口的应用；ChatGPT、WorkBuddy 为现场样本
- 功能点：按键映射 → 打开应用
- 现象：ChatGPT 被唤起时桌面出现空白窄条；WorkBuddy 偶发白屏。
- 复现条件：目标应用的主窗口暂不可见，但同身份进程存在辅助顶层窗口；触发映射打开应用。
- 正常预期：只恢复目标应用的实际主窗口，并确认该窗口成为前台；没有合格窗口时继续启动/等待，不主动显示辅助窗口。
- 证据：2026-09-27 现场只读枚举确认 ChatGPT 的 `crashpad_SessionEndWatcher` 为 136×39 顶层窗口，WorkBuddy 有 135×37 的同类隐藏窗口和 1920×1019 的隐藏 `Chrome_WidgetWin_0`。原测试包 `bce92c2` 安装后，用户又复现“关闭 ChatGPT 窗口到任务栏再打开出现白屏”：04:49:15 UTC 日志记录一个隐藏、正常尺寸候选被接受，随后 `took_show_path=true` 并误报 `foreground_observed`。现场再次枚举发现 ChatGPT 同时有一个有标题的 1102×771 `Chrome_WidgetWin_1` 主窗口和一个无标题的 960×720 `Chrome_WidgetWin_1` 空白窗口；后者也无 owner、非 tool window，旧筛选无法区分。
- 根因：最初窗口选择只检查进程/AUMID、owner、tool window 与可见性，把第一个隐藏窗口直接 `ShowWindow(SW_SHOW)`；前台读回只比对 PID。首次修复虽排除了已知辅助类并比对精确 HWND，但没有识别同样标作 `Chrome_WidgetWin_1` 的无标题预创建空白窗口。WorkBuddy 白屏具体是否每次都来自其隐藏的 `Chrome_WidgetWin_0`，旧日志未记录 HWND/窗口用途，不能反推每次现场。
- 修复：两个身份匹配入口共用候选检查，排除框架辅助窗口、极小/未布局窗口、DWM cloaked 窗口，以及无标题的 Chromium/Electron `Chrome_WidgetWin_1`；前台读回要求选定的 HWND 本身成为前台；移除注册应用中按 PID 直接判成功的捷径。日志记录候选通过/拒绝原因和尺寸分类，只读取标题长度，不记录标题内容、应用身份或个人路径。
- 验证：首次修复的候选分类、精确 HWND 判据、ChatGPT/WorkBuddy AppsFolder 启动链和独立进程 foreground lock 重试曾 `passed`，但用户关窗后再打开实测为 `failed`。补充“同尺寸、同窗口类，仅标题有无不同”的回归测试先因函数签名不符而失败，再于实现后 `passed`。单线程运行 `scripts/ci-preflight.ps1` 7/7 `passed`（第一次并行运行中无关的全局按键门控单测偶发失败）；本机 ChatGPT 与 WorkBuddy 的 AppsFolder 前台及独立进程 foreground lock 重试均 `passed`。新安装包及关窗后实体按键再次观察待执行；RC001/RC003 实体按键为 `deferred`。
- 隐私检查：日志和文档不含窗口标题、设备身份、个人路径、语音内容或凭据。
