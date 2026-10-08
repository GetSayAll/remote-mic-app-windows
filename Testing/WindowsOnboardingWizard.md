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
- 字号：标题 27px、正文 14–15.5px、卡片标题 16px。第①步含「第一次使用：遥控器怎么连电脑」
  卡片（长按 TV 配对 / 主页+菜单 重置 / Windows 高级选项），与第②步的详细说明一致。
- 第⑤步通过提示为绿色成功卡 + 对勾；最后一步右栏显示应用 logo + 对勾徽标。
- 右栏 = 插图 + 检查卡（按步骤换标题：连接检查 / 音频检查 / 工具检查 / 实时检查 / 按键检查）；
  窗口过矮（高度 ≤ 660）或过窄（宽度 ≤ 1024）时检查卡折到内容下方。
- 按键检测步骤（②遥控器 / ⑥普通按键）期间会**暂挂用户已配置的自定义按键**：只收集
  边沿，不触发任何映射动作；离开这两步立即恢复（日志 `mapping_suspend` / `mapping_resume`）。

## 诊断信息（2026-10-05 新增）

1. 任意步骤点左下「复制诊断信息」→ 应显示「已复制，可直接粘贴发给开发者」；粘贴到记事本检查内容。
2. 文本为**英文**（`SayAll diagnostics` 开头），必须包含：App version、Build（40 位源码修订）、
   Build channel、**Windows version（major.minor.build）**、Architecture、`Wizard step: <token> (n/7)`、
   Gate、Blocked、Remote / Audio / Input tool / Vokie / Buttons observed / Mapping。
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

## 用例二：步骤①–③（欢迎 / 遥控器 / 语音设备）

1. 第②步点「打开蓝牙设置」：系统「蓝牙和其他设备」页应真实弹出。
2. 第②步的「首次连接遥控器」卡应写清：长按「TV」约 3 秒进入配对；没有反应时同时长按「主页」+「菜单」
   重置蓝牙并重新进入配对；Windows 列表里找不到时先点开「高级选项」（或“更多蓝牙选项”）再刷新。
3. 扫描并连接遥控器；按一下普通按键：门禁从「按一下普通按键」变为可继续；按语音键不应误判为普通按键。
   此期间按已配置映射的键（如 OK → Enter）**不应触发**用户的自定义动作（检测步骤暂挂映射）。
4. 第③步：检测到带「推荐」标记的 CABLE 设备并自动选中；没有时给出安装与重启说明。

## 用例三（核心）：第⑤步「按住说话验证」，三种工具各跑一遍

对 豆包 / 微信 / Vokie 分别执行：

1. 进入第⑤步：输入框应自动获得焦点；核对卡（编号两行）显示本次工具与快捷键，没有可勾选的复选框。
2. 正常路径：按住遥控器语音键说一整句 → 松开。
   预期：实时状态「正在接收你说的话…」→「正在等文字出现…」；输入框出现文字；结果行显示「成功：文字已经出现在输入框里」；「继续」变为可用。
3. 阴性对照：不碰遥控器，直接用键盘在输入框打字。
   预期：判定失败，文案「检测到键盘输入。这一步请只用遥控器语音键，不要用键盘打字。」；「重新测试」清空输入框并重新开始，再跑一次正常路径应通过。
4. 焦点用例：先点输入框以外的地方（失焦）再按住语音键 →
   预期：失败文案「输入框没有聚焦。先点一下输入框…」；测试过程中让窗口失焦 →
   预期：失败文案「测试过程中输入框失去了焦点…」。
