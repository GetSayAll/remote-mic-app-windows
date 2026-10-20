# 聚焦输入框：地址栏可学习 + 微信（无 UIA 树应用）位置点击兜底

- 状态：实现完成；自动化 passed；真机取证 passed（Chrome 地址栏学习、位置点击机制）；
  微信目视验收 deferred（需用户在按键页记录位置后确认）
- 影响范围：按键页聚焦方式、组合动作聚焦步骤、UIA 学习/聚焦门槛、新增 `recorded_point` 策略
- 报告：2026-10-15 用户现场（"微信和Chrome无法学习成功" + "按键那边也需要聚焦输入框，参考 Mac 端"）

## 现象

1. 在 Chrome 里用「学习输入框」学地址栏，报 `输入框学习失败：SendInput failed: no_candidate`。
2. 微信 4.x（Qt）任何位置都学不到输入框。
3. 按键页的「聚焦输入框」动作与「打开后聚焦方式」面板在 2026-10-03 被隐藏，Mac 端有但 Windows 侧看不到入口。

## 根因（真机取证）

1. **地址栏**：`EXCLUDED_TERMS` 含「地址和搜索栏」，学习路径与自动选择共用同一硬门槛，
   用户**主动**点击地址栏也被拒。Chrome 实测（`Testing/probe-uia-focus.ps1`）：
   omnibox 是 `Edit / OmniboxViewViews / name=地址和搜索栏`，`belongs_to_target=true`、
   可聚焦、非只读——完全可用，只是被排除词挡住。
2. **微信 4.x**：主窗口与渲染子窗口对 `OBJID_CLIENT` 与 `UiaRootObjectId` **都返回 0**
   （`sayall-msaa-probe.ps1` 实测；Chrome 对照组分别返回 49543 / 65536），
   整棵 UIA 树只有 2 个 `Pane`、0 个可聚焦元素。这是应用侧边界，不是本产品缺陷；
   公开 API 无路可走，只能按位置点击。
3. 隐藏入口：`showAppFocusStrategy` / `DEVICE_ACTION_SECTION_ENABLED` 两个开关为 false。

## 修复

- **门槛分档**：新增 `FocusCandidate::passes_learning_gate()`（用户主动指定的元素：
  不含排除词，仍拒绝密码/敏感/只读/不可聚焦）；`passes_hard_gate()`（自动选择）保持原样。
  学习采集（`capture_focused_target`）、记录匹配（`recorded_candidate_score`）、
  按记录聚焦（`FocusChoice::Recorded` 的候选枚举）统一走学习档；
  `RecordedFocusTarget::normalized` 不再用排除词丢弃记录。
- **新策略 `recorded_point`**：`RecordedPoint { xRatio, yRatio, windowWidth, windowHeight }`；
  学习 = 读物理光标并要求落在目标窗口内（`learn_focus_point`）；
  聚焦 = 激活目标 → 窗口尺寸校验（±25%）→ 换算绝对坐标 → 物理光标移动 + 左键单击
  （`recorded_point_absolute` + `SendInputRuntime::move_cursor_and_click`）。
  尺寸变化超限拒绝点击（`window_size_changed`），不赌运气点错地方。
- **界面恢复**：按键页「设备操作 / 聚焦输入框」动作与「打开后聚焦方式」面板恢复可见
  （对齐 Mac 的 `action.focus_input` 与聚焦方式三选一，本机多一档「记录点击位置」）；
  宏页面聚焦步骤同样提供「记录点击位置」，与按键页共用同一份聚焦档案。
- 失败原因全部为稳定 token，前端映射为中文（`focusPointErrorLabel`）。

## 验证

- 单测（sayall-windows 382 passed）：学习档接受地址栏、自动档仍拒绝；记录地址栏可重新命中；
  `RecordedPoint` 换算/越界/尺寸漂移；`AppFocusProfile::normalized` 对 `recorded_point` 的自洽性。
- 前端：vitest 398 passed | 10 skipped（含新策略与失败文案用例）、`vue-tsc`、`vite build`。
- 真机（本机 Windows 11，2026-10-15）：
  - Chrome 地址栏：修复前 `focused_before=none + learn_failed=NoCandidate`；
    修复后 `learned control_type=Edit id_chars=9 class_chars=16 name_chars=6 elapsed_ms=107`
    （= view_1012 / OmniboxViewViews / 地址和搜索栏）。
  - 位置点击：记事本记录 `x_ratio=0.5000 y_ratio=0.5961 window=888x671`
    （= 窗口 rect (1945,294) 内的 (2389,694)）；点击阶段 `absolute=(2389,694)`、
    `click_submitted=ok`、`focused_after=Document/RichEditD2DPT`。
- deferred：微信的实际聚焦效果需要用户在按键页现场确认（应用不暴露 UIA，读不到客观焦点）。

## 边界

- 位置点击只在目标窗口尺寸与记录时相差 ≤25% 时执行；超限报 `window_size_changed`。
- 目标窗口必须已由本产品激活（`launch_app` 先行），且点击前校验它在最前面（`not_foreground`）。
- 位置点击不做图像识别、不读窗口内容，只记坐标比例与窗口尺寸。
