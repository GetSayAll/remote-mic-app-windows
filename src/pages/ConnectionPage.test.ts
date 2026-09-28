import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AudioEndpoint, AudioSnapshot, ConnectionSnapshot, RuntimeSnapshot } from "../lib/bridge";
import ConnectionPage from "./ConnectionPage.vue";

type ShortcutCaptureHandler = (edge: {
  key: string;
  isPressed: boolean;
  source?: "real" | "injected";
}) => void;

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
      staleRemoteEventCount: 0,
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
  captureEdgeHandler: null as ShortcutCaptureHandler | null,
  getConnectionSnapshot: vi.fn(),
  getAudioSnapshot: vi.fn(),
  listAudioEndpoints: vi.fn(),
  selectAudioEndpoint: vi.fn(),
  openVbCableDownloadPage: vi.fn(),
  getVoiceHoldHotkey: vi.fn(),
  setVoiceHoldHotkey: vi.fn(),
  startShortcutCapture: vi.fn(),
  stopShortcutCapture: vi.fn(),
  subscribeShortcutCaptureEdges: vi.fn(),
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
    subscribeShortcutCaptureEdges: mocks.subscribeShortcutCaptureEdges,
  };
});

describe("VB-CABLE first-launch guidance", () => {
  beforeEach(() => {
    mocks.endpoints = [];
    mocks.captureEdgeHandler = null;
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
    mocks.getVoiceHoldHotkey.mockResolvedValue({
      keys: ["left_control", "left_windows"],
    });
    mocks.setVoiceHoldHotkey.mockImplementation(async (hotkey) => hotkey);
    mocks.startShortcutCapture.mockResolvedValue([]);
    mocks.stopShortcutCapture.mockResolvedValue(undefined);
    mocks.subscribeShortcutCaptureEdges.mockImplementation(
      async (handler: ShortcutCaptureHandler) => {
        mocks.captureEdgeHandler = handler;
        return () => {};
      },
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

  it("recommends only the standard VB-Audio CABLE Input, never the other endpoints", async () => {
    const cableA: AudioEndpoint = {
      id: "cable-a",
      name: "CABLE-A Input (VB-Audio Cable A)",
      isVirtualCableCandidate: true,
    };
    const speaker: AudioEndpoint = {
      id: "speaker",
      name: "扬声器 (Realtek Audio)",
      isVirtualCableCandidate: false,
    };
    mocks.endpoints = [cableA, cableEndpoint, speaker];
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    // 多个候选端点时不自动选择，需要用户显式展开列表。
    expect(mocks.selectAudioEndpoint).not.toHaveBeenCalled();
    await wrapper
      .findAll("button")
      .find((button) => button.text() === "选择设备")!
      .trigger("click");
    await flushPromises();

    const marks = wrapper.findAll(".endpoint-list .endpoint-recommend");
    expect(marks).toHaveLength(1);
    expect(marks[0].text()).toBe("推荐");

    const items = wrapper.findAll(".endpoint-list li");
    expect(items).toHaveLength(3);
    expect(items[0].text()).toContain("CABLE-A Input");
    expect(items[1].text()).toContain("CABLE Input (VB-Audio Virtual Cable)");
    expect(items[2].text()).toContain("扬声器");
    expect(items[0].text()).not.toContain("推荐");
    expect(items[2].text()).not.toContain("推荐");
    expect(items[0].text()).toContain("其他音频设备");
    expect(items[2].text()).toContain("其他音频设备");
    wrapper.unmount();
  });

  it("keeps a lone 右 Alt in the draft until the user explicitly saves it", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".voice-hotkey-presets button")
      .find((button) => button.text() === "修改快捷键")!
      .trigger("click");
    await flushPromises();

    mocks.captureEdgeHandler!({ key: "right_alt", isPressed: true });
    await flushPromises();
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();
    expect(
      wrapper
        .findAll(".voice-hotkey-capture-actions button")
        .find((button) => button.text() === "保存")!
        .attributes("disabled"),
    ).toBeDefined();

    mocks.captureEdgeHandler!({ key: "right_alt", isPressed: false });
    await new Promise((resolve) => setTimeout(resolve, 260));
    await flushPromises();
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();
    expect(wrapper.find(".voice-hotkey-capture").text()).toContain("右 Alt");
    expect(
      wrapper
        .findAll(".voice-hotkey-capture-actions button")
        .find((button) => button.text() === "保存")!
        .attributes("disabled"),
    ).toBeUndefined();

    await wrapper
      .findAll(".voice-hotkey-capture-actions button")
      .find((button) => button.text() === "保存")!
      .trigger("click");
    await flushPromises();
    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({ keys: ["right_alt"] });
    expect(wrapper.text()).toContain("按住说话快捷键已设为 右 Alt");
    // 默认项与关闭项仍在列，误录可一键回退。
    const presetTexts = wrapper
      .findAll(".voice-hotkey-presets button")
      .map((button) => button.text());
    expect(presetTexts).toContain("左 Ctrl + 左 Win（默认）");
    expect(presetTexts).toContain("关闭");
    wrapper.unmount();
  });

  it("cancels capture with Esc and keeps the current hotkey", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".voice-hotkey-presets button")
      .find((button) => button.text() === "修改快捷键")!
      .trigger("click");
    await flushPromises();

    mocks.captureEdgeHandler!({ key: "escape", isPressed: true });
    await flushPromises();
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();
    expect(mocks.stopShortcutCapture).toHaveBeenCalledOnce();
    expect(wrapper.find(".voice-hotkey-capture").exists()).toBe(false);
    expect(wrapper.text()).toContain("已取消录入");
    wrapper.unmount();
  });

  it("keeps 左 Ctrl + 左 Win as the default hold-to-talk hotkey", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    const defaultButton = wrapper
      .findAll(".voice-hotkey-presets button")
      .find((button) => button.text().includes("默认"))!;
    expect(defaultButton.text()).toBe("左 Ctrl + 左 Win（默认）");
    expect(defaultButton.classes()).toContain("primary-button");
    expect(defaultButton.attributes("disabled")).toBeDefined();
    expect(wrapper.text()).toContain("默认快捷键：左 Ctrl + 左 Win");
  });

  it("accumulates Ctrl and Win across separate presses and saves only on explicit confirmation", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".voice-hotkey-presets button")
      .find((button) => button.text() === "修改快捷键")!
      .trigger("click");
    await flushPromises();
    expect(mocks.startShortcutCapture).toHaveBeenCalledOnce();
    expect(mocks.captureEdgeHandler).not.toBeNull();
    expect(wrapper.find(".voice-hotkey-capture").text()).toContain("请按下快捷键");
    expect(wrapper.find(".voice-hotkey-recorder").text()).toContain(
      "也可以逐个轻按每个键",
    );

    // 微信输入法可能吞掉同时按下的完成键。先只到达 Ctrl，松开后不得保存半截。
    mocks.captureEdgeHandler!({ key: "left_control", isPressed: true, source: "real" });
    mocks.captureEdgeHandler!({ key: "left_control", isPressed: false, source: "real" });
    await flushPromises();
    expect(wrapper.find(".voice-hotkey-capture").text()).toContain("左 Ctrl");
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();

    // 用户在同一录入器里再轻按一次缺少的 Win，草稿合并为完整组合。
    mocks.captureEdgeHandler!({ key: "left_windows", isPressed: true, source: "real" });
    mocks.captureEdgeHandler!({ key: "left_windows", isPressed: false, source: "real" });
    await flushPromises();
    expect(wrapper.find(".voice-hotkey-capture").text()).toContain("左 Ctrl + 左 Win");
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();
    expect(mocks.stopShortcutCapture).not.toHaveBeenCalled();

    await wrapper
      .findAll(".voice-hotkey-capture-actions button")
      .find((button) => button.text() === "保存")!
      .trigger("click");
    await flushPromises();

    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({
      keys: ["left_control", "left_windows"],
    });
    expect(mocks.stopShortcutCapture).toHaveBeenCalledOnce();
    expect(wrapper.find(".voice-hotkey-capture").exists()).toBe(false);
    expect(wrapper.text()).toContain("按住说话快捷键已设为 左 Ctrl + 左 Win");
    wrapper.unmount();
  });

  it("asks to release pre-held keys first and only updates the draft after arming", async () => {
    // 2026-09-27 回归：录入开始时仍有按键按住（preheld），其边沿对录入
    // 不可见；后端等 preheld 全部松开后才投递边沿，前端先提示松手，
    // 杜绝把新组合截断成半截（"只剩左 Ctrl"）。
    mocks.startShortcutCapture.mockResolvedValue(["left_windows"]);
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".voice-hotkey-presets button")
      .find((button) => button.text() === "修改快捷键")!
      .trigger("click");
    await flushPromises();
    expect(wrapper.find(".voice-hotkey-capture").text()).toContain("请先松开所有按键");
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();

    // 后端武装后的第一条边沿：提示清除，正常进入录入。
    mocks.captureEdgeHandler!({ key: "left_control", isPressed: true });
    await flushPromises();
    expect(wrapper.find(".voice-hotkey-capture").text()).not.toContain("请先松开所有按键");

    mocks.captureEdgeHandler!({ key: "left_windows", isPressed: true });
    mocks.captureEdgeHandler!({ key: "left_control", isPressed: false });
    await flushPromises();
    // 组合未全部松开前不落盘（此处 Win 是松手后重新按下的新鲜按键）。
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();

    mocks.captureEdgeHandler!({ key: "left_windows", isPressed: false });
    await flushPromises();
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();
    expect(wrapper.find(".voice-hotkey-capture").text()).toContain("左 Ctrl + 左 Win");

    await wrapper
      .findAll(".voice-hotkey-capture-actions button")
      .find((button) => button.text() === "保存")!
      .trigger("click");
    await flushPromises();
    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({
      keys: ["left_control", "left_windows"],
    });
    wrapper.unmount();
  });

  it("keeps every modifier in the draft when keys are released out of order", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".voice-hotkey-presets button")
      .find((button) => button.text() === "修改快捷键")!
      .trigger("click");
    await flushPromises();

    // 回归（Bugs/2026-09-27）：左 Ctrl + 左 Win 先松 Win、后松 Ctrl，
    // 不能把组合截断成只剩 Ctrl。
    mocks.captureEdgeHandler!({ key: "left_control", isPressed: true });
    mocks.captureEdgeHandler!({ key: "left_windows", isPressed: true });
    mocks.captureEdgeHandler!({ key: "left_windows", isPressed: false });
    await flushPromises();
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();

    mocks.captureEdgeHandler!({ key: "left_control", isPressed: false });
    await new Promise((resolve) => setTimeout(resolve, 260));
    await flushPromises();
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();
    expect(wrapper.find(".voice-hotkey-capture").text()).toContain("左 Ctrl + 左 Win");

    await wrapper
      .findAll(".voice-hotkey-capture-actions button")
      .find((button) => button.text() === "保存")!
      .trigger("click");
    await flushPromises();
    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({
      keys: ["left_control", "left_windows"],
    });
    expect(mocks.stopShortcutCapture).toHaveBeenCalledOnce();
    expect(wrapper.text()).toContain("按住说话快捷键已设为 左 Ctrl + 左 Win");
    wrapper.unmount();
  });

  it("merges a replayed injected key into the draft without auto-saving", async () => {
    // 2026-09-27 真机回归（Bugs/2026-09-27-ime-chord-hook-eats-active-hotkey-capture.md）：
    // 微信输入法的语音和弦就是默认的 左 Ctrl + 左 Win。按下该组合时它吞掉
    // 左 Win 的物理边沿、随后把整个组合以注入副本重放——真实到达后端的只有
    // 左 Ctrl 的真实边沿 + 左 Win 的注入副本，且副本可能晚于物理松开。
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".voice-hotkey-presets button")
      .find((button) => button.text() === "修改快捷键")!
      .trigger("click");
    await flushPromises();

    // 物理按下：只有左 Ctrl 的真实边沿（左 Win 被输入法吞掉）。
    mocks.captureEdgeHandler!({ key: "left_control", isPressed: true, source: "real" });
    // 物理松开：左 Ctrl 的真实抬起先到——此时组合看似"只剩左 Ctrl"。
    mocks.captureEdgeHandler!({ key: "left_control", isPressed: false, source: "real" });
    await flushPromises();
    // 稳定窗口内不得提前落盘（否则就是"只剩左 Ctrl"）。
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();

    // 输入法重放的注入副本晚到：完整的两键组合。
    mocks.captureEdgeHandler!({ key: "left_control", isPressed: true, source: "injected" });
    mocks.captureEdgeHandler!({ key: "left_windows", isPressed: true, source: "injected" });
    mocks.captureEdgeHandler!({ key: "left_windows", isPressed: false, source: "injected" });
    mocks.captureEdgeHandler!({ key: "left_control", isPressed: false, source: "injected" });
    await new Promise((resolve) => setTimeout(resolve, 260));
    await flushPromises();
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();
    expect(wrapper.find(".voice-hotkey-capture").text()).toContain("左 Ctrl + 左 Win");

    await wrapper
      .findAll(".voice-hotkey-capture-actions button")
      .find((button) => button.text() === "保存")!
      .trigger("click");
    await flushPromises();

    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({
      keys: ["left_control", "left_windows"],
    });
    expect(wrapper.text()).toContain("按住说话快捷键已设为 左 Ctrl + 左 Win");
    wrapper.unmount();
  });

  it("clears the current draft without ending the capture session", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".voice-hotkey-presets button")
      .find((button) => button.text() === "修改快捷键")!
      .trigger("click");
    await flushPromises();

    mocks.captureEdgeHandler!({ key: "right_alt", isPressed: true });
    mocks.captureEdgeHandler!({ key: "right_alt", isPressed: false });
    await flushPromises();
    expect(wrapper.find(".voice-hotkey-capture").text()).toContain("右 Alt");

    await wrapper
      .findAll(".voice-hotkey-capture-actions button")
      .find((button) => button.text() === "清空")!
      .trigger("click");
    await flushPromises();
    expect(wrapper.find(".voice-hotkey-capture").text()).toContain("请按下快捷键");
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();
    expect(mocks.stopShortcutCapture).not.toHaveBeenCalled();
    wrapper.unmount();
  });

  it("does not infer or save a half chord even when WeType voice was observed", async () => {
    mocks.stopShortcutCapture.mockResolvedValue({ wetypeVoice: "observed" });
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".voice-hotkey-presets button")
      .find((button) => button.text() === "修改快捷键")!
      .trigger("click");
    await flushPromises();

    // 只有左 Ctrl 的真实边沿到达（左 Win 被吞，半截会话）。
    mocks.captureEdgeHandler!({ key: "left_control", isPressed: true, source: "real" });
    mocks.captureEdgeHandler!({ key: "left_control", isPressed: false, source: "real" });
    await flushPromises();
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();

    await wrapper
      .findAll(".voice-hotkey-capture-actions button")
      .find((button) => button.text() === "取消")!
      .trigger("click");
    await flushPromises();
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("已取消录入");
    wrapper.unmount();
  });

  it("warns that a non-WeType chord will not trigger hold-to-talk voice", async () => {
    // Win + 右 Ctrl 不是微信输入法热键：边沿透传能录上，但按住说话靠注入该
    // 组合唤起微信输入法语音，不一致就无法生效——必须把这一后果告诉用户。
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".voice-hotkey-presets button")
      .find((button) => button.text() === "修改快捷键")!
      .trigger("click");
    await flushPromises();

    mocks.captureEdgeHandler!({ key: "right_windows", isPressed: true });
    mocks.captureEdgeHandler!({ key: "right_control", isPressed: true });
    mocks.captureEdgeHandler!({ key: "right_control", isPressed: false });
    mocks.captureEdgeHandler!({ key: "right_windows", isPressed: false });
    await flushPromises();
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();

    await wrapper
      .findAll(".voice-hotkey-capture-actions button")
      .find((button) => button.text() === "保存")!
      .trigger("click");
    await flushPromises();

    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({
      keys: ["right_windows", "right_control"],
    });
    expect(wrapper.text()).toContain("已设为 右 Win + 右 Ctrl");
    expect(wrapper.text()).toContain("若与微信输入法语音键不一致将无法生效");
    wrapper.unmount();
  });

  it("rejects the reserved Win+L combination instead of saving a lock-screen shortcut", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".voice-hotkey-presets button")
      .find((button) => button.text() === "修改快捷键")!
      .trigger("click");
    await flushPromises();

    mocks.captureEdgeHandler!({ key: "left_windows", isPressed: true });
    mocks.captureEdgeHandler!({ key: "l", isPressed: true });
    mocks.captureEdgeHandler!({ key: "l", isPressed: false });
    mocks.captureEdgeHandler!({ key: "left_windows", isPressed: false });
    await flushPromises();

    await wrapper
      .findAll(".voice-hotkey-capture-actions button")
      .find((button) => button.text() === "保存")!
      .trigger("click");
    await flushPromises();

    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();
    expect(wrapper.find(".voice-hotkey-recorder").exists()).toBe(true);
    expect(wrapper.text()).toContain("Win + L 是 Windows 保留组合");
    wrapper.unmount();
  });

  it("rejects Ctrl+Alt+Delete as a Windows security combination", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".voice-hotkey-presets button")
      .find((button) => button.text() === "修改快捷键")!
      .trigger("click");
    await flushPromises();

    for (const key of ["left_control", "right_alt", "delete"]) {
      mocks.captureEdgeHandler!({ key, isPressed: true });
      mocks.captureEdgeHandler!({ key, isPressed: false });
    }
    await flushPromises();

    await wrapper
      .findAll(".voice-hotkey-capture-actions button")
      .find((button) => button.text() === "保存")!
      .trigger("click");
    await flushPromises();

    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("Ctrl + Alt + Delete 是 Windows 安全组合");
    wrapper.unmount();
  });
});
