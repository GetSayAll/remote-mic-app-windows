// @vitest-environment jsdom

import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import AppsPage from "./AppsPage.vue";
import { appsViewSession } from "./apps-view-session";
import {
  activateAppForLearning,
  getApplicationIcon,
  getButtonMappings,
  getCombinedActions,
  learnFocusPoint,
  learnFocusTarget,
  learnScrollRegion,
  listPresetApps,
  resolvePresetTarget,
  saveButtonMappings,
  scanRegisteredApps,
  startShortcutCapture,
  stopShortcutCapture,
  subscribeShortcutCaptureEdges,
  testAppFocus,
  testScrollRegion,
  type ShortcutCaptureEdge,
} from "../lib/bridge";

const baseMappings = {
  enabled: true,
  actions: {},
  applications: [] as Array<{ name: string; path: string }>,
  focusProfiles: {} as Record<string, unknown>,
  scrollRegions: {} as Record<string, unknown>,
};

vi.mock("../lib/bridge", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/bridge")>();
  return {
    ...actual,
    getButtonMappings: vi.fn(async () => ({
      enabled: true,
      actions: {},
      applications: [],
      focusProfiles: {},
      scrollRegions: {},
    })),
    saveButtonMappings: vi.fn(async (mappings: unknown) => mappings),
    getApplicationIcon: vi.fn(async (path: string) =>
      path.toLowerCase().endsWith("cursor.exe") ? "data:image/png;base64,AAAA" : null,
    ),
    resolvePresetTarget: vi.fn(async () => null),
    listPresetApps: vi.fn(async () => [
      { id: "notepad", name: "记事本", installed: true, path: "C:\\Windows\\System32\\notepad.exe" },
      {
        id: "wechat",
        name: "微信",
        installed: true,
        path: "C:\\Program Files\\Tencent\\WeChat\\Weixin.exe",
      },
      { id: "chrome", name: "Chrome 浏览器", installed: true, path: "C:\\Program Files\\Chrome\\chrome.exe" },
      {
        id: "netease_music",
        name: "网易云音乐",
        installed: true,
        path: "C:\\Program Files\\Netease\\CloudMusic\\cloudmusic.exe",
      },
    ]),
    getCombinedActions: vi.fn(async () => ({ formatVersion: 1, definitions: [] })),
    activateAppForLearning: vi.fn(async () => undefined),
    learnFocusTarget: vi.fn(async () => ({
      controlType: "Edit",
      automationId: "chat-input",
      className: "RichEdit",
    })),
    learnFocusPoint: vi.fn(async () => ({
      xRatio: 0.5,
      yRatio: 0.82,
      windowWidth: 1200,
      windowHeight: 800,
    })),
    testAppFocus: vi.fn(async () => undefined),
    learnScrollRegion: vi.fn(async () => ({
      controlType: "Document",
      automationId: "RootWebArea",
      className: "",
      normalizedRect: { x: 0, y: 0.1, width: 1, height: 0.8 },
    })),
    testScrollRegion: vi.fn(async () => ({
      method: "scroll_pattern",
      beforePercent: 10,
      afterPercent: 90,
      moved: true,
      atEdge: false,
    })),
    scanRegisteredApps: vi.fn(async () => []),
    startShortcutCapture: vi.fn(async () => []),
    stopShortcutCapture: vi.fn(async () => null),
    subscribeShortcutCaptureEdges: vi.fn(async () => () => {}),
  };
});

function configureMappings(overrides: Partial<typeof baseMappings>) {
  vi.mocked(getButtonMappings).mockResolvedValue({
    ...baseMappings,
    ...overrides,
  } as never);
}

async function mountPage(props: Record<string, unknown> = {}) {
  const wrapper = mount(AppsPage, {
    props: { runtime: null, ...props },
  });
  await flushPromises();
  await flushPromises();
  return wrapper;
}

