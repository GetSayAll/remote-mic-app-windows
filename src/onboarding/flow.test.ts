import { describe, expect, it } from "vitest";
import {
  ONBOARDING_STEPS,
  evaluateGate,
  nextStep,
  normalizeStep,
  phaseOf,
  previousStep,
  type OnboardingContext,
  type OnboardingStep,
} from "./flow";

function makeContext(
  patch: {
    remote?: Partial<OnboardingContext["remote"]>;
    audio?: Partial<OnboardingContext["audio"]>;
    voiceTool?: Partial<OnboardingContext["voiceTool"]>;
    voiceTest?: Partial<OnboardingContext["voiceTest"]>;
    controls?: Partial<OnboardingContext["controls"]>;
  } = {},
): OnboardingContext {
  return {
    remote: {
      connectionPhase: "ready",
      bleVoiceReady: true,
      rawInputPhase: "ready",
      remoteButtonObserved: true,
      ...patch.remote,
    },
    audio: {
      recommendedEndpointAvailable: true,
      selectedEndpointRecommended: true,
      wasapiReady: true,
      lastError: null,
      ...patch.audio,
    },
    voiceTool: {
      tool: "wechat",
      doubaoCaptureEnabled: false,
      vokieInstalled: false,
      vokieRunning: false,
      otherHotkeyChosen: false,
      ...patch.voiceTool,
    },
    voiceTest: { verified: true, ...patch.voiceTest },
    controls: { distinctButtons: 3, ...patch.controls },
  };
}

describe("onboarding step order", () => {
  it("keeps the confirmed Mac order (D1): welcome, remote, audio, voice_tool, voice_test, controls, complete", () => {
    expect(ONBOARDING_STEPS).toEqual([
      "welcome",
      "remote",
      "audio",
      "voice_tool",
      "voice_test",
      "controls",
      "complete",
    ]);
  });

  it("maps phases: prepare for welcome, setup for the three config steps, try_it for the rest", () => {
    expect(phaseOf("welcome")).toBe("prepare");
    expect(phaseOf("remote")).toBe("setup");
    expect(phaseOf("audio")).toBe("setup");
    expect(phaseOf("voice_tool")).toBe("setup");
    expect(phaseOf("voice_test")).toBe("try_it");
    expect(phaseOf("controls")).toBe("try_it");
    expect(phaseOf("complete")).toBe("try_it");
  });

  it("navigates forward and backward without auto-skipping", () => {
    expect(previousStep("welcome")).toBeNull();
    expect(nextStep("welcome")).toBe("remote");
    expect(previousStep("audio")).toBe("remote");
    expect(nextStep("controls")).toBe("complete");
    expect(nextStep("complete")).toBeNull();
    expect(previousStep("remote")).toBe("welcome");
  });

  it("normalizes unknown persisted steps to welcome (older/newer writers must not strand the flow)", () => {
    expect(normalizeStep("voice_test")).toBe("voice_test");
    expect(normalizeStep("legacy_step")).toBe("welcome");
    expect(normalizeStep("")).toBe("welcome");
    expect(normalizeStep(null)).toBe("welcome");
    expect(normalizeStep(42)).toBe("welcome");
  });
});

