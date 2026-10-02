import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AudioEndpoint, AudioSnapshot, ConnectionSnapshot, RuntimeSnapshot } from "../lib/bridge";
import { VOICE_HOTKEY_CUSTOM_CAPTURE_ENABLED } from "../lib/feature-flags";
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

/** 当前选中（高亮）的输入工具卡片。 */
function selectedToolCard(wrapper: VueWrapper): string {
  return wrapper.find(".tool-card.selected").find("strong").text();
}

/** 切到"其他工具"并进入自定义组合键录入（录入入口在 feature flag 之后）。 */
async function startCustomCapture(wrapper: VueWrapper): Promise<void> {
  await wrapper
    .findAll(".tool-card")
    .find((card) => card.text().includes("其他工具"))!
    .trigger("click");
  await flushPromises();
  await wrapper
    .findAll(".chip-select .chip")
    .find((chip) => chip.text() === "自定义组合键")!
    .trigger("click");
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
  getVoiceInputTool: vi.fn(),
  setVoiceInputTool: vi.fn(),
  getVokieInstallation: vi.fn(),
  openVokieHomepage: vi.fn(),
  launchVokie: vi.fn(),
  getOtherVoiceHotkey: vi.fn(),
  setOtherVoiceHotkey: vi.fn(),
  startShortcutCapture: vi.fn(),
  stopShortcutCapture: vi.fn(),
  subscribeShortcutCaptureEdges: vi.fn(),
  getRc003TaskStatus: vi.fn(),
  enableRc003Capture: vi.fn(),
  disableRc003Capture: vi.fn(),
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
    getVoiceInputTool: mocks.getVoiceInputTool,
    setVoiceInputTool: mocks.setVoiceInputTool,
    getVokieInstallation: mocks.getVokieInstallation,
    openVokieHomepage: mocks.openVokieHomepage,
    launchVokie: mocks.launchVokie,
    getOtherVoiceHotkey: mocks.getOtherVoiceHotkey,
    setOtherVoiceHotkey: mocks.setOtherVoiceHotkey,
    startShortcutCapture: mocks.startShortcutCapture,
    stopShortcutCapture: mocks.stopShortcutCapture,
    subscribeShortcutCaptureEdges: mocks.subscribeShortcutCaptureEdges,
    getRc003TaskStatus: mocks.getRc003TaskStatus,
    enableRc003Capture: mocks.enableRc003Capture,
    disableRc003Capture: mocks.disableRc003Capture,
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
    mocks.getVoiceInputTool.mockResolvedValue("wechat");
    mocks.setVoiceInputTool.mockImplementation(async (tool) => tool);
    mocks.getVokieInstallation.mockResolvedValue({ installed: true, running: true });
    mocks.getOtherVoiceHotkey.mockResolvedValue(null);
    mocks.setOtherVoiceHotkey.mockImplementation(async (keys: string[]) => keys);
    mocks.openVokieHomepage.mockResolvedValue(undefined);
    mocks.launchVokie.mockResolvedValue(undefined);
    mocks.getRc003TaskStatus.mockResolvedValue({
      installed: true,
      authorizationRequired: false,
      enabled: false,
      helperPath: null,
      lastError: null,
    });
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

  // 输入工具卡片（2026-09-30 设计稿 v3）：选工具即自动落该工具的快捷键。
  // 顺序固定为 豆包 > 微信 > 其他（Andy 要求豆包排第一）；选中态跟随持久化的
  // 工具选择，而不是卡片顺序。
  it("工具卡片顺序为豆包/微信/其他，选中态跟随已保存的工具选择", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    const order = wrapper.findAll(".tool-card strong").map((node) => node.text());
    expect(order).toEqual(["豆包输入法", "微信输入法", "Vokie", "其他工具"]);
    expect(selectedToolCard(wrapper)).toBe("微信输入法");
    // 微信不需要"支持更多输入工具"开关，面板不渲染它。
    expect(wrapper.find(".capture-switch").exists()).toBe(false);
    expect(wrapper.text()).toContain("微信输入法不需要“支持更多输入工具”开关");
    wrapper.unmount();
  });

  it("老配置没有工具选择时按当前快捷键推断并落存一次", async () => {
    mocks.getVoiceInputTool.mockResolvedValue(null);
    mocks.getVoiceHoldHotkey.mockResolvedValue({ keys: ["right_alt"] });
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    expect(selectedToolCard(wrapper)).toBe("豆包输入法");
    expect(mocks.setVoiceInputTool).toHaveBeenCalledWith("doubao");
    wrapper.unmount();
  });

  it("点豆包卡片：自动把快捷键设为右 Alt、落存工具选择并显示该工具的清单", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".tool-card")
      .find((card) => card.text().includes("豆包输入法"))!
      .trigger("click");
    await flushPromises();

    expect(mocks.setVoiceInputTool).toHaveBeenCalledWith("doubao");
    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({ keys: ["right_alt"] });
    expect(selectedToolCard(wrapper)).toBe("豆包输入法");
    expect(wrapper.text()).toContain("按住说话快捷键已设为 右 Alt");
    expect(wrapper.findAll(".checklist li").map((item) => item.text())).toEqual([
      "1豆包麦克风选 CABLE Output",
      "2豆包长按语音键选 右 Alt",
      "3切到豆包后，按住遥控器语音键说话",
    ]);
    expect(wrapper.find(".capture-switch").exists()).toBe(true);
    wrapper.unmount();
  });

  it("「其他工具」记住按键：切去豆包再切回来恢复上次选择（含「不按键」）", async () => {
    mocks.getVoiceInputTool.mockResolvedValue("other");
    mocks.getVoiceHoldHotkey.mockResolvedValue({ keys: ["left_alt"] });
    mocks.getOtherVoiceHotkey.mockResolvedValue(["left_alt"]);
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    const otherCard = wrapper
      .findAll(".tool-card")
      .find((card) => card.text().includes("其他工具"))!;

    // 切去豆包（会改写按住说话快捷键），再切回「其他工具」：按记忆恢复左 Alt。
    await wrapper
      .findAll(".tool-card")
      .find((card) => card.text().includes("豆包"))!
      .trigger("click");
    await flushPromises();
    expect(mocks.setVoiceHoldHotkey).toHaveBeenLastCalledWith({ keys: ["right_alt"] });

    mocks.getOtherVoiceHotkey.mockResolvedValue(["left_alt"]);
    await otherCard.trigger("click");
    await flushPromises();
    expect(mocks.setVoiceHoldHotkey).toHaveBeenLastCalledWith({ keys: ["left_alt"] });

    // 在「其他工具」面板里改选「不按键」：立即生效并记住（[] = 明确不按键）。
    mocks.getVoiceHoldHotkey.mockResolvedValue(null);
    const noneChip = wrapper
      .find(".setup-col:nth-child(2)")
      .findAll("button")
      .find((button) => button.text().includes("不按键"))!;
    await noneChip.trigger("click");
    await flushPromises();
    expect(mocks.setVoiceHoldHotkey).toHaveBeenLastCalledWith(null);
    expect(mocks.setOtherVoiceHotkey).toHaveBeenLastCalledWith([]);
    wrapper.unmount();
  });

  it("「其他工具」从未选过按键：切回来保持现状，不擅自改写", async () => {
    mocks.getVoiceInputTool.mockResolvedValue("other");
    mocks.getVoiceHoldHotkey.mockResolvedValue({ keys: ["left_alt"] });
    mocks.getOtherVoiceHotkey.mockResolvedValue(null);
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".tool-card")
      .find((card) => card.text().includes("其他工具"))!
      .trigger("click");
    await flushPromises();

    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();
    wrapper.unmount();
  });

  it("点 Vokie 卡片：自动设为右 Alt、落存工具选择并显示 Vokie 的准备清单", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".tool-card")
      .find((card) => card.text().includes("Vokie"))!
      .trigger("click");
    await flushPromises();

    expect(mocks.setVoiceInputTool).toHaveBeenCalledWith("vokie");
    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({ keys: ["right_alt"] });
    expect(selectedToolCard(wrapper)).toBe("Vokie");
    expect(wrapper.findAll(".checklist li").map((item) => item.text())).toEqual([
      "1Vokie 麦克风选 CABLE Output",
      "2Vokie 快捷键保持默认的 右 Alt",
      "3在要写字的地方按住遥控器语音键说话",
    ]);
    // 已安装且在运行（默认 mock）：不显示官网入口，也不提示未运行。
    expect(wrapper.text()).not.toContain("没有检测到 Vokie");
    expect(wrapper.text()).not.toContain("Vokie 没有运行");
    // Vokie 面板不显示“支持更多输入工具”开关（2026-10-01 Andy 要求）。
    expect(wrapper.find(".setup-col:nth-child(2) .capture-switch").exists()).toBe(false);
    wrapper.unmount();
  });

  it("已安装但没运行：提示先启动 Vokie（没运行不会响应右 Alt）", async () => {
    mocks.getVokieInstallation.mockResolvedValue({ installed: true, running: false });
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".tool-card")
      .find((card) => card.text().includes("Vokie"))!
      .trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("Vokie 没有运行");
    expect(wrapper.text()).not.toContain("没有检测到 Vokie");

    // 没运行 → 第 ② 步给出「打开 Vokie」按钮：点它调用 launchVokie 并提示重新检测。
    const launchButton = wrapper
      .findAll("button")
      .find((button) => button.text().includes("打开 Vokie"))!;
    expect(launchButton).toBeDefined();
    await launchButton.trigger("click");
    await flushPromises();
    expect(mocks.launchVokie).toHaveBeenCalledTimes(1);
    expect(wrapper.text()).toContain("已打开 Vokie");

    // 启动后点“重新检测”：提示消失。
    mocks.getVokieInstallation.mockResolvedValue({ installed: true, running: true });
    mocks.getOtherVoiceHotkey.mockResolvedValue(null);
    mocks.setOtherVoiceHotkey.mockImplementation(async (keys: string[]) => keys);
    const panel = wrapper.find(".setup-col:nth-child(2)");
    await panel
      .findAll("button")
      .find((button) => button.text().includes("重新检测"))!
      .trigger("click");
    await flushPromises();
    expect(wrapper.text()).not.toContain("Vokie 没有运行");
    wrapper.unmount();
  });

  it("第 ② 步常显提示：避免其他 App 同键 + 当前输入法不对时再按一次", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();
    expect(wrapper.text()).toContain("避免其他 App 使用同一个快捷键");
    expect(wrapper.text()).toContain("可能需要再按住一次才能正常使用");
    wrapper.unmount();
  });

  it("选豆包且 Vokie 正在运行：豆包面板给出“右 Alt 会被 Vokie 抢先”的提示", async () => {
    mocks.getVoiceInputTool.mockResolvedValue("doubao");
    mocks.getVokieInstallation.mockResolvedValue({ installed: true, running: true });
    mocks.getOtherVoiceHotkey.mockResolvedValue(null);
    mocks.setOtherVoiceHotkey.mockImplementation(async (keys: string[]) => keys);
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    expect(selectedToolCard(wrapper)).toBe("豆包输入法");
    expect(wrapper.text()).toContain("检测到 Vokie 正在运行");
    wrapper.unmount();
  });

  it("未安装 Vokie：显示官网入口与重新检测，点官网按钮调用 openVokieHomepage", async () => {
    mocks.getVokieInstallation.mockResolvedValue({ installed: false, running: false });
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".tool-card")
      .find((card) => card.text().includes("Vokie"))!
      .trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("没有检测到 Vokie");
    const panel = wrapper.find(".setup-col:nth-child(2)");
    const siteButton = panel
      .findAll("button")
      .find((button) => button.text().includes("打开官网"))!;
    expect(siteButton).toBeDefined();
    await siteButton.trigger("click");
    await flushPromises();
    expect(mocks.openVokieHomepage).toHaveBeenCalledOnce();

    // 装好之后点“重新检测”：提示消失。
    mocks.getVokieInstallation.mockResolvedValue({ installed: true, running: true });
    mocks.getOtherVoiceHotkey.mockResolvedValue(null);
    mocks.setOtherVoiceHotkey.mockImplementation(async (keys: string[]) => keys);
    await panel
      .findAll("button")
      .find((button) => button.text().includes("重新检测"))!
      .trigger("click");
    await flushPromises();
    expect(wrapper.text()).not.toContain("没有检测到 Vokie");
    wrapper.unmount();
  });

  it("切换输入工具期间不闪「未同步」黄标（两段 IPC 之间按乐观态显示）", async () => {
    // 按住说话快捷键写入挂在半路：复现"工具已切、快捷键还在写盘"的中间态。
    const gate: { release: (() => void) | null } = { release: null };
    mocks.setVoiceHoldHotkey.mockImplementation(
      (hotkey: unknown) =>
        new Promise((resolve) => {
          gate.release = () => resolve(hotkey);
        }),
    );
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    // 起点：微信 + 左 Ctrl + 左 Win（beforeEach 默认），面板显示"已自动设置"。
    const panelText = () =>
      wrapper.find(".setup-col:nth-child(2)").text();
    expect(panelText()).toContain("已自动设置");

    // 点豆包：点击后 nextTick 已过、快捷键尚未写盘——旧实现这里会闪"未同步"。
    await wrapper
      .findAll(".tool-card")
      .find((card) => card.text().includes("豆包"))!
      .trigger("click");
    expect(panelText()).not.toContain("未同步");

    gate.release?.();
    await flushPromises();
    expect(panelText()).toContain("已自动设置");
    expect(panelText()).not.toContain("未同步");
    wrapper.unmount();
  });

  it("切换输入工具但快捷键写盘失败：回到「未同步」而不是停在乐观态", async () => {
    mocks.setVoiceHoldHotkey.mockRejectedValue(new Error("写入失败"));
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".tool-card")
      .find((card) => card.text().includes("豆包"))!
      .trigger("click");
    await flushPromises();

    expect(wrapper.find(".setup-col:nth-child(2)").text()).toContain("未同步");
    wrapper.unmount();
  });

  it("点其他工具卡片：不改变当前快捷键，改为提供按键芯片与自定义占位", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".tool-card")
      .find((card) => card.text().includes("其他工具"))!
      .trigger("click");
    await flushPromises();

    expect(mocks.setVoiceInputTool).toHaveBeenCalledWith("other");
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();
    const chips = wrapper.findAll(".chip-select .chip").map((chip) => chip.text());
    expect(chips.slice(0, 3)).toEqual(["右 Alt", "左 Alt", "不按键"]);
    // 自定义组合键入口：feature flag 关闭时是禁用占位，打开时是可点入口。
    expect(chips[3]).toBe(
      VOICE_HOTKEY_CUSTOM_CAPTURE_ENABLED ? "自定义组合键" : "自定义组合键（暂未开放）",
    );
    expect(wrapper.find(".chip-select .chip:disabled").exists()).toBe(
      !VOICE_HOTKEY_CUSTOM_CAPTURE_ENABLED,
    );
    wrapper.unmount();
  });

  it("其他工具：选左 Alt 落盘左 Alt，选不按键落盘关闭", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();
    await wrapper
      .findAll(".tool-card")
      .find((card) => card.text().includes("其他工具"))!
      .trigger("click");
    await flushPromises();

    await wrapper
      .findAll(".chip-select .chip")
      .find((chip) => chip.text() === "左 Alt")!
      .trigger("click");
    await flushPromises();
    expect(mocks.setVoiceHoldHotkey).toHaveBeenLastCalledWith({ keys: ["left_alt"] });
    expect(
      wrapper.find(".chip-select .chip[aria-pressed='true']").text(),
    ).toBe("左 Alt");

    await wrapper
      .findAll(".chip-select .chip")
      .find((chip) => chip.text() === "不按键")!
      .trigger("click");
    await flushPromises();
    expect(mocks.setVoiceHoldHotkey).toHaveBeenLastCalledWith(null);
    expect(
      wrapper.find(".chip-select .chip[aria-pressed='true']").text(),
    ).toBe("不按键");
    wrapper.unmount();
  });

  it("默认工具为微信时，面板显示 左 Ctrl + 左 Win 且标记已自动设置", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    const panel = wrapper.find(".setup-col:nth-child(2)");
    expect(panel.text()).toContain("左 Ctrl");
    expect(panel.text()).toContain("左 Win");
    expect(panel.text()).toContain("已自动设置");
    wrapper.unmount();
  });

  it("工具选择保存失败时回退到原工具并显示错误", async () => {
    mocks.setVoiceInputTool.mockRejectedValueOnce(new Error("磁盘写入失败"));
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper
      .findAll(".tool-card")
      .find((card) => card.text().includes("豆包输入法"))!
      .trigger("click");
    await flushPromises();

    expect(selectedToolCard(wrapper)).toBe("微信输入法");
    expect(wrapper.text()).toContain("磁盘写入失败");
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();
    wrapper.unmount();
  });

  it.skipIf(!VOICE_HOTKEY_CUSTOM_CAPTURE_ENABLED)("accepts a lone modifier as the hold-to-talk hotkey (长按右 Alt 一类)", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await startCustomCapture(wrapper);

    mocks.captureEdgeHandler!({ key: "right_alt", isPressed: true });
    await flushPromises();
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();

    mocks.captureEdgeHandler!({ key: "right_alt", isPressed: false });
    await settleVoiceCapture();
    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({ keys: ["right_alt"] });
    expect(wrapper.text()).toContain("按住说话快捷键已设为 右 Alt");
    // 其他工具的按键芯片应与落盘结果一致（误录可一键换回）。
    expect(wrapper.find(".chip-select .chip[aria-pressed='true']").text()).toBe("右 Alt");
    wrapper.unmount();
  });

  it.skipIf(!VOICE_HOTKEY_CUSTOM_CAPTURE_ENABLED)("cancels capture with Esc and keeps the current hotkey", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await startCustomCapture(wrapper);

    mocks.captureEdgeHandler!({ key: "escape", isPressed: true });
    await flushPromises();
    expect(mocks.setVoiceHoldHotkey).not.toHaveBeenCalled();
    expect(mocks.stopShortcutCapture).toHaveBeenCalledOnce();
    expect(wrapper.find(".voice-hotkey-capture").exists()).toBe(false);
    expect(wrapper.text()).toContain("已取消录入");
    wrapper.unmount();
  });

  it.skipIf(!VOICE_HOTKEY_CUSTOM_CAPTURE_ENABLED)("records a custom hold-to-talk chord and only saves it after every key is released", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await startCustomCapture(wrapper);
    expect(mocks.startShortcutCapture).toHaveBeenCalledOnce();
    expect(mocks.captureEdgeHandler).not.toBeNull();
    expect(wrapper.find(".voice-hotkey-capture").text()).toContain(
      "请按下你输入工具当前设置的语音键",
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

  it.skipIf(!VOICE_HOTKEY_CUSTOM_CAPTURE_ENABLED)("asks to release pre-held keys first and saves the full chord after arming", async () => {
    // 2026-09-27 回归：录入开始时仍有按键按住（preheld），其边沿对录入
    // 不可见；后端等 preheld 全部松开后才投递边沿，前端先提示松手，
    // 杜绝把新组合截断成半截（"只剩左 Ctrl"）。
    mocks.startShortcutCapture.mockResolvedValue(["left_windows"]);
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await startCustomCapture(wrapper);
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

  it.skipIf(!VOICE_HOTKEY_CUSTOM_CAPTURE_ENABLED)("keeps every modifier of a modifier-only chord when keys are released out of order", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await startCustomCapture(wrapper);

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

  it.skipIf(!VOICE_HOTKEY_CUSTOM_CAPTURE_ENABLED)("saves the full chord when the swallowed key only arrives as a replayed injected copy", async () => {
    // 2026-09-27 真机回归（Bugs/2026-09-27-ime-chord-hook-eats-active-hotkey-capture.md）：
    // 微信输入法的语音和弦就是默认的 左 Ctrl + 左 Win。按下该组合时它吞掉
    // 左 Win 的物理边沿、随后把整个组合以注入副本重放——真实到达后端的只有
    // 左 Ctrl 的真实边沿 + 左 Win 的注入副本，且副本可能晚于物理松开。
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await startCustomCapture(wrapper);

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

  it.skipIf(!VOICE_HOTKEY_CUSTOM_CAPTURE_ENABLED)("keeps the whole modifier-only chord when the first-pressed modifier is released last", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await startCustomCapture(wrapper);

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

  it.skipIf(!VOICE_HOTKEY_CUSTOM_CAPTURE_ENABLED)("infers the WeType chord when only one edge survived but WeType voice was triggered", async () => {
    // 2026-09-27 探针结论：微信输入法吞掉其语音热键组成键的物理边沿发生在
    // RIT 层，对本进程零/半截边沿（低级钩子、Raw Input、GetAsyncKeyState 都
    // 看不到）；其麦克风在录入期间被触发（observed）是唯一旁证 → 推断用户按
    // 的就是微信输入法语音热键，落盘产品默认组合并说明原因。
    mocks.stopShortcutCapture.mockResolvedValue({ wetypeVoice: "observed" });
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await startCustomCapture(wrapper);

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

  it.skipIf(!VOICE_HOTKEY_CUSTOM_CAPTURE_ENABLED)("keeps a lone modifier when WeType voice was not triggered", async () => {
    // 单修饰键（豆包"长按右 Alt"一类）合法：微信输入法语音未被触发时不得推断。
    mocks.stopShortcutCapture.mockResolvedValue({ wetypeVoice: "not_observed" });
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await startCustomCapture(wrapper);

    mocks.captureEdgeHandler!({ key: "right_alt", isPressed: true });
    mocks.captureEdgeHandler!({ key: "right_alt", isPressed: false });
    await settleVoiceCapture();

    expect(mocks.setVoiceHoldHotkey).toHaveBeenCalledWith({ keys: ["right_alt"] });
    expect(wrapper.text()).not.toContain("已按微信输入法语音键默认值");
    wrapper.unmount();
  });

  it.skipIf(!VOICE_HOTKEY_CUSTOM_CAPTURE_ENABLED)("warns that a non-WeType chord will not trigger hold-to-talk voice", async () => {
    // Win + 右 Ctrl 不是微信输入法热键：边沿透传能录上，但按住说话靠注入该
    // 组合唤起微信输入法语音，不一致就无法生效——必须把这一后果告诉用户。
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await startCustomCapture(wrapper);

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

/**
 * 遥控器型号显示（2026-10-01 Andy 要求）：已识别型号时状态条标题直接显示型号，
 * 未识别（GATT 2A24 未读回）或未连接时退回蓝牙广播名 / 阶段文案。
 */
describe("connection page remote model", () => {
  beforeEach(() => {
    mocks.getConnectionSnapshot.mockResolvedValue(emptyConnection);
    mocks.getAudioSnapshot.mockResolvedValue(emptyAudio);
    mocks.listAudioEndpoints.mockResolvedValue([]);
    mocks.getVoiceHoldHotkey.mockResolvedValue({ keys: ["left_control", "left_windows"] });
    mocks.setVoiceHoldHotkey.mockImplementation(async (hotkey) => hotkey);
    mocks.getVoiceInputTool.mockResolvedValue("wechat");
    mocks.setVoiceInputTool.mockImplementation(async (tool) => tool);
    mocks.getVokieInstallation.mockResolvedValue({ installed: true, running: true });
    mocks.getOtherVoiceHotkey.mockResolvedValue(null);
    mocks.setOtherVoiceHotkey.mockImplementation(async (keys: string[]) => keys);
    mocks.openVokieHomepage.mockResolvedValue(undefined);
    mocks.getRc003TaskStatus.mockResolvedValue({
      installed: true,
      authorizationRequired: false,
      enabled: false,
      helperPath: null,
      lastError: null,
    });
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

  it.each([
    ["rc001", "小米蓝牙语音遥控器 2"],
    ["rc003", "小米蓝牙语音遥控器 2 Pro"],
  ] as const)("%s 连接后标题显示型号", async (model, expected) => {
    mocks.getConnectionSnapshot.mockResolvedValue({
      ...emptyConnection,
      phase: "ready",
      remoteName: "小米蓝牙语音遥控器",
      remoteModel: model,
    });
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    expect(wrapper.find(".connection-title").text()).toBe(expected);
    wrapper.unmount();
  });

  it("型号未识别时退回蓝牙广播名，未连接时显示阶段文案", async () => {
    mocks.getConnectionSnapshot.mockResolvedValue({
      ...emptyConnection,
      phase: "ready",
      remoteName: "小米蓝牙语音遥控器",
      remoteModel: "unknown",
    });
    let wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();
    expect(wrapper.find(".connection-title").text()).toBe("小米蓝牙语音遥控器");
    wrapper.unmount();

    mocks.getConnectionSnapshot.mockResolvedValue(emptyConnection);
    wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();
    expect(wrapper.find(".connection-title").text()).toBe("尚未连接");
    wrapper.unmount();
  });
});

/**
 * 「支持更多输入工具」开关（2026-09-29）：与按键页「全按键支持」同一设置项
 * 的连接页入口。判据（授权确认、失败回退、权威对账）与 ButtonsPage 同源。
 */
describe("connection page rc003 capture switch", () => {
  const taskStatus = (
    overrides: Partial<import("../lib/bridge").Rc003TaskStatus> = {},
  ) => ({
    installed: true,
    authorizationRequired: false,
    enabled: false,
    helperPath: null,
    lastError: null,
    ...overrides,
  });

  afterEach(() => {
    // 本 describe 不继承上面 VB-CABLE describe 的 afterEach（作用域只在
    // 各自 describe 内）；没有它，spy 调用计数跨用例累计，"未被调用"
    // 断言会吃到上一个用例的调用记录（本次失败实证）。
    vi.clearAllMocks();
  });

  beforeEach(() => {
    // vi.clearAllMocks 只清调用记录不清实现（ButtonsPage 2026-09-28 教训）：
    // 每个 beforeEach 显式重置实现。
    mocks.getConnectionSnapshot.mockResolvedValue(emptyConnection);
    mocks.getAudioSnapshot.mockResolvedValue(emptyAudio);
    mocks.listAudioEndpoints.mockResolvedValue([]);
    mocks.getVoiceHoldHotkey.mockResolvedValue({ keys: ["left_control", "left_windows"] });
    mocks.setVoiceHoldHotkey.mockImplementation(async (hotkey) => hotkey);
    // 开关只在需要它的工具面板里渲染：这里固定为豆包（要求开启）。
    mocks.getVoiceInputTool.mockResolvedValue("doubao");
    mocks.setVoiceInputTool.mockImplementation(async (tool) => tool);
    mocks.getVokieInstallation.mockResolvedValue({ installed: true, running: true });
    mocks.getOtherVoiceHotkey.mockResolvedValue(null);
    mocks.setOtherVoiceHotkey.mockImplementation(async (keys: string[]) => keys);
    mocks.openVokieHomepage.mockResolvedValue(undefined);
    mocks.startShortcutCapture.mockResolvedValue([]);
    mocks.stopShortcutCapture.mockResolvedValue(undefined);
    mocks.subscribeShortcutCaptureEdges.mockImplementation(
      async (handler: ShortcutCaptureHandler) => {
        mocks.captureEdgeHandler = handler;
        return () => {};
      },
    );
    mocks.getRc003TaskStatus.mockResolvedValue(taskStatus());
    mocks.enableRc003Capture.mockResolvedValue(taskStatus({ enabled: true }));
    mocks.disableRc003Capture.mockResolvedValue(taskStatus({ enabled: false }));
  });

  it("挂载对账后开关显示权威状态，对账失败显示占位符", async () => {
    mocks.getRc003TaskStatus.mockResolvedValue(taskStatus({ enabled: true }));
    let wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();
    expect(wrapper.find(".capture-switch").exists()).toBe(true);
    expect((wrapper.find(".capture-switch").element as HTMLInputElement).checked).toBe(
      true,
    );
    wrapper.unmount();

    // 对账失败（IPC 抛错）：开关不猜状态，渲染占位符且不出可点的 input。
    mocks.getRc003TaskStatus.mockRejectedValue(new Error("ipc down"));
    wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();
    expect(wrapper.find(".capture-switch").exists()).toBe(false);
    expect(wrapper.find(".toggle-placeholder").exists()).toBe(true);
    wrapper.unmount();
  });

  it("豆包面板在开关未开启时提示「还差一步」，开启后提示已就绪", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();
    expect(wrapper.text()).toContain("需要开启");
    expect(wrapper.text()).toContain("还差一步");

    await wrapper.find(".capture-switch").setValue(true);
    await flushPromises();
    expect(wrapper.text()).toContain("已开启");
    expect(wrapper.text()).not.toContain("还差一步");
    wrapper.unmount();
  });

  it("已授权（authorizationRequired=false）时开启直接执行，不开确认弹窗", async () => {
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();
    await wrapper.find(".capture-switch").setValue(true);
    await flushPromises();

    expect(mocks.enableRc003Capture).toHaveBeenCalledTimes(1);
    expect(mocks.disableRc003Capture).not.toHaveBeenCalled();
    expect(wrapper.findComponent({ name: "EnhancedCaptureConfirmDialog" }).exists()).toBe(false);
    expect((wrapper.find(".capture-switch").element as HTMLInputElement).checked).toBe(
      true,
    );
    wrapper.unmount();
  });

  it("需要授权时先弹确认弹窗，确认后才调用 enable", async () => {
    mocks.getRc003TaskStatus.mockResolvedValue(
      taskStatus({ installed: false, authorizationRequired: true }),
    );
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper.find(".capture-switch").setValue(true);
    await flushPromises();
    // 未确认：不调 enable，开关 DOM 写回关闭。
    expect(mocks.enableRc003Capture).not.toHaveBeenCalled();
    const dialog = wrapper.findComponent({ name: "EnhancedCaptureConfirmDialog" });
    expect(dialog.exists()).toBe(true);
    expect((wrapper.find(".capture-switch").element as HTMLInputElement).checked).toBe(
      false,
    );

    // 确认后走 enable，开关随结果开启。
    await dialog.vm.$emit("confirm");
    await flushPromises();
    expect(mocks.enableRc003Capture).toHaveBeenCalledTimes(1);
    expect((wrapper.find(".capture-switch").element as HTMLInputElement).checked).toBe(
      true,
    );
    wrapper.unmount();
  });

  it("操作失败（lastError）：开关回退原状态并显示提示，不静默保持翻转", async () => {
    mocks.enableRc003Capture.mockResolvedValue(
      taskStatus({ enabled: false, lastError: "授权未完成（UAC 被取消）" }),
    );
    const wrapper = mount(ConnectionPage, { props: { runtime } });
    await flushPromises();

    await wrapper.find(".capture-switch").setValue(true);
    await flushPromises();

    expect((wrapper.find(".capture-switch").element as HTMLInputElement).checked).toBe(
      false,
    );
    expect(wrapper.text()).toContain("授权未完成（UAC 被取消）");
    wrapper.unmount();
  });
});
