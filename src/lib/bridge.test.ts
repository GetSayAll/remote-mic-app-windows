import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  actionSummary,
  audioPhaseLabel,
  captureInputErrorLabel,
  chordLabel,
  connectionPhaseLabel,
  formatDiagnosticReport,
  GITHUB_REPOSITORY_URL,
  isRecommendedVoiceEndpoint,
  OFFICIAL_WEBSITE_URL,
  openGitHubRepository,
  openLogDirectory,
  openOfficialWebsite,
  openVbCableDownloadPage,
  remoteModelLabel,
  VB_CABLE_DOWNLOAD_URL,
  type AudioPhase,
  type ConnectionPhase,
  type DiagnosticReport,
} from "./bridge";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

describe("mouse actions", () => {
  it("summarizes clicks, movement, and wheel amounts", () => {
    expect(actionSummary({ type: "scroll", direction: "down", steps: 5 })).toBe("滚轮向下 5 格");
    expect(actionSummary({ type: "mouse_move", direction: "left", distance: 75 })).toBe("鼠标向左 75 px");
    expect(actionSummary({ type: "mouse_click", kind: "double_left" })).toBe("左键双击");
  });

  it("summarizes the focus-input action", () => {
    // 动作摘要会显示在按键格子上，文案即用户可见串（product-copy 口径）。
    expect(actionSummary({ type: "focus_input" })).toBe("聚焦输入框");
  });
});

describe("voice endpoint recommendation", () => {
  const candidate = (name: string, isVirtualCableCandidate = true) => ({
    id: name,
    name,
    isVirtualCableCandidate,
  });

  it("recommends only the standard VB-Audio CABLE Input", () => {
    expect(isRecommendedVoiceEndpoint(candidate("CABLE Input (VB-Audio Virtual Cable)"))).toBe(
      true,
    );
    // 后端的候选判定更宽（自动选择与安装检测用）：非标准端点不得进推荐位。
    expect(isRecommendedVoiceEndpoint(candidate("CABLE-A Input (VB-Audio Cable A)"))).toBe(false);
    expect(isRecommendedVoiceEndpoint(candidate("CABLE Input (CI Simulation)"))).toBe(false);
    expect(isRecommendedVoiceEndpoint(candidate("扬声器 (Realtek Audio)", false))).toBe(false);
  });

  it("uses the vendor key names for keys that were confusable in Chinese", () => {
    expect(chordLabel({ keys: ["backspace"] })).toBe("Backspace");
    expect(chordLabel({ keys: ["delete"] })).toBe("Delete");
    expect(chordLabel({ keys: ["control", "c"] })).toBe("Ctrl + C");
    expect(chordLabel({ keys: ["left_windows", "shift", "s"] })).toBe("左 Win + Shift + S");
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
    expect(remoteModelLabel("rc001")).toBe("小米蓝牙语音遥控器 2");
    expect(remoteModelLabel("rc003")).toBe("小米蓝牙语音遥控器 2 Pro");
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

describe("设置页外部入口（官网 / GitHub）", () => {
  afterEach(() => {
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.mocked(openUrl).mockReset();
  });

  it("指向已确认的官网与 Windows 仓库地址", () => {
    // 2026-10-02 用户指定：官网入口带 `?from=win` 来源标记，便于官网区分来客。
    expect(OFFICIAL_WEBSITE_URL).toBe("https://sayall.app/?from=win");
    expect(GITHUB_REPOSITORY_URL).toBe("https://github.com/GetSayAll/remote-mic-app-windows");
  });

  it("浏览器预览下用新标签打开，不调用 Tauri opener", async () => {
    const open = vi.spyOn(window, "open").mockImplementation(() => null);

    await openOfficialWebsite();
    await openGitHubRepository();

    expect(open).toHaveBeenNthCalledWith(1, OFFICIAL_WEBSITE_URL, "_blank", "noopener,noreferrer");
    expect(open).toHaveBeenNthCalledWith(2, GITHUB_REPOSITORY_URL, "_blank", "noopener,noreferrer");
    expect(openUrl).not.toHaveBeenCalled();
    open.mockRestore();
  });

  it("Tauri 运行时交给 opener 插件，地址必须与 capability 白名单逐字一致", async () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    vi.mocked(openUrl).mockResolvedValue(undefined);

    await openOfficialWebsite();
    await openGitHubRepository();

    // 传出的字符串与 src-tauri/capabilities/default.json 的 allow 列表是同一份
    // 契约：多一个字符、少一个尾斜杠都会被插件判为 ForbiddenUrl。
    expect(vi.mocked(openUrl).mock.calls.map(([url]) => url)).toEqual([
      OFFICIAL_WEBSITE_URL,
      GITHUB_REPOSITORY_URL,
    ]);
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

describe("microphone failure labels", () => {
  it("keeps external selections and tells how to handle pending recovery", () => {
    expect(captureInputErrorLabel("external_change")).toContain("保留现在的选择");
    expect(captureInputErrorLabel("journal_invalid")).toContain("选择恢复原设备或保留当前选择");
    expect(captureInputErrorLabel("restore_not_confirmed")).toContain("尚未确认");
  });
  it("offers a next step without exposing diagnostic fields or Windows error codes", () => {
    for (const code of ["capture_endpoint_missing", "normal_roles_split", "route_worker_busy", "route_worker_unavailable_hr_80004005", "unknown_internal_code"]) {
      const label = captureInputErrorLabel(code);
      expect(label).toMatch(/请/);
      expect(label).not.toContain(code);
      expect(label).not.toContain("80004005");
    }
    expect(captureInputErrorLabel("cancelled")).toContain("已取消");
    expect(captureInputErrorLabel(null)).toBe("");
  });
});
