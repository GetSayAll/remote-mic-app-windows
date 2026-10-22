// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import MacrosPage from "./MacrosPage.vue";
import { clearMacroEditorSession } from "./macro-editor-session";
import type { MacroDefinition, MacroLibraryDocument } from "../lib/bridge";

type CaptureHandler = (edge: { key: string; isPressed: boolean; source?: string }) => void;
let shortcutCaptureHandler: CaptureHandler | null = null;

const baseDefinition: MacroDefinition = {
  schemaVersion: "0.1",
  macroId: "macro.aaaaaaa1",
  version: "1.0.0",
  name: "打开记事本并聚焦",
  steps: [
    { stepId: "step.01", action: "open_application", parameters: { target: "notepad" } },
    { stepId: "step.02", action: "focus_input", parameters: { target: "notepad" } },
  ],
};

const initialLibrary: MacroLibraryDocument = {
  formatVersion: 1,
  definitions: [baseDefinition],
};

const saveMacroSpy = vi.hoisted(() => vi.fn());
const deleteMacroSpy = vi.hoisted(() => vi.fn());
const testMacroDefinitionSpy = vi.hoisted(() => vi.fn());
const testMacroStepSpy = vi.hoisted(() => vi.fn());
const testMacroSpy = vi.hoisted(() => vi.fn());
const getMacroRunStatusSpy = vi.hoisted(() => vi.fn());
const learnFocusTargetSpy = vi.hoisted(() => vi.fn());
const learnFocusPointSpy = vi.hoisted(() => vi.fn());
const getButtonMappingsSpy = vi.hoisted(() => vi.fn());
const pickCustomAppSpy = vi.hoisted(() => vi.fn());
const saveButtonMappingsSpy = vi.hoisted(() => vi.fn());
const startShortcutCaptureSpy = vi.hoisted(() => vi.fn());
const stopShortcutCaptureSpy = vi.hoisted(() => vi.fn());
const exportSpy = vi.hoisted(() => vi.fn());
const importSpy = vi.hoisted(() => vi.fn());
const getCombinedActionsSpy = vi.hoisted(() => vi.fn());
const macroReferencesSpy = vi.hoisted(() => vi.fn());

vi.mock("../lib/bridge", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/bridge")>();
  return {
    ...actual,
    getCombinedActions: getCombinedActionsSpy,
    macroReferences: macroReferencesSpy,
    saveMacro: saveMacroSpy,
    deleteMacro: deleteMacroSpy,
    exportCombinedActions: exportSpy,
    importCombinedActions: importSpy,
    testMacro: testMacroSpy,
    testMacroDefinition: testMacroDefinitionSpy,
    testMacroStep: testMacroStepSpy,
    getMacroRunStatus: getMacroRunStatusSpy,
    cancelMacro: vi.fn(async () => true),
    learnFocusTarget: learnFocusTargetSpy,
    learnFocusPoint: learnFocusPointSpy,
    getButtonMappings: getButtonMappingsSpy,
    pickCustomApp: pickCustomAppSpy,
    scanRegisteredApps: vi.fn(async () => []),
    saveButtonMappings: saveButtonMappingsSpy,
    startShortcutCapture: startShortcutCaptureSpy,
    stopShortcutCapture: stopShortcutCaptureSpy,
    subscribeShortcutCaptureEdges: vi.fn(async (handler: CaptureHandler) => {
      shortcutCaptureHandler = handler;
      return () => {};
    }),
    listPresetApps: vi.fn(async () => [
      { id: "notepad", name: "记事本", installed: true },
      { id: "sayall", name: "无线麦", installed: true },
    ]),
  };
});

async function mountPage(): Promise<VueWrapper<InstanceType<typeof MacrosPage>>> {
  const wrapper = mount(MacrosPage, {
    props: { runtime: null },
  });
  await flushPromises();
  return wrapper;
}

function findButton(wrapper: VueWrapper, text: string) {
  const button = wrapper.findAll("button").find((item) => item.text().includes(text));
  expect(button, `按钮「${text}」应存在`).toBeTruthy();
  return button!;
}

