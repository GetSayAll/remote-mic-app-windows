import { beforeEach, describe, expect, it, vi } from "vitest";
import { mount, type VueWrapper } from "@vue/test-utils";
import ButtonsPage from "./ButtonsPage.vue";

type EdgeHandler = (edge: { button: string; isPressed: boolean }) => void;
type GestureHandler = (gesture: { button: string; trigger: string }) => void;

let edgeHandler: EdgeHandler | null = null;
let gestureHandler: GestureHandler | null = null;

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
  };
});

import { saveButtonMappings } from "../lib/bridge";
import type { RuntimeSnapshot } from "../lib/bridge";

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

async function mountPage(): Promise<VueWrapper> {
  const wrapper = mount(ButtonsPage, { props: { runtime } });
  await vi.waitFor(() => {
    if (!edgeHandler || !gestureHandler) throw new Error("事件订阅未完成");
  });
  return wrapper;
}

beforeEach(() => {
  edgeHandler = null;
  gestureHandler = null;
  vi.mocked(saveButtonMappings).mockClear();
});

describe("buttons mapping page", () => {
  it("renders the remote canvas with 12 button cards, the voice card and 36 trigger cells", async () => {
    const wrapper = await mountPage();
    expect(wrapper.findAll(".mapping-card")).toHaveLength(13);
    expect(wrapper.findAll(".mapping-cell")).toHaveLength(36);
    const voiceCard = wrapper.find(".voice-card");
    expect(voiceCard.text()).toContain("语音键");
    expect(voiceCard.text()).toContain("按住说话");
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

  it("applies a preset to the editing target and persists on save", async () => {
    const wrapper = await mountPage();
    const powerCard = wrapper
      .findAll(".mapping-card")
      .find((card) => card.text().includes("电源"));
    await powerCard!.findAll(".mapping-cell")[2]!.trigger("click");

    const editor = wrapper.find(".mapping-editor");
    expect(editor.text()).toContain("电源 · 长按");
    const chips = editor.findAll(".chip");
    const escapeChip = chips.find((chip) => chip.text() === "Esc");
    await escapeChip!.trigger("click");

    const saveButton = wrapper
      .findAll("button")
      .find((button) => button.text().includes("保存映射"));
    await saveButton!.trigger("click");
    await vi.waitFor(() => {
      if (vi.mocked(saveButtonMappings).mock.calls.length === 0) {
        throw new Error("保存未触发");
      }
    });
    const saved = vi.mocked(saveButtonMappings).mock.calls[0]![0] as {
      actions: Record<string, { long: { type: string; chord?: { keys: string[] } } }>;
    };
    expect(saved.actions.power!.long.type).toBe("shortcut");
    expect(saved.actions.power!.long.chord!.keys).toEqual(["escape"]);
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
      if (!wrapper.text().includes("触发 1 次")) {
        throw new Error("页脚统计未更新");
      }
    });
  });
});
