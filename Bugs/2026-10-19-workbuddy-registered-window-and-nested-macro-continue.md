# WorkBuddy 聚焦/记录位置全部不可用 + 嵌套组合动作只执行第一个

- 状态：已修复；真机取证 passed（WorkBuddy 窗口识别、记录位置）；嵌套执行自动化 passed
- 影响范围：注册应用（「扫描本机应用」）的窗口判定、组合动作嵌套失败语义
- 报告：2026-10-19 用户现场

## 现象

1. WorkBuddy 已打开，但「聚焦已记录的输入框」「记录点击位置」都不可用，
   提示「记录位置失败：目标应用没有可见窗口，请先打开再试」。
2. 一个组合动作里放两个「执行已有组合动作」，实际只执行了第一个。

## 根因

1. **注册应用的窗口只按 AUMID 查**：WorkBuddy 由「扫描本机应用」登记为
   `shell:AppsFolder\WorkBuddy.WorkBuddy`。它是 Electron 应用，窗口没有 AUMID
   （`window_presence=false`），而 `target_window_visibility` / `application_readiness`
   对注册目标只走 `WindowSelector::AppUserModelId` ⇒ 永远得到 0 个窗口。
   启动路径本来就会用 shell 项的 `System.Link.TargetParsingPath` 解析真实 exe
   （AUMID 与 exe 双口径），窗口判定没有复用这条口径。
2. **嵌套动作缺依赖会中断父动作**：用户的第一个子动作「打开微信」里有
   `focus_input(wechat)`，而微信没有学习档案 ⇒ 子动作终态 `missing_dependency`
   ⇒ 父动作按「首败即停」立即结束，第二个子动作没有机会执行。
   日志取证：`map_macro_step ... action=run_macro terminal_result=missing_dependency`，
   父动作 `steps=1`。

## 修复

- `app_launcher::target_window_visibility` / `application_readiness` 对注册目标改为
  **AUMID + shell 解析出的 exe 路径双口径**（`resolve_registered_executable_path`，
  与启动路径同一来源），并在 `app_readiness_probe` 里打印解析出的 exe 便于取证。
- 组合动作执行器：**「执行已有组合动作」步骤缺依赖不再阻断后续步骤**——该步如实
  记为 `missing_dependency`，后面的步骤继续执行，整宏终态仍报 `missing_dependency`
  （不把问题粉饰成成功）。普通步骤的「首败即停」保持不变。

## 验证

- 真机（WorkBuddy 正在运行，窗口 `Chrome_WidgetWin_1`）：
  - 修复前：`known=true running_process=false visible_window=false window_presence=false`
  - 修复后：`known=true running_process=true visible_window=true window_presence=true`；
    `focus_probe --learn-point "shell:AppsFolder\WorkBuddy.WorkBuddy"` →
    `learned_point x_ratio=0.4080 y_ratio=0.8444 window=2054x1710`
  - WorkBuddy UIA 有可聚焦 Edit 候选（`candidate_count=1`，可输入、非只读），
    「聚焦已记录的输入框」路径可用。
- 单测：新增 `nested_missing_dependency_does_not_skip_later_nested_steps`
  （父动作两个嵌套步骤：第一步缺失、第二步照常执行；终态 `missing_dependency`）。
  `cargo test --workspace` 全绿（core 56 / windows 387 / app 131）。

## 边界

- 注册应用若 shell 项解析不到 exe（`TargetParsingPath` 缺失），仍只能按 AUMID 查；
  这类目标应改用「添加应用」直接选 exe。
- 嵌套动作的普通失败（`step_failed`）仍会终止父动作；只有缺依赖改为继续。
