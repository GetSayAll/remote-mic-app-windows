import { describe, expect, it, vi } from "vitest";
import {
  audioPhaseLabel,
  connectionPhaseLabel,
  formatDiagnosticReport,
  openVbCableDownloadPage,
  remoteModelLabel,
  VB_CABLE_DOWNLOAD_URL,
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
      "正在连接遥控器",
      "正在连接遥控器",
      "正在确认语音功能",
      "已连接",
      "正在接收语音",
      "正在结束本次语音",
      "正在等待遥控器重连",
      "电脑已进入睡眠",
      "遥控器已断开",
      "连接失败",
    ]);
  });

  it("展示 RC001、RC003 和未知型号", () => {
    expect(remoteModelLabel("rc001")).toBe("小米蓝牙遥控器 2");
    expect(remoteModelLabel("rc003")).toBe("小米蓝牙遥控器 2 Pro");
    expect(remoteModelLabel("unknown")).toBe("连接后显示");
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
      "尚未选择设备",
      "已就绪",
      "正在写入语音",
      "正在结束",
      "语音设备出错",
      "当前环境不支持语音设备",
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
      buttonMapping: {
        enabled: true,
        gateActive: false,
        listenerActive: false,
        swallowedEdges: 0,
        leakedDowns: 0,
        firedGestures: 0,
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

describe("VB-CABLE download guidance", () => {
  it("opens only the official VB-Audio page in browser preview", async () => {
    const open = vi.spyOn(window, "open").mockImplementation(() => null);

    await openVbCableDownloadPage();

    expect(open).toHaveBeenCalledWith(VB_CABLE_DOWNLOAD_URL, "_blank", "noopener,noreferrer");
    open.mockRestore();
  });
});
