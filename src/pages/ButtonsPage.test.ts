import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import ButtonsPage from "./ButtonsPage.vue";

type EdgeHandler = (edge: { button: string; isPressed: boolean }) => void;
type GestureHandler = (gesture: { button: string; trigger: string }) => void;
type ShortcutCaptureHandler = (edge: { key: string; isPressed: boolean }) => void;

let edgeHandler: EdgeHandler | null = null;
let gestureHandler: GestureHandler | null = null;
let shortcutCaptureHandler: ShortcutCaptureHandler | null = null;

vi.mock("../lib/bridge", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/bridge")>();
  return {
    ...actual,
    getButtonMappings: vi.fn(async () => ({
      enabled: true,
      actions: {
        ok: {
          single: { type: "shortcut", chord: { keys: ["enter"] } },
          double: { type: "disabled" },
          long: { type: "disabled" },
        },
      },
    })),
    getButtonMappingSnapshot: vi.fn(async () => ({
      enabled: true,
      gateActive: true,
      listenerActive: true,
      swallowedEdges: 3,
      leakedDowns: 0,
      firedGestures: 1,
      lastFired: null,
      lastError: null,
    })),
    saveButtonMappings: vi.fn(async (mappings: unknown) => mappings),
    learnFocusTarget: vi.fn(async () => ({
      controlType: "Edit",
      automationId: "chat-input",
      className: "RichEdit",
    })),
    testAppFocus: vi.fn(async () => undefined),
    exportButtonMappingConfiguration: vi.fn(async () => true),
    importButtonMappingConfiguration: vi.fn(async () => ({
      enabled: false,
      actions: {
        power: {
          single: { type: "shortcut", chord: { keys: ["escape"] } },
          double: { type: "disabled" },
          long: { type: "disabled" },
        },
      },
    })),
    resetButtonMappings: vi.fn(async () => ({ enabled: true, actions: {} })),
    scanRegisteredApps: vi.fn(async () => [
      { name: "Registered Example", path: "shell:AppsFolder\\Example!App" },
    ]),
    testButtonMapping: vi.fn(async () => ({
      available: true,
      submittedBatches: 1,
      submittedEvents: 2,
      lastError: null,
    })),
    subscribeButtonEdges: vi.fn(async (handler: EdgeHandler) => {
      edgeHandler = handler;
      return () => {};
    }),
    subscribeButtonGestures: vi.fn(async (handler: GestureHandler) => {
      gestureHandler = handler;
      return () => {};
    }),
    startShortcutCapture: vi.fn(async () => undefined),
    stopShortcutCapture: vi.fn(async () => undefined),
    subscribeShortcutCaptureEdges: vi.fn(async (handler: ShortcutCaptureHandler) => {
      shortcutCaptureHandler = handler;
      return () => {};
    }),
    // 三键捕获：默认「未授权，开启会触发 UAC」；个别用例用 mockResolvedValue 覆盖。
    // authorizationRequired 与 Rust enable_capture 的授权判定同源
    // （任务未注册，或安装/升级写下了重授权标记）。
    getRc003TaskStatus: vi.fn(async () => ({
      installed: false,
      authorizationRequired: true,
      enabled: false,
      helperPath: null,
      lastError: null,
    })),
    enableRc003Capture: vi.fn(async () => ({
      installed: true,
      authorizationRequired: false,
      enabled: true,
      helperPath: null,
      lastError: null,
    })),
    disableRc003Capture: vi.fn(async () => ({
      // 关闭不移除任务：授权保留（与真实语义一致）。
      installed: true,
      authorizationRequired: false,
      enabled: false,
      helperPath: null,
      lastError: null,
    })),
    getRc003BridgeSnapshot: vi.fn(async () => ({
      phase: "stopped",
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
    })),
  };
});

import {
  exportButtonMappingConfiguration,
  getButtonMappings,
  enableRc003Capture,
  disableRc003Capture,
  getRc003BridgeSnapshot,
  getRc003TaskStatus,
  importButtonMappingConfiguration,
  learnFocusTarget,
  subscribeButtonEdges,
  subscribeButtonGestures,
  saveButtonMappings,
  startShortcutCapture,
  stopShortcutCapture,
} from "../lib/bridge";
import type { ButtonMappings, Rc003BridgeSnapshot, RuntimeSnapshot } from "../lib/bridge";

const runtime: RuntimeSnapshot = {
  appVersion: "0.1.0",
  platform: {
    platform: "windows",
    windowsApiAvailable: true,
    bleScanAvailable: true,
    bleVoiceReady: true,
    wasapiReady: false,
    rawInputReady: true,
    sendInputReady: true,
    verificationStatus: "测试",
    connection: {
      phase: "ready",
      remoteName: "小米蓝牙语音遥控器",
      remoteModel: "rc003",
      capabilities: null,
      voiceState: "idle",
      decodedSamples: 0,
      generation: 0,
      reconnectAttempt: 0,
      powerNotificationsAvailable: false,
      lastError: null,
    },
    audio: {
      phase: "unsupported",
      selectedEndpointId: null,
      selectedEndpointName: null,
      queuedSamples: 0,
      submittedSamples: 0,
      generation: 0,
      lastError: null,
    },
    rawInput: {
      phase: "ready",
      matchedDeviceCount: 1,
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
      gateActive: true,
      listenerActive: true,
      swallowedEdges: 3,
      leakedDowns: 0,
      firedGestures: 1,
      lastFired: null,
      lastError: null,
    },
  },
};

async function mountPage(model: "rc001" | "rc003" | "unknown" = "rc003"): Promise<VueWrapper> {
  const snapshot =
    model === "rc003"
      ? runtime
      : {
          ...runtime,
          platform: {
            ...runtime.platform,
            connection: { ...runtime.platform.connection, remoteModel: model },
          },
        };
  const wrapper = mount(ButtonsPage, { props: { runtime: snapshot } });
  await vi.waitFor(() => {
    if (!edgeHandler || !gestureHandler) throw new Error("事件订阅未完成");
  });
  return wrapper;
}

