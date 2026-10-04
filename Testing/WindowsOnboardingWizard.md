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
4. 每个工具测试前按向导第⑤步的核对卡逐项确认：工具麦克风 = CABLE Output；工具内语音键与 SayAll 一致；Vokie 保持运行；豆包需开启「支持更多输入工具」。

## 用例一：进入向导与中断继续

1. 全新状态（或删除向导状态文件）启动：应直接进入向导，停在第①步。
2. 老用户状态启动：不应进入向导；走设置页重跑入口 → 应进入向导并停在第①步。
3. 走到第④步后直接退出应用，重新启动：应恢复到第④步，不回到第①步。
4. 在向导里点「返回」：回到上一步；不丢进度、不自动前进。

预期：进入或跳过向导只由向导状态决定；重跑入口只重置向导进度，不清除设备 / 映射 / 音频设置。

失败判定：老用户被强制重走向导；或应进入时停在主界面；退出再进丢步骤；重跑入口清空任何既有设置。

## 用例二：步骤①–③（欢迎 / 遥控器 / 语音设备）

1. 第②步点「打开蓝牙设置」：系统「蓝牙和其他设备」页应真实弹出。
2. 扫描并连接遥控器；按一下普通按键：门禁从「按一下普通按键」变为可继续；按语音键不应误判为普通按键。
3. 第③步：检测到带「推荐」标记的 CABLE 设备并自动选中；没有时给出安装与重启说明。

## 用例三（核心）：第⑤步「按住说话验证」，三种工具各跑一遍

对 豆包 / 微信 / Vokie 分别执行：

1. 进入第⑤步：输入框应自动获得焦点；核对卡显示本次工具与快捷键。
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
    `connect_requested`、`open_bluetooth_settings`；
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

1. 第⑥步：按除语音键外的 3 个不同普通按键（如 主页 / OK / 方向键）→ 计数到 3 后可继续；期间遥控器普通按键不应执行用户已配置的映射动作（临时暂挂）；按语音键出现「这是语音键…」纠偏、且不计入。
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

## 验证边界

- 本手册只覆盖向导；语音质量、按键映射日常行为、安装升级矩阵仍按各自手册。
- 自动化（`pnpm test` / `cargo test --workspace` / runtime-simulation 检查）只能证明代码路径，不能替代本手册的真机步骤。
