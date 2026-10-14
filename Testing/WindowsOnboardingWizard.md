# Windows 首次设置向导（Onboarding）真机验收手册

## 适用范围

- 仓库：`GetSayAll/remote-mic-app-windows`，分支 `feat/onboarding`
- 硬件：小米蓝牙语音遥控器 2（RC001）与 2 Pro（RC003）**分别执行**；单型号结果不能代替另一种
- 前置依赖：VB-CABLE、三种输入工具（豆包输入法 / 微信输入法 / Vokie）各一个可登录账号
- 相关记录：探针与实现决策见 `docs/investigations/2026-10-04-windows-onboarding-feasibility.md`
- 自动化结果（vitest / Rust 测试 / CI）不替代本手册，任何真机步骤未执行一律记为 deferred

## 测试前准备

1. 记录提交 SHA、测试包路径与版本号、Windows 版本、VB-CABLE 版本。
2. 安装本地测试包（保留既有用户数据）。本构建已打开向导开关；老机器走「设置 → 首次设置 → 重新运行向导」进入。
3. 打开「权限」页确认日志路径，清空旧日志后开始。
4. 每个工具测试前按向导第⑤步的核对卡逐项确认：工具麦克风 = CABLE Output；工具内语音键与 SayAll 一致；Vokie 保持运行；豆包需开启「支持更多输入工具」。核对卡是编号提示行（**没有勾选框**）——放行只看真实文字上屏。

## 界面结构（2026-10-05 改版，参考 Mac App）

- 顶部仅一行面包屑「准备 › 设置 › 试一下」+ 一条细进度条；页内不再重复产品名。
- 布局自上而下：标题固定在内容列左上角**同一位置**（没有上一步的步骤保留同高占位），
  切换步骤/内容变化时页面不抖动；内容过长时在内容列内滚动。
- 内容列左上角是「‹ 返回」（chevron 图标 + 文字，与正文左对齐）；**没有整幅页脚横幅**。
- 底部动作行在内容列内部：左下「复制诊断信息」（无下划线，含复制反馈），右下主按钮
  （`继续` / `开始使用`，门禁提示语在按钮上方、右对齐）；右侧栏通到窗口底部。
- 字号：标题 27px、正文 14–15.5px、卡片标题 16px。第①步含「首次使用：遥控器怎么连电脑」
  卡片（1–5 条：准备 / 打开蓝牙设置与添加设备 / 长按 TV 配对 / 主页+菜单 重置 / 选中设备；
  第 2 条下方有一个「打开蓝牙设置」按钮行，2026-10-13 现场指定位置）。
- 第⑤步通过提示为绿色成功卡 + 对勾；最后一步右栏显示应用 logo + 对勾徽标。
- 右栏 = 插图 + 检查卡（按步骤换标题：连接检查 / 音频检查 / 工具检查 / 实时检查 / 按键检查）；
  窗口过矮（高度 ≤ 660）或过窄（宽度 ≤ 1024）时检查卡折到内容下方。
- 按键检测步骤（②遥控器 / ⑥普通按键）期间会**暂挂用户已配置的自定义按键**：只收集
  边沿，不触发任何映射动作；离开这两步立即恢复（日志 `mapping_suspend` / `mapping_resume`）。

## 诊断信息（2026-10-05 新增）

1. 任意步骤点左下「复制诊断信息」→ 应显示「已复制，可直接粘贴发给开发者」；粘贴到记事本检查内容。
   2026-10-13 现场：复制成功后**只留下方那条绿色提示**——按钮文字不再变成「已复制到剪贴板」。