describe("onboarding gates", () => {
  const allBlocks: Array<[OnboardingStep, Partial<Parameters<typeof makeContext>[0]>, string]> = [
    ["remote", { remote: { connectionPhase: "connecting" } }, "remote.not_connected"],
    ["remote", { remote: { rawInputPhase: "starting" } }, "remote.listener_not_ready"],
    ["remote", { remote: { remoteButtonObserved: false } }, "remote.button_not_ready"],
    ["audio", { audio: { recommendedEndpointAvailable: false } }, "audio.install_required"],
    ["audio", { audio: { selectedEndpointRecommended: false } }, "audio.not_selected"],
    ["audio", { audio: { wasapiReady: false } }, "audio.not_ready"],
    ["voice_tool", { voiceTool: { tool: null } }, "tool.not_selected"],
    [
      "voice_tool",
      { voiceTool: { tool: "doubao", doubaoCaptureEnabled: false } },
      "tool.doubao.authorization_required",
    ],
    [
      "voice_tool",
      { voiceTool: { tool: "doubao", doubaoCaptureEnabled: true, vokieRunning: true } },
      "tool.conflict.vokie_running",
    ],
    [
      "voice_tool",
      { voiceTool: { tool: "vokie", vokieInstalled: false } },
      "tool.vokie.not_installed",
    ],
    [
      "voice_tool",
      { voiceTool: { tool: "vokie", vokieInstalled: true, vokieRunning: false } },
      "tool.vokie.not_running",
    ],
    [
      "voice_tool",
      { voiceTool: { tool: "other", otherHotkeyChosen: false } },
      "tool.hotkey_unset",
    ],
    ["voice_test", { voiceTest: { verified: false } }, "voice_test.not_verified"],
    ["controls", { controls: { distinctButtons: 2 } }, "controls.not_confirmed"],
  ];

  it.each(allBlocks)("blocks %s with stable code %s", (step, patch, code) => {
    const gate = evaluateGate(step, makeContext(patch));
    expect(gate.ok).toBe(false);
    expect(gate.code).toBe(code);
  });

  it("keeps every block code log-safe (lowercase token with dots/underscores only)", () => {
    for (const [, patch, code] of allBlocks) {
      expect(code).toMatch(/^[a-z0-9_.-]+$/);
    }
  });

  it("passes welcome unconditionally and passes every step with a satisfied context", () => {
    expect(evaluateGate("welcome", makeContext())).toEqual({ ok: true, code: null });
    for (const step of ONBOARDING_STEPS) {
      expect(evaluateGate(step, makeContext()).ok).toBe(true);
    }
  });

  it("treats wechat as immediately satisfied and doubao as needing both capture and no Vokie conflict", () => {
    expect(evaluateGate("voice_tool", makeContext({ voiceTool: { tool: "wechat" } }))).toEqual({
      ok: true,
      code: null,
    });
    expect(
      evaluateGate(
        "voice_tool",
        makeContext({ voiceTool: { tool: "doubao", doubaoCaptureEnabled: true, vokieRunning: false } }),
      ),
    ).toEqual({ ok: true, code: null });
  });

  it("checks conflict before authorization for doubao so the user fixes the tool clash first (D4)", () => {
    const gate = evaluateGate(
      "voice_tool",
      makeContext({
        voiceTool: { tool: "doubao", doubaoCaptureEnabled: false, vokieRunning: true },
      }),
    );
    expect(gate.code).toBe("tool.conflict.vokie_running");
  });

  it("reports the first unsatisfied runtime condition on complete (no flow reset)", () => {
    expect(evaluateGate("complete", makeContext()).ok).toBe(true);
    // 完成页只重查连接是否仍就绪，不要求重新按过按键。
    expect(
      evaluateGate("complete", makeContext({ remote: { remoteButtonObserved: false } })).ok,
    ).toBe(true);
    expect(evaluateGate("complete", makeContext({ remote: { connectionPhase: "reconnecting" } })).code).toBe(
      "remote.not_connected",
    );
    expect(evaluateGate("complete", makeContext({ audio: { wasapiReady: false } })).code).toBe(
      "audio.not_ready",
    );
    expect(
      evaluateGate("complete", makeContext({ voiceTool: { tool: "vokie", vokieInstalled: true, vokieRunning: false } }))
        .code,
    ).toBe("tool.vokie.not_running");
    // 第⑤步的真实验证是完成条件的一部分（会话级事实，未通过不得完成）。
    expect(evaluateGate("complete", makeContext({ voiceTest: { verified: false } })).code).toBe(
      "voice_test.not_verified",
    );
  });

  it("requires three distinct control buttons and does not treat voice sessions as control input", () => {
    expect(evaluateGate("controls", makeContext({ controls: { distinctButtons: 3 } })).ok).toBe(true);
    expect(evaluateGate("controls", makeContext({ controls: { distinctButtons: 0 } })).ok).toBe(false);
  });
});
