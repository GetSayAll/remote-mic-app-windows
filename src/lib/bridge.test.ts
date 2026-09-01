import { describe, expect, it } from "vitest";
import {
  audioPhaseLabel,
  connectionPhaseLabel,
  formatDiagnosticReport,
  formatUsageDuration,
  type AudioPhase,
  type ConnectionPhase,
  type DiagnosticReport,
} from "./bridge";

describe("connection phase presentation", () => {
  it("covers every serialized Rust connection phase", () => {
    const phases: ConnectionPhase[] = [
      "idle",
      "connecting",
      "discovering",
      "awaiting_capabilities",
      "ready",
      "streaming",
      "draining",
      "reconnecting",
      "suspended",
      "disconnected",
      "failed",
    ];

    expect(phases.map(connectionPhaseLabel)).toEqual([
      "尚未连接",
      "正在打开 RC003",
      "正在发现 ATVV 服务与特征",
      "正在确认 ATVV 能力",
      "BLE / ATVV 已就绪",
      "正在接收 RC003 语音",
      "正在结束本次语音",
      "正在等待重新连接 RC003",
      "Windows 已进入睡眠",
      "RC003 已断开",
      "连接失败",
    ]);
  });

  it("covers every serialized Rust audio phase", () => {
    const phases: AudioPhase[] = [
      "unconfigured",
      "ready",
      "streaming",
      "draining",
      "failed",
      "unsupported",
    ];

    expect(phases.map(audioPhaseLabel)).toEqual([
      "尚未选择输出端点",
      "WASAPI 已就绪",
      "正在写入 Windows 音频端点",
      "正在排空 Windows 音频缓冲",
      "WASAPI 输出失败",
      "当前环境不支持 WASAPI",
    ]);
  });
});

describe("diagnostic report presentation", () => {
  it("adds an explicit generation time without changing the captured report", () => {
    const report: DiagnosticReport = {
      schemaVersion: 1,
      appVersion: "0.1.0",
      platform: "windows",
      verificationStatus: "待真机验证",
      capabilities: {
        windowsApiAvailable: true,
        bleScanAvailable: true,
        bleVoiceReady: false,
        wasapiReady: false,
        rawInputReady: false,
        sendInputReady: true,
      },
      connection: {
        phase: "disconnected",
        capabilitiesConfirmed: false,
        sampleRate: null,
        frameSize: null,
        decodedSamples: 0,
        generation: 2,
        reconnectAttempt: 1,
        powerNotificationsAvailable: true,
        errorPresent: false,
      },
      audio: {
        phase: "unconfigured",
        endpointConfigured: false,
        queuedSamples: 0,
        submittedSamples: 0,
        generation: 0,
        errorPresent: false,
      },
      rawInput: {
        phase: "stopped",
        matchedDeviceCount: 0,
        rawEventCount: 0,
        semanticEdgeCount: 0,
        lastButton: null,
        lastIsPressed: null,
        errorPresent: false,
      },
      sendInput: {
        available: true,
        submittedBatches: 0,
        submittedEvents: 0,
        errorPresent: false,
      },
    };

    const formatted = JSON.parse(formatDiagnosticReport(report, "2026-09-01T00:00:00.000Z"));
    expect(formatted.generatedAt).toBe("2026-09-01T00:00:00.000Z");
    expect(formatted.connection.generation).toBe(2);
    expect(formatted).not.toHaveProperty("remoteName");
    expect(formatted.audio).not.toHaveProperty("selectedEndpointName");
  });
});

describe("usage statistics presentation", () => {
  it("formats seconds, minutes and hours without exposing fractional samples", () => {
    expect(formatUsageDuration(12.4)).toBe("12秒");
    expect(formatUsageDuration(125)).toBe("2分5秒");
    expect(formatUsageDuration(3_661)).toBe("1小时1分钟");
    expect(formatUsageDuration(Number.NaN)).toBe("0秒");
  });
});