beforeEach(() => {
  edgeHandler = null;
  gestureHandler = null;
  shortcutCaptureHandler = null;
  vi.mocked(getButtonMappings).mockClear();
  vi.mocked(subscribeButtonEdges).mockClear();
  vi.mocked(subscribeButtonGestures).mockClear();
  vi.mocked(saveButtonMappings).mockClear();
  // mockClear 只清调用记录，**不清实现**：学习输入框的默认实现要在每个用例里重置。
  vi.mocked(learnFocusTarget).mockClear();
  vi.mocked(learnFocusTarget).mockResolvedValue({
    controlType: "Edit",
    automationId: "chat-input",
    className: "RichEdit",
  });
  vi.mocked(exportButtonMappingConfiguration).mockClear();
  vi.mocked(importButtonMappingConfiguration).mockClear();
  vi.mocked(startShortcutCapture).mockClear();
  vi.mocked(stopShortcutCapture).mockClear();
  // mockClear 只清调用记录，**不清实现**：前面用例留下的 mockRejectedValue /
  // mockResolvedValue 会渗进后面的用例，必须显式重置。
  localStorage.clear();
  vi.mocked(getRc003TaskStatus).mockResolvedValue({
    installed: false,
    authorizationRequired: true,
    enabled: false,
    helperPath: null,
    lastError: null,
  });
  vi.mocked(enableRc003Capture).mockResolvedValue({
    installed: true,
    authorizationRequired: false,
    enabled: true,
    helperPath: null,
    lastError: null,
  });
  vi.mocked(disableRc003Capture).mockResolvedValue({
    installed: true,
    authorizationRequired: false,
    enabled: false,
    helperPath: null,
    lastError: null,
  });
  // 桥接快照同样要重置实现（mockClear 不清实现，见上）。
  vi.mocked(getRc003BridgeSnapshot).mockResolvedValue(bridgeSnapshot("stopped"));
});

/** 构造一份指定相位的桥接快照（其余计数字段对 UI 无关，取零值）。 */
function bridgeSnapshot(phase: Rc003BridgeSnapshot["phase"]): Rc003BridgeSnapshot {
  return {
    phase,
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
  };
}

/** 「全按键支持」所在的开关行（页面有多个 toggle-row，必须按文本定位）。 */
function captureRow(page: VueWrapper) {
  return page
    .findAll(".toggle-row")
    .filter((row) => row.text().includes("全按键支持"))[0];
}

/** 2026-09-28 用户定稿：三键关闭态提示（不再带「提示：」前缀）。 */
const TRI_KEY_HINT = "返回 / 音量+ / 音量−需要开启全按键支持才能使用";

/** 2026-09-28 用户定稿：开关悬停提示随开关状态切换。 */
const CAPTURE_SWITCH_OFF_TITLE =
  "开启后支持使用返回 / 音量+ / 音量−，其他按键将同步优化";
const CAPTURE_SWITCH_ON_TITLE = "关闭后返回 / 音量+ / 音量−将不可映射";

/** 三键捕获开启前的确认弹窗（未弹出时为 undefined）。 */
function confirmDialog(page: VueWrapper) {
  return page
    .findAll("dialog")
    .find((dialog) => dialog.classes().includes("capture-confirm-dialog"));
}

