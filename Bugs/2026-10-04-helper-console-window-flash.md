# 打开全按键支持 / 启动时助手窗口一闪而过

- 发现日期：2026-10-04（用户现场反馈）
- 状态：已修复（本地测试包已在本机安装并实测；发布动作未授权、未执行）
- 影响范围：Windows 10/11 x64；无线麦 SayAll Windows 版「全按键支持」（RC003 增强捕获轨）；
  RC001 不经过该助手，不受影响
- 功能点：RC003 增强捕获 Helper（`sayall-helper.exe`）——计划任务触发（`schtasks /run`）
  与提权安装拉起（`ShellExecuteExW(runas)`）两条启动路径
- 现象：App 启动（开关保持开启时自动拉起助手）或拨动「全按键支持」开关后，
  助手运行会"闪"出一个窗口，用户困惑
- 复现条件：
  1. 打开「全按键支持」（UAC 允许）→ 触发计划任务；
  2. 开关保持开启，重启 App → 启动时自动触发。
  本机用同一条 `/run` 通路复现（非提权任务；提权任务需现场点一次 UAC）
- 正常预期：助手在后台运行；用户不应看到任何窗口，也不应出现前台焦点被抢走
- 证据：
  - `hardware/RC003/evidence/2026-10-04-console-flash-baseline.json`
    （基线：控制台子系统助手 → conhost 子进程 + 一个 Windows Terminal 窗口，
    与助手同生命周期可见 2.2–2.3 s；2/2 轮）
  - `hardware/RC003/evidence/2026-10-04-console-flash-control-cmd.json`
    （对照：`cmd.exe /c ping` 同样产生可见窗口、与进程同生命周期——证明机制成立）
  - `hardware/RC003/evidence/2026-10-04-console-flash-fixed.json`
    （修复后：3/3 轮零 helper 相关 conhost、零相关窗口，助手照常跑完）
  - `hardware/RC003/evidence/2026-10-04-console-flash-fixed-taskstatus.json`
    与 `2026-10-04-console-flash-counterfactual-no-cnw.json`
    （子进程路径：`schtasks` 带 `CREATE_NO_WINDOW` → 0 窗口；临时去掉该标志的反向
    对照构建 → 窗口闪现 127–235 ms，证明该标志必不可少）
  - `hardware/RC003/evidence/2026-10-04-helper-console-elimination.log`（汇总与日志摘录）
- 根因（已确认）：
  - 助手此前是**控制台子系统**程序。计划任务 / `ShellExecuteEx` 拉起时系统为其分配
    控制台；本机默认终端（Windows Terminal）把控制台显示为一个可见窗口，并在助手
    存活期间一直可见——`hide_console_window()`（`ShowWindow(GetConsoleWindow(), SW_HIDE)`）
    在 ConPTY 托管下只拿到隐藏的 pseudo window，对可见窗口无效（Ctrl 实测：
    窗口可见 2.2–2.3 s，退出时才消失）。
  - 提权任务（`/rl highest`）不参与默认终端移交，预计退化为经典 conhost 黑框、
    自隐藏前可见——即用户看到的"一闪而过"。该形态本机无法复现（当前会话无管理员
    权限，无法点 UAC）；结构性结论不依赖它：只要进程是控制台子系统，就会被系统
    分配控制台窗口。
- 修复（最小改动）：
  - `hardware/RC003/helper/src/main.rs`
    - `#![cfg_attr(windows, windows_subsystem = "windows")]`：GUI 子系统，从根上
      不再创建控制台（两条拉起路径都不再可能有窗口）；
    - `hide_console_window()` → `bootstrap_console()`：手动运行（cmd / PowerShell /
      `run-helper*.cmd`）`AttachConsole(ATTACH_PARENT_PROCESS)` + 无效句柄接
      `CONOUT$`，输出照旧可见；无父控制台（计划任务 / 提权安装）时 stdout/stderr
      指向 `NUL`——`Logger::line` 每次都 `println!`，无效句柄会让它 panic，NUL 让
      打印成为无害空写；
    - 子进程 `schtasks` 补 `CREATE_NO_WINDOW`（否则 GUI 子系统父进程下这个控制台
      子进程会被新分配控制台 = 又一个窗口）；
    - `[ENV]` 日志新增 `console=attached|nul`；自检新增「stdout/stderr 可写」回归项。
  - `src-tauri/src/rc003_task.rs`：提权安装 `nShow` 1 → 0（SW_HIDE 双保险；
    纵使拉起的是未升级的旧版控制台助手也不闪）。
  - `src-tauri/src/lib.rs`：`refocus_main_window_soon` 注释更新（该兜底保留为廉价
    保险，真机确认全程无焦点扰动后可删除——不在本次改动中删）。
- 验证（2026-10-04，本机 Windows 11；除注明外均实际执行并观察）：
  - 计划任务路径（非提权，同一条 `/run` 通路）：修复后 3/3 轮无 conhost、无窗口；
    `--selftest` 任务退出码 0（`schtasks /query /v` 上次结果 0），日志 `console=nul`、
    「控制台引导：stdout/stderr 可写」PASS、结论「全部通过」= `passed`；
  - 手动路径：控制台运行 `--selftest` 输出可见、`console=attached`、退出码 0 = `passed`；
  - helper 侧自动化：`cargo fmt --check` + `cargo test --release`（23 passed）= `passed`；
  - 主程序侧自动化：`cargo fmt --all -- --check`、`cargo test --workspace`、
    `cargo check --workspace`、`cargo check -p sayall-windows-app --features
    runtime-simulation`（见提交说明）= `passed`；
  - **本地测试包实测（2026-10-04 22:15 起，安装到本机 currentUser 目录）**：
    安装器
    `target/release/bundle/nsis/无线麦 SayAll_0.5.0_x64-setup.exe`
    （sha256=52ff53b0…）静默安装退出码 0；安装后 `sayall-helper.exe`
    与构建一致（sha256=e7e1ee29…）且 PE Subsystem=2（GUI）。真机路径观测：
    ① App 启动自动触发（22:16:34）→ 日志 `elevated=true … console=nul`；
    ② 受控触发真实计划任务（22:19:00，探针 30 s）→ 新助手 elevated=true、
    `console=nul`，零助手相关控制台窗口、零 conhost 子进程；
    ③ 现场拨动开关走启用/授权路径（22:19:02 关 → 22:19:11 开，UAC 由用户点击）→
    `enable … terminal_result=passed`、`elevated_install … exit_code=0`，
    同一观测窗口无任何新增可见窗口 = `passed`（本机）。
  - **deferred**：RC001/RC003 真机按键链路与睡眠恢复（与窗口路径无关，未覆盖）；
    发布相关动作未执行（未获授权）。
- 隐私检查：证据只含进程 id、窗口类别/标题、构建 sha256 与助手脱敏日志行；
  不含设备身份、语音内容、个人路径或凭据。
