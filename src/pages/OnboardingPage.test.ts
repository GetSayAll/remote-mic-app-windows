import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AudioEndpoint,
  AudioPhase,
  AudioSnapshot,
  ConnectionPhase,
  DiagnosticReport,
  RawInputPhase,
  RuntimeSnapshot,
  VoiceSessionState,
} from "../lib/bridge";
import OnboardingPage from "./OnboardingPage.vue";

const mocks = vi.hoisted(() => ({
  reportFrontendEvent: vi.fn(),
  getOnboardingState: vi.fn(),
  saveOnboardingStep: vi.fn(),
  scanPairedRemotes: vi.fn(),
  connectRemote: vi.fn(),
  subscribeButtonEdges: vi.fn(),
  listAudioEndpoints: vi.fn(),
  getAudioSnapshot: vi.fn(),
  selectAudioEndpoint: vi.fn(),
  openVbCableDownloadPage: vi.fn(),
  openWindowsSettings: vi.fn(),
  getVoiceInputTool: vi.fn(),
  getVoiceHoldHotkey: vi.fn(),
  getOtherVoiceHotkey: vi.fn(),
  setOtherVoiceHotkey: vi.fn(),
  getRc003TaskStatus: vi.fn(),
  enableRc003Capture: vi.fn(),
  disableRc003Capture: vi.fn(),
  getVokieInstallation: vi.fn(),
  openVokieHomepage: vi.fn(),
  launchVokie: vi.fn(),
  stageOnboardingVoiceBinding: vi.fn(),
  setMappingSuspension: vi.fn(),
  completeOnboarding: vi.fn(),
  getRuntimeSnapshot: vi.fn(),
  getDiagnosticReport: vi.fn(),
  beginKeyObservation: vi.fn(),
  endKeyObservation: vi.fn(),
  rc003Disabled: {
    installed: false,
    authorizationRequired: true,
    enabled: false,
    helperPath: null as string | null,
    lastError: null as string | null,
  },
  buttonHandler: null as ((edge: { button: string; isPressed: boolean }) => void) | null,
}));

vi.mock("../lib/frontend-diagnostics", async (importOriginal) => {
  const original = await importOriginal<typeof import("../lib/frontend-diagnostics")>();
  return { ...original, reportFrontendEvent: mocks.reportFrontendEvent };
});

vi.mock("../lib/bridge", async (importOriginal) => {
  const original = await importOriginal<typeof import("../lib/bridge")>();
  return {
    ...original,
    getOnboardingState: mocks.getOnboardingState,
    saveOnboardingStep: mocks.saveOnboardingStep,
    scanPairedRemotes: mocks.scanPairedRemotes,
    connectRemote: mocks.connectRemote,
    subscribeButtonEdges: mocks.subscribeButtonEdges,
    listAudioEndpoints: mocks.listAudioEndpoints,
    getAudioSnapshot: mocks.getAudioSnapshot,
    selectAudioEndpoint: mocks.selectAudioEndpoint,
    openVbCableDownloadPage: mocks.openVbCableDownloadPage,
    openWindowsSettings: mocks.openWindowsSettings,
    getVoiceInputTool: mocks.getVoiceInputTool,
    getVoiceHoldHotkey: mocks.getVoiceHoldHotkey,
    getOtherVoiceHotkey: mocks.getOtherVoiceHotkey,
    setOtherVoiceHotkey: mocks.setOtherVoiceHotkey,
    getRc003TaskStatus: mocks.getRc003TaskStatus,
    enableRc003Capture: mocks.enableRc003Capture,
    disableRc003Capture: mocks.disableRc003Capture,
    getVokieInstallation: mocks.getVokieInstallation,
    openVokieHomepage: mocks.openVokieHomepage,
    launchVokie: mocks.launchVokie,
    stageOnboardingVoiceBinding: mocks.stageOnboardingVoiceBinding,
    setMappingSuspension: mocks.setMappingSuspension,
    completeOnboarding: mocks.completeOnboarding,
    getRuntimeSnapshot: mocks.getRuntimeSnapshot,
    getDiagnosticReport: mocks.getDiagnosticReport,
    beginKeyObservation: mocks.beginKeyObservation,
    endKeyObservation: mocks.endKeyObservation,
  };
});

const activeState = (step: string) => ({
  flowVersion: 1,
  completedVersion: 0,
  step,
  isActive: true,
});

function runtimeWith(
  patch: {
    connectionPhase?: ConnectionPhase;
    bleVoiceReady?: boolean;
    rawInputPhase?: RawInputPhase;
    voiceState?: VoiceSessionState;
    reconnectAttempt?: number;
    audioPhase?: AudioPhase;
    decodedSamples?: number;
    submittedSamples?: number;
    queuedSamples?: number;
    audioLastError?: string | null;
    connectionGeneration?: number;
  } = {},
): RuntimeSnapshot {
  return {
    appVersion: "0.6.0",
    platform: {
      platform: "windows",
      windowsApiAvailable: true,
      bleScanAvailable: true,
      bleVoiceReady: patch.bleVoiceReady ?? false,
      wasapiReady: true,
      rawInputReady: patch.rawInputPhase === "ready",
      sendInputReady: true,
      verificationStatus: "测试",
      connection: {
        phase: patch.connectionPhase ?? "idle",
        remoteName: null,
        remoteModel: "unknown",
        capabilities: null,
        voiceState: patch.voiceState ?? "idle",
        decodedSamples: patch.decodedSamples ?? 0,
        generation: patch.connectionGeneration ?? 0,
        reconnectAttempt: patch.reconnectAttempt ?? 0,
        powerNotificationsAvailable: false,
        lastError: null,
      },
      audio: {
        phase: patch.audioPhase ?? "unconfigured",
        selectedEndpointId: null,
        selectedEndpointName: null,
        queuedSamples: patch.queuedSamples ?? 0,
        submittedSamples: patch.submittedSamples ?? 0,
        generation: 0,
        lastError: patch.audioLastError ?? null,
      },
      rawInput: {
        phase: patch.rawInputPhase ?? "stopped",
        matchedDeviceCount: 0,
        rawEventCount: 0,
        semanticEdgeCount: 0,
        lastButton: null,
        lastIsPressed: null,
        activeButtons: [],
        lastError: null,
        staleRemoteEventCount: 0,
      },
      buttonMapping: {
        enabled: true,
        gateActive: false,
        listenerActive: true,
        swallowedEdges: 0,
        leakedDowns: 0,
        firedGestures: 0,
        lastFired: null,
        lastError: null,
      },
    },
  };
}

