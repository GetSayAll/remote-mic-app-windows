/**
 * 向导结构化日志：把流程事件翻译成 `reportFrontendEvent` 的固定载荷。
 *
 * 日志进入 `frontend event=onboarding ...` 行（sayall-diagnostic.log），与
 * LOGGING.md 的「入口/分支/外部调用/终态」要求对齐；`step` / `code` / `detail`
 * 都是稳定 token，Rust 侧逐字校验，含空格或中文会被记成 `invalid`。
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
  | "navigation"
  | "remote_observed"
  | "audio_route"
  | "tool_selected"
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
