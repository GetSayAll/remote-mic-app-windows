vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async (command: string) => command === "get_ui_preferences" ? {lockButtonSelection:true,templatesExpanded:true,associationsExpanded:true} : undefined) }));
import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ApplicationTemplateBindings from "./ApplicationTemplateBindings.vue";
import {
  listPresetApps, listRunningApps, pickCustomApp,
  removeApplicationBinding,
  reorderApplicationAssociations, saveMappingConfiguration, upsertApplicationBinding,
  type MappingConfiguration, type TemplateCatalogEntry,
} from "../lib/bridge";

vi.mock("../lib/bridge", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/bridge")>();
  return {
    ...actual,
    listPresetApps: vi.fn(), listRunningApps: vi.fn(), pickCustomApp: vi.fn(),
    removeApplicationBinding: vi.fn(),
    reorderApplicationAssociations: vi.fn(), saveMappingConfiguration: vi.fn(),
    upsertApplicationBinding: vi.fn(),
  };
});

const base: MappingConfiguration = {
  menuTemplateSwitchEnabled: false, mappingNoticeEnabled: true, commonMappings: { enabled: true, actions: {} },
  templates: [{ id: "scene", name: "Codex 模板", mappings: { enabled:true, actions:{} } }, { id:"buttons",name:"工作按键",mappings:{enabled:true,actions:{}} }],
  applicationBindings: [{ applicationId: "codex", templateId: "scene", menuOrder: 0, launchTarget: null }],
  buttonMappingFollowEnabled: false,
};

const catalog: TemplateCatalogEntry[] = [
  { id:"preset-agent", name:"Agent", kind:"direct", builtIn:true, buttonMappings:{enabled:true,actions:{}} },
  ...base.templates.map(t => ({ id:t.id, name:t.name, kind:"direct" as const, builtIn:false, buttonMappings:t.mappings })),
];

function mountPanel(configuration = structuredClone(base)) {
  return mount(ApplicationTemplateBindings, { props: { configuration, catalog: structuredClone(catalog) } });
}

function button(wrapper: VueWrapper, label: string) {
  return wrapper.findAll("button").find((item) => item.text() === label)!;
}

