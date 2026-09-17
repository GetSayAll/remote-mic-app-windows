import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AudioEndpoint,
  AudioSnapshot,
  ConnectionSnapshot,
  RuntimeSnapshot,
  VoiceTarget,
  VoiceTargetSnapshot,
} from "../lib/bridge";
import ConnectionPage from "./ConnectionPage.vue";

const emptyConnection: ConnectionSnapshot = {
  phase: "idle",
  remoteName: null,
  remoteModel: "unknown",
  capabilities: null,
  voiceState: "idle",
  decodedSamples: 0,
  generation: 0,
  reconnectAttempt: 0,
  powerNotificationsAvailable: false,
  lastError: null,
};

const emptyAudio: AudioSnapshot = {
  phase: "unconfigured",
  selectedEndpointId: null,
  selectedEndpointName: null,
  queuedSamples: 0,
  submittedSamples: 0,
  generation: 0,
  lastError: null,
};

const runtime: RuntimeSnapshot = {
  appVersion: "0.1.0",
  platform: {
    platform: "windows",
    windowsApiAvailable: true,
    bleScanAvailable: true,
    bleVoiceReady: false,
    wasapiReady: false,
    rawInputReady: false,
    sendInputReady: true,
    verificationStatus: "测试",
    connection: emptyConnection,
    audio: emptyAudio,
    rawInput: {
      phase: "stopped",
      matchedDeviceCount: 0,
      rawEventCount: 0,
      semanticEdgeCount: 0,
      lastButton: null,
      lastIsPressed: null,
      activeButtons: [],
      lastError: null,
    },
    buttonMapping: {
      enabled: true,
      gateActive: false,
      listenerActive: false,
      swallowedEdges: 0,
      leakedDowns: 0,
      firedGestures: 0,
      lastFired: null,
      lastError: null,
    },
  },
};

const cableEndpoint: AudioEndpoint = {
  id: "cable-input",
  name: "CABLE Input (VB-Audio Virtual Cable)",
  isVirtualCableCandidate: true,
};

/** 微信输入法基线（多数用例的语音目标起点）。 */
const weTypeVoiceTarget: VoiceTargetSnapshot = {
  target: "we_type",
  hotkey: null,
  enabled: true,
  doubaoMode: "hold",
  resolvedHotkey: { keys: ["left_control", "left_windows"] },
  defaultHotkey: { keys: ["left_control", "left_windows"] },
  supportsSessionActivation: true,
  injectionShape: "hold_while_key_down",
};

const mocks = vi.hoisted(() => ({
  endpoints: [] as AudioEndpoint[],
  getConnectionSnapshot: vi.fn(),
  getAudioSnapshot: vi.fn(),
  listAudioEndpoints: vi.fn(),
  selectAudioEndpoint: vi.fn(),
  openVbCableDownloadPage: vi.fn(),
  getVoiceTargetConfig: vi.fn(),
  setVoiceTargetConfig: vi.fn(),
}));

vi.mock("../lib/bridge", async (importOriginal) => {
  const original = await importOriginal<typeof import("../lib/bridge")>();
  return {
    ...original,
    getConnectionSnapshot: mocks.getConnectionSnapshot,
    getAudioSnapshot: mocks.getAudioSnapshot,
    listAudioEndpoints: mocks.listAudioEndpoints,
    selectAudioEndpoint: mocks.selectAudioEndpoint,
    openVbCableDownloadPage: mocks.openVbCableDownloadPage,
    getVoiceTargetConfig: mocks.getVoiceTargetConfig,
    setVoiceTargetConfig: mocks.setVoiceTargetConfig,
  };
});

