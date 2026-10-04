import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AudioEndpoint,
  AudioSnapshot,
  ConnectionPhase,
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
        decodedSamples: 0,
        generation: 0,
        reconnectAttempt: patch.reconnectAttempt ?? 0,
        powerNotificationsAvailable: false,
        lastError: null,
      },
      audio: {
        phase: "unconfigured",
        selectedEndpointId: null,
        selectedEndpointName: null,
        queuedSamples: 0,
        submittedSamples: 0,
        generation: 0,
        lastError: null,
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

async function mountWizard(runtime: RuntimeSnapshot): Promise<VueWrapper> {
  const wrapper = mount(OnboardingPage, { props: { runtime } });
  await flushPromises();
  await flushPromises();
  return wrapper;
}

describe("Onboarding wizard shell", () => {
  beforeEach(() => {
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
  });

  afterEach(() => {
    mocks.buttonHandler = null;
  });

  it("restores the persisted step, logs it, and keeps the wizard open", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    const wrapper = await mountWizard(runtimeWith());

    expect(wrapper.text()).toContain("连接小米蓝牙语音遥控器");
    expect(mocks.saveOnboardingStep).toHaveBeenCalledWith("remote");
    expect(onboardingEvents("started")).toHaveLength(1);
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
    expect(wrapper.text()).toContain("连接小米蓝牙语音遥控器");
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
      .find((button) => button.text() === "连接")!
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

    await wrapper.find("footer .secondary-button").trigger("click");
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
    // 门禁已满足，但步骤④（输入工具）尚未接入：本分支在 audio 止步，按钮保持禁用。
    expect(continueButton.attributes("disabled")).toBeDefined();
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
