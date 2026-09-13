import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import RegisteredAppsDialog from "./RegisteredAppsDialog.vue";
import { scanRegisteredApps } from "../lib/bridge";
vi.mock("../lib/bridge", () => ({ scanRegisteredApps: vi.fn() }));
const apps = [
  { name: "Alpha", path: "shell:AppsFolder\\Alpha!App" },
  { name: "Beta", path: "shell:AppsFolder\\Beta!App" },
  { name: "Gamma", path: "shell:AppsFolder\\Gamma!App" },
];
beforeEach(() => { vi.mocked(scanRegisteredApps).mockReset(); vi.mocked(scanRegisteredApps).mockResolvedValue(apps); });
describe("registered app picker", () => {
  it("scans, selects all new applications, and excludes previously added targets", async () => {
    const wrapper = mount(RegisteredAppsDialog, { props: { knownApps: [apps[0]!], saving: false, saveError: null } });
    await flushPromises();
    expect(scanRegisteredApps).toHaveBeenCalledOnce();
    expect(wrapper.get('input[aria-label="Alpha"]').attributes("disabled")).toBeDefined();
    await wrapper.get('input[aria-label="全选当前结果"]').setValue(true);
    await wrapper.get(".primary-button").trigger("click");
    expect(wrapper.emitted("add")?.[0]?.[0]).toEqual(apps.slice(1));
    wrapper.unmount();
  });
  it("filters results, supports individual selection, and cancels without adding", async () => {
    const wrapper = mount(RegisteredAppsDialog, { props: { knownApps: [], saving: false, saveError: null } });
    await flushPromises();
    await wrapper.get('input[type="search"]').setValue("beta");
    expect(wrapper.findAll(".picker-app")).toHaveLength(1);
    await wrapper.get('input[aria-label="Beta"]').setValue(true);
    await wrapper.get('input[type="search"]').setValue("");
    expect((wrapper.get('input[aria-label="Beta"]').element as HTMLInputElement).checked).toBe(true);
    await wrapper.get('button[aria-label="关闭应用选择"]').trigger("click");
    expect(wrapper.emitted("close")).toHaveLength(1);
    expect(wrapper.emitted("add")).toBeUndefined();
    wrapper.unmount();
  });
  it("reports scan failures and retries", async () => {
    vi.mocked(scanRegisteredApps).mockRejectedValueOnce(new Error("scan unavailable"));
    const wrapper = mount(RegisteredAppsDialog, { props: { knownApps: [], saving: false, saveError: null } });
    await flushPromises();
    expect(wrapper.get('[role="alert"]').text()).toContain("scan unavailable");
    await wrapper.findAll("button").find(button => button.text() === "重新扫描")!.trigger("click");
    await flushPromises();
    expect(wrapper.find('[role="alert"]').exists()).toBe(false);
    expect(wrapper.findAll(".picker-app")).toHaveLength(3);
    wrapper.unmount();
  });
});