beforeEach(() => {
  shortcutCaptureHandler = null;
  // 编辑会话是模块级的（跨页面切换保留进度）；用例之间必须清空，否则上一条的草稿会在下一条挂载时恢复。
  clearMacroEditorSession();
  getCombinedActionsSpy.mockReset().mockResolvedValue(structuredClone(initialLibrary));
  saveMacroSpy.mockReset().mockImplementation(async (definition: MacroDefinition) => ({
    formatVersion: 1,
    definitions: [definition],
  }));
  deleteMacroSpy.mockReset().mockResolvedValue({
    library: { formatVersion: 1, definitions: [] },
    removedMacroIds: ["macro.aaaaaaa1"],
    rewrittenMacroIds: [],
  });
  macroReferencesSpy.mockReset().mockResolvedValue({ buttons: [], nestedIn: [] });
  testMacroDefinitionSpy.mockReset().mockResolvedValue("test.00000000000000000000000000000001");
  testMacroStepSpy.mockReset().mockResolvedValue("test.00000000000000000000000000000002");
  testMacroSpy.mockReset().mockResolvedValue(undefined);
  getMacroRunStatusSpy.mockReset().mockResolvedValue({ running: false, report: null });
  learnFocusTargetSpy.mockReset().mockResolvedValue({
    controlType: "Edit",
    automationId: "chat-input",
    className: "RichEdit",
  });
  learnFocusPointSpy.mockReset().mockResolvedValue({
    xRatio: 0.5,
    yRatio: 0.82,
    windowWidth: 1200,
    windowHeight: 800,
  });
  getButtonMappingsSpy.mockReset().mockResolvedValue({ enabled: true, actions: {} });
  pickCustomAppSpy.mockReset().mockResolvedValue(null);
  saveButtonMappingsSpy.mockReset().mockImplementation(async (mappings: unknown) => mappings);
  startShortcutCaptureSpy.mockReset().mockResolvedValue([]);
  stopShortcutCaptureSpy.mockReset().mockResolvedValue(null);
  exportSpy.mockReset().mockResolvedValue(true);
  importSpy.mockReset().mockResolvedValue(null);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("组合动作页 · 库浏览", () => {
  it("加载后渲染动作列表与只读步骤（名称 + N 个步骤）", async () => {
    const wrapper = await mountPage();
    const items = wrapper.findAll(".macro-item");
    expect(items).toHaveLength(1);
    expect(items[0].text()).toContain("打开记事本并聚焦");
    expect(items[0].text()).toContain("2 个步骤");
    expect(wrapper.findAll(".step-item")).toHaveLength(2);
    expect(wrapper.text()).toContain("聚焦学习过的输入框");
  });

  it("空库显示空态与创建引导（对齐 Mac 文案）", async () => {
    getCombinedActionsSpy.mockResolvedValue({ formatVersion: 1, definitions: [] });
    const wrapper = await mountPage();
    expect(wrapper.text()).toContain("创建第一个组合动作");
    expect(wrapper.text()).toContain("把打开 App、按键或输入框聚焦组合起来，再设到遥控器按键上。");
  });

  it("同一条动作多版本只显示一条（2026-10-15 现场：编辑后变两个）", async () => {
    // 现场形态：编辑一次后文件里同时有 1.0.0 与 1.0.1 两个版本。
    getCombinedActionsSpy.mockResolvedValue({
      formatVersion: 1,
      definitions: [
        baseDefinition,
        { ...baseDefinition, version: "1.0.1", name: "改名后的动作" },
      ],
    });
    const wrapper = await mountPage();
    const items = wrapper.findAll(".macro-item");
    expect(items).toHaveLength(1);
    expect(items[0].text()).toContain("改名后的动作");
  });

  it("保存返回多版本时列表仍只显示一条（保存后不出现重复）", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "测试与保存").trigger("click");
    await flushPromises();
    // 后端按不可变版本语义把新版本追加在旧版本之后。
    saveMacroSpy.mockResolvedValue({
      formatVersion: 1,
      definitions: [
        baseDefinition,
        { ...baseDefinition, version: "1.0.1", name: "打开记事本并聚焦" },
      ],
    });
    await findButton(wrapper, "保存修改").trigger("click");
    await flushPromises();
    expect(wrapper.findAll(".macro-item")).toHaveLength(1);
  });

  it("删除确认列出受影响的按键与嵌套引用；确认后调 delete_macro", async () => {
    macroReferencesSpy.mockResolvedValue({
      buttons: [
        { button: "ok", trigger: "single" },
        { button: "home", trigger: "long" },
      ],
      nestedIn: ["父动作"],
    });
    const wrapper = await mountPage();
    await findButton(wrapper, "删除").trigger("click");
    await flushPromises();
    expect(macroReferencesSpy).toHaveBeenCalledWith("macro.aaaaaaa1");
    // 按键会改成未设置、嵌套引用会显示不可用——两者都要在确认前说清楚。
    expect(wrapper.text()).toContain("这些按键会改为");
    expect(wrapper.text()).toContain("确定·单击、主页·长按");
    expect(wrapper.text()).toContain("这些组合动作嵌套引用了它");
    expect(wrapper.text()).toContain("父动作");
    const confirm = wrapper
      .findAll(".delete-confirm button")
      .find((item) => item.text() === "删除")!;
    await confirm.trigger("click");
    await flushPromises();
    expect(deleteMacroSpy).toHaveBeenCalledWith("macro.aaaaaaa1");
    expect(wrapper.text()).toContain("2 个引用它的按键已改为未设置");
  });

  it("没有引用时确认弹窗明说没有影响", async () => {
    macroReferencesSpy.mockResolvedValue({ buttons: [], nestedIn: [] });
    const wrapper = await mountPage();
    await findButton(wrapper, "删除").trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("没有按键或组合动作引用它");
  });

  it("测试动作按最新版本执行并轮询终态文案", async () => {
    getMacroRunStatusSpy.mockResolvedValue({
      running: false,
      report: {
        macroId: "macro.aaaaaaa1",
        result: "completed_with_unknown_result",
        elapsedMs: 120,
        steps: [],
      },
    });
    const wrapper = await mountPage();
    await findButton(wrapper, "测试动作").trigger("click");
    await flushPromises();
    expect(testMacroSpy).toHaveBeenCalledWith("macro.aaaaaaa1");
    // 轮询一次后状态行显示终态文案（不谎报成功）。
    await new Promise((resolve) => setTimeout(resolve, 500));
    await flushPromises();
    expect(wrapper.text()).toContain("组合动作已执行，但无法确认第三方 App 的结果");
  });
});