2. 文本为**英文**（`SayAll diagnostics` 开头），必须包含：App version、Build（40 位源码修订）、
   Build channel、**Windows version（major.minor.build）**、Architecture、`Wizard step: <token> (n/7)`、
   Gate、Blocked，以及运行行：`Remote`（含 `error=` 与 **`button_observed=`**——②步是否已收到
   普通按键）、**`Raw input`**（phase / matched / raw_events / semantic_edges / stale_events）、
   `Audio`（含 `error=`）、`Input tool`（含 **`capture=`**——「全按键支持」的**真实捕获状态**
   off / starting / active / inactive / unknown；与 `full_key_support=`（开关设置）并列——
   只有 `active` 才算生效，`inactive`（过宽限无所有权）与可能长期停留的 `starting`（助手/桥
   未就绪）都是没生效，2026-10-14；浏览器预览与未启用该机制的环境显示 `capture=off`，属正常）、
   `Vokie`、
   **`Voice test`**（attempt 阶段与终态码，
   无终态写 `none`）、`Buttons observed`、`Mapping`。
   现场形状自证（2026-10-10）：②步"按键已收到但应用未连接"应表现为
   `Remote: phase=idle … button_observed=yes` + 门禁 `remote.not_connected`。
   复制时会对音频端点、**输入工具与按住说话快捷键**做一次只读刷新（2026-10-13 起）——
   已配置豆包的机器在任何步骤复制都应显示 `Input tool: doubao hotkey=…`，不是 `unset`；
   新装机器上快捷键按生效值显示（默认 左 Ctrl + 左 Win）。
   同口径（2026-10-14 起）：**「全按键支持」的开关状态与真实捕获状态（`capture=`）也做只读刷新**
   ——①–③步复制不再把开着开关的机器报成 `full_key_support=off`；捕获状态读不到、或出现未知
   token 时写 `capture=unknown`（读取失败各留一条 `read_capture_switch` / `read_capture_state`
   失败日志）。**本条待真机复核**（复制真实块对照 `capture=active|inactive|starting`）。
   同日起多一段 **`IME:`**（当前输入法是不是所选工具 / 系统里可用输入法数量 / 工具 token）——
   ③诊断"语音没出字是不是因为没切输入法"时先看这一行：
   `IME: detected=yes match=no installed_count=3 expected_tool=doubao`（`detected=no` = 读不到，
   界面不据此报警；复制块**保持全英文**，输入法显示名不进块，需要名字时看应用日志的
   `ime_status action=…` 行）。
3. 隐私检查：不得出现蓝牙地址、HID 路径、音频端点 id / 名称、文件路径、用户名或任何输入文字。
4. 日志核对：`frontend event=onboarding phase=action reason=copy_diagnostics` 成对出现
   （begin `result=unknown` → end `result=passed detail=chars_N`）；剪贴板不可用时 end `result=failed`。
5. 启动日志核对：`app_lifecycle event=process_start ... windows_version=<major.minor.build> windows_build=<build>`
   —— 不再是 `unknown`。

失败判定：复制提示缺失；文本缺 Windows 版本 / Build；出现设备身份或路径；日志缺 copy_diagnostics 对。

## 用例一：进入向导与中断继续

1. 全新状态（或删除向导状态文件）启动：应直接进入向导，停在第①步。
2. 老用户状态启动：不应进入向导；走设置页重跑入口 → 应进入向导并停在第①步。
3. 走到第④步后直接退出应用，重新启动：应恢复到第④步，不回到第①步。
4. 在向导里点内容列左上角的「← 返回」：回到上一步；不丢进度、不自动前进。

预期：进入或跳过向导只由向导状态决定；重跑入口只重置向导进度，不清除设备 / 映射 / 音频设置。

失败判定：老用户被强制重走向导；或应进入时停在主界面；退出再进丢步骤；重跑入口清空任何既有设置。


### 重启后落在⑤步的两种情形（2026-10-13 现场）

- **已选过工具**（磁盘上有 `voice_input_tool`）：⑤步核对卡必须写出**具体工具名**
  （如「输入法切到“微信输入法”。」），不得出现「你选的输入工具」这类回退文案；
  日志 `read_tool_state step=voice_test`。
- **从未选过工具**：应**自动退回④步**（标题「选择你要用的输入工具」），
  日志 `tool_missing_fallback detail=to_tool_step`；不允许在缺前置状态的⑤步上做验证。

## 用例二：步骤①–③（欢迎 / 遥控器 / 语音设备）

