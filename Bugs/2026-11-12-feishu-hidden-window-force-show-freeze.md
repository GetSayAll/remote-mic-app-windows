# 菜单键打开飞书：隐藏窗口被强制显示后界面不可用

- 发现日期：2026-11-12
- 状态：六轮修复完成并出本地测试包（产品同路径探针/真机可见性探针 passed；待遥控器/安装版复测）
- 影响范围：Windows 版 1.9.0（`8448abd`）；RC003 菜单键映射到飞书桌面快捷方式；自定义 exe/lnk 目标均可能受影响
- 功能点：按键映射 → 打开应用（custom `.lnk` / `.exe`）
- 现象：飞书已运行并收进托盘后，按菜单键会把隐藏的主窗口强行显示出来；窗口可见、进程 `Responding=true`，但界面点击不进入应用，表现为卡死。再次按键可能重复强制显示，或落到重新启动入口。
- 复现条件：
  1. 飞书已启动，关闭到托盘（主窗口 `Chrome_WidgetWin_1` 隐藏）；
  2. 菜单键目标为飞书桌面快捷方式（custom `.lnk`，解析到 `%LOCALAPPDATA%\Feishu\Feishu.exe`）；
  3. 按菜单键。
- 正常预期：由飞书自己的托盘/单实例入口恢复主窗口，窗口内部状态一致、可交互；SayAll 不应直接 `ShowWindow` 应用自己隐藏的窗口。
- 证据（本机 2026-11-12，日志时间 UTC，本地 +8）：
  - `02:38:08.200` / `02:38:10.972`：`window_candidate ... class=Chrome_WidgetWin_1 visible=true minimized=true` → `show_and_force_foreground ... visible_before=true took_show_path=false`，两次恢复最小化窗口正常。
  - `02:38:12.888`：`window_candidate ... class=Chrome_WidgetWin_1 visible=false minimized=false` → `02:38:12.905 show_and_force_foreground ... visible_before=false took_show_path=true` → `hidden_candidate_attempt ... terminal_result=passed`，随后用户报告界面不可用。
  - `02:40:19.243`：同一路径再次出现 `visible=false` + `took_show_path=true`。
  - `responder` 检查：窗口 `IsWindowVisible=true`、`IsHungAppWindow=false`、`DwmGetWindowAttribute(DWMWA_CLOAKED)=0`、`SendMessageTimeout(WM_NULL)=1`；说明不是 OS 级挂起，而是应用内部显示状态脱节。
  - `%LOCALAPPDATA%\Feishu\Feishu.exe` 是启动器，实际窗口属于同目录 `app\Feishu.exe`；`activate_running` 按 exe 名匹配到全部飞书进程，因而能看到并显示隐藏主窗口。
- 根因（代码）：
  - `crates/sayall-windows/src/app_launcher.rs:1131` 的 custom `.exe` / `.lnk` 分支直接调用 `activate_running`；
  - `activate_running` → `activate_process_windows` → `activate_hidden_candidates`（`app_launcher.rs:2034`、`:2538`）在没有可见窗口时逐个 `ShowWindow(SW_SHOW)` 隐藏候选（`:2481`）；
  - 2026-10-03 的「隐藏窗口交给应用自己打开」修复只覆盖预设应用与注册应用，custom 路径没有走 `running_window_disposition`。
- 修复（已实现，`crates/sayall-windows/src/app_launcher.rs`）：
  1. custom `.exe` / `.lnk` 先解析真实 exe 与参数，查询窗口可见性；
  2. 只有可见/最小化窗口才走 `activate_running`；
  3. 主窗口隐藏时改走托盘图标 → 应用自身启动入口（`.lnk`/exe）→ 有界复核原隐藏窗口自己变可见 → 再置前；
  4. 不复用 `activate_hidden_candidates` 直接显示第三方隐藏窗口；
  5. 日志增加 custom 路径的 phase/terminal_result，仍不记录路径。
