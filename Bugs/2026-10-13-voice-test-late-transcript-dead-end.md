# 向导第⑤步：文字迟于 3 秒窗口出现 → 输入框有字但判定失败、无法继续

- 发现日期：2026-10-13
- 状态：已修复（自动化 passed；真机复验 deferred）
- 影响范围：向导第⑤步「按住说话验证」；现场报告来自 RC003 机器、输入工具=豆包输入法
  （Windows 10.0.26200，版本以当轮本地测试包为准）；RC001 未观察。
- 功能点：onboarding 第⑤步（`src/pages/OnboardingPage.vue` 语音段 +
  `src/onboarding/voice-attempt.ts` + `src/onboarding/steps/VoiceTestStep.vue`）
- 现象：用户看到文字已经出现在输入框里，结果卡却写「语音链路正常，但输入工具没有写出
  文字」，右栏「文字已经出现」为 `!`，「继续」不可用——除「重新测试」外没有出路。
- 复现条件：会话 / 采样 / 投递三行全绿（attempt 判定到 `waiting_transcript`），但
  ① 文字晚于「会话结束后 3 秒」窗口才进入输入框；或 ② 文字进入输入框但**没有派发
  DOM `input` 事件**（该窗口内事件判据看不到）。
- 正常预期：文字出现在输入框即视为通过。Mac 侧门禁按输入框现值实时复算
  （`OnboardingView.swift`：`transcriptionAppeared = !transcript.trimmed.isEmpty`），
  迟到文字会把「继续」放开；Windows 原先只看事件、且终态粘死，属独有差异。
- 证据：
  - 现场截图：输入框有文字 + `voice.no_transcript` 文案 + 「文字已经出现」`!` +
    「继续」禁用；
  - 页面级复现测试（修复前红）：晚到文字被忽略、无事件写入不可见
    （`src/pages/OnboardingPage.test.ts`）；
  - 代码路径：`voice-attempt.ts:230` 3 秒截止 → `voice.no_transcript`；页面
    `onVoiceInput` 对终态早退（原实现直接 `return`）。
- 根因（已确认）：
  1. 通过判据只认 DOM `input` 事件（`transcriptSeen`），从不读输入框现值；
  2. 3 秒窗口到期即产生终态，迟到文字被静默忽略（页面层无翻案通道），与 Mac 的
     现值门禁行为不一致；失败文案此时与屏幕上的事实矛盾。
  - 仍未知：本轮现场文字到底迟到多久——原日志只记 `voice.no_transcript`，没有
    「迟到」这一事实（本次修复补上了 `late_transcript detail=after_<ms>`）。
- 修复（最小改动）：
  - `VoiceTestStep.vue`：暴露 `readValue()`（读回输入框现值）；
  - `OnboardingPage.vue`：① `waiting_transcript` 期间每帧读回现值，非空则先结算
    物理键计数再记转写（日志 `transcript_value_readback`）；② `voice.no_transcript`
    失败后保留轮询作「迟到监视」（只读 DOM、不发快照 IPC）与物理键观察窗口；
    迟到文字在物理键计数为 0 时翻案为通过（`late_transcript detail=after_<ms>`，
    并放开「继续」），计数 > 0 保持失败（`late_transcript_manual`，同一 attempt
    不再翻案）；③ 诊断块新增 `late=` 与 `terminal=late_transcript`。
  - 终态规则不动：`voice-attempt.ts` 的「终态不可改写」只约束追踪器本身；迟到接受
    是页面层的受控通道（`voice-attempt.ts` 头部注释已同步）。
- 验证：`vitest run` **336 passed / 11 skipped**（新增 4 例：迟到接受、迟到+物理键
  阴性对照、无事件写入读回、失败后读回）；`vue-tsc --noEmit` 与 `vite build` passed。
  真机复验 deferred（需真实遥控器 + 真实输入工具冷启动路径，按
  `Testing/WindowsOnboardingWizard.md` 用例三第 7 项执行）。
- 隐私检查：新增日志只含稳定 token、计数与毫秒；不含输入文字、设备身份、端点或路径。
