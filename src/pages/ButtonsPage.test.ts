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
    getUserHidSnapshot: vi.fn(async () => ({ available: true, phase: "stopped", reason: null, scope: null, cleanupConfirmed: false })),
    startUserHid: vi.fn(async () => ({ available: true, phase: "starting", reason: null, scope: null, cleanupConfirmed: false })),
    stopUserHid: vi.fn(async () => ({ available: true, phase: "stopped", reason: null, scope: null, cleanupConfirmed: true })),
    scanRegisteredApps: vi.fn(async () => [{ name: "Registered Example", path: "shell:AppsFolder\\Example!App" }]),
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
  };
});

import {
  startUserHid,
  stopUserHid,
  exportButtonMappingConfiguration,
  getButtonMappings,
  importButtonMappingConfiguration,
  subscribeButtonEdges,
  subscribeButtonGestures,
  saveButtonMappings,
  startShortcutCapture,
  stopShortcutCapture,
} from "../lib/bridge";
import type { ButtonMappings, RuntimeSnapshot } from "../lib/bridge";

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
  vi.mocked(exportButtonMappingConfiguration).mockClear();
  vi.mocked(importButtonMappingConfiguration).mockClear();
  vi.mocked(startShortcutCapture).mockClear();
  vi.mocked(stopShortcutCapture).mockClear();
});