- 验证：
  - 单元测试 `custom_path_hidden_window_delegates_to_app_instead_of_force_show` 先红后绿；
    `cargo test -p sayall-windows --lib`：**547 passed / 0 failed / 16 ignored**。
  - 产品同路径探针（`app_readiness_probe --activate <飞书桌面快捷方式>`，先把已运行的主窗口
    置为隐藏态）：日志看到 `phase=tray_icon matched=false method=not_found` →
    `phase=relaunch_entry reason=window_hidden_by_app` → 应用自己把原隐藏窗口变可见
    （`window_observed=true`）→ `show_and_force_foreground visible_before=true took_show_path=false`
    → `terminal_result=passed`；**没有再出现 `took_show_path=true`**。
  - `cargo fmt --all -- --check` → passed；`cargo check -p sayall-windows-app --features runtime-simulation` → passed；
    `cargo test --workspace` → passed（sayall-windows 547 + app 218 等，0 failed）。
  - 本地测试包（**已被第二轮包取代**）：`target/release/bundle/nsis/无线麦 SayAll_1.9.0_x64-setup.exe`
    （SHA-256 `C8CB49808A596A30CA759C3AB976C10DA471741DD7295A0787EAA6E77FEAB16D`）；
    前端嵌入校验 18 个资源全部 embedded；静默安装 exit 0，安装版 exe 含新日志串，
    与 staging exe 仅差 3 字节（NSIS bundle 标记，预期）。
  - 待用户用安装包按「飞书关闭到托盘 → 遥控器菜单键」复测；RC001/RC003 分别记录。
- 相关未修边界：
  - 注册应用走 `activate_install_directory_family` 时，可见性判定仍只覆盖 AUMID + 精确 exe 路径；
    若窗口属于同安装目录的兄弟进程，可能绕过隐藏判定。本次本机复现是 custom `.lnk`，未改该路径。

## 第二轮（2026-11-12 用户复测后）：菜单键「本机扫描」的注册应用仍卡死

- 用户复测：主页配置的飞书 `.lnk` 能打开但有延迟/鼠标转圈；菜单键改为「本机扫描」添加的
  `shell:AppsFolder\com.electron.Feishu` 后，打开直接卡死。
- 现场日志（`03:01:32Z`，本地 11:01）：
  - `registered_app_launch phase=identity_resolved aumid_available=true executable_path_available=true`
  - 没有隐藏分支；随后 `window_candidate ... class=Chrome_WidgetWin_1 visible=false` →
    `show_and_force_foreground ... visible_before=false took_show_path=true` →
    `prelaunch_activation result=activated source=install_directory`。
- 根因：注册项解析到根目录 `Feishu.exe`，真正持有窗口的是同目录 `app\Feishu.exe`；
  `launch_registered_app` 的隐藏判定只查 AUMID + 精确 exe 路径，两者都命中不了窗口，
  于是进入 `activate_install_directory_family`，该兜底没有隐藏窗口判定，直接把隐藏主窗口
  `ShowWindow(SW_SHOW)` 出来。
- 修复：
  1. `WindowSelector::InstallDirectoryFamily`：把同安装目录同族 PID 纳入可见性查询；
     `launch_registered_app` 在隐藏判定前合并 `by_identity + by_path + by_family`。
  2. `pids_for_install_directory_family` 与激活兜底共用同一份进程扫描，避免两套口径。
  3. `deferred_hidden_window` 存在时，`foreground_readback` 不再调用
     `observe_registered_foreground` 的隐藏候选兜底；只接受应用自己显示出来的窗口。
- 验证（产品同路径探针，先把飞书主窗口置为隐藏态）：
  - `phase=tray_icon failed`（飞书托盘图标不在 UIA 可枚举范围）
  - `phase=prelaunch_activation result=deferred reason=windows_hidden_by_app visible=0 hidden=1`
  - `phase=activation_contract method=application_activation_manager terminal_result=failed`
  - `phase=activation_contract method=shell_fallback terminal_result=submitted`
  - `phase=app_self_shown terminal_result=passed`
  - `phase=completed terminal_result=passed target_result=foreground_observed elapsed_ms=659`
  - **没有** `took_show_path=true`；飞书进程集合未变（单实例恢复）。
- 新本地测试包：
  `target/release/bundle/nsis/无线麦 SayAll_1.9.0_x64-setup.exe`
  （SHA-256 `AA983126DA3D4F8D4FBAF71CFF4607D915D54A24E86426D3912A7815FEEECBF7`）；
  安装器 exit 0，安装版 exe 含 `phase=foreground_readback target_result=deferred` 新日志串。
