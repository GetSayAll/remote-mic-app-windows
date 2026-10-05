import { defineComponent } from "vue";
import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useUiPreference, type UiPreferences } from "./ui-preferences";
const mock = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mock.invoke }));
let disk: UiPreferences;
beforeEach(() => {
  disk = { lockButtonSelection:false, templatesExpanded:false, associationsExpanded:false };
  mock.invoke.mockReset().mockImplementation(async (command, args) => {
    if (command === "get_ui_preferences") return { ...disk };
    disk[args.field as keyof UiPreferences] = args.enabled;
  });
});
function fixture(field: keyof UiPreferences) {
  return defineComponent({ setup: () => ({ pref:useUiPreference(field) }),
    template:'<details :open="pref.value.value" @toggle="pref.toggleDetails"><summary>Section</summary><input type="checkbox" :checked="pref.value.value" @change="pref.toggleCheckbox"><span>Draft retained</span></details><p role="alert">{{pref.error.value}}</p>' });
}
describe("persisted UI preferences", () => {
  it.each(["templatesExpanded", "associationsExpanded"] as const)("loads closed %s without initial toggle writes, and reopens closed", async field => {
    let resolve!: (value: UiPreferences) => void;
    mock.invoke.mockImplementationOnce(() => new Promise(done => { resolve = done; }));
    const wrapper = mount(fixture(field));
    await wrapper.get("details").trigger("toggle");
    expect(mock.invoke).toHaveBeenCalledTimes(1);
    resolve({ ...disk }); await flushPromises();
    expect((wrapper.get("details").element as HTMLDetailsElement).open).toBe(false);
    await wrapper.get("details").trigger("toggle");
    expect(mock.invoke).toHaveBeenCalledTimes(1);
    wrapper.unmount();
    const reopened = mount(fixture(field)); await flushPromises();
    expect((reopened.get("details").element as HTMLDetailsElement).open).toBe(false);
    expect(mock.invoke.mock.calls.every(call => call[0] === "get_ui_preferences")).toBe(true);
    reopened.unmount();
  });
  it("persists the editor selection lock, guards duplicate native checkbox changes and retains the node", async () => {
    const wrapper = mount(fixture("lockButtonSelection")); await flushPromises();
    let finish!: () => void;
    mock.invoke.mockImplementationOnce(() => new Promise<void>(done => { finish=()=>{disk.lockButtonSelection=true;done();}; }));
    const checkbox = wrapper.get("input"); const node = checkbox.element as HTMLInputElement;
    await checkbox.setValue(true); await checkbox.setValue(true);
    expect(node.checked).toBe(false);
    expect(mock.invoke.mock.calls.filter(call => call[0] === "set_ui_preference")).toHaveLength(1);
    finish(); await flushPromises();
    expect(checkbox.element).toBe(node); expect(node.checked).toBe(true);
    wrapper.unmount();
    const reopened = mount(fixture("lockButtonSelection")); await flushPromises();
    expect((reopened.get("input").element as HTMLInputElement).checked).toBe(true); reopened.unmount();
  });
  it("keeps the persisted value and visible error after failure, then permits a retry", async () => {
    const wrapper=mount(fixture("templatesExpanded"));await flushPromises();
    mock.invoke.mockRejectedValueOnce(new Error("write rejected"));
    const details=wrapper.get("details"); (details.element as HTMLDetailsElement).open=true;
    await details.trigger("toggle");await flushPromises();
    expect((details.element as HTMLDetailsElement).open).toBe(false);
    expect(wrapper.get('[role="alert"]').text()).toContain("保存失败");
    expect(disk.templatesExpanded).toBe(false);
    (details.element as HTMLDetailsElement).open=true;
    await details.trigger("toggle");await flushPromises();
    expect(disk.templatesExpanded).toBe(true);
    expect(wrapper.get('[role="alert"]').text()).toBe("");wrapper.unmount();
  });
});
