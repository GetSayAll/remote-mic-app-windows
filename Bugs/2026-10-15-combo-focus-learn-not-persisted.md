# 组合动作「聚焦学习过的输入框」：宏页面学习不落盘 + 失败文案是机器码

- 状态：已修复（自动化 passed；真机复验 deferred——需重装含本修复的包）
- 影响范围：组合动作页「聚焦学习过的输入框」步骤（学习入口与错误提示）
- 报告：2026-10-15 用户现场（"有哪些前置动作要完成？直接测试提示 输入框学习失败：SendInput failed: no_candidate"）

## 现象

组合动作页的「学习输入框」显示成功，但该步骤仍然不可用（提示需要先在按键页学习）；
学习失败时提示 `输入框学习失败：SendInput failed: no_candidate`（机器码）。

## 根因

1. **宏页面的学习结果被丢弃**：按键页的学习流程会把采到的记录写进
   `button-mappings.json` 的 `focusProfiles[target]`（策略 `recorded_element`），
   宏页面只调 `learn_focus_target` 显示"成功"、**没有落盘**⇒ 执行期按
   `focusProfiles` 解析目标时永远找不到档案（缺依赖）。
2. **失败文案**：底层是 `FocusFailure::as_str()` 的稳定 token
   （`no_candidate` 等），经 `PlatformError::SendInput` 的 `Display`
   （`SendInput failed: {0}`）拼出来直接展示给用户。
   `no_candidate` 的真实含义：3 秒采样窗口内**没有采到合格的可编辑输入框**——
   常见于焦点还在无线麦自己身上、点到了非输入区域、目标是管理员权限窗口。

## 修复

- 宏页面学习流程与按键页**同一落盘口径**：`getButtonMappings` → 写入
  `focusProfiles[target] = { strategy: "recorded_element", recorded }` →
  `saveButtonMappings`；成功后刷新档案状态。
- 步骤卡新增**前置状态行**（"已学习这个 App 的输入框" / "还没有学习过"），
  按钮按状态显示「学习输入框 / 重新学习」；前置说明按实际采样口径重写
  （倒计时结束前切到目标 App 点进输入框并保持；密码/搜索/地址栏/只读不记录）。
- `focusLearnErrorLabel`：`no_candidate / self_foreground / not_foreground /
  target_exited / not_accessible / timeout / cancelled` 映射成用户语言，
  并剥掉 `SendInput failed:` 前缀。

## 验证

- vitest：学习成功后 `saveButtonMappings` 载荷里 `focusProfiles.notepad.strategy
  === "recorded_element"`（正是本次回归点）；失败路径显示中文且不含
  `no_candidate`；已学习态显示「已学习」+「重新学习」。全量 394 passed | 11 skipped。
- `vue-tsc` + `vite build` passed。

## 边界

- 「测试这步 / 测试动作」仍遵循生产语义：档案缺失或定位失败按
  `missing_dependency` / `step_failed` 报；管理员权限目标窗口读不到 UIA 时
  走 `not_accessible` 分支。