describe("buttons mapping page", () => {
  it("adds scanned apps to the library without changing button bindings", async () => {
    const wrapper = await mountPage();
    await flushPromises();
    const powerCard = wrapper
      .findAll(".mapping-card")
      .find((item) => item.find(".mapping-card-title strong").text() === "电源")!;
    await powerCard.findAll(".mapping-cell")[0]!.trigger("click");
    await wrapper
      .findAll("button")
      .find((button) => button.text() === "扫描本机应用")!
      .trigger("click");
    await flushPromises();
    await wrapper.get('input[aria-label="全选当前结果"]').setValue(true);
    await wrapper.get(".registered-apps-dialog .primary-button").trigger("click");
    await flushPromises();

    const saved = vi.mocked(saveButtonMappings).mock.lastCall![0];
    expect(saved.applications).toEqual([
      { name: "Registered Example", path: "shell:AppsFolder\\Example!App" },
    ]);
    expect(saved.actions.power).toBeUndefined();
    expect(saved.actions.ok?.single).toEqual({
      type: "shortcut",
      chord: { keys: ["enter"] },
    });
    wrapper.unmount();
  });

  it("renders the remote canvas with 12 button cards, the voice card and 36 trigger cells", async () => {
    const wrapper = await mountPage();
    expect(wrapper.findAll(".mapping-card")).toHaveLength(13);
    expect(wrapper.findAll(".mapping-cell")).toHaveLength(36);
    const voiceCard = wrapper.find(".voice-card");
    expect(voiceCard.text()).toContain("语音键");
    expect(voiceCard.text()).toContain("按住说话");
  });

  it("does not register listeners or polling after unmounting during initial load", async () => {
    let resolveMappings!: (value: Awaited<ReturnType<typeof getButtonMappings>>) => void;
    const pendingMappings = new Promise<Awaited<ReturnType<typeof getButtonMappings>>>(
      (resolve) => {
        resolveMappings = resolve;
      },
    );
    vi.mocked(getButtonMappings).mockImplementationOnce(() => pendingMappings);
    const intervalSpy = vi.spyOn(window, "setInterval");

    const wrapper = mount(ButtonsPage, { props: { runtime } });
    await flushPromises();
    wrapper.unmount();
    resolveMappings({ enabled: true, actions: {} });
    await flushPromises();

    expect(subscribeButtonEdges).not.toHaveBeenCalled();
    expect(subscribeButtonGestures).not.toHaveBeenCalled();
    expect(intervalSpy).not.toHaveBeenCalled();
    intervalSpy.mockRestore();
  });

  it("immediately releases a listener that resolves after the page is unmounted", async () => {
    let resolveUnlisten!: (unlisten: () => void) => void;
    const pendingUnlisten = new Promise<() => void>((resolve) => {
      resolveUnlisten = resolve;
    });
    vi.mocked(subscribeButtonEdges).mockImplementationOnce(() => pendingUnlisten);
    const stopEdges = vi.fn();

    const wrapper = mount(ButtonsPage, { props: { runtime } });
    await vi.waitFor(() => expect(subscribeButtonEdges).toHaveBeenCalledOnce());
    const intervalSpy = vi.spyOn(window, "setInterval");
    wrapper.unmount();
    resolveUnlisten(stopEdges);
    await flushPromises();

    expect(stopEdges).toHaveBeenCalledOnce();
    expect(subscribeButtonGestures).not.toHaveBeenCalled();
    expect(intervalSpy).not.toHaveBeenCalled();
    intervalSpy.mockRestore();
  });

  it("saves, exports and imports a versioned mapping configuration from the footer", async () => {
    const wrapper = await mountPage();
    const button = (label: string) =>
      wrapper.findAll(".mapping-footer button").find((item) => item.text() === label)!;

    await button("保存配置").trigger("click");
    await vi.waitFor(() => expect(saveButtonMappings).toHaveBeenCalled());
    await flushPromises();
    expect(wrapper.text()).toContain("配置已保存并生效");

    await button("导出配置…").trigger("click");
    await vi.waitFor(() => expect(exportButtonMappingConfiguration).toHaveBeenCalledOnce());
    expect(wrapper.text()).toContain("按键映射配置已导出");

    await button("导入配置…").trigger("click");
    await vi.waitFor(() => expect(importButtonMappingConfiguration).toHaveBeenCalledOnce());
    expect(wrapper.text()).toContain("按键映射配置已导入并生效");
    const powerCard = wrapper
      .findAll(".mapping-card")
      .find((card) => card.text().includes("电源"))!;
    expect(powerCard.text()).toContain("Esc");
  });

  it("marks configured cells and opens the editor with the correct target", async () => {
    const wrapper = await mountPage();
    const okCard = wrapper
      .findAll(".mapping-card")
      .find((card) => card.text().includes("确定"));
    expect(okCard).toBeDefined();
    expect(okCard!.text()).toContain("Enter");

    const singleCell = okCard!.findAll(".mapping-cell")[0]!;
    expect(singleCell.classes()).toContain("set");
    await singleCell.trigger("click");
    expect(wrapper.find(".mapping-editor").text()).toContain("确定 · 单击");
  });

  it("applies a preset to the editing target and auto-persists (对齐 Mac 即时保存)", async () => {
    const wrapper = await mountPage();
    const powerCard = wrapper
      .findAll(".mapping-card")
      .find((card) => card.text().includes("电源"));
    await powerCard!.findAll(".mapping-cell")[2]!.trigger("click");

    const editor = wrapper.find(".mapping-editor");
    expect(editor.text()).toContain("电源 · 长按");
    // 点击 Esc 预设即自动保存（无需保存按钮）。
    const chips = editor.findAll(".chip");
    const escapeChip = chips.find((chip) => chip.text() === "Esc");
    await escapeChip!.trigger("click");
    await vi.waitFor(() => {
      if (vi.mocked(saveButtonMappings).mock.calls.length === 0) {
        throw new Error("自动保存未触发");
      }
    });
    const saved = vi.mocked(saveButtonMappings).mock.calls[0]![0] as {
      actions: Record<string, { long: { type: string; chord?: { keys: string[] } } }>;
    };
    expect(saved.actions.power!.long.type).toBe("shortcut");
    expect(saved.actions.power!.long.chord!.keys).toEqual(["escape"]);
    await flushPromises();

    // 禁用按键按钮：禁用当前格并自动保存。
    const disableButton = wrapper
      .findAll("button")
      .find((button) => button.text() === "禁用按键");
    expect(disableButton).toBeDefined();
    await disableButton!.trigger("click");
    await vi.waitFor(() => {
      if (vi.mocked(saveButtonMappings).mock.calls.length < 2) {
        throw new Error("禁用后未自动保存");
      }
    });
    const disabledSaved = vi.mocked(saveButtonMappings).mock.calls[1]![0] as {
      actions: Record<string, { long: { type: string } }>;
    };
    expect(disabledSaved.actions.power!.long.type).toBe("disabled");
  });

  it("配置「打开应用」的聚焦方式并学习输入框（策略与字段自洽）", async () => {
    const wrapper = await mountPage();
    const okCard = wrapper.findAll(".mapping-card").find((card) => card.text().includes("确定"));
    await okCard!.findAll(".mapping-cell")[0]!.trigger("click");
    const editor = wrapper.find(".mapping-editor");
    const notepadChip = editor.findAll(".chip").find((chip) => chip.text() === "记事本");
    expect(notepadChip).toBeDefined();
    await notepadChip!.trigger("click");
    await flushPromises();

    // 选中目标后出现聚焦方式面板，默认「只打开应用」。
    const panel = editor.find(".focus-profile");
    expect(panel.exists()).toBe(true);
    const recordedChip = panel
      .findAll(".chip")
      .find((chip) => chip.text() === "聚焦已记录的输入框");
    await recordedChip!.trigger("click");
    await vi.waitFor(() => {
      const saved = vi.mocked(saveButtonMappings).mock.calls.at(-1)?.[0] as {
        focusProfiles?: Record<string, { strategy: string }>;
      };
      if (saved?.focusProfiles?.notepad?.strategy !== "recorded_element") {
        throw new Error("策略未写进聚焦档案");
      }
    });

    // 切换策略时必须丢掉异策略字段（Rust 侧 normalized 会整条丢弃不自洽的档案）。
    const shortcutChip = panel
      .findAll(".chip")
      .find((chip) => chip.text() === "用应用快捷键聚焦");
    await shortcutChip!.trigger("click");
    await vi.waitFor(() => {
      const saved = vi.mocked(saveButtonMappings).mock.calls.at(-1)?.[0] as {
        focusProfiles?: Record<string, { strategy: string; recorded?: unknown }>;
      };
      const profile = saved?.focusProfiles?.notepad;
      if (profile?.strategy !== "app_shortcut" || profile.recorded !== undefined) {
        throw new Error("切换策略后仍带着旧策略字段");
      }
    });

    // 学习输入框：写入 recorded 并置为 recorded_element。
    await recordedChip!.trigger("click");
    await flushPromises();
    const learnChip = panel.findAll(".chip").find((chip) => chip.text() === "开始学习输入框");
    expect(learnChip).toBeDefined();
    await learnChip!.trigger("click");
    await vi.waitFor(() => {
      if (!wrapper.text().includes("已记录该输入框（只记录控件特征，不含输入内容）")) {
        throw new Error("学习结果提示未出现");
      }
    });
    const learned = vi.mocked(saveButtonMappings).mock.calls.at(-1)?.[0] as {
      focusProfiles?: Record<string, { strategy: string; recorded?: { automationId: string } }>;
    };
    expect(learned.focusProfiles?.notepad?.strategy).toBe("recorded_element");
    expect(learned.focusProfiles?.notepad?.recorded?.automationId).toBe("chat-input");

    // 学习失败时给可解释文案，不静默。
    vi.mocked(learnFocusTarget).mockRejectedValueOnce(new Error("no_candidate"));
    await learnChip!.trigger("click");
    await vi.waitFor(() => {
      if (!wrapper.text().includes("no_candidate")) {
        throw new Error("学习失败文案未出现");
      }
    });
  });

  it("configures mouse actions with independent validated amounts", async () => {    const wrapper = await mountPage();
    await flushPromises();
    const powerCard = wrapper
      .findAll(".mapping-card")
      .find((card) => card.text().includes("电源"))!;
    await powerCard.findAll(".mapping-cell")[0]!.trigger("click");
    const choose = async (label: string) => {
      await wrapper
        .findAll(".mapping-editor button")
        .find((button) => button.text() === label)!
        .trigger("click");
      await flushPromises();
    };

    await choose("滚轮向下");
    await wrapper.get('input[aria-label="每次滚动格数"]').setValue("5");
    await flushPromises();
    expect(vi.mocked(saveButtonMappings).mock.lastCall![0].actions.power!.single).toEqual({
      type: "scroll",
      direction: "down",
      steps: 5,
    });

    const saveCount = vi.mocked(saveButtonMappings).mock.calls.length;
    await wrapper.get('input[aria-label="每次滚动格数"]').setValue("101");
    await flushPromises();
    expect(vi.mocked(saveButtonMappings).mock.calls).toHaveLength(saveCount);
    expect(wrapper.text()).toContain("请输入 1 到 100 之间的整数");

    await choose("左键双击");
    expect(vi.mocked(saveButtonMappings).mock.lastCall![0].actions.power!.single).toEqual({
      type: "mouse_click",
      kind: "double_left",
    });
    wrapper.unmount();
  });

  it("records a physical Win+L chord directly by default", async () => {
    const wrapper = await mountPage();
    const powerCard = wrapper
      .findAll(".mapping-card")
      .find((card) => card.text().includes("电源"))!;
    await powerCard.findAll(".mapping-cell")[0]!.trigger("click");
    const captureButton = wrapper
      .findAll(".mapping-editor .chip")
      .find((button) => button.text().includes("录入自定义快捷键"))!;
    await captureButton.trigger("click");

    shortcutCaptureHandler!({ key: "left_windows", isPressed: true });
    shortcutCaptureHandler!({ key: "l", isPressed: true });
    shortcutCaptureHandler!({ key: "l", isPressed: false });
    await flushPromises();
    expect(stopShortcutCapture).not.toHaveBeenCalled();
    shortcutCaptureHandler!({ key: "left_windows", isPressed: false });
    await vi.waitFor(() => expect(stopShortcutCapture).toHaveBeenCalledOnce());
    const saved = vi.mocked(saveButtonMappings).mock.calls.at(-1)?.[0] as ButtonMappings;
    expect(saved.actions.power?.single).toEqual({
      type: "shortcut",
      chord: { keys: ["left_windows", "l"] },
    });
  });

  it("records Win+L safely after the user enables fallback mode", async () => {
    const wrapper = await mountPage();
    const powerCard = wrapper
      .findAll(".mapping-card")
      .find((card) => card.text().includes("电源"))!;
    await powerCard.findAll(".mapping-cell")[0]!.trigger("click");
    const safeToggle = wrapper.find(".safe-capture-toggle input");
    const shortcutRow = wrapper.find(".custom-shortcut-row");
    const toggleRow = wrapper.find(".safe-capture-toggle");
    expect(shortcutRow.element.nextElementSibling).toBe(toggleRow.element);
    expect(safeToggle.classes()).toContain("toggle-input");
    expect(safeToggle.element.nextElementSibling?.textContent).toContain(
      "直接录入无法完成或会触发系统动作时再开启",
    );
    expect((safeToggle.element as HTMLInputElement).checked).toBe(false);
    await safeToggle.setValue(true);
    const captureButton = wrapper
      .findAll(".mapping-editor .chip")
      .find((button) => button.text().includes("录入自定义快捷键"))!;
    await captureButton.trigger("click");
    await vi.waitFor(() => expect(startShortcutCapture).toHaveBeenCalledOnce());

    const leftWin = wrapper
      .findAll(".capture-modifiers .chip")
      .find((button) => button.text() === "左 Win")!;
    await leftWin.trigger("click");
    shortcutCaptureHandler!({ key: "l", isPressed: true });
    await vi.waitFor(() => {
      const calls = vi.mocked(saveButtonMappings).mock.calls;
      const saved = calls.at(-1)?.[0] as ButtonMappings | undefined;
      const action = saved?.actions.power?.single;
      if (action?.type !== "shortcut" || action.chord.keys.join("+") !== "left_windows+l") {
        throw new Error("Win+L 未保存");
      }
    });
    await vi.waitFor(() =>
      expect(wrapper.text()).toContain("已录入 左 Win + L，松开全部按键后完成"),
    );
    shortcutCaptureHandler!({ key: "l", isPressed: false });
    await vi.waitFor(() => expect(stopShortcutCapture).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(wrapper.text()).toContain("快捷键已录入：左 Win + L"));
  });

  it("highlights the card for a pressed physical button and clears it on release", async () => {
    const wrapper = await mountPage();
    const upCard = () =>
      wrapper.findAll(".mapping-card").find((card) => card.text().includes("上"));
    expect(upCard()!.classes()).not.toContain("active");

    edgeHandler!({ button: "up", isPressed: true });
    await vi.waitFor(() => {
      if (!upCard()!.classes().includes("active")) throw new Error("未高亮");
    });
    edgeHandler!({ button: "up", isPressed: false });
    await vi.waitFor(() => {
      if (upCard()!.classes().includes("active")) throw new Error("未解除高亮");
    });
  });

  it("keeps the selection locked while pressing the remote unless unlocked", async () => {
    const wrapper = await mountPage();
    // 默认锁定：按下"返回"不改变当前选中（未选中任何键时仍为空）。
    edgeHandler!({ button: "back", isPressed: true });
    const backCard = () =>
      wrapper.findAll(".mapping-card").find((card) => card.text().includes("返回"));
    await vi.waitFor(() => {
      if (!backCard()!.classes().includes("active")) throw new Error("未高亮");
    });
    expect(backCard()!.classes()).not.toContain("selected");

    // 解锁后：按下即选中该键的编辑。
    const toggles = wrapper.findAll(".toggle-row");
    const lockToggle = toggles.find((row) => row.text().includes("锁定当前按键"));
    const input = lockToggle!.find("input");
    await input.setValue(false);
    edgeHandler!({ button: "back", isPressed: true });
    await vi.waitFor(() => {
      if (!backCard()!.classes().includes("selected")) throw new Error("未跟随选中");
    });
  });

  it("shows the fired gesture feedback from engine events", async () => {
    const wrapper = await mountPage();
    gestureHandler!({ button: "ok", trigger: "single" });
    await vi.waitFor(() => {
      // 手势反馈 = 对应格子出现闪烁态（flashed），600ms 后自动消失。
      if (!wrapper.find(".mapping-cell.flashed").exists()) {
        throw new Error("手势触发后格子未出现闪烁反馈");
      }
    });
  });

  /** 编辑器内按标签找 chip 并返回其禁用态。 */
  function chipState(wrapper: VueWrapper, label: string): boolean {
    const chip = wrapper
      .findAll(".mapping-editor .chip")
      .find((element) => element.text().includes(label));
    expect(chip, `未找到 chip：${label}`).toBeDefined();
    return (chip!.element as HTMLButtonElement).disabled;
  }

  async function openCell(
    wrapper: VueWrapper,
    cardLabel: string,
    triggerIndex: number,
  ): Promise<void> {
    const card = wrapper.findAll(".mapping-card").find((c) => c.text().includes(cardLabel));
    expect(card, `未找到卡片：${cardLabel}`).toBeDefined();
    await card!.findAll(".mapping-cell")[triggerIndex]!.trigger("click");
    expect(wrapper.find(".mapping-editor").exists()).toBe(true);
  }

  it("全开放：确定·单击所有操作可配（注入链路已真机验证）+ 单响应提示", async () => {
    const wrapper = await mountPage();
    await openCell(wrapper, "确定", 0);
    expect(chipState(wrapper, "Enter")).toBe(false);
    expect(chipState(wrapper, "Home")).toBe(false);
    expect(chipState(wrapper, "空格")).toBe(false);
    expect(chipState(wrapper, "Ctrl + V")).toBe(false);
    expect(chipState(wrapper, "录入自定义快捷键")).toBe(false);
    expect(chipState(wrapper, "＋ 添加应用")).toBe(false);
    // 武装族按键显示冷首按副作用提示（信息性，不门控）。
    expect(wrapper.find(".mapping-editor").text()).toContain("它原本的按键效果");
  });

  it("预设芯片显示实际按键组合，功能描述退为悬停提示", async () => {
    const wrapper = await mountPage();
    await openCell(wrapper, "电源", 0);
    const chips = wrapper.findAll(".mapping-editor .chip");
    const texts = chips.map((chip) => chip.text());

    expect(texts).toContain("Ctrl + C");
    expect(texts).toContain("Alt + Tab");
    expect(texts).toContain("Backspace");
    expect(texts).toContain("左 Win + Shift + S");
    expect(texts).not.toContain("复制");
    expect(texts).not.toContain("退格");
    expect(texts).not.toContain("截图");
    expect(chips.find((chip) => chip.text() === "Ctrl + C")!.attributes("title")).toBe("复制");
    expect(chips.find((chip) => chip.text() === "Alt + Tab")!.attributes("title")).toBe(
      "切换窗口",
    );
  });

  it("全开放：确定·双击与 TV 所有操作可配 + 各自的单响应提示", async () => {
    const wrapper = await mountPage();
    await openCell(wrapper, "确定", 1);
    expect(chipState(wrapper, "Enter")).toBe(false);
    expect(chipState(wrapper, "录入自定义快捷键")).toBe(false);
    expect(chipState(wrapper, "＋ 添加应用")).toBe(false);
    expect(wrapper.find(".mapping-editor").text()).toContain("它原本的按键效果");

    await openCell(wrapper, "TV", 0);
    expect(chipState(wrapper, "Enter")).toBe(false);
    expect(chipState(wrapper, "静音")).toBe(false);
    expect(chipState(wrapper, "录入自定义快捷键")).toBe(false);
    expect(chipState(wrapper, "＋ 添加应用")).toBe(false);
    expect(wrapper.find(".mapping-editor").text()).toContain("这个键只执行你配置的动作");
  });

  it("左键与其余方向键同样开放自定义并显示结构性泄漏提示", async () => {
    const wrapper = await mountPage();
    await openCell(wrapper, "左", 0);
    expect(chipState(wrapper, "←")).toBe(false);
    expect(chipState(wrapper, "Backspace")).toBe(false);
    expect(chipState(wrapper, "录入自定义快捷键")).toBe(false);
    expect(wrapper.find(".mapping-editor").text()).toContain("它原本的按键效果");

    // 与型号无关：RC001 上左键同样开放。
    const rc001 = await mountPage("rc001");
    const leftCellRc001 = rc001
      .findAll(".mapping-card")
      .find((c) => c.text().includes("左"))!
      .findAll(".mapping-cell")[0]!;
    expect((leftCellRc001.element as HTMLButtonElement).disabled).toBe(false);
  });

  it("电源（直接归因族）全开放且无单响应提示", async () => {
    const wrapper = await mountPage();
    await openCell(wrapper, "电源", 2);
    expect(chipState(wrapper, "Esc")).toBe(false);
    expect(chipState(wrapper, "左 Win + Shift + S")).toBe(false);
    expect(chipState(wrapper, "录入自定义快捷键")).toBe(false);
    expect(chipState(wrapper, "＋ 添加应用")).toBe(false);
    expect(wrapper.find(".mapping-editor").text()).not.toContain("原生按键动作");
  });

  it("返回/音量±全型号开放自定义", async () => {
    for (const model of ["rc003", "rc001", "unknown"] as const) {
      const wrapper = await mountPage(model);
      const backCell = wrapper
        .findAll(".mapping-card")
        .find((c) => c.text().includes("返回"))!
        .findAll(".mapping-cell")[0]!;
      expect((backCell.element as HTMLButtonElement).disabled, `${model} 返回格子应启用`).toBe(false);
      const volumeCell = wrapper
        .findAll(".mapping-card")
        .find((c) => c.text().includes("音量"))!
        .findAll(".mapping-cell")[0]!;
      expect((volumeCell.element as HTMLButtonElement).disabled, `${model} 音量格子应启用`).toBe(false);
      await backCell.trigger("click");
      expect(wrapper.find(".mapping-editor").exists()).toBe(true);
    }
  });

  it("返回/音量±按型号如实标注源头捕获能力（不静默降级）", async () => {
    // 未授权：如实指向开关，而不是沿用任何旧占位文案。
    vi.mocked(getRc003TaskStatus).mockResolvedValue({
      installed: false,
      authorizationRequired: true,
      enabled: false,
      helperPath: null,
      lastError: null,
    });
    const rc003 = await mountPage("rc003");
    const rc003Back = rc003
      .findAll(".mapping-card")
      .find((c) => c.text().includes("返回"))!
      .findAll(".mapping-cell")[0]!;
    await rc003Back.trigger("click");
    await vi.waitFor(
      () => {
        // 2026-09-28 定稿：未开启时整句就是一句话（细节由确认弹窗承载）。
        expect(rc003.find(".capability-note").text()).toBe(TRI_KEY_HINT);
      },
      { timeout: 4000 },
    );

    // 已授权：如实说"现在生效"，且绝不回到旧占位文案。
    vi.mocked(getRc003TaskStatus).mockResolvedValue({
      installed: true,
      authorizationRequired: false,
      enabled: true,
      helperPath: null,
      lastError: null,
    });
    const rc003On = await mountPage("rc003");
    const rc003OnBack = rc003On
      .findAll(".mapping-card")
      .find((c) => c.text().includes("返回"))!
      .findAll(".mapping-cell")[0]!;
    await rc003OnBack.trigger("click");
    await vi.waitFor(
      () => {
        expect(rc003On.find(".capability-note").text()).toContain("映射现在生效");
      },
      { timeout: 4000 },
    );
    expect(rc003On.find(".capability-note").text()).not.toContain(
      "不进 Windows 输入栈",
    );

    // 2026-09-28 定稿：开启态下其他按键走「已优化」口径。
    const rc003OnPower = rc003On
      .findAll(".mapping-card")
      .find((c) => c.text().includes("电源"))!
      .findAll(".mapping-cell")[0]!;
    await rc003OnPower.trigger("click");
    await vi.waitFor(
      () => {
        expect(rc003On.find(".capability-note").text()).toBe(
          "全按键支持已启用，此按键的映射已优化",
        );
      },
      { timeout: 4000 },
    );

    // 型号不再区分（2026-09-24 产品决策）：RC001 上同一个开关状态驱动的
    // 说明，RC001 专属文案（VK 0xFF）不回流。
    const rc001 = await mountPage("rc001");
    const rc001Volume = rc001
      .findAll(".mapping-card")
      .find((c) => c.text().includes("音量"))!
      .findAll(".mapping-cell")[0]!;
    await rc001Volume.trigger("click");
    expect(rc001.find(".capability-note").text()).not.toContain("VK 0xFF");
  });

  it("全按键支持开关常驻页面头部，不依赖编辑面板（2026-09-27 挪至标题行）", async () => {
    vi.mocked(getRc003TaskStatus).mockResolvedValue({
      installed: true,
      authorizationRequired: false,
      enabled: true,
      helperPath: null,
      lastError: null,
    });
    const page = await mountPage("rc003");
    const home = page
      .findAll(".mapping-card")
      .find((card) => card.find(".mapping-card-title strong").text() === "主页")!;
    await home.findAll(".mapping-cell")[0]!.trigger("click");
    await vi.waitFor(() => {
      expect(captureRow(page)).toBeDefined();
    });
    // 开启态「其他按键」（主页）走 2026-09-28 定稿的「已优化」口径；
    // 本用例重点是 note 常驻头部、不依赖编辑面板展开路径。
    await vi.waitFor(() => {
      expect(page.find(".capability-note").text()).toBe(
        "全按键支持已启用，此按键的映射已优化",
      );
    });
  });

  it("全按键支持开关使用与「启用自定义按键功能」一致的 Switch 样式（2026-09-28）", async () => {
    const page = await mountPage("rc003");
    const checkbox = captureRow(page)!.find('input[type="checkbox"]');
    expect(checkbox.classes()).toContain("toggle-input");
  });

  it("桥接状态收进开关旁的行内圆点，不再插入独立状态行（2026-09-28 抖动修复）", async () => {
    vi.mocked(getRc003TaskStatus).mockResolvedValue({
      installed: true,
      authorizationRequired: false,
      enabled: true,
      helperPath: null,
      lastError: null,
    });
    const page = await mountPage("rc003");

    // listening：黄点 + 悬停文案；页面上不存在独立的胶囊状态行。
    vi.mocked(getRc003BridgeSnapshot).mockResolvedValue(bridgeSnapshot("listening"));
    await vi.waitFor(
      () => {
        const dot = captureRow(page)!.find(".status-dot");
        expect(dot.exists()).toBe(true);
        expect(dot.classes()).toContain("pending");
        expect(dot.attributes("title")).toContain("正在启动");
      },
      { timeout: 3000 },
    );
    expect(page.findAll(".device-chip").filter((chip) => chip.text().includes("全按键支持"))).toHaveLength(0);

    // connected：绿点。同样不允许出现独立状态行。
    vi.mocked(getRc003BridgeSnapshot).mockResolvedValue(bridgeSnapshot("connected"));
    await vi.waitFor(
      () => {
        expect(captureRow(page)!.find(".status-dot").classes()).toContain("success");
      },
      { timeout: 3000 },
    );
    expect(page.findAll(".device-chip").filter((chip) => chip.text().includes("全按键支持"))).toHaveLength(0);
  });

  it("状态圆点只在开关打开时出现（2026-09-28 Andy 要求）：关闭时即使桥接快照在也不显示", async () => {
    vi.mocked(getRc003TaskStatus).mockResolvedValue({
      installed: true,
      authorizationRequired: false,
      enabled: false,
      helperPath: null,
      lastError: null,
    });
    // 桥接快照故意给 connected：旧实现只要 rc003BridgeText 非空就画点，
    // 开关关着也亮绿点——本用例就是那条行为的阳性对照。
    vi.mocked(getRc003BridgeSnapshot).mockResolvedValue(bridgeSnapshot("connected"));
    const page = await mountPage("rc003");
    await vi.waitFor(
      () => {
        expect(captureRow(page)!.find('input[type="checkbox"]').exists()).toBe(true);
      },
      { timeout: 3000 },
    );
    expect(captureRow(page)!.find(".status-dot").exists()).toBe(false);
  });

  it("状态未就绪时渲染同尺寸占位符、不渲染开关本体（与「启动行为」同法的无动画挂载，2026-09-28）", async () => {
    // 对账失败 → rc003CaptureEnabled 保持 null → 只允许占位符顶位。
    vi.mocked(getRc003TaskStatus).mockRejectedValue(new Error("ipc unavailable"));
    const page = await mountPage("rc003");
    const row = captureRow(page)!;
    expect(row.find('input[type="checkbox"]').exists()).toBe(false);
    expect(row.find(".toggle-placeholder").exists()).toBe(true);
  });

  it("缓存命中时进页首帧即渲染开关与圆点终值，不等 IPC（2026-09-28 Andy 要求：无延迟、无灰→绿跳变）", () => {
    localStorage.setItem(
      "sayall.rc003Capture.uiCache",
      JSON.stringify({ enabled: true, phase: "connected" }),
    );
    // 状态与桥接快照的 IPC 永不返回：旧行为里开关要等首次对账（串在映射
    // 加载 + 三次订阅之后）、圆点要等 1 秒轮询首跳——本用例证明两者都在
    // 首帧就位，IPC 迟到不产生可见的空档或状态跳变。
    vi.mocked(getRc003TaskStatus).mockImplementation(() => new Promise(() => {}));
    vi.mocked(getRc003BridgeSnapshot).mockImplementation(() => new Promise(() => {}));
    const page = mount(ButtonsPage, { props: { runtime } });
    const row = captureRow(page)!;
    const checkbox = row.find('input[type="checkbox"]');
    expect(checkbox.exists()).toBe(true);
    expect((checkbox.element as HTMLInputElement).checked).toBe(true);
    const dot = row.find(".status-dot");
    expect(dot.exists()).toBe(true);
    expect(dot.classes()).toContain("success");
    page.unmount();
  });

  it("缓存与权威状态不一致时以对账结果为准并回写（卸载后重装回落关闭，2026-09-28）", async () => {
    localStorage.setItem(
      "sayall.rc003Capture.uiCache",
      JSON.stringify({ enabled: true, phase: "connected" }),
    );
    // 卸载后重装：授权已撤销（authorizationRequired），持久化意图回落为关。
    vi.mocked(getRc003TaskStatus).mockResolvedValue({
      installed: false,
      authorizationRequired: true,
      enabled: false,
      helperPath: null,
      lastError: null,
    });
    const page = await mountPage("rc003");
    await vi.waitFor(
      () => {
        const checkbox = captureRow(page)!.find('input[type="checkbox"]');
        expect((checkbox.element as HTMLInputElement).checked).toBe(false);
      },
      { timeout: 3000 },
    );
    const cached = JSON.parse(localStorage.getItem("sayall.rc003Capture.uiCache") ?? "null") as {
      enabled?: unknown;
    };
    expect(cached?.enabled).toBe(false);
  });

  it("状态与桥接快照就绪后写入缓存，供下次进页首帧渲染（2026-09-28）", async () => {
    vi.mocked(getRc003TaskStatus).mockResolvedValue({
      installed: true,
      authorizationRequired: false,
      enabled: true,
      helperPath: null,
      lastError: null,
    });
    vi.mocked(getRc003BridgeSnapshot).mockResolvedValue(bridgeSnapshot("connected"));
    const page = await mountPage("rc003");
    await vi.waitFor(
      () => {
        const cached = JSON.parse(
          localStorage.getItem("sayall.rc003Capture.uiCache") ?? "null",
        ) as { enabled?: unknown; phase?: unknown } | null;
        expect(cached).toMatchObject({ enabled: true, phase: "connected" });
      },
      { timeout: 3000 },
    );
    page.unmount();
  });

  it("缓存内容非法（垃圾/越界值）时忽略，回落占位符与旧行为（2026-09-28）", () => {
    localStorage.setItem(
      "sayall.rc003Capture.uiCache",
      JSON.stringify({ enabled: "yes", phase: "hacked" }),
    );
    vi.mocked(getRc003TaskStatus).mockImplementation(() => new Promise(() => {}));
    const page = mount(ButtonsPage, { props: { runtime } });
    const row = captureRow(page)!;
    expect(row.find('input[type="checkbox"]').exists()).toBe(false);
    expect(row.find(".toggle-placeholder").exists()).toBe(true);
    expect(row.find(".status-dot").exists()).toBe(false);
    page.unmount();
  });

  it("桥接段异步失败：红点 + 底部提示条给出失败文案（开关已开、无法走 toggle 失败分支）", async () => {
    vi.mocked(getRc003TaskStatus).mockResolvedValue({
      installed: true,
      authorizationRequired: false,
      enabled: true,
      helperPath: null,
      lastError: null,
    });
    const page = await mountPage("rc003");
    vi.mocked(getRc003BridgeSnapshot).mockResolvedValue(bridgeSnapshot("failed"));
    await vi.waitFor(
      () => {
        const dot = captureRow(page)!.find(".status-dot");
        expect(dot.exists()).toBe(true);
        expect(dot.classes()).toContain("error");
      },
      { timeout: 3000 },
    );
    await vi.waitFor(() => {
      expect(page.text()).toContain("全按键支持开启失败");
    });
  });

  it("UAC 被取消（enable 拒绝）时开关保持关闭、显示错误（2026-09-24 用户报告）", async () => {
    // 复现链（升级/重装后）：任务删不掉但重授权标记在 → 开关回落关闭 →
    // 用户打开 → 确认弹窗点「开启」→ UAC → 选「否」→ enable 拒绝。
    // 此时开关绝不能翻成开启。
    vi.mocked(getRc003TaskStatus).mockResolvedValue({
      installed: true,
      authorizationRequired: true,
      enabled: false,
      helperPath: null,
      lastError: null,
    });
    vi.mocked(enableRc003Capture).mockRejectedValue(
      new Error("启用 RC003 三键捕获失败：授权未完成（UAC 被取消或安装失败）"),
    );
    const page = await mountPage("rc003");
    const back = page
      .findAll(".mapping-card")
      .find((c) => c.text().includes("返回"))!
      .findAll(".mapping-cell")[0]!;
    await back.trigger("click");
    // 页面有多个 toggle-row（总开关/全按键支持/锁定选择），必须按文本定位。
    await vi.waitFor(() => {
      expect(
        page
          .findAll(".toggle-row")
          .filter((row) => row.text().includes("全按键支持")).length,
      ).toBe(1);
    });
    const checkbox = page
      .findAll(".toggle-row")
      .filter((row) => row.text().includes("全按键支持"))[0]
      .find('input[type="checkbox"]');
    // 等初始化完成（rc003CaptureEnabled !== null 之前 checkbox 是 disabled，
    // disabled 元素的 change 事件不会触发——这正是要测的行为的前提）。
    await vi.waitFor(() => {
      expect((checkbox.element as HTMLInputElement).disabled).toBe(false);
    });
    expect((checkbox.element as HTMLInputElement).checked).toBe(false);
    // 忠实模拟浏览器：点击真实复选框时，**DOM 先被浏览器翻成勾选**，然后
    // 才派发 change 事件。原来只 trigger("change") 永远复现不出
    // 「状态是关、界面是开」——Vue 判定 :checked 前后都 false 而不打补丁。
    (checkbox.element as HTMLInputElement).checked = true;
    await checkbox.trigger("change");
    // 每次开启都先弹确认：点「开启」后才会真正调用 enable。
    const uacDialog = confirmDialog(page);
    expect(uacDialog).toBeDefined();
    await uacDialog!.findAll("button").find((b) => b.text() === "开启")!.trigger("click");
    await vi.waitFor(() => {
      // 错误走到页面既有的提示条……
      expect(page.text()).toContain("UAC 被取消");
    });
    // ……且开关仍是关闭：实际系统状态没变，开关翻过去就是"证据说谎"。
    const captureRow = page
      .findAll(".toggle-row")
      .filter((row) => row.text().includes("全按键支持"))[0];
    expect(
      (captureRow.find('input[type="checkbox"]').element as HTMLInputElement)
        .checked,
    ).toBe(false);
  });
});

