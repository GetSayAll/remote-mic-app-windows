// @vitest-environment jsdom

import { mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { effectiveLanguage } from "../lib/language-state";
import WindowTitleBar from "./WindowTitleBar.vue";

const bridge = vi.hoisted(() => ({
  hideMainWindowFromTitleBar: vi.fn<() => Promise<void>>(),
  minimizeMainWindow: vi.fn<() => Promise<void>>(),
  toggleMaximizeMainWindow: vi.fn<() => Promise<void>>(),
}));

vi.mock("../lib/bridge", () => bridge);

describe("window title bar", () => {
  beforeEach(() => {
    effectiveLanguage.value = "zh-Hans";
    bridge.hideMainWindowFromTitleBar.mockReset().mockResolvedValue();
    bridge.minimizeMainWindow.mockReset().mockResolvedValue();
    bridge.toggleMaximizeMainWindow.mockReset().mockResolvedValue();
  });

  it("不显示 logo 或文字，只保留拖拽区和三枚窗口按钮", () => {
    const wrapper = mount(WindowTitleBar);

    expect(wrapper.text()).toBe("");
    expect(wrapper.find(".window-titlebar-drag").attributes()).toHaveProperty(
      "data-tauri-drag-region",
    );
    const buttons = wrapper.findAll<HTMLButtonElement>(".window-control");
    expect(buttons.map((button) => button.attributes("aria-label"))).toEqual([
      "最小化",
      "最大化/还原",
      "关闭窗口",
    ]);
  });

  it("窗口按钮分别调用最小化、最大化/还原与收进托盘", async () => {
    const wrapper = mount(WindowTitleBar);
    const buttons = wrapper.findAll<HTMLButtonElement>(".window-control");

    await buttons[0].trigger("click");
    await buttons[1].trigger("click");
    await buttons[2].trigger("click");

    expect(bridge.minimizeMainWindow).toHaveBeenCalledTimes(1);
    expect(bridge.toggleMaximizeMainWindow).toHaveBeenCalledTimes(1);
    expect(bridge.hideMainWindowFromTitleBar).toHaveBeenCalledTimes(1);
  });

  it("双击拖拽区切换最大化/还原", async () => {
    const wrapper = mount(WindowTitleBar);
    await wrapper.get(".window-titlebar-drag").trigger("dblclick");
    expect(bridge.toggleMaximizeMainWindow).toHaveBeenCalledTimes(1);
  });

  it("英文模式下窗口按钮的无障碍名称同步切换", () => {
    effectiveLanguage.value = "en";
    const wrapper = mount(WindowTitleBar);
    expect(
      wrapper.findAll(".window-control").map((button) => button.attributes("aria-label")),
    ).toEqual(["Minimize", "Maximize / Restore", "Close Window"]);
  });
});
