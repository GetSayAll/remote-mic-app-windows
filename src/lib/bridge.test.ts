import { describe, expect, it, vi } from "vitest";
import {
  actionSummary,
  audioPhaseLabel,
  connectionPhaseLabel,
  formatDiagnosticReport,
  filterButtonReady,
  userHidButtonReady,
  identityShortcutByButton,
  openVbCableDownloadPage,
  remoteModelLabel,
  shortcutCapability,
  VB_CABLE_DOWNLOAD_URL,
  type AudioPhase,
  type ConnectionPhase,
  type DiagnosticReport,
  type RawInputSnapshot,
} from "./bridge";

describe("experimental user-mode capture capability", () => {
  it("keeps helper confirmation separate from driver confirmation and fails closed", () => {
    const raw: RawInputSnapshot = {
      phase: "ready", matchedDeviceCount: 1, rawEventCount: 0, semanticEdgeCount: 0,
      lastButton: null, lastIsPressed: null, activeButtons: [], lastError: null,
      confirmedUserHidButtons: ["back"],
    };
    expect(userHidButtonReady("back", "rc003", raw)).toBe(true);
    expect(filterButtonReady("back", "rc003", raw)).toBe(false);
    expect(shortcutCapability("back", "double", "rc003", raw)).toBe("all");
    expect(userHidButtonReady("volume_up", "rc003", raw)).toBe(false);
    expect(userHidButtonReady("back", "rc001", raw)).toBe(false);
    expect(userHidButtonReady("back", "unknown", raw)).toBe(false);
    expect(userHidButtonReady("back", "rc003", undefined)).toBe(false);
    expect(userHidButtonReady("back", "rc003", { ...raw, confirmedUserHidButtons: undefined })).toBe(false);
    expect(userHidButtonReady("back", "rc003", { ...raw, matchedDeviceCount: 2 })).toBe(false);
    expect(userHidButtonReady("back", "rc003", { ...raw, phase: "stopped" })).toBe(false);
  });
});

describe("mouse wheel actions", () => {
  it("shows mouse action types and per-binding amounts", () => {
    expect(actionSummary({ type: "scroll", direction: "down", steps: 5 })).toBe("滚轮向下 5 格");
    expect(actionSummary({ type: "mouse_move", direction: "left", distance: 75 })).toBe("鼠标向左 75 px");
    expect(actionSummary({ type: "mouse_click", kind: "double_left" })).toBe("左键双击");
    expect(actionSummary({ type: "mouse_click", kind: "right" })).toBe("右键单击");
  });
  it("labels both wheel directions independently from keyboard arrows", () => {
    expect(actionSummary({ type: "scroll", direction: "up" })).toBe("滚轮向上");
    expect(actionSummary({ type: "scroll", direction: "down" })).toBe("滚轮向下");
  });
});

describe("mapping capability matrix（单响应判定，用于信息提示）", () => {
  it("requires a current per-key RC003 confirmation, including old-host fallback", () => {
    const raw: RawInputSnapshot = {
      phase: "ready", matchedDeviceCount: 1, rawEventCount: 2, semanticEdgeCount: 0,
      lastButton: null, lastIsPressed: null, activeButtons: [], lastError: null,
      confirmedFilterButtons: ["volume_up"],
    };
    expect(shortcutCapability("volume_up", "long", "rc003", raw)).toBe("all");
    expect(filterButtonReady("volume_down", "rc003", raw)).toBe(false);
    expect(filterButtonReady("volume_up", "rc001", raw)).toBe(false);
    expect(filterButtonReady("volume_up", "unknown", raw)).toBe(false);
    expect(filterButtonReady("volume_up", "rc003", undefined)).toBe(false);
    expect(filterButtonReady("volume_up", "rc003", { ...raw, confirmedFilterButtons: undefined })).toBe(false);
    expect(filterButtonReady("volume_up", "rc003", { ...raw, matchedDeviceCount: 2 })).toBe(false);
    for (const phase of ["failed", "stopped", "starting", "unsupported"] as const) {
      expect(filterButtonReady("volume_up", "rc003", { ...raw, phase })).toBe(false);
    }
  });
  it("直接归因族（电源/菜单）全部触发单响应", () => {
    expect(shortcutCapability("power", "long", "rc003")).toBe("all");
    expect(shortcutCapability("power", "single", "rc003")).toBe("all");
    expect(shortcutCapability("menu", "double", "rc003")).toBe("all");
  });

  it("武装族（确定/方向/主页）单击可同键对冲，双击/长按判定为附带原生动作", () => {
    expect(shortcutCapability("ok", "single", "rc003")).toBe("identity");
    expect(shortcutCapability("ok", "double", "rc003")).toBe("none");
    expect(shortcutCapability("ok", "long", "rc003")).toBe("none");
    expect(shortcutCapability("up", "single", "rc003")).toBe("identity");
    expect(shortcutCapability("down", "single", "rc001")).toBe("identity");
    expect(shortcutCapability("left", "single", "rc003")).toBe("identity");
    expect(shortcutCapability("left", "double", "rc001")).toBe("none");
    expect(shortcutCapability("right", "long", "rc001")).toBe("none");
    expect(shortcutCapability("home", "single", "rc003")).toBe("identity");
  });

  it("keeps TV and unconfirmed filter buttons unavailable", () => {
    expect(shortcutCapability("tv", "single", "rc003")).toBe("none");
    expect(shortcutCapability("tv", "long", "rc001")).toBe("none");
    expect(shortcutCapability("back", "single", "rc003")).toBe("none");
    expect(shortcutCapability("volume_up", "single", "rc003")).toBe("none");
    expect(shortcutCapability("volume_down", "single", "unknown")).toBe("none");
    expect(shortcutCapability("back", "single", "unknown")).toBe("none");
    expect(shortcutCapability("back", "single", "rc001")).toBe("none");
    expect(shortcutCapability("volume_up", "long", "rc001")).toBe("none");
    expect(shortcutCapability("volume_down", "double", "rc001")).toBe("none");
  });

  it("identityShortcutByButton 对齐 Rust native_key（泄漏对冲判定依据）", () => {
    expect(identityShortcutByButton.ok).toBe("enter");
    expect(identityShortcutByButton.up).toBe("up");
    expect(identityShortcutByButton.down).toBe("down");
    expect(identityShortcutByButton.left).toBe("left");
    expect(identityShortcutByButton.right).toBe("right");
    expect(identityShortcutByButton.home).toBe("home");
    expect(identityShortcutByButton.tv).toBeUndefined();
    expect(identityShortcutByButton.power).toBeUndefined();
  });
});

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
