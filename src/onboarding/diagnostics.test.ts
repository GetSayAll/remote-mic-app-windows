import { describe, expect, it } from "vitest";
import { buildOnboardingEvent } from "./diagnostics";

describe("onboarding diagnostics payloads", () => {
  it("prefixes every onboarding event with the same feature token and keeps stable reasons", () => {
    const payload = buildOnboardingEvent({
      kind: "step_entered",
      result: "passed",
      reason: "enter",
      step: "remote",
    });
    expect(payload).toEqual({
      event: "onboarding",
      phase: "step_entered",
      result: "passed",
      reason: "enter",
      step: "remote",
    });
  });

  it("carries blocked reasons with failure code and optional detail token", () => {
    const payload = buildOnboardingEvent({
      kind: "step_blocked",
      result: "failed",
      reason: "gate_blocked",
      step: "audio",
      code: "audio.install_required",
      detail: "download_dialog",
    });
    expect(payload).toEqual({
      event: "onboarding",
      phase: "step_blocked",
      result: "failed",
      reason: "gate_blocked",
      step: "audio",
      code: "audio.install_required",
      detail: "download_dialog",
    });
  });

  it("omits empty optional fields so the log line stays exact", () => {
    const payload = buildOnboardingEvent({
      kind: "navigation",
      result: "passed",
      reason: "user_back",
      code: null,
      detail: undefined,
    });
    expect(payload).toEqual({
      event: "onboarding",
      phase: "navigation",
      result: "passed",
      reason: "user_back",
    });
  });

  it("keeps elapsedMs only when provided", () => {
    expect(
      buildOnboardingEvent({
        kind: "step_passed",
        result: "passed",
        reason: "gate_satisfied",
        step: "controls",
        elapsedMs: 1234,
      }).elapsedMs,
    ).toBe(1234);
    expect(
      buildOnboardingEvent({
        kind: "step_passed",
        result: "passed",
        reason: "gate_satisfied",
        step: "controls",
      }).elapsedMs,
    ).toBeUndefined();
  });
});
