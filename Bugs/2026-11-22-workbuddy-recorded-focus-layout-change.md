# WorkBuddy「已学习输入框」在应用改版/切页后无法聚焦

- 发现日期：2026-11-22
- 状态：已修复；自动化、生产探针与本地测试安装包均 passed
- 影响范围：Windows 1.9.4；WorkBuddy 5.11.3（Electron 37.10.3）；「打开应用」聚焦方式为
  「打开后自动聚焦」（`recorded_element`）；遥控器与「测试打开与聚焦」入口均复现
- 功能点：聚焦与翻页 → WorkBuddy → 输入框学习与打开后聚焦
- 现象：界面显示「已记录输入框·已学习」，但按遥控器或点「测试打开与聚焦」都不会把
  焦点放进 WorkBuddy 的输入框；同一目标改用「自动选择输入框」可以成功
- 复现条件：WorkBuddy 停留在新任务页（输入框在页面中部），而学习记录来自聊天页
  （输入框贴底）；按绑定键打开/激活 WorkBuddy，等待聚焦结果
- 正常预期：打开 WorkBuddy 后，焦点落到它的输入框；无法定位时给出可解释原因，而不是
  反复重试到 `self_foreground`

## 证据

- 学习记录（2026-11-11 12:44:21，窗口 2326×1649）：
  `focus_learn_cursor phase=hit_test normalized_x=0.411 normalized_y=0.819 candidates=1 hits=1`
  → `focus_learn phase=completed terminal_result=passed`
- 保存的档案只有控件类型 + 归一化矩形：
  `controlType=Edit`，`automationId/className/name/windowTitle` 全空，
  `normalizedRect=(0.05125, 0.810647, 0.885×0.077435)`
- 2026-11-22 15:53:11 同一次会话里改用自动选择：
  `open_app_focus_best_composer` → `focus_attempt index=2 ... result=focused` →
  `focus_result ... terminal_result=passed attempts=2 elapsed_ms=598`
- 15:53:19 起切回「打开后自动聚焦」后连续失败：
  `focus_scan level=narrow path=cache elements=2 candidates=1` →
  `focus_attempt ... result=no_candidate`（8 次）→
  `focus_result ... reason=no_candidate` 或 `reason=self_foreground`
- 现场 UIA 候选（生产探针，修复前）：
  `type=Edit id_chars=0 class_chars=0 name_chars=0 focused=true text_pattern=true
  readonly=false rect=(0.333,0.330,0.601×0.120)`
  与保存的贴底矩形曼哈顿距离约 1.09，远超 0.05；匿名记录没有语义锚点，
  `recorded_candidate_score` 因此拒绝，`best_recorded_index` 返回 `None`
- 15:55:30 WorkBuddy 收在托盘时日志出现 `minimized=true`、
  `show_and_force_foreground ... took_show_path=false`，随后同样落到隐藏窗口的
  2 元素扫描；这是同一现场的第二条失败路径。安装包复验时 WorkBuddy 仍是
  `minimized=true`，新的唯一候选兜底同样让它通过（见下方安装版记录）

## 根因

WorkBuddy 5.11 的同一个 composer 在不同页面位置不同：聊天页贴底，新任务页居中。
2026-11-11 的学习记录是匿名 `Edit`，只有矩形锚点；旧规则要求无语义记录必须与保存
矩形几乎重合（距离 ≤0.05），页面切换后必然不命中。聚焦服务正确地拒绝把焦点送进
另一个输入框，但没有「唯一候选」兜底，于是 8 次尝试全部 `no_candidate`；重试期间
前台回到本应用后又被归类为 `self_foreground`。这不是 UIA 不可用，也不是遥控器链路
问题——自动选择路径在同一时刻可以聚焦成功。

## 修复

- `crates/sayall-windows/src/focus.rs`：新增 `single_recorded_candidate_fallback`。
  仅当记录没有 automationId / className / name / windowTitle / context token、
  只有 controlType + 归一化矩形，且当前扫描**恰好只有一个**同类型候选并通过学习
  门槛时，退回该候选；多个候选、带语义锚点的记录、只读/密码控件仍返回 `None`。
- `crates/sayall-windows/src/focus_windows.rs`：`recorded_element` 在精确匹配失败后
  尝试上述兜底；新增 `focus_recorded_fallback phase=selected reason=target_moved`
  与 `focus_recorded_match ... reason=no_matching_candidate` 结构化日志。
  兜底日志只表示“选中候选”，最终是否拿到焦点仍由 `element_has_focus` 读回决定。
- `crates/sayall-windows/examples/focus_probe.rs`：补 `--recorded-rect`、`--service`
  诊断入口，可在真机上复现「记录矩形 + 生产重试服务」路径；候选输出补 `class_chars`。

## 验证

- 自动化 passed：`cargo test -p sayall-windows`（569 passed / 17 ignored，含新增
  `geometric_record_falls_back_to_single_candidate_after_layout_change`）；
  `cargo fmt --all -- --check`。
- 真机探针 passed（2026-11-22 16:08，WorkBuddy 5.11.3，前台 pid 一致）：
  先把焦点移出输入框（`focused_before=none`，候选 `focused=false`），再用保存的
  底部矩形执行生产重试服务：
  - 第一次尝试：`focus_recorded_fallback phase=selected` → `result=no_candidate`
    （SetFocus 后读回尚未拿到焦点）
  - 第二次尝试：`result=focused`
  - 最终 `FocusReport { focused: true, attempts: 2, elapsed_ms: 400 }`
  这与自动选择路径的两次尝试节奏一致，证明记录矩形变化后仍能把焦点放回唯一输入框。
- 本地测试安装包 passed（2026-11-23 00:24 构建；`origin/main` `da30e290` + 本修复，
  本机测试签名，**不可发布**）：
  - 安装器 SHA-256 `03D32BA8D33DC3587731865385B42C8C41AA337A3FB5AFB95B1A58A029C1C829`
  - `verify-local-test-install.ps1 -ExpectRevision -NoLaunch`：helper / gadget / MCP
    哈希对齐、app exe 仅 3 字节 UNK→NSS、内嵌修订号 `da30e290`，全部 PASS
  - 遥控器实体按键（日志 `VolumeUp`，WorkBuddy `minimized=true`）：
    `focus_recorded_fallback phase=selected` → 第 1 次 `no_candidate` → 第 2 次
    `focused`，`focus_result ... terminal_result=passed attempts=2 elapsed_ms=492`
  - UI「测试打开与聚焦」：同一路径 `focus_result ... terminal_result=passed
    attempts=2 elapsed_ms=444`
- **deferred**：RC001 与 RC003 分型号记录（本次日志未区分型号）、冷态首用/连续按/
  断连/睡眠恢复矩阵；这些不能用本次单机安装包结果代替。

## 隐私检查

只记录控件类型、归一化矩形、候选数量与结果标签；未包含输入内容、应用内文本、
真实蓝牙地址、HID 路径、个人路径或凭据。
