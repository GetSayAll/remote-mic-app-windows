import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import type { SidePanel } from "../../onboarding/panel";
import StepSide from "./StepSide.vue";

/** 2026-11-24 用户要求：首步与完成步统一使用「几何鸭」应用图标。 */
const FACETED_DUCK = "/app-icon-faceted-duck.webp";

describe("StepSide", () => {
  it("marks the side panel empty when the step has no check card", () => {
    const panel: SidePanel = { visual: "app", caption: "无线说话，也能随手控制" };
    const wrapper = mount(StepSide, { props: { panel } });

    expect(wrapper.get(".onboarding-side").classes()).toContain("is-empty");
    expect(wrapper.find(".onboarding-check-card").exists()).toBe(false);
  });

  it("uses the faceted-duck icon on the welcome step", () => {
    const panel: SidePanel = { visual: "app", caption: "无线说话，也能随手控制" };
    const wrapper = mount(StepSide, { props: { panel } });

    expect(wrapper.get("img.side-appicon").attributes("src")).toBe(FACETED_DUCK);
  });

  it("uses the faceted-duck icon and keeps the done badge on the complete step", () => {
    const panel: SidePanel = { visual: "done", caption: "开始使用无线麦" };
    const wrapper = mount(StepSide, { props: { panel } });

    expect(wrapper.get("img.side-appicon").attributes("src")).toBe(FACETED_DUCK);
    expect(wrapper.find(".side-done-badge").exists()).toBe(true);
  });

  it("keeps the panel visible when a check card exists", () => {
    const panel: SidePanel = {
      visual: "remote",
      cardTitle: "连接检查",
      rows: [
        { label: "遥控器已连接", state: "ok" },
        { label: "控制按键已收到", state: "pending" },
      ],
    };
    const wrapper = mount(StepSide, { props: { panel } });

    expect(wrapper.get(".onboarding-side").classes()).not.toContain("is-empty");
    expect(wrapper.find(".onboarding-check-card").exists()).toBe(true);
  });
});
