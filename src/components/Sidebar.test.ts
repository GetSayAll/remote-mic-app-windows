// @vitest-environment jsdom

import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import { publicNavigationItems } from "../navigation";
import Sidebar from "./Sidebar.vue";

describe("sidebar", () => {
  it("底部显示运行快照的版本号，不再显示“预览版”", () => {
    const wrapper = mount(Sidebar, { props: { activePage: "settings", version: "0.5.0" } });

    const footer = wrapper.get(".sidebar-footer");
    expect(footer.text()).toBe("0.5.0");
    expect(wrapper.text()).not.toContain("预览版");
    // 状态圆点随“预览版”一起退场：底部只留版本号。
    expect(wrapper.find(".status-dot").exists()).toBe(false);
  });

  it("版本还没读到时底部留空，不编造版本号", () => {
    const wrapper = mount(Sidebar, { props: { activePage: "buttons", version: null } });

    expect(wrapper.get(".sidebar-footer").text()).toBe("");
  });

  it("按键之后是「组合动作」，推荐在连接之后，末位页是「设置」并带齿轮图标", () => {
    const wrapper = mount(Sidebar, { props: { activePage: "settings", version: "0.5.0" } });
    const publicLabels = new Set(publicNavigationItems.map((item) => item.label));
    const items = wrapper
      .findAll(".nav-item")
      .filter((item) => publicLabels.has(item.text()));

    expect(items.map((item) => item.text())).toEqual([
      "按键",
      // 2026-10-20 用户指定：按键、组合动作、连接、推荐、权限、设置。
      "组合动作",
      "连接",
      "推荐",
      "权限",
      "设置",
    ]);
    // 推荐入口使用 Mac 的 sparkles 语义图标（大四角星 + 两颗小星）。
    const recommendationIcon = items[3].get("svg");
    expect(recommendationIcon.findAll("path")).toHaveLength(3);
    // 组合动作入口使用层叠方块图标（3 条描边）。
    expect(items[1].get("svg").findAll("path")).toHaveLength(3);
    const lastIcon = items[5].get("svg");
    // 齿轮形状：外圈齿形 + 中央圆孔（区别于旧「关于」的 info.circle 单圆）。
    const paths = lastIcon.findAll("path");
    expect(paths).toHaveLength(2);
    expect(paths[1].attributes("d")).toContain("a3.2 3.2");
  });

  it("组合动作页用层叠方块图标（对齐 Mac square.stack.3d.up）", () => {
    const wrapper = mount(Sidebar, { props: { activePage: "macros", version: "0.5.0" } });
    const items = wrapper.findAll(".nav-item");

    const macrosItem = items[1];
    expect(macrosItem.text()).toBe("组合动作");
    const paths = macrosItem.get("svg").findAll("path");
    // 三层描边：顶层方块 + 两条底边（与键盘图标的两段明显不同）。
    expect(paths).toHaveLength(3);
    expect(paths[0].attributes("d")).toContain("l9 4.6");
  });
});