const connectedRuntime = () =>
  runtimeWith({ connectionPhase: "ready", bleVoiceReady: true, rawInputPhase: "ready" });

const cableEndpoint: AudioEndpoint = {
  id: "cable-input",
  name: "CABLE Input (VB-Audio Virtual Cable)",
  isVirtualCableCandidate: true,
};

function onboardingEvents(phase: string): Array<Record<string, unknown>> {
  return mocks.reportFrontendEvent.mock.calls
    .map((call) => call[0] as Record<string, unknown>)
    .filter((payload) => payload.event === "onboarding" && payload.phase === phase);
}

/** 诊断报告夹具：与 Rust `get_diagnostic_report`（schema v2）同形。 */
function diagnosticReport(): DiagnosticReport {
  return {
    schemaVersion: 2,
    appVersion: "0.5.0",
    appBuild: "unknown",
    sourceRevision: "fe326f8a1b2c3d4e5f60718293a4b5c6d7e8f901",
    buildChannel: "local-test",
    windowsVersion: "10.0.26100",
    processArchitecture: "x86_64",
    platform: "windows",
    verificationStatus: "真机验证中",
    capabilities: {
      windowsApiAvailable: true,
      bleScanAvailable: true,
      bleVoiceReady: false,
      wasapiReady: true,
      rawInputReady: false,
      sendInputReady: true,
    },
    connection: {
      phase: "idle",
      capabilitiesConfirmed: false,
      sampleRate: null,
      frameSize: null,
      decodedSamples: 0,
      generation: 0,
      reconnectAttempt: 0,
      powerNotificationsAvailable: false,
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
}

async function mountWizard(
  runtime: RuntimeSnapshot,
  options: { attach?: boolean } = {},
): Promise<VueWrapper> {
  const wrapper = mount(OnboardingPage, {
    props: { runtime },
    ...(options.attach ? { attachTo: document.body } : {}),
  });
  await flushPromises();
  await flushPromises();
  return wrapper;
}

function resetWizardMocks(): void {
  mocks.reportFrontendEvent.mockReset();
    mocks.getOnboardingState.mockReset();
    mocks.saveOnboardingStep.mockReset();
    mocks.scanPairedRemotes.mockReset();
    mocks.connectRemote.mockReset();
    mocks.subscribeButtonEdges.mockReset();
    mocks.listAudioEndpoints.mockReset();
    mocks.getAudioSnapshot.mockReset();
    mocks.selectAudioEndpoint.mockReset();
    mocks.openVbCableDownloadPage.mockReset();
    mocks.openWindowsSettings.mockReset();
    mocks.getVoiceInputTool.mockReset();
    mocks.getVoiceHoldHotkey.mockReset();
    mocks.getOtherVoiceHotkey.mockReset();
    mocks.setOtherVoiceHotkey.mockReset();
    mocks.getRc003TaskStatus.mockReset();
    mocks.enableRc003Capture.mockReset();
    mocks.disableRc003Capture.mockReset();
    mocks.getVokieInstallation.mockReset();
    mocks.openVokieHomepage.mockReset();
    mocks.launchVokie.mockReset();
    mocks.stageOnboardingVoiceBinding.mockReset();
    mocks.setMappingSuspension.mockReset();
    mocks.completeOnboarding.mockReset();
    mocks.getRuntimeSnapshot.mockReset();
    mocks.getDiagnosticReport.mockReset();
    mocks.beginKeyObservation.mockReset();
    mocks.endKeyObservation.mockReset();

    mocks.getOnboardingState.mockResolvedValue(activeState("welcome"));
    mocks.saveOnboardingStep.mockImplementation(async (step: string) => activeState(step));
    mocks.subscribeButtonEdges.mockImplementation(
      async (handler: (edge: { button: string; isPressed: boolean }) => void) => {
        mocks.buttonHandler = handler;
        return () => {
          mocks.buttonHandler = null;
        };
      },
    );
    mocks.listAudioEndpoints.mockResolvedValue([]);
    mocks.getAudioSnapshot.mockResolvedValue({
      phase: "unconfigured",
      selectedEndpointId: null,
      selectedEndpointName: null,
      queuedSamples: 0,
      submittedSamples: 0,
      generation: 0,
      lastError: null,
    } satisfies AudioSnapshot);
    mocks.getVoiceInputTool.mockResolvedValue(null);
    mocks.getVoiceHoldHotkey.mockResolvedValue({ keys: ["left_control", "left_windows"] });
    mocks.getOtherVoiceHotkey.mockResolvedValue(null);
    mocks.setOtherVoiceHotkey.mockImplementation(async (keys: string[]) => keys);
    mocks.rc003Disabled = {
      installed: false,
      authorizationRequired: true,
      enabled: false,
      helperPath: null,
      lastError: null,
    };
    mocks.getRc003TaskStatus.mockImplementation(async () => mocks.rc003Disabled);
    mocks.enableRc003Capture.mockImplementation(async () => ({
      ...mocks.rc003Disabled,
      enabled: true,
    }));
    mocks.disableRc003Capture.mockImplementation(async () => mocks.rc003Disabled);
    mocks.getVokieInstallation.mockResolvedValue({ installed: false, running: false });
    mocks.openVokieHomepage.mockResolvedValue(undefined);
    mocks.launchVokie.mockResolvedValue(undefined);
    mocks.stageOnboardingVoiceBinding.mockImplementation(async () => activeState("voice_tool"));
    mocks.setMappingSuspension.mockResolvedValue(true);
    mocks.completeOnboarding.mockImplementation(async () => activeState("complete"));
    mocks.getRuntimeSnapshot.mockImplementation(async () => runtimeWith());
    mocks.getDiagnosticReport.mockImplementation(async () => diagnosticReport());
    let observationId = 0;
    mocks.beginKeyObservation.mockImplementation(async () => {
      observationId += 1;
      return observationId;
    });
    mocks.endKeyObservation.mockResolvedValue(0);
    mocks.scanPairedRemotes.mockResolvedValue([]);
    mocks.connectRemote.mockImplementation(async (id: string) => ({
      phase: "connecting",
      remoteName: "小米蓝牙语音遥控器",
      remoteModel: "rc003",
      capabilities: null,
      voiceState: "idle",
      decodedSamples: 0,
      generation: 0,
      reconnectAttempt: 0,
      powerNotificationsAvailable: false,
      lastError: null,
      remoteId: id,
    }));
}

function installWizardHooks(): void {
  beforeEach(resetWizardMocks);
  afterEach(() => {
    mocks.buttonHandler = null;
  });
}

describe("Onboarding wizard shell", () => {
  installWizardHooks();

  it("restores the persisted step, logs it, and keeps the wizard open", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    const wrapper = await mountWizard(runtimeWith());

    expect(wrapper.text()).toContain("连接遥控器");
    expect(wrapper.find(".onboarding-back").exists()).toBe(true);
    expect(wrapper.text()).toContain("连接检查");
    expect(mocks.saveOnboardingStep).toHaveBeenCalledWith("remote");
    // begin（result=unknown）与终态各一条；终态只允许一条。
    expect(onboardingEvents("started").filter((p) => p.result !== "unknown")).toHaveLength(1);
    expect(onboardingEvents("started").some((p) => p.reason === "state_read_requested")).toBe(true);
    expect(onboardingEvents("step_entered").some((p) => p.step === "remote")).toBe(true);
  });

  it("advances from welcome to remote and persists the new step", async () => {
    const wrapper = await mountWizard(runtimeWith());
    expect(wrapper.text()).toContain("欢迎使用无线麦 SayAll");

    const continueButton = wrapper.find("footer .primary-button");
    expect(continueButton.attributes("disabled")).toBeUndefined();
    await continueButton.trigger("click");
    await flushPromises();

    expect(mocks.saveOnboardingStep).toHaveBeenCalledWith("remote");
    expect(wrapper.text()).toContain("连接遥控器");
    expect(wrapper.find(".onboarding-back").exists()).toBe(true);
    const passed = onboardingEvents("step_passed");
    expect(passed.some((p) => p.step === "welcome" && p.reason === "user_continue")).toBe(true);
  });

  it("blocks the remote step until connected, listening and a control button is observed", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    const wrapper = await mountWizard(runtimeWith());

    // 未连接：阻断，且给用户可读原因。
    expect(onboardingEvents("step_blocked").some((p) => p.code === "remote.not_connected")).toBe(true);
    expect(wrapper.text()).toContain("先连接遥控器");

    // 已连接但还没按键：换成 button_not_ready，日志只记一次（状态未变化不重复刷）。
    await wrapper.setProps({ runtime: connectedRuntime() });
    await flushPromises();
    const readyBlocked = onboardingEvents("step_blocked").filter(
      (p) => p.code === "remote.button_not_ready",
    );
    expect(readyBlocked).toHaveLength(1);
    expect(wrapper.text()).toContain("按一下遥控器上的普通按键");

    // 收到生产链路普通按键：门禁通过，恢复事件落日志。
    mocks.buttonHandler?.({ button: "home", isPressed: true });
    await flushPromises();
    expect(onboardingEvents("step_recovered").some((p) => p.step === "remote")).toBe(true);
    expect(wrapper.find("footer .primary-button").attributes("data-gate-ready")).toBe("true");
  });

  it("scans paired remotes, connects the chosen one, and opens Windows Bluetooth settings", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    mocks.scanPairedRemotes.mockResolvedValue([
      { id: "dev-1", name: "小米蓝牙语音遥控器 2 Pro", model: "rc003", isSupportedCandidate: true },
    ]);
    const wrapper = await mountWizard(runtimeWith());

    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("扫描已配对设备"))!
      .trigger("click");
    await flushPromises();
    expect(mocks.scanPairedRemotes).toHaveBeenCalledTimes(1);
    expect(wrapper.text()).toContain("小米蓝牙语音遥控器 2 Pro");

    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("已配对的小米蓝牙语音遥控器") && button.text().endsWith("连接"))!
      .trigger("click");
    await flushPromises();
    expect(mocks.connectRemote).toHaveBeenCalledWith("dev-1");
    expect(wrapper.text()).toContain("正在确认语音功能");

    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("打开蓝牙设置"))!
      .trigger("click");
    await flushPromises();
    expect(mocks.openWindowsSettings).toHaveBeenCalledWith("bluetooth");
  });

  it("corrects a mistaken voice-key press while waiting for a control button", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    const wrapper = await mountWizard(connectedRuntime());

    await wrapper.setProps({ runtime: runtimeWith({ connectionPhase: "streaming", bleVoiceReady: true, rawInputPhase: "ready", voiceState: "streaming" }) });
    await flushPromises();
    expect(wrapper.text()).toContain("刚才按的是语音键");

    mocks.buttonHandler?.({ button: "ok", isPressed: true });
    await flushPromises();
    expect(wrapper.text()).not.toContain("刚才按的是语音键");
    expect(wrapper.text()).toContain("已收到遥控器按键");
  });

  it("navigates back without losing the wizard and logs the user action", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    const wrapper = await mountWizard(runtimeWith());

    // 新外壳：返回在内容列左上角；页脚只剩一个主按钮。
    expect(wrapper.find("footer .secondary-button").exists()).toBe(false);
    await wrapper.find(".onboarding-back").trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("欢迎使用无线麦 SayAll");
    const nav = onboardingEvents("navigation");
    expect(nav.some((p) => p.reason === "user_back" && p.step === "remote" && p.detail === "welcome")).toBe(true);
    expect(mocks.saveOnboardingStep).toHaveBeenCalledWith("welcome");
  });

  it("shows the VB-CABLE install path when no recommended endpoint exists", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("audio"));
    const wrapper = await mountWizard(runtimeWith());

    expect(wrapper.text()).toContain("还没有 VB-CABLE");
    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("打开官方下载页"))!
      .trigger("click");
    await flushPromises();
    expect(mocks.openVbCableDownloadPage).toHaveBeenCalledTimes(1);
  });

  it("auto-selects the only recommended endpoint and unlocks the audio gate when ready", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("audio"));
    mocks.listAudioEndpoints.mockResolvedValue([cableEndpoint]);
    mocks.selectAudioEndpoint.mockResolvedValue({
      phase: "ready",
      selectedEndpointId: cableEndpoint.id,
      selectedEndpointName: cableEndpoint.name,
      queuedSamples: 0,
      submittedSamples: 0,
      generation: 1,
      lastError: null,
    } satisfies AudioSnapshot);

    const wrapper = await mountWizard(runtimeWith());

    expect(mocks.selectAudioEndpoint).toHaveBeenCalledWith(cableEndpoint.id);
    const continueButton = wrapper.find("footer .primary-button");
    expect(continueButton.attributes("data-gate-ready")).toBe("true");
    expect(continueButton.attributes("disabled")).toBeUndefined();

    // 继续 → 步骤④（输入工具）。
    await continueButton.trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("选择你要用的输入工具");
  });

  it("keeps a recoverable state-read error with a retry entry", async () => {
    mocks.getOnboardingState.mockRejectedValueOnce(new Error("读取向导状态失败"));
    const wrapper = await mountWizard(runtimeWith());

    expect(wrapper.text()).toContain("无法读取设置进度");
    expect(onboardingEvents("started").some((p) => p.result === "failed")).toBe(true);

    mocks.getOnboardingState.mockResolvedValue(activeState("welcome"));
    await wrapper
      .findAll("button")
      .find((button) => button.text() === "重试")!
      .trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("欢迎使用无线麦 SayAll");
  });
});