1. 第①步应给出完整首次配对流程（2026-10-13 第五轮现场定稿文案）：
   ① 准备好：小米蓝牙语音遥控器 2 或 2 Pro（目前只支持这两种型号），并确保它有电 →
   ② 在电脑上打开蓝牙设置（可直接点击下方按钮），打开电脑的蓝牙开关后点击「添加设备」然后选择
   「蓝牙」选项（2026-10-13 现场改文案：按钮名加中文引号）
   （**第 2 条下方有一个「打开蓝牙设置」按钮行**，点击只唤起系统「蓝牙和其他设备」页，
   不写任何"已打开…"提示；按钮行与上下两条留出间距）→ ③ 长按「TV」键约 3 秒，直到指示灯
   （小米蓝牙语音遥控器 2 Pro 底部）开始闪烁 → ④ 同时长按「主页」+「菜单」直到指示灯快速闪烁
   （重置蓝牙并重新进入配对模式）→ ⑤ 选中「小米蓝牙语音遥控器」，等待配对完成，列表里找不到
   设备时先点开「显示所有设备」；结尾一句「配对失败，或遥控器以前配过别的设备：回到第 4 步
   重置蓝牙后重新配对」。
   **布局硬约束**：1100×688（应用最小窗口的 WebView 实测尺寸）下第①步不得超屏——「继续」与
   「复制诊断信息」必须直接可见，不允许为了塞内容把按钮挤出屏幕（2026-10-13 实测：五项 + 结尾句
   会超屏 97px、「继续」按钮出屏；合并准备项后内容 486px、按钮可见，测量脚本记录见该轮会话）。
2. 第①步点「打开蓝牙设置」（步骤 2 下方那个）：系统「蓝牙和其他设备」页应真实弹出，页面
   **不出现**「已打开系统蓝牙设置…」一类提示。第②步的失败修复卡（蓝牙不可用 / 还没配对时
   才出现）里另有一个「打开蓝牙设置」入口，那里仍写过程消息。
3. 进入②步的自动动作（2026-10-13 现场扩了一条）：**进入②步先扫一次，把已配对设备直接列在
   下面**（日志 `detail=entry` / `entry_found_N`）——应用已连接、或启动时恢复了会话时也要列，
   不再要用户自己点「扫描已配对设备」。随后按原条件自动连接：恰好只有 1 个已配对的小米遥控器
   **且应用还没连上**（未连接、输入通道在位）时才自动连（`detail=auto`）；候选不唯一 / 一个都没有 /
   已连接 / 已手动断开时只列不连。停在②步期间在系统里新配对遥控器（输入通道变为在位）后自动连接。
   候选不唯一时用手动流程：扫描 → 选中设备「连接」（日志 `detail=manual`）。
4. 扫描与设备行（2026-10-13 现场）：第②步只保留「扫描已配对设备」并**保持常显**，且与下方
   内容留出间距；扫描进行中只给状态行标题「正在寻找小米遥控器…」，**不再写**「保持遥控器在旁边，
   已配对的设备会出现在下方」。「打开蓝牙设置」入口已移到第①步（见第 2 项）。
   扫描结果为空时，在按钮下方给出「当前没有已配对设备，请先按照上一步完成设备配对。」。
   设备行右侧的「连接」做成按钮观感（药丸形），点击后依次显示「正在连接…」→ 连接成功后变
   不可点的「已连接」状态（同一时刻只有一台显示已连接）；**连接后显示的名称用系统名称
   （能分清 2 / 2 Pro），不再依赖型号标签**（型号解析不出时那是占位文案）。
   **已连接那一行与下方「… · 已连接」显示具体型号**（2026-10-13 现场：Windows 可能给两支遥控器
   完全相同的名字，型号是唯一能分清 2 / 2 Pro 的字段；型号未知才退回系统名字）。
   **按到另一支遥控器时给出解释**（2026-10-13 现场）：等待本步普通按键期间，Raw Input 的
   `stale_remote_event_count` 在本步内增长（= 另一支在发按键）⇒ 就地提示
   「刚收到的是另一支遥控器的按键…」（未连接时提示先点一行「连接」）；整步只提示一次，
   日志 `other_remote_button`。**换遥控器（2026-10-13 现场）**：已连着一支时点另一支的「连接」→
   应用先**显式断开**之前那支（日志 `ble_switch action=disconnect_previous terminal_result=passed`；
   界面上旧行立刻回到「连接」、连接信息回未连接态），再连新的；点同一支只是普通（重）连接。
   同一支遥控器换过 HID 实例时，同地址的其他实例路径也接受（日志
   `raw_input target_path_accept`）。**归属判据（2026-10-13 现场修正）**：已连接时 Raw Input
   **只认当前那一支**——绑定路径与同地址的其他实例都算；绑定若停在另一支上，那一支一律按
   "另一支"处理并计数。按**另一支**的按键**不得**让本步通过（应出现「另一支遥控器」提示）。
   **换连后必须重按（2026-10-13 现场）**：连甲支并按出绿灯后，改连乙支 ⇒ 本行回到
   「等待控制按键」、页脚门禁重新阻断，必须按一下**乙支**才放行（日志
   `button_observation_reset detail=remote_switched`）；切换过程中快照瞬时清空（先断开甲支）
   不算换机，不重置。
   边界：Windows 自己在 HID/HOGP 层维持的蓝牙链路不由应用控制，
   应用只能释放自己的会话与按键归因目标。
