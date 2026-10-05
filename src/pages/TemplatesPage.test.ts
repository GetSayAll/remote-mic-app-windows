vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async (command: string) => command === "get_ui_preferences" ? {lockButtonSelection:true,templatesExpanded:true,associationsExpanded:true} : undefined) }));
import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import TemplatesPage from "./TemplatesPage.vue";
import ApplicationTemplateBindings from "../components/ApplicationTemplateBindings.vue";
import {
  getMappingConfiguration, listPresetApps, saveMappingConfiguration,
  getSceneSnapshot, subscribeSceneEvents,
  getTemplateCatalog, resetBuiltinTemplate, setButtonMappingFollowEnabled, setMappingNoticeEnabled, setMenuTemplateSwitchEnabled, updateButtonMappingTemplate,
  listRunningApps, removeApplicationBinding, upsertApplicationBinding,
  type MappingConfiguration,
} from "../lib/bridge";

vi.mock("../lib/bridge", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/bridge")>();
  return {
    ...actual,
    getSceneSnapshot: vi.fn(), subscribeSceneEvents: vi.fn(),
    getMappingConfiguration: vi.fn(), listPresetApps: vi.fn(),
    updateButtonMappingTemplate: vi.fn(), resetBuiltinTemplate: vi.fn(), saveMappingConfiguration: vi.fn(),
    getTemplateCatalog: vi.fn(), setButtonMappingFollowEnabled: vi.fn(), setMappingNoticeEnabled: vi.fn(), setMenuTemplateSwitchEnabled: vi.fn(),
    listRunningApps: vi.fn(), removeApplicationBinding: vi.fn(),
    upsertApplicationBinding: vi.fn(),
  };
});

const configuration = {
  menuTemplateSwitchEnabled: false, mappingNoticeEnabled: true, commonMappings: { enabled: true, actions: {} },
  applicationBindings: [{ applicationId: "codex", templateId: "buttons", menuOrder: 0, launchTarget: null }],
  templates: [{ id: "buttons", name: "工作", mappings: { enabled: true, actions: {} } }],
  buttonMappingFollowEnabled: false,
};

const catalog = [
  ...["Agent", "聊天工具", "浏览器"].map((name, i) => ({ id:["preset-agent","preset-chat","preset-browser"][i]!,name,kind:"direct" as const,builtIn:true,buttonMappings:{enabled:true,actions:{}} })),
  { id:"buttons",name:"工作",kind:"direct" as const,builtIn:false,buttonMappings:{enabled:true,actions:{}} },
];

const stubs = {
  MappingTemplatesPanel: true,
  ApplicationTemplateBindings: {
    props: ["configuration"],
    template: "<div data-test='associations'>程序关联</div>",
  },
};