describe("Onboarding diagnostics copy", () => {
  installWizardHooks();

  const clipboard = { writeText: vi.fn<(text: string) => Promise<void>>() };

  beforeEach(() => {
    clipboard.writeText.mockReset();
    clipboard.writeText.mockResolvedValue();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: clipboard.writeText },
    });
  });

  afterEach(() => {
    delete (navigator as { clipboard?: unknown }).clipboard;
  });

  it("一次复制出含 Windows 版本 / App 版本 / Build 与向导状态的诊断块", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    // 音频端点只读刷新：复制时读真实列表，而不是第①/②步可能还没加载的页面状态。
    mocks.listAudioEndpoints.mockResolvedValue([cableEndpoint]);
    mocks.getAudioSnapshot.mockResolvedValue({
      phase: "ready",
      selectedEndpointId: cableEndpoint.id,
      selectedEndpointName: cableEndpoint.name,
      queuedSamples: 0,
      submittedSamples: 0,
      generation: 1,
      lastError: null,
    } satisfies AudioSnapshot);
    const wrapper = await mountWizard(runtimeWith());

    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("复制诊断信息"))!
      .trigger("click");
    await flushPromises();

    expect(clipboard.writeText).toHaveBeenCalledTimes(1);
    const text = clipboard.writeText.mock.calls[0][0];
    expect(text).toContain("App 版本: 0.5.0");
    expect(text).toContain("Build: fe326f8a1b2c3d4e5f60718293a4b5c6d7e8f901");
    expect(text).toContain("Windows 版本: 10.0.26100");
    expect(text).toContain("步骤标识: remote");
    expect(text).toContain("门禁: remote.not_connected");
    expect(text).toContain("音频: 状态=ready 推荐设备=1 已选推荐=是");
    // 脱敏：设备 id / 端点身份不进剪贴板（挡住将来加错字段）。
    expect(text).not.toContain("cable-input");
    expect(text).not.toContain("CABLE Input");
    expect(wrapper.text()).toContain("已复制，可直接粘贴发给开发者");
    const copy = onboardingEvents("action").filter((p) => p.reason === "copy_diagnostics");
    expect(copy.map((p) => p.result)).toEqual(["unknown", "passed"]);
  });

  it("剪贴板不可用时不静默：给出失败提示并落 failed 日志", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    clipboard.writeText.mockRejectedValue(new Error("denied"));
    const wrapper = await mountWizard(runtimeWith());

    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("复制诊断信息"))!
      .trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("复制失败");
    const copy = onboardingEvents("action").filter((p) => p.reason === "copy_diagnostics");
    expect(copy.map((p) => p.result)).toEqual(["unknown", "failed"]);
  });
});