5. 扫描并连接遥控器；按一下普通按键：门禁从「按一下普通按键」变为可继续；按语音键不应误判为普通按键。
   此期间按已配置映射的键（如 OK → Enter）**不应触发**用户的自定义动作（检测步骤暂挂映射）。
6. 两态文案（2026-10-10 新增，现场反馈）：**按键已收到但应用还没连接**时，没扫到设备写
   「按键已收到。还差一步：点“扫描已配对设备”选中遥控器连接。」，列表已列出时写
   「按键已收到。还差一步：点遥控器一行右侧的“连接”。」；两种都不得写「这一项可以继续了」。
   2026-10-13 现场（第四轮）：连接就绪、按键也收到时**该行不再写任何说明句**——整行已是就绪
   状态、页脚门禁也放行，原先那句「这一项可以继续了。」属重复，已删除；此时行里只剩标题。
   同轮（2026-10-13）：已连接、连接中（含自动连接）都不显示任何连接说明卡——点「连接」后
   不应闪现旧卡；连接成功不写「正在确认语音功能…」进度消息（引导由按键行承担），
   连接就绪时残留的过程消息随即消失。
7. 门禁提示行（2026-10-13）：底部「先连接遥控器，再继续。」一类提示句应始终保持**一行**不换行，
   提示出现/消失时上方按钮行不上下跳位。
8. 第③步：检测到带「推荐」标记的 CABLE 设备并自动选中；没有时给出安装与重启说明。

## 用例三（核心）：第⑤步「按住说话验证」，三种工具各跑一遍

对 豆包 / 微信 / Vokie 分别执行：

1. 进入第⑤步：输入框应自动获得焦点；核对卡是**编号提示行**、没有可勾选的复选框。
   2026-10-14 外观核对：输入框聚焦时**只保留 1.5px 细边框、不显示焦点环**——
   重启后第一次进入（未点过任何位置）与从上一页进来，两种进入方式外观一致。
   2026-10-13 现场定稿：微信 / 豆包 = **三项**，第一项「输入法切到“<工具名>”。」；
   Vokie / 其他工具 = **两项**（不依赖切换输入法，不列该项）。
2. 正常路径：按住遥控器语音键说一整句 → 松开。
   预期：实时状态「正在接收你说的话…」→「正在等文字出现…」；输入框出现文字；结果行显示「成功：文字已经出现在输入框里」；「继续」变为可用。
3. **输入法兜底（2026-10-13 现场）**：
   - 点一下输入框、按一下语音键，各应触发一次输入法检查（日志 `ime_check`，
     `detail=focus` / `detail=voice_key`；结果 passed/failed/unknown）。
   - 当前会话输入法**不是**所选工具（例如系统停在微软拼音）→ 出现说明卡：
     「输入法还没切到“<工具名>”：现在系统里选的是“<当前输入法>”…」；切好后再点输入框，
     提示消失（`ime_check result=passed`）。
   - 读不到输入法状态（老系统/TSF 不可用）→ **不提示**（fail-open），日志 `result=unknown`。
   - **实验开关（临时，见文末"向导内直接切输入法实验"）**：开时日志出现
     `ime_inline_switch action=read enabled=true`，此时点输入框会尝试在**应用窗口前台**
     也直接切换（`ime_activation scope=wizard_inline`）。