describe("APP adaptation page", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(resolvePresetTarget).mockResolvedValue(null);
    appsViewSession.selectedTarget = null;
    configureMappings({});
  });

  it("shows the empty state and opens the existing app picker", async () => {
    const wrapper = await mountPage();
    expect(wrapper.text()).toContain("还没有添加任何 App");
    expect(wrapper.find(".list-panel").exists()).toBe(false);

    await wrapper.findAll("button").find((button) => button.text() === "添加 App")!.trigger("click");
    await flushPromises();
    expect(wrapper.find(".registered-apps-dialog").exists()).toBe(true);
  });

  it("adds a scanned app without changing button bindings", async () => {
    vi.mocked(scanRegisteredApps).mockResolvedValueOnce([
      { name: "Cursor", path: "C:\\Apps\\Cursor.exe" },
    ]);
    configureMappings({
      actions: {
        ok: {
          single: { type: "open_app", target: "notepad" },
          double: { type: "disabled" },
          long: { type: "disabled" },
        },
      },
    });
    const wrapper = await mountPage();
    await wrapper.findAll("button").find((button) => button.text() === "添加 App")!.trigger("click");
    await flushPromises();

    await wrapper.get('.picker-row input[aria-label="Cursor"]').setValue(true);
    await wrapper.get(".picker-actions .primary-button").trigger("click");
    await flushPromises();

    const saved = vi.mocked(saveButtonMappings).mock.calls.at(-1)?.[0] as {
      actions: Record<string, unknown>;
      applications: Array<{ name: string; path: string }>;
    };
    expect(saved.applications).toEqual([{ name: "Cursor", path: "C:\\Apps\\Cursor.exe" }]);
    expect(saved.actions).toEqual({
      ok: {
        single: { type: "open_app", target: "notepad" },
        double: { type: "disabled" },
        long: { type: "disabled" },
      },
    });
    expect(wrapper.find(".detail-panel").text()).toContain("Cursor");
  });

  it("lists only manually added apps and marks a user-added preset as built-in", async () => {
    configureMappings({
      applications: [
        { name: "Cursor", path: "C:\\Apps\\Cursor.exe" },
        { name: "微信", path: "C:\\Apps\\Weixin.exe" },
        { name: "Chrome 浏览器", path: "C:\\Program Files\\Chrome\\chrome.exe" },
      ],
    });
    const wrapper = await mountPage();

    const rows = wrapper.findAll(".app-row");
    expect(rows.map((row) => row.find(".app-row-name").text())).toEqual([
      "Cursor",
      "微信",
      "Chrome 浏览器",
    ]);
    expect(rows[0].find(".app-row-status").text()).toBe("只打开 · 需学习");
    expect(rows[2].find(".app-row-status").text()).toBe("快捷键 · 预置可用");
    expect(wrapper.find(".detail-panel").text()).toContain("Cursor");
    await rows[2].trigger("click");
    await flushPromises();
    expect(wrapper.find(".detail-panel").text()).toContain("预置可用");
    expect(wrapper.find(".detail-panel").text()).toContain("使用 APP 快捷键聚焦");
    expect(wrapper.find(".focus-editor").exists()).toBe(true);
    expect(wrapper.find(".scroll-card").exists()).toBe(true);
    expect(wrapper.find(".scroll-card .scroll-status").text()).toBe("预置可用");
  });

  it("shows an explicitly navigated preset without adding it to the library", async () => {
    const wrapper = await mountPage({ initialTarget: "notepad" });
    expect(wrapper.find(".app-row").exists()).toBe(true);
    expect(wrapper.find(".detail-panel").text()).toContain("记事本");
    expect(wrapper.text()).not.toContain("还没有添加任何 App");
    expect(saveButtonMappings).not.toHaveBeenCalled();
  });

  it("switches focus strategy and writes only the selected target's profile", async () => {
    configureMappings({
      applications: [{ name: "Cursor", path: "C:\\Apps\\Cursor.exe" }],
      focusProfiles: {
        "C:\\Other.exe": { strategy: "app_shortcut", shortcut: { keys: ["left_control", "k"] } },
      },
    });
    const wrapper = await mountPage();

    const option = wrapper
      .findAll(".focus-option")
      .find((button) => button.text().includes("打开后自动聚焦"))!;
    await option.trigger("click");
    await flushPromises();

    const saved = vi.mocked(saveButtonMappings).mock.calls.at(-1)?.[0] as {
      applications: Array<{ path: string }>;
      focusProfiles: Record<string, { strategy: string }>;
    };
    expect(saved.applications).toEqual([{ name: "Cursor", path: "C:\\Apps\\Cursor.exe" }]);
    expect(saved.focusProfiles["C:\\Apps\\Cursor.exe"]?.strategy).toBe("recorded_element");
    expect(saved.focusProfiles["C:\\Other.exe"]?.strategy).toBe("app_shortcut");
  });

  it("keeps a learned input field when switching focus strategy", async () => {
    const target = "C:\\Apps\\Cursor.exe";
    configureMappings({
      applications: [{ name: "Cursor", path: target }],
      focusProfiles: {
        [target]: {
          strategy: "recorded_element",
          recorded: {
            controlType: "Edit",
            automationId: "chat-input",
            className: "RichEdit",
          },
        },
      },
    });
    const wrapper = await mountPage();

    const option = wrapper
      .findAll(".focus-option")
      .find((button) => button.text().includes("使用 APP 快捷键聚焦"))!;
    await option.trigger("click");
    await flushPromises();

    const saved = vi.mocked(saveButtonMappings).mock.calls.at(-1)?.[0] as {
      focusProfiles: Record<
        string,
        {
          strategy: string;
          recorded?: { automationId: string };
          point?: { xRatio: number };
          shortcut?: { keys: string[] };
        }
      >;
    };
    expect(saved.focusProfiles[target]?.strategy).toBe("app_shortcut");
    expect(saved.focusProfiles[target]?.recorded?.automationId).toBe("chat-input");
  });

  it("learns an input field and saves the structural record", async () => {
    configureMappings({
      applications: [{ name: "Cursor", path: "C:\\Apps\\Cursor.exe" }],
    });
    const wrapper = await mountPage();
    await wrapper.findAll(".focus-option").find((button) => button.text().includes("打开后自动聚焦"))!.trigger("click");
    await flushPromises();

    const learn = wrapper.findAll("button").find((button) => button.text() === "开始学习输入框")!;
    await learn.trigger("click");
    await flushPromises();

    expect(activateAppForLearning).toHaveBeenCalledWith("C:\\Apps\\Cursor.exe");
    expect(learnFocusTarget).toHaveBeenCalled();
    const saved = vi.mocked(saveButtonMappings).mock.calls.at(-1)?.[0] as {
      focusProfiles: Record<string, { strategy: string; recorded?: { automationId: string } }>;
    };
    expect(saved.focusProfiles["C:\\Apps\\Cursor.exe"]?.strategy).toBe("recorded_element");
    expect(saved.focusProfiles["C:\\Apps\\Cursor.exe"]?.recorded?.automationId).toBe("chat-input");
    expect(wrapper.text()).toContain("输入框学习成功");
  });

  it("ignores a learning result that lands after the strategy changed", async () => {
    let resolveLearn!: (value: {
      controlType: string;
      automationId: string;
      className: string;
    }) => void;
    vi.mocked(learnFocusTarget).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveLearn = resolve;
        }),
    );
    configureMappings({
      applications: [{ name: "Cursor", path: "C:\\Apps\\Cursor.exe" }],
    });
    const wrapper = await mountPage();
    await wrapper
      .findAll(".focus-option")
      .find((button) => button.text().includes("打开后自动聚焦"))!
      .trigger("click");
    await flushPromises();
    await wrapper.findAll("button").find((button) => button.text() === "开始学习输入框")!.trigger("click");
    await flushPromises();

    await wrapper
      .findAll(".focus-option")
      .find((button) => button.text().includes("只打开 APP"))!
      .trigger("click");
    await flushPromises();

    resolveLearn({
      controlType: "Edit",
      automationId: "stale-input",
      className: "RichEdit",
    });
    await flushPromises();

    const saved = vi.mocked(saveButtonMappings).mock.calls.at(-1)?.[0] as {
      focusProfiles: Record<string, { strategy: string; recorded?: { automationId: string } }>;
    };
    expect(saved.focusProfiles["C:\\Apps\\Cursor.exe"]?.strategy).toBe("open_only");
    expect(saved.focusProfiles["C:\\Apps\\Cursor.exe"]?.recorded?.automationId).toBeUndefined();
  });

  it("offers click position for apps that expose no input field", async () => {
    configureMappings({
      applications: [{ name: "微信", path: "C:\\Apps\\Weixin.exe" }],
      focusProfiles: {
        "C:\\Apps\\Weixin.exe": { strategy: "recorded_point" },
      },
    });
    const wrapper = await mountPage();

    expect(wrapper.findAll(".focus-option").map((button) => button.text())).toEqual([
      "只打开 APP",
      "打开后自动聚焦",
      "使用 APP 快捷键聚焦",
      "记录点击位置",
    ]);
    expect(wrapper.find(".detail-panel").text()).toContain("点击位置");
    expect(wrapper.find(".focus-editor").text()).toContain("开始记录位置");
    expect(wrapper.find(".focus-editor").text()).not.toContain("旧版聚焦方式");
  });

  it("lets a preset without a reliable focus method learn a click position", async () => {
    vi.useFakeTimers();
    const target = "C:\\Program Files\\Tencent\\WeChat\\Weixin.exe";
    configureMappings({
      applications: [{ name: "微信", path: target }],
    });
    const wrapper = await mountPage();
    expect(wrapper.find(".app-row-status").text()).toBe("自动选择 · 预置可用");
    expect(wrapper.find(".focus-editor").exists()).toBe(true);
    expect(wrapper.findAll(".focus-option").map((button) => button.text())).toEqual([
      "自动选择输入框",
      "只打开 APP",
      "打开后自动聚焦",
      "使用 APP 快捷键聚焦",
      "记录点击位置",
    ]);
    expect(wrapper.find(".focus-option.selected").text()).toContain("自动选择输入框");
    expect(wrapper.find(".focus-status").text()).toBe("预置可用");
    expect(wrapper.find(".scroll-card .scroll-status").text()).toBe("预置可用");

    const pointOption = wrapper
      .findAll(".focus-option")
      .find((button) => button.text().includes("记录点击位置"))!;
    await pointOption.trigger("click");
    await flushPromises();

    const start = wrapper
      .findAll("button")
      .find((button) => button.text() === "开始记录位置")!;
    await start.trigger("click");
    await flushPromises();
    expect(activateAppForLearning).toHaveBeenCalledWith(target);

    vi.advanceTimersByTime(6000);
    await flushPromises();

    expect(learnFocusPoint).toHaveBeenCalledWith(target);
    const saved = vi.mocked(saveButtonMappings).mock.calls.at(-1)?.[0] as {
      focusProfiles: Record<string, { strategy: string; point?: { xRatio: number } }>;
    };
    expect(saved.focusProfiles.wechat?.strategy).toBe("recorded_point");
    expect(saved.focusProfiles.wechat?.point?.xRatio).toBe(0.5);
    expect(wrapper.find(".app-row-status").text()).toContain("点击位置");
    vi.useRealTimers();
  });

  it("shows the preset badge beside the heading only for auto selection", async () => {
    const target = "shell:AppsFolder\\WorkBuddy.WorkBuddy";
    vi.mocked(resolvePresetTarget).mockImplementation(async (value) =>
      value === target ? "workbuddy" : null,
    );
    configureMappings({
      applications: [{ name: "WorkBuddy", path: target }],
    });
    const wrapper = await mountPage();

    expect(wrapper.findAll(".focus-option").map((button) => button.text())).toContain(
      "自动选择输入框",
    );
    expect(wrapper.find(".focus-status").text()).toBe("预置可用");

    await wrapper
      .findAll(".focus-option")
      .find((button) => button.text().includes("只打开 APP"))!
      .trigger("click");
    await flushPromises();
    expect(wrapper.find(".focus-status").exists()).toBe(false);
  });

  it("lets a preset override and restore its preset scroll region", async () => {
    const target = "C:\\Program Files\\Tencent\\WeChat\\Weixin.exe";
    configureMappings({
      applications: [{ name: "微信", path: target }],
    });
    const wrapper = await mountPage();
    expect(wrapper.find(".scroll-card .scroll-status").text()).toBe("预置可用");

    await wrapper
      .findAll("button")
      .find((button) => button.text() === "学习滚动区域")!
      .trigger("click");
    await flushPromises();

    expect(activateAppForLearning).toHaveBeenCalledWith(target);
    expect(learnScrollRegion).toHaveBeenCalled();
    const learned = vi.mocked(saveButtonMappings).mock.calls.at(-1)?.[0] as {
      scrollRegions?: Record<string, { automationId?: string }>;
    };
    expect(learned.scrollRegions?.wechat?.automationId).toBe("RootWebArea");
    expect(wrapper.find(".scroll-card .scroll-status").text()).toBe("已学习");
    expect(wrapper.find(".app-row-status").text()).toContain("已学习");

    await wrapper
      .findAll("button")
      .find((button) => button.text() === "清除学习结果")!
      .trigger("click");
    await flushPromises();
    const cleared = vi.mocked(saveButtonMappings).mock.calls.at(-1)?.[0] as {
      scrollRegions?: Record<string, unknown>;
    };
    expect(cleared.scrollRegions).toBeUndefined();
    expect(wrapper.find(".scroll-card .scroll-status").text()).toBe("预置可用");
  });

  it("blocks deletion while a button still opens the app", async () => {
    configureMappings({
      applications: [{ name: "Cursor", path: "C:\\Apps\\Cursor.exe" }],
      actions: {
        ok: {
          single: { type: "open_app", target: "C:\\Apps\\Cursor.exe" },
          double: { type: "disabled" },
          long: { type: "disabled" },
        },
      },
    });
    const wrapper = await mountPage();

    expect(wrapper.text()).toContain("还有 1 处绑定在使用它");
    expect(wrapper.findAll("button").some((button) => button.text() === "删除这个 App 的设置")).toBe(false);
    expect(wrapper.find(".binding-row").text()).toContain("确定");
  });

  it("deletes an unbound custom app and its profile after confirmation", async () => {
    configureMappings({
      applications: [
        { name: "Cursor", path: "C:\\Apps\\Cursor.exe" },
        { name: "微信", path: "C:\\Apps\\Weixin.exe" },
      ],
      focusProfiles: {
        "C:\\Apps\\Cursor.exe": { strategy: "recorded_element" },
        "C:\\Apps\\Weixin.exe": { strategy: "open_only" },
      },
    });
    const wrapper = await mountPage();
    expect(wrapper.find(".app-tile.large").exists()).toBe(false);
    expect(wrapper.find(".binding-card .section-divider").exists()).toBe(true);
    expect(wrapper.find(".binding-card .danger-link").exists()).toBe(true);
    expect(wrapper.text()).not.toContain("0 处");
    expect(wrapper.text()).not.toContain("只删除 SayAll 保存的设置");
    await wrapper.findAll("button").find((button) => button.text() === "删除这个 App 的设置")!.trigger("click");
    await flushPromises();
    expect(wrapper.find(".confirm-dialog").exists()).toBe(true);

    await wrapper.findAll("button").find((button) => button.text() === "删除设置")!.trigger("click");
    await flushPromises();

    const saved = vi.mocked(saveButtonMappings).mock.calls.at(-1)?.[0] as {
      applications: Array<{ path: string }>;
      focusProfiles: Record<string, unknown>;
    };
    expect(saved.applications).toEqual([
      { name: "微信", path: "C:\\Apps\\Weixin.exe" },
    ]);
    expect(saved.focusProfiles).toEqual({
      "C:\\Apps\\Weixin.exe": { strategy: "open_only" },
    });
    expect(wrapper.find(".page-notice").exists()).toBe(false);
  });

  it("shows preset focus and scroll as normal editable sections", async () => {
    configureMappings({
      applications: [
        { name: "Chrome 浏览器", path: "C:\\Program Files\\Chrome\\chrome.exe" },
      ],
    });
    const wrapper = await mountPage();
    const detail = wrapper.find(".detail-panel").text();
    expect(detail).not.toContain("C:\\Program Files\\Chrome\\chrome.exe");
    expect(detail).not.toContain("预置能力");
    expect(detail).toContain("使用 APP 快捷键聚焦");
    expect(wrapper.find(".focus-editor").exists()).toBe(true);
    expect(wrapper.find(".focus-option.selected").text()).toContain(
      "使用 APP 快捷键聚焦",
    );
    expect(wrapper.find(".scroll-card").exists()).toBe(true);
    expect(wrapper.find(".scroll-card .scroll-status").text()).toBe("预置可用");
    expect(wrapper.find(".scroll-card").text()).toContain(
      "预置滚动区域已可用；重新学习会覆盖预置区域。",
    );
    expect(wrapper.find(".delete-row").exists()).toBe(true);
  });

  it("deletes a preset profile stored under the preset id", async () => {
    configureMappings({
      applications: [
        { name: "Chrome 浏览器", path: "C:\\Program Files\\Chrome\\chrome.exe" },
      ],
      focusProfiles: { chrome: { strategy: "open_only" } },
    });
    const wrapper = await mountPage();
    await wrapper
      .findAll("button")
      .find((button) => button.text() === "删除这个 App 的设置")!
      .trigger("click");
    await flushPromises();
    await wrapper
      .findAll("button")
      .find((button) => button.text() === "删除设置")!
      .trigger("click");
    await flushPromises();

    const saved = vi.mocked(saveButtonMappings).mock.calls.at(-1)?.[0] as {
      applications: Array<{ path: string }>;
      focusProfiles?: Record<string, unknown>;
    };
    expect(saved.applications).toEqual([]);
    expect(saved.focusProfiles).toBeUndefined();
  });

  it("loads the real application icon instead of a letter tile", async () => {
    configureMappings({
      applications: [{ name: "Cursor", path: "C:\\Apps\\Cursor.exe" }],
    });
    const wrapper = await mountPage();
    await flushPromises();

    expect(getApplicationIcon).toHaveBeenCalledWith("C:\\Apps\\Cursor.exe");
    expect(wrapper.find(".app-tile img").attributes("src")).toBe(
      "data:image/png;base64,AAAA",
    );
    expect(wrapper.find(".app-tile").text()).toBe("");
  });

  it("navigates from a binding to the matching button editor", async () => {
    configureMappings({
      applications: [{ name: "Cursor", path: "C:\\Apps\\Cursor.exe" }],
      actions: {
        ok: {
          single: { type: "open_app", target: "C:\\Apps\\Cursor.exe" },
          double: { type: "disabled" },
          long: { type: "disabled" },
        },
      },
    });
    const wrapper = await mountPage();
    await wrapper.find(".binding-row").trigger("click");
    expect(wrapper.emitted("navigate")?.at(-1)).toEqual([
      { page: "buttons", params: { button: "ok", trigger: "single" } },
    ]);
  });

  it("uses the initial target from the buttons page", async () => {
    configureMappings({
      applications: [{ name: "Cursor", path: "C:\\Apps\\Cursor.exe" }],
    });
    const wrapper = await mountPage({ initialTarget: "C:\\Apps\\Cursor.exe" });
    expect(wrapper.find(".detail-panel").text()).toContain("Cursor");
  });

  it("remembers the selected app after leaving and returning", async () => {
    configureMappings({
      applications: [
        { name: "Cursor", path: "C:\\Apps\\Cursor.exe" },
        { name: "微信", path: "C:\\Apps\\Weixin.exe" },
      ],
    });
    const first = await mountPage();
    const secondRow = first.findAll(".app-row").find((row) => row.text().includes("微信"))!;
    await secondRow.trigger("click");
    await flushPromises();
    expect(secondRow.classes()).toContain("selected");
    first.unmount();

    const second = await mountPage();
    const selected = second.findAll(".app-row").find((row) => row.classes().includes("selected"));
    expect(selected?.text()).toContain("微信");
  });

  it("captures an app focus shortcut and releases capture on unmount", async () => {
    let emitEdge: ((edge: ShortcutCaptureEdge) => void) | null = null;
    vi.mocked(subscribeShortcutCaptureEdges).mockImplementationOnce(async (callback) => {
      emitEdge = callback;
      return () => {};
    });
    configureMappings({
      applications: [{ name: "Cursor", path: "C:\\Apps\\Cursor.exe" }],
    });
    const wrapper = await mountPage();
    await wrapper
      .findAll(".focus-option")
      .find((button) => button.text().includes("使用 APP 快捷键聚焦"))!
      .trigger("click");
    await flushPromises();
    await wrapper.findAll("button").find((button) => button.text() === "录入快捷键")!.trigger("click");
    await flushPromises();
    expect(startShortcutCapture).toHaveBeenCalled();

    emitEdge!({ key: "left_control", isPressed: true });
    emitEdge!({ key: "k", isPressed: true });
    emitEdge!({ key: "k", isPressed: false });
    emitEdge!({ key: "left_control", isPressed: false });
    await flushPromises();

    const saved = vi.mocked(saveButtonMappings).mock.calls.at(-1)?.[0] as {
      focusProfiles: Record<string, { strategy: string; shortcut?: { keys: string[] } }>;
    };
    expect(saved.focusProfiles["C:\\Apps\\Cursor.exe"]?.strategy).toBe("app_shortcut");
    expect(saved.focusProfiles["C:\\Apps\\Cursor.exe"]?.shortcut?.keys).toEqual([
      "left_control",
      "k",
    ]);
    expect(wrapper.find(".modifier-grid").exists()).toBe(false);
    expect(wrapper.find(".shortcut-message").text()).toContain("聚焦快捷键已录入");
    expect(wrapper.find(".page-notice").exists()).toBe(false);

    wrapper.unmount();
    expect(stopShortcutCapture).toHaveBeenCalled();
  });

  it("keeps shortcut cancellation feedback inline instead of the page notice", async () => {
    vi.mocked(subscribeShortcutCaptureEdges).mockImplementationOnce(async () => () => {});
    configureMappings({
      applications: [{ name: "Cursor", path: "C:\\Apps\\Cursor.exe" }],
    });
    const wrapper = await mountPage();
    await wrapper
      .findAll(".focus-option")
      .find((button) => button.text().includes("使用 APP 快捷键聚焦"))!
      .trigger("click");
    await flushPromises();
    await wrapper.findAll("button").find((button) => button.text() === "录入快捷键")!.trigger("click");
    await flushPromises();

    window.dispatchEvent(new Event("blur"));
    await flushPromises();

    expect(wrapper.find(".shortcut-message").text()).toContain("窗口失去焦点，已取消录入");
    expect(wrapper.find(".page-notice").exists()).toBe(false);
    expect(wrapper.find(".modifier-grid").exists()).toBe(false);
    wrapper.unmount();
  });

  it("shows a learned scroll region and tests paging", async () => {
    const target = "C:\\Apps\\Cursor.exe";
    configureMappings({
      applications: [{ name: "Cursor", path: target }],
      scrollRegions: {
        [target]: {
          controlType: "Document",
          automationId: "RootWebArea",
          normalizedRect: { x: 0, y: 0.1, width: 1, height: 0.8 },
        },
      },
    });
    const wrapper = await mountPage();
    expect(wrapper.find(".scroll-card").text()).toContain("已学习");
    expect(wrapper.find(".app-row").text()).toContain("已学习");

    await wrapper
      .findAll("button")
      .find((button) => button.text() === "测试向上翻页")!
      .trigger("click");
    await flushPromises();

    expect(testScrollRegion).toHaveBeenCalledWith(target, "page_up");
    expect(wrapper.find(".scroll-message").text()).toContain("内容已移动，这个区域可以翻页。");
    expect(wrapper.find(".scroll-message").classes()).toContain("success");
  });

  it("learns and clears a scroll region without touching focus profiles", async () => {
    const target = "C:\\Apps\\Cursor.exe";
    configureMappings({ applications: [{ name: "Cursor", path: target }] });
    const wrapper = await mountPage();
    expect(wrapper.find(".scroll-card").text()).toContain("需学习");

    await wrapper
      .findAll("button")
      .find((button) => button.text() === "学习滚动区域")!
      .trigger("click");
    await flushPromises();

    expect(activateAppForLearning).toHaveBeenCalledWith(target);
    expect(learnScrollRegion).toHaveBeenCalled();
    const learned = vi.mocked(saveButtonMappings).mock.calls.at(-1)?.[0] as {
      scrollRegions?: Record<string, { automationId?: string }>;
    };
    expect(learned.scrollRegions?.[target]?.automationId).toBe("RootWebArea");
    expect(wrapper.find(".scroll-card").text()).toContain("已学习");

    await wrapper
      .findAll("button")
      .find((button) => button.text() === "清除学习结果")!
      .trigger("click");
    await flushPromises();
    const cleared = vi.mocked(saveButtonMappings).mock.calls.at(-1)?.[0] as {
      scrollRegions?: Record<string, unknown>;
    };
    expect(cleared.scrollRegions).toBeUndefined();
    expect(wrapper.find(".scroll-message").exists()).toBe(false);
  });

  it("sends a focus test request", async () => {
    configureMappings({
      applications: [{ name: "Cursor", path: "C:\\Apps\\Cursor.exe" }],
    });
    const wrapper = await mountPage();
    await wrapper.findAll("button").find((button) => button.text() === "测试打开与聚焦")!.trigger("click");
    await flushPromises();
    expect(testAppFocus).toHaveBeenCalledWith("C:\\Apps\\Cursor.exe");
  });

  it("does not crash when optional sources fail", async () => {
    vi.mocked(listPresetApps).mockRejectedValueOnce(new Error("scan failed"));
    vi.mocked(getCombinedActions).mockRejectedValueOnce(new Error("macros failed"));
    configureMappings({
      applications: [{ name: "Cursor", path: "C:\\Apps\\Cursor.exe" }],
    });
    const wrapper = await mountPage();
    expect(wrapper.find(".detail-panel").text()).toContain("Cursor");
    expect(wrapper.text()).not.toContain("无法读取聚焦与翻页设置");
  });
});
