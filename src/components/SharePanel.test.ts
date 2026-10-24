// @vitest-environment jsdom

import { mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SHARE_WEBSITE_URL } from "../lib/bridge";
import SharePanel from "./SharePanel.vue";

function stubClipboard(): ReturnType<typeof vi.fn> {
  const writeText = vi.fn<(text: string) => Promise<void>>();
  writeText.mockResolvedValue();
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  });
  return writeText;
}

afterEach(() => {
  vi.restoreAllMocks();
  delete (navigator as { clipboard?: unknown }).clipboard;
});

describe("分享无线麦面板", () => {
  it("默认收起，点击后才在原页面展开二维码", async () => {
    stubClipboard();
    const wrapper = mount(SharePanel);

    expect(wrapper.get("#profile-share-title").text()).toBe("分享无线麦 SayAll");
    expect(wrapper.get(".share-toggle").text()).toContain("显示二维码");
    expect(wrapper.find(".share-qr").exists()).toBe(false);

    await wrapper.get(".share-toggle").trigger("click");

    expect(wrapper.get(".share-toggle").attributes("aria-expanded")).toBe("true");
    expect(wrapper.get(".share-toggle").text()).toContain("收起");
    expect(wrapper.get(".share-qr img").attributes("src")).toBe("/app-share-qr.png");
    expect(wrapper.get(".share-copy code").text()).toBe(SHARE_WEBSITE_URL);
    wrapper.unmount();
  });

  it("复制链接使用二维码中的同一个地址并显示成功", async () => {
    const writeText = stubClipboard();
    const wrapper = mount(SharePanel);
    await wrapper.get(".share-toggle").trigger("click");

    await wrapper.get(".share-copy-button").trigger("click");
    await wrapper.vm.$nextTick();

    expect(writeText).toHaveBeenCalledWith("https://sayall.app/?from=win_share");
    expect(wrapper.get(".share-copy-result").text()).toBe("链接已复制");
    expect(wrapper.get(".share-copy-result").classes()).toContain("success");
    wrapper.unmount();
  });

  it("剪贴板失败时显示明确失败，不假装复制成功", async () => {
    const writeText = stubClipboard();
    writeText.mockRejectedValueOnce(new Error("denied"));
    const wrapper = mount(SharePanel);
    await wrapper.get(".share-toggle").trigger("click");

    await wrapper.get(".share-copy-button").trigger("click");
    await wrapper.vm.$nextTick();

    expect(wrapper.get(".share-copy-result").text()).toBe("链接复制失败");
    expect(wrapper.get(".share-copy-result").classes()).toContain("failure");
    wrapper.unmount();
  });

  it("再次点击收起后二维码从原页面移除", async () => {
    stubClipboard();
    const wrapper = mount(SharePanel);

    await wrapper.get(".share-toggle").trigger("click");
    expect(wrapper.find(".share-panel-content").exists()).toBe(true);
    await wrapper.get(".share-toggle").trigger("click");

    expect(wrapper.get(".share-toggle").attributes("aria-expanded")).toBe("false");
    expect(wrapper.find(".share-panel-content").exists()).toBe(false);
    wrapper.unmount();
  });
});