describe("全按键支持开启前确认弹窗", () => {
  async function openCaptureToggle(page: VueWrapper) {
    const back = page
      .findAll(".mapping-card")
      .find((c) => c.text().includes("返回"))!
      .findAll(".mapping-cell")[0]!;
    await back.trigger("click");
    await vi.waitFor(() => {
      expect(
        page
          .findAll(".toggle-row")
          .filter((row) => row.text().includes("全按键支持")).length,
      ).toBe(1);
    });
    const checkbox = captureRow(page)!.find('input[type="checkbox"]');
    // CI 双核慢机余量：授权状态已改为挂载后立即对账（微任务），正常瞬时可过；
    // 4000ms 只防无关步骤偶发慢（与下方 capability-note 用例同一口径）。
    await vi.waitFor(
      () => {
        expect((checkbox.element as HTMLInputElement).disabled).toBe(false);
      },
      { timeout: 4000 },
    );
    return checkbox;
  }

  function captureCheckboxChecked(page: VueWrapper): boolean {
    // 每次重新取元素：Vue 重渲染可能替换 DOM 节点，早先拿到的引用会变成
    // 已卸载节点，读到过期状态（实测约 1/3 概率让本用例假失败）。
    return (captureRow(page)!.find('input[type="checkbox"]').element as HTMLInputElement)
      .checked;
  }

  beforeEach(() => {
    vi.mocked(enableRc003Capture).mockClear();
    vi.mocked(disableRc003Capture).mockClear();
  });

  it("开关悬停提示随状态切换；弹窗文案不再出现内部视角表述", async () => {
    const page = await mountPage("rc003");
    await openCaptureToggle(page);

    // 2026-09-28 定稿：关闭态指向「开启后支持使用…」。
    expect(captureRow(page)!.attributes("title")).toBe(
      CAPTURE_SWITCH_OFF_TITLE,
    );

    const checkbox = captureRow(page)!.find('input[type="checkbox"]');
    (checkbox.element as HTMLInputElement).checked = true;
    await checkbox.trigger("change");
    await flushPromises();

    // 默认 mock 是「未授权」场景：change 先弹确认弹窗，开关此时尚未
    // 真正开启——title 保持关闭态文案是正确行为。
    const dialog = confirmDialog(page);
    expect(dialog).toBeDefined();
    // 2026-09-27 用户要求去掉「Windows 平时看不见它们」。
    expect(dialog!.text()).not.toContain("Windows 平时看不见它们");
    // 2026-09-28 二次定稿：授权语义改成「升级保留、卸载后重装才撤销」。
    expect(dialog!.text()).toContain("升级/覆盖安装后无需重新授权");
    expect(dialog!.text()).toContain("卸载后重装才需要");
    expect(dialog!.text()).toContain("防作弊");

    // 点弹窗「开启」完成授权 → 真正开启后悬停提示切到「关闭后…将不可映射」。
    await dialog!
      .findAll("button")
      .find((b) => b.text() === "开启")!
      .trigger("click");
    await flushPromises();
    await vi.waitFor(() => {
      expect(captureRow(page)!.attributes("title")).toBe(
        CAPTURE_SWITCH_ON_TITLE,
      );
    });
  });

  it("需要授权的开启才先弹确认；授权已在时直接开启，不再打扰", async () => {
    const page = await mountPage("rc003");
    const checkbox = await openCaptureToggle(page);

    // ── 场景 A：任务未注册（首次开启）→ 先弹确认 ──
    // 浏览器先把 DOM 翻成勾选，再派发 change（与既有用例同一保真写法）。
    (checkbox.element as HTMLInputElement).checked = true;
    await checkbox.trigger("change");
    await flushPromises();

    const dialog = confirmDialog(page);
    expect(dialog).toBeDefined();
    expect(vi.mocked(enableRc003Capture)).not.toHaveBeenCalled();

    // 取消：不开启，开关保持关闭。
    await dialog!.findAll("button").find((b) => b.text() === "取消")!.trigger("click");
    await flushPromises();
    expect(confirmDialog(page)).toBeUndefined();
    expect(vi.mocked(enableRc003Capture)).not.toHaveBeenCalled();
    expect(captureCheckboxChecked(page)).toBe(false);

    // 再次点开：仍先弹（还没授权）。
    (checkbox.element as HTMLInputElement).checked = true;
    await checkbox.trigger("change");
    await flushPromises();
    const second = confirmDialog(page);
    expect(second).toBeDefined();

    // 确认：真正开启。
    await second!.findAll("button").find((b) => b.text() === "开启")!.trigger("click");
    await flushPromises();
    expect(vi.mocked(enableRc003Capture)).toHaveBeenCalledTimes(1);
    await vi.waitFor(() => {
      expect(captureCheckboxChecked(page)).toBe(true);
    });

    // ── 场景 B：授权已在（关闭只结束助手、任务保留）→ 直接开启不弹 ──
    // 轮询状态同步改为「授权已在、开关关着」，与真实系统一致；先改 mock
    // 再操作，避免秒级轮询用旧状态覆盖 rc003Task 造成竞态。
    vi.mocked(getRc003TaskStatus).mockResolvedValue({
      installed: true,
      authorizationRequired: false,
      enabled: false,
      helperPath: null,
      lastError: null,
    });
    const offSwitch = captureRow(page)!.find('input[type="checkbox"]');
    (offSwitch.element as HTMLInputElement).checked = false;
    await offSwitch.trigger("change");
    await flushPromises();
    const onSwitch = captureRow(page)!.find('input[type="checkbox"]');
    (onSwitch.element as HTMLInputElement).checked = true;
    await onSwitch.trigger("change");
    await flushPromises();
    expect(confirmDialog(page)).toBeUndefined();
    expect(vi.mocked(enableRc003Capture)).toHaveBeenCalledTimes(2);
    await vi.waitFor(() => {
      expect(captureCheckboxChecked(page)).toBe(true);
    });
  });

  it("升级/重装后（任务删不掉但重授权标记在）：仍先弹确认再开启", async () => {
    // 这正是 2026-09-27 报告的缺陷场景：旧实现把「已读过」记在 localStorage，
    // 重装后标记仍在 → 弹窗消失；新判据 authorizationRequired 由安装器标记
    // 驱动，与「已读过」无关——重装后那次开启必然先弹。
    vi.mocked(getRc003TaskStatus).mockResolvedValue({
      installed: true,
      authorizationRequired: true,
      enabled: false,
      helperPath: null,
      lastError: null,
    });
    const page = await mountPage("rc003");
    const checkbox = await openCaptureToggle(page);
    (checkbox.element as HTMLInputElement).checked = true;
    await checkbox.trigger("change");
    await flushPromises();

    const dialog = confirmDialog(page);
    expect(dialog).toBeDefined();
    expect(vi.mocked(enableRc003Capture)).not.toHaveBeenCalled();

    // 确认后开启（UAC 在这次 IPC 里发生）。
    await dialog!.findAll("button").find((b) => b.text() === "开启")!.trigger("click");
    await flushPromises();
    expect(vi.mocked(enableRc003Capture)).toHaveBeenCalledTimes(1);
    await vi.waitFor(() => {
      expect(captureCheckboxChecked(page)).toBe(true);
    });
  });

  it("挂载后立即对账授权状态：不等 1 秒轮询首跳，开关即可用", async () => {
    // PR #132 CI 实测：授权状态只在挂载 1 秒后的首次 interval 轮询里落地，
    // CI 慢机上 openCaptureToggle 的 waitFor（默认 1000ms）被压线超时；
    // 真机上则是「进页面头 1 秒开关点不动、无解释」。挂载后必须立即拉一次。
    const page = await mountPage("rc003");
    // 只清微任务队列、不推进真实时间：1 秒后的首次 interval 轮询不会跑。
    // 若授权状态仍依赖轮询首跳，下面的断言必失败。
    await flushPromises();
    const back = page
      .findAll(".mapping-card")
      .find((c) => c.text().includes("返回"))!
      .findAll(".mapping-cell")[0]!;
    await back.trigger("click");
    await flushPromises();
    const row = page
      .findAll(".toggle-row")
      .filter((r) => r.text().includes("全按键支持"))[0]!;
    expect(
      (row.find('input[type="checkbox"]').element as HTMLInputElement).disabled,
    ).toBe(false);
  });

  // 2026-10-01 Andy 要求：头部设备胶囊显示遥控器型号（而不是蓝牙广播名）。
  it.each([
    ["rc001" as const, "小米蓝牙语音遥控器 2"],
    ["rc003" as const, "小米蓝牙语音遥控器 2 Pro"],
  ])("头部设备胶囊显示遥控器型号（%s）", async (model, expected) => {
    const page = await mountPage(model);
    expect(page.find(".device-chip").text()).toContain(expected);
    page.unmount();
  });

  it("型号未读回时头部设备胶囊退回蓝牙广播名", async () => {
    const page = await mountPage("unknown");
    expect(page.find(".device-chip").text()).toContain("小米蓝牙语音遥控器");
    expect(page.find(".device-chip").text()).not.toContain("连接后显示");
    page.unmount();
  });
});
