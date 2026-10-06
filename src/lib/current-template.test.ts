import { defineComponent } from "vue";
import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getSceneSnapshot, selectCurrentTemplate, subscribeSceneEvents, type SceneEvent, type SceneSnapshot } from "./bridge";
import { useCurrentTemplate } from "./current-template";

vi.mock("./bridge", () => ({ getSceneSnapshot: vi.fn(), selectCurrentTemplate: vi.fn(), subscribeSceneEvents: vi.fn(), isTauriRuntime: () => false }));
const consumers = new Set<(event: SceneEvent) => void>();
const snapshot = (id: string | null, revision = 1): SceneSnapshot => ({ mappingNoticeEnabled: true, mappingNotice: { kind: id ? "template" : "common", templateId: id, name: id, actionsAvailable: true }, mappingNoticeRevision: revision, enabled: true, generation: revision, foregroundGeneration: 1, applicationId: "app", templateId: id, panel: null, updateDefault: false, selectedIndex: null, menuItems: [], waitingForRelease: false, voiceActive: false, status: null });
const Harness = defineComponent({ setup: () => ({ state: useCurrentTemplate() }), template: '<span>{{ state.applied.value ? state.templateId.value ?? "common" : "unknown" }}</span>' });

describe("applied template state", () => {
  beforeEach(() => {
    vi.clearAllMocks(); consumers.clear();
    vi.mocked(getSceneSnapshot).mockResolvedValue(snapshot("first"));
    vi.mocked(subscribeSceneEvents).mockImplementation(async callback => { consumers.add(callback); return () => consumers.delete(callback); });
  });

  it("keeps mounted consumers synchronized and rereads the applied value after page reopening", async () => {
    const first = mount(Harness), second = mount(Harness); await flushPromises();
    const changed = snapshot("second", 2);
    for (const callback of consumers) callback({ type: "mapping_applied", notice: changed.mappingNotice!, revision: 2 });
    await flushPromises();
    expect(first.text()).toBe("second"); expect(second.text()).toBe("second");
    first.unmount(); second.unmount(); expect(consumers.size).toBe(0);
    vi.mocked(getSceneSnapshot).mockResolvedValueOnce(changed);
    const reopened = mount(Harness); await flushPromises(); expect(reopened.text()).toBe("second");
    expect(selectCurrentTemplate).not.toHaveBeenCalled(); reopened.unmount();
  });

  it("does not let a late initial read roll back a newer applied event", async () => {
    let resolve!: (value: SceneSnapshot) => void;
    vi.mocked(getSceneSnapshot).mockReturnValueOnce(new Promise(done => { resolve = done; }));
    const wrapper = mount(Harness); await flushPromises();
    for (const callback of consumers) callback({ type: "snapshot", snapshot: snapshot("newer", 3) });
    resolve(snapshot("older", 2)); await flushPromises();
    expect(wrapper.text()).toBe("newer"); wrapper.unmount();
  });

  it("shows common when the mapping engine confirms common is applied", async () => {
    const wrapper = mount(Harness); await flushPromises();
    const changed = snapshot(null, 2);
    for (const callback of consumers) callback({ type: "mapping_applied", notice: changed.mappingNotice!, revision: 2 });
    await flushPromises();
    expect(selectCurrentTemplate).not.toHaveBeenCalled();
    expect(wrapper.text()).toBe("common"); wrapper.unmount();
  });

  it("keeps the previous applied template when a snapshot only changes the candidate", async () => {
    const wrapper = mount(Harness); await flushPromises();
    for (const callback of consumers) callback({ type: "snapshot", snapshot: { ...snapshot("first"), templateId: "candidate" } });
    await flushPromises();
    expect(wrapper.text()).toBe("first");
    expect(selectCurrentTemplate).not.toHaveBeenCalled(); wrapper.unmount();
  });

  it("releases a subscription that finishes after the page has closed", async () => {
    let resolve!: (stop: () => void) => void;
    vi.mocked(subscribeSceneEvents).mockReturnValueOnce(new Promise(done => { resolve = done; }));
    const stop = vi.fn(); const wrapper = mount(Harness); wrapper.unmount();
    resolve(stop); await flushPromises();
    expect(stop).toHaveBeenCalledOnce(); expect(getSceneSnapshot).not.toHaveBeenCalled();
  });
});