describe("buttons mapping page", () => {
  it("serializes rapid amount edits without letting a stale save reset the latest direction", async () => {
    const wrapper = await mountPage();
    const card = wrapper.findAll(".mapping-card").find(item => item.find(".mapping-card-title strong").text() === "电源")!;
    await card.findAll(".mapping-cell")[0]!.trigger("click");
    let release!: (value: ButtonMappings) => void;
    let first!: ButtonMappings;
    vi.mocked(saveButtonMappings).mockImplementationOnce(value => { first = value; return new Promise(resolve => { release = resolve; }); });
    const choose = (label: string) => wrapper.findAll(".mapping-editor button").find(item => item.text() === label)!.trigger("click");
    await choose("滚轮向上"); await flushPromises();
    await wrapper.get('input[aria-label="每次滚动格数"]').setValue("7");
    await choose("滚轮向下");
    release(first); await flushPromises();
    expect(vi.mocked(saveButtonMappings).mock.lastCall![0].actions.power?.single).toEqual({ type: "scroll", direction: "down", steps: 7 });
    expect(card.text()).toContain("滚轮向下 7 格");
    wrapper.unmount();
  });
  it("adds scanned apps to the configuration library without assigning or changing buttons", async () => {
    const wrapper = await mountPage();
    const card = wrapper.findAll(".mapping-card").find(item => item.find(".mapping-card-title strong").text() === "电源")!;
    await card.findAll(".mapping-cell")[0]!.trigger("click");
    await wrapper.findAll("button").find(button => button.text() === "扫描本机应用")!.trigger("click");
    await flushPromises();
    await wrapper.get('input[aria-label="全选当前结果"]').setValue(true);
    await wrapper.get(".registered-apps-dialog .primary-button").trigger("click");
    await flushPromises();
    const saved = vi.mocked(saveButtonMappings).mock.lastCall![0];
    expect(saved.applications).toEqual([{ name: "Registered Example", path: "shell:AppsFolder\\Example!App" }]);
    expect(saved.actions.power).toBeUndefined();
    expect(saved.actions.ok?.single).toEqual({ type: "shortcut", chord: { keys: ["enter"] } });
    expect(wrapper.find(".registered-apps-dialog").exists()).toBe(false);
    const option = wrapper.findAll(".saved-app-grid button").find(button => button.text() === "Registered Example")!;
    await option.trigger("click"); await flushPromises();
    expect(vi.mocked(saveButtonMappings).mock.lastCall![0].actions.power?.single).toEqual({ type: "open_app", target: "shell:AppsFolder\\Example!App" });
    wrapper.unmount();
  });
  it("keeps wheel amounts independent per cell and rejects invalid input", async () => {
    const wrapper = await mountPage();
    const card = wrapper.findAll(".mapping-card").find(item => item.find(".mapping-card-title strong").text() === "电源")!;
    const choose = async (label: string) => { await wrapper.findAll(".mapping-editor button").find(item => item.text() === label)!.trigger("click"); await flushPromises(); };
    await card.findAll(".mapping-cell")[0]!.trigger("click");
    await choose("滚轮向下");
    await wrapper.get('input[aria-label="每次滚动格数"]').setValue("5");
    await flushPromises();
    expect(vi.mocked(saveButtonMappings).mock.lastCall![0].actions.power?.single).toEqual({ type: "scroll", direction: "down", steps: 5 });
    const saves = vi.mocked(saveButtonMappings).mock.calls.length;
    for (const invalid of ["0", "101", "1.5", ""]) {
      await wrapper.get('input[aria-label="每次滚动格数"]').setValue(invalid); await flushPromises();
    }
    expect(vi.mocked(saveButtonMappings).mock.calls.length).toBe(saves);
    await card.findAll(".mapping-cell")[1]!.trigger("click");
    await choose("滚轮向上");
    expect(vi.mocked(saveButtonMappings).mock.lastCall![0].actions.power?.double).toEqual({ type: "scroll", direction: "up", steps: 1 });
    await card.findAll(".mapping-cell")[0]!.trigger("click");
    expect((wrapper.get('input[aria-label="每次滚动格数"]').element as HTMLInputElement).value).toBe("5");
    wrapper.unmount();
  });

  it("configures all pointer directions, distance, click types, and disabling", async () => {
    const wrapper = await mountPage();
    const card = wrapper.findAll(".mapping-card").find(item => item.find(".mapping-card-title strong").text() === "电源")!;
    await card.findAll(".mapping-cell")[2]!.trigger("click");
    for (const [direction, label] of [["up", "鼠标向上"], ["down", "鼠标向下"], ["left", "鼠标向左"], ["right", "鼠标向右"]]) {
      await wrapper.get(`button[aria-label="${label}"]`).trigger("click"); await flushPromises();
      await wrapper.get('input[aria-label="每次移动像素"]').setValue("75"); await flushPromises();
      expect(vi.mocked(saveButtonMappings).mock.lastCall![0].actions.power?.long).toEqual({ type: "mouse_move", direction, distance: 75 });
    }
    const saves = vi.mocked(saveButtonMappings).mock.calls.length;
    for (const invalid of ["0", "2001", "1.5", ""]) {
      await wrapper.get('input[aria-label="每次移动像素"]').setValue(invalid); await flushPromises();
    }
    expect(vi.mocked(saveButtonMappings).mock.calls.length).toBe(saves);
    for (const [kind, label] of [["left", "左键单击"], ["right", "右键单击"], ["double_left", "左键双击"], ["middle", "中键单击"]]) {
      await wrapper.findAll(".mapping-editor button").find(item => item.text() === label)!.trigger("click"); await flushPromises();
      expect(vi.mocked(saveButtonMappings).mock.lastCall![0].actions.power?.long).toEqual({ type: "mouse_click", kind });
    }
    await wrapper.findAll(".mapping-editor button").find(item => item.text() === "禁用按键")!.trigger("click"); await flushPromises();
    expect(vi.mocked(saveButtonMappings).mock.lastCall![0].actions.power?.long).toEqual({ type: "disabled" });
    expect(vi.mocked(saveButtonMappings).mock.lastCall![0].actions.up).toBeUndefined();
    wrapper.unmount();
  });
  it("offers wheel actions on a non-direction key and lets the user replace or disable them", async () => {
    const wrapper = await mountPage();
    const card = wrapper.findAll(".mapping-card").find((item) =>
      item.find(".mapping-card-title strong").text() === "电源"
    )!;
    await card.findAll(".mapping-cell")[1]!.trigger("click");
    const preset = (label: string) => wrapper.findAll(".mapping-editor button").find((item) => item.text() === label)!;
    await preset("滚轮向下").trigger("click");
    await flushPromises();
    expect(vi.mocked(saveButtonMappings).mock.lastCall![0].actions.power?.double).toEqual({ type: "scroll", direction: "down", steps: 1 });
    expect(vi.mocked(saveButtonMappings).mock.lastCall![0].actions.up).toBeUndefined();
    expect(vi.mocked(saveButtonMappings).mock.lastCall![0].actions.down).toBeUndefined();
    await preset("Enter").trigger("click");
    await flushPromises();
    expect(vi.mocked(saveButtonMappings).mock.lastCall![0].actions.power?.double).toEqual({ type: "shortcut", chord: { keys: ["enter"] } });
    await preset("滚轮向上").trigger("click");
    await flushPromises();
    await preset("禁用按键").trigger("click");
    await flushPromises();
    expect(vi.mocked(saveButtonMappings).mock.lastCall![0].actions.power?.double).toEqual({ type: "disabled" });
    wrapper.unmount();
  });

  it.each(["up", "down"] as const)("saves %s as a wheel action without changing other mappings", async (direction) => {
    const wrapper = await mountPage();
    const label = direction === "up" ? "上" : "下";
    const card = wrapper.findAll(".mapping-card").find((item) =>
      item.find(".mapping-card-title strong").text() === label
    )!;
    await card.findAll(".mapping-cell")[0]!.trigger("click");
    const wheelLabel = direction === "up" ? "滚轮向上" : "滚轮向下";
    const preset = wrapper.findAll(".mapping-editor button").find((item) => item.text() === wheelLabel)!;
    await preset.trigger("click");
    await flushPromises();
    const saved = vi.mocked(saveButtonMappings).mock.lastCall![0];
    expect(saved.actions[direction]).toEqual({
      single: { type: "scroll", direction, steps: 1 },
      double: { type: "disabled" },
      long: { type: "disabled" },
    });
    expect(saved.actions.ok?.single).toEqual({ type: "shortcut", chord: { keys: ["enter"] } });
    expect(preset.classes()).toContain("selected");
    expect(card.text()).toContain(wheelLabel);
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

    // 禁用按键按钮：禁用当前格并自动保存。
    await flushPromises();
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
    expect(chipState(wrapper, "粘贴")).toBe(false);
    expect(chipState(wrapper, "录入自定义快捷键")).toBe(false);
    expect(chipState(wrapper, "＋ 添加应用")).toBe(false);
    // 武装族按键显示冷首按原生副作用提示（信息性，不门控）。
    expect(wrapper.find(".mapping-editor").text()).toContain("原生按键动作");
  });

  it("全开放：确定·双击与 TV 所有操作可配 + 各自的单响应提示", async () => {
    const wrapper = await mountPage();
    await openCell(wrapper, "确定", 1);
    expect(chipState(wrapper, "Enter")).toBe(false);
    expect(chipState(wrapper, "录入自定义快捷键")).toBe(false);
    expect(chipState(wrapper, "＋ 添加应用")).toBe(false);
    expect(wrapper.find(".mapping-editor").text()).toContain("原生按键动作");

    await openCell(wrapper, "TV", 0);
    expect(chipState(wrapper, "Enter")).toBe(false);
    expect(chipState(wrapper, "静音")).toBe(false);
    expect(chipState(wrapper, "录入自定义快捷键")).toBe(false);
    expect(chipState(wrapper, "＋ 添加应用")).toBe(false);
    expect(wrapper.find(".mapping-editor").text()).toContain("遥控器优先");
  });

  it("左键与其余方向键同样开放自定义并显示结构性泄漏提示", async () => {
    const wrapper = await mountPage();
    await openCell(wrapper, "左", 0);
    expect(chipState(wrapper, "←")).toBe(false);
    expect(chipState(wrapper, "退格")).toBe(false);
    expect(chipState(wrapper, "录入自定义快捷键")).toBe(false);
    expect(wrapper.find(".mapping-editor").text()).toContain("原生按键动作");

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
    expect(chipState(wrapper, "截图")).toBe(false);
    expect(chipState(wrapper, "录入自定义快捷键")).toBe(false);
    expect(chipState(wrapper, "＋ 添加应用")).toBe(false);
    expect(wrapper.find(".mapping-editor").text()).not.toContain("原生按键动作");
  });

  it("keeps filter buttons disabled without observed driver pairs on every model", async () => {
    for (const model of ["rc003", "rc001", "unknown"] as const) {
      const wrapper = await mountPage(model);
      const backCell = wrapper
        .findAll(".mapping-card")
        .find((c) => c.text().includes("返回"))!
        .findAll(".mapping-cell")[0]!;
      expect(
        (backCell.element as HTMLButtonElement).disabled,
        `${model} 返回格子应禁用`,
      ).toBe(true);
      const volumeCell = wrapper
        .findAll(".mapping-card")
        .find((c) => c.text().includes("音量"))!
        .findAll(".mapping-cell")[0]!;
      expect(
        (volumeCell.element as HTMLButtonElement).disabled,
        `${model} 音量格子应禁用`,
      ).toBe(true);
      await backCell.trigger("click");
      expect(wrapper.find(".mapping-editor").exists()).toBe(false);
      expect(backCell.attributes("title")).toContain(model === "rc001" ? "仅支持 RC003" : "首次完整按下和松开");
      wrapper.unmount();
    }
  });

  it("requires explicit helper consent and does not stop the helper on page navigation", async () => {
    const wrapper = await mountPage();
    const toggle = wrapper.get('input[aria-label="RC003 增强采集"]');
    await toggle.setValue(true);
    expect(wrapper.get('[role="dialog"]').text()).toContain("反作弊");
    expect(startUserHid).not.toHaveBeenCalled();
    await wrapper.findAll('[role="dialog"] button').find(button => button.text() === "取消")!.trigger("click");
    expect(wrapper.find('[role="dialog"]').exists()).toBe(false);
    expect((toggle.element as HTMLInputElement).checked).toBe(false);
    await toggle.setValue(true);
    await wrapper.findAll('[role="dialog"] button').find(button => button.text() === "同意并开启")!.trigger("click");
    await flushPromises();
    expect(startUserHid).toHaveBeenCalledTimes(1);
    expect((toggle.element as HTMLInputElement).checked).toBe(true);
    wrapper.unmount();
    expect(stopUserHid).not.toHaveBeenCalled();
  });

  it("opens helper-confirmed keys without claiming driver attribution and closes on reset", async () => {
    const wrapper = await mountPage();
    await wrapper.setProps({ runtime: { ...runtime, platform: { ...runtime.platform,
      rawInput: { ...runtime.platform.rawInput, confirmedUserHidButtons: ["back"] },
    } } });
    const back = wrapper.findAll(".mapping-card").find(card => card.find("strong").text() === "返回")!;
    expect(back.text()).toContain("实验信号已确认");
    expect(back.text()).not.toContain("驱动信号已确认");
    await openCell(wrapper, "返回", 0);
    expect(wrapper.get(".mapping-editor").text()).toContain("宿主代理来源");
    await wrapper.setProps({ runtime });
    expect(wrapper.find(".mapping-editor").exists()).toBe(false);
    expect((back.get(".mapping-cell").element as HTMLButtonElement).disabled).toBe(true);
    wrapper.unmount();
  });

  it("opens only confirmed keys and preserves a saved binding after capability loss", async () => {
    const wrapper = await mountPage();
    const card = (label: string) => wrapper.findAll(".mapping-card").find(
      item => item.find(".mapping-card-title strong").text() === label,
    )!;
    const confirmed: RuntimeSnapshot = {
      ...runtime,
      platform: {
        ...runtime.platform,
        rawInput: { ...runtime.platform.rawInput, confirmedFilterButtons: ["volume_up"] },
      },
    };
    await wrapper.setProps({ runtime: confirmed });
    expect(card("音量+").text()).toContain("驱动信号已确认");
    expect((card("音量+").get(".mapping-cell").element as HTMLButtonElement).disabled).toBe(false);
    expect((card("音量−").get(".mapping-cell").element as HTMLButtonElement).disabled).toBe(true);
    expect((card("返回").get(".mapping-cell").element as HTMLButtonElement).disabled).toBe(true);
    await openCell(wrapper, "音量+", 0);
    expect(wrapper.get(".mapping-editor").text()).toContain("不作全局吞键");
    await wrapper.findAll(".mapping-editor button").find(button => button.text() === "滚轮向上")!.trigger("click");
    await flushPromises();
    expect(vi.mocked(saveButtonMappings).mock.lastCall![0].actions.volume_up?.single).toEqual({ type: "scroll", direction: "up", steps: 1 });

    const saves = vi.mocked(saveButtonMappings).mock.calls.length;
    await wrapper.setProps({ runtime });
    expect(wrapper.find(".mapping-editor").exists()).toBe(false);
    expect((card("音量+").get(".mapping-cell").element as HTMLButtonElement).disabled).toBe(true);
    expect(card("音量+").text()).toContain("滚轮向上");
    expect(vi.mocked(saveButtonMappings).mock.calls.length).toBe(saves);
    await wrapper.setProps({ runtime: confirmed });
    await openCell(wrapper, "音量+", 0);
    expect(card("音量+").text()).toContain("滚轮向上");

    await wrapper.setProps({ runtime: {
      ...confirmed,
      platform: { ...confirmed.platform, connection: { ...confirmed.platform.connection, remoteModel: "rc001" } },
    } });
    expect(wrapper.find(".mapping-editor").exists()).toBe(false);
    expect((card("音量+").get(".mapping-cell").element as HTMLButtonElement).disabled).toBe(true);
    wrapper.unmount();
  });
});