describe("templates page", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(getSceneSnapshot).mockResolvedValue({ templateId: "preset-agent", mappingNotice: { kind: "template", templateId: "preset-agent", name: "Agent", actionsAvailable: true }, mappingNoticeRevision: 1, generation: 1 } as import("../lib/bridge").SceneSnapshot);
    vi.mocked(subscribeSceneEvents).mockResolvedValue(() => {});
    vi.mocked(getMappingConfiguration).mockResolvedValue(structuredClone(configuration));
    vi.mocked(getTemplateCatalog).mockResolvedValue(structuredClone(catalog));
    vi.mocked(listPresetApps).mockResolvedValue([{ id: "edge", name: "Edge", installed: true }]);
    vi.mocked(updateButtonMappingTemplate).mockImplementation(async (id, mappings) => ({ id, name: "工作", mappings }));
    vi.mocked(setButtonMappingFollowEnabled).mockResolvedValue({ ...structuredClone(configuration), buttonMappingFollowEnabled: true });
    vi.mocked(saveMappingConfiguration).mockImplementation(async (next) => structuredClone(next));
    vi.mocked(listRunningApps).mockResolvedValue([{ applicationId: "edge", name: "Edge", preset: true }]);
  });

  it("highlights only the applied template and changes it on runtime application events", async () => {
    let handler!: (event: import("../lib/bridge").SceneEvent) => void;
    vi.mocked(subscribeSceneEvents).mockImplementation(async callback => { handler = callback; return () => {}; });
    const wrapper = mount(TemplatesPage, { global: { stubs } }); await flushPromises();
    expect(wrapper.findAll('.template-row.current-template')).toHaveLength(1);
    expect(wrapper.get('.template-row.current-template').text()).toContain("Agent");
    await wrapper.findAll('.template-row')[3]!.findAll('button').find(button => button.text() === "编辑")!.trigger('click');
    expect(wrapper.get('.template-row.current-template').text()).toContain("Agent");
    handler({ type: "mapping_applied", revision: 2, notice: { kind: "template", templateId: "buttons", name: "工作", actionsAvailable: true } });
    await flushPromises();
    expect(wrapper.get('.template-row.current-template').text()).toContain("工作");
    expect(wrapper.findAll('.template-row.current-template')).toHaveLength(1);
    wrapper.unmount();
  });

  it("does not highlight a pending scene candidate before mappings are applied", async () => {
    vi.mocked(getSceneSnapshot).mockResolvedValueOnce({ templateId: "buttons", mappingNotice: { kind: "template", templateId: "preset-agent", name: "Agent", actionsAvailable: true }, mappingNoticeRevision: 1, generation: 2 } as import("../lib/bridge").SceneSnapshot);
    const wrapper = mount(TemplatesPage, { global: { stubs } }); await flushPromises();
    expect(wrapper.get('.template-row.current-template').text()).toContain("Agent");
    wrapper.unmount();
  });

  it("changes only the persisted notice switch and reads its saved value on reload", async () => {
    vi.mocked(setMappingNoticeEnabled).mockImplementation(async enabled => ({ ...structuredClone(configuration), mappingNoticeEnabled: enabled }));
    const wrapper = mount(TemplatesPage, { global: { stubs } }); await flushPromises();
    const previousStatus = wrapper.find(".operation-message").text();
    const toggle = wrapper.get('input[aria-label="模板切换提示"]');
    expect((toggle.element as HTMLInputElement).checked).toBe(true);
    await toggle.setValue(false); await flushPromises();
    expect(setMappingNoticeEnabled).toHaveBeenCalledWith(false);
    expect((toggle.element as HTMLInputElement).checked).toBe(false);
    expect(wrapper.find(".operation-message").text()).toBe(previousStatus);
    expect(wrapper.get(".rule-feedback").text()).toBe("");
    expect(saveMappingConfiguration).not.toHaveBeenCalled();
    expect(setButtonMappingFollowEnabled).not.toHaveBeenCalled();
    wrapper.unmount();
    vi.mocked(getMappingConfiguration).mockResolvedValue({ ...structuredClone(configuration), mappingNoticeEnabled: false });
    const reopened = mount(TemplatesPage, { global: { stubs } }); await flushPromises();
    expect((reopened.get('input[aria-label="模板切换提示"]').element as HTMLInputElement).checked).toBe(false);
    await reopened.get('input[aria-label="模板切换提示"]').setValue(true); await flushPromises();
    expect(setMappingNoticeEnabled).toHaveBeenLastCalledWith(true);
    reopened.unmount();
  });

  it("enables menu switching through a narrow persisted field without replacing templates", async () => {
    vi.mocked(setMenuTemplateSwitchEnabled).mockImplementation(async enabled => ({ ...structuredClone(configuration), menuTemplateSwitchEnabled: enabled }));
    const wrapper = mount(TemplatesPage, { global: { stubs } }); await flushPromises();
    const previousStatus = wrapper.find(".operation-message").text();
    await wrapper.get('input[aria-label="菜单键选择完整模板"]').setValue(true); await flushPromises();
    expect(setMenuTemplateSwitchEnabled).toHaveBeenCalledExactlyOnceWith(true);
    expect(saveMappingConfiguration).not.toHaveBeenCalled();
    expect(setButtonMappingFollowEnabled).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("同程序换窗不变");
    expect(wrapper.find(".operation-message").text()).toBe(previousStatus);
    wrapper.unmount();
  });

  it("keeps rule feedback local and the persisted checkbox value after a failed save", async () => {
    let rejectSave!: (reason: Error) => void;
    vi.mocked(setMappingNoticeEnabled).mockReturnValueOnce(new Promise((_, reject) => { rejectSave = reject; }));
    const wrapper = mount(TemplatesPage, { global: { stubs } });
    await flushPromises();
    await wrapper.findAll("button").find(button => button.text() === "刷新关联状态")!.trigger("click");
    await flushPromises();
    const previousStatus = wrapper.get(".operation-message").text();
    const feedback = wrapper.get(".rule-feedback").element;
    const toggle = wrapper.get('input[aria-label="模板切换提示"]');
    await toggle.setValue(false);
    expect((toggle.element as HTMLInputElement).checked).toBe(true);
    expect(wrapper.get(".operation-message").text()).toBe(previousStatus);
    rejectSave(new Error("写入被拒绝"));
    await flushPromises();
    expect(wrapper.get(".rule-feedback").element).toBe(feedback);
    expect(wrapper.get(".rule-feedback").text()).toContain("模板切换提示：写入被拒绝");
    expect((toggle.element as HTMLInputElement).checked).toBe(true);
    expect(wrapper.get(".operation-message").text()).toBe(previousStatus);
    vi.mocked(setMappingNoticeEnabled).mockResolvedValueOnce({ ...structuredClone(configuration), mappingNoticeEnabled: false });
    await toggle.setValue(false);
    await flushPromises();
    expect((toggle.element as HTMLInputElement).checked).toBe(false);
    expect(wrapper.get(".rule-feedback").text()).toBe("");
    expect(wrapper.get(".operation-message").text()).toBe(previousStatus);
    wrapper.unmount();
  });

  it("keeps other controls enabled and stable during a rule save and guards repeated clicks", async () => {
    let resolveSave!: (value: MappingConfiguration) => void;
    vi.mocked(setMappingNoticeEnabled).mockReturnValueOnce(new Promise(resolve => { resolveSave = resolve; }));
    const wrapper = mount(TemplatesPage); await flushPromises();
    const refresh = wrapper.findAll("button").find(button => button.text() === "刷新关联状态")!;
    const add = wrapper.findAll("button").find(button => button.text() === "添加程序")!;
    const card = wrapper.get(".run-modes").element;
    const controls = wrapper.findAll(".mode-row input");
    const nodes = controls.map(control => control.element);
    const notice = controls[2]!;
    await notice.setValue(false);
    expect(notice.attributes("aria-busy")).toBe("true");
    expect(notice.attributes("aria-disabled")).toBe("true");
    expect(refresh.attributes("disabled")).toBeUndefined();
    expect(add.attributes("disabled")).toBeUndefined();
    expect(wrapper.get(".run-modes").element).toBe(card);
    controls.forEach((control, index) => {
      expect(control.attributes("disabled")).toBeUndefined();
      expect(control.element).toBe(nodes[index]);
    });
    await notice.setValue(false);
    expect(setMappingNoticeEnabled).toHaveBeenCalledOnce();
    expect((notice.element as HTMLInputElement).checked).toBe(true);
    resolveSave({ ...structuredClone(configuration), mappingNoticeEnabled: false });
    await flushPromises();
    expect((notice.element as HTMLInputElement).checked).toBe(false);
    expect(notice.attributes("aria-busy")).toBe("false");
    expect(wrapper.get(".run-modes").element).toBe(card);
    wrapper.unmount();
  });

  it("merges only the saved rule when independent replies arrive out of order", async () => {
    let resolveFollow!: (value: MappingConfiguration) => void;
    let resolveMenu!: (value: MappingConfiguration) => void;
    vi.mocked(setButtonMappingFollowEnabled).mockReturnValueOnce(new Promise(resolve => { resolveFollow = resolve; }));
    vi.mocked(setMenuTemplateSwitchEnabled).mockReturnValueOnce(new Promise(resolve => { resolveMenu = resolve; }));
    const wrapper = mount(TemplatesPage, { global: { stubs } }); await flushPromises();
    const follow = wrapper.get('input[aria-label="按程序加载默认模板"]');
    const menu = wrapper.get('input[aria-label="菜单键选择完整模板"]');
    await follow.setValue(true); await menu.setValue(true);
    expect(setButtonMappingFollowEnabled).toHaveBeenCalledOnce();
    expect(setMenuTemplateSwitchEnabled).toHaveBeenCalledOnce();
    resolveMenu({ ...structuredClone(configuration), menuTemplateSwitchEnabled: true });
    await flushPromises();
    resolveFollow({ ...structuredClone(configuration), buttonMappingFollowEnabled: true });
    await flushPromises();
    expect((follow.element as HTMLInputElement).checked).toBe(true);
    expect((menu.element as HTMLInputElement).checked).toBe(true);
    expect(wrapper.findComponent(ApplicationTemplateBindings).props("configuration").templates).toEqual(configuration.templates);
    wrapper.unmount();
  });

  it("edits all three columns in a scrollable dialog and persists only the selected template", async () => {
    const wrapper = mount(TemplatesPage, { global: { stubs } });
    await flushPromises();
    await wrapper.findAll(".complete-template-panel .template-row")[3]!.findAll("button").find((button) => button.text() === "编辑")!.trigger("click");
    expect(wrapper.find("[role='dialog']").attributes("aria-label")).toContain("编辑完整按键模板");
    expect(wrapper.findAll(".template-grid .mapping-cell")).toHaveLength(39);
    await wrapper.findAll(".template-grid .mapping-cell")[0]!.trigger("click");
    wrapper.findComponent({ name: "ButtonActionEditor" }).vm.$emit("update", { type: "shortcut", chord: { keys: ["enter"] } });
    await wrapper.findAll("[role='dialog'] button").find((button) => button.text() === "保存模板")!.trigger("click");
    await flushPromises();
    expect(updateButtonMappingTemplate).toHaveBeenCalledWith("buttons", expect.objectContaining({
      actions: expect.objectContaining({ back: expect.objectContaining({ single: { type: "shortcut", chord: { keys: ["enter"] } } }) }),
    }));
    expect(saveMappingConfiguration).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("若正在使用此模板，实际按键映射会同步更新");
    expect(wrapper.find("[role='dialog']").exists()).toBe(false);
    await wrapper.findAll(".complete-template-panel .template-row")[3]!.findAll("button").find((button) => button.text() === "编辑")!.trigger("click");
    expect(wrapper.findAll(".template-grid .mapping-cell")[0]!.text()).toContain("Enter");
  });

  it("orders independently collapsible sections and preserves their state through refresh and rule saves", async () => {
    const wrapper = mount(TemplatesPage); await flushPromises();
    expect(wrapper.findAll(".run-modes, .complete-template-panel, .application-associations").map(section => section.classes()[section.classes().length - 1]))
      .toEqual(["run-modes", "complete-template-panel", "application-associations"]);
    const templates = wrapper.get("details.complete-template-panel");
    const associations = wrapper.get("details.application-associations");
    expect((templates.element as HTMLDetailsElement).open).toBe(true);
    expect((associations.element as HTMLDetailsElement).open).toBe(true);
    expect(templates.get("summary").text()).toContain("4 个模板");
    expect(associations.get("summary").text()).toContain("1 个程序");
    const childId = wrapper.findComponent(ApplicationTemplateBindings).vm.$.uid;
    const nodes = [templates.element, associations.element];
    await templates.get("summary").trigger("click");
    expect((templates.element as HTMLDetailsElement).open).toBe(false);
    expect((associations.element as HTMLDetailsElement).open).toBe(true);
    await associations.get("summary").trigger("click");
    await wrapper.findAll("button").find(button => button.text() === "刷新关联状态")!.trigger("click");
    await flushPromises();
    vi.mocked(setMappingNoticeEnabled).mockResolvedValueOnce({ ...structuredClone(configuration), mappingNoticeEnabled: false });
    await wrapper.get('input[aria-label="模板切换提示"]').setValue(false);
    await flushPromises();
    expect(wrapper.findComponent(ApplicationTemplateBindings).vm.$.uid).toBe(childId);
    expect(wrapper.get("details.complete-template-panel").element).toBe(nodes[0]);
    expect(wrapper.get("details.application-associations").element).toBe(nodes[1]);
    expect((templates.element as HTMLDetailsElement).open).toBe(false);
    expect((associations.element as HTMLDetailsElement).open).toBe(false);
    await templates.get("summary").trigger("click");
    expect((templates.element as HTMLDetailsElement).open).toBe(true);
    expect((associations.element as HTMLDetailsElement).open).toBe(false);
    wrapper.unmount();
  });

  it("keeps header actions separate from collapse and preserves the editor draft", async () => {
    const wrapper = mount(TemplatesPage); await flushPromises();
    const templates = wrapper.get("details.complete-template-panel");
    const associations = wrapper.get("details.application-associations");
    expect(templates.get("summary").find("button").exists()).toBe(false);
    expect(associations.get("summary").find("button").exists()).toBe(false);
    await associations.findAll("button").find(button => button.text() === "添加程序")!.trigger("click");
    await flushPromises();
    expect((associations.element as HTMLDetailsElement).open).toBe(true);
    expect(wrapper.find("[role='dialog']").exists()).toBe(true);
    await wrapper.findAll("button").find(button => button.text() === "取消")!.trigger("click");
    await templates.findAll("button").find(button => button.text() === "编辑")!.trigger("click");
    await wrapper.findAll(".template-grid .mapping-cell")[0]!.trigger("click");
    wrapper.findComponent({ name: "ButtonActionEditor" }).vm.$emit("update", { type: "shortcut", chord: { keys: ["space"] } });
    await flushPromises();
    expect((templates.element as HTMLDetailsElement).open).toBe(true);
    await templates.get("summary").trigger("click");
    await templates.get("summary").trigger("click");
    expect(wrapper.find("[role='dialog']").exists()).toBe(true);
    expect(wrapper.findAll(".template-grid .mapping-cell")[0]!.text()).toContain("空格");
    expect(updateButtonMappingTemplate).not.toHaveBeenCalled();
    wrapper.unmount();
  });

  it("keeps the edit dialog and draft visible when saving fails", async () => {
    vi.mocked(updateButtonMappingTemplate).mockRejectedValueOnce(new Error("磁盘只读"));
    const wrapper = mount(TemplatesPage, { global: { stubs } });
    await flushPromises();
    await wrapper.findAll("button").find((button) => button.text() === "编辑")!.trigger("click");
    await wrapper.findAll(".template-grid .mapping-cell")[0]!.trigger("click");
    wrapper.findComponent({ name: "ButtonActionEditor" }).vm.$emit("update", { type: "shortcut", chord: { keys: ["space"] } });
    await wrapper.findAll("[role='dialog'] button").find((button) => button.text() === "保存模板")!.trigger("click");
    await flushPromises();
    expect(wrapper.find("[role='dialog']").exists()).toBe(true);
    expect(wrapper.text()).toContain("磁盘只读");
    expect(wrapper.findAll(".template-grid .mapping-cell")[0]!.text()).toContain("空格");
  });

  it("only exposes program defaults, menu selection and notice controls", async () => {
    vi.mocked(getMappingConfiguration).mockResolvedValue({ ...structuredClone(configuration) });
    const wrapper = mount(TemplatesPage, { global: { stubs } });
    await flushPromises();
    const modeButtons = wrapper.findAll('.mode-row input[type="checkbox"]');
    expect(modeButtons).toHaveLength(3);
    expect(wrapper.text()).not.toContain("场景控制");
    expect(wrapper.findComponent({ name: "MappingTemplatesPanel" }).exists()).toBe(false);
    await modeButtons[0]!.setValue(true);
    await flushPromises();
    expect(setButtonMappingFollowEnabled).toHaveBeenCalledWith(true);

    expect(saveMappingConfiguration).not.toHaveBeenCalled();
  });

  it("reads fresh configuration without unmounting or replacing the association wizard draft", async () => {
    const wrapper = mount(TemplatesPage);
    await flushPromises();
    const child = wrapper.findComponent(ApplicationTemplateBindings);
    const associationElement = child.find(".application-associations").element;
    await child.findAll("button").find((item) => item.text() === "添加程序")!.trigger("click");
    await flushPromises();
    await child.find(".choice-card").trigger("click");
    await child.findAll("button").find((item) => item.text() === "下一步")!.trigger("click");
    await child.findAll(".template-choice").find((item) => item.text().includes("工作"))!.trigger("click");
    let rejectRead!: (error: Error) => void;
    vi.mocked(getMappingConfiguration).mockReturnValueOnce(new Promise((_, reject) => { rejectRead = reject; }));
    const refresh = wrapper.findAll("button").find((item) => item.text() === "刷新关联状态")!;
    await refresh.trigger("click");
    expect(wrapper.find(".application-associations").element).toBe(associationElement);
    expect(child.find(".template-choice.selected").text()).toContain("工作");
    expect(child.props("disabled")).toBe(true);
    rejectRead(new Error("刷新读取失败"));
    await flushPromises();
    expect(child.find(".template-choice.selected").text()).toContain("工作");
    expect(wrapper.text()).toContain("刷新读取失败");
    vi.mocked(getMappingConfiguration).mockResolvedValueOnce({ ...structuredClone(configuration), buttonMappingFollowEnabled: true });
    await refresh.trigger("click");
    await flushPromises();
    expect(child.props("configuration").buttonMappingFollowEnabled).toBe(true);
    expect(child.find(".template-choice.selected").text()).toContain("工作");
    expect(getMappingConfiguration).toHaveBeenCalledTimes(3);
    expect(saveMappingConfiguration).not.toHaveBeenCalled();
    expect(setButtonMappingFollowEnabled).not.toHaveBeenCalled();
    expect(upsertApplicationBinding).not.toHaveBeenCalled();
  });

  it("preserves an unsaved button action when refreshing the catalog", async () => {
    const wrapper = mount(TemplatesPage, { global: { stubs } });
    await flushPromises();
    await wrapper.findAll("button").find((item) => item.text() === "编辑")!.trigger("click");
    await wrapper.findAll(".template-grid .mapping-cell")[0]!.trigger("click");
    wrapper.findComponent({ name: "ButtonActionEditor" }).vm.$emit("update", { type: "shortcut", chord: { keys: ["space"] } });
    await wrapper.findAll("button").find((item) => item.text() === "刷新关联状态")!.trigger("click");
    await flushPromises();
    expect(wrapper.find("[role='dialog']").exists()).toBe(true);
    expect(wrapper.findAll(".template-grid .mapping-cell")[0]!.text()).toContain("空格");
    expect(updateButtonMappingTemplate).not.toHaveBeenCalled();
  });

  it("blocks parent refresh and follow changes until a child save settles", async () => {
    let rejectSave!: (error: Error) => void;
    vi.mocked(removeApplicationBinding).mockReturnValueOnce(new Promise<MappingConfiguration>((_, reject) => { rejectSave = reject; }));
    const wrapper = mount(TemplatesPage);
    await flushPromises();
    await wrapper.find(".association-actions").findAll("button")[1]!.trigger("click");
    const refresh = wrapper.findAll("button").find((item) => item.text() === "刷新关联状态")!;
    expect(refresh.attributes("disabled")).toBeDefined();
    expect(wrapper.find(".mode-row input").attributes("disabled")).toBeDefined();
    await refresh.trigger("click");
    await wrapper.find(".mode-row input").trigger("change");
    expect(getMappingConfiguration).toHaveBeenCalledOnce();
    expect(setButtonMappingFollowEnabled).not.toHaveBeenCalled();
    rejectSave(new Error("关联保存失败"));
    await flushPromises();
    expect(wrapper.findAll(".association-row")).toHaveLength(1);
    expect(wrapper.text()).toContain("关联保存失败");
    expect(refresh.attributes("disabled")).toBeUndefined();
  });

  it("edits stable defaults and resets only an explicitly confirmed default", async () => {
    const wrapper = mount(TemplatesPage, { global: { stubs } });
    await flushPromises();
    const rows = wrapper.findAll(".complete-template-panel .template-row");
    expect(rows.slice(0, 3).map((row) => row.text())).toEqual(expect.arrayContaining([
      expect.stringContaining("Agent"), expect.stringContaining("聊天工具"), expect.stringContaining("浏览器"),
    ]));
    expect(rows[0]!.text()).toContain("编辑");
    expect(rows[0]!.text()).toContain("复位模板");
    expect(rows[0]!.text()).toContain("复制");
    expect(rows[0]!.text()).not.toContain("重命名");
    expect(rows[0]!.text()).not.toContain("删除");
    expect(rows[3]!.text()).toContain("编辑");
    expect(rows[3]!.text()).toContain("重命名");
    expect(rows[3]!.text()).toContain("删除");
    const confirm = vi.spyOn(window, "confirm").mockReturnValueOnce(false).mockReturnValueOnce(true);
    const reset = rows[0]!.findAll("button").find(button => button.text() === "复位模板")!;
    await reset.trigger("click"); await flushPromises();
    expect(resetBuiltinTemplate).not.toHaveBeenCalled();
    vi.mocked(resetBuiltinTemplate).mockResolvedValue(structuredClone(configuration));
    await reset.trigger("click"); await flushPromises();
    expect(resetBuiltinTemplate).toHaveBeenCalledExactlyOnceWith("preset-agent");
    expect(confirm).toHaveBeenLastCalledWith(expect.stringContaining("只复位"));
    expect(saveMappingConfiguration).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("已复位");
  });

  it("shows an actionable load failure", async () => {
    vi.mocked(getMappingConfiguration).mockRejectedValueOnce(new Error("读取失败"));
    const wrapper = mount(TemplatesPage, { global: { stubs } });
    await flushPromises();
    expect(wrapper.text()).toContain("读取失败");
    expect(wrapper.findAll("button").some((button) => button.text() === "重试")).toBe(true);
  });
});
