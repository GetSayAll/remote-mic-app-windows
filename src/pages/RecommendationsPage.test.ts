// @vitest-environment jsdom

import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import RecommendationsPage from "./RecommendationsPage.vue";

const bridge = vi.hoisted(() => ({
  goBackRecommendationsPage: vi.fn(),
  hideRecommendationsPage: vi.fn(),
  isTauriRuntime: vi.fn(),
  openRecommendationsWebsite: vi.fn(),
  retryRecommendationsPage: vi.fn(),
  showRecommendationsPage: vi.fn(),
}));

const eventApi = vi.hoisted(() => ({ listen: vi.fn() }));
const diagnostics = vi.hoisted(() => ({ reportFrontendEvent: vi.fn() }));
const themeBridge = vi.hoisted(() => ({
  effective: "light" as "light" | "dark",
  initialize: undefined as undefined | (() => Promise<void>),
}));
const languageBridge = vi.hoisted(() => ({
  effective: "zh-Hans" as "zh-Hans" | "en",
  initialize: undefined as undefined | (() => Promise<void>),
}));

vi.mock("../lib/bridge", () => bridge);
vi.mock("@tauri-apps/api/event", () => eventApi);
vi.mock("../lib/frontend-diagnostics", () => diagnostics);
vi.mock("../lib/theme", async () => {
  const { ref } = await import("vue");
  return {
    useTheme: () => ({ effectiveTheme: ref(themeBridge.effective) }),
    initializeTheme: () =>
      themeBridge.initialize ? themeBridge.initialize() : Promise.resolve(),
  };
});
vi.mock("../lib/language", async () => {
  const { ref } = await import("vue");
  return {
    useLanguage: () => ({ effectiveLanguage: ref(languageBridge.effective) }),
    initializeLanguage: () =>
      languageBridge.initialize ? languageBridge.initialize() : Promise.resolve(),
  };
});

afterEach(() => {
  vi.clearAllMocks();
  themeBridge.effective = "light";
  themeBridge.initialize = undefined;
  languageBridge.effective = "zh-Hans";
  languageBridge.initialize = undefined;
});

