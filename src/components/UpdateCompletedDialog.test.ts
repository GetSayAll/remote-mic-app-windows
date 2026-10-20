// @vitest-environment jsdom

import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import UpdateCompletedDialog from "./UpdateCompletedDialog.vue";

describe("update completed dialog", () => {
  beforeEach(() => {
    // jsdom 没有 showModal，组件会退回 open 属性；这里清掉上一个用例留下的节点。
    document.body.innerHTML = "";
  });

  it("文案与 Mac update.completed.* 同义：标题 + 版本号", () => {
    const wrapper = mount(UpdateCompletedDialog, { props: { version: "1.0.0" } });

    expect(wrapper.get("h3").text()).toBe("无线麦已更新");
    expect(wrapper.text()).toContain("已成功更新到版本 1.0.0。");
    expect(wrapper.get("button").text()).toBe("好");
  });

  it("点「好」发出关闭事件（父级负责清掉版本号）", async () => {
    const wrapper = mount(UpdateCompletedDialog, { props: { version: "1.0.0" } });

    await wrapper.get("button").trigger("click");
    expect(wrapper.emitted("close")).toHaveLength(1);
    wrapper.unmount();
    await flushPromises();
  });
});
