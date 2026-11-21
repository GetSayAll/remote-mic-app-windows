/**
 * 「重置遥控器」引导的判据（2026-11-21 用户要求）：只有确认按键不会到达时才提示，
 * 已收到按键后绝不能提示——重置会打断一条已经工作的链路。
 */
import { describe, expect, it } from "vitest";
import {
  RESET_GUIDANCE_DELAY_MS,
  isGenericRemoteName,
  shouldShowResetGuidance,
} from "./reset-guidance";

describe("isGenericRemoteName", () => {
  it("recognizes the Windows generic name case/whitespace-insensitively", () => {
    expect(isGenericRemoteName("MI RC")).toBe(true);
    expect(isGenericRemoteName("  mi rc ")).toBe(true);
    expect(isGenericRemoteName("小米蓝牙语音遥控器 2 Pro")).toBe(false);
    expect(isGenericRemoteName("")).toBe(false);
    expect(isGenericRemoteName(null)).toBe(false);
    expect(isGenericRemoteName(undefined)).toBe(false);
  });
});

describe("shouldShowResetGuidance", () => {
  const base = {
    connected: false,
    buttonObserved: false,
    genericRemoteName: false,
    connectedWithoutButtonMs: 0,
    rawInputPhase: "stopped" as const,
  };

  it("never shows after a button was observed", () => {
    expect(
      shouldShowResetGuidance({
        ...base,
        connected: true,
        buttonObserved: true,
        genericRemoteName: true,
        rawInputPhase: "awaiting",
      }),
    ).toBe(false);
  });

  it("shows immediately for the generic MI RC name, even before connecting", () => {
    expect(shouldShowResetGuidance({ ...base, genericRemoteName: true })).toBe(true);
  });

  it("waits the full delay after connecting before prompting a reset", () => {
    expect(
      shouldShowResetGuidance({
        ...base,
        connected: true,
        connectedWithoutButtonMs: RESET_GUIDANCE_DELAY_MS - 1,
      }),
    ).toBe(false);
    expect(
      shouldShowResetGuidance({
        ...base,
        connected: true,
        connectedWithoutButtonMs: RESET_GUIDANCE_DELAY_MS,
      }),
    ).toBe(true);
  });

  it("shows at once when Raw Input is awaiting the remote's button channel", () => {
    expect(
      shouldShowResetGuidance({
        ...base,
        connected: true,
        rawInputPhase: "awaiting",
      }),
    ).toBe(true);
  });

  it("does not show while disconnected (the reconnect card owns that case)", () => {
    expect(
      shouldShowResetGuidance({
        ...base,
        connected: false,
        connectedWithoutButtonMs: RESET_GUIDANCE_DELAY_MS * 10,
      }),
    ).toBe(false);
  });
});
