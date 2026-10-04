import { describe, expect, it } from "vitest";
import {
  createVoiceAttemptTracker,
  type VoiceAttemptInput,
  type VoiceAttemptTerminal,
} from "./voice-attempt";

function frame(patch: Partial<VoiceAttemptInput> = {}): VoiceAttemptInput {
  return {
    voiceState: "idle",
    decodedSamples: 0,
    audioPhase: "ready",
    submittedSamples: 0,
    queuedSamples: 0,
    audioLastError: null,
    generation: 1,
    ...patch,
  };
}

function tracker(attemptId = 1) {
  return createVoiceAttemptTracker({ attemptId });
}

describe("voice attempt tracker", () => {
  it("passes a full session with delivery evidence and transcript in window", () => {
    const attempt = tracker();
    attempt.arm(frame({ decodedSamples: 100 }), 0);

    expect(
      attempt.observe(
        frame({ voiceState: "streaming", audioPhase: "streaming", decodedSamples: 140, submittedSamples: 300 }),
        200,
      ),
    ).toBeNull();
    expect(
      attempt.observe(
        frame({ voiceState: "draining", audioPhase: "draining", decodedSamples: 160, submittedSamples: 640, queuedSamples: 40 }),
        400,
      ),
    ).toBeNull();
    expect(
      attempt.observe(frame({ decodedSamples: 168, submittedSamples: 640, queuedSamples: 0 }), 700),
    ).toBeNull();
    expect(attempt.state()).toBe("waiting_transcript");

    const terminal = attempt.notifyTranscript(900);
    expect(terminal).toEqual({
      attemptId: 1,
      result: "passed",
      code: null,
      evidence: {
        generation: 1,
        startObserved: true,
        decodedSamples: 68,
        submittedSamples: 640,
        queuedEnd: 0,
        drainObserved: true,
      },
      elapsedMs: 900,
    } satisfies VoiceAttemptTerminal);
  });

  it("classifies a session with no samples", () => {
    const attempt = tracker();
    attempt.arm(frame({ decodedSamples: 100 }), 0);
    attempt.observe(frame({ voiceState: "streaming", audioPhase: "streaming", decodedSamples: 100 }), 200);
    const terminal = attempt.observe(frame({ decodedSamples: 100, submittedSamples: 0, queuedSamples: 0 }), 400);
    expect(terminal?.result).toBe("failed");
    expect(terminal?.code).toBe("voice.no_samples");
  });

  it("classifies decoded-but-not-delivered as a delivery failure", () => {
    const attempt = tracker();
    attempt.arm(frame({ decodedSamples: 100 }), 0);
    attempt.observe(frame({ voiceState: "streaming", audioPhase: "streaming", decodedSamples: 150 }), 200);
    const terminal = attempt.observe(frame({ decodedSamples: 160, submittedSamples: 0, queuedSamples: 0 }), 400);
    expect(terminal?.result).toBe("failed");
    expect(terminal?.code).toBe("voice.audio_delivery_failed");
    expect(terminal?.evidence.decodedSamples).toBe(60);
    expect(terminal?.evidence.submittedSamples).toBe(0);
  });

  it("classifies an audio error at the end as a delivery failure even with text pending", () => {
    const attempt = tracker();
    attempt.arm(frame({ decodedSamples: 100 }), 0);
    attempt.observe(frame({ voiceState: "streaming", audioPhase: "streaming", decodedSamples: 150 }), 200);
    const terminal = attempt.observe(
      frame({ decodedSamples: 150, submittedSamples: 10, audioPhase: "failed", audioLastError: "interrupted" }),
      400,
    );
    expect(terminal?.code).toBe("voice.audio_delivery_failed");
  });

  it("fails with session_not_ended when draining never settles", () => {
    const attempt = tracker();
    attempt.arm(frame(), 0);
    attempt.observe(frame({ voiceState: "streaming", audioPhase: "streaming", decodedSamples: 100 }), 200);
    expect(
      attempt.observe(frame({ voiceState: "draining", audioPhase: "draining", decodedSamples: 150 }), 400),
    ).toBeNull();
    expect(
      attempt.observe(frame({ voiceState: "draining", audioPhase: "draining", decodedSamples: 160 }), 8_000),
    ).toBeNull();
    const terminal = attempt.observe(
      frame({ voiceState: "draining", audioPhase: "draining", decodedSamples: 160 }),
      8_600,
    );
    expect(terminal?.code).toBe("voice.session_not_ended");
  });

  it("fails with no_transcript when the 3s window expires", () => {
    const attempt = tracker();
    attempt.arm(frame(), 0);
    attempt.observe(frame({ voiceState: "streaming", audioPhase: "streaming", decodedSamples: 120, submittedSamples: 200 }), 200);
    attempt.observe(frame({ decodedSamples: 130, submittedSamples: 400, queuedSamples: 0 }), 700);
    expect(attempt.observe(frame({ decodedSamples: 130, submittedSamples: 400 }), 3_000)).toBeNull();
    const terminal = attempt.observe(frame({ decodedSamples: 130, submittedSamples: 400 }), 3_800);
    expect(terminal?.code).toBe("voice.no_transcript");
    expect(terminal?.evidence.drainObserved).toBe(true);
  });

  it("fails on manual keyboard input and never rewrites the terminal", () => {
    const attempt = tracker();
    attempt.arm(frame(), 0);
    attempt.observe(frame({ voiceState: "streaming", audioPhase: "streaming", decodedSamples: 120, submittedSamples: 200 }), 200);
    attempt.observe(frame({ decodedSamples: 130, submittedSamples: 400, queuedSamples: 0 }), 700);

    const terminal = attempt.notifyManualInput(900);
    expect(terminal?.code).toBe("voice.manual_input");
    // 迟到转写与迟到帧都不能改写终态。
    expect(attempt.notifyTranscript(1_000)).toBeNull();
    expect(attempt.observe(frame({ decodedSamples: 200, submittedSamples: 900 }), 1_100)).toBeNull();
  });

  it("accepts a transcript that arrives before the session-end frame", () => {
    const attempt = tracker();
    attempt.arm(frame(), 0);
    attempt.observe(frame({ voiceState: "streaming", audioPhase: "streaming", decodedSamples: 150, submittedSamples: 300 }), 200);
    expect(attempt.notifyTranscript(800)).toBeNull();
    const terminal = attempt.observe(frame({ decodedSamples: 160, submittedSamples: 640, queuedSamples: 0 }), 900);
    expect(terminal?.result).toBe("passed");
    expect(terminal?.elapsedMs).toBe(900);
  });

  it("classifies a session that fell entirely between two frames without pretending it observed the start", () => {
    const attempt = tracker();
    attempt.arm(frame({ decodedSamples: 100 }), 0);
    const terminal = attempt.observe(
      frame({ decodedSamples: 120, submittedSamples: 200, queuedSamples: 0 }),
      300,
    );
    expect(terminal).toBeNull();
    expect(attempt.state()).toBe("waiting_transcript");
    const passed = attempt.notifyTranscript(400);
    expect(passed?.result).toBe("passed");
    expect(passed?.evidence.startObserved).toBe(false);
    expect(passed?.evidence.decodedSamples).toBe(20);
  });

  it("supports an explicit abort with a known cause and ignores it after terminal", () => {
    const attempt = tracker(7);
    attempt.arm(frame(), 0);
    const terminal = attempt.abort("voice.session_not_started", 500);
    expect(terminal?.attemptId).toBe(7);
    expect(terminal?.code).toBe("voice.session_not_started");
    expect(attempt.abort("voice.no_samples", 600)).toBeNull();
  });
});
