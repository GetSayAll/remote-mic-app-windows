# Alt+Tab 切到/离开无线麦后“卡死”（上一个应用是无线麦时连按两下）

- 发现日期：2026-11-19
- 状态：已修复（本地测试包已安装并完成置顶/Alt+Tab 复验；等待报障环境与 Windows 10/真机确认）
- 影响范围：Windows 10/11；用户反馈版本未知（1.8.3 旧版或 1.9.x 均需区分）；
  RC001/RC003 与机型无关；按键绑定为「切换应用」或一次性 `Alt+Tab` 时均需核对。
- 功能点：按键映射 → 切换应用（`app_switcher`）/ 自定义快捷键 `Alt+Tab` →
  `SendInput` 注入；窗口前台/置顶与系统切换器。
- 现象：用户按下绑定为 `Alt+Tab`（切回上一个程序）的按键两次后，如果上一个
  应用是无线麦，表现为“卡死”。其他上一个应用时未出现。
- 复现条件（用户描述）：
  1. 上一个应用为无线麦；
  2. 连续触发绑定为 `Alt+Tab` / 「切换应用」的按键两次；
  3. 随后无线麦无响应、无法继续切换或只能结束进程（具体表现待确认）。
- 正常预期：每次触发只完成一次切换；无线麦保持可响应；Alt 不残留；
  切换器最多按设计在 15 秒无操作后自动退出。

## 本地证据（2026-11-19）

运行实例：`ver=1.9.3`，`source_revision=2a0ee83`（本分支本地测试构建），
Windows 11 26200。该实例的诊断日志通过 `SAYALL_GATT_LOG` 指向
`%LOCALAPPDATA%\SayAll\login-diagnostic-2.log`（测试覆盖路径）。

### 1. 用户本机两轮「切换应用」会话（10:26:09Z–10:26:21Z）

- 10:26:09.874 Menu 按下 → `map_fire button=Menu action=app_switcher phase=start operation=1`；
  10:26:09.980 `switcher_seen=true`。
- 10:26:11/11.5/12.0 三次 Menu 按下 → 每次 `phase=press ... action=Next`；
  10:26:14.466 Ok 按下 → `AltUp`，`phase=end reason=confirmed alt_down_after=false
  duration_ms=4592 tab_count=4 confirm_count=1`。
- 10:26:14.892 立即开始 operation=2；左/右/Menu 导航后 10:26:20.952 Ok →
  `AltUp`，`phase=end reason=confirmed alt_down_after=false duration_ms=6058
  tab_count=2 previous_count=2 confirm_count=1`。
- 之后 10:26:22/10:26:32 的 10 秒心跳、10:31/10:33 的音量键动作均正常，
  主进程没有出现日志中断或 Application Hang 事件。

结论：这一现场里无线麦**没有**死锁；两轮会话都正常收尾并释放 Alt。

### 2. 受控重放（10:43Z–10:44Z）

用 RC003 桥接协议注入真实的 Menu/Ok 边沿（`E <ts> 65` / `E <ts> 28`，
助手在每次连接后自动重连），按以下两条路径重放：

| 路径 | 结果 |
| --- | --- |
| 从 Notepad 触发，上一项为无线麦：一次 Menu + Ok 确认 | 无线麦成为前台；`alt_down_after=false`；`Responding=True`；`IsHungAppWindow=False`；`WM_NULL` 0 ms |
| 无线麦在前台，立即再次触发：一次 Menu + Ok 确认 | 切回 Notepad；`alt_down_after=false`；响应正常 |
| 同一次会话连按两次 Menu 再 Ok（`tab_count=2`） | 切到下一项（非无线麦）；`alt_down_after=false`；响应正常 |
| 15 秒不操作（会话 3） | `reason=timeout escape=true alt_down_after=false`；左 Alt 读回为 0；响应正常 |
| 一次性 `Alt+Tab` 单批注入（Alt↓ Tab↓ Tab↑ Alt↑），连续 4 次 | 前后台在无线麦/Notepad 间往返；每轮 `left_alt_down=false`；无未响应 |

`SendInput` 单批注入与按键引擎实际使用的 `send_key_tap_with` 同形；`Alt+Tab`
会话使用独立的 `AltDown` / `Tab` / `AltUp`，两者均未复现挂起。

### 3. 置顶窗口对照（可复原实验）

- 将无线麦主窗口设为 `WS_EX_TOPMOST`（exstyle 从 `0x00040110` 变为
  `0x00040118`），打开系统切换器后枚举可见顶层窗口：
  `XamlExplorerHostIslandWindow`（切换器）在 Z 序第 0 位，无线麦在第 4 位；
  切换器本身可见，没有被置顶窗口遮住。
- 但这验证了另一条已知边界：无线麦退出置顶前，非 topmost 的目标窗口在
  Z 序上会被无线麦压住。焦点已切给目标、用户却仍看到无线麦，表现就像
  “切换无效/卡死”。