describe("Onboarding input tool step", () => {
  installWizardHooks();

  async function mountToolStep(): Promise<VueWrapper> {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_tool"));
    return mountWizard(runtimeWith());
  }

  function toolCard(wrapper: VueWrapper, title: string) {
    return wrapper.findAll("button").find((button) => button.text().includes(title))!;
  }

  it("stages the chosen tool with its fixed chord and blocks 豆包 until capture is enabled", async () => {
    const wrapper = await mountToolStep();

    await toolCard(wrapper, "豆包输入法").trigger("click");
    await flushPromises();
    expect(mocks.stageOnboardingVoiceBinding).toHaveBeenCalledWith("doubao", { keys: ["right_alt"] });
    const continueButton = wrapper.find("footer .primary-button");
    expect(continueButton.attributes("data-gate-code")).toBe("tool.doubao.authorization_required");
    expect(wrapper.text()).toContain("支持更多输入工具");

    // 点击开关 → 确认弹窗 → 开启 → 状态读回 → 门禁通过。
    await wrapper.find('input[type="checkbox"]').trigger("change");
    await flushPromises();
    expect(wrapper.text()).toContain("开启“全按键支持”？");
    await wrapper.findAll("button").find((button) => button.text() === "开启")!.trigger("click");
    await flushPromises();
    expect(mocks.enableRc003Capture).toHaveBeenCalledTimes(1);
    expect(continueButton.attributes("data-gate-ready")).toBe("true");
    expect(continueButton.attributes("data-gate-code")).toBe("ok");
  });

  it("blocks 豆包 while Vokie is running and recovers after it closes", async () => {
    mocks.getVokieInstallation.mockResolvedValue({ installed: true, running: true });
    const wrapper = await mountToolStep();

    await toolCard(wrapper, "豆包输入法").trigger("click");
    await flushPromises();
    const continueButton = wrapper.find("footer .primary-button");
    expect(continueButton.attributes("data-gate-code")).toBe("tool.conflict.vokie_running");
    expect(wrapper.text()).toContain("请先退出 Vokie");

    mocks.getVokieInstallation.mockResolvedValue({ installed: true, running: false });
    await wrapper.findAll("button").find((button) => button.text() === "重新检测")!.trigger("click");
    await flushPromises();
    // 冲突解除后，剩下一道门是「支持更多输入工具」未开启。
    expect(continueButton.attributes("data-gate-code")).toBe("tool.doubao.authorization_required");
  });

  it("walks the Vokie install → launch → running flow", async () => {
    const wrapper = await mountToolStep();

    await toolCard(wrapper, "Vokie").trigger("click");
    await flushPromises();
    expect(mocks.stageOnboardingVoiceBinding).toHaveBeenCalledWith("vokie", { keys: ["right_alt"] });
    expect(wrapper.text()).toContain("没有检测到 Vokie");

    await wrapper.findAll("button").find((button) => button.text().includes("打开官网"))!.trigger("click");
    await flushPromises();
    expect(mocks.openVokieHomepage).toHaveBeenCalledTimes(1);

    mocks.getVokieInstallation.mockResolvedValue({ installed: true, running: false });
    await wrapper.findAll("button").find((button) => button.text() === "重新检测")!.trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("已安装但没有运行");

    await wrapper.findAll("button").find((button) => button.text().includes("打开 Vokie"))!.trigger("click");
    await flushPromises();
    expect(mocks.launchVokie).toHaveBeenCalledTimes(1);

    mocks.getVokieInstallation.mockResolvedValue({ installed: true, running: true });
    await wrapper.findAll("button").find((button) => button.text() === "重新检测")!.trigger("click");
    await flushPromises();
    expect(wrapper.find("footer .primary-button").attributes("data-gate-ready")).toBe("true");
  });

  it("requires an explicit key choice for 其他工具 and remembers 不按键", async () => {
    const wrapper = await mountToolStep();

    await toolCard(wrapper, "其他工具").trigger("click");
    await flushPromises();
    expect(mocks.stageOnboardingVoiceBinding).not.toHaveBeenCalled();
    const continueButton = wrapper.find("footer .primary-button");
    expect(continueButton.attributes("data-gate-code")).toBe("tool.hotkey_unset");

    await wrapper.findAll("button").find((button) => button.text() === "右 Alt")!.trigger("click");
    await flushPromises();
    expect(mocks.setOtherVoiceHotkey).toHaveBeenCalledWith(["right_alt"]);
    expect(mocks.stageOnboardingVoiceBinding).toHaveBeenCalledWith("other", { keys: ["right_alt"] });
    expect(continueButton.attributes("data-gate-ready")).toBe("true");

    await wrapper.findAll("button").find((button) => button.text() === "不按键")!.trigger("click");
    await flushPromises();
    expect(mocks.setOtherVoiceHotkey).toHaveBeenLastCalledWith([]);
    expect(mocks.stageOnboardingVoiceBinding).toHaveBeenLastCalledWith("other", null);
    expect(continueButton.attributes("data-gate-ready")).toBe("true");
  });

  it("treats 微信输入法 as immediately satisfied on the wizard side", async () => {
    const wrapper = await mountToolStep();

    await toolCard(wrapper, "微信输入法").trigger("click");
    await flushPromises();
    expect(mocks.stageOnboardingVoiceBinding).toHaveBeenCalledWith("wechat", {
      keys: ["left_control", "left_windows"],
    });
    const continueButton = wrapper.find("footer .primary-button");
    expect(continueButton.attributes("data-gate-ready")).toBe("true");
    // 门禁已满足：继续可进入第⑤步（按住说话验证）。
    expect(continueButton.attributes("disabled")).toBeUndefined();
    await continueButton.trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("按住遥控器语音键，试一句话");
    wrapper.unmount();
  });
});

