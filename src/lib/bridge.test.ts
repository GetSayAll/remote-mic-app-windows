import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  actionSummary,
  audioPhaseLabel,
  chordLabel,
  connectionPhaseLabel,
  formatDiagnosticReport,
  GITHUB_REPOSITORY_URL,
  getOpenWindowAtLaunch,
  identityShortcutByButton,
  isRecommendedVoiceEndpoint,
  latestMacroDefinitions,
  OFFICIAL_WEBSITE_URL,
  openGitHubRepository,
  openLogDirectory,
  openOfficialWebsite,
  openRecommendationsWebsite,
  openVbCableDownloadPage,
  registerCommonPhraseAvailability,
  RECOMMENDATIONS_URL,
  remoteModelLabel,
  setOpenWindowAtLaunch,
  shortcutCapability,
  showRecommendationsPage,
  hideRecommendationsPage,
  takeUpdateCompletedVersion,
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

describe("common phrases action（公开构建兼容与可用性）", () => {
  it("summarizes the open-common-phrases action with the component availability", () => {
    // 缺私有组件（公开构建默认）：已保存的绑定不静默替换，标为「当前不可用」。
    registerCommonPhraseAvailability(false);
    expect(actionSummary({ type: "open_common_phrases" })).toBe("打开常用语（当前不可用）");
    registerCommonPhraseAvailability(true);
    expect(actionSummary({ type: "open_common_phrases" })).toBe("打开常用语");
    // 复原全局状态，避免污染其他用例（注册是模块级缓存，同预设应用名）。
    registerCommonPhraseAvailability(false);
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

  it("TV 仍无同键能力，但返回/音量±已开放自定义", () => {
    expect(shortcutCapability("tv", "single", "rc003")).toBe("none");
    expect(shortcutCapability("tv", "long", "rc001")).toBe("none");
    for (const button of ["back", "volume_up", "volume_down"] as const) {
      expect(shortcutCapability(button, "single", "rc003")).toBe("all");
      expect(shortcutCapability(button, "double", "rc001")).toBe("all");
      expect(shortcutCapability(button, "long", "unknown")).toBe("all");
    }
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
      schemaVersion: 4,
      appVersion: "0.1.0",
      appBuild: "1001",
      sourceRevision: "0123456789012345678901234567890123456789",
      buildChannel: "local-test",
      windowsVersion: "10.0.26100",
      processArchitecture: "x86_64",
      bootType: "full",
      fastStartupEnabled: true,
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
  ime: {
    detected: false,
    activeName: null,
    installedNames: [],
    expectedName: null,
    matchesExpected: null,
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

describe("推荐页入口（官网页面宿主）", () => {
  afterEach(() => {
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.mocked(invoke).mockReset();
    vi.mocked(openUrl).mockReset();
  });

  it("只指向官网推荐页，不在客户端保存推荐内容", () => {
    expect(RECOMMENDATIONS_URL).toBe("https://sayall.app/recommendations/");
  });

  it("浏览器预览下明确不可用，并可直接打开官网", async () => {
    const open = vi.spyOn(window, "open").mockImplementation(() => null);

    await expect(showRecommendationsPage("light")).rejects.toThrow(
      "当前是浏览器预览，无法在应用内打开推荐页面",
    );
    await openRecommendationsWebsite();

    expect(open).toHaveBeenCalledWith(
      RECOMMENDATIONS_URL,
      "_blank",
      "noopener,noreferrer",
    );
    expect(invoke).not.toHaveBeenCalled();
    open.mockRestore();
  });

  it("Tauri 运行时按精确命令名调用 Rust，并回传本次加载 generation", async () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    vi.mocked(invoke)
      .mockResolvedValueOnce(7)
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce(undefined);

    await expect(showRecommendationsPage("dark")).resolves.toBe(7);
    await hideRecommendationsPage();
    await openRecommendationsWebsite();

    expect(vi.mocked(invoke).mock.calls).toEqual([
      ["show_recommendations", { theme: "dark" }],
      ["hide_recommendations"],
      ["open_recommendations_website"],
    ]);
  });
});

describe("组合动作：同一条动作只保留最新版本（2026-10-15 现场回归）", () => {
  const make = (macroId: string, version: string, name: string) => ({
    schemaVersion: "0.1",
    macroId,
    version,
    name,
    steps: [
      {
        stepId: "step.01",
        action: "open_application" as const,
        parameters: { target: "notepad" },
      },
    ],
  });

  it("同 id 多版本只保留最新，顺序按首次出现（编辑后不跳位）", () => {
    const grouped = latestMacroDefinitions([
      make("macro.aaaaaaa1", "1.0.0", "旧名字"),
      make("macro.aaaaaaa1", "1.0.1", "新名字"),
      make("macro.bbbbbbb2", "1.0.0", "另一个"),
    ]);
    expect(grouped).toHaveLength(2);
    expect(grouped[0].name).toBe("新名字");
    expect(grouped[0].version).toBe("1.0.1");
    expect(grouped[1].macroId).toBe("macro.bbbbbbb2");
  });

  it("新版本在前时同样只留最新", () => {
    const grouped = latestMacroDefinitions([
      make("macro.aaaaaaa1", "1.0.2", "最新"),
      make("macro.aaaaaaa1", "1.0.0", "旧"),
    ]);
    expect(grouped).toHaveLength(1);
    expect(grouped[0].version).toBe("1.0.2");
  });

  it("空库与单条原样返回", () => {
    expect(latestMacroDefinitions([])).toEqual([]);
    const single = [make("macro.aaaaaaa1", "1.0.0", "唯一")];
    expect(latestMacroDefinitions(single)).toEqual(single);
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

describe("启动时自动打开主面板（设置页「启动行为」）", () => {
  afterEach(() => {
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.mocked(invoke).mockReset();
  });

  it("浏览器预览按新装默认返回开启，不调用 IPC", async () => {
    await expect(getOpenWindowAtLaunch()).resolves.toBe(true);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("Tauri 运行时使用精确命令名与参数名", async () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    vi.mocked(invoke).mockResolvedValue(false);

    await expect(getOpenWindowAtLaunch()).resolves.toBe(false);
    expect(invoke).toHaveBeenCalledWith("get_open_window_at_launch");

    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockResolvedValue(false);
    await expect(setOpenWindowAtLaunch(false)).resolves.toBe(false);
    // 参数名必须与 Rust `set_open_window_at_launch(enabled: bool)` 一致；
    // 不匹配时 Tauri 只会报缺参，界面只能看到一句 IPC 失败。
    expect(invoke).toHaveBeenCalledWith("set_open_window_at_launch", { enabled: false });
  });

  it("浏览器预览下保存明确不可用", async () => {
    await expect(setOpenWindowAtLaunch(false)).rejects.toThrow(
      "当前是浏览器预览，无法设置启动时打开主面板",
    );
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("更新完成提示（对齐 Mac `update.completed.*`）", () => {
  afterEach(() => {
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.mocked(invoke).mockReset();
  });

  it("浏览器预览没有更新机制，返回 null 且不调 IPC", async () => {
    await expect(takeUpdateCompletedVersion()).resolves.toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("Tauri 运行时使用精确命令名", async () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    vi.mocked(invoke).mockResolvedValue("1.0.0");

    await expect(takeUpdateCompletedVersion()).resolves.toBe("1.0.0");
    expect(invoke).toHaveBeenCalledWith("take_update_completed_version");
  });
});
