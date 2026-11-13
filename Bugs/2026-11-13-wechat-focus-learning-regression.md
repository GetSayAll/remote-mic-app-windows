# 1.9 回归：微信「聚焦学习」失效，记录点击位置入口消失

- 发现日期：2026-11-13
- 状态：已修复（自动化 passed；真实微信目视验收 deferred）
- 影响范围：1.9.0 / 1.9.1；组合动作「聚焦学习过的输入框」、APP 适配页预置应用、按键页「打开 APP + 聚焦」
- 功能点：聚焦档案解析、预设应用聚焦配置、`recorded_point` 学习
- 现象：用户反馈 1.8 能学会聚焦微信，1.9 怎么学都不成功；想把「打开微信」+「聚焦到某个群」做成组合动作，学不出来，也不知道去哪里操作。
- 复现条件：Windows + 微信 4.x；在组合动作里选择内置预设「微信」作为聚焦目标，或在 APP 适配页手动添加微信。
- 正常预期：微信 4.x 不暴露 UIA 树，应允许用户记录点击位置；已学过的档案应优先于预设默认值执行。

## 证据

- 微信 4.x 主窗口与渲染子窗口的 `OBJID_CLIENT` / `UiaRootObjectId` 都返回 0，UIA 树没有可聚焦输入框；只能走 `recorded_point`（`Bugs/2026-10-15-focus-address-bar-and-recorded-point.md`、`TODO.md` 已知边界）。
- 1.8.3 的 `open_app_effective_profile` 顺序是「显式档案 → 预设默认」；1.9 的 `focus_profile_for_target` 改成「预设 → 显式档案」，微信的预设默认 `auto_composer` 覆盖了用户学过的 `recorded_point`（`crates/sayall-windows/src/button_mapping.rs`）。
- 1.9 的 `AppFocusProfileEditor` 只保留「只打开 APP / 打开后自动聚焦」两个主选项，`recorded_point` 不再提供入口。
- APP 适配页对预置应用只显示只读「预置能力」卡，不渲染编辑器；组合动作页对内置预设不显示摘要和「去配置」。
- 数据未丢失：`migrate_preset_focus_profiles` 会保留 `recorded_point` / `recorded_element` / `app_shortcut`，只是执行时被跳过。

## 根因

1. **执行侧**：预设优先解析导致 1.8 已学档案失效。
2. **界面侧**：记录点击位置入口被移除，预置应用没有可进入的学习界面，组合动作页也没有配置入口。

## 修复

- `focus_profile_for_target` 恢复「显式档案优先，预设默认兜底」，并支持预设 id / 真实路径两种档案键互相查找。
- 前端统一使用 `effectiveProfileForTarget`：按键页、组合动作页、APP 适配页同一口径。
- `AppFocusProfileEditor` 恢复「记录点击位置」选项与 6 秒学习流程；APP 适配页对没有可靠内置聚焦的预置（微信等 `auto_composer`）或已有显式档案的预置显示编辑器。
- 组合动作页为预置目标显示聚焦摘要和「去配置」；显式导航到未加入应用库的预置时，APP 适配页显示该预置的详情，但不写入应用库。
- 列表摘要显示实际聚焦方式；预置聚焦卡区分「预置可用 / 需设置 / 已自定义」。

## 验证

- `cargo fmt --all -- --check` passed。
- `cargo test --workspace -j 1` passed：`sayall-windows` 556 passed / 17 ignored；`sayall-windows-app` 220 passed / 2 ignored；其余 workspace 测试 passed。
- `cargo check -p sayall-windows-app --features runtime-simulation` passed。
- `pnpm test` passed：689 passed | 10 skipped。
- `pnpm build`（`vue-tsc --noEmit` + `vite build`）passed。
- 新增回归用例：微信 `recorded_point` 档案优先于预设默认；预置微信可学习点击位置并按预设 id 落盘；预置目标显示摘要与「去配置」；旧 `recorded_point` 档案在编辑器里可见可执行；预置滚动区域可学习覆盖并在清除后恢复预置。
- 本地测试包：2026-11-14 变更后重新构建并安装验证，结果见交付说明。
- 真实微信目视确认：**deferred**。需要在本机微信 4.x 里记录一个群聊输入框位置，确认「打开微信 → 聚焦到该群」实际点击落点正确。

## 隐私检查

本记录未包含个人路径、设备身份、聊天内容或凭据；只保留代码路径、策略名和测试计数。

## 2026-11-14 追加：预置 APP 与普通 APP 一致

用户要求移除只读「预置能力」卡：预置 APP 也显示聚焦方式与滚动区域，预置默认策略显示为选中并标「预置可用」；用户仍可重新学习覆盖，滚动区域清除后恢复预置。已同步修改 `AppsPage`、`AppFocusProfileEditor` 与滚动区域预设 id/路径别名解析；自动化 passed。