- 待用户复测：菜单键注册应用「飞书关闭到托盘 → 按菜单键」；主页 `.lnk` 的延迟是否可接受。
- 隐私检查：未新增路径、标题、设备身份或语音内容到日志。

## 第三轮（2026-11-12 用户复测后）：冷启动首按偶发未到前台

- 用户复测：有一次没有拉到前台，鼠标转圈。
- 现场日志（`03:22:06Z`，本地 11:22）：
  - `registered_app_launch phase=prelaunch_activation result=not_running`
  - `activation_contract method=shell_fallback terminal_result=submitted`
  - `foreground_readback method=shell_fallback target_result=foreground_denied`
  - `completed terminal_result=passed target_result=launched_without_foreground elapsed_ms=2165`
  - 同时间段没有 `focus_wait_foreground`，也没有 `open_app_profile_foreground phase=retry`。
- 根因：`OpenOnly` 策略的冷启动前台等待/重试被
  `is_button_profile_host_available()` 门控；公开构建不注入私有键位方案宿主，
  门控恒为 false，整段等待被跳过。`launch_registered_app` 自身的 1.9 秒读回预算
  先耗尽，窗口随后才出现，于是首按停在后台。
- 修复（`crates/sayall-windows/src/button_mapping.rs`）：
  - 新增 `ensure_open_only_foreground`：先有界等 3 秒，失败按应用入口重试一次，
    再等 2.5 秒；日志 `open_app_profile_foreground phase=retry|completed`。
  - 移除私有宿主门控，公开构建同样执行。
  - 回归测试 `open_only_foreground_retries_without_private_button_profile_host`
    （默认宿主不可用时仍等待 2 次、重试 1 次）。
- 验证：`cargo test --workspace` passed（sayall-windows 548 + app 218 等，0 failed）；
  `cargo fmt --all -- --check` passed。
- 新本地测试包：
  `target/release/bundle/nsis/无线麦 SayAll_1.9.0_x64-setup.exe`
  （SHA-256 `5330E2977CDCBB461E9D090D680EF063CA97E91B486D6ED8A5F4696F8E78B06A`）；
  安装器 exit 0，安装版 exe 含新等待/重试日志串。
- 代价：冷启动首按最坏会阻塞约 5.5 秒（3 秒 + 重试 2.5 秒）；窗口通常在前 3 秒内
  出现，因此正常路径只多等 0–3 秒。后续可改为后台任务以彻底避免引擎线程阻塞。
- 待用户复测：彻底退出飞书后按菜单键，确认首按能到前台；以及连续按、断连/睡眠恢复。
- 隐私检查：未新增路径、标题、设备身份或语音内容到日志。

## 第四轮（2026-11-12 用户复测后）：关掉飞书后它又自己打开

- 现象：飞书窗口打开后，用户把窗口关掉，过一会儿它又被重新打开。
- 现场日志（`03:37:54Z–03:38:42Z`，本地 11:37–11:38）：
  - `registered_app_launch ... target_result=foreground_observed`
  - 紧接着 `focus_wait_foreground phase=completed terminal_result=timeout elapsed_ms=3057`
  - `open_app_profile_foreground phase=retry` → 再次 `registered_app_launch` / `shell_fallback`
  - 同样的 timeout/retry 重复多轮。
- 根因（两层）：
  1. `target_window_visibility` 对注册应用只查 AUMID + 精确 exe 路径，对 custom `.lnk`
     只查解析后的精确 exe 路径；飞书窗口属于同目录 `app\Feishu.exe`，所以前台等待
     永远看不到它，即使 `registered_app_launch` 已经 `foreground_observed`。
  2. 前台等待超时后的“重试”调用的是 `launch_app`（启动/激活），不是“只激活”；
     用户此时已经关掉窗口，重试就把它重新启动。两条叠加表现为“关掉后又自己打开”。