describe("Onboarding controls & completion steps", () => {
  installWizardHooks();

  const completeReadyRuntime = () =>
    runtimeWith({
      connectionPhase: "ready",
      bleVoiceReady: true,
      rawInputPhase: "ready",
      audioPhase: "ready",
    });

  const readyAudioSnapshot = () =>
    ({
      phase: "ready",
      selectedEndpointId: cableEndpoint.id,
      selectedEndpointName: cableEndpoint.name,
      queuedSamples: 0,
      submittedSamples: 0,
      generation: 1,
      lastError: null,
    }) satisfies AudioSnapshot;

  it("suspends mappings while the controls step collects 3 distinct buttons", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("controls"));
    const wrapper = await mountWizard(connectedRuntime());

    expect(mocks.setMappingSuspension).toHaveBeenCalledWith(true);
    const continueButton = wrapper.find("footer .primary-button");
    expect(continueButton.attributes("data-gate-code")).toBe("controls.not_confirmed");

    // 同一个键重复按只累计次数，不算新的不同按键。
    mocks.buttonHandler?.({ button: "home", isPressed: true });
    mocks.buttonHandler?.({ button: "home", isPressed: true });
    mocks.buttonHandler?.({ button: "ok", isPressed: true });
    await flushPromises();
    expect(continueButton.attributes("data-gate-ready")).toBe("false");
    expect(wrapper.text()).toContain("再按 1 个不同的普通按键。");
    expect(wrapper.findAll(".onboarding-dots .dot.on")).toHaveLength(2);

    mocks.buttonHandler?.({ button: "back", isPressed: true });
    await flushPromises();
    expect(continueButton.attributes("data-gate-ready")).toBe("true");
    expect(onboardingEvents("step_recovered").some((p) => p.step === "controls")).toBe(true);

    // 继续 → 完成步骤：映射执行恢复（回到用户的正式配置）。
    await continueButton.trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("设置完成");
    expect(mocks.setMappingSuspension).toHaveBeenLastCalledWith(false);
  });

  it("corrects a mistaken voice-key press during the controls step", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("controls"));
    const wrapper = await mountWizard(connectedRuntime());

    await wrapper.setProps({
      runtime: runtimeWith({
        connectionPhase: "streaming",
        bleVoiceReady: true,
        rawInputPhase: "ready",
        voiceState: "streaming",
      }),
    });
    await flushPromises();
    expect(wrapper.text()).toContain("这是语音键");

    mocks.buttonHandler?.({ button: "home", isPressed: true });
    await flushPromises();
    expect(wrapper.text()).not.toContain("这是语音键");
  });

  it("navigates back from controls to the voice test step and releases the suspension", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("controls"));
    const wrapper = await mountWizard(connectedRuntime());

    await wrapper.find(".onboarding-back").trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("按住遥控器语音键，试一句话");
    expect(mocks.saveOnboardingStep).toHaveBeenCalledWith("voice_test");
    expect(mocks.setMappingSuspension).toHaveBeenLastCalledWith(false);
    wrapper.unmount();
  });

  it("re-checks runtime conditions on the complete step and routes an unsatisfied page back", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("complete"));
    mocks.listAudioEndpoints.mockResolvedValue([cableEndpoint]);
    mocks.getAudioSnapshot.mockResolvedValue(readyAudioSnapshot());
    const wrapper = await mountWizard(completeReadyRuntime());

    expect(wrapper.text()).toContain("设置完成");
    const continueButton = wrapper.find("footer .primary-button");
    expect(continueButton.attributes("data-gate-code")).toBe("tool.not_selected");

    await wrapper
      .findAll("button")
      .find((button) => button.text() === "去修复")!
      .trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("选择你要用的输入工具");
    expect(
      onboardingEvents("navigation").some(
        (p) => p.reason === "fix_regression" && p.detail === "voice_tool",
      ),
    ).toBe(true);
  });

  it("blocks completion until the hold-to-talk verification passes", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("complete"));
    mocks.listAudioEndpoints.mockResolvedValue([cableEndpoint]);
    mocks.getAudioSnapshot.mockResolvedValue(readyAudioSnapshot());
    mocks.getVoiceInputTool.mockResolvedValue("wechat");
    const wrapper = await mountWizard(completeReadyRuntime());

    const continueButton = wrapper.find("footer .primary-button");
    expect(continueButton.attributes("data-gate-code")).toBe("voice_test.not_verified");
    expect(wrapper.text()).toContain("完成一次真实的语音上屏测试");

    // 「去修复」跳回第⑤步重新验证。
    await wrapper
      .findAll("button")
      .find((button) => button.text() === "去修复")!
      .trigger("click");
    await flushPromises();
    expect(
      onboardingEvents("navigation").some(
        (p) => p.reason === "fix_regression" && p.detail === "voice_test",
      ),
    ).toBe(true);
    expect(wrapper.text()).toContain("按住遥控器语音键，试一句话");
    expect(mocks.saveOnboardingStep).toHaveBeenCalledWith("voice_test");
    wrapper.unmount();
  });
});

