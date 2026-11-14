// @vitest-environment jsdom

import { mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it } from "vitest";
import { effectiveLanguage } from "../lib/language-state";
import { publicNavigationItems } from "../navigation";
import Sidebar from "./Sidebar.vue";

describe("sidebar", () => {
  beforeEach(() => {
    effectiveLanguage.value = "zh-Hans";
  });

  it("品牌区显示应用名与版本（对齐 Mac PR 803 的紧凑侧栏）", () => {
    const wrapper = mount(Sidebar, {
      props: { activePage: "settings", appVersion: "0.5.0" },
    });

    expect(wrapper.find(".sidebar-footer").exists()).toBe(false);
    expect(wrapper.get(".sidebar-brand").text()).toContain("SayAll");
    expect(wrapper.get(".brand-version").text()).toBe("0.5.0");
    expect(wrapper.get(".sidebar-brand img").attributes("src")).toBe(
      "/app-icon-faceted-duck.webp",
    );
  });

  it("顶部页面顺序不变，账号入口固定在最后", () => {
    const wrapper = mount(Sidebar, {
      props: { activePage: "settings" },
    });
    // 私有构建可能追加自己的页面；这里只核对公开页面的相对顺序。
    const publicLabels = new Set([...publicNavigationItems.map((item) => item.label), "我的"]);
    const items = wrapper
      .findAll(".nav-item")
      .filter((item) => publicLabels.has(item.text()));

    expect(items.map((item) => item.text())).toEqual([
      "按键",
      // 2026-11-09：聚焦与翻页紧跟在按键之后（对齐 Mac）。
      "聚焦与翻页",
      // 2026-10-25 用户指定：推荐放在连接上方。
      "组合动作",
      "推荐",
      "连接",
      "回眸",
      "设置",
      "我的",
    ]);
    // APP 适配入口使用四宫格图标（对齐 Mac square.grid.2x2）。
    expect(items[1].get("svg").findAll("path")).toHaveLength(4);
    // 推荐入口使用 Mac 的 sparkles 语义图标（大四角星 + 两颗小星）。
    const recommendationIcon = items[3].get("svg");
    expect(recommendationIcon.findAll("path")).toHaveLength(3);
    // 回眸入口使用 SF text.bubble.fill 的实心气泡 + 镂空文字；紧挨设置上方。
    expect(items[5].text()).toBe("回眸");
    const bubblePaths = items[5].get("svg").findAll("path");
    expect(bubblePaths).toHaveLength(1);
    expect(bubblePaths[0].attributes("fill")).toBe("currentColor");
    expect(bubblePaths[0].attributes("fill-rule")).toBe("evenodd");
    // 组合动作入口使用层叠方块图标（3 条描边）。
    expect(items[2].get("svg").findAll("path")).toHaveLength(3);
    const settingsIcon = items[6].get("svg");
    // 齿轮形状：外圈齿形 + 中央圆孔（区别于旧「关于」的 info.circle 单圆）。
    const paths = settingsIcon.findAll("path");
    expect(paths).toHaveLength(2);
    expect(paths[1].attributes("d")).toContain("a3.2 3.2");
    expect(wrapper.find(".share-nav-item").exists()).toBe(false);
    expect(wrapper.get(".profile-nav-item").attributes("aria-label")).toBe("我的");
    // 2026-10-31 权限页并入设置：侧栏不再有「权限」入口。
    expect(wrapper.text()).not.toContain("权限");
  });

  it("导航与账号入口采用图标在左、文字在右的紧凑横排", () => {
    const wrapper = mount(Sidebar, { props: { activePage: "settings" } });
    const item = wrapper.get(".nav-item");
    const children = Array.from(item.element.children);
    expect(children[0].tagName.toLowerCase()).toBe("svg");
    expect(children[1].classList.contains("nav-label")).toBe(true);
    const profile = wrapper.get(".profile-nav-item");
    expect(profile.element.firstElementChild?.tagName.toLowerCase()).toBe("svg");
    expect(profile.find(".nav-label").exists()).toBe(true);
  });

  it("底部账号入口使用 person.crop.circle，实心头肩与 Mac 一致", () => {
    const wrapper = mount(Sidebar, {
      props: { activePage: "profile", appIcon: "standard" },
    });

    expect(wrapper.find(".profile-nav-item img").exists()).toBe(false);
    const paths = wrapper.get(".profile-nav-item svg").findAll("path");
    expect(paths).toHaveLength(3);
    expect(paths.some((path) => path.attributes("d")?.includes("M12 2.5a9.5"))).toBe(true);
    // SF person.crop.circle 的头肩是实心剪影，只有外圈是描边。
    expect(paths.filter((path) => path.attributes("fill") === "currentColor")).toHaveLength(2);
    // 顶部品牌区仍跟随应用图标设置。
    expect(wrapper.get(".sidebar-brand img").attributes("src")).toBe("/app-logo.webp");
  });

  it("已登录时账号入口完整显示调用方传入的名字并允许换行", () => {
    const wrapper = mount(Sidebar, {
      props: { activePage: "profile", profileLabel: "Christopher Anderson" },
    });

    const profile = wrapper.get(".profile-nav-item");
    expect(profile.text()).toBe("Christopher Anderson");
    expect(profile.classes()).toContain("active");
    expect(profile.find(".nav-label").classes()).toContain("nav-label");
    expect(profile.text()).not.toContain("@");
  });

  it("按键图标使用接近 Mac keyboard 的宽高比，并保留两行键位", () => {
    const wrapper = mount(Sidebar, { props: { activePage: "settings" } });
    const buttonsItem = wrapper
      .findAll(".nav-item")
      .find((item) => item.text() === "按键");

    expect(buttonsItem).toBeTruthy();
    const paths = buttonsItem!.get("svg").findAll("path");
    expect(paths).toHaveLength(5);
    expect(paths[0].attributes("d")).toContain("v12.9");
    expect(paths[1].attributes("d")).toContain("7.7");
    expect(paths[3].attributes("d")).toContain("13.9");
    expect(paths[4].attributes("d")).toBe("M6.6 17.2h10.8");
  });

  it("组合动作页用层叠方块图标（对齐 Mac square.stack.3d.up）", () => {
    const wrapper = mount(Sidebar, { props: { activePage: "macros" } });
    const macrosItem = wrapper
      .findAll(".nav-item")
      .find((item) => item.text() === "组合动作");

    expect(macrosItem).toBeTruthy();
    const paths = macrosItem!.get("svg").findAll("path");
    // 三层描边：顶层方块 + 两条底边（与键盘图标的两段明显不同）。
    expect(paths).toHaveLength(3);
    expect(paths[0].attributes("d")).toContain("l9 4.6");
  });

  it("英文模式下侧栏条目、导航标签切换为英文", () => {
    effectiveLanguage.value = "en";
    const wrapper = mount(Sidebar, { props: { activePage: "settings" } });
    const publicEnglishLabels = new Set([
      "Buttons",
      "Focus/Scroll",
      "Routines",
      "Picks",
      "Connection",
      "Reflections",
      "Settings",
      "Me",
    ]);
    const items = wrapper
      .findAll(".nav-item")
      .filter((item) => publicEnglishLabels.has(item.text()));

    expect(items.map((item) => item.text())).toEqual([
      "Buttons",
      "Focus/Scroll",
      "Routines",
      "Picks",
      "Connection",
      "Reflections",
      "Settings",
      "Me",
    ]);
    expect(wrapper.get("nav").attributes("aria-label")).toBe("Page navigation");
  });
});