- 修复：
  1. `target_window_visibility` 的注册应用分支合并 `InstallDirectoryFamily`；
     custom `.lnk` 分支也合并同族窗口（空 AUMID，仅按安装目录限定）。
  2. 新增 `activate_existing_window`：只激活**已经可见**的窗口，不启动、不显示隐藏窗口。
  3. `ensure_open_only_foreground` 的重试改为 `activate_existing_window`；第二轮等待
     末尾再做一次“可见窗口”激活 + 500ms 复核；没有可见窗口就保持关闭。
  4. 回归测试：`open_only_foreground_never_relaunches_a_closed_target`（没有可见窗口时
     只尝试激活两次、绝不启动）；`feishu_family_visibility_matches_foreground_wait_selector`
     （真机 ignored，本机以 `SAYALL_TEST_FEISHU_TARGET` / `SAYALL_TEST_FEISHU_LNK` 手动跑过，
     `passed`）。
- 验证：`cargo test --workspace` passed（sayall-windows 549 + app 218 等，0 failed）；
  `cargo fmt --all -- --check` passed。
- 新本地测试包：
  `target/release/bundle/nsis/无线麦 SayAll_1.9.0_x64-setup.exe`
  （SHA-256 `D74757638506CAD30586E2F88B1B30504A4E34246BE7254E756ABBFF3876D66F`）；
  安装器 exit 0，安装版 exe 含 `phase=retry_activate` 新日志串。
- 待用户复测：飞书打开后立即关闭，确认不再自动重开；冷启动首按仍应到前台。
- 隐私检查：未新增路径、标题、设备身份或语音内容到日志。
- 隐私检查：本文不含完整个人路径、窗口标题、设备身份或语音内容；仅保留公开窗口类名、进程数量与时序。

## 第五轮（2026-11-12 用户反馈「有延迟」）：缩短冷启动/恢复的前台等待

- 用户反馈：菜单键/主页打开飞书有明显延迟。
- 日志定位（2026-11-12，UTC）：
  - 正常路径已经很快：注册应用隐藏恢复 `completed elapsed_ms≈300–320`，可见但未置前
    `≈100–113`，custom `.lnk` 可见/最小化 `≈30–40`，`focus_wait_foreground` 40–57ms。
  - 慢点一：窗口可见但未到前台时，`focus_wait_foreground phase=completed
    terminal_result=timeout elapsed_ms=3009`（04:02:55）、`3092`（04:03:21），随后仍固定
    再等 `2559`/`2564ms`；旧实现最坏 5.5 秒才结束，而且窗口在前 3 秒内到前台也要等满。
  - 慢点二：`registered_app_launch phase=app_self_shown terminal_result=failed` 固定消耗
    1.2 秒（04:04:50.415 → 04:04:51.617）；这次窗口在 49ms 后就已置前
    （04:04:51.668 `focus_wait_foreground ... passed elapsed_ms=49`），读回预算白等。
- 修复：
  1. `ensure_open_only_foreground` 改为轮询：先做一次零等待检查（已在前台立即返回）；
     之后最多 8 轮，每轮「只激活已存在的可见窗口 + 等 500ms」，窗口一到前台立即返回。
     仍只调用 `activate_existing_window`，不启动、不显示隐藏窗口——第四轮「关掉后又
     自己打开」的保证不变。
  2. 前台保障预算从 5.5 秒降到 4.0 秒；窗口出现后最多再多等 500ms（旧实现最多 3 秒）。
  3. `app_self_shown` 读回改用 `readback_hidden_window_shown_brief`（3 次 × 200ms =
     0.6 秒，旧值 6 次 = 1.2 秒）；成功路径通常在 300ms 内，不受影响，失败路径少等 0.6 秒。
  4. 回归测试：`open_only_foreground_returns_immediately_when_already_foreground`
     （已在前台：1 次等待、0 次激活）；既有
     `open_only_foreground_retries_without_private_button_profile_host` 与
     `open_only_foreground_never_relaunches_a_closed_target` 改为 2 轮断言。
- 验证：
  - `cargo fmt --all -- --check` → passed；
  - `cargo test --workspace` → passed（sayall-windows 550 passed / 17 ignored，
    sayall-windows-app 218 passed / 2 ignored，0 failed）。
  - 本地测试包：
    `target/release/bundle/nsis/无线麦 SayAll_1.9.0_x64-setup.exe`
    （SHA-256 `2A2698EC716BE9FC6073A3496012D4746317D9B03A871FD1736FB1310C5C5275`）；
    前端嵌入 18/18；静默安装 exit 0；安装版 exe 与 staging exe 仅差 3 字节
    （NSIS bundle 标记，预期）；已重启安装版并连上遥控器。
