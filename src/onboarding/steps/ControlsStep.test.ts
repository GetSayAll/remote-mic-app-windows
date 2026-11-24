/**
 * 普通按键步骤（2026-11-24 用户要求）：静音键不再出现在芯片格，也不参与
 * 3 个不同按键的进度与门禁计数。
 */
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import ControlsStep from "./ControlsStep.vue";

const CHIP_LABELS = [
  "电源",
  "主页",
  "返回",
  "菜单",
  "TV",
  "确定",
  "上",
  "下",
  "左",
  "右",
  "音量+",
  "音量−",
];

function chipLabels(wrapper: ReturnType<typeof mount>): string[] {
  return wrapper.findAll(".onboarding-bchip").map((node) => node.text());
}

describe("ControlsStep 按键芯片格", () => {
  it("固定展示 12 个普通按键，不再包含静音", () => {
    const wrapper = mount(ControlsStep, {
      props: { observed: [], voiceKeyMistake: false },
    });

    expect(chipLabels(wrapper)).toEqual(CHIP_LABELS);
    expect(chipLabels(wrapper)).not.toContain("静音");
    wrapper.unmount();
  });

  it("静音边沿不参与进度点与剩余计数", () => {
    const wrapper = mount(ControlsStep, {
      props: {
        observed: [
          { button: "home", count: 1 },
          { button: "ok", count: 1 },
          { button: "volume_mute", count: 2 },
        ],
        voiceKeyMistake: false,
      },
    });

    expect(wrapper.findAll(".onboarding-dots .dot.on")).toHaveLength(2);
    expect(wrapper.text()).toContain("再按 1 个不同的普通按键。");
    wrapper.unmount();
  });
});
