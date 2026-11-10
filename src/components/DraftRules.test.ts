// @vitest-environment jsdom

import { mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { draftRulesStorageKey } from "../lib/draft-rules";
import { effectiveLanguage } from "../lib/language-state";
import DraftRules from "./DraftRules.vue";

describe("draft rules editor", () => {
  beforeEach(() => {
    localStorage.clear();
    effectiveLanguage.value = "zh-Hans";
  });

  afterEach(() => vi.restoreAllMocks());

  it("saves just the rule, applies explicitly, and restores the original draft", async () => {
    const original = "请展开示例。临时草稿";
    const replacement = "const sample = '$&';\n\t// 示例";
    const wrapper = mount(DraftRules, { props: { modelValue: original } });
    await wrapper.get("[data-draft-rule-add]").trigger("click");
    await wrapper.get("[data-draft-rule-kind]").setValue("snippet");
    await wrapper.get("[data-draft-rule-trigger]").setValue("展开示例");
    await wrapper.get("[data-draft-rule-replacement]").setValue(replacement);
    await wrapper.get("[data-draft-rule-save]").trigger("click");
    expect(wrapper.emitted("update:modelValue")).toBeUndefined();
    const saved = localStorage.getItem(draftRulesStorageKey);
    expect(JSON.parse(saved!)).toEqual([{ kind: "snippet", trigger: "展开示例", replacement }]);
    expect(saved).not.toContain("临时草稿");

    const applied = `请${replacement}。临时草稿`;
    await wrapper.get("[data-draft-rule-apply]").trigger("click");
    expect(wrapper.emitted("update:modelValue")).toEqual([[applied]]);
    await wrapper.setProps({ modelValue: applied });
    await wrapper.get("[data-draft-rule-undo]").trigger("click");
    expect(wrapper.emitted("update:modelValue")).toEqual([[applied], [original]]);
  });

  it.each([false, true])("drops undo after a newer draft (same text and new revision: %s)", async (sameText) => {
    localStorage.setItem(draftRulesStorageKey, JSON.stringify([{ kind: "term", trigger: "鲁米", replacement: "露米" }]));
    const wrapper = mount(DraftRules, { props: { modelValue: "鲁米", draftRevision: 1 } });
    await wrapper.get("[data-draft-rule-apply]").trigger("click");
    await wrapper.setProps({ modelValue: "露米" });
    expect(wrapper.find("[data-draft-rule-undo]").exists()).toBe(true);
    await wrapper.setProps(sameText ? { draftRevision: 2 } : { modelValue: "手动编辑" });
    expect(wrapper.find("[data-draft-rule-undo]").exists()).toBe(false);
  });

  it("warns about corrupt data and leaves saved data alone when saving fails", async () => {
    localStorage.setItem(draftRulesStorageKey, "{broken");
    const wrapper = mount(DraftRules, { props: { modelValue: "鲁米" } });
    expect(wrapper.get("[role=alert]").text()).toContain("本地规则失败");
    expect(wrapper.get("[data-draft-rule-apply]").attributes("disabled")).toBeDefined();
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("unavailable"); });
    await wrapper.get("[data-draft-rule-add]").trigger("click");
    await wrapper.get("[data-draft-rule-trigger]").setValue("鲁米");
    await wrapper.get("[data-draft-rule-replacement]").setValue("露米");
    await wrapper.get("[data-draft-rule-save]").trigger("click");
    expect(wrapper.get("[role=alert]").text()).toContain("本地规则失败");
    expect(localStorage.getItem(draftRulesStorageKey)).toBe("{broken");
    expect(wrapper.get("[data-draft-rule-apply]").attributes("disabled")).toBeDefined();
  });

  it("edits and removes saved rules without changing the draft", async () => {
    localStorage.setItem(draftRulesStorageKey, JSON.stringify([{ kind: "term", trigger: "鲁米", replacement: "露米" }]));
    const wrapper = mount(DraftRules, { props: { modelValue: "鲁米", disabled: true } });
    expect(wrapper.get("[data-draft-rule-edit]").attributes("disabled")).toBeDefined();
    expect(wrapper.get("[data-draft-rule-apply]").attributes("disabled")).toBeDefined();
    await wrapper.setProps({ disabled: false });
    await wrapper.get("[data-draft-rule-edit]").trigger("click");
    await wrapper.get("[data-draft-rule-replacement]").setValue("Lumi");
    await wrapper.get("[data-draft-rule-save]").trigger("click");
    expect(JSON.parse(localStorage.getItem(draftRulesStorageKey)!)).toEqual([
      { kind: "term", trigger: "鲁米", replacement: "Lumi" },
    ]);
    await wrapper.get("[data-draft-rule-delete]").trigger("click");
    expect(localStorage.getItem(draftRulesStorageKey)).toBe("[]");
    expect(wrapper.get("[data-draft-rule-apply]").attributes("disabled")).toBeDefined();
    expect(wrapper.emitted("update:modelValue")).toBeUndefined();
  });
});
