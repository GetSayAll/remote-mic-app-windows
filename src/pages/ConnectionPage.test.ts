import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AudioEndpoint, AudioSnapshot, ConnectionSnapshot, RuntimeSnapshot } from "../lib/bridge";
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

const mocks = vi.hoisted(() => ({
  endpoints: [] as AudioEndpoint[],
  getConnectionSnapshot: vi.fn(),
  getAudioSnapshot: vi.fn(),
  listAudioEndpoints: vi.fn(),
  selectAudioEndpoint: vi.fn(),
  openVbCableDownloadPage: vi.fn(),
  getVoiceHoldHotkey: vi.fn(),
  setVoiceHoldHotkey: vi.fn(),
  startShortcutCapture: vi.fn(),
  stopShortcutCapture: vi.fn(),
  shortcutCaptureHandlers: [] as Array<(edge: { key: string; isPressed: boolean }) => void>,
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
    getVoiceHoldHotkey: mocks.getVoiceHoldHotkey,
    setVoiceHoldHotkey: mocks.setVoiceHoldHotkey,
    startShortcutCapture: mocks.startShortcutCapture,
    stopShortcutCapture: mocks.stopShortcutCapture,
    subscribeShortcutCaptureEdges: vi.fn(
      (handler: (edge: { key: string; isPressed: boolean }) => void) => {
        mocks.shortcutCaptureHandlers.push(handler);
        return Promise.resolve(() => {
          mocks.shortcutCaptureHandlers = mocks.shortcutCaptureHandlers.filter(
            (registered) => registered !== handler,
          );
        });
      },
    ),
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
    mocks.getVoiceHoldHotkey.mockResolvedValue(null);
    mocks.setVoiceHoldHotkey.mockImplementation(async (hotkey: { keys: string[] } | null) =>
      hotkey,
    );
    mocks.startShortcutCapture.mockResolvedValue(undefined);
    mocks.stopShortcutCapture.mockResolvedValue(undefined);
    mocks.shortcutCaptureHandlers = [];
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

describe("hold-to-talk hotkey presets and custom capture", () => {
  function pressKey(code: string): void {
    window.dispatchEvent(new KeyboardEvent("keydown", { code, cancelable: true }));
  }

  function releaseKey(code: string): void {
    window.dispatchEvent(new KeyboardEvent("keyup", { code, cancelable: true }));
  }

  beforeEach(() => {
    mocks.getConnectionSnapshot.mockResolvedValue(emptyConnection);
    mocks.getAudioSnapshot.mockResolvedValue(emptyAudio);
    mocks.listAudioEndpoints.mockResolvedValue([]);
    mocks.getVoiceHoldHotkey.mockResolvedValue(null);
    mocks.setVoiceHoldHotkey.mockImplementation(async (hotkey: { keys: string[] } | null) =>
      hotkey,
    );
    mocks.startShortcutCapture.mockResolvedValue(undefined);
    mocks.stopShortcutCapture.mockResolvedValue(undefined);
    mocks.shortcutCaptureHandlers = [];
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("saves the Windows voice-typing preset in one click", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    const preset = wrapper
      .findAll(".voice-hotkey-presets button")
      .find((button) => button.text().includes("Windows 听写"));
    expect(preset).toBeTruthy();
    await preset!.trigger("click");
    await flushPromises();

    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({ keys: ["left_windows", "h"] });
    expect(wrapper.text()).toContain("按住说话快捷键已设为");
    wrapper.unmount();
  });

  it("captures and saves a custom hotkey after all keys are released", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper.get(".custom-shortcut-row .chip").trigger("click");
    await flushPromises();
    expect(mocks.startShortcutCapture).toHaveBeenCalledOnce();
    expect(wrapper.text()).toContain("请按下快捷键组合");

    pressKey("MetaLeft");
    pressKey("KeyH");
    releaseKey("KeyH");
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();
    releaseKey("MetaLeft");
    await flushPromises();

    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({ keys: ["left_windows", "h"] });
    expect(mocks.stopShortcutCapture).toHaveBeenCalled();
    expect(wrapper.text()).toContain("按住说话快捷键已设为");
    wrapper.unmount();
  });

  it("cancels the capture with Escape without touching the saved hotkey", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper.get(".custom-shortcut-row .chip").trigger("click");
    await flushPromises();

    pressKey("Escape");
    await flushPromises();

    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();
    expect(mocks.stopShortcutCapture).toHaveBeenCalled();
    expect(wrapper.text()).toContain("已取消录入，快捷键未修改");
    wrapper.unmount();
  });

  it("supports safe capture mode with on-screen modifiers and native edges", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper.get(".safe-capture-toggle input").setValue(true);
    await wrapper.get(".custom-shortcut-row .chip").trigger("click");
    await flushPromises();

    const leftWin = wrapper
      .findAll(".preset-grid .chip")
      .find((button) => button.text() === "左 Win");
    expect(leftWin).toBeTruthy();
    await leftWin!.trigger("click");
    expect(wrapper.text()).toContain("左 Win");

    // 原生钩子边沿（shortcut-capture-edge 事件源）也应驱动同一状态机。
    const edges = mocks.shortcutCaptureHandlers[0];
    expect(edges).toBeTruthy();
    edges!({ key: "h", isPressed: true });
    edges!({ key: "h", isPressed: false });
    await flushPromises();

    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({ keys: ["left_windows", "h"] });
    expect(mocks.stopShortcutCapture).toHaveBeenCalled();
    wrapper.unmount();
  });
});
