import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { effectiveLanguage } from "../lib/language-state";
import RegisteredAppsDialog from "./RegisteredAppsDialog.vue";
import { scanRegisteredApps } from "../lib/bridge";
vi.mock("../lib/bridge", () => ({ scanRegisteredApps: vi.fn() }));
const apps = [
  { name: "Alpha", path: "shell:AppsFolder\\Alpha!App" },
  { name: "Beta", path: "shell:AppsFolder\\Beta!App" },
  { name: "Gamma", path: "shell:AppsFolder\\Gamma!App" },
];
beforeEach(() => { effectiveLanguage.value = "zh-Hans"; vi.mocked(scanRegisteredApps).mockReset(); vi.mocked(scanRegisteredApps).mockResolvedValue(apps); });
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
    // 原始 IPC 错误只进控制台，界面给固定文案。
    expect(wrapper.get('[role="alert"]').text()).toContain("无法读取本机应用列表，请稍后重试。");
    await wrapper.findAll("button").find(button => button.text() === "重新扫描")!.trigger("click");
    await flushPromises();
    expect(wrapper.find('[role="alert"]').exists()).toBe(false);
    expect(wrapper.findAll(".picker-app")).toHaveLength(3);
    wrapper.unmount();
  });

  it("英文模式下标题、搜索框与按钮切换为英文", async () => {
    effectiveLanguage.value = "en";
    const wrapper = mount(RegisteredAppsDialog, { props: { knownApps: [apps[0]!], saving: false, saveError: null } });
    await flushPromises();

    expect(wrapper.get("h3").text()).toBe("Installed Apps");
    expect(wrapper.get('input[type="search"]').attributes("placeholder")).toBe("Search apps");
    expect(wrapper.get('input[type="search"]').attributes("aria-label")).toBe("Search installed apps");
    expect(wrapper.get('button[aria-label="Close app picker"]').attributes("title")).toBe("Close");
    expect(wrapper.findAll("button").map(button => button.text())).toEqual([
      "×",
      "Scan Again",
      "Cancel",
      "Add Selected (0)",
    ]);
    expect(wrapper.get(".picker-select-row span").text()).toBe("3 apps · 0 selected");
    expect(wrapper.get(".picker-app small").text()).toBe("Added");
    wrapper.unmount();
  });
});
