import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { effectiveLanguage } from "../lib/language-state";
import RegisteredAppsDialog from "./RegisteredAppsDialog.vue";
import { getApplicationIcon, pickCustomApp, scanRegisteredApps } from "../lib/bridge";

vi.mock("../lib/bridge", () => ({
  scanRegisteredApps: vi.fn(),
  pickCustomApp: vi.fn(),
  getApplicationIcon: vi.fn(),
}));

const apps = [
  { name: "Alpha", path: "shell:AppsFolder\\Alpha!App" },
  { name: "Beta", path: "shell:AppsFolder\\Beta!App" },
  { name: "Gamma", path: "shell:AppsFolder\\Gamma!App" },
];

beforeEach(() => {
  effectiveLanguage.value = "zh-Hans";
  vi.mocked(scanRegisteredApps).mockReset();
  vi.mocked(scanRegisteredApps).mockResolvedValue(apps);
  vi.mocked(pickCustomApp).mockReset();
  vi.mocked(pickCustomApp).mockResolvedValue(null);
  vi.mocked(getApplicationIcon).mockReset();
  vi.mocked(getApplicationIcon).mockResolvedValue(null);
});

describe("registered app picker", () => {
  it("scans into single-select radios and keeps known apps selectable", async () => {
    const wrapper = mount(RegisteredAppsDialog, {
      props: { knownApps: [apps[0]!], saving: false, saveError: null },
    });
    await flushPromises();

    expect(scanRegisteredApps).toHaveBeenCalledOnce();
    expect(wrapper.find('input[aria-label="全选当前结果"]').exists()).toBe(false);
    expect(wrapper.find(".picker-foot-hint").exists()).toBe(false);
    expect(wrapper.find(".picker-selection-sub").exists()).toBe(false);
    expect(wrapper.findAll('input[type="radio"]')).toHaveLength(3);
    const alpha = wrapper.get('input[aria-label="Alpha"]');
    expect((alpha.element as HTMLInputElement).disabled).toBe(false);
    expect(wrapper.get(".picker-added").text()).toBe("已添加");
    wrapper.unmount();
  });

  it("locks both methods after one selection until the user clears it", async () => {
    const wrapper = mount(RegisteredAppsDialog, {
      props: { knownApps: [], saving: false, saveError: null },
    });
    await flushPromises();

    await wrapper.get('input[aria-label="Alpha"]').setValue(true);
    await flushPromises();
    expect(wrapper.get(".picker-selection").text()).toContain("Alpha");
    expect(wrapper.get(".picker-selection").text()).toContain("本机扫描");
    expect((wrapper.get('input[aria-label="Beta"]').element as HTMLInputElement).disabled).toBe(true);
    expect(
      (wrapper.get(".picker-program-pick").element as HTMLButtonElement).disabled,
    ).toBe(true);
    expect(wrapper.get(".picker-hint").text()).toContain("要重新选择");

    // 锁定期间即使合成 change 也不得换选；真实点击会被 disabled 拦下。
    await wrapper.get('input[aria-label="Beta"]').trigger("change");
    await flushPromises();
    expect(wrapper.get(".picker-selection").text()).toContain("Alpha");

    await wrapper.get(".picker-clear").trigger("click");
    await flushPromises();
    expect(wrapper.get(".picker-selection").text()).toContain("还没有选择应用");
    expect((wrapper.get('input[aria-label="Beta"]').element as HTMLInputElement).disabled).toBe(false);

    await wrapper.get('input[aria-label="Beta"]').setValue(true);
    await flushPromises();
    expect(wrapper.get(".picker-selection").text()).toContain("Beta");
    wrapper.unmount();
  });

  it("shows a selected scan app with a real icon when the shell returns one", async () => {
    vi.mocked(getApplicationIcon).mockResolvedValueOnce("data:image/png;base64,abc");
    const wrapper = mount(RegisteredAppsDialog, {
      props: { knownApps: [], saving: false, saveError: null },
    });
    await flushPromises();
    await wrapper.get('input[aria-label="Alpha"]').setValue(true);
    await flushPromises();

    expect(getApplicationIcon).toHaveBeenCalledWith(apps[0]!.path);
    expect(wrapper.get("img.picker-app-icon").attributes("src")).toBe(
      "data:image/png;base64,abc",
    );
    wrapper.unmount();
  });

  it("shows a real icon for a manually picked shortcut", async () => {
    vi.mocked(pickCustomApp).mockResolvedValue({
      name: "Custom",
      path: "C:\\Apps\\Custom.lnk",
    });
    vi.mocked(getApplicationIcon).mockResolvedValueOnce("data:image/png;base64,file");
    const wrapper = mount(RegisteredAppsDialog, {
      props: { knownApps: [], saving: false, saveError: null },
    });
    await flushPromises();
    await wrapper.get(".picker-program-pick").trigger("click");
    await flushPromises();

    expect(getApplicationIcon).toHaveBeenCalledWith("C:\\Apps\\Custom.lnk");
    expect(wrapper.get("img.picker-app-icon").attributes("src")).toBe(
      "data:image/png;base64,file",
    );
    wrapper.unmount();
  });

  it("confirms one scanned app with source scan", async () => {
    const wrapper = mount(RegisteredAppsDialog, {
      props: { knownApps: [], saving: false, saveError: null },
    });
    await flushPromises();
    await wrapper.get('input[aria-label="Alpha"]').setValue(true);
    expect(wrapper.get(".picker-actions .primary-button").text()).toBe("添加应用");
    await wrapper.get(".picker-actions .primary-button").trigger("click");

    expect(wrapper.emitted("select")?.[0]).toEqual([apps[0], "scan"]);
    expect(wrapper.emitted("add")).toBeUndefined();
    wrapper.unmount();
  });

  it("manual program pick fills the selection area and confirms source file", async () => {
    vi.mocked(pickCustomApp).mockResolvedValue({
      name: "Custom",
      path: "C:\\Apps\\Custom.exe",
    });
    const wrapper = mount(RegisteredAppsDialog, {
      props: { knownApps: [], saving: false, saveError: null },
    });
    await flushPromises();
    await wrapper.get(".picker-program-pick").trigger("click");
    await flushPromises();

    expect(wrapper.get(".picker-selection").text()).toContain("Custom");
    expect(wrapper.get(".picker-selection").text()).toContain("程序选择");
    expect(wrapper.get(".picker-selection").text()).not.toContain("C:\\Apps\\Custom.exe");
    expect(wrapper.find(".picker-selection-path").exists()).toBe(false);

    await wrapper.get(".picker-actions .primary-button").trigger("click");
    expect(wrapper.emitted("select")?.[0]).toEqual([
      { name: "Custom", path: "C:\\Apps\\Custom.exe" },
      "file",
    ]);
    wrapper.unmount();
  });

  it("reports scan failures and retries", async () => {
    vi.mocked(scanRegisteredApps).mockRejectedValueOnce(new Error("scan unavailable"));
    const wrapper = mount(RegisteredAppsDialog, {
      props: { knownApps: [], saving: false, saveError: null },
    });
    await flushPromises();
    expect(wrapper.get('[role="alert"]').text()).toContain("无法读取本机应用列表，请稍后重试。");
    await wrapper.get(".picker-rescan").trigger("click");
    await flushPromises();
    expect(wrapper.find('[role="alert"]').exists()).toBe(false);
    expect(wrapper.findAll(".picker-row")).toHaveLength(3);
    wrapper.unmount();
  });

  it("uses English copy and source labels in English mode", async () => {
    effectiveLanguage.value = "en";
    const wrapper = mount(RegisteredAppsDialog, {
      props: { knownApps: [apps[0]!], saving: false, saveError: null },
    });
    await flushPromises();

    expect(wrapper.get("h3").text()).toBe("Choose App");
    expect(wrapper.findAll("h4").map((node) => node.text())).toEqual(["Local Scan", "Program"]);
    expect(wrapper.get(".picker-subtitle").text()).toBe(
      "Scan this PC or choose a program. You can select one app.",
    );
    expect(wrapper.get(".picker-program-pick").text()).toContain(
      "Choose a program or shortcut",
    );
    expect(wrapper.get(".picker-selection").text()).toContain("No app selected");
    expect(wrapper.get(".picker-hint").text()).toContain(
      "No app selected. Choose one from the two sections above.",
    );
    expect(wrapper.get(".picker-actions .primary-button").text()).toBe("Add App");
    expect(wrapper.get(".picker-added").text()).toBe("Added");

    await wrapper.get('input[aria-label="Alpha"]').setValue(true);
    await flushPromises();
    expect(wrapper.get(".picker-source").text()).toBe("Local Scan");
    expect(wrapper.get(".picker-clear").text()).toBe("Clear Selection");
    expect(wrapper.get(".picker-hint").text()).toContain(
      "App selected. To choose another, cancel the selected app below first.",
    );
    wrapper.unmount();
  });
});
