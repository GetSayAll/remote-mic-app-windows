// @vitest-environment jsdom

import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import RecommendationsPage from "./RecommendationsPage.vue";

const bridge = vi.hoisted(() => ({
  hideRecommendationsPage: vi.fn(),
  isTauriRuntime: vi.fn(),
  openRecommendationsWebsite: vi.fn(),
  showRecommendationsPage: vi.fn(),
}));

const eventApi = vi.hoisted(() => ({ listen: vi.fn() }));

vi.mock("../lib/bridge", () => bridge);
vi.mock("@tauri-apps/api/event", () => eventApi);
vi.mock("../lib/theme", async () => {
  const { ref } = await import("vue");
  return { useTheme: () => ({ effectiveTheme: ref("light") }) };
});

afterEach(() => {
  vi.clearAllMocks();
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

    expect(bridge.showRecommendationsPage).toHaveBeenCalledWith("light");
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
});