4. 阴性对照：不碰遥控器，直接用键盘在输入框打字。
   预期：判定失败，文案「检测到键盘输入。这一步请只用遥控器语音键，不要用键盘打字。」；「重新测试」清空输入框并重新开始，再跑一次正常路径应通过。
   判定口径（2026-10-13 收窄，现场误报修复）：只有「可能产生文字的按键」且「按下时没有 Ctrl/Alt/Win 按住」才计入——
   纯修饰键、功能键 / 方向键 / 截图键（如裸 PrintScreen）、以及 Ctrl/Alt/Win 组合热键（如截图 Win+Shift+S）都不算；
   打大写字母（Shift+字母）与裸字母照常计入，阴性对照不放松。反例复核：⑤步停留期间按一次截图热键，
   再完成一次正常语音会话 → 应通过（日志 `input_observation … action=end count=0 keys=none`）。
5. 焦点用例：先点输入框以外的地方（失焦）再按住语音键 →
   预期：失败文案「输入框没有聚焦。先点一下输入框…」；测试过程中输入框**真的**失去焦点（点到别处）→
   预期：失败文案「测试过程中输入框失去了焦点…」。
   2026-10-13 现场修正：别的窗口盖上来（工具语音条、截图遮罩、系统弹窗）产生的**窗口级 blur 不算失焦**——
   输入框仍持有焦点、文字照样落进去，必须判通过（此前会把已上屏的会话误判成「输入框没有聚焦」）。
6. 快速点按（不足 0.5 秒）：应给出可见失败并可重试，不出现卡死或空白状态。
8. **键盘右 Alt 路径（2026-10-04 钩子迁移修复的核心验收，见
   Bugs/2026-10-04-ll-hooks-break-in-app-ime-voice.md）**：应用窗口前台、焦点在输入框时，
   按住键盘**右 Alt**（即所选工具的按住说话快捷键）→ 语音条应出现；松开 → 语音条消失。
   此前"前台根窗口进程 == LL 钩子所在进程"时该路径必失败；修复后两个 LL 钩子均在
   按键宿主进程（`sayall-windows-app.exe --sayall-key-host`），宿主永不持有前台窗口。
   日志核对：`key_host action=hook_report detail=HOOK kind=f5 installed=1` 与
   `detail=HOOK kind=gate installed=1` 各一条；若缺失，钩子未安装、判定降级。
7. **迟到文字（2026-10-13 修复，现场报告「文字出现但无法继续」）**：文字晚于「松开后
   3 秒」窗口才出现（冷启动首用时最容易遇到）时，**不再判死**——文字出现在输入框后，
   结果卡自动转为「成功：文字已经出现在输入框里」，「继续」可用；日志追加一条
   `phase=voice_attempt result=passed reason=late_transcript detail=after_<ms>`（毫秒 =
   迟到延迟），诊断块显示 `terminal=late_transcript late=yes`。
   阴性对照不变：失败后改用键盘打字 → 保持失败，日志 `reason=late_transcript_manual`。
   文字写进输入框但没有派发输入事件（少数输入法）也按现值判定，日志 `reason=transcript_value_readback`。
9. **门控回归（切片 3 门控迁出后）**：遥控器已映射按键（如方向键 / OK）在映射启用时
   仍触发配置动作且不残留原生按键；按住说话快捷键注入成对（按下 DOWN / 释放 UP）；
   断连或退出应用后无粘键（同一按键连按无卡死）。验收时 helper 开 / 关各跑一遍：
   门控迁移与「全按键支持」helper 相互独立，helper 关闭不得影响上述行为。
10. 日志核对（每次 attempt 必须成对）：
   - `frontend event=onboarding phase=voice_attempt result=unknown reason=armed …`（一次 attempt 一条）；
   - 终态一条：`result=passed` 或 `result=failed reason=voice.*`，`detail=d…_s…_q…_drain0|1` 为采样 / 投递 / 队列计数；
   - 迟到接受 / 迟到否决 / 现值读回各一条（`late_transcript` / `late_transcript_manual` / `transcript_value_readback`）；
   - `input_observation … action=begin/end` 成对出现；`action=end … count=0`（正常路径，判定口径见用例三.4）；
     `action=end` 行含 `keys=`（窗口内计入的按键列表，最多 12 个）——出现非 0 计数时一眼看清是哪些键被计入。
   - 日志不得包含任何输入文字内容。

## 分步日志（卡住定位速查，2026-10-05 起）

