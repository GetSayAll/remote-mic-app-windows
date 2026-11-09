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
  learnFocusTarget,
  listPresetApps,
  saveButtonMappings,
  scanRegisteredApps,
  startShortcutCapture,
  stopShortcutCapture,
  subscribeShortcutCaptureEdges,
  testAppFocus,
  type ShortcutCaptureEdge,
} from "../lib/bridge";

const baseMappings = {
  enabled: true,
  actions: {},
  applications: [] as Array<{ name: string; path: string }>,
  focusProfiles: {} as Record<string, unknown>,
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
    })),
    saveButtonMappings: vi.fn(async (mappings: unknown) => mappings),
    getApplicationIcon: vi.fn(async (path: string) =>
      path.toLowerCase().endsWith("cursor.exe") ? "data:image/png;base64,AAAA" : null,
    ),
    listPresetApps: vi.fn(async () => [
      { id: "notepad", name: "记事本", installed: true, path: "C:\\Windows\\System32\\notepad.exe" },
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
    testAppFocus: vi.fn(async () => undefined),
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
    appsViewSession.selectedTarget = null;
    configureMappings({});
  });

  it("shows the empty state and opens the existing app picker", async () => {
    const wrapper = await mountPage();
    expect(wrapper.text()).toContain("还没有添加任何 APP");
    expect(wrapper.find(".list-panel").exists()).toBe(false);

    await wrapper.findAll("button").find((button) => button.text() === "添加 APP")!.trigger("click");
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
    await wrapper.findAll("button").find((button) => button.text() === "添加 APP")!.trigger("click");
    await flushPromises();

    await wrapper.get(".picker-app input").setValue(true);
    const add = wrapper.findAll("button").find((button) => button.text().startsWith("添加所选"))!;
    await add.trigger("click");
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

  it("lists added apps first and configured presets at their real paths", async () => {
    configureMappings({
      applications: [
        { name: "Cursor", path: "C:\\Apps\\Cursor.exe" },
        { name: "微信", path: "C:\\Apps\\Weixin.exe" },
      ],
      focusProfiles: {
        netease_music: { strategy: "open_only" },
      },
    });
    const wrapper = await mountPage();

    const rows = wrapper.findAll(".app-row");
    expect(rows.map((row) => row.find(".app-row-name").text())).toEqual([
      "Cursor",
      "微信",
      "网易云音乐",
    ]);
    expect(rows[0].find(".app-row-status").text()).toBe("只打开 · 需学习");
    expect(rows[2].text()).not.toContain("内置");
    expect(wrapper.find(".detail-panel").text()).toContain("Cursor");
    await rows[2].trigger("click");
    await flushPromises();
    expect(wrapper.find(".app-path").exists()).toBe(false);
    expect(wrapper.find(".detail-panel").text()).not.toContain(
      "C:\\Program Files\\Netease\\CloudMusic\\cloudmusic.exe",
    );
    expect(wrapper.find(".detail-panel").text()).toContain("只打开 APP");
  });

  it("selects a preset by its real path and keeps the explicit focus profile", async () => {
    configureMappings({
      focusProfiles: { chrome: { strategy: "open_only" } },
    });
    const wrapper = await mountPage();
    const chromeRow = wrapper.findAll(".app-row").find((row) => row.text().includes("Chrome"));
    await chromeRow!.trigger("click");
    await flushPromises();

    const detail = wrapper.find(".detail-panel").text();
    expect(detail).not.toContain("C:\\Program Files\\Chrome\\chrome.exe");
    expect(detail).not.toContain("内置预设");
    expect(detail).toContain("只打开 APP");
    // 显式档案覆盖内置快捷键默认值。
    expect(wrapper.find(".focus-editor").text()).toContain("只打开");
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

  it("hides the removed auto and click options but keeps legacy profiles visible", async () => {
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
    ]);
    expect(wrapper.find(".detail-panel").text()).toContain("点击位置");
    expect(wrapper.find(".focus-editor").text()).toContain(
      "当前为旧版聚焦方式：记录点击位置",
    );
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
    expect(wrapper.findAll("button").some((button) => button.text() === "删除这个 APP 的适配")).toBe(false);
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
    expect(wrapper.text()).not.toContain("只删除 SayAll 保存的适配");
    await wrapper.findAll("button").find((button) => button.text() === "删除这个 APP 的适配")!.trigger("click");
    await flushPromises();
    expect(wrapper.find(".confirm-dialog").exists()).toBe(true);

    await wrapper.findAll("button").find((button) => button.text() === "删除适配")!.trigger("click");
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
    expect(wrapper.text()).toContain("已删除这个 APP 的适配");
  });

  it("shows an installed preset at its real path and deletes only its profile", async () => {
    configureMappings({
      focusProfiles: { chrome: { strategy: "open_only" } },
    });
    const wrapper = await mountPage();
    expect(wrapper.find(".detail-panel").text()).not.toContain(
      "C:\\Program Files\\Chrome\\chrome.exe",
    );
    expect(wrapper.find(".detail-panel").text()).not.toContain("内置预设");
    await wrapper.findAll("button").find((button) => button.text() === "删除这个 APP 的适配")!.trigger("click");
    await flushPromises();
    await wrapper.findAll("button").find((button) => button.text() === "删除适配")!.trigger("click");
    await flushPromises();

    const saved = vi.mocked(saveButtonMappings).mock.calls.at(-1)?.[0] as {
      applications?: unknown;
      focusProfiles?: Record<string, unknown>;
    };
    expect(saved.focusProfiles ?? {}).toEqual({});
    expect(saved.applications).toEqual([]);
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

  it("can expose a preset target passed from the buttons page", async () => {
    const wrapper = await mountPage({ initialTarget: "notepad" });
    expect(wrapper.find(".detail-panel").text()).toContain("记事本");
    expect(wrapper.find(".detail-panel").text()).toContain("自动选择输入框");
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
      .findAll("button")
      .find((button) => button.text() === "改用 APP 快捷键聚焦")!
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
      .findAll("button")
      .find((button) => button.text() === "改用 APP 快捷键聚焦")!
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
    expect(wrapper.text()).not.toContain("无法读取 APP 适配设置");
  });
});
