/**
 * 向导结构化日志：把流程事件翻译成 `reportFrontendEvent` 的固定载荷。
 *
 * 日志进入 `frontend event=onboarding ...` 行（sayall-diagnostic.log），与
 * LOGGING.md 的「入口/分支/外部调用/终态」要求对齐；`step` / `code` / `detail`
 * 都是稳定 token，Rust 侧逐字校验，含空格或中文会被记成 `invalid`。
 *
 * 「用户在哪一步卡住」的定位约定（2026-10-05 用户要求）：
 * - 每个外部调用/用户操作落 `kind=action` 对：begin=`result=unknown`，
 *   end=`result=passed|failed` 且带 `elapsed_ms`；**begin 之后没有 end 即为
 *   卡在该调用**；
 * - `kind=heartbeat` 每 30s 一条（刻意保留的重复心跳，是"状态未变化不重复刷"
 *   的显式例外），携带当前 step 与门禁码，用于"用户长时间停留在某一步"的定位；
 * - `step_entered` / `step_blocked` / `step_recovered` / 各步骤的领域事件
 *   （remote_observed/audio_route/tool_selected/authorization/voice_attempt…）
 *   共同构成"最后一条日志 = 当时所处环节"的证据链。
 *
 * 脱敏红线（LOGGING.md 隐私红线）：detail/step/code 只允许稳定 token 与计数，
 * 不含设备 id/名称、音频端点 id/名称、文件路径、错误原文或任何用户输入内容。
 */
import {
  reportFrontendEvent,
  type FrontendDiagnosticEvent,
} from "../lib/frontend-diagnostics";
import type { OnboardingStep } from "./flow";

/** 事件种类（kind 即日志里的 phase）：语义固定，扩展时同步设计稿 §8 与测试。 */
export type OnboardingEventKind =
  | "started"
  | "migration"
  | "step_entered"
  | "step_passed"
  | "step_blocked"
  | "step_retry"
  | "step_recovered"
  | "persist"
  | "navigation"
  | "remote_observed"
  | "voice_attempt"
  | "audio_route"
  | "tool_selected"
  | "action"
  | "heartbeat"
  | "authorization"
  | "binding"
  | "completed"
  | "restarted";

export interface OnboardingEventInput {
  kind: OnboardingEventKind;
  result: "passed" | "failed" | "unknown";
  /** 稳定英文 token（snake_case），说明"发生了什么"。 */
  reason: string;
  /** 事件所属步骤（导航事件里表示"从哪一步"）。 */
  step?: OnboardingStep;
  /** 失败码（设计稿 §7）；仅 result=failed 时应有值。 */
  code?: string | null;
  /** 次要 token（导航目标、工具名等）。 */
  detail?: string | null;
  elapsedMs?: number;
}

export function buildOnboardingEvent(input: OnboardingEventInput): FrontendDiagnosticEvent {
  const payload: FrontendDiagnosticEvent = {
    event: "onboarding",
    phase: input.kind,
    result: input.result,
    reason: input.reason,
  };
  if (input.step !== undefined) payload.step = input.step;
  if (input.code) payload.code = input.code;
  if (input.detail) payload.detail = input.detail;
  if (input.elapsedMs !== undefined) payload.elapsedMs = input.elapsedMs;
  return payload;
}

export function reportOnboardingEvent(input: OnboardingEventInput): void {
  reportFrontendEvent(buildOnboardingEvent(input));
}