- 未覆盖：真实遥控器按键的端到端时延（RC001/RC003）、冷态首用，以及「关掉后不重开」
  仍待用户真机复测。自动化测试只证明轮询边界与「只激活、不启动」的契约。
- 隐私检查：未新增路径、标题、设备身份或语音内容到日志；本文只保留时序与日志 token。

### 第五轮补充：主页键比菜单键慢约 0.3 秒（非回归，保持现状）

- 用户复测认为「菜单键更快，主页键相对慢」。日志（2026-11-12 04:18–04:20Z）
  显示差异不在打开路径：
  - 菜单键未配置双击/长按，走原始路径，**按下沿立即触发**；隐藏恢复约 304ms，
    可见恢复约 91ms，加前台等待合计约 170–350ms。
  - 主页键配置了双击（Win+D）和长按（Alt+Tab），单击必须等 300ms 双击窗：
    释放到触发实测 300–315ms；隐藏恢复约 372ms，可见恢复约 92ms，
    合计约 440ms（可见）/ 700ms（隐藏）。
  - 两条路径的打开成本只差约 70ms，其余全是手势消歧时间。
- 可选优化（均需改手势语义，本次未实施）：双击窗从按下沿计时（保留手势，
  典型快 0.1–0.2 秒）、双击窗 300→200ms、或主页键去掉双击/长按。
- 用户决定：保持现状，不改手势语义。

## 第六轮（2026-11-12 代码评审后）：前台保障移出引擎线程 + 失败终态日志

- 评审发现：`ensure_open_only_foreground` 在按钮分发的引擎线程内同步执行，
  目标应用 4 秒内到不了前台时，后续按键会排队；公开构建在第三轮之前完全
  不等待，因此这是新增的耦合。自定义隐藏目标的启动入口失败时，日志只有
  `reason` 没有终态。
- 修复：
  1. 新增 `schedule_open_only_foreground`：在后台线程执行 8×500ms 的前台保障，
     引擎立即返回。全局 generation 每次打开递增，旧任务在下一轮检查时退出
     （`open_app_profile_foreground phase=completed target_result=superseded`），
     不会在新一次打开之后继续抢焦点。
  2. 线程创建失败记录 `target_result=skipped reason=thread_unavailable`。
  3. 自定义隐藏目标的 `launch_explicit` 失败补
     `terminal_result=failed reason=launch_entry_failed window_observed=false`。
- 行为保证不变：后台任务只调用 `activate_existing_window`，不启动、不显示
  隐藏窗口；`open_only_foreground_never_relaunches_a_closed_target` 继续通过。
- 回归测试：新增 `open_only_foreground_stops_when_superseded_by_a_newer_open`
  （零检查 + 一轮等待后检测到新一代，停止激活并返回 false）；其余三个
  open_only 测试同步加入取消回调。
- 验证：`cargo fmt --all -- --check` passed；`cargo test --workspace` passed
  （sayall-windows 551 passed / 17 ignored，sayall-windows-app 218 passed /
  2 ignored，0 failed）。
- 新本地测试包：
  `target/release/bundle/nsis/无线麦 SayAll_1.9.0_x64-setup.exe`
  （SHA-256 `35067F130C7A27198D41575902DF7043280C75007D7596733952A03C77D6FF39`）；
  前端嵌入 18/18；静默安装 exit 0；安装版 exe 与 staging exe 仅差 3 字节
  （NSIS bundle 标记，预期），含 `target_result=superseded` /
  `launch_entry_failed` 新日志串；已重启安装版并连上遥控器。
- 日志顺序变化：`map_launch result=ok` 现在可能早于 `open_app_profile_foreground
  phase=completed`，因为等待已异步化。
- 待用户复测：冷启动首按仍到前台；快速连按两个打开键时，旧任务不应把窗口
  从新前台抢走；关掉窗口后不重开。真实遥控器端到端行为仍为 `deferred`。
- 隐私检查：未新增路径、标题、设备身份或语音内容到日志；线程名与日志 token
  均为产品固定串。

