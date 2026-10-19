# 记录位置随窗口变化失效、匿名输入框学不到、组合动作聚焦步骤超时

- 状态：已修复；自动化 passed；真机（安装包）passed：嵌套聚焦、6 秒学习窗口、类名匹配均通过
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
4. **只有类名特征的记录永远达不到匹配门槛**：DimAgent 的档案是
   `controlType=Group` + `className="tiptap ProseMirror outline-none ProseMirror-focused"`，
   没有 automationId / name / 矩形。类名命中只加 140 分，而阈值是 180 ⇒
   `best_recorded_index` 永远返回 `None`，日志只看到最终 `reason=timeout`。
   探针现场取证：候选恰好 1 个（hash `bad23a84`，focused=true），
   `attempt=NoCandidate { candidates: 1 }`。
5. **嵌套动作的激活是异步的**：聚焦服务按「当前前台进程」执行，`launch_app` 后立即提交
   请求时前几次尝试打在上一个前台应用上（WeChat 等），3 秒预算被耗尽。

## 修复

- `RecordedPoint` 增加四边物理像素距离（left/top/right/bottom）：
  窗口尺寸变化 ≤2% 仍用比例；超过则按**记录时最近的那条边**对齐（贴底输入框、贴顶搜索框
  都能跟上尺寸变化）；旧记录没有边距，保持原 25% 容差。
- 学习兜底：焦点元素不可用时，扫描目标窗口的可输入候选，取**矩形包含鼠标位置**的那一个
  （不依赖 `ElementFromPoint`，Chromium 可能只返回 render host）。
- 匿名输入框：`RecordedFocusTarget::normalized` 允许「无任何语义特征、只有矩形」的记录；
  `recorded_candidate_score` 对这类记录要求矩形曼哈顿距离 ≤ 0.05 才认账。
- 组合动作 `focus_input` 默认超时 2s → 8s（覆盖「等前台 2.5s + 聚焦服务 3s 重试预算」）。
- 嵌套组合动作里 `launch_app` 的激活是异步的，聚焦服务按**当前前台进程**执行；
  提交聚焦请求前新增有界等待 `wait_for_target_foreground(2.5s)`，避免前几次尝试
  打在旧前台应用上。单独执行时目标已在前台，所以只有嵌套场景暴露。
- 学习窗口统一 6 秒（后端 `LEARN_WINDOW`，前端倒计时只做展示），
  组合动作页点「学习输入框」立即开始采样，不再先空等 10 秒。
- 类名命中的分值 140 → 180：单独类名即可达到阈值；同名控件并列时分差仍为 0，
  继续拒绝，避免点错。
- 诊断日志补齐：`focus_wait_foreground`（是否等到前台，含耗时）、
  `focus_attempt index= pid= kind= result=`（每次尝试的前台 PID 与结果），
  失败时不必再用探针复现。

## 验证

- 真机：WorkBuddy 点击输入框 → `learned control_type=Edit id_chars=0 class_chars=0 name_chars=0`
  （修复前 `learn_failed=NoCandidate`）。
- 真机：DimAgent 前台 → 聚焦服务 `focused=true attempts=1 elapsed_ms=1384`
  （服务本身可用；步骤超时是瓶颈，已提到 8s）。
- 真机（2026-10-19 第三轮，安装包 `57fa5a47…`，内嵌修订 `a40c918a`）：
  - 探针：同一份 DimAgent 档案，修复前 `attempt=NoCandidate { candidates: 1 }`；
    类名分值调整后 `attempt=Focused { candidates: 1 }`。
  - 单步组合动作：学习窗口实测 6134 ms（目标未聚焦 → 6 秒后报"没有采到可用的输入框"）；
    聚焦 DimAgent 输入框后 `focus_learn phase=completed terminal_result=passed`。
  - 嵌套组合动作（WeChat 前台 → 打开微信 → 聚焦 DimAgent）：
    `focus_wait_foreground passed elapsed_ms=14`；
    `focus_attempt index=1 result=no_candidate`，`index=2 result=focused`；
    `focus_result terminal_result=passed attempts=2 elapsed_ms=4500`；
    `map_macro_terminal terminal_result=completed steps=2`，UI 显示「组合动作执行成功」。
- 单测新增：边距锚定（贴底 60px 在窗口变高 200px 后仍贴底）、旧记录保持 25% 容差、
  匿名 Edit 仅矩形匹配（距离 >5% 不认）、仅类名记录达到阈值且同名并列仍拒绝。
  `cargo test --workspace` 全绿（core 56 / windows 389 / app 131）；
  前端 422 passed（含「学习窗口 6 秒」用例）。

## 边界

- 记录位置仍不感知**布局变化**（例如输入框从底部移到顶部）：只能保证窗口尺寸变化时按边对齐。
- 匿名 Edit 只靠矩形：同窗口内若存在两个几乎重合的可输入控件，会因分差不足拒绝聚焦。
- 嵌套动作的聚焦依赖「目标应用能否被激活到前台」；被系统拒绝前台切换时仍会超时报错。
