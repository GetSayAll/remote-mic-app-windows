// @vitest-environment jsdom

import { mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SHARE_WEBSITE_URL } from "../lib/bridge";
import ShareDialog from "./ShareDialog.vue";

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
  it("显示本机二维码、固定官网链接和复制入口", () => {
    stubClipboard();
    const wrapper = mount(ShareDialog);

    expect(wrapper.get("#share-dialog-title").text()).toBe("分享无线麦 SayAll");
    expect(wrapper.get(".share-qr img").attributes("src")).toBe("/app-share-qr.png");
    expect(wrapper.get(".share-copy code").text()).toBe(SHARE_WEBSITE_URL);
    expect(wrapper.get(".share-copy-button").text()).toContain("复制链接");
    wrapper.unmount();
  });

  it("复制链接使用二维码中的同一个地址并显示成功", async () => {
    const writeText = stubClipboard();
    const wrapper = mount(ShareDialog);

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
    const wrapper = mount(ShareDialog);

    await wrapper.get(".share-copy-button").trigger("click");
    await wrapper.vm.$nextTick();

    expect(wrapper.get(".share-copy-result").text()).toBe("链接复制失败");
    expect(wrapper.get(".share-copy-result").classes()).toContain("failure");
    wrapper.unmount();
  });

  it("支持 Esc 和关闭按钮退出", async () => {
    stubClipboard();
    const wrapper = mount(ShareDialog);

    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(wrapper.emitted("close")).toHaveLength(1);

    await wrapper.get(".share-close").trigger("click");
    expect(wrapper.emitted("close")).toHaveLength(2);
    wrapper.unmount();
  });
});