describe("推荐页状态壳", () => {
  it("浏览器预览不伪造推荐内容，给出官网入口", async () => {
    bridge.isTauriRuntime.mockReturnValue(false);
    bridge.openRecommendationsWebsite.mockResolvedValue(undefined);

    const wrapper = mount(RecommendationsPage);
    await flushPromises();

    expect(wrapper.text()).toContain("浏览器预览中无法内嵌官网推荐页面");
    expect(bridge.showRecommendationsPage).not.toHaveBeenCalled();

    await wrapper.get(".primary-button").trigger("click");
    expect(bridge.openRecommendationsWebsite).toHaveBeenCalledTimes(1);
    wrapper.unmount();
  });

  it("只接受当前 generation 的失败事件，离开页面时隐藏子 WebView", async () => {
    type Handler = (event: {
      payload: { generation: number; status: "ready" | "failed" };
    }) => void;
    let handler: Handler | undefined;
    eventApi.listen.mockImplementation(async (_name: string, callback: Handler) => {
      handler = callback;
      return () => undefined;
    });
    bridge.isTauriRuntime.mockReturnValue(true);
    bridge.showRecommendationsPage.mockResolvedValue(3);
    bridge.hideRecommendationsPage.mockResolvedValue(undefined);

    const wrapper = mount(RecommendationsPage);
    await flushPromises();

    expect(bridge.showRecommendationsPage).toHaveBeenCalledWith("light", "zh-Hans");
    expect(wrapper.text()).toContain("正在加载网页…");

    handler?.({ payload: { generation: 2, status: "failed" } });
    await flushPromises();
    expect(wrapper.text()).toContain("正在加载网页…");

    handler?.({ payload: { generation: 3, status: "failed" } });
    await flushPromises();
    expect(wrapper.text()).toContain("暂时无法加载此页面");

    wrapper.unmount();
    expect(bridge.hideRecommendationsPage).toHaveBeenCalledTimes(1);
  });

  it("加载超时显示失败页，迟到成功事件被忽略，重试会重新加载", async () => {
    vi.useFakeTimers();
    try {
      type Handler = (event: {
        payload: { generation: number; status: "ready" | "failed" };
      }) => void;
      let handler: Handler | undefined;
      eventApi.listen.mockImplementation(async (_name: string, callback: Handler) => {
        handler = callback;
        return () => undefined;
      });
      bridge.isTauriRuntime.mockReturnValue(true);
      bridge.showRecommendationsPage.mockResolvedValue(1);
      bridge.hideRecommendationsPage.mockResolvedValue(undefined);

      const wrapper = mount(RecommendationsPage);
      await vi.advanceTimersByTimeAsync(0);
      expect(wrapper.text()).toContain("正在加载网页…");

      // 20s 内没有 ready/failed 事件：必须落到失败页，而不是一直转圈。
      await vi.advanceTimersByTimeAsync(20_000);
      expect(wrapper.text()).toContain("暂时无法加载此页面");
      expect(wrapper.text()).toContain("重试");
      expect(bridge.hideRecommendationsPage).toHaveBeenCalledTimes(1);
      expect(diagnostics.reportFrontendEvent).toHaveBeenCalledWith(
        expect.objectContaining({
          event: "recommendations_page",
          phase: "load",
          result: "failed",
          reason: "page_load_timeout",
        }),
      );

      // 超时后迟到的成功事件属于已放弃的那一代，不得把界面切回加载/成功态。
      handler?.({ payload: { generation: 1, status: "ready" } });
      await vi.advanceTimersByTimeAsync(0);
      expect(wrapper.text()).toContain("暂时无法加载此页面");

      const retry = wrapper
        .findAll("button")
        .find((button) => button.text() === "重试");
      expect(retry).toBeDefined();
      bridge.retryRecommendationsPage.mockResolvedValue(2);
      await retry!.trigger("click");
      await vi.advanceTimersByTimeAsync(0);
      expect(bridge.retryRecommendationsPage).toHaveBeenCalledTimes(1);
      expect(bridge.showRecommendationsPage).toHaveBeenCalledTimes(1);
      expect(wrapper.text()).toContain("正在加载网页…");

      wrapper.unmount();
    } finally {
      vi.useRealTimers();
    }
  });

  it("显示命令失败也与 Mac 使用同一句固定文案，错误只进诊断日志", async () => {
    eventApi.listen.mockResolvedValue(() => undefined);
    bridge.isTauriRuntime.mockReturnValue(true);
    bridge.showRecommendationsPage.mockRejectedValue(
      new Error("无法读取主窗口尺寸 /tmp/secret"),
    );
    bridge.hideRecommendationsPage.mockResolvedValue(undefined);

    const wrapper = mount(RecommendationsPage);
    await flushPromises();

    expect(wrapper.text()).toContain(
      "暂时无法加载此页面，你可以前往官网查看，或稍后重试。",
    );
    expect(wrapper.text()).not.toContain("无法读取主窗口尺寸");
    expect(wrapper.text()).not.toContain("/tmp/secret");
    expect(diagnostics.reportFrontendEvent).toHaveBeenCalledWith(
      expect.objectContaining({
        event: "recommendations_page",
        phase: "show",
        result: "failed",
        reason: "show_command_failed",
      }),
    );

    wrapper.unmount();
  });

  it("ready 且 canGoBack 时显示顶部返回条，点击调用原生后退", async () => {
    type Handler = (event: {
      payload: {
        generation: number;
        status: "ready" | "failed";
        canGoBack?: boolean;
      };
    }) => void;
    let handler: Handler | undefined;
    eventApi.listen.mockImplementation(async (_name: string, callback: Handler) => {
      handler = callback;
      return () => undefined;
    });
    bridge.isTauriRuntime.mockReturnValue(true);
    bridge.showRecommendationsPage.mockResolvedValue(5);
    bridge.goBackRecommendationsPage.mockResolvedValue(undefined);
    bridge.hideRecommendationsPage.mockResolvedValue(undefined);

    const wrapper = mount(RecommendationsPage);
    await flushPromises();

    handler?.({ payload: { generation: 5, status: "ready", canGoBack: true } });
    await flushPromises();
    expect(wrapper.text()).toContain("返回上一页");

    const back = wrapper
      .findAll("button")
      .find((button) => button.text() === "返回上一页");
    expect(back).toBeDefined();
    await back!.trigger("click");
    await flushPromises();
    expect(bridge.goBackRecommendationsPage).toHaveBeenCalledTimes(1);

    handler?.({ payload: { generation: 5, status: "ready", canGoBack: false } });
    await flushPromises();
    expect(wrapper.text()).not.toContain("返回上一页");

    wrapper.unmount();
  });

  it("等待外观初始化后再首次加载：深色不会先加载一版浅色", async () => {
    eventApi.listen.mockResolvedValue(() => undefined);
    bridge.isTauriRuntime.mockReturnValue(true);
    bridge.showRecommendationsPage.mockResolvedValue(9);
    bridge.hideRecommendationsPage.mockResolvedValue(undefined);
    themeBridge.effective = "dark";
    let finishInitialization: (() => void) | undefined;
    themeBridge.initialize = () =>
      new Promise<void>((resolve) => {
        finishInitialization = resolve;
      });

    const wrapper = mount(RecommendationsPage);
    await flushPromises();
    expect(bridge.showRecommendationsPage).not.toHaveBeenCalled();

    finishInitialization?.();
    await flushPromises();
    expect(bridge.showRecommendationsPage).toHaveBeenCalledTimes(1);
    expect(bridge.showRecommendationsPage).toHaveBeenCalledWith("dark", "zh-Hans");

    wrapper.unmount();
  });

  it("英文语言使用英文固定文案与语言参数", async () => {
    type Handler = (event: {
      payload: { generation: number; status: "ready" | "failed" };
    }) => void;
    let handler: Handler | undefined;
    eventApi.listen.mockImplementation(async (_name: string, callback: Handler) => {
      handler = callback;
      return () => undefined;
    });
    bridge.isTauriRuntime.mockReturnValue(true);
    bridge.showRecommendationsPage.mockResolvedValue(11);
    bridge.hideRecommendationsPage.mockResolvedValue(undefined);
    languageBridge.effective = "en";

    const wrapper = mount(RecommendationsPage);
    await flushPromises();

    expect(bridge.showRecommendationsPage).toHaveBeenCalledWith("light", "en");
    expect(wrapper.get(".recommendations-page").attributes("aria-label")).toBe(
      "Picks",
    );
    expect(wrapper.text()).toContain("Loading page…");

    handler?.({ payload: { generation: 11, status: "failed" } });
    await flushPromises();
    expect(wrapper.text()).toContain(
      "This page is temporarily unavailable. Open the website or try again later.",
    );
    expect(wrapper.text()).toContain("Open website");
    expect(wrapper.text()).toContain("Retry");

    wrapper.unmount();
  });
});
