import { invoke } from "@tauri-apps/api/core";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  actionSummary,
  audioPhaseLabel,
  connectionPhaseLabel,
  formatDiagnosticReport,
  identityShortcutByButton,
  openLogDirectory,
  openVbCableDownloadPage,
  remoteModelLabel,
  shortcutCapability,
  VB_CABLE_DOWNLOAD_URL,
  type AudioPhase,
  type ConnectionPhase,
  type DiagnosticReport,
} from "./bridge";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("mouse actions", () => {
  it("summarizes clicks, movement, and wheel amounts", () => {
    expect(actionSummary({ type: "scroll", direction: "down", steps: 5 })).toBe("滚轮向下 5 格");
    expect(actionSummary({ type: "mouse_move", direction: "left", distance: 75 })).toBe("鼠标向左 75 px");
    expect(actionSummary({ type: "mouse_click", kind: "double_left" })).toBe("左键双击");
  });
});

describe("mapping capability matrix（单响应判定，用于信息提示）", () => {
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

  it("TV 与返回/音量±全型号判定为不可配（2026-09-07 用户决策：两型号行为一致）", () => {
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

  it("展示 RC001、RC003、Chromecast 和未知型号", () => {
    expect(remoteModelLabel("rc001")).toBe("小米蓝牙遥控器 2");
    expect(remoteModelLabel("rc003")).toBe("小米蓝牙遥控器 2 Pro");
    expect(remoteModelLabel("chromecast")).toBe("Chromecast Remote（谷歌）");
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

describe("诊断日志目录入口", () => {
  afterEach(() => {
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.mocked(invoke).mockReset();
  });

  it("在 Tauri 运行时按精确命令名交给 Rust，并原样回传目录", async () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const directory = "C:\\Users\\probe\\AppData\\Local\\SayAll\\Logs";
    vi.mocked(invoke).mockResolvedValue(directory);

    await expect(openLogDirectory()).resolves.toBe(directory);
    // 命令名写错或 Rust 侧漏注册时这里会红——这是该入口唯一的前端契约。
    // 前端不拼接、不传路径参数：目录由 Rust 从日志初始化结果推导。
    expect(invoke).toHaveBeenCalledWith("open_log_directory");
  });

  it("浏览器预览下明确不可用而不是静默失败", async () => {
    await expect(openLogDirectory()).rejects.toThrow("当前是浏览器预览，无法打开日志目录");
    expect(invoke).not.toHaveBeenCalled();
  });
});
