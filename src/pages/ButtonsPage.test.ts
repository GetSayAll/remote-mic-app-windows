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
    // 三键捕获：默认未授权；个别用例用 mockResolvedValue 覆盖授权状态。
    getRc003TaskStatus: vi.fn(async () => ({
      installed: false,
      enabled: false,
      helperPath: null,
      lastError: null,
    })),
    enableRc003Capture: vi.fn(async () => ({
      installed: true,
      enabled: true,
      helperPath: null,
      lastError: null,
    })),
    disableRc003Capture: vi.fn(async () => ({
      installed: false,
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
    })),
  };
});

import {
  exportButtonMappingConfiguration,
  getButtonMappings,
  enableRc003Capture,
  disableRc003Capture,
  getRc003TaskStatus,
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
  vi.mocked(exportButtonMappingConfiguration).mockClear();
  vi.mocked(importButtonMappingConfiguration).mockClear();
  vi.mocked(startShortcutCapture).mockClear();
  vi.mocked(stopShortcutCapture).mockClear();
  // mockClear 只清调用记录，**不清实现**：前面用例留下的 mockRejectedValue /
  // mockResolvedValue 会渗进后面的用例，必须显式重置。
  localStorage.clear();
  vi.mocked(getRc003TaskStatus).mockResolvedValue({
    installed: false,
    enabled: false,
    helperPath: null,
    lastError: null,
  });
  vi.mocked(enableRc003Capture).mockResolvedValue({
    installed: true,
    enabled: true,
    helperPath: null,
    lastError: null,
  });
  vi.mocked(disableRc003Capture).mockResolvedValue({
    installed: false,
    enabled: false,
    helperPath: null,
    lastError: null,
  });
});

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

  it("configures mouse actions with independent validated amounts", async () => {
    const wrapper = await mountPage();
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
    // 武装族按键显示冷首按原生副作用提示（信息性，不门控）。
    expect(wrapper.find(".mapping-editor").text()).toContain("原生按键动作");
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
    expect(chipState(wrapper, "Backspace")).toBe(false);
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
        expect(rc003.find(".capability-note").text()).toContain(
          "需先开启「全按键支持」开关",
        );
      },
      { timeout: 4000 },
    );

    // 已授权：如实说"现在生效"，且绝不回到旧占位文案。
    vi.mocked(getRc003TaskStatus).mockResolvedValue({
      installed: true,
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

  it("UAC 被取消（enable 拒绝）时开关保持关闭、显示错误（2026-09-24 用户报告）", async () => {
    // 复现链：marker 存在 → 开关回落关闭 → 用户打开 → UAC → 选「否」
    // → enable 拒绝。此时开关绝不能翻成开启。
    // （2026-09-26 起首次开启会先弹确认弹窗；本用例只关心 enable 被拒，
    // 故预置"已确认"标记跳过弹窗。）
    localStorage.setItem(CONFIRM_KEY, "1");
    vi.mocked(getRc003TaskStatus).mockResolvedValue({
      installed: true,
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

const CONFIRM_KEY = "sayall.enhancedCapture.confirmShown";

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
    const checkbox = page
      .findAll(".toggle-row")
      .filter((row) => row.text().includes("全按键支持"))[0]
      .find('input[type="checkbox"]');
    await vi.waitFor(() => {
      expect((checkbox.element as HTMLInputElement).disabled).toBe(false);
    });
    return checkbox;
  }

  function confirmDialog(page: VueWrapper) {
    return page
      .findAll("dialog")
      .find((d) => d.classes().includes("capture-confirm-dialog"));
  }

  beforeEach(() => {
    localStorage.clear();
    vi.mocked(enableRc003Capture).mockClear();
    vi.mocked(disableRc003Capture).mockClear();
  });

  it("首次开启：先弹确认；取消则不开启；确认后开启并记住，之后不再弹", async () => {
    const page = await mountPage("rc003");
    const checkbox = await openCaptureToggle(page);

    // 浏览器先把 DOM 翻成勾选，再派发 change（与既有用例同一保真写法）。
    (checkbox.element as HTMLInputElement).checked = true;
    await checkbox.trigger("change");
    await flushPromises();

    // 弹窗出现，且此刻绝不能已经调过 enable。
    const dialog = confirmDialog(page);
    expect(dialog).toBeDefined();
    expect(dialog!.text()).toContain("升级或重装无线麦后");
    expect(dialog!.text()).toContain("防作弊");
    expect(vi.mocked(enableRc003Capture)).not.toHaveBeenCalled();

    // 取消：不开启、开关保持关闭、"已确认"标记不写入。
    await dialog!.findAll("button").find((b) => b.text() === "取消")!.trigger("click");
    await flushPromises();
    expect(confirmDialog(page)).toBeUndefined();
    expect(vi.mocked(enableRc003Capture)).not.toHaveBeenCalled();
    expect((checkbox.element as HTMLInputElement).checked).toBe(false);
    expect(localStorage.getItem(CONFIRM_KEY)).toBeNull();

    // 再次点开：仍先弹（还没确认过）。
    (checkbox.element as HTMLInputElement).checked = true;
    await checkbox.trigger("change");
    await flushPromises();
    const second = confirmDialog(page);
    expect(second).toBeDefined();

    // 确认：真正开启，并写入标记。
    await second!.findAll("button").find((b) => b.text() === "开启")!.trigger("click");
    await flushPromises();
    expect(vi.mocked(enableRc003Capture)).toHaveBeenCalledTimes(1);
    expect(localStorage.getItem(CONFIRM_KEY)).toBe("1");
    await vi.waitFor(() => {
      expect((checkbox.element as HTMLInputElement).checked).toBe(true);
    });

    // 关 → 再开：不再弹，直接开启。
    vi.mocked(enableRc003Capture).mockClear();
    (checkbox.element as HTMLInputElement).checked = false;
    await checkbox.trigger("change");
    await flushPromises();
    (checkbox.element as HTMLInputElement).checked = true;
    await checkbox.trigger("change");
    await flushPromises();
    expect(confirmDialog(page)).toBeUndefined();
    expect(vi.mocked(enableRc003Capture)).toHaveBeenCalledTimes(1);
  });

  it("已确认过（本地有标记）：点开关直接开启，不弹窗", async () => {
    localStorage.setItem(CONFIRM_KEY, "1");
    const page = await mountPage("rc003");
    const checkbox = await openCaptureToggle(page);
    (checkbox.element as HTMLInputElement).checked = true;
    await checkbox.trigger("change");
    await flushPromises();
    expect(confirmDialog(page)).toBeUndefined();
    expect(vi.mocked(enableRc003Capture)).toHaveBeenCalledTimes(1);
    await vi.waitFor(() => {
      expect((checkbox.element as HTMLInputElement).checked).toBe(true);
    });
  });
});