describe("Onboarding voice test step", () => {
  installWizardHooks();

  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "Date"] });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  function queueSnapshots(...frames: RuntimeSnapshot[]): void {
    let index = 0;
    mocks.getRuntimeSnapshot.mockImplementation(async () => {
      const frame = frames[Math.min(index, frames.length - 1)];
      index += 1;
      return frame;
    });
  }

  const streamingFrame = (patch: Parameters<typeof runtimeWith>[0] = {}) =>
    runtimeWith({
      connectionPhase: "streaming",
      bleVoiceReady: true,
      rawInputPhase: "ready",
      voiceState: "streaming",
      audioPhase: "streaming",
      ...patch,
    });

  it("passes a held session once delivered samples and text are observed", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue("wechat");
    mocks.getVoiceHoldHotkey.mockResolvedValue({ keys: ["left_control", "left_windows"] });
    queueSnapshots(
      runtimeWith(),
      streamingFrame({ decodedSamples: 120, submittedSamples: 320 }),
      streamingFrame({ voiceState: "draining", audioPhase: "draining", decodedSamples: 150, submittedSamples: 640, queuedSamples: 40 }),
      runtimeWith({ connectionPhase: "ready", bleVoiceReady: true, rawInputPhase: "ready", audioPhase: "ready", decodedSamples: 160, submittedSamples: 640, queuedSamples: 0 }),
    );

    const wrapper = await mountWizard(connectedRuntime(), { attach: true });
    await vi.advanceTimersByTimeAsync(200); // 开始帧
    await vi.advanceTimersByTimeAsync(200); // 收尾帧

    // 文字出现（DOM 输入事件）→ 结算手动输入计数（0）→ 记录转写。
    await wrapper.find("input.onboarding-voice-input").setValue("你好");
    await flushPromises();
    // 观察窗口排除报告层合成的和弦键（微信：左 Ctrl + 左 Win）。
    expect(mocks.beginKeyObservation).toHaveBeenCalledWith([0xa2, 0x5b]);

    await vi.advanceTimersByTimeAsync(200); // 结束帧 → 通过
    await flushPromises();

    expect(wrapper.text()).toContain("成功：文字已经出现在输入框里");
    // 2026-10-05 改版定稿：核对卡是提示行（编号），不设勾选框——通过判据只有真实文字上屏。
    expect(wrapper.text()).toContain("按你选的工具核对两件事");
    expect(wrapper.findAll('input[type="checkbox"]')).toHaveLength(0);
    const continueButton = wrapper.find("footer .primary-button");
    expect(continueButton.attributes("data-gate-ready")).toBe("true");
    expect(onboardingEvents("voice_attempt").some((p) => p.result === "passed")).toBe(true);

    await continueButton.trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("按一下遥控器的普通按键");
    wrapper.unmount();
  });

  it("fails the negative control: typed text with a counted physical key", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue("wechat");
    mocks.endKeyObservation.mockResolvedValue(1);
    queueSnapshots(runtimeWith(), streamingFrame({ decodedSamples: 120, submittedSamples: 320 }));

    const wrapper = await mountWizard(connectedRuntime(), { attach: true });
    await vi.advanceTimersByTimeAsync(200);

    await wrapper.find("input.onboarding-voice-input").setValue("手打的字");
    await flushPromises();

    expect(wrapper.text()).toContain("检测到键盘输入");
    expect(
      onboardingEvents("voice_attempt").some((p) => p.reason === "voice.manual_input"),
    ).toBe(true);
    const continueButton = wrapper.find("footer .primary-button");
    expect(continueButton.attributes("data-gate-code")).toBe("voice_test.not_verified");
    expect(wrapper.findAll("button").some((button) => button.text() === "重新测试")).toBe(true);
    wrapper.unmount();
  });

  it("clears the box and re-arms a fresh attempt on retry", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue("wechat");
    mocks.endKeyObservation.mockResolvedValueOnce(1).mockResolvedValue(0);
    queueSnapshots(runtimeWith(), streamingFrame({ decodedSamples: 120, submittedSamples: 320 }));

    const wrapper = await mountWizard(connectedRuntime(), { attach: true });
    await vi.advanceTimersByTimeAsync(200);
    await wrapper.find("input.onboarding-voice-input").setValue("手打的字");
    await flushPromises();
    expect(wrapper.text()).toContain("检测到键盘输入");

    await wrapper
      .findAll("button")
      .find((button) => button.text() === "重新测试")!
      .trigger("click");
    await flushPromises();

    const box = wrapper.find("input.onboarding-voice-input").element as HTMLInputElement;
    expect(box.value).toBe("");
    expect(wrapper.text()).not.toContain("检测到键盘输入");
    expect(
      onboardingEvents("voice_attempt").filter((p) => p.reason === "armed"),
    ).toHaveLength(2);
    expect(onboardingEvents("step_retry").some((p) => p.step === "voice_test")).toBe(true);
    wrapper.unmount();
  });

  it("asks the user to click back into the box when a session starts unfocused", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue("doubao");
    queueSnapshots(runtimeWith(), streamingFrame({ decodedSamples: 80, submittedSamples: 100 }));

    const wrapper = await mountWizard(connectedRuntime(), { attach: true });
    await wrapper.find("input.onboarding-voice-input").trigger("blur");
    await flushPromises();
    await vi.advanceTimersByTimeAsync(200);
    await flushPromises();

    expect(wrapper.text()).toContain("输入框没有聚焦");
    expect(
      onboardingEvents("voice_attempt").some((p) => p.reason === "voice.input_target_not_ready"),
    ).toBe(true);
    wrapper.unmount();
  });
});