- 实验后已还原为非置顶（`0x00040110`），未留下置顶位。

## 根因判断

**当前构建（1.9.3 / source 2a0ee83）未复现主进程死锁。** 现有证据不能把本次
反馈归因到单一根因；按“上一个应用是无线麦”这一条件，最可能落在以下三类：

1. **旧版本已知死锁（版本边界，待报障版本确认）**
   - 1.8.3 及更早使用 tao 0.35.3：`Alt+Tab` / `Win+D` 注入引发焦点消息重入，
     `KEY_EVENT_BUILDERS` 自锁，主窗口未响应。记录见
     `Bugs/2026-11-05-tao-keyboard-reentrancy-deadlock.md`。
   - 1.9.0 起升级 Tauri 2.12.5 / tao 0.37.2（当前 `Cargo.lock` 为 0.37.2），
     本地单批 `Alt+Tab` 连打 4 次不再挂起。
   - 若报障人在 1.8.3 或更早，升级到 1.9.3 后复测即可验证；不能拿旧版日志
     推断当前版本。

2. **主窗口置顶导致目标被压住（可解释“只有上一个应用是无线麦时”）**
   - 无线麦若带 `WS_EX_TOPMOST`，切到别的应用后焦点已变，但无线麦仍在 Z 序
     最高，用户看到界面没换，容易描述为“卡死”。
   - 现有修复只在“打开别的应用”（`activate_or_launch`）前撤顶；启动时、
     Alt+Tab 前后都没有兜底。若置顶位来自外部窗口工具或历史状态，就可能
     在本次反馈的路径上残留。
   - 需报障人确认：卡死时无线麦窗口是否仍在最上面、任务管理器是否显示
     “未响应”。

3. **「切换应用」会话的 15 秒设计被感知为卡死（非崩溃）**
   - 会话期间保持 Alt 并遮蔽 OK/返回/方向/TV；不按 OK/返回时最长 15 秒
     才自动取消。期间遥控器按键被吞，容易像“卡死”。
   - 本地 15 秒超时路径已确认会发 Esc、释放 Alt（`alt_down_after=false`）。
   - 若报障人看到切换器一直存在、约 15 秒后恢复，这属于产品交互边界，
     不是进程死锁；需要单独评估提示或超时策略。

## 修复

针对可解释“只有上一个应用是无线麦时才像卡死”的置顶分支，增加了结果层兜底；
不改变 Alt 注入、切换器会话或 15 秒超时语义：

- `src-tauri/src/lib.rs`：启动时在注册 `set_self_drop_topmost` 回调后主动调用
  `request_self_drop_topmost()`，清掉历史/外部工具遗留的 `WS_EX_TOPMOST`；
  日志 `window_topmost_guard reason=startup dropped=<bool>`。
- `src-tauri/src/lib.rs`：主窗口 `Focused(false)` 时再次调用同一回调——
  Alt+Tab / Win+D 切走时先把自身从置顶层撤下，避免焦点已切给目标、Z 序上
  仍压着目标；真正撤掉时日志
  `window_topmost_guard reason=focus_lost dropped=true terminal_result=passed`。
- `crates/sayall-windows/src/button_mapping.rs`：新增共用
  `drop_stale_topmost_for_app_switcher(source)`，在
  `ButtonAction::AppSwitcher` 与组合动作入口
  `EngineMessage::StartAppSwitcher` 的 `!was_active` 分支各调用一次。
  这是评审实测出的缺口：报告场景里无线麦在**后台**，`Focused(false)` 不会
  再触发；若置顶位是在后台被外部工具设置/恢复的，只靠失焦兜底不够。
  真正撤掉时日志
  `window_topmost_guard reason=app_switcher_start dropped=true terminal_result=passed source=button|macro`。
- `crates/sayall-windows/src/button_mapping.rs`：`ButtonAction::Shortcut`
  遇到一次性 `Alt+Tab`（`chord.is_alt_tab()`）时同样先撤顶。报告里的
  “按键设为 Alt+Tab”可能是旧的一次性绑定，不经过 `AppSwitcher` 分支；
  日志 `window_topmost_guard reason=alt_tab_shortcut dropped=true terminal_result=passed`。
- 复用既有 `app_launcher::drop_window_topmost`（读 Win32 样式 →
  `SetWindowPos(HWND_NOTOPMOST)` → 读回确认）与 `set_always_on_top(false)`
  同步 tao flag，不新增 Win32 写入路径。
- 新增源码契约测试
  `ui_hint_tests::stale_topmost_is_dropped_at_startup_and_on_main_focus_loss`，
  锁住“启动”和“主窗口失焦”两个入口；`button_mapping::tests::
  app_switcher_start_drops_stale_topmost_before_opening_switcher` 与
  `alt_tab_shortcut_drops_stale_topmost_before_injection` 锁住引擎侧的
  会话启动与一次性快捷键兜底，防止后续重构回退。