describe("application template binding wizard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(listRunningApps).mockResolvedValue([{ applicationId: "edge", name: "Edge 浏览器", preset: true }]);
    vi.mocked(listPresetApps).mockResolvedValue([{ id: "edge", name: "Edge 浏览器", installed: true }]);
    vi.mocked(pickCustomApp).mockResolvedValue(null);
    vi.mocked(upsertApplicationBinding).mockResolvedValue(structuredClone(base));
    vi.mocked(removeApplicationBinding).mockResolvedValue(structuredClone(base));
    vi.mocked(saveMappingConfiguration).mockImplementation(async (next) => structuredClone(next));
    vi.mocked(reorderApplicationAssociations).mockResolvedValue(structuredClone(base));
  });

  it("shows only saved associations until the user opens Add Program", async () => {
    const wrapper = mountPanel({
      ...structuredClone(base), applicationBindings: [{ applicationId: "codex", templateId: "buttons", menuOrder: 0, launchTarget: null }],
    });
    expect(wrapper.text()).toContain("Codex");
    expect(wrapper.text()).not.toContain("Edge 浏览器");
    expect(listRunningApps).not.toHaveBeenCalled();
    expect(listPresetApps).not.toHaveBeenCalled();
    expect(wrapper.find(".association-table-header").text()).toContain("软件名称");
    expect(wrapper.find(".association-table-header").text()).toContain("选择的模板");

    await button(wrapper, "添加程序").trigger("click");
    await flushPromises();
    expect(listRunningApps).toHaveBeenCalledOnce();
    expect(listPresetApps).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("Edge 浏览器");
    expect(wrapper.findAll(".choice-card").map((item) => item.text()).join(" ")).not.toContain("Codex");
    await button(wrapper, "取消").trigger("click");
    expect(upsertApplicationBinding).not.toHaveBeenCalled();
    expect(upsertApplicationBinding).not.toHaveBeenCalled();
  });

  it("shows an empty list and add entry without discovering programs", () => {
    const empty = { ...structuredClone(base), applicationBindings: [] };
    const wrapper = mountPanel(empty);
    expect(wrapper.text()).toContain("尚未添加程序关联");
    expect(wrapper.find(".association-table").exists()).toBe(false);
    expect(button(wrapper, "添加程序")).toBeDefined();
    expect(listRunningApps).not.toHaveBeenCalled();
  });

  it("persists only after Program → Template → Confirm and does not enable follow", async () => {
    const wrapper = mountPanel();
    await button(wrapper, "添加程序").trigger("click");
    await flushPromises();
    await wrapper.findAll(".choice-card").find((item) => item.text().includes("Edge 浏览器"))!.trigger("click");
    await button(wrapper, "下一步").trigger("click");
    await wrapper.findAll(".choice-card").find((item) => item.text().includes("工作按键"))!.trigger("click");
    await button(wrapper, "下一步").trigger("click");
    expect(upsertApplicationBinding).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("不会自动开启");
    await button(wrapper, "确认添加").trigger("click");
    await flushPromises();
    expect(upsertApplicationBinding).toHaveBeenCalledWith({ applicationId: "edge", templateId: "buttons", menuOrder: 1, launchTarget: null });
  });

  it("keeps selections when going back and blocks an identical duplicate", async () => {
    const duplicate = {
      ...structuredClone(base),
      applicationBindings: [{ applicationId: "edge", templateId: "buttons", menuOrder: 0, launchTarget: null }],
    };
    const wrapper = mountPanel(duplicate);
    await button(wrapper, "添加程序").trigger("click");
    await flushPromises();
    await wrapper.findAll(".choice-card").find((item) => item.text().includes("Edge 浏览器"))!.trigger("click");
    await button(wrapper, "下一步").trigger("click");
    await wrapper.findAll(".choice-card").find((item) => item.text().includes("工作按键"))!.trigger("click");
    await button(wrapper, "下一步").trigger("click");
    await button(wrapper, "上一步").trigger("click");
    expect(wrapper.find(".choice-card.selected").text()).toContain("工作按键");
    await button(wrapper, "下一步").trigger("click");
    await button(wrapper, "确认更换").trigger("click");
    expect(wrapper.text()).toContain("无需重复添加");
    expect(upsertApplicationBinding).not.toHaveBeenCalled();
  });

  it("replaces the other template contract only after final confirmation", async () => {
    vi.mocked(listRunningApps).mockResolvedValueOnce([{ applicationId: "codex", name: "Codex", preset: true }]);
    const wrapper = mountPanel();
    expect(wrapper.find(".association-table").exists()).toBe(true);
    await button(wrapper, "添加程序").trigger("click");
    await flushPromises();
    await wrapper.findAll(".choice-card").find((item) => item.text().includes("Codex"))!.trigger("click");
    await button(wrapper, "下一步").trigger("click");
    expect(wrapper.text()).toContain("Codex 模板");
    await wrapper.findAll(".choice-card").find((item) => item.text().includes("工作按键"))!.trigger("click");
    await button(wrapper, "下一步").trigger("click");
    expect(wrapper.text()).toContain("原模板对象不会删除");
    await button(wrapper, "确认更换").trigger("click");
    await flushPromises();
    expect(upsertApplicationBinding).toHaveBeenCalledWith({ applicationId: "codex", templateId: "buttons", menuOrder: 0, launchTarget: null });
  });

  it("reports discovery and save failures without losing the active dialog", async () => {
    vi.mocked(listRunningApps).mockRejectedValueOnce(new Error("窗口枚举失败"));
    const wrapper = mountPanel();
    await button(wrapper, "添加程序").trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("窗口枚举失败");
    expect(button(wrapper, "重试")).toBeDefined();

    vi.mocked(listRunningApps).mockResolvedValueOnce([{ applicationId: "edge", name: "Edge 浏览器", preset: true }]);
    await button(wrapper, "重试").trigger("click");
    await flushPromises();
    await wrapper.findAll(".choice-card").find((item) => item.text().includes("Edge 浏览器"))!.trigger("click");
    await button(wrapper, "下一步").trigger("click");
    await wrapper.findAll(".choice-card").find((item) => item.text().includes("工作按键"))!.trigger("click");
    await button(wrapper, "下一步").trigger("click");
    vi.mocked(upsertApplicationBinding).mockRejectedValueOnce(new Error("保存失败"));
    await button(wrapper, "确认添加").trigger("click");
    await flushPromises();
    expect(wrapper.find("[role='dialog']").exists()).toBe(true);
    expect(wrapper.text()).toContain("保存失败");
  });

  it("shows semantic bindings and keeps their existing configuration untouched", async () => {
    const ordered = {
      ...structuredClone(base),
      templates: [
        ...structuredClone(base.templates),
        { id: "chat", name: "聊天", mappings: { enabled:true, actions:{} } },
      ],
      applicationBindings: [
        { applicationId: "codex", templateId: "scene", menuOrder: 0, launchTarget: null },
        { applicationId: "wechat", templateId: "chat", menuOrder: 1, launchTarget: null },
      ],
    };
    const wrapper = mountPanel(ordered);
    await flushPromises();
    expect(wrapper.findAll(".association-row")).toHaveLength(2);
    expect(wrapper.text()).toContain("Codex 模板");
    expect(ordered.applicationBindings.map((item) => item.applicationId)).toEqual(["codex", "wechat"]);
    expect(saveMappingConfiguration).not.toHaveBeenCalled();
    expect(removeApplicationBinding).not.toHaveBeenCalled();
  });

  it("uses the same four action slots for direct and semantic associations", () => {
    const wrapper = mountPanel({
      ...structuredClone(base),
      applicationBindings: [{ applicationId: "code", templateId: "buttons", menuOrder: 0, launchTarget:null }, { applicationId: "codex", templateId: "scene", menuOrder: 1, launchTarget: null }],
    });
    const rows = wrapper.findAll(".association-row");
    expect(rows).toHaveLength(2);
    for (const row of rows) {
      expect(row.findAll(".association-actions button")).toHaveLength(2);
      expect(row.text()).toContain("更换模板");
    }
  });

  it("reorders across kinds on a row drop and does not save an unchanged drop", async () => {
    const configuration = { ...structuredClone(base), applicationBindings: [{ applicationId: "direct", templateId: "buttons", menuOrder: 0, launchTarget:null }, { applicationId: "semantic", templateId: "scene", menuOrder: 1, launchTarget: null }] };
    const wrapper = mountPanel(configuration); const rows = wrapper.findAll(".association-row");
    await rows[1]!.trigger("dragstart", { dataTransfer: { setData: () => {}, effectAllowed: "" } });
    await rows[0]!.trigger("drop"); await flushPromises();
    expect(reorderApplicationAssociations).toHaveBeenCalledWith(["semantic", "direct"]);
    vi.mocked(reorderApplicationAssociations).mockClear();
    await rows[0]!.trigger("dragstart", { dataTransfer: { setData: () => {}, effectAllowed: "" } });
    await rows[0]!.trigger("drop"); await flushPromises();
    expect(reorderApplicationAssociations).not.toHaveBeenCalled();
  });

  it("rejects drags from action buttons including a row-targeted native dragstart", async () => {
    const wrapper = mountPanel({ ...structuredClone(base), applicationBindings: [...structuredClone(base.applicationBindings), { applicationId: "edge", templateId: "buttons", menuOrder: 1, launchTarget:null }] });
    const rows = wrapper.findAll(".association-row");
    const action = rows[1]!.find("button");
    await action.trigger("dragstart");
    await rows[0]!.trigger("drop");
    await action.trigger("pointerdown");
    await rows[1]!.trigger("dragstart");
    await rows[0]!.trigger("drop");
    expect(reorderApplicationAssociations).not.toHaveBeenCalled();
    expect(wrapper.emitted("busyChange")).toBeUndefined();
  });

  it("keeps the old mixed order on failure and rejects another operation while saving", async () => {
    let rejectSave!: (error: Error) => void;
    vi.mocked(reorderApplicationAssociations).mockReturnValueOnce(new Promise((_, reject) => { rejectSave = reject; }));
    const wrapper = mountPanel({ ...structuredClone(base), applicationBindings: [...structuredClone(base.applicationBindings), { applicationId: "edge", templateId: "buttons", menuOrder: 1, launchTarget:null }] });
    const rows = wrapper.findAll(".association-row");
    const before = rows.map((row) => row.find("strong").text());
    await rows[1]!.trigger("dragstart");
    await rows[0]!.trigger("drop");
    expect(wrapper.emitted("busyChange")).toEqual([[true]]);
    await rows[0]!.trigger("dragstart");
    await rows[1]!.trigger("drop");
    await rows[0]!.findAll("button")[1]!.trigger("click");
    expect(reorderApplicationAssociations).toHaveBeenCalledOnce();
    expect(removeApplicationBinding).not.toHaveBeenCalled();
    rejectSave(new Error("排序保存失败"));
    await flushPromises();
    expect(wrapper.findAll(".association-row").map((row) => row.find("strong").text())).toEqual(before);
    expect(wrapper.emitted("saved")).toBeUndefined();
    expect(wrapper.emitted("busyChange")).toEqual([[true], [false]]);
    expect(wrapper.text()).toContain("排序保存失败");
  });

  it("cancels an in-flight drag when the parent starts refreshing", async () => {
    const wrapper = mountPanel({ ...structuredClone(base), applicationBindings: [...structuredClone(base.applicationBindings), { applicationId: "edge", templateId: "buttons", menuOrder: 1, launchTarget:null }] });
    const rows = wrapper.findAll(".association-row");
    await rows[1]!.trigger("dragstart");
    await wrapper.setProps({ disabled: true });
    await rows[0]!.trigger("drop");
    await wrapper.setProps({ disabled: false });
    await rows[0]!.trigger("drop");
    expect(reorderApplicationAssociations).not.toHaveBeenCalled();
  });
});
