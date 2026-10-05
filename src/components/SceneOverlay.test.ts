import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MappingNotice, SceneEvent, SceneSnapshot } from "../lib/bridge";
import SceneOverlay from "./SceneOverlay.vue";

const mocks = vi.hoisted(() => ({ get: vi.fn(), subscribe: vi.fn(), invoke: vi.fn(), stop: vi.fn() }));
vi.mock("../lib/bridge", () => ({ getSceneSnapshot: mocks.get, subscribeSceneEvents: mocks.subscribe }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
let receive: (event: SceneEvent) => void;
const direct: MappingNotice = { kind: "direct", templateId: "actual", name: "实际前台模板", actionsAvailable: true };
function snapshot(panel: SceneSnapshot["panel"] = null): SceneSnapshot {
  return { mappingNoticeEnabled: true, mappingNotice: null, mappingNoticeRevision: 0, enabled: true, generation: 1,
    foregroundGeneration: 1, applicationId: "notepad", templateId: null, updateDefault:false, panel, selectedIndex: null, menuItems: [],
    waitingForRelease: false, voiceActive: false, status: null };
}
beforeEach(() => {
  vi.useFakeTimers(); vi.clearAllMocks();
  mocks.get.mockResolvedValue(null);
  mocks.invoke.mockResolvedValue(undefined);
  mocks.subscribe.mockImplementation(async (handler: typeof receive) => { receive = handler; return mocks.stop; });
});
afterEach(() => vi.useRealTimers());

describe("applied mapping switch notice", () => {
  it("routes rapid native DOWN/UP only inside the full template menu in order", async () => {
    const wrapper = mount(SceneOverlay); await flushPromises();
    receive({ type:"snapshot", snapshot: snapshot("template") }); await flushPromises();
    const down = new KeyboardEvent("keydown", { key:"Enter", cancelable:true });
    const up = new KeyboardEvent("keyup", { key:"Enter", cancelable:true });
    window.dispatchEvent(down); window.dispatchEvent(up); await flushPromises();
    expect(down.defaultPrevented).toBe(true); expect(up.defaultPrevented).toBe(true);
    expect(mocks.invoke.mock.calls).toEqual([
      ["template_menu_key", { generation:1, key:"Enter", down:true }],
      ["template_menu_key", { generation:1, key:"Enter", down:false }],
    ]);
    receive({ type:"snapshot", snapshot:snapshot() }); await flushPromises();
    window.dispatchEvent(new KeyboardEvent("keydown", { key:"Enter" })); await flushPromises();
    expect(mocks.invoke).toHaveBeenCalledTimes(2);
    wrapper.unmount();
  });
  it("restores the persisted preference each opening and sends a change before a rapid confirm", async () => {
    const wrapper=mount(SceneOverlay);await flushPromises();
    receive({type:"snapshot",snapshot:snapshot("template")});await flushPromises();
    const toggle=wrapper.get('input[type="checkbox"]');
    expect((toggle.element as HTMLInputElement).checked).toBe(false);
    await toggle.setValue(true);
    window.dispatchEvent(new KeyboardEvent("keydown",{key:"Enter"}));
    window.dispatchEvent(new KeyboardEvent("keyup",{key:"Enter"}));await flushPromises();
    expect(mocks.invoke.mock.calls.map(call=>call[0])).toEqual(["set_template_menu_update_default","template_menu_key","template_menu_key"]);
    expect(mocks.invoke).toHaveBeenCalledWith("set_template_menu_update_default",{generation:1,enabled:true});
    receive({type:"snapshot",snapshot:{...snapshot("template"),generation:2,updateDefault:true}});await flushPromises();
    expect((wrapper.get('input[type="checkbox"]').element as HTMLInputElement).checked).toBe(true);wrapper.unmount();
  });
  it("shows pending/failure honestly and prevents duplicate checkbox changes while saving", async () => {
    const wrapper=mount(SceneOverlay);await flushPromises();
    receive({type:"snapshot",snapshot:{...snapshot("template"),updateDefault:true,preferencePending:true}});await flushPromises();
    const checkbox=wrapper.get('input[type="checkbox"]');
    await checkbox.setValue(false);await flushPromises();
    expect((checkbox.element as HTMLInputElement).checked).toBe(true);
    expect(mocks.invoke).not.toHaveBeenCalled();expect(wrapper.text()).toContain("正在保存选项");
    receive({type:"snapshot",snapshot:{...snapshot("template"),preferenceError:true}});await flushPromises();
    expect((checkbox.element as HTMLInputElement).checked).toBe(false);
    expect(wrapper.text()).toContain("选项保存失败");wrapper.unmount();
  });
  it("does not display an old checkbox IPC failure in a newly opened panel", async () => {
    const wrapper=mount(SceneOverlay);await flushPromises();
    receive({type:"snapshot",snapshot:snapshot("template")});await flushPromises();
    let reject!: (reason: Error) => void;
    mocks.invoke.mockImplementationOnce(() => new Promise((_resolve, fail) => { reject=fail; }));
    await wrapper.get('input[type="checkbox"]').setValue(true);await flushPromises();
    receive({type:"snapshot",snapshot:{...snapshot("template"),generation:2,updateDefault:true}});await flushPromises();
    reject(new Error("old panel closed"));await flushPromises();
    expect(wrapper.find('[role="alert"]').exists()).toBe(false);
    expect((wrapper.get('input[type="checkbox"]').element as HTMLInputElement).checked).toBe(true);wrapper.unmount();
  });
  it("keeps feedback/list/checkbox nodes and scroll stable across preference receipts", async () => {
    const wrapper=mount(SceneOverlay); await flushPromises();
    const value={...snapshot("template"),selectedIndex:0,menuItems:[{label:"Agent",templateId:"agent",applicationId:null,running:true}]};
    receive({type:"snapshot",snapshot:value}); await flushPromises();
    const list=wrapper.get("ul").element;
    const feedback=wrapper.get(".preference-feedback").element;
    const checkbox=wrapper.get('input[type="checkbox"]').element as HTMLInputElement;
    const viewport=vi.spyOn(list,"getBoundingClientRect");
    list.scrollTop=17;
    for (const state of [{updateDefault:true,preferencePending:true},{updateDefault:true,preferencePending:false},{updateDefault:false,preferenceError:true}]) {
      receive({type:"snapshot",snapshot:{...value,...state}}); await flushPromises();
      expect(wrapper.get("ul").element).toBe(list);
      expect(wrapper.get(".preference-feedback").element).toBe(feedback);
      expect(wrapper.get('input[type="checkbox"]').element).toBe(checkbox);
      expect(checkbox.disabled).toBe(false);
      expect(list.scrollTop).toBe(17);
      expect(wrapper.findAll(".preference-feedback small")).toHaveLength(3);
    }
    expect(viewport).not.toHaveBeenCalled();
    expect(wrapper.get('[role="alert"]').text()).toContain("选项保存失败");
    wrapper.unmount();
  });
  it("never claims a persisted default before the real save result", async()=>{
    const wrapper=mount(SceneOverlay);await flushPromises();
    receive({type:"mapping_applied",notice:{...direct,defaultSaveStatus:"saving"},revision:1});await flushPromises();
    expect(wrapper.text()).toContain("正在保存");expect(wrapper.text()).not.toContain("已更新");
    receive({type:"mapping_applied",notice:{...direct,defaultSaveStatus:"failed"},revision:2});await flushPromises();
    expect(wrapper.text()).toContain("保存失败，本次临时使用");
    receive({type:"mapping_applied",notice:{...direct,defaultSaveStatus:"saved"},revision:3});await flushPromises();
    expect(wrapper.text()).toContain("已更新程序默认");wrapper.unmount();
  });
  it("reflects remote hold intent on the existing checkbox without writing it back", async () => {
    const wrapper = mount(SceneOverlay); await flushPromises();
    receive({ type:"snapshot", snapshot:snapshot("template") }); await flushPromises();
    const input = wrapper.get('input[type="checkbox"]').element as HTMLInputElement;
    expect(wrapper.text()).toContain("长按菜单键：切换此项并记住");
    for (const updateDefault of [true, false]) {
      receive({ type:"snapshot", snapshot:{ ...snapshot("template"), updateDefault } });
      await flushPromises();
      expect(wrapper.get('input[type="checkbox"]').element).toBe(input);
      expect(input.checked).toBe(updateDefault);
      expect(mocks.invoke).not.toHaveBeenCalled();
    }
    wrapper.unmount();
  });
  it("scrolls a long menu to the selected last row and back to the first", async () => {
    const wrapper = mount(SceneOverlay); await flushPromises();
    const items = Array.from({ length:30 }, (_, i) => ({ label:"Template " + i, templateId:"t" + i, applicationId:null, running:false }));
    const value = { ...snapshot("template"), menuItems:items, selectedIndex:0 };
    receive({ type:"snapshot", snapshot:value }); await flushPromises();
    const list = wrapper.get("ul").element;
    vi.spyOn(list, "getBoundingClientRect").mockReturnValue({ top:40, bottom:240 } as DOMRect);
    const last = wrapper.findAll("li")[29].element;
    vi.spyOn(last, "getBoundingClientRect").mockReturnValue({ top:1780, bottom:1840 } as DOMRect);
    receive({ type:"snapshot", snapshot:{ ...value, selectedIndex:29 } }); await flushPromises();
    expect(list.scrollTop).toBe(1600);
    vi.spyOn(wrapper.findAll("li")[0].element, "getBoundingClientRect").mockReturnValue({ top:-1560, bottom:-1500 } as DOMRect);
    receive({ type:"snapshot", snapshot:value }); await flushPromises();
    expect(list.scrollTop).toBe(0);
    wrapper.unmount();
  });
  it("shows the initial confirmed profile once and stays quiet on duplicate snapshots", async () => {
    mocks.get.mockResolvedValue({ ...snapshot(), mappingNotice: direct, mappingNoticeRevision: 1 });
    const wrapper = mount(SceneOverlay); await flushPromises();
    expect(wrapper.text()).toContain("当前模板：实际前台模板");
    await vi.advanceTimersByTimeAsync(2400);
    expect(wrapper.find("[role=status]").exists()).toBe(false);
    expect(mocks.invoke).toHaveBeenCalledWith("dismiss_mapping_notice", { revision: 1 });
    receive({ type: "mapping_applied", notice: direct, revision: 1 }); await flushPromises();
    expect(wrapper.find("[role=status]").exists()).toBe(false);
    wrapper.unmount();
  });
  it("shows new effective states and ignores an older response", async () => {
    const wrapper = mount(SceneOverlay); await flushPromises();
    receive({ type: "mapping_applied", notice: direct, revision: 2 }); await flushPromises();
    receive({ type: "mapping_applied", notice: { ...direct, name: "旧请求" }, revision: 1 }); await flushPromises();
    expect(wrapper.text()).toContain("实际前台模板");
    receive({ type: "mapping_applied", notice: { kind: "unconfigured", name: null, templateId: null, actionsAvailable: false }, revision: 3 }); await flushPromises();
    expect(wrapper.text()).toBe("当前窗口未配置映射");
    wrapper.unmount();
  });
  it("distinguishes disabled and unavailable from an enabled template", async () => {
    const wrapper = mount(SceneOverlay); await flushPromises();
    receive({ type: "mapping_applied", notice: { ...direct, kind: "disabled", actionsAvailable: false }, revision: 1 }); await flushPromises();
    expect(wrapper.text()).toContain("映射已停用");
    receive({ type: "mapping_applied", notice: { ...direct, actionsAvailable: false }, revision: 2 }); await flushPromises();
    expect(wrapper.text()).toContain("按键暂不可用");
    wrapper.unmount();
  });
  it("gives the existing menu priority and never revives a notice when it closes", async () => {
    const wrapper = mount(SceneOverlay); await flushPromises();
    receive({ type: "mapping_applied", notice: direct, revision: 1 }); await flushPromises();
    receive({ type: "snapshot", snapshot: snapshot("template") }); await flushPromises();
    expect(wrapper.text()).toContain("完整按键模板");
    receive({ type: "mapping_applied", notice: { ...direct, name: "下一模板" }, revision: 2 }); await flushPromises();
    await vi.advanceTimersByTimeAsync(3000);
    expect(mocks.invoke).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("完整按键模板");
    receive({ type: "snapshot", snapshot: snapshot() }); await flushPromises();
    expect(wrapper.find("[role=status]").exists()).toBe(false);
    wrapper.unmount();
  });
  it("hides immediately when disabled, keeps menus, and only shows a new change after re-enabling", async () => {
    const wrapper = mount(SceneOverlay); await flushPromises();
    receive({ type: "mapping_applied", notice: direct, revision: 1 }); await flushPromises();
    receive({ type: "mapping_notice_enabled", enabled: false }); await flushPromises();
    expect(wrapper.find("[role=status]").exists()).toBe(false);
    receive({ type: "mapping_applied", notice: direct, revision: 2 }); await flushPromises();
    await vi.advanceTimersByTimeAsync(3000);
    expect(mocks.invoke).not.toHaveBeenCalled();
    receive({ type: "snapshot", snapshot: { ...snapshot("template"), mappingNoticeEnabled: false } }); await flushPromises();
    expect(wrapper.text()).toContain("完整按键模板");
    receive({ type: "snapshot", snapshot: { ...snapshot(), mappingNoticeEnabled: false } });
    receive({ type: "mapping_notice_enabled", enabled: true }); await flushPromises();
    expect(wrapper.find("[role=status]").exists()).toBe(false);
    receive({ type: "mapping_applied", notice: direct, revision: 3 }); await flushPromises();
    expect(wrapper.text()).toContain("实际前台模板");
    wrapper.unmount();
  });
  it("honors a persisted disabled initial state", async () => {
    mocks.get.mockResolvedValue({ ...snapshot(), mappingNoticeEnabled: false, mappingNotice: direct, mappingNoticeRevision: 1 });
    const wrapper = mount(SceneOverlay); await flushPromises();
    expect(wrapper.find("[role=status]").exists()).toBe(false);
    await vi.advanceTimersByTimeAsync(3000); expect(mocks.invoke).not.toHaveBeenCalled(); wrapper.unmount();
  });
  it("sizes wrapped content from its measured height without shortening its text", async () => {
    const bounds = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ height: 96 } as DOMRect);
    const wrapper = mount(SceneOverlay); await flushPromises();
    const long = { ...direct, name: "长模板名称".repeat(12) };
    receive({ type: "mapping_applied", notice: long, revision: 1 }); await flushPromises();
    expect(wrapper.text()).toContain(long.name);
    expect(mocks.invoke).toHaveBeenCalledWith("size_mapping_notice", { revision: 1, height: 112 });
    wrapper.unmount(); bounds.mockRestore();
  });
  it("cancels expiry and unsubscribes on disposal", async () => {
    const wrapper = mount(SceneOverlay); await flushPromises();
    receive({ type: "mapping_applied", notice: direct, revision: 1 }); await flushPromises();
    wrapper.unmount(); await vi.advanceTimersByTimeAsync(3000);
    expect(mocks.stop).toHaveBeenCalledOnce();
    expect(mocks.invoke).not.toHaveBeenCalled();
  });
});
