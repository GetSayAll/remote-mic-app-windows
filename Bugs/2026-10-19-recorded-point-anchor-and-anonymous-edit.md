# 记录位置随窗口变化失效、匿名输入框学不到、组合动作聚焦步骤超时

- 状态：已修复；自动化 passed；真机取证 passed（学习兜底 / 聚焦服务），位置锚定与超时需装新包复验
- 影响范围：聚焦位置记录、输入框学习兜底、组合动作「聚焦学习过的输入框」步骤
- 报告：2026-10-19 用户现场（WorkBuddy 位置变化后无法聚焦、WorkBuddy 输入框无法记录、
  DimAgent 已学习但组合动作执行失败）

## 现象

1. WorkBuddy「记录点击位置」可用，但窗口位置/尺寸变化后再聚焦失败。
2. WorkBuddy「聚焦已记录的输入框」无法记录（学习失败：`no_candidate`）。
3. DimAgent 已学习过输入框，但组合动作里「聚焦学习过的输入框」步骤执行失败；
   日志 `focus_result kind=launch_then_focus attempts=3 elapsed_ms≈2050 reason=cancelled`。

## 根因

1. **记录只有比例 + 25% 尺寸容差**：窗口高度一变（如 1281→1710），比例点落到消息区；
   容差外直接拒绝。聊天输入框这类贴底控件需要按「最近的边」对齐，而不是死守比例。
2. **WorkBuddy（Electron）把焦点报成顶层 `Window`（`focusable=false`）**，
   学习路径只接受 `GetFocusedElement` 的结果 ⇒ 永远是 `no_candidate`；
   它的输入框是匿名 `Edit`（无 automationId / className / name），旧模型也不允许
   只靠矩形记录。
3. **步骤超时 2 秒 < 聚焦服务预算 3 秒**：`launch_then_focus` 需要等目标应用激活 +
   UIA 重试（服务预算 3 秒），2 秒时步骤判失败并丢弃 runner，服务随即报 `cancelled`。

## 修复

- `RecordedPoint` 增加四边物理像素距离（left/top/right/bottom）：
  窗口尺寸变化 ≤2% 仍用比例；超过则按**记录时最近的那条边**对齐（贴底输入框、贴顶搜索框
  都能跟上尺寸变化）；旧记录没有边距，保持原 25% 容差。
- 学习兜底：焦点元素不可用时，扫描目标窗口的可输入候选，取**矩形包含鼠标位置**的那一个
  （不依赖 `ElementFromPoint`，Chromium 可能只返回 render host）。
- 匿名输入框：`RecordedFocusTarget::normalized` 允许「无任何语义特征、只有矩形」的记录；
  `recorded_candidate_score` 对这类记录要求矩形曼哈顿距离 ≤ 0.05 才认账。
- 组合动作 `focus_input` 默认超时 2s → 6s（覆盖聚焦服务 3 秒重试预算 + 激活时间）。

## 验证

- 真机：WorkBuddy 点击输入框 → `learned control_type=Edit id_chars=0 class_chars=0 name_chars=0`
  （修复前 `learn_failed=NoCandidate`）。
- 真机：DimAgent 前台 → 聚焦服务 `focused=true attempts=1 elapsed_ms=1384`
  （服务本身可用；步骤超时是瓶颈，已提到 6s）。
- 单测新增：边距锚定（贴底 60px 在窗口变高 200px 后仍贴底）、旧记录保持 25% 容差、
  匿名 Edit 仅矩形匹配（距离 >5% 不认）。`cargo test --workspace` 全绿
  （core 56 / windows 388 / app 131）。

## 边界

- 记录位置仍不感知**布局变化**（例如输入框从底部移到顶部）：只能保证窗口尺寸变化时按边对齐。
- 匿名 Edit 只靠矩形：同窗口内若存在两个几乎重合的可输入控件，会因分差不足拒绝聚焦。