仍未改动：旧版本（≤1.8.3）的 tao 0.35.3 重入死锁不在本修复范围内，需升级；
「切换应用」15 秒无操作超时仍按产品设计保持。

## 验证

- `cargo fmt --all -- --check`：`passed`。
- 新增契约测试（修复前先红、修复后绿）：
  `cargo test -p sayall-windows-app --lib stale_topmost_is_dropped_at_startup_and_on_main_focus_loss`：
  `failed` → `passed`。
- 引擎侧兜底契约：
  `cargo test -p sayall-windows drops_stale_topmost`：`passed`（2 条：
  会话启动 + 一次性 Alt+Tab）。
- `cargo test -p sayall-windows-app --lib`：`passed`（222 passed / 2 ignored）。
- `cargo check -p sayall-windows-app --features runtime-simulation`：`passed`。
- 真实窗口验证（debug 构建，Windows 11 26200）：
  - 启动日志 `window_topmost_guard reason=startup dropped=false`；启动即带
    topmost 的正样本无法用新进程构造（Windows 不跨进程持久化该样式），
    该分支的正样本为 `deferred`；
  - 将主窗口设为 `WS_EX_TOPMOST`（exstyle `0x00040118`）后合成一次
    `Alt+Tab` 切走，主窗口失焦后 exstyle 读回 `0x00040110`，日志出现
    `window_topmost_guard reason=focus_lost dropped=true terminal_result=passed`；
  - 故障前行为是代码级推断（旧路径的 `Focused(false)` 没有撤顶调用），
    未单独用旧包复测同一操作；不把它写成实测结论。
- 评审补充实测（后台置顶缺口，修复前）：
  - 把无线麦置顶后最小化 → 聚焦 Notepad → `SW_SHOWNOACTIVATE` 恢复无线麦：
    `fg=Notepad`、无线麦可见、exstyle `0x00040118`；
  - 触发「切换应用」启动、再 OK 确认：exstyle 始终 `0x00040118`，
    证明 `Focused(false)` 不会在“后台置顶”场景触发。
- 本地测试包（2026-11-19，含按键/组合动作/一次性快捷键三条入口兜底）：
  - `target\release\bundle\nsis\无线麦 SayAll_1.9.3_x64-setup.exe`
    SHA-256 `569de5b571c4865899bad45d6ce68aab3d377ed2736185df8054904029c5004a`；
    `.sig` 为本机测试密钥（与 CI pubkey 不匹配属预期，**绝不可发布**）。
  - `scripts/verify-local-test-install.ps1 -ExpectRevision -ExpectAppMarker window_topmost_guard`：
    `passed`——helper/gadget/sayall-mcp 哈希对齐；app exe 差 3 字节（UNK→NSS）；
    内嵌修订号 `810079d1`（工作区修复尚未提交，故修订号是父提交；
    标记 `window_topmost_guard count=4` 证明四处守卫已进包）；安装版启动截图 passed。
  - 安装版实测（同一后台置顶场景，修复后，pid 23232）：
    exstyle `0x00040118` → 触发「切换应用」→ 会话启动即读回 `0x00040110`，
    日志出现 `window_topmost_guard reason=app_switcher_start dropped=true
    terminal_result=passed source=button`（11:31:29Z）；随后 OK 确认，
    `Responding=True`。
  - 安装版实测（前置场景）：启动日志
    `window_topmost_guard reason=startup dropped=false`；主窗口前置置顶后
    单批 `Alt+Tab` 切走，exstyle 读回 `0x00040110`，日志出现
    `window_topmost_guard reason=focus_lost dropped=true terminal_result=passed`、
    `Responding=True`、左 Alt 读回为 0。
  - 组合动作入口（`source=macro`）与一次性 `Alt+Tab` 快捷键的**真机绑定
    路径**：`deferred`——当前本机映射是「切换应用」动作，没有把同一个按键
    临时改成一次性快捷键，也没有运行含 Alt+Tab 的组合动作；两条路径已由
    源码契约测试和共享回调覆盖，需在最终还原映射后用真机再点一次。
- 报障人环境、真实 RC001/RC003 实体按键、Windows 10：
  `deferred`——缺报障版本、诊断日志与卡死时的窗口/进程状态。

## 下一步

1. 本地测试包已安装并复验（见上）；后续只需报障人确认版本与卡死表现，
   若仍在 1.8.3 或更早，先升级 1.9.3 复测。
2. 若报障人确认是「切换应用」15 秒会话被感知为卡死，另开交互改进项，
   不与本次置顶兜底混在一起。
3. 在 Windows 10 与 RC001/RC003 实体按键上按 `Testing/WindowsAppSwitcher.md`
   回归置顶场景；当前为 `deferred`。

## 隐私检查

本文只包含版本号、时间戳、布尔值、Z 序序号、稳定的日志 token 和产品路径；
不含设备身份、蓝牙地址、HID 路径、窗口标题、个人目录、语音内容或凭据。
