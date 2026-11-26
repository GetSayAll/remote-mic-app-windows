import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import HelpQrDialog from "./HelpQrDialog.vue";

describe("HelpQrDialog", () => {
  it("shows both official QR codes and emits close", async () => {
    const wrapper = mount(HelpQrDialog, { attachTo: document.body });

    expect(wrapper.get("h3").text()).toBe("配置有问题？");
    expect(wrapper.text()).toContain("扫码关注抖音或小红书，查看设置教程。");
    expect(wrapper.findAll(".help-qr-card img").map((img) => img.attributes("src"))).toEqual([
      "/douyin-qr.png",
      "/xhs-qr.png",
    ]);

    await wrapper.get(".help-qr-footer button").trigger("click");
    expect(wrapper.emitted("close")).toHaveLength(1);

    wrapper.unmount();
  });
});
