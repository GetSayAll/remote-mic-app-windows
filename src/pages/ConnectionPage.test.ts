import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AudioEndpoint, AudioSnapshot, ConnectionSnapshot, RuntimeSnapshot } from "../lib/bridge";
import ConnectionPage from "./ConnectionPage.vue";

type ShortcutCaptureHandler = (edge: {
  key: string;
  isPressed: boolean;
  source?: "real" | "injected";
}) => void;

/**
 * 落盘稳定窗口（200ms）之后才定稿：外部钩子吞掉完成键的物理边沿后会以注入
 * 副本重放整个组合，副本可能晚于物理松开到达（见 ConnectionPage 的
 * scheduleVoiceCaptureFinish），因此断言落盘前必须等过该窗口。
 */
async function settleVoiceCapture(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 260));
  await flushPromises();
}

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

  it("accepts a lone modifier as the hold-to-talk hotkey (长按右 Alt 一类)", async () => {
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

    mocks.captureEdgeHandler!({ key: "right_alt", isPressed: false });
    await settleVoiceCapture();
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

  it("records a custom hold-to-talk chord and only saves it after every key is released", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".voice-hotkey-presets button")
      .find((button) => button.text() === "修改快捷键")!
      .trigger("click");
    await flushPromises();
    expect(mocks.startShortcutCapture).toHaveBeenCalledOnce();
    expect(mocks.captureEdgeHandler).not.toBeNull();
    expect(wrapper.find(".voice-hotkey-capture").text()).toContain(
      "请按下微信输入法当前设置的语音键",
    );

    mocks.captureEdgeHandler!({ key: "right_alt", isPressed: true });
    mocks.captureEdgeHandler!({ key: "d", isPressed: true });
    await flushPromises();
    expect(wrapper.find(".voice-hotkey-capture").text()).toContain("右 Alt + D");
    // 物理键未松开前不落盘：Win+L 一类组合不会在录入中提前生效。
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();

    mocks.captureEdgeHandler!({ key: "d", isPressed: false });
    expect(mocks.stopShortcutCapture).not.toHaveBeenCalled();
    mocks.captureEdgeHandler!({ key: "right_alt", isPressed: false });
    await settleVoiceCapture();

    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({ keys: ["right_alt", "d"] });
    expect(mocks.stopShortcutCapture).toHaveBeenCalledOnce();
    expect(wrapper.find(".voice-hotkey-capture").exists()).toBe(false);
    expect(wrapper.text()).toContain("按住说话快捷键已设为 右 Alt + D");
    wrapper.unmount();
  });

  it("asks to release pre-held keys first and saves the full chord after arming", async () => {
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
    await settleVoiceCapture();
    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({
      keys: ["left_control", "left_windows"],
    });
    wrapper.unmount();
  });

  it("keeps every modifier of a modifier-only chord when keys are released out of order", async () => {
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
    await settleVoiceCapture();
    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({
      keys: ["left_control", "left_windows"],
    });
    expect(mocks.stopShortcutCapture).toHaveBeenCalledOnce();
    expect(wrapper.text()).toContain("按住说话快捷键已设为 左 Ctrl + 左 Win");
    wrapper.unmount();
  });

  it("saves the full chord when the swallowed key only arrives as a replayed injected copy", async () => {
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
    await settleVoiceCapture();

    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({
      keys: ["left_control", "left_windows"],
    });
    expect(wrapper.text()).toContain("按住说话快捷键已设为 左 Ctrl + 左 Win");
    wrapper.unmount();
  });

  it("keeps the whole modifier-only chord when the first-pressed modifier is released last", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".voice-hotkey-presets button")
      .find((button) => button.text() === "修改快捷键")!
      .trigger("click");
    await flushPromises();

    mocks.captureEdgeHandler!({ key: "left_windows", isPressed: true });
    mocks.captureEdgeHandler!({ key: "left_control", isPressed: true });
    mocks.captureEdgeHandler!({ key: "left_control", isPressed: false });
    mocks.captureEdgeHandler!({ key: "left_windows", isPressed: false });
    await settleVoiceCapture();

    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({
      keys: ["left_windows", "left_control"],
    });
    expect(wrapper.text()).toContain("按住说话快捷键已设为 左 Win + 左 Ctrl");
    // 默认组合不需要"与微信输入法语音键不一致"的提醒（顺序无关比较）。
    expect(wrapper.text()).not.toContain("不一致将无法生效");
    wrapper.unmount();
  });

  it("infers the WeType chord when only one edge survived but WeType voice was triggered", async () => {
    // 2026-09-27 探针结论：微信输入法吞掉其语音热键组成键的物理边沿发生在
    // RIT 层，对本进程零/半截边沿（低级钩子、Raw Input、GetAsyncKeyState 都
    // 看不到）；其麦克风在录入期间被触发（observed）是唯一旁证 → 推断用户按
    // 的就是微信输入法语音热键，落盘产品默认组合并说明原因。
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
    await settleVoiceCapture();

    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({
      keys: ["left_control", "left_windows"],
    });
    expect(wrapper.text()).toContain("已按微信输入法语音键默认值 左 Ctrl + 左 Win");
    wrapper.unmount();
  });

  it("keeps a lone modifier when WeType voice was not triggered", async () => {
    // 单修饰键（豆包"长按右 Alt"一类）合法：微信输入法语音未被触发时不得推断。
    mocks.stopShortcutCapture.mockResolvedValue({ wetypeVoice: "not_observed" });
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".voice-hotkey-presets button")
      .find((button) => button.text() === "修改快捷键")!
      .trigger("click");
    await flushPromises();

    mocks.captureEdgeHandler!({ key: "right_alt", isPressed: true });
    mocks.captureEdgeHandler!({ key: "right_alt", isPressed: false });
    await settleVoiceCapture();

    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({ keys: ["right_alt"] });
    expect(wrapper.text()).not.toContain("已按微信输入法语音键默认值");
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
    await settleVoiceCapture();

    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({
      keys: ["right_windows", "right_control"],
    });
    expect(wrapper.text()).toContain("已设为 右 Win + 右 Ctrl");
    expect(wrapper.text()).toContain("若与微信输入法语音键不一致将无法生效");
    wrapper.unmount();
  });
});
