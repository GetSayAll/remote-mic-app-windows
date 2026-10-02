# 打开已运行的微信：强制显示的是托盘消息窗口，「目标应用点不动」

- 发现日期：2026-10-03
- 状态：两轮修复均已实现并做过产品路径真机验证；**遥控器按键端到端复测待用户执行**，
  第二轮（"点不动"）的修复效果**尚未在真实托盘状态上确认**
- 影响范围：Windows 版 main `6693a53` 及之前 7e80b26 之后的所有版本；RC001/RC003 与设备型号无关；
  影响的第三方应用：把主窗口收进托盘的应用（实测微信 4；注册应用路径实测 WorkBuddy）
- 功能点：按键映射「打开应用」（预设应用 / 已运行注册应用切回已有窗口）
- 现象：
  1. 打开已收进托盘的微信时，被显示并置前的是**托盘图标消息窗口**（1920×1025、无内容）；
  2. 即便显示的是真正主窗口，用户仍报"**打开后点不动**"——窗口可见但客户端区点击不进入应用，
     双击标题栏（非客户区，由系统处理）之后才能关闭
- 复现条件：
  1. 微信先启动并"关闭到托盘"（主窗口 `Qt51514QWindowIcon` 隐藏）；
  2. 遥控器按已映射为"打开应用（微信）"的按键；
  3. 观察前台窗口与点击响应。
- 正常预期：微信真正的主窗口被显示并置于前台，点击可用
- 证据：
  - 候选枚举实测（2026-10-03，本机，`EnumWindows` 顺序 = 启动器遍历顺序）：
    `Qt51514WxTrayIconMessageWindowClass`（1920×1025、无 owner、非 `WS_EX_TOOLWINDOW`、无内容）
    排在 `Qt51514QWindowIcon`（1134×865，真正主窗口）之前，且**两者都通过当时的候选过滤**；
    当时规则取"第一个隐藏候选"，即选中托盘消息窗口。
  - 单独显示该消息窗口的后果实测：`ShowWindow(SW_SHOW)` 后它变为可见、覆盖整屏并抢到前台，
    其内部无任何 UI，点击全部落在它身上 → 与用户报告"点不动"一致。
  - 结构化日志（修复前）：`app_launcher action=window_candidate terminal_result=accepted
    reason=main_candidate visible=false size_class=normal` ×2（两个候选），随后
    `show_and_force_foreground passed ... visible_before=false took_show_path=true`，
    日志不含被选中窗口身份 → 无法从日志断言选错窗口（本 Bug 的第二个问题）。
  - 排除项：本次冻结期间应用与助手进程均无忙等（app 0.19 s/6 s、bridge 线程 0.06–0.09 s）、
    无 ≥8 s 日志断档、`tx_stall=0`、`map_launch result=ok`（22 ms–2.16 s），
    因此与 #168 的出站忙等、#170 的上行串行化无关。
- 根因：候选过滤 `window_rejection_reason` 的辅助窗口类清单不含消息/托盘窗口类；而
  "已运行 → 切回已有窗口"路径按枚举顺序取第一个隐藏候选并强制显示，于是把无内容的
  托盘消息窗口当成主窗口显示并置前。
- 修复（`crates/sayall-windows/src/app_launcher.rs`，最小改动）：
  1. `window_rejection_reason` 增加 `MessageWindow` / `TrayIcon` 类名规则（`auxiliary_class`）；
  2. 记录被选中窗口的类名与每次尝试：`show_and_force_foreground class=...`、
     `window_candidate ... class=...`、`hidden_candidate_attempt attempt=N class=...`；
  3. 隐藏候选改为按枚举顺序取前 3 个逐个尝试（`activate_hidden_candidates`），
     置前后做有界读回复核（`readback_with_recheck`，2 次 ×120 ms），未生效的候选若本次是
     我们显示出来的会恢复隐藏，避免留下空白窗口。
