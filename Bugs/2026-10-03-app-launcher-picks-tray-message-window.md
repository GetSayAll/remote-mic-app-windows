# 打开已运行的微信：强制显示的是托盘消息窗口，「目标应用点不动」

- 发现日期：2026-10-03
- 状态：已修复（单元测试 + 产品路径真机验证 passed；遥控器按键端到端复测待用户执行）
- 影响范围：Windows 版 main `6693a53` 及之前 7e80b26 之后的所有版本；RC001/RC003 与设备型号无关；
  影响的第三方应用：把主窗口收进托盘、且存在"消息/托盘窗口类"的应用（实测微信 4 的
  `Qt51514WxTrayIconMessageWindowClass`）
- 功能点：按键映射「打开应用」（预设应用 / 已运行注册应用切回已有窗口）
- 现象：遥控器打开已在运行、已收进托盘的微信后，目标应用窗口点不动、无法交互；
  同时可能留下一个无内容的空白窗口盖在屏幕上
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