describe("VB-CABLE first-launch guidance", () => {
  beforeEach(() => {
    mocks.endpoints = [];
    mocks.getConnectionSnapshot.mockResolvedValue(emptyConnection);
    mocks.getAudioSnapshot.mockResolvedValue(emptyAudio);
    mocks.listAudioEndpoints.mockImplementation(async () => mocks.endpoints);
    mocks.selectAudioEndpoint.mockImplementation(async (endpointId: string) => ({
      ...emptyAudio,
      phase: "ready",
      selectedEndpointId: endpointId,
      selectedEndpointName: cableEndpoint.name,
    }));
    mocks.openVbCableDownloadPage.mockResolvedValue(undefined);
    mocks.getVoiceTargetConfig.mockResolvedValue({ ...weTypeVoiceTarget });
    mocks.setVoiceTargetConfig.mockImplementation(
      async (target: VoiceTarget, hotkey: unknown, enabled: boolean, doubaoMode?: string) => ({
        ...weTypeVoiceTarget,
        target,
        hotkey,
        enabled,
        doubaoMode: doubaoMode ?? weTypeVoiceTarget.doubaoMode,
      }),
    );
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("groups each status dot with its heading for vertical alignment", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    const headings = wrapper.findAll(".status-heading");
    expect(headings).toHaveLength(2);
    for (const heading of headings) {
      expect(heading.find(".status-dot").exists()).toBe(true);
      expect(heading.find("strong").exists()).toBe(true);
    }
    wrapper.unmount();
  });

  it("automatically selects the only VB-CABLE endpoint when no endpoint was configured", async () => {
    mocks.endpoints = [cableEndpoint];
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    expect(mocks.selectAudioEndpoint).toHaveBeenCalledOnce();
    expect(mocks.selectAudioEndpoint).toHaveBeenCalledWith(cableEndpoint.id);
    expect(wrapper.text()).toContain("已自动选择 CABLE Input");
    expect(wrapper.text()).not.toContain("需要安装 VB-CABLE");
    expect(wrapper.text()).not.toContain("系统语音输入");
    wrapper.unmount();
  });

  it("waits for the saved endpoint and does not replace an existing selection", async () => {
    const savedAudio: AudioSnapshot = {
      ...emptyAudio,
      phase: "ready",
      selectedEndpointId: "saved-speaker",
      selectedEndpointName: "已保存的扬声器",
    };
    let resolveAudio: ((snapshot: AudioSnapshot) => void) | undefined;
    mocks.endpoints = [cableEndpoint];
    mocks.getAudioSnapshot.mockImplementationOnce(
      () =>
        new Promise<AudioSnapshot>((resolve) => {
          resolveAudio = resolve;
        }),
    );

    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();
    expect(mocks.listAudioEndpoints).not.toHaveBeenCalled();

    resolveAudio?.(savedAudio);
    await flushPromises();

    expect(mocks.listAudioEndpoints).toHaveBeenCalledOnce();
    expect(mocks.selectAudioEndpoint).not.toHaveBeenCalled();
    wrapper.unmount();
  });

  it("shows the official installation action when VB-CABLE is unavailable", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    expect(mocks.selectAudioEndpoint).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("需要安装 VB-CABLE");
    expect(wrapper.text()).toContain("完成后需重启电脑");

    await wrapper.get(".vb-cable-callout .primary-button").trigger("click");
    await flushPromises();
    expect(mocks.openVbCableDownloadPage).toHaveBeenCalledOnce();
    wrapper.unmount();
  });
});

describe("豆包语音模式选择", () => {
  const doubaoSnapshot: VoiceTargetSnapshot = {
    target: "doubao",
    hotkey: null,
    enabled: true,
    doubaoMode: "hold",
    resolvedHotkey: { keys: ["right_alt"] },
    defaultHotkey: { keys: ["right_alt"] },
    supportsSessionActivation: true,
    injectionShape: "hold_while_key_down",
  };

  beforeEach(() => {
    mocks.endpoints = [];
    mocks.getConnectionSnapshot.mockResolvedValue(emptyConnection);
    mocks.getAudioSnapshot.mockResolvedValue(emptyAudio);
    mocks.listAudioEndpoints.mockImplementation(async () => []);
    mocks.openVbCableDownloadPage.mockResolvedValue(undefined);
    mocks.getVoiceTargetConfig.mockResolvedValue({ ...weTypeVoiceTarget });
    mocks.setVoiceTargetConfig.mockImplementation(
      async (target: VoiceTarget, hotkey: unknown, enabled: boolean, doubaoMode?: string) => ({
        ...doubaoSnapshot,
        target,
        hotkey,
        enabled,
        doubaoMode: doubaoMode ?? doubaoSnapshot.doubaoMode,
      }),
    );
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("仅在选中豆包输入法时显示模式选择", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();
    expect(wrapper.find(".doubao-mode-block").exists()).toBe(false);

    const doubaoButton = wrapper
      .findAll(".voice-hotkey-presets button")
      .find((button) => button.text() === "豆包输入法");
    await doubaoButton!.trigger("click");
    await flushPromises();

    const block = wrapper.get(".doubao-mode-block");
    expect(block.text()).toContain("长按模式");
    expect(block.text()).toContain("免按模式");
    wrapper.unmount();
  });

  it("切到免按模式时把 doubaoMode 传给后端并提示松手不会结束", async () => {
    mocks.getVoiceTargetConfig.mockResolvedValue({ ...doubaoSnapshot });
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    const handsFree = wrapper
      .get(".doubao-mode-block")
      .findAll("button")
      .find((button) => button.text() === "免按模式");
    await handsFree!.trigger("click");
    await flushPromises();

    expect(mocks.setVoiceTargetConfig).toHaveBeenCalledWith("doubao", null, true, "handsfree");
    expect(wrapper.get(".doubao-mode-block").text()).toContain("再按一下结束");
    wrapper.unmount();
  });

  // 回归防护：后端 `doubao_mode` 缺省会落回 `hold`。若 `applyVoiceTarget` /
  // `setVoiceInjectionEnabled` 不透传当前模式，用户选好的免按模式会在每次
  // 切换输入法或开关注入时被静默重置。
  it("切换输入法与开关注入时保留已选的豆包模式", async () => {
    mocks.getVoiceTargetConfig.mockResolvedValue({
      ...doubaoSnapshot,
      doubaoMode: "handsfree",
    });
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    const weTypeButton = wrapper
      .findAll(".voice-hotkey-presets button")
      .find((button) => button.text() === "微信输入法");
    await weTypeButton!.trigger("click");
    await flushPromises();

    expect(mocks.setVoiceTargetConfig).toHaveBeenCalledWith("we_type", null, true, "handsfree");
    wrapper.unmount();
  });
});
