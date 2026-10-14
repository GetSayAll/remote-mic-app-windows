import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AudioEndpoint,
  AudioPhase,
  AudioSnapshot,
  ConnectionPhase,
  DiagnosticReport,
  RawInputPhase,
  Rc003BridgeSnapshot,
  RuntimeSnapshot,
  VoiceSessionState,
} from "../lib/bridge";
import { readFileSync } from "node:fs";
import { join } from "node:path";
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
  getImeStatus: vi.fn(),
  getOtherVoiceHotkey: vi.fn(),
  setOtherVoiceHotkey: vi.fn(),
  getRc003TaskStatus: vi.fn(),
  getRc003BridgeSnapshot: vi.fn(),
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
    getImeStatus: mocks.getImeStatus,
    getOtherVoiceHotkey: mocks.getOtherVoiceHotkey,
    setOtherVoiceHotkey: mocks.setOtherVoiceHotkey,
    getRc003TaskStatus: mocks.getRc003TaskStatus,
    getRc003BridgeSnapshot: mocks.getRc003BridgeSnapshot,
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
    remoteModel?: "rc001" | "rc003" | "unknown";
    staleRemoteEventCount?: number;
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
        remoteModel: patch.remoteModel ?? "unknown",
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
        staleRemoteEventCount: patch.staleRemoteEventCount ?? 0,
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
    schemaVersion: 3,
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
    ime: {
      detected: true,
      activeName: "微软拼音",
      installedNames: ["微软拼音", "微信输入法"],
      expectedName: "微信输入法",
      matchesExpected: false,
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

/** 桥接快照夹具（诊断块只看 captureHealth，其余字段按"零值"填）。 */
function rc003BridgeSnapshot(
  overrides: Partial<Rc003BridgeSnapshot> = {},
): Rc003BridgeSnapshot {
  return {
    phase: "connected",
    port: 0,
    helperPid: 0,
    acceptedTotal: 0,
    deniedTotal: 0,
    replacedTotal: 0,
    edgesApplied: 0,
    usagesDropped: 0,
    malformedTotal: 0,
    watchdogReleaseTotal: 0,
    pressedUsages: [],
    lastRxAgeMs: null,
    targetGeneration: 0,
    targetUsages: [],
    ownedUsages: [],
    captureHealth: "off",
    ...overrides,
  };
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
    mocks.getRc003BridgeSnapshot.mockReset();
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
    // 默认：输入法读不到（fail-open——界面不提示）。个别用例再覆盖成"读得到"。
    mocks.getImeStatus.mockResolvedValue({
      detected: false,
      active: null,
      installed: [],
      expected: null,
      matchesExpected: null,
    });
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
    mocks.getRc003BridgeSnapshot.mockResolvedValue(rc003BridgeSnapshot());
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
    // 标题位置稳定（2026-10-05 要求）：第①步没有上一步，但保留同高占位，
    // 各步标题固定在同一位置、切换时不抖动。
    expect(wrapper.find(".onboarding-back--placeholder").exists()).toBe(true);

    const continueButton = wrapper.find(".onboarding-actions .primary-button");
    expect(continueButton.attributes("disabled")).toBeUndefined();
    await continueButton.trigger("click");
    await flushPromises();

    expect(mocks.saveOnboardingStep).toHaveBeenCalledWith("remote");
    expect(wrapper.text()).toContain("连接遥控器");
    expect(wrapper.find(".onboarding-back").exists()).toBe(true);
    expect(wrapper.find(".onboarding-back--placeholder").exists()).toBe(false);
    const passed = onboardingEvents("step_passed");
    expect(passed.some((p) => p.step === "welcome" && p.reason === "user_continue")).toBe(true);
  });

  it("第①步给完整配对流程（含步骤2下方的打开蓝牙设置），且点击不写提示", async () => {
    const wrapper = await mountWizard(runtimeWith());

    // 2026-10-13 现场定稿文案。
    expect(wrapper.text()).toContain("首次使用：遥控器怎么连电脑");
    expect(wrapper.text()).toContain(
      "准备好：小米蓝牙语音遥控器 2 或 2 Pro（目前只支持这两种型号），并确保它有电。",
    );
    expect(wrapper.text()).toContain(
      "在电脑上打开蓝牙设置（可直接点击下方按钮），打开电脑的蓝牙开关后点击「添加设备」然后选择「蓝牙」选项。",
    );
    expect(wrapper.text()).toContain("长按「TV」键约 3 秒");
    expect(wrapper.text()).toContain("同时长按「主页」+「菜单」");
    expect(wrapper.text()).toContain("先点开「显示所有设备」");
    expect(wrapper.text()).toContain("回到第 4 步重置蓝牙后重新配对");
    // 底部那段提示按现场要求删掉。
    expect(wrapper.text()).not.toContain("设置中途可以随时退出");
    // 编号连续 1–5（第 2 条下方是「打开蓝牙设置」按钮行，不占编号）。
    const nums = wrapper.findAll(".onboarding-num").map((node) => node.text());
    expect(nums).toEqual(["1", "2", "3", "4", "5"]);
    const actionRow = wrapper.find(".onboarding-numlist-action");
    expect(actionRow.exists()).toBe(true);
    expect(actionRow.findAll("button").map((node) => node.text())).toEqual(["打开蓝牙设置"]);
    // 按钮在列表里的位置：第 2 条之后（DOM 顺序：1、2、按钮行、3…）。
    const listItems = wrapper.findAll(".onboarding-numlist li");
    expect(listItems[0]!.find(".onboarding-num").text()).toBe("1");
    expect(listItems[1]!.find(".onboarding-num").text()).toBe("2");
    expect(listItems[2]!.classes()).toContain("onboarding-numlist-action");
    expect(listItems[2]!.findAll("button").map((node) => node.text())).toEqual(["打开蓝牙设置"]);
    expect(listItems[3]!.find(".onboarding-num").text()).toBe("3");

    // 点击只唤起系统设置页，不写"已打开…"提示（2026-10-13 现场要求）。
    await actionRow.find("button").trigger("click");
    await flushPromises();
    expect(mocks.openWindowsSettings).toHaveBeenCalledWith("bluetooth");
    expect(wrapper.text()).not.toContain("已打开系统蓝牙设置");
    const actions = onboardingEvents("action").filter((p) => p.reason === "open_bluetooth_settings");
    expect(actions.map((p) => p.result)).toEqual(["unknown", "passed"]);
    wrapper.unmount();
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
    expect(wrapper.find(".onboarding-actions .primary-button").attributes("data-gate-ready")).toBe("true");
  });

  it("按键已收到但还没找到遥控器时，说明句指向扫描", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    // 系统里没有已配对遥控器：自动连接不介入（扫描 0 台），保留"按键已收到、未连接"现场
    // （2026-10-10 用户报障：系统侧按键通道可用 ≠ 应用自己的会话已建立）。
    mocks.scanPairedRemotes.mockResolvedValue([]);
    const wrapper = await mountWizard(runtimeWith({ rawInputPhase: "ready" }));

    mocks.buttonHandler?.({ button: "home", isPressed: true });
    await flushPromises();

    expect(wrapper.text()).toContain("已收到遥控器按键");
    expect(wrapper.text()).toContain("按键已收到。还差一步：点“扫描已配对设备”选中遥控器连接。");
    expect(wrapper.text()).not.toContain("这一项可以继续了");
    expect(onboardingEvents("step_blocked").some((p) => p.code === "remote.not_connected")).toBe(true);
    wrapper.unmount();
  });

  it("设备已列出（候选不唯一）时，说明句改为指向设备行上的连接", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    mocks.scanPairedRemotes.mockResolvedValue([
      { id: "dev-1", name: "小米蓝牙语音遥控器 2 Pro", model: "rc003", isSupportedCandidate: true },
      { id: "dev-2", name: "小米蓝牙语音遥控器 2", model: "rc001", isSupportedCandidate: true },
    ]);
    const wrapper = await mountWizard(runtimeWith({ rawInputPhase: "ready" }));
    await flushPromises();

    mocks.buttonHandler?.({ button: "home", isPressed: true });
    await flushPromises();

    expect(wrapper.text()).toContain("按键已收到。还差一步：点遥控器一行右侧的“连接”。");
    wrapper.unmount();
  });

  it("已连接且已收到按键时，按键行不再写任何说明句", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    const wrapper = await mountWizard(connectedRuntime());

    mocks.buttonHandler?.({ button: "home", isPressed: true });
    await flushPromises();

    expect(wrapper.text()).toContain("已收到遥控器按键");
    // 2026-10-13 现场：整行就绪 + 门禁放行，不再重复写"这一项可以继续了。"——
    // 该行此时只剩标题，没有 <small> 说明句。
    expect(wrapper.text()).not.toContain("这一项可以继续了");
    const buttonRow = wrapper
      .findAll(".onboarding-status-row")
      .find((row) => row.text().includes("已收到遥控器按键"));
    expect(buttonRow).toBeDefined();
    expect(buttonRow!.find("small").exists()).toBe(false);
    wrapper.unmount();
  });

  it("启动恢复的连接也能标出已连接那一行（两行同名时靠设备令牌对上）", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    // 本机现场：两支遥控器名字完全一样，连接还是启动时恢复的（界面没点过「连接」）。
    mocks.scanPairedRemotes.mockResolvedValue([
      { id: "dev-1", name: "小米蓝牙语音遥控器", model: "rc001", isSupportedCandidate: true, token: "aaaa1111" },
      { id: "dev-2", name: "小米蓝牙语音遥控器", model: "rc001", isSupportedCandidate: true, token: "bbbb2222" },
    ]);
    const ready = runtimeWith({ connectionPhase: "ready", bleVoiceReady: true, remoteModel: "rc001" });
    ready.platform.connection.remoteToken = "bbbb2222";
    const wrapper = await mountWizard(ready);
    await flushPromises();

    const rows = wrapper.findAll(".onboarding-option");
    expect(rows).toHaveLength(2);
    expect(rows[0]!.text()).not.toContain("已连接");
    expect(rows[1]!.text()).toContain("已连接");
    // 已连接那一行显示型号——型号是唯一能区分两支同名遥控器的字段。
    expect(rows[1]!.text()).toContain("小米蓝牙语音遥控器 2");
    expect(rows[1]!.attributes("disabled")).toBeDefined();
    wrapper.unmount();
  });

  it("已连接时显示具体型号（2 / 2 Pro），不只给系统名字", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    // 本机现场：Windows 给两支遥控器的名字完全一样（「小米蓝牙语音遥控器」）。
    mocks.scanPairedRemotes.mockResolvedValue([
      { id: "dev-1", name: "小米蓝牙语音遥控器", model: "rc003", isSupportedCandidate: true },
    ]);
    const wrapper = await mountWizard(runtimeWith());
    await flushPromises();

    // 手动连接那一支 → 型号（GATT 读回）随之可见。
    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("小米蓝牙语音遥控器"))!
      .trigger("click");
    await flushPromises();
    await wrapper.setProps({
      runtime: runtimeWith({ connectionPhase: "ready", bleVoiceReady: true, remoteModel: "rc003" }),
    });
    await flushPromises();

    expect(wrapper.text()).toContain("小米蓝牙语音遥控器 2 Pro · 已连接");
    expect(wrapper.findAll(".onboarding-option")[0]!.text()).toContain("小米蓝牙语音遥控器 2 Pro");
    wrapper.unmount();
  });

  it("换连另一支遥控器后要重新按一下（之前那支收到的按键不算数）", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    mocks.scanPairedRemotes.mockResolvedValue([
      { id: "dev-1", name: "小米蓝牙语音遥控器", model: "rc003", isSupportedCandidate: true, token: "aaaa1111" },
      { id: "dev-2", name: "小米蓝牙语音遥控器", model: "rc001", isSupportedCandidate: true, token: "bbbb2222" },
    ]);
    // 启动恢复：已经连着甲支（2 Pro），令牌来自运行时快照。
    const connectedA = runtimeWith({
      connectionPhase: "ready",
      bleVoiceReady: true,
      rawInputPhase: "ready",
      remoteModel: "rc003",
    });
    connectedA.platform.connection.remoteToken = "aaaa1111";
    const wrapper = await mountWizard(connectedA);
    await flushPromises();

    mocks.buttonHandler?.({ button: "home", isPressed: true });
    await flushPromises();
    expect(wrapper.text()).toContain("已收到遥控器按键");
    expect(wrapper.find(".onboarding-actions .primary-button").attributes("data-gate-ready")).toBe("true");

    // 现场（2026-10-13）：按甲支通过后换连乙支，必须重新接收按键。切换期间快照会先清掉
    // 旧令牌（断开上一支）、再带上新令牌——瞬时清空不算换机，不重置。
    const between = runtimeWith({ connectionPhase: "ready", bleVoiceReady: true, rawInputPhase: "ready" });
    await wrapper.setProps({ runtime: between });
    await flushPromises();
    expect(wrapper.text()).toContain("已收到遥控器按键");

    const connectedB = runtimeWith({
      connectionPhase: "ready",
      bleVoiceReady: true,
      rawInputPhase: "ready",
      remoteModel: "rc001",
    });
    connectedB.platform.connection.remoteToken = "bbbb2222";
    await wrapper.setProps({ runtime: connectedB });
    await flushPromises();

    // 回到"等待控制按键"，门禁重新阻断，日志留一条。
    expect(wrapper.text()).toContain("等待控制按键");
    expect(wrapper.text()).toContain("按一下遥控器上的普通按键");
    expect(wrapper.find(".onboarding-actions .primary-button").attributes("data-gate-ready")).toBe("false");
    expect(
      onboardingEvents("action").some(
        (p) => p.step === "remote" && p.reason === "button_observation_reset",
      ),
    ).toBe(true);

    // 重新按一下 → 恢复通过。
    mocks.buttonHandler?.({ button: "home", isPressed: true });
    await flushPromises();
    expect(wrapper.text()).toContain("已收到遥控器按键");
    expect(wrapper.find(".onboarding-actions .primary-button").attributes("data-gate-ready")).toBe("true");
    wrapper.unmount();
  });

  it("按到另一支遥控器时给出解释，而不是让界面看起来没反应", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    mocks.scanPairedRemotes.mockResolvedValue([
      { id: "dev-1", name: "小米蓝牙语音遥控器", model: "rc003", isSupportedCandidate: true },
    ]);
    const connected = () =>
      runtimeWith({ connectionPhase: "ready", bleVoiceReady: true, remoteModel: "rc003" });
    const wrapper = await mountWizard(connected());
    await flushPromises();
    expect(wrapper.text()).not.toContain("另一支遥控器");

    // Raw Input 把"另一支"的报文计入 stale → 给一句解释（整步只提示一次并落日志）。
    await wrapper.setProps({ runtime: { ...connected(), platform: { ...connected().platform, rawInput: { ...connected().platform.rawInput, staleRemoteEventCount: 5 } } } });
    await flushPromises();
    expect(wrapper.text()).toContain("刚收到的是另一支遥控器的按键");
    expect(wrapper.text()).toContain("当前连接的是「小米蓝牙语音遥控器 2 Pro」");
    const logged = onboardingEvents("action").filter((p) => p.reason === "other_remote_button");
    expect(logged).toHaveLength(1);

    // 收到本体按键后让位给正常的"已收到遥控器按键"。
    mocks.buttonHandler?.({ button: "home", isPressed: true });
    await flushPromises();
    expect(wrapper.text()).not.toContain("刚收到的是另一支遥控器的按键");
    expect(wrapper.text()).toContain("已收到遥控器按键");
    wrapper.unmount();
  });

  it("打开蓝牙设置、扫描、连接：连接成功后行上显示已连接", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    mocks.scanPairedRemotes.mockResolvedValue([
      { id: "dev-1", name: "小米蓝牙语音遥控器 2 Pro", model: "rc003", isSupportedCandidate: true },
    ]);
    const wrapper = await mountWizard(runtimeWith());

    // 两个动作按钮可见（2026-10-13 现场：暂时按"总是显示"放出，见 RemoteStep）。
    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("扫描已配对设备"))!
      .trigger("click");
    await flushPromises();
    // 进入②步的那次自动扫描（entry）+ 本次手动扫描。
    expect(mocks.scanPairedRemotes).toHaveBeenCalledTimes(2);
    expect(wrapper.text()).toContain("小米蓝牙语音遥控器 2 Pro");
    // 扫描按钮常显（2026-10-13 现场），② 步不再有「打开蓝牙设置」（已移到第①步）；
    // 设备行的「连接」做成按钮观感。
    const buttonTexts = wrapper.findAll("button").map((button) => button.text());
    expect(buttonTexts.some((text) => text.includes("扫描已配对设备"))).toBe(true);
    expect(buttonTexts.some((text) => text.includes("打开蓝牙设置"))).toBe(false);
    expect(wrapper.findAll(".option-action").map((node) => node.text())).toEqual(["连接"]);

    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("小米蓝牙语音遥控器 2 Pro"))!
      .trigger("click");
    await flushPromises();
    expect(mocks.connectRemote).toHaveBeenCalledWith("dev-1");
    // 连接成功不写进度消息（2026-10-13 现场要求），引导由按键行承担。
    expect(wrapper.text()).not.toContain("正在确认语音功能");

    // 运行时快照进入 ready：设备行换成"已连接"状态，且不再可点。
    const ready = runtimeWith({ connectionPhase: "ready", bleVoiceReady: true });
    await wrapper.setProps({ runtime: ready });
    await flushPromises();
    expect(wrapper.findAll(".option-action").map((node) => node.text())).toEqual(["已连接"]);
    expect(wrapper.findAll(".onboarding-option")[0]!.attributes("disabled")).toBeDefined();
    // 连接后显示的名称用的是系统名称（2026-10-13 现场：必须能分清 2 / 2 Pro），
    // 而不是运行时型号标签（型号解析不出时那是占位文案）。
    expect(wrapper.text()).toContain("小米蓝牙语音遥控器 2 Pro · 已连接");
    wrapper.unmount();
  });

  it("扫描没有找到已配对遥控器时，给出“按照上一步配对”的提示", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    mocks.scanPairedRemotes.mockResolvedValue([]);
    const wrapper = await mountWizard(runtimeWith());

    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("扫描已配对设备"))!
      .trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("当前没有已配对设备，请先按照上一步完成设备配对。");
    expect(wrapper.text()).toContain("扫描已配对设备");
    expect(wrapper.text()).not.toContain("打开蓝牙设置");
    wrapper.unmount();
  });

  it("扫描进行中只给状态行标题，不再写“保持遥控器在旁边”那句", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    // 扫描挂起：组件停在"正在扫描"（按钮文字也变成「正在扫描…」）。
    mocks.scanPairedRemotes.mockImplementation(() => new Promise(() => {}));
    const wrapper = await mountWizard(runtimeWith());
    await flushPromises();

    await wrapper.find(".onboarding-chips button").trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("正在寻找小米遥控器…");
    expect(wrapper.text()).not.toContain("保持遥控器在旁边");
    wrapper.unmount();
  });

  it("进入②步且系统里恰好一个已配对遥控器时自动连接（日志带 auto 标记）", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    mocks.scanPairedRemotes.mockResolvedValue([
      { id: "dev-1", name: "小米蓝牙语音遥控器 2 Pro", model: "rc003", isSupportedCandidate: true },
    ]);
    const wrapper = await mountWizard(runtimeWith({ rawInputPhase: "ready" }));
    await flushPromises();

    expect(mocks.scanPairedRemotes).toHaveBeenCalledTimes(1);
    expect(mocks.connectRemote).toHaveBeenCalledWith("dev-1");
    // 自动扫描的结果行与设备卡都要出现（连接过程不再写进度消息，2026-10-13）。
    expect(wrapper.text()).toContain("找到 1 个已配对的小米遥控器");
    expect(wrapper.text()).toContain("已配对的小米蓝牙语音遥控器");
    const scan = onboardingEvents("action").filter((p) => p.reason === "scan_requested");
    // 2026-10-13：进入②步的自动扫描（entry）先带出列表，再由自动连接接手。
    expect(scan.map((p) => p.detail)).toEqual(["entry", "entry_found_1"]);
    const connect = onboardingEvents("action").filter((p) => p.reason === "connect_requested");
    expect(connect.map((p) => p.result)).toEqual(["unknown", "passed"]);
    expect(connect[0].detail).toBe("auto");
    wrapper.unmount();
  });

  it("停留在②步时遥控器出现（系统侧刚配对完成）后自动连接", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    mocks.scanPairedRemotes.mockResolvedValue([
      { id: "dev-1", name: "小米蓝牙语音遥控器 2 Pro", model: "rc003", isSupportedCandidate: true },
    ]);
    const wrapper = await mountWizard(runtimeWith());
    // 2026-10-13 现场：进入②步先扫一次把设备列出来（不再等输入通道在位）。
    expect(mocks.scanPairedRemotes).toHaveBeenCalledTimes(1);
    expect(wrapper.text()).toContain("找到 1 个已配对的小米遥控器");
    // 未连接但输入通道还没在位：先不自动连接。
    expect(mocks.connectRemote).not.toHaveBeenCalled();

    await wrapper.setProps({ runtime: runtimeWith({ rawInputPhase: "ready" }) });
    await flushPromises();
    await flushPromises();

    expect(mocks.connectRemote).toHaveBeenCalledWith("dev-1");
    wrapper.unmount();
  });

  it("候选不唯一时不自动连接，交给用户选择", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    mocks.scanPairedRemotes.mockResolvedValue([
      { id: "dev-1", name: "小米蓝牙语音遥控器 2 Pro", model: "rc003", isSupportedCandidate: true },
      { id: "dev-2", name: "小米蓝牙语音遥控器 2", model: "rc001", isSupportedCandidate: true },
    ]);
    const wrapper = await mountWizard(runtimeWith({ rawInputPhase: "ready" }));
    await flushPromises();

    expect(mocks.connectRemote).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("找到 2 个已配对的小米遥控器");
    wrapper.unmount();
  });

  it("已连接或用户已手动断开时不自动连接，但列表仍直接带出", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    mocks.scanPairedRemotes.mockResolvedValue([
      { id: "dev-1", name: "小米蓝牙语音遥控器 2 Pro", model: "rc003", isSupportedCandidate: true },
    ]);
    const connected = await mountWizard(connectedRuntime());
    await flushPromises();
    // 2026-10-13 现场：应用已连接（含启动恢复）时原来不扫描 → ②步空列表；现在照扫。
    expect(mocks.scanPairedRemotes).toHaveBeenCalledTimes(1);
    expect(mocks.connectRemote).not.toHaveBeenCalled();
    expect(connected.text()).toContain("小米蓝牙语音遥控器 2 Pro");
    const entryScan = onboardingEvents("action").filter((p) => p.reason === "scan_requested");
    expect(entryScan[entryScan.length - 1].detail).toBe("entry_found_1");
    connected.unmount();

    mocks.scanPairedRemotes.mockClear();
    const disconnected = await mountWizard(
      runtimeWith({ connectionPhase: "disconnected", rawInputPhase: "ready" }),
    );
    await flushPromises();
    expect(mocks.scanPairedRemotes).toHaveBeenCalledTimes(1);
    expect(mocks.connectRemote).not.toHaveBeenCalled();
    disconnected.unmount();
  });

  it("连接成功不再写「正在确认语音功能…」进度消息", async () => {
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
    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("已配对的小米蓝牙语音遥控器") && button.text().endsWith("连接"))!
      .trigger("click");
    await flushPromises();

    expect(mocks.connectRemote).toHaveBeenCalledWith("dev-1");
    // 引导由下方按键状态行承担（"按一下遥控器上的普通按键…"），不再叠一条进度消息。
    expect(wrapper.text()).not.toContain("正在确认语音功能");
    expect(wrapper.text()).toContain("按一下遥控器上的普通按键");
  });

  it("连接就绪后清掉残留的连接过程消息", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    mocks.scanPairedRemotes.mockResolvedValue([
      { id: "dev-1", name: "小米蓝牙语音遥控器 2 Pro", model: "rc003", isSupportedCandidate: true },
    ]);
    // 连接失败写下的原因消息：连接随后就绪时必须被清掉（2026-10-13 现场保留的行为）。
    mocks.connectRemote.mockRejectedValueOnce(new Error("遥控器没有响应，请重试。"));
    const wrapper = await mountWizard(runtimeWith());

    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("扫描已配对设备"))!
      .trigger("click");
    await flushPromises();
    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("小米蓝牙语音遥控器 2 Pro"))!
      .trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("遥控器没有响应，请重试。");

    // 之后连接就绪（例如用户在系统里或应用里让它连上）：残留消息随之消失。
    await wrapper.setProps({ runtime: connectedRuntime() });
    await flushPromises();
    expect(wrapper.text()).not.toContain("遥控器没有响应，请重试。");
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

    // 新外壳（2026-10-05 反馈后）：返回在内容列左上；不再有整幅页脚横幅；
    // 复制诊断在动作行左侧、主按钮在右侧。
    expect(wrapper.find("footer").exists()).toBe(false);
    expect(wrapper.find(".onboarding-actions-left .onboarding-diagnostics-button").exists()).toBe(true);
    expect(wrapper.find(".onboarding-actions-right .primary-button").exists()).toBe(true);
    const back = wrapper.find(".onboarding-back");
    expect(back.find("svg.onboarding-back-icon").exists()).toBe(true);
    expect(back.text()).toContain("返回");
    await wrapper.find(".onboarding-back").trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("欢迎使用无线麦 SayAll");
    const nav = onboardingEvents("navigation");
    expect(nav.some((p) => p.reason === "user_back" && p.step === "remote" && p.detail === "welcome")).toBe(true);
    expect(mocks.saveOnboardingStep).toHaveBeenCalledWith("welcome");
  });

  it("suspends custom button mappings while the remote step is detecting buttons", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    const wrapper = await mountWizard(runtimeWith());
    await flushPromises();

    // 2026-10-05 用户要求：检测按键时不得触发用户已配置的自定义动作。
    expect(mocks.setMappingSuspension).toHaveBeenCalledWith(true);

    // 离开检测步骤（② → ①）立即恢复执行。
    await wrapper.find(".onboarding-back").trigger("click");
    await flushPromises();
    expect(mocks.setMappingSuspension).toHaveBeenLastCalledWith(false);
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
    const continueButton = wrapper.find(".onboarding-actions .primary-button");
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
    expect(text).toContain("App version: 0.5.0");
    expect(text).toContain("Build: fe326f8a1b2c3d4e5f60718293a4b5c6d7e8f901");
    expect(text).toContain("Windows version: 10.0.26100");
    expect(text).toContain("Wizard step: remote (2/7)");
    expect(text).toContain("Gate: remote.not_connected");
    expect(text).toContain("Blocked: yes");
    expect(text).toContain("Audio: phase=ready recommended=1 selected=yes");
    // 2026-10-10 新增：②步按键观察、输入通道计数、错误位与语音测试状态
    // （②步"按键已收到但未连接"的现场靠这几项就能自证）。
    expect(text).toContain("button_observed=no");
    expect(text).toContain(
      "Raw input: phase=stopped matched=0 raw_events=0 semantic_edges=0 stale_events=0",
    );
    expect(text).toContain("Voice test: phase=waiting_start terminal=none");
    expect(text).toContain("error=no");
    // 内容全英文（用户要求）；界面提示仍是中文。
    expect(text).not.toMatch(/[\u3000-\u9fff\uff00-\uffef]/);
    // 脱敏：设备 id / 端点身份不进剪贴板（挡住将来加错字段）。
    expect(text).not.toContain("cable-input");
    expect(text).not.toContain("CABLE Input");
    expect(wrapper.text()).toContain("已复制，可直接粘贴发给开发者");
    // 2026-10-13 现场：复制成功只留下方那条绿色提示——按钮文字不再变成"已复制到剪贴板"。
    expect(wrapper.text()).not.toContain("已复制到剪贴板");
    const copyButton = wrapper
      .findAll("button")
      .find((button) => button.text().includes("复制诊断信息"));
    expect(copyButton).toBeDefined();
    const copy = onboardingEvents("action").filter((p) => p.reason === "copy_diagnostics");
    expect(copy.map((p) => p.result)).toEqual(["unknown", "passed"]);
  });

  it("①/②步复制时只读刷新输入工具与快捷键，不沿用页面未加载的 unset", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    // 页面在②步还没读过这两项（④/⑤步才加载）：复制时应直接读磁盘真值。
    mocks.getVoiceInputTool.mockResolvedValue("doubao");
    mocks.getVoiceHoldHotkey.mockResolvedValue({ keys: ["right_alt"] });
    const wrapper = await mountWizard(runtimeWith());

    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("复制诊断信息"))!
      .trigger("click");
    await flushPromises();

    const text = clipboard.writeText.mock.calls[0][0];
    expect(text).toContain("Input tool: doubao hotkey=right_alt full_key_support=off capture=off");
  });

  it("②步复制读回「全按键支持」的真实状态：开关开着但没生效也自证（2026-10-14）", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    // 开关设置在磁盘上是开的（②步页面上从没读过它，旧行为会误报 off）；
    // 同时捕获状态是"没生效"——两者并列写进块里，不用日志就能分流。
    mocks.getRc003TaskStatus.mockResolvedValue({
      installed: true,
      authorizationRequired: false,
      enabled: true,
      helperPath: null,
      lastError: null,
    });
    mocks.getRc003BridgeSnapshot.mockResolvedValue(
      rc003BridgeSnapshot({ captureHealth: "inactive" }),
    );
    const wrapper = await mountWizard(runtimeWith());

    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("复制诊断信息"))!
      .trigger("click");
    await flushPromises();

    const text = clipboard.writeText.mock.calls[0][0];
    expect(text).toContain("full_key_support=on capture=inactive");
  });

  it("捕获状态读不到时写 capture=unknown，复制本身不失败", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    mocks.getRc003BridgeSnapshot.mockRejectedValue(new Error("read failed"));
    const wrapper = await mountWizard(runtimeWith());

    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("复制诊断信息"))!
      .trigger("click");
    await flushPromises();

    const text = clipboard.writeText.mock.calls[0][0];
    expect(text).toContain("capture=unknown");
    expect(wrapper.text()).toContain("已复制，可直接粘贴发给开发者");
    // 失败也留痕：块里只有 unknown 时，靠这条行动日志分流「读不到」与「真 off」。
    expect(
      onboardingEvents("action").some(
        (p) => p.reason === "read_capture_state" && p.result === "failed",
      ),
    ).toBe(true);
  });

  it("载荷里出现未知捕获状态时回落 capture=unknown（白名单校验）", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    // 类型约束在运行时不存在：后端多回了一个未知 token 也不能原样进块。
    mocks.getRc003BridgeSnapshot.mockResolvedValue(
      rc003BridgeSnapshot({ captureHealth: "bogus" as never }),
    );
    const wrapper = await mountWizard(runtimeWith());

    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("复制诊断信息"))!
      .trigger("click");
    await flushPromises();

    const text = clipboard.writeText.mock.calls[0][0];
    expect(text).toContain("capture=unknown");
  });

  it("复制时开关读失败退回页面状态、捕获读失败写 unknown，都不影响复制", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_tool"));
    // ④步加载时读到「开启」；复制时的只读刷新失败 → 退回页面状态（on）。
    mocks.getRc003TaskStatus.mockResolvedValueOnce({
      installed: true,
      authorizationRequired: false,
      enabled: true,
      helperPath: null,
      lastError: null,
    });
    mocks.getRc003TaskStatus.mockRejectedValueOnce(new Error("read failed"));
    mocks.getRc003BridgeSnapshot.mockRejectedValue(new Error("read failed"));
    const wrapper = await mountWizard(runtimeWith());

    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("复制诊断信息"))!
      .trigger("click");
    await flushPromises();

    const text = clipboard.writeText.mock.calls[0][0];
    expect(text).toContain("full_key_support=on capture=unknown");
    // 恰好两次：④步加载一次 + 复制时刷新一次——第二次失败被独立留痕，
    // 断言调用次数可防「④步不再读」时该用例退化成假绿。
    expect(mocks.getRc003TaskStatus).toHaveBeenCalledTimes(2);
    expect(
      onboardingEvents("action").some(
        (p) => p.reason === "read_capture_switch" && p.result === "failed",
      ),
    ).toBe(true);
  });

  it("只读刷新失败时退回页面状态，复制本身不失败", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("remote"));
    mocks.getVoiceInputTool.mockRejectedValue(new Error("read failed"));
    mocks.getVoiceHoldHotkey.mockRejectedValue(new Error("read failed"));
    const wrapper = await mountWizard(runtimeWith());

    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("复制诊断信息"))!
      .trigger("click");
    await flushPromises();

    const text = clipboard.writeText.mock.calls[0][0];
    expect(text).toContain("Input tool: unset hotkey=unset");
    expect(wrapper.text()).toContain("已复制，可直接粘贴发给开发者");
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
    const continueButton = wrapper.find(".onboarding-actions .primary-button");
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

  it("向导里的同一个开关：无状态字、进行中转圈、失败只给用户语言（2026-10-10）", async () => {
    const wrapper = await mountToolStep();
    await toolCard(wrapper, "豆包输入法").trigger("click");
    await flushPromises();

    // 关闭态：开关与状态提示已合并成一个开关——开关行里只有名字，没有状态字。
    expect(wrapper.find(".switch-state").exists()).toBe(false);
    const switchRow = wrapper.find(".onboarding-switch-row");
    expect(switchRow.text()).toBe("支持更多输入工具");
    expect(switchRow.find(".toggle-placeholder").exists()).toBe(false);

    // 进行中：开关置灰 + 转圈，且不配文字（真机上这一段是授权窗口期间）。
    let release!: (value: Awaited<ReturnType<typeof mocks.enableRc003Capture>>) => void;
    mocks.enableRc003Capture.mockImplementation(
      () =>
        new Promise((resolve) => {
          release = resolve;
        }),
    );
    await wrapper.find('input[type="checkbox"]').trigger("change");
    await flushPromises();
    await wrapper.findAll("button").find((button) => button.text() === "开启")!.trigger("click");
    await flushPromises();
    expect(wrapper.find<HTMLInputElement>('input[type="checkbox"]').element.disabled).toBe(true);
    expect(wrapper.find(".toggle-spinner").exists()).toBe(true);
    expect(wrapper.text()).not.toContain("正在设置");

    // 失败：唯一允许出现的文字是映射后的用户语言，不带内部原文。
    release({
      installed: false,
      authorizationRequired: true,
      enabled: false,
      helperPath: null,
      lastError: "启用全按键支持失败：授权未完成（UAC 被取消）。",
    });
    await flushPromises();
    expect(wrapper.find(".toggle-spinner").exists()).toBe(false);
    expect(wrapper.find(".onboarding-error").text()).toBe(
      "没有完成系统授权，“支持更多输入工具”保持关闭。",
    );
    expect(wrapper.text()).not.toContain("UAC");
    expect(wrapper.text()).not.toContain("授权未完成");
  });

  it("blocks 豆包 while Vokie is running and recovers after it closes", async () => {    mocks.getVokieInstallation.mockResolvedValue({ installed: true, running: true });
    const wrapper = await mountToolStep();

    await toolCard(wrapper, "豆包输入法").trigger("click");
    await flushPromises();
    const continueButton = wrapper.find(".onboarding-actions .primary-button");
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
    expect(wrapper.find(".onboarding-actions .primary-button").attributes("data-gate-ready")).toBe("true");
  });

  it("requires an explicit key choice for 其他工具 and remembers 不按键", async () => {
    const wrapper = await mountToolStep();

    await toolCard(wrapper, "其他工具").trigger("click");
    await flushPromises();
    expect(mocks.stageOnboardingVoiceBinding).not.toHaveBeenCalled();
    const continueButton = wrapper.find(".onboarding-actions .primary-button");
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
    const continueButton = wrapper.find(".onboarding-actions .primary-button");
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
    const continueButton = wrapper.find(".onboarding-actions .primary-button");
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
    // 真实用户能到⑥步，说明④步已经选过工具；补上这个前置（2026-10-13 起⑤步缺工具会退回④步）。
    mocks.getVoiceInputTool.mockResolvedValue("doubao");
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
    const continueButton = wrapper.find(".onboarding-actions .primary-button");
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

    const continueButton = wrapper.find(".onboarding-actions .primary-button");
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
    // 2026-10-05 要求：通过提示用绿色成功样式 + 对勾。
    expect(wrapper.find(".onboarding-card.success .onboarding-success .ok-mark").exists()).toBe(true);
    // 2026-10-05 改版定稿：核对卡是提示行（编号），不设勾选框——通过判据只有真实文字上屏。
    // 2026-10-13 现场定稿：微信 / 豆包时三项且"输入法"排第一；Vokie / 其他工具两项。
    expect(wrapper.text()).toContain("按你选的工具核对三件事");
    expect(wrapper.text()).toContain("输入法切到“微信输入法”。");
    expect(wrapper.text()).toContain("麦克风选 CABLE Output（在“微信输入法”的语音输入设置里）。");
    expect(wrapper.findAll('input[type="checkbox"]')).toHaveLength(0);
    const continueButton = wrapper.find(".onboarding-actions .primary-button");
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
    const continueButton = wrapper.find(".onboarding-actions .primary-button");
    expect(continueButton.attributes("data-gate-code")).toBe("voice_test.not_verified");
    expect(wrapper.findAll("button").some((button) => button.text() === "重新测试")).toBe(true);
    wrapper.unmount();
  });

  // 2026-10-14 现场：`voice.no_transcript` 不总是"麦克风 / 工具配置错"——「支持更多输入工具」
  // 未生效时豆包收不到报告层合成出的语音键，失败解释按**失败瞬间**的捕获状态分流。
  it("no_transcript 失败 + 捕获状态 starting：解释为「还在准备中」", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue("doubao");
    mocks.getRc003BridgeSnapshot.mockResolvedValue(
      rc003BridgeSnapshot({ captureHealth: "starting" }),
    );
    mocks.endKeyObservation.mockResolvedValue(0);
    queueSnapshots(
      runtimeWith(),
      streamingFrame({ decodedSamples: 120, submittedSamples: 320 }),
      runtimeWith({
        connectionPhase: "ready",
        bleVoiceReady: true,
        rawInputPhase: "ready",
        audioPhase: "ready",
        decodedSamples: 160,
        submittedSamples: 640,
        queuedSamples: 0,
      }),
    );

    const wrapper = await mountWizard(connectedRuntime(), { attach: true });
    await vi.advanceTimersByTimeAsync(200); // 开始帧（streaming）
    await vi.advanceTimersByTimeAsync(200); // 收尾帧 → waiting_transcript
    await vi.advanceTimersByTimeAsync(3_200); // 转写窗口（3 秒）到期 → no_transcript
    await flushPromises();

    expect(wrapper.text()).toContain("还在准备中");
    expect(wrapper.text()).toContain("“支持更多输入工具”");
    expect(wrapper.text()).not.toContain("语音链路正常");
    wrapper.unmount();
  });

  it("no_transcript 失败 + 捕获状态 inactive：给出「关掉再打开」指引", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue("doubao");
    mocks.getRc003BridgeSnapshot.mockResolvedValue(
      rc003BridgeSnapshot({ captureHealth: "inactive" }),
    );
    mocks.endKeyObservation.mockResolvedValue(0);
    queueSnapshots(
      runtimeWith(),
      streamingFrame({ decodedSamples: 120, submittedSamples: 320 }),
      runtimeWith({
        connectionPhase: "ready",
        bleVoiceReady: true,
        rawInputPhase: "ready",
        audioPhase: "ready",
        decodedSamples: 160,
        submittedSamples: 640,
        queuedSamples: 0,
      }),
    );

    const wrapper = await mountWizard(connectedRuntime(), { attach: true });
    await vi.advanceTimersByTimeAsync(200);
    await vi.advanceTimersByTimeAsync(200);
    await vi.advanceTimersByTimeAsync(3_200);
    await flushPromises();

    expect(wrapper.text()).toContain("还没有生效");
    expect(wrapper.text()).toContain("关掉再打开");
    expect(wrapper.text()).not.toContain("语音链路正常");
    wrapper.unmount();
  });

  // 复核补测（2026-10-14）：三类边界——在途细化不覆盖新会话 / 细化读失败回落 /
  // 非豆包不分流。第一类正是首版被 Vue ref 深代理陷阱静默拦死的那条路径。
  it("重试后的旧细化回调不覆盖新会话（会话世代作废旧回调）", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue("doubao");
    let resolveSnapshotDeferred: { resolve?: (value: Rc003BridgeSnapshot) => void } = {};
    mocks.getRc003BridgeSnapshot.mockImplementation(
      () =>
        new Promise<Rc003BridgeSnapshot>((resolve) => {
          resolveSnapshotDeferred.resolve = resolve;
        }),
    );
    mocks.endKeyObservation.mockResolvedValue(0);
    queueSnapshots(
      runtimeWith(),
      streamingFrame({ decodedSamples: 120, submittedSamples: 320 }),
      runtimeWith({
        connectionPhase: "ready",
        bleVoiceReady: true,
        rawInputPhase: "ready",
        audioPhase: "ready",
        decodedSamples: 160,
        submittedSamples: 640,
        queuedSamples: 0,
      }),
    );

    const wrapper = await mountWizard(connectedRuntime(), { attach: true });
    await vi.advanceTimersByTimeAsync(200);
    await vi.advanceTimersByTimeAsync(200);
    await vi.advanceTimersByTimeAsync(3_200); // no_transcript 终态；细化读取挂起
    await flushPromises();
    expect(wrapper.text()).toContain("语音链路正常");

    // 用户点「重新测试」：会话世代 +1；旧回调随后返回也不得写入。
    await wrapper
      .findAll("button")
      .find((button) => button.text() === "重新测试")!
      .trigger("click");
    await flushPromises();
    resolveSnapshotDeferred.resolve?.(rc003BridgeSnapshot({ captureHealth: "starting" }));
    await flushPromises();

    expect(wrapper.text()).not.toContain("还在准备中");
    expect(wrapper.text()).not.toContain("语音链路正常");
    wrapper.unmount();
  });

  it("细化现读捕获状态失败 → 回落 unknown，保留原排查文案", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue("doubao");
    mocks.getRc003BridgeSnapshot.mockRejectedValue(new Error("read failed"));
    mocks.endKeyObservation.mockResolvedValue(0);
    queueSnapshots(
      runtimeWith(),
      streamingFrame({ decodedSamples: 120, submittedSamples: 320 }),
      runtimeWith({
        connectionPhase: "ready",
        bleVoiceReady: true,
        rawInputPhase: "ready",
        audioPhase: "ready",
        decodedSamples: 160,
        submittedSamples: 640,
        queuedSamples: 0,
      }),
    );

    const wrapper = await mountWizard(connectedRuntime(), { attach: true });
    await vi.advanceTimersByTimeAsync(200);
    await vi.advanceTimersByTimeAsync(200);
    await vi.advanceTimersByTimeAsync(3_200);
    await flushPromises();

    expect(wrapper.text()).toContain("语音链路正常");
    expect(wrapper.text()).not.toContain("还在准备中");
    wrapper.unmount();
  });

  it("非豆包工具不分流：不读捕获状态，仍用原排查文案", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue("wechat");
    mocks.getRc003BridgeSnapshot.mockResolvedValue(
      rc003BridgeSnapshot({ captureHealth: "starting" }),
    );
    mocks.endKeyObservation.mockResolvedValue(0);
    queueSnapshots(
      runtimeWith(),
      streamingFrame({ decodedSamples: 120, submittedSamples: 320 }),
      runtimeWith({
        connectionPhase: "ready",
        bleVoiceReady: true,
        rawInputPhase: "ready",
        audioPhase: "ready",
        decodedSamples: 160,
        submittedSamples: 640,
        queuedSamples: 0,
      }),
    );

    const wrapper = await mountWizard(connectedRuntime(), { attach: true });
    await vi.advanceTimersByTimeAsync(200);
    await vi.advanceTimersByTimeAsync(200);
    await vi.advanceTimersByTimeAsync(3_200);
    await flushPromises();

    expect(wrapper.text()).toContain("语音链路正常");
    expect(mocks.getRc003BridgeSnapshot).not.toHaveBeenCalled();
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

  it("核对卡按工具分两档：豆包 / 微信三项（输入法排第一），Vokie / 其他两项（2026-10-13 现场）", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue("doubao");
    const wrapper = await mountWizard(connectedRuntime(), { attach: true });
    await flushPromises();

    let items = wrapper.findAll(".onboarding-numlist li");
    expect(wrapper.text()).toContain("按你选的工具核对三件事");
    expect(items).toHaveLength(3);
    expect(items[0]!.text()).toContain("输入法切到“豆包输入法”。");

    // 换成不需要切输入法的工具：那一项消失、标题回到两件事。
    wrapper.unmount();
    mocks.getVoiceInputTool.mockResolvedValue("vokie");
    const vokie = await mountWizard(connectedRuntime(), { attach: true });
    await flushPromises();

    items = vokie.findAll(".onboarding-numlist li");
    expect(vokie.text()).toContain("按你选的工具核对两件事");
    expect(items).toHaveLength(2);
    expect(vokie.text()).not.toContain("输入法切到");
    vokie.unmount();
  });

  it("试一下页：点输入框时确保输入法，切不过去就给解释（2026-10-13 现场）", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue("doubao");
    mocks.getImeStatus.mockResolvedValue({
      detected: true,
      active: "微软拼音",
      installed: ["微软拼音", "豆包输入法"],
      expected: "豆包输入法",
      matchesExpected: false,
    });
    const wrapper = await mountWizard(connectedRuntime(), { attach: true });

    await wrapper.find("input.onboarding-voice-input").trigger("focus");
    await flushPromises();

    expect(mocks.getImeStatus).toHaveBeenCalled();
    expect(wrapper.text()).toContain("输入法还没切到“豆包输入法”");
    expect(wrapper.text()).toContain("现在系统里选的是“微软拼音”");
    expect(
      onboardingEvents("action").some((p) => p.reason === "ime_check" && p.result === "failed"),
    ).toBe(true);

    // 已经切好：不打扰。
    mocks.getImeStatus.mockResolvedValue({
      detected: true,
      active: "豆包输入法",
      installed: ["豆包输入法"],
      expected: "豆包输入法",
      matchesExpected: true,
    });
    await wrapper.find("input.onboarding-voice-input").trigger("focus");
    await flushPromises();
    expect(wrapper.text()).not.toContain("输入法还没切到");
    expect(
      onboardingEvents("action").some((p) => p.reason === "ime_check" && p.result === "passed"),
    ).toBe(true);
    wrapper.unmount();
  });

  it("读不到输入法状态时不提示（fail-open）", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue("doubao");
    const wrapper = await mountWizard(connectedRuntime(), { attach: true });

    await wrapper.find("input.onboarding-voice-input").trigger("focus");
    await flushPromises();

    expect(mocks.getImeStatus).toHaveBeenCalled();
    expect(wrapper.text()).not.toContain("输入法还没切到");
    wrapper.unmount();
  });

  it("重启后直接落在⑤步：按磁盘配置补读工具与快捷键（不再显示“你选的输入工具”）", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue("wechat");
    mocks.getVoiceHoldHotkey.mockResolvedValue({ keys: ["right_alt"] });
    const wrapper = await mountWizard(connectedRuntime(), { attach: true });
    await flushPromises();

    // 工具名出现在核对卡里（此前 stagedTool 未读 ⇒ 回退文案"你选的输入工具"，2026-10-13 现场）。
    expect(wrapper.text()).toContain("输入法切到“微信输入法”。");
    expect(wrapper.text()).not.toContain("你选的输入工具");
    expect(mocks.getVoiceInputTool).toHaveBeenCalled();
    wrapper.unmount();
  });

  it("重启后落在⑤步但从未选过工具：退回④步补选（不给缺前置状态的一页做验证）", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue(null);
    const wrapper = await mountWizard(connectedRuntime(), { attach: true });
    await flushPromises();

    expect(wrapper.text()).toContain("选择你要用的输入工具");
    expect(mocks.saveOnboardingStep).toHaveBeenCalledWith("voice_tool");
    expect(
      onboardingEvents("action").some(
        (p) => p.reason === "tool_missing_fallback" && p.result === "failed",
      ),
    ).toBe(true);
    wrapper.unmount();
  });

  it("按语音键（会话开始）时也检查一次输入法（只读、不切换）", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue("doubao");
    queueSnapshots(runtimeWith(), streamingFrame({ decodedSamples: 80, submittedSamples: 100 }));

    const wrapper = await mountWizard(connectedRuntime(), { attach: true });
    await wrapper.find("input.onboarding-voice-input").trigger("focus");
    await flushPromises();
    mocks.getImeStatus.mockClear();

    await vi.advanceTimersByTimeAsync(200);
    await flushPromises();

    expect(mocks.getImeStatus).toHaveBeenCalled();
    wrapper.unmount();
  });

  it("accepts a late transcript after the failed 3s window（2026-10-13 报告：文字出现但无法继续；对齐 Mac 现值门禁）", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue("doubao");
    mocks.getVoiceHoldHotkey.mockResolvedValue({ keys: ["right_alt"] });
    // 生产现场：豆包语音测试时开关必然开着、报告层正常——显式给 active，让失败卡
    // 保留原排查文案（off/starting 会命中 2026-10-14 的分流提示）。
    mocks.getRc003BridgeSnapshot.mockResolvedValue(
      rc003BridgeSnapshot({ captureHealth: "active" }),
    );
    queueSnapshots(
      runtimeWith(),
      streamingFrame({ decodedSamples: 120, submittedSamples: 320 }),
      runtimeWith({
        connectionPhase: "ready",
        bleVoiceReady: true,
        rawInputPhase: "ready",
        audioPhase: "ready",
        decodedSamples: 160,
        submittedSamples: 640,
        queuedSamples: 0,
      }),
    );

    const wrapper = await mountWizard(connectedRuntime(), { attach: true });
    await vi.advanceTimersByTimeAsync(200); // 开始帧
    await vi.advanceTimersByTimeAsync(200); // 收尾帧 → waiting_transcript

    // 3 秒窗口内没有文字 → 超时失败（现场截图：文字行 "!"）。
    await vi.advanceTimersByTimeAsync(4_000);
    await flushPromises();
    expect(wrapper.text()).toContain("输入工具没有写出文字");

    // 文字迟到到达：观察窗口内没有物理键 → 翻案为通过（不是死路）。
    await wrapper.find("input.onboarding-voice-input").setValue("你好啊");
    await flushPromises();

    expect(wrapper.text()).toContain("成功：文字已经出现在输入框里");
    const continueButton = wrapper.find(".onboarding-actions .primary-button");
    expect(continueButton.attributes("data-gate-ready")).toBe("true");
    const late = onboardingEvents("voice_attempt").filter((p) => p.reason === "late_transcript");
    expect(late).toHaveLength(1);
    expect(late[0].result).toBe("passed");
    expect(String(late[0].detail)).toMatch(/^after_\d+$/);

    // 迟到接受同样要能继续（现场症状是"无法继续"）。
    await continueButton.trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("按一下遥控器的普通按键");
    wrapper.unmount();
  });

  it("still refuses late text when a physical key was counted（阴性对照不放松）", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue("doubao");
    mocks.getVoiceHoldHotkey.mockResolvedValue({ keys: ["right_alt"] });
    // 同上：显式 active，避免失败卡命中 2026-10-14 的捕获状态分流提示。
    mocks.getRc003BridgeSnapshot.mockResolvedValue(
      rc003BridgeSnapshot({ captureHealth: "active" }),
    );
    queueSnapshots(
      runtimeWith(),
      streamingFrame({ decodedSamples: 120, submittedSamples: 320 }),
      runtimeWith({
        connectionPhase: "ready",
        bleVoiceReady: true,
        rawInputPhase: "ready",
        audioPhase: "ready",
        decodedSamples: 160,
        submittedSamples: 640,
        queuedSamples: 0,
      }),
    );

    const wrapper = await mountWizard(connectedRuntime(), { attach: true });
    await vi.advanceTimersByTimeAsync(200);
    await vi.advanceTimersByTimeAsync(200);
    await vi.advanceTimersByTimeAsync(4_000);
    await flushPromises();
    expect(wrapper.text()).toContain("输入工具没有写出文字");

    // 迟到结算时报出 1 个物理键：这是用户自己打的字，保持失败。
    mocks.endKeyObservation.mockResolvedValueOnce(1);
    await wrapper.find("input.onboarding-voice-input").setValue("手打的字");
    await flushPromises();

    expect(wrapper.text()).toContain("输入工具没有写出文字");
    const continueButton = wrapper.find(".onboarding-actions .primary-button");
    expect(continueButton.attributes("data-gate-ready")).toBe("false");
    expect(
      onboardingEvents("voice_attempt").some((p) => p.reason === "late_transcript_manual"),
    ).toBe(true);
    expect(
      onboardingEvents("voice_attempt").filter((p) => p.reason === "late_transcript"),
    ).toHaveLength(0);
    wrapper.unmount();
  });

  it("sees text that lands in the box without a DOM input event（读回输入框现值）", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue("doubao");
    mocks.getVoiceHoldHotkey.mockResolvedValue({ keys: ["right_alt"] });
    queueSnapshots(
      runtimeWith(),
      streamingFrame({ decodedSamples: 120, submittedSamples: 320 }),
      runtimeWith({
        connectionPhase: "ready",
        bleVoiceReady: true,
        rawInputPhase: "ready",
        audioPhase: "ready",
        decodedSamples: 160,
        submittedSamples: 640,
        queuedSamples: 0,
      }),
    );

    const wrapper = await mountWizard(connectedRuntime(), { attach: true });
    await vi.advanceTimersByTimeAsync(200); // 开始帧
    await vi.advanceTimersByTimeAsync(200); // 收尾帧 → waiting_transcript

    // 文字进了输入框，但没有派发 DOM input 事件（本测试直接写 value，不 trigger）。
    const box = wrapper.find("input.onboarding-voice-input").element as HTMLInputElement;
    box.value = "你好啊";
    await flushPromises();

    await vi.advanceTimersByTimeAsync(200); // 下一帧轮询读回现值 → 通过
    await flushPromises();

    expect(box.value).toBe("你好啊");
    expect(wrapper.text()).toContain("成功：文字已经出现在输入框里");
    const continueButton = wrapper.find(".onboarding-actions .primary-button");
    expect(continueButton.attributes("data-gate-ready")).toBe("true");
    expect(
      onboardingEvents("voice_attempt").some((p) => p.reason === "transcript_value_readback"),
    ).toBe(true);
    wrapper.unmount();
  });

  it("accepts event-less late text through the late watch（失败后读回现值）", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue("doubao");
    mocks.getVoiceHoldHotkey.mockResolvedValue({ keys: ["right_alt"] });
    // 同上：显式 active，避免失败卡命中 2026-10-14 的捕获状态分流提示。
    mocks.getRc003BridgeSnapshot.mockResolvedValue(
      rc003BridgeSnapshot({ captureHealth: "active" }),
    );
    queueSnapshots(
      runtimeWith(),
      streamingFrame({ decodedSamples: 120, submittedSamples: 320 }),
      runtimeWith({
        connectionPhase: "ready",
        bleVoiceReady: true,
        rawInputPhase: "ready",
        audioPhase: "ready",
        decodedSamples: 160,
        submittedSamples: 640,
        queuedSamples: 0,
      }),
    );

    const wrapper = await mountWizard(connectedRuntime(), { attach: true });
    await vi.advanceTimersByTimeAsync(200);
    await vi.advanceTimersByTimeAsync(200);
    await vi.advanceTimersByTimeAsync(4_000);
    await flushPromises();
    expect(wrapper.text()).toContain("输入工具没有写出文字");

    const box = wrapper.find("input.onboarding-voice-input").element as HTMLInputElement;
    box.value = "你好啊"; // 迟到且没有 DOM 事件
    await vi.advanceTimersByTimeAsync(200); // 迟到监视帧读回现值
    await flushPromises();

    expect(wrapper.text()).toContain("成功：文字已经出现在输入框里");
    const continueButton = wrapper.find(".onboarding-actions .primary-button");
    expect(continueButton.attributes("data-gate-ready")).toBe("true");
    wrapper.unmount();
  });

  it("别的窗口盖上来时的窗口级 blur 不算失去输入目标（文字仍会落进输入框）", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue("doubao");
    queueSnapshots(runtimeWith(), streamingFrame({ decodedSamples: 80, submittedSamples: 100 }));

    const wrapper = await mountWizard(connectedRuntime(), { attach: true });
    const box = wrapper.find("input.onboarding-voice-input");
    await box.trigger("focus");
    await flushPromises();
    // 工具语音条/系统弹窗抢焦点：浏览器补一个 blur，但元素仍持有 DOM 焦点。
    await box.trigger("blur");
    await flushPromises();

    await vi.advanceTimersByTimeAsync(200);
    await flushPromises();

    // 不得据此判失败（2026-10-13 现场：文字已上屏却提示"输入框没有聚焦"）。
    expect(wrapper.text()).not.toContain("输入框没有聚焦");
    expect(wrapper.text()).not.toContain("失去了焦点");
    expect(
      onboardingEvents("voice_attempt").some((p) => p.reason === "voice.input_target_not_ready"),
    ).toBe(false);
    wrapper.unmount();
  });

  it("asks the user to click back into the box when a session starts unfocused", async () => {
    mocks.getOnboardingState.mockResolvedValue(activeState("voice_test"));
    mocks.getVoiceInputTool.mockResolvedValue("doubao");
    queueSnapshots(runtimeWith(), streamingFrame({ decodedSamples: 80, submittedSamples: 100 }));

    const wrapper = await mountWizard(connectedRuntime(), { attach: true });
    await (wrapper.find("input.onboarding-voice-input").element as HTMLInputElement).blur();
    await flushPromises();
    await vi.advanceTimersByTimeAsync(200);
    await flushPromises();

    expect(wrapper.text()).toContain("输入框没有聚焦");
    expect(
      onboardingEvents("voice_attempt").some((p) => p.reason === "voice.input_target_not_ready"),
    ).toBe(true);
    wrapper.unmount();
  });

  it("⑤步输入框聚焦时不显示焦点环（统一为细边框）", () => {
    // 2026-10-14 现场定稿：聚焦时只保留细边框——此前"第一次进来"（未用鼠标）显示焦点环、
    // "从上一页进来"被 body.pointer-nav 抑制，同一个聚焦状态两种外观；
    // 这里锁样式源码防回改（像素证据见 Bugs 记录）。
    // 跨平台：CI 检出可能是 CRLF、本地是 LF——先归一化行尾再匹配（2026-10-14 CI 实测教训）。
    const source = readFileSync(join(process.cwd(), "src/pages/OnboardingPage.vue"), "utf8")
      .replace(/\r\n/g, "\n");
    const block = source.match(
      /\.onboarding-voice-input:focus,\n\.onboarding-voice-input:focus-visible \{([\s\S]*?)\n\}/,
    );
    expect(block?.[1]).toBeDefined();
    expect(block?.[1]).toContain("outline: none");
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
    // 进入②步的 entry 扫描 + 手动点击的一次；两次都只带计数。
    expect(scan.map((p) => p.result)).toEqual(["unknown", "passed", "unknown", "passed"]);
    expect(scan[1].detail).toBe("entry_found_1");
    expect(scan[3].detail).toBe("manual_found_1");
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