describe("Onboarding per-step diagnostics", () => {
  installWizardHooks();

  it("logs scan begin/end with counts only, never device identity or name", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    mocks.scanPairedRemotes.mockResolvedValue([
      { id: "aa:bb:cc:dd:ee:ff", name: "Secret Remote 2 Pro" },
    ]);
    const wrapper = await mountWizard(runtimeWith());

    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("扫描已配对设备"))!
      .trigger("click");
    await flushPromises();

    const scan = onboardingEvents("action").filter((p) => p.reason === "scan_requested");
    expect(scan.map((p) => p.result)).toEqual(["unknown", "passed"]);
    expect(scan[1].detail).toBe("found_1");
    // 脱敏：设备 id / 名称不得进入日志载荷。
    const serialized = JSON.stringify(mocks.reportFrontendEvent.mock.calls);
    expect(serialized).not.toContain("aa:bb");
    expect(serialized).not.toContain("Secret");
    wrapper.unmount();
  });

  it("logs connect begin/end without the device id", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    mocks.scanPairedRemotes.mockResolvedValue([
      { id: "AA:BB:CC:DD:EE:FF", name: "小米蓝牙语音遥控器 2 Pro" },
    ]);
    const wrapper = await mountWizard(runtimeWith());
    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("扫描已配对设备"))!
      .trigger("click");
    await flushPromises();
    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("已配对的小米蓝牙语音遥控器") && button.text().endsWith("连接"))!
      .trigger("click");
    await flushPromises();

    const connect = onboardingEvents("action").filter((p) => p.reason === "connect_requested");
    expect(connect.map((p) => p.result)).toEqual(["unknown", "passed"]);
    const serialized = JSON.stringify(mocks.reportFrontendEvent.mock.calls);
    expect(serialized).not.toContain("AA:BB");
    wrapper.unmount();
  });

  it("logs a failed action when button observation subscription fails", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    mocks.subscribeButtonEdges.mockRejectedValue(new Error("subscribe failed"));
    const wrapper = await mountWizard(runtimeWith());
    await flushPromises();

    const observation = onboardingEvents("action").filter((p) => p.reason === "button_observation");
    expect(observation.map((p) => p.result)).toEqual(["unknown", "failed"]);
    wrapper.unmount();
  });

  it("logs audio refresh/select with counts and route state, without endpoint identity", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("audio"));
    mocks.listAudioEndpoints.mockResolvedValue([cableEndpoint]);
    mocks.selectAudioEndpoint.mockResolvedValue({
      phase: "ready",
      selectedEndpointId: cableEndpoint.id,
      selectedEndpointName: cableEndpoint.name,
      queuedSamples: 0,
      submittedSamples: 0,
      generation: 1,
      lastError: null,
    } satisfies AudioSnapshot);
    const wrapper = await mountWizard(runtimeWith());
    await flushPromises();

    const refresh = onboardingEvents("action").filter((p) => p.reason === "refresh_endpoints");
    expect(refresh.map((p) => p.result)).toEqual(["unknown", "passed"]);
    expect(refresh[1].detail).toBe("auto_total_1_rec_1");
    const select = onboardingEvents("action").filter((p) => p.reason === "select_endpoint");
    expect(select.map((p) => p.result)).toEqual(["unknown", "passed"]);
    expect(onboardingEvents("audio_route").at(-1)?.reason).toBe("ready");
    // 脱敏：端点 id / 名称不得进入日志载荷。
    const serialized = JSON.stringify(mocks.reportFrontendEvent.mock.calls);
    expect(serialized).not.toContain("cable-input");
    expect(serialized).not.toContain("CABLE Input");
    wrapper.unmount();
  });

  it("logs a failed action when the Vokie launch request fails", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_tool"));
    mocks.getVokieInstallation.mockResolvedValue({ installed: true, running: false });
    mocks.launchVokie.mockRejectedValue(new Error("launch failed"));
    const wrapper = await mountWizard(runtimeWith());

    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("Vokie"))!
      .trigger("click");
    await flushPromises();
    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("打开 Vokie"))!
      .trigger("click");
    await flushPromises();

    const launch = onboardingEvents("action").filter((p) => p.reason === "vokie_launch");
    expect(launch.map((p) => p.result)).toEqual(["unknown", "failed"]);
    wrapper.unmount();
  });

  it("logs complete refresh failures with a desensitized source token", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("complete"));
    mocks.getVoiceInputTool.mockRejectedValue(new Error("读取失败"));
    const wrapper = await mountWizard(connectedRuntime());
    await flushPromises();

    const refresh = onboardingEvents("action").filter((p) => p.reason === "complete_refresh");
    expect(refresh.map((p) => p.result)).toEqual(["unknown", "failed"]);
    expect(refresh[1].detail).toBe("tool");
    wrapper.unmount();
  });

  it("emits a periodic heartbeat carrying the current step for stuck localization", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "Date"] });
    try {
      mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
      const wrapper = await mountWizard(runtimeWith());
      await vi.advanceTimersByTimeAsync(30_000);
      const beats = onboardingEvents("heartbeat");
      expect(beats.length).toBeGreaterThanOrEqual(1);
      expect(beats[0].step).toBe("remote");
      wrapper.unmount();
    } finally {
      vi.useRealTimers();
    }
  });
});