describe("组合动作页 · 编辑向导", () => {
  it("新建：名称 20 字硬截；基本设置里选动作是替换不是累加，且不越过名称门禁", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "新建").trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("新建组合动作");
    // 新建不再先报“已创建”（保存前不写库），也不显示已删除的长提示。
    expect(wrapper.text()).not.toContain("组合动作已创建");
    expect(wrapper.text()).not.toContain("只负责把光标");

    // 新建即带默认名称（对齐 Mac 的“先有名字再编辑”），不再卡在空名称。
    const nameInput = wrapper.get('.basic-body input[type="text"]');
    expect((nameInput.element as HTMLInputElement).value).toBe("组合动作 1");

    await nameInput.setValue("一二三四五六七八九十一二三四五六七八九十二十一");
    expect((nameInput.element as HTMLInputElement).value).toHaveLength(20);

    // 名称已填：选动作停留本步（不自动跳第②步，2026-10-15 门禁修复）。
    await findButton(wrapper, "打开应用").trigger("click");
    await flushPromises();
    expect(wrapper.find(".basic-body").exists()).toBe(true);
    expect(wrapper.text()).toContain("已有 1 个步骤");
    // 选中反馈：动作卡高亮并带勾，避免“点了像没反应”（2026-10-19 现场）。
    const selected = wrapper.find(".action-card.selected");
    expect(selected.exists()).toBe(true);
    expect(selected.text()).toContain("打开应用");
    expect(selected.text()).toContain("✓");

    await findButton(wrapper, "下一步").trigger("click");
    await flushPromises();
    expect(wrapper.find(".steps-body").exists()).toBe(true);
  });

  it("编排步骤的应用选择与按键页一致：chip 网格，点选立即写入步骤", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    const body = wrapper.find(".step-card-body");
    expect(body.find("select").exists()).toBe(false);
    const chips = body.findAll(".preset-grid .chip");
    expect(chips.map((chip) => chip.text())).toContain("记事本");
    const sayallChip = chips.find((chip) => chip.text() === "无线麦");
    expect(sayallChip, "已安装预设应用应出现在 chip 网格").toBeTruthy();
    await sayallChip!.trigger("click");
    await flushPromises();
    expect(body.find(".chip.selected").text()).toBe("无线麦");
    expect(body.text()).toContain("当前：无线麦");
  });

  it("名称为空时不给进入下一步（点按钮与点步骤都拦下，2026-10-15 现场）", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "新建").trigger("click");
    await flushPromises();
    // 新建默认带名称，这里手动清空以验证门禁。
    await wrapper.get('.basic-body input[type="text"]').setValue("");
    // 先选一个动作（此时名称为空）。
    await findButton(wrapper, "打开应用").trigger("click");
    await flushPromises();
    // 「下一步」禁用。
    const next = findButton(wrapper, "下一步");
    expect(next.attributes("disabled")).toBeDefined();
    // 点步骤 pill 也被拦下并给出原因。
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    expect(wrapper.find(".basic-body").exists()).toBe(true);
    expect(wrapper.text()).toContain("先填写组合动作名称");
  });

  it("测试启动失败显示中文原因而不是机器码（invalid:name_empty）", async () => {
    testMacroDefinitionSpy.mockRejectedValueOnce(new Error("invalid:name_empty"));
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "测试与保存").trigger("click");
    await flushPromises();
    await findButton(wrapper, "▶ 测试动作").trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("测试失败：请先填写组合动作名称");
    expect(wrapper.text()).not.toContain("invalid:name_empty");
  });

  it("编辑已存动作：保存走新版本号（patch +1）", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    // 直接进入第③步保存。
    await findButton(wrapper, "测试与保存").trigger("click");
    await flushPromises();
    await findButton(wrapper, "保存修改").trigger("click");
    await flushPromises();
    expect(saveMacroSpy).toHaveBeenCalledTimes(1);
    const saved = saveMacroSpy.mock.calls[0][0] as MacroDefinition;
    expect(saved.version).toBe("1.0.1");
    expect(saved.macroId).toBe("macro.aaaaaaa1");
  });

  it("新建动作保存时版本从 1.0.0 起", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "新建").trigger("click");
    await flushPromises();
    await wrapper.get('.basic-body input[type="text"]').setValue("新动作");
    await findButton(wrapper, "打开应用").trigger("click");
    await flushPromises();
    // 基本设置 →（下一步）编排步骤 →（下一步）测试与保存。
    await findButton(wrapper, "下一步").trigger("click");
    await flushPromises();
    await findButton(wrapper, "下一步").trigger("click");
    await flushPromises();
    await findButton(wrapper, "保存修改").trigger("click");
    await flushPromises();
    const saved = saveMacroSpy.mock.calls[0][0] as MacroDefinition;
    expect(saved.version).toBe("1.0.0");
    expect(saved.name).toBe("新动作");
  });

  it("快捷键录入：边沿累积，全部松开后写入组合键并显示", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    // 编辑从第①步开始：先进入「编排步骤」。
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    const shortcutCard = wrapper
      .findAll(".action-card")
      .find((item) => item.text().includes("单键或快捷键"))!;
    await shortcutCard.trigger("click");
    await flushPromises();

    await findButton(wrapper, "开始录入").trigger("click");
    await flushPromises();
    expect(startShortcutCaptureSpy).toHaveBeenCalled();

    shortcutCaptureHandler?.({ key: "left_control", isPressed: true });
    shortcutCaptureHandler?.({ key: "left_alt", isPressed: true });
    shortcutCaptureHandler?.({ key: "n", isPressed: true });
    shortcutCaptureHandler?.({ key: "n", isPressed: false });
    shortcutCaptureHandler?.({ key: "left_alt", isPressed: false });
    shortcutCaptureHandler?.({ key: "left_control", isPressed: false });
    await flushPromises();

    expect(wrapper.get('[data-testid="chord-display"]').text()).toContain("Ctrl + Alt + N");
    expect(wrapper.text()).toContain("快捷键录入成功");
  });

  it("Esc 取消录入且不写入", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    const shortcutCard = wrapper
      .findAll(".action-card")
      .find((item) => item.text().includes("单键或快捷键"))!;
    await shortcutCard.trigger("click");
    await flushPromises();
    await findButton(wrapper, "开始录入").trigger("click");
    await flushPromises();
    shortcutCaptureHandler?.({ key: "escape", isPressed: true });
    await flushPromises();
    expect(wrapper.text()).toContain("已取消录入");
    expect(wrapper.get('[data-testid="chord-display"]').text()).toContain("尚未录入按键");
  });

  it("录入按钮点击后立即显示录入态，再次点击可取消", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    const shortcutCard = wrapper
      .findAll(".action-card")
      .find((item) => item.text().includes("单键或快捷键"))!;
    await shortcutCard.trigger("click");
    await flushPromises();

    await findButton(wrapper, "开始录入").trigger("click");
    await flushPromises();
    expect(wrapper.get('[data-testid="combo-capture-button"]').text()).toContain("录入中");
    expect(wrapper.get('[data-testid="combo-capture-hint"]').text()).toContain("请按一次单键或组合键");

    await wrapper.get('[data-testid="combo-capture-button"]').trigger("click");
    await flushPromises();
    expect(stopShortcutCaptureSpy).toHaveBeenCalled();
    expect(wrapper.text()).toContain("已取消录入");
    expect(wrapper.get('[data-testid="combo-capture-button"]').text()).toContain("开始录入");
  });

  it("支持录入 Ctrl+[ 并保存为组合动作的按键", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    const shortcutCard = wrapper
      .findAll(".action-card")
      .find((item) => item.text().includes("单键或快捷键"))!;
    await shortcutCard.trigger("click");
    await flushPromises();

    await findButton(wrapper, "开始录入").trigger("click");
    await flushPromises();
    shortcutCaptureHandler?.({ key: "left_control", isPressed: true });
    shortcutCaptureHandler?.({ key: "oem4", isPressed: true });
    shortcutCaptureHandler?.({ key: "oem4", isPressed: false });
    shortcutCaptureHandler?.({ key: "left_control", isPressed: false });
    await flushPromises();

    expect(wrapper.get('[data-testid="chord-display"]').text()).toContain("Ctrl + [");
  });

  it("支持录入 Ctrl+] 并保存为组合动作的按键", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    const shortcutCard = wrapper
      .findAll(".action-card")
      .find((item) => item.text().includes("单键或快捷键"))!;
    await shortcutCard.trigger("click");
    await flushPromises();

    await findButton(wrapper, "开始录入").trigger("click");
    await flushPromises();
    shortcutCaptureHandler?.({ key: "left_control", isPressed: true });
    shortcutCaptureHandler?.({ key: "oem6", isPressed: true });
    shortcutCaptureHandler?.({ key: "oem6", isPressed: false });
    shortcutCaptureHandler?.({ key: "left_control", isPressed: false });
    await flushPromises();

    expect(wrapper.get('[data-testid="chord-display"]').text()).toContain("Ctrl + ]");
  });

  it("支持录入 Ctrl+小键盘 + 并显示为小键盘按键", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    const shortcutCard = wrapper
      .findAll(".action-card")
      .find((item) => item.text().includes("单键或快捷键"))!;
    await shortcutCard.trigger("click");
    await flushPromises();

    await findButton(wrapper, "开始录入").trigger("click");
    await flushPromises();
    shortcutCaptureHandler?.({ key: "left_control", isPressed: true });
    shortcutCaptureHandler?.({ key: "numpad_add", isPressed: true });
    shortcutCaptureHandler?.({ key: "numpad_add", isPressed: false });
    shortcutCaptureHandler?.({ key: "left_control", isPressed: false });
    await flushPromises();

    expect(wrapper.get('[data-testid="chord-display"]').text()).toContain("Ctrl + 小键盘 +");
  });

  it("只按修饰键时给出明确说明，而不是静默无反应", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    const shortcutCard = wrapper
      .findAll(".action-card")
      .find((item) => item.text().includes("单键或快捷键"))!;
    await shortcutCard.trigger("click");
    await flushPromises();

    await findButton(wrapper, "开始录入").trigger("click");
    await flushPromises();
    shortcutCaptureHandler?.({ key: "left_alt", isPressed: true });
    shortcutCaptureHandler?.({ key: "left_alt", isPressed: false });
    await flushPromises();

    expect(wrapper.text()).toContain("仅按住 Alt/Ctrl 等修饰键暂不支持");
  });

  it("常用快捷键里的 Enter 显示为 Enter，而不是 Return", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    const shortcutCard = wrapper
      .findAll(".action-card")
      .find((item) => item.text().includes("单键或快捷键"))!;
    await shortcutCard.trigger("click");
    await flushPromises();

    const enterChip = wrapper.findAll(".chip").find((item) => item.text() === "Enter")!;
    await enterChip.trigger("click");
    await flushPromises();
    const display = wrapper.get('[data-testid="chord-display"]').text();
    expect(display).toContain("Enter");
    expect(display).not.toMatch(/return/i);
  });

  it("离开组合动作页时停止仍在进行的录入", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    const shortcutCard = wrapper
      .findAll(".action-card")
      .find((item) => item.text().includes("单键或快捷键"))!;
    await shortcutCard.trigger("click");
    await flushPromises();
    await findButton(wrapper, "开始录入").trigger("click");
    await flushPromises();

    wrapper.unmount();
    await flushPromises();
    expect(stopShortcutCaptureSpy).toHaveBeenCalled();
  });

  it("「执行已有组合动作」候选排除当前动作自身", async () => {
    getCombinedActionsSpy.mockResolvedValue({
      formatVersion: 1,
      definitions: [
        baseDefinition,
        { ...baseDefinition, macroId: "macro.bbbbbbb2", name: "另一个动作", steps: [baseDefinition.steps[0]] },
      ],
    });
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    const nestedCard = wrapper
      .findAll(".action-card")
      .find((item) => item.text().includes("执行已有组合动作"))!;
    await nestedCard.trigger("click");
    await flushPromises();
    const chips = wrapper.findAll(".step-card-body .chip").map((item) => item.text());
    expect(chips).toContain("另一个动作");
    expect(chips).not.toContain("打开记事本并聚焦");
    expect(wrapper.find(".step-card-body select").exists()).toBe(false);
  });

  it("学习窗口为 6 秒：点按钮立即采样并显示倒计时", async () => {
    let resolveLearn: (value: unknown) => void = () => {};
    learnFocusTargetSpy.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveLearn = resolve;
      }),
    );
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    const focusItem = wrapper
      .findAll(".step-edit-item")
      .find((item) => item.text().includes("聚焦学习过的输入框"))!;
    await focusItem.trigger("click");
    await flushPromises();
    await findButton(wrapper, "学习输入框").trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("6 秒内点击目标输入框");
    expect(learnFocusTargetSpy).toHaveBeenCalled();
    resolveLearn({ controlType: "Edit", automationId: "chat-input", className: "RichEdit" });
    await flushPromises();
    expect(wrapper.text()).toContain("输入框学习成功");
  });

  it("学习输入框：写入该 App 的聚焦档案（与按键页同一落盘口径）", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    const focusItem = wrapper
      .findAll(".step-edit-item")
      .find((item) => item.text().includes("聚焦学习过的输入框"))!;
    await focusItem.trigger("click");
    await flushPromises();
    // 学习窗口 6 秒由后端执行：点按钮立即开始采样（不再等 10 秒倒计时）。
    await findButton(wrapper, "学习输入框").trigger("click");
    await flushPromises();
    expect(learnFocusTargetSpy).toHaveBeenCalled();
    // 关键回归（2026-10-15 现场）：学到的记录必须落进 focusProfiles，
    // 否则"聚焦学习过的输入框"步骤永远没有档案可用。
    expect(saveButtonMappingsSpy).toHaveBeenCalled();
    const saved = saveButtonMappingsSpy.mock.calls.at(-1)![0] as {
      focusProfiles: Record<string, { strategy: string }>;
    };
    expect(saved.focusProfiles.notepad?.strategy).toBe("recorded_element");
    expect(wrapper.text()).toContain("输入框学习成功");
  });

  it("学习失败显示中文原因（不再出现 SendInput failed / no_candidate 机器码）", async () => {
    vi.useFakeTimers();
    learnFocusTargetSpy.mockRejectedValueOnce(new Error("SendInput failed: no_candidate"));
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    const focusItem = wrapper
      .findAll(".step-edit-item")
      .find((item) => item.text().includes("聚焦学习过的输入框"))!;
    await focusItem.trigger("click");
    await flushPromises();
    await findButton(wrapper, "学习输入框").trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("没有采到可用的输入框");
    expect(wrapper.text()).not.toContain("no_candidate");
    expect(saveButtonMappingsSpy).not.toHaveBeenCalled();
  });

  it("记录点击位置：写入 recorded_point 档案（没有 UIA 树的应用走这条路）", async () => {
    vi.useFakeTimers();
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    const focusItem = wrapper
      .findAll(".step-edit-item")
      .find((item) => item.text().includes("聚焦学习过的输入框"))!;
    await focusItem.trigger("click");
    await flushPromises();
    await findButton(wrapper, "记录点击位置").trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("秒后自动记录鼠标位置");
    await vi.advanceTimersByTimeAsync(6_500);
    await flushPromises();
    vi.useRealTimers();
    expect(learnFocusPointSpy).toHaveBeenCalledWith("notepad");
    const saved = saveButtonMappingsSpy.mock.calls.at(-1)![0] as {
      focusProfiles: Record<string, { strategy: string; point?: { yRatio: number } }>;
    };
    expect(saved.focusProfiles.notepad?.strategy).toBe("recorded_point");
    expect(saved.focusProfiles.notepad?.point?.yRatio).toBeCloseTo(0.82);
    expect(wrapper.text()).toContain("点击位置记录成功");
  });

  it("记录位置失败时提示中文原因（光标不在目标窗口内）", async () => {
    vi.useFakeTimers();
    learnFocusPointSpy.mockRejectedValueOnce(new Error("SendInput failed: cursor_outside_target"));
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    const focusItem = wrapper
      .findAll(".step-edit-item")
      .find((item) => item.text().includes("聚焦学习过的输入框"))!;
    await focusItem.trigger("click");
    await flushPromises();
    await findButton(wrapper, "记录点击位置").trigger("click");
    await vi.advanceTimersByTimeAsync(6_500);
    await flushPromises();
    vi.useRealTimers();
    expect(wrapper.text()).toContain("鼠标不在目标应用的窗口里");
    expect(saveButtonMappingsSpy).not.toHaveBeenCalled();
  });

  it("已学习过的 App 显示「已学习」并提供「重新学习」（前置可见）", async () => {
    getButtonMappingsSpy.mockResolvedValue({
      enabled: true,
      actions: {},
      focusProfiles: {
        notepad: {
          strategy: "recorded_element",
          recorded: { controlType: "Edit", automationId: "editor", className: "RichEdit" },
        },
      },
    });
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    const focusItem = wrapper
      .findAll(".step-edit-item")
      .find((item) => item.text().includes("聚焦学习过的输入框"))!;
    await focusItem.trigger("click");
    await flushPromises();
    expect(wrapper.get('[data-testid="focus-state"]').text()).toContain("已学习");
    expect(findButton(wrapper, "重新学习")).toBeTruthy();
  });

  it("保存失败显示错误状态且不关闭编辑器", async () => {
    saveMacroSpy.mockRejectedValue(new Error("immutable_version_conflict"));
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "测试与保存").trigger("click");
    await flushPromises();
    await findButton(wrapper, "保存修改").trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("保存失败");
    expect(wrapper.find(".wizard").exists()).toBe(true);
  });

  it("导入取消时不改变现有列表", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "导入").trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("已取消导入");
    expect(wrapper.findAll(".macro-item")).toHaveLength(1);
  });

  it("切到其他页面再回来保留编辑进度（草稿跨挂载恢复）", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await wrapper.get('.basic-body input[type="text"]').setValue("未保存的名字");
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    expect(wrapper.find(".steps-body").exists()).toBe(true);

    // 模拟切页：组件卸载后重新挂载。
    wrapper.unmount();
    const restored = await mountPage();
    expect(restored.find(".wizard").exists()).toBe(true);
    expect(restored.find(".steps-body").exists()).toBe(true);
    expect(restored.text()).toContain("未保存的名字");
  });

  it("右侧显示当前动作的使用位置（按键与嵌套引用）", async () => {
    macroReferencesSpy.mockResolvedValue({
      buttons: [{ button: "ok", trigger: "single" }],
      nestedIn: ["父动作"],
    });
    const wrapper = await mountPage();
    await flushPromises();
    expect(wrapper.text()).toContain("使用位置");
    expect(wrapper.text()).toContain("按键：确定 · 单击");
    expect(wrapper.text()).toContain("嵌套于：父动作");
  });

  it("步骤未配置完整时「下一步」禁用并说明原因", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "新建").trigger("click");
    await flushPromises();
    await findButton(wrapper, "单键或快捷键").trigger("click");
    await flushPromises();
    await findButton(wrapper, "下一步").trigger("click");
    await flushPromises();
    expect(wrapper.find(".steps-body").exists()).toBe(true);
    const next = findButton(wrapper, "下一步");
    expect(next.attributes("disabled")).toBeDefined();
    expect(wrapper.text()).toContain("第 1 步还没设置完：还没有选择或录入按键");
    expect(wrapper.find(".steps-body").exists()).toBe(true);
  });

  it("「执行已有组合动作」用 chip 选择，不再使用下拉框", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "新建").trigger("click");
    await flushPromises();
    await findButton(wrapper, "执行已有组合动作").trigger("click");
    await flushPromises();
    await findButton(wrapper, "下一步").trigger("click");
    await flushPromises();
    expect(wrapper.find(".step-card-body select").exists()).toBe(false);
    const chip = wrapper.findAll(".step-card-body .chip").find((item) => item.text() === "打开记事本并聚焦");
    expect(chip).toBeTruthy();
    await chip!.trigger("click");
    await flushPromises();
    expect(wrapper.find(".step-card-body .chip.selected").text()).toBe("打开记事本并聚焦");
  });

  it("「扫描本机应用」在组合动作页打开本机应用弹窗", async () => {
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    const scan = wrapper.findAll("button").find((item) => item.text() === "扫描本机应用")!;
    await scan.trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("本机应用");
  });

  it("单键或快捷键列出已保存与常用的快捷键，点选即写入", async () => {
    getButtonMappingsSpy.mockResolvedValue({
      enabled: true,
      actions: {},
      focusProfiles: {
        notepad: { strategy: "app_shortcut", shortcut: { keys: ["control", "k"] } },
      },
    });
    const wrapper = await mountPage();
    await findButton(wrapper, "新建").trigger("click");
    await flushPromises();
    await findButton(wrapper, "单键或快捷键").trigger("click");
    await flushPromises();
    await findButton(wrapper, "下一步").trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("已保存的快捷键");
    expect(wrapper.text()).toContain("记事本 · Ctrl + K");
    const copy = wrapper.findAll(".chip").find((chip) => chip.text() === "复制")!;
    await copy.trigger("click");
    await flushPromises();
    expect(wrapper.get('[data-testid="chord-display"]').text()).toContain("Ctrl + C");
  });

  it("「添加应用」写入共享自定义应用列表并选中", async () => {
    pickCustomAppSpy.mockResolvedValue({ name: "自定义App", path: "C:\\Apps\\Custom.exe" });
    const wrapper = await mountPage();
    await findButton(wrapper, "编辑").trigger("click");
    await flushPromises();
    await findButton(wrapper, "编排步骤").trigger("click");
    await flushPromises();
    const add = wrapper.findAll("button").find((item) => item.text().includes("添加应用"))!;
    await add.trigger("click");
    await flushPromises();
    const saved = saveButtonMappingsSpy.mock.calls.at(-1)?.[0] as {
      applications?: Array<{ name: string; path: string }>;
    };
    expect(saved.applications).toEqual([{ name: "自定义App", path: "C:\\Apps\\Custom.exe" }]);
    expect(wrapper.find(".chip.selected").text()).toBe("自定义App");
  });
});
