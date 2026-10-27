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

  it("不显示版本号（版本号在窗口标题）", () => {
    const wrapper = mount(Sidebar, { props: { activePage: "settings" } });

    expect(wrapper.find(".sidebar-footer").exists()).toBe(false);
    expect(wrapper.text()).not.toContain("0.5.0");
  });

  it("顶部页面顺序不变，个人中心固定在最后", () => {
    const wrapper = mount(Sidebar, {
      props: { activePage: "settings" },
    });
    // 私有构建可能追加自己的页面；这里只核对公开页面的相对顺序。
    const publicLabels = new Set(publicNavigationItems.map((item) => item.label));
    const items = wrapper
      .findAll(".nav-item")
      .filter((item) => publicLabels.has(item.text()));

    expect(items.map((item) => item.text())).toEqual([
      "按键",
      // 2026-10-25 用户指定：推荐放在连接上方。
      "组合动作",
      "推荐",
      "连接",
      "回眸",
      "权限",
      "设置",
      "个人中心",
    ]);
    // 推荐入口使用 Mac 的 sparkles 语义图标（大四角星 + 两颗小星）。
    const recommendationIcon = items[2].get("svg");
    expect(recommendationIcon.findAll("path")).toHaveLength(3);
    // 回眸入口使用文本气泡图标；紧挨权限上方。
    expect(items[4].text()).toBe("回眸");
    expect(items[4].get("svg").findAll("path")).toHaveLength(2);
    // 组合动作入口使用层叠方块图标（3 条描边）。
    expect(items[1].get("svg").findAll("path")).toHaveLength(3);
    const settingsIcon = items[6].get("svg");
    // 齿轮形状：外圈齿形 + 中央圆孔（区别于旧「关于」的 info.circle 单圆）。
    const paths = settingsIcon.findAll("path");
    expect(paths).toHaveLength(2);
    expect(paths[1].attributes("d")).toContain("a3.2 3.2");
    expect(wrapper.find(".share-nav-item").exists()).toBe(false);
    expect(wrapper.get(".profile-nav-item").attributes("aria-label")).toBe("个人中心");
  });

  it("个人中心图标使用与托盘同源的应用 logo", async () => {
    const wrapper = mount(Sidebar, {
      props: { activePage: "profile", appIcon: "standard" },
    });

    expect(wrapper.get(".profile-nav-item img").attributes("src")).toBe("/app-logo.png");
    await wrapper.setProps({ appIcon: "faceted-duck" });
    expect(wrapper.get(".profile-nav-item img").attributes("src")).toBe(
      "/app-icon-faceted-duck.png",
    );
  });

  it("已登录时个人入口只显示调用方传入的账号前缀", () => {
    const wrapper = mount(Sidebar, {
      props: { activePage: "profile", profileLabel: "abcdefghij" },
    });

    const profile = wrapper.get(".profile-nav-item");
    expect(profile.text()).toBe("abcdefghij");
    expect(profile.classes()).toContain("active");
    expect(profile.text()).not.toContain("@");
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
      "Routines",
      "Picks",
      "Connection",
      "Reflections",
      "Permissions",
      "Settings",
      "Profile",
    ]);
    const items = wrapper
      .findAll(".nav-item")
      .filter((item) => publicEnglishLabels.has(item.text()));

    expect(items.map((item) => item.text())).toEqual([
      "Buttons",
      "Routines",
      "Picks",
      "Connection",
      "Reflections",
      "Permissions",
      "Settings",
      "Profile",
    ]);
    expect(wrapper.get("nav").attributes("aria-label")).toBe("Page navigation");
  });
});
