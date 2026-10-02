// @vitest-environment jsdom

import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
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

  it("末位页是「设置」并带齿轮图标（不再是「关于」）", () => {
    const wrapper = mount(Sidebar, { props: { activePage: "settings", version: "0.5.0" } });
    const items = wrapper.findAll(".nav-item");

    expect(items.map((item) => item.text())).toEqual(["按键", "连接", "权限", "设置"]);
    const lastIcon = items[3].get("svg");
    // 齿轮形状：外圈齿形 + 中央圆孔（区别于旧「关于」的 info.circle 单圆）。
    const paths = lastIcon.findAll("path");
    expect(paths).toHaveLength(2);
    expect(paths[1].attributes("d")).toContain("a3.2 3.2");
  });
});
