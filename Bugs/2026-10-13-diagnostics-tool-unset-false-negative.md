# 向导①/②步复制诊断块把已配置的输入工具报成 `unset`

- 发现日期：2026-10-13（Andy 本机诊断块发现：块里 `Input tool: unset hotkey=unset`，
  而磁盘上 `voice_input_tool=doubao` 且 `voice-hold-hotkey.json` 有值）
- 状态：已修复（自动化 passed；真机复核 deferred）
- 影响范围：0.8.6（origin/main）及之前：向导第①–③步复制出的诊断块；
  只影响报障文本的可读性，不影响任何运行行为
- 功能点：Onboarding「复制诊断信息」的合成（`src/onboarding/diagnostics-text.ts` 的输入）
- 现象：诊断块显示 `Input tool: unset hotkey=unset`，与磁盘真实配置（`doubao` +
  已保存快捷键）不符——支持会据此误判"这台机器还没配置输入工具"
- 复现条件：机器上已配置输入工具与快捷键 → 让向导停在①/②/③步（这两项只在④/⑤步加载）
  → 点「复制诊断信息」
- 正常预期：诊断块在任何步骤都反映设备的真实配置（与音频那一行同口径）
- 证据：`%APPDATA%\app.getsayall.remote-mic.windows\settings.json` 的
  `voice_input_tool=doubao` + `voice-hold-hotkey.json` 有值，与块里的 `unset` 矛盾；
  代码路径 `copyDiagnostics` 用 `stagedTool/configuredTool/currentHotkey`（页面状态），
  而页面只在④/⑤步加载它们
- 根因：诊断块这两项读的是**页面状态**；页面在①–③步从未读取过工具/快捷键。
  音频那一行早已因为同样的理由改成"复制时只读刷新"（见 `copyDiagnostics` 注释），
  工具/快捷键漏了同一处理
- 修复：`copyDiagnostics` 在复制时对 `getVoiceInputTool()` / `getVoiceHoldHotkey()` 做一次
  **只读**读取（`Promise.all` + try/catch）；读取失败退回页面状态、复制本身不失败。
  取值优先级：向导会话里已暂存的值 > 磁盘读取值 > 页面已加载值。新装机器上
  `getVoiceHoldHotkey()` 返回生效的默认和弦（左 Ctrl + 左 Win），因此块里不再出现
  误导性的 `unset`
- 验证：`pnpm vitest run src/pages/OnboardingPage.test.ts` 先红后绿（41 passed；
  新增两例：②步复制出 `Input tool: doubao hotkey=right_alt`；读取失败退回 `unset`
  且复制仍成功）；全量 `pnpm test` 301 passed | 11 skipped、`pnpm build`
  （vue-tsc --noEmit + vite build）passed。**deferred**：真机按
  `Testing/WindowsOnboardingWizard.md`「诊断信息」一节复核（已配置工具的机器在①/②步
  复制应显示真实工具与快捷键）
- 隐私检查：新增字段仍只有稳定 token 与布尔（工具名 token、快捷键组合 token、
  无端点/设备身份、无路径）