- 验证：
  - 单元测试：`window_candidate_rejects_tray_message_windows`（先红后绿，真实类名 +
    真主窗口类负例）、`foreground_readback_rechecks_before_giving_up`（含边界：复核封顶、
    首次成功不等待；已用变异检查确认判别力）。
  - 真机（产品同路径，`cargo run -p sayall-windows --example registered_app_activate_probe -- wechat`，
    微信处于托盘隐藏状态）：前台读回 = `Qt51514QWindowIcon`（真正主窗口）、主窗口中心命中
    ON TOP、托盘消息窗口始终 `visible=0`；日志：
    `window_candidate ... rejected reason=auxiliary_class class=Qt51514WxTrayIconMessageWindowClass`、
    `hidden_candidate_attempt attempt=1 class=Qt51514QWindowIcon terminal_result=passed` → `passed`
  - 回归基线：`cargo test --workspace`（247+55+29+4+1+1 全绿）、`cargo check --workspace`、
    `cargo check -p sayall-windows-app --features runtime-simulation`、`cargo fmt --all -- --check` → `passed`
  - 待用户执行（`deferred`）：遥控器按"打开应用"键触发同一条路径的端到端复测，
    以及微信/WorkBuddy 各自的真实点击可用性确认
- 隐私检查：日志与本文仅含窗口类名（框架公开字符串）与计数，不含窗口标题、个人路径、
  设备身份或语音内容

## 第二轮（2026-10-03 用户复测后追加）：「打开后点不动」

- 用户复测结论：第一轮修复（不再选中托盘消息窗口）生效，但**微信与 WorkBuddy 打开后仍然
  点不动**；双击标题栏（非客户区，由系统处理）之后才能操作/关闭。
- 追加根因：直接 `ShowWindow` 一个被**应用自己**收进托盘的窗口，只改变操作系统的可见性，
  应用内部仍认为窗口处于隐藏态——窗口出现了，但客户端区点击不进入应用。开始菜单/桌面
  快捷方式打开同一应用时窗口正常可用，差别就在于窗口是由**应用自己**恢复的，内部状态一致。
  本仓库自身窗口早有同类记录：Win32 `ShowWindow` 会让 tao 的 `WindowFlags::VISIBLE` 缓存
  脱节（见 `activate_or_launch` 的 SELF_SHOW_SYNC 注释），这里只是换成了第三方应用。
- 追加修复：
  1. `window_visibility(WindowSelector::{ExecutableNames, AppUserModelId})`：只读枚举目标
     当前可见/隐藏候选窗口数量与第一个隐藏候选；静默版过滤（不写候选日志，避免翻倍）。
  2. `running_window_disposition(visible, hidden)`（纯逻辑）：有可见窗口 → 直接激活；
     **只有隐藏窗口 → `DelegateToApp`**；没有窗口 → 启动。
  3. 预设应用（微信）：`DelegateToApp` 时按入口（开始菜单快捷方式 → 安装路径 → exe 名）
     重新拉起——与"其他打开方式"一致；随后**有界复核"原先那个隐藏窗口"是否自己变可见**
     （6×200 ms）。只数"有没有可见窗口"会被应用一闪而过的提示窗口骗过（本机实测假阳性），
     因此收紧为按已知句柄复核；未观察到才回落到旧的"显示隐藏窗口 + 置前"路径（保持兜底）。
  4. 注册应用（WorkBuddy / Word / WPS）：`launch_registered_app` 的预激活之前先做可见性
     判断，只有隐藏窗口时跳过直接激活、改走应用自己的激活契约（`ActivateApplication`），
     日志 `prelaunch_activation result=deferred reason=windows_hidden_by_app`。
- 追加验证：
  - 单元测试：`running_window_disposition_prefers_app_owned_opening_for_hidden_windows`
    （可见 → 激活 / 仅隐藏 → 交给应用 / 无窗口 → 启动）；既有 27 项 app_launcher 测试全绿。
  - 真机（产品同路径探针，微信隐藏态）：`window_visibility` 判定 `visible=0 hidden=1` →
    `phase=relaunch_entry` → `source=start_menu_shortcut` 拉起；日志含 `terminal_result=failed
    reason=app_window_not_observed` → 回落 `show_and_force_foreground`，行为与旧路径一致
    （本机合成隐藏态无法复现"应用内部自认隐藏"，故应用自己显示窗口的成功路径在本机不可判定）。
  - **`deferred`**：第二轮修复的真实效果必须由用户按遥控器（微信 / WorkBuddy 均先收进托盘）
    复测确认；若仍点不动，下一个候选机制是改走应用托盘图标的官方入口或对目标窗口做
    文档化的激活+布局唤醒序列。