5. 快速点按（不足 0.5 秒）：应给出可见失败并可重试，不出现卡死或空白状态。
6. **键盘右 Alt 路径（2026-10-04 钩子迁移修复的核心验收，见
   Bugs/2026-10-04-ll-hooks-break-in-app-ime-voice.md）**：应用窗口前台、焦点在输入框时，
   按住键盘**右 Alt**（即所选工具的按住说话快捷键）→ 语音条应出现；松开 → 语音条消失。
   此前"前台根窗口进程 == LL 钩子所在进程"时该路径必失败；修复后两个 LL 钩子均在
   按键宿主进程（`sayall-windows-app.exe --sayall-key-host`），宿主永不持有前台窗口。
   日志核对：`key_host action=hook_report detail=HOOK kind=f5 installed=1` 与
   `detail=HOOK kind=gate installed=1` 各一条；若缺失，钩子未安装、判定降级。
7. **门控回归（切片 3 门控迁出后）**：遥控器已映射按键（如方向键 / OK）在映射启用时
   仍触发配置动作且不残留原生按键；按住说话快捷键注入成对（按下 DOWN / 释放 UP）；
   断连或退出应用后无粘键（同一按键连按无卡死）。验收时 helper 开 / 关各跑一遍：
   门控迁移与「全按键支持」helper 相互独立，helper 关闭不得影响上述行为。
8. 日志核对（每次 attempt 必须成对）：
   - `frontend event=onboarding phase=voice_attempt result=unknown reason=armed …`（一次 attempt 一条）；
   - 终态一条：`result=passed` 或 `result=failed reason=voice.*`，`detail=d…_s…_q…_drain0|1` 为采样 / 投递 / 队列计数；
   - `input_observation … action=begin/end` 成对出现；`action=end … count=0`（正常路径）。
   - 日志不得包含任何输入文字内容。

## 分步日志（卡住定位速查，2026-10-05 起）

向导每一步都落 `frontend event=onboarding` 结构化日志；**用户报"卡在某一步"时，
按时间取最后几条日志即可定位环节**。约定：

- `phase=action` 成对出现：begin 时 `result=unknown`，end 时 `result=passed|failed`
  且带 `elapsed_ms`；**有 begin 没有 end = 卡在该外部调用**。
  各步的 reason 速查：
  - ② 遥控器：`button_observation`（按键边沿订阅）、`scan_requested`（扫描，`detail=found_N`）、
    `connect_requested`、`open_bluetooth_settings`、`mapping_suspend` / `mapping_resume`
    （检测期间暂挂自定义按键）；
  - ③ 语音设备：`refresh_endpoints`（`detail=auto|manual_total_N_rec_M`）、`select_endpoint`
    （`detail=auto|manual|fallback`）、`open_download_page`；另有 `phase=audio_route`
    （`reason=missing|not_selected|selected|ready`，状态去重）；
  - ④ 输入工具：`read_tool_state`、`stage_binding`（`detail=<工具>`）、`save_other_keys`、
    `vokie_detect`（`detail=installed_0|1_running_0|1`）、`open_vokie_site`、`vokie_launch`；
  - ⑤ 按住说话：`armed` / 终态 / `session_stopped` / `observation_begin_failed` 等
    （见上一条核对）；
  - ⑥ 普通按键：`button_observation`、`mapping_suspend` / `mapping_resume`；
  - ⑦ 完成：`complete_refresh`（失败时 `detail=tool|vokie|capture|audio` 指出来源）。
- `phase=heartbeat` 每 30 秒一条（`reason=alive`）：携带当前 `step`、门禁 `code` 与
  步骤内等待摘要（如 `obs_0_n0`、`streaming`）。**长时间停留某一步时最后一跳就是现场**；
  这是"状态未变化不重复刷"的刻意例外。
- 脱敏红线：所有 `step/code/detail` 均为稳定 token 或计数，不含设备 id/名称、
  音频端点 id/名称、文件路径、错误原文与任何用户输入内容（自动化用例对
  遥控器/端点身份字段做了"绝不出现"断言）。

失败判定：正常路径判定失败或看不到文字；键盘打字仍判通过；焦点类失败文案缺失；快速点按导致卡死、重复 attempt、或出现两个终态；日志缺 attempt 配对或含文字内容。

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