向导每一步都落 `frontend event=onboarding` 结构化日志；**用户报"卡在某一步"时，
按时间取最后几条日志即可定位环节**。约定：

- `phase=action` 成对出现：begin 时 `result=unknown`，end 时 `result=passed|failed`
  且带 `elapsed_ms`；**有 begin 没有 end = 卡在该外部调用**。
  各步的 reason 速查：
  - ② 遥控器：`button_observation`（按键边沿订阅）、`scan_requested`（扫描；begin 行
    `detail=auto|manual`，end 行 `detail=auto_found_N` / `manual_found_N`）、
    `connect_requested`（`detail=auto|manual`；auto = 候选唯一时的自动连接，
    2026-10-10 起）、`open_bluetooth_settings`、`mapping_suspend` / `mapping_resume`
    （检测期间暂挂自定义按键）；**停在②步且没有 `scan_requested` = 自动连接未触发**
    （输入通道不在位 / 应用已连接 / 用户已手动断开）；
  - ③ 语音设备：`refresh_endpoints`（`detail=auto|manual_total_N_rec_M`）、`select_endpoint`
    （`detail=auto|manual|fallback`）、`open_download_page`；另有 `phase=audio_route`
    （`reason=missing|not_selected|selected|ready`，状态去重）；
  - ④ 输入工具：`read_tool_state`、`stage_binding`（`detail=<工具>`）、`save_other_keys`、
    `vokie_detect`（`detail=installed_0|1_running_0|1`）、`open_vokie_site`、`vokie_launch`；
  - ⑤ 按住说话：`armed` / 终态 / `session_stopped` / `observation_begin_failed` /
    `transcript_value_readback`（现值读回）/ `late_transcript` / `late_transcript_manual`
    等（见上一条核对）；
  - ⑥ 普通按键：`button_observation`、`mapping_suspend` / `mapping_resume`；
  - ⑦ 完成：`complete_refresh`（失败时 `detail=tool|vokie|capture|audio` 指出来源）。
- `phase=heartbeat` 每 30 秒一条（`reason=alive`）：携带当前 `step`、门禁 `code` 与
  步骤内等待摘要（如 `obs_0_n0`、`streaming`）。**长时间停留某一步时最后一跳就是现场**；
  这是"状态未变化不重复刷"的刻意例外。
- 脱敏红线：所有 `step/code/detail` 均为稳定 token 或计数，不含设备 id/名称、
  音频端点 id/名称、文件路径、错误原文与任何用户输入内容（自动化用例对
  遥控器/端点身份字段做了"绝不出现"断言）。

失败判定：正常路径判定失败或看不到文字；键盘打字仍判通过（含失败后用键盘打字）；焦点类失败文案缺失；快速点按导致卡死、重复 attempt、或出现两个终态；日志缺 attempt 配对或含文字内容。

## 用例四：步骤⑥⑦（普通按键体验 / 完成）

1. 第⑥步：按除语音键外的 3 个不同普通按键（如 主页 / OK / 方向键）→ 已按下的键在芯片格里点亮、进度点走到 3/3 后可继续（同一键重复按不计）；期间遥控器普通按键不应执行用户已配置的映射动作（临时暂挂）；按语音键出现「这是语音键…」纠偏、且不计入。
2. 第⑦步：清单全绿 →「开始使用」；提交后落回主界面「连接」页。
3. 完成后再从设置页重跑入口进入：应回到第①步（不影响既有设置）。
4. 断开遥控器后打开第⑦步（若可重现）：清单对应项转红，给「去修复」跳回对应步骤。

预期：第⑥步的暂挂只影响向导会话，离开后映射恢复；第⑦步提交的是第④步 staged 的工具与快捷键。

失败判定：暂挂期间仍执行映射或退出后映射不恢复；第⑦步在清单有红项时仍可完成；提交后工具 / 快捷键与第④步所选不一致。

## 用例五：两种型号分别记录

- RC001 与 RC003 各自完整执行用例一 ~ 四；分别写明每步结果与失败码。
- 任一型号任一用例失败即整体判失败，不用另一型号的结果顶替。

## 输入法：只检查 + 文字提醒（2026-10-13 定稿，不做自动切换）

