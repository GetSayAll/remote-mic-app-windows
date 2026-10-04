import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AudioEndpoint,
  AudioPhase,
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
        phase: patch.audioPhase ?? "unconfigured",
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
    // 门禁已满足，但步骤⑤（按住说话验证）尚未接入：本分支在 voice_tool 止步，按钮保持禁用。
    expect(continueButton.attributes("disabled")).toBeDefined();
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
    expect(wrapper.text()).toContain("× 2");

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

  it("skips the not-yet-implemented voice_test step when navigating back from controls", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("controls"));
    const wrapper = await mountWizard(connectedRuntime());

    await wrapper.find("footer .secondary-button").trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("选择你要用的输入工具");
    expect(mocks.saveOnboardingStep).toHaveBeenCalledWith("voice_tool");
    expect(mocks.setMappingSuspension).toHaveBeenLastCalledWith(false);
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

    // 第⑤步尚未接入：修复请求不跳转（记录 fix_blocked），停留在完成页。
    await wrapper
      .findAll("button")
      .find((button) => button.text() === "去修复")!
      .trigger("click");
    await flushPromises();
    expect(
      onboardingEvents("navigation").some(
        (p) => p.reason === "fix_blocked" && p.detail === "voice_test",
      ),
    ).toBe(true);
    expect(wrapper.text()).toContain("设置完成");
    expect(mocks.saveOnboardingStep).not.toHaveBeenCalledWith("voice_test");
  });
});
