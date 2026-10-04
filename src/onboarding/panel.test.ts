import { describe, expect, it } from "vitest";
import { buildSidePanel, type SidePanelContext } from "./panel";

function context(patch: Partial<SidePanelContext> = {}): SidePanelContext {
  return {
    connectionPhase: "idle",
    bleVoiceReady: false,
    remoteButtonObserved: false,
    recommendedEndpointCount: 0,
    selectedRecommended: false,
    wasapiReady: false,
    audioLastError: null,
    scanningAudio: false,
    tool: null,
    hotkeyReady: false,
    captureEnabled: false,
    vokieRunning: false,
    voicePhase: "waiting_start",
    voiceTerminalCode: null,
    voiceResultPassed: false,
    voiceEvidence: null,
    distinctButtons: 0,
    mappingSuspended: false,
    ...patch,
  };
}

describe("onboarding side panel", () => {
  it("welcome and complete use the visual-only panel", () => {
    expect(buildSidePanel("welcome", context())).toEqual({
      visual: "app",
      caption: "无线说话，也能随手控制",
    });
    expect(buildSidePanel("complete", context())).toEqual({
      visual: "done",
      caption: "开始使用无线麦",
    });
  });

  it("remote rows follow connection and button observation", () => {
    const idle = buildSidePanel("remote", context());
    expect(idle.cardTitle).toBe("连接检查");
    expect(idle.rows?.map((row) => row.state)).toEqual(["pending", "pending"]);

    const connecting = buildSidePanel(
      "remote",
      context({ connectionPhase: "connecting" }),
    );
    expect(connecting.rows?.[0].state).toBe("busy");

    const ready = buildSidePanel(
      "remote",
      context({ connectionPhase: "ready", bleVoiceReady: true }),
    );
    expect(ready.rows?.[0].state).toBe("ok");

    const observed = buildSidePanel(
      "remote",
      context({ connectionPhase: "ready", bleVoiceReady: true, remoteButtonObserved: true }),
    );
    expect(observed.rows?.map((row) => row.state)).toEqual(["ok", "ok"]);
  });

  it("audio rows layer detection, selection and readiness", () => {
    const panel = buildSidePanel(
      "audio",
      context({
        recommendedEndpointCount: 2,
        selectedRecommended: true,
        wasapiReady: true,
      }),
    );
    expect(panel.rows?.map((row) => row.state)).toEqual(["ok", "ok", "ok"]);

    const missing = buildSidePanel("audio", context());
    expect(missing.rows?.[0].state).toBe("warn");

    const notReady = buildSidePanel(
      "audio",
      context({
        recommendedEndpointCount: 1,
        selectedRecommended: true,
        wasapiReady: false,
        scanningAudio: true,
      }),
    );
    expect(notReady.rows?.[2].state).toBe("busy");

    const errored = buildSidePanel(
      "audio",
      context({
        recommendedEndpointCount: 1,
        selectedRecommended: true,
        audioLastError: "设备被占用",
      }),
    );
    expect(errored.rows?.[2].state).toBe("warn");
  });

  it("voice tool rows follow the staged tool", () => {
    const none = buildSidePanel("voice_tool", context());
    expect(none.rows?.map((row) => row.label)).toEqual([
      "已选择输入工具",
      "语音键已适配",
      "等待选择输入工具",
    ]);

    const doubaoBlocked = buildSidePanel(
      "voice_tool",
      context({ tool: "doubao", hotkeyReady: true }),
    );
    expect(doubaoBlocked.rows?.[2]).toEqual({
      label: "「支持更多输入工具」已开启",
      state: "warn",
    });

    const doubaoReady = buildSidePanel(
      "voice_tool",
      context({ tool: "doubao", hotkeyReady: true, captureEnabled: true }),
    );
    expect(doubaoReady.rows?.map((row) => row.state)).toEqual(["ok", "ok", "ok"]);

    const wechat = buildSidePanel(
      "voice_tool",
      context({ tool: "wechat", hotkeyReady: true }),
    );
    expect(wechat.rows?.[2]).toEqual({ label: "无需额外设置", state: "ok" });

    const vokie = buildSidePanel(
      "voice_tool",
      context({ tool: "vokie", hotkeyReady: true, vokieRunning: true }),
    );
    expect(vokie.rows?.[2]).toEqual({ label: "Vokie 已运行", state: "ok" });
  });

  it("voice test rows mirror the attempt phases and terminal evidence", () => {
    const waiting = buildSidePanel("voice_test", context());
    expect(waiting.rows?.map((row) => row.state)).toEqual([
      "pending",
      "pending",
      "pending",
      "pending",
    ]);

    const streaming = buildSidePanel("voice_test", context({ voicePhase: "streaming" }));
    expect(streaming.rows?.map((row) => row.state)).toEqual([
      "ok",
      "busy",
      "busy",
      "pending",
    ]);

    const passed = buildSidePanel(
      "voice_test",
      context({
        voicePhase: "terminal",
        voiceResultPassed: true,
        voiceEvidence: { decodedSamples: 160, submittedSamples: 640, drainObserved: true },
      }),
    );
    expect(passed.rows?.map((row) => row.state)).toEqual(["ok", "ok", "ok", "ok"]);

    const noSession = buildSidePanel(
      "voice_test",
      context({ voicePhase: "terminal", voiceTerminalCode: "voice.session_not_started" }),
    );
    expect(noSession.rows?.[0].state).toBe("warn");

    const noSamples = buildSidePanel(
      "voice_test",
      context({ voicePhase: "terminal", voiceTerminalCode: "voice.no_samples" }),
    );
    expect(noSamples.rows?.[1].state).toBe("warn");

    const delivery = buildSidePanel(
      "voice_test",
      context({
        voicePhase: "terminal",
        voiceTerminalCode: "voice.audio_delivery_failed",
      }),
    );
    expect(delivery.rows?.[2].state).toBe("warn");

    const noTranscript = buildSidePanel(
      "voice_test",
      context({ voicePhase: "terminal", voiceTerminalCode: "voice.no_transcript" }),
    );
    expect(noTranscript.rows?.[3].state).toBe("warn");
  });

  it("controls rows carry the counter and the mapping suspension", () => {
    const two = buildSidePanel("controls", context({ distinctButtons: 2, mappingSuspended: true }));
    expect(two.rows?.[0]).toEqual({
      label: "已按 3 个不同的普通按键",
      state: "busy",
      value: "2 / 3",
    });
    expect(two.rows?.[1]?.state).toBe("ok");

    const three = buildSidePanel("controls", context({ distinctButtons: 3 }));
    expect(three.rows?.[0]?.state).toBe("ok");
    expect(three.rows?.[1]?.state).toBe("pending");
  });
});