⑤步的输入框就在应用自己的窗口里。早期版本在点输入框 / 按语音键时会**自动**把输入法切到
所选工具；2026-10-13 Andy 明确决定**去掉自动切换**，改为只读检查 + 文字提醒：

- 点输入框、按语音键各触发一次**只读**查询（不切换任何输入法），日志
  `ime_check detail=focus|voice_key`（passed / failed / unknown）。
- 当前会话输入法不是所选工具时，页面给一张说明卡：
  「输入法还没切到“<工具名>”：现在系统里选的是“<当前输入法>”。请在桌面右下角把输入法切成
  “<工具名>”，或者回到上一步改选你的工具。」切好后（再点输入框）提示消失。
- 读不到输入法状态（老系统 / TSF 不可用）→ **不提示**（fail-open），日志 `result=unknown`。
- 生产路径（按下语音键、工具选择的立即对齐）与主页面工具卡的行为**不在本次范围内**，保持原样。

验收：把系统输入法故意切到别的（如微软拼音）→ 点输入框应出现说明卡、**输入法不得被自动改动**；
手动切回所选工具后再点输入框，提示消失。日志侧看 `ime_check` 与 `ime_status action=queried`，
不应再出现任何 `ime_activation scope=wizard_inline`。

## 证据与日志收集

- 每轮结束导出 `sayall-diagnostic.log` 相关时间段；报告包含提交、构建路径、Windows 版本、型号、工具、步骤、失败码与日志片段。
- 脱敏边界：不复制语音内容或识别文字；不记录真实设备地址、HID 路径、端点 ID / 名称。

## 模拟硬件信号（无真机跑向导；2026-10-05 新增）

没有物理遥控器时用模拟信号跑完整向导，用于日常回归与问题复现；**不替代**上面的真机用例。

装置：`GetSayAll/hardware-simulation`（Windows 实现）把 Profile / Scenario 导出成"应用信号脚本"，应用在 `runtime-simulation` 构建里按脚本时间线回放，事件走**生产**解析链路（HID 报告 → `decode_report_usages` + `ButtonStateMerger`；ATVV → `AtvvVoicePipeline`）：

```powershell
# 1) 生成信号脚本（任一 scenario 均可）
hardware-sim export-app-script <profile.json> <scenario.json> --out script.json
# 2) 构建仿真可执行文件（一次性）
cargo build -p sayall-windows-app --features runtime-simulation     # 或 --release
# 3) 起前端 dev server（仿真二进制的 devUrl 指向 2430）
pnpm dev
# 4) 回放：隔离状态目录 + 指定脚本（应用会保持运行，便于界面走查）
powershell -NoProfile -ExecutionPolicy Bypass -File Testing\run-hardware-signal-script.ps1 `
    -ScriptPath Testing\hardware-scripts\rc003-onboarding-walkthrough.json -StopExisting
```

- 状态隔离：`SAYALL_RUNTIME_SIMULATION_STATE_DIR` 下生成独立的 `settings.json` / `onboarding.json`，**不动**用户的真实向导状态；向导每次从第①步开始。
- 覆盖：② 的按键门禁、③ 的端点推荐与自动选择、④ 的工具选择、⑤ 的会话与解码采样（`voice_attempt` 终端码）、⑥ 的 3 个不同按键、⑦ 的提交与收尾；日志含 `hardware_script action=apply ... terminal_result=` 逐条记录。
- 转写文字：模拟轨道没有输入法注入，第⑤步的转写由驱动写入输入框并派发 `input` 事件（真机仍必须真人说话）。
- 已跑通记录（2026-10-05，RC003 scenario）：①②③④⑥⑦ 全绿，⑤ `voice_attempt result=passed detail=d240_s240_q0_drain1`，收尾 `wizard_finished elapsed_ms=71920`；截图与日志摘录见 `artifacts/local-test/2026-10-05-onboarding-simulation/`。
- 边界：模拟 ≠ 真机——配对、射频、权限、驱动、真实音质与安装包行为仍按本手册真机执行；两种型号仍要分别做真机用例一~四。

## 验证边界

- 本手册只覆盖向导；语音质量、按键映射日常行为、安装升级矩阵仍按各自手册。
- 自动化（`pnpm test` / `cargo test --workspace` / runtime-simulation 检查）只能证明代码路径，不能替代本手册的真机步骤。
