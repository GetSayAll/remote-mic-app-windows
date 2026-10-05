// @vitest-environment jsdom

import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Ref } from "vue";
import { ref } from "vue";
import type { RuntimeSnapshot } from "../lib/bridge";
import { useAppUpdate, type AppUpdatePhase } from "../lib/app-update";
import SettingsPage from "./SettingsPage.vue";

const phase: Ref<AppUpdatePhase> = ref("idle");
const info = ref<Awaited<ReturnType<typeof useAppUpdate>>["info"]["value"]>(null);
const errorMessage = ref("");
const progress = ref({ downloaded: 0, contentLength: null as number | null, finished: false });
const includePrereleases = ref(false);
const preferenceBusy = ref(false);
const preferenceError = ref("");
const check = vi.fn<() => Promise<void>>();
const install = vi.fn<() => Promise<void>>();
const loadUpdatePreferences = vi.fn<() => Promise<void>>();
const setIncludePrereleases = vi.fn<(enabled: boolean) => Promise<void>>();
const themePreference = ref<"system" | "light" | "dark">("system");
const themeBusy = ref(false);
const themeError = ref("");
const setThemePreference = vi.fn<(value: "system" | "light" | "dark") => Promise<void>>();
// vi.mock 工厂会被提升到 import 之前，工厂里只能引用 vi.hoisted 出来的句柄。
const bridge = vi.hoisted(() => ({
  getLaunchAtLogin: vi.fn<() => Promise<boolean>>(),
  setLaunchAtLogin: vi.fn<(enabled: boolean) => Promise<boolean>>(),
  getAppIcon: vi.fn<() => Promise<"standard" | "faceted-duck">>(),
  setAppIcon: vi.fn<(identifier: "standard" | "faceted-duck") => Promise<"standard" | "faceted-duck">>(),
}));

vi.mock("../lib/app-update", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/app-update")>();
  return {
    ...actual,
    useAppUpdate: () => ({
      phase,
      info,
      errorMessage,
      progress,
      includePrereleases,
      preferenceBusy,
      preferenceError,
      check,
      install,
      loadUpdatePreferences,
      setIncludePrereleases,
    }),
  };
});

vi.mock("../lib/theme", () => ({
  useTheme: () => ({
    preference: themePreference,
    busy: themeBusy,
    errorMessage: themeError,
    setThemePreference,
  }),
}));

vi.mock("../lib/bridge", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/bridge")>();
  return { ...actual, ...bridge };
});

const runtime: RuntimeSnapshot = {
  appVersion: "0.5.0",
  platform: {
    platform: "browser-preview",
    windowsApiAvailable: false,
    bleScanAvailable: false,
    bleVoiceReady: false,
    wasapiReady: false,
    rawInputReady: false,
    sendInputReady: false,
    verificationStatus: "浏览器预览不代表真机通过",
    connection: {
      phase: "idle",
      remoteName: null,
      remoteModel: "unknown",
      capabilities: null,
      voiceState: "idle",
      decodedSamples: 0,
      generation: 0,
      reconnectAttempt: 0,
      powerNotificationsAvailable: false,
      lastError: null,
    },
    audio: {
      phase: "unsupported",
      selectedEndpointId: null,
      selectedEndpointName: null,
      queuedSamples: 0,
      submittedSamples: 0,
      generation: 0,
      lastError: null,
    },
    rawInput: {
      phase: "unsupported",
      matchedDeviceCount: 0,
      rawEventCount: 0,
      semanticEdgeCount: 0,
      lastButton: null,
      lastIsPressed: false,
      activeButtons: [],
      lastError: null,
      staleRemoteEventCount: 0,
    },
    buttonMapping: {
      enabled: true,
      gateActive: false,
      observedButtons: [],
      listenerActive: false,
      swallowedEdges: 0,
      leakedDowns: 0,
      firedGestures: 0,
      lastFired: null,
      lastError: null,
    },
  },
};

describe("settings page", () => {
  beforeEach(() => {
    phase.value = "idle";
    info.value = null;
    errorMessage.value = "";
    progress.value = { downloaded: 0, contentLength: null, finished: false };
    includePrereleases.value = false;
    preferenceBusy.value = false;
    preferenceError.value = "";
    check.mockReset();
    install.mockReset();
    loadUpdatePreferences.mockReset();
    setIncludePrereleases.mockReset();
    themePreference.value = "system";
    themeBusy.value = false;
    themeError.value = "";
    setThemePreference.mockReset();
    bridge.getLaunchAtLogin.mockReset().mockResolvedValue(false);
    bridge.setLaunchAtLogin.mockReset();
    // 后端默认值（2026-10-04 起 = 几何鸭）：mock 也按新装默认返回，与
    // `AppSettings::default()` 的用例语义对齐。
    bridge.getAppIcon.mockReset().mockResolvedValue("faceted-duck");
    bridge.setAppIcon.mockReset();
  });

  it("页面标题为「设置」，顶部显示应用标识、标语与当前版本", async () => {
    const wrapper = mount(SettingsPage, { props: { runtime } });
    await flushPromises();

    expect(wrapper.get("h1").text()).toBe("设置");
    expect(wrapper.text()).toContain("无线麦 SayAll");
    expect(wrapper.text()).toContain("让语音触手可及");
    expect(wrapper.text()).toContain("当前版本");
    expect(wrapper.text()).toContain("0.5.0");
    expect(wrapper.text()).toContain("检查预览版更新");
    // 2026-10-02 用户指定：这些说明文字都去掉。
    expect(wrapper.text()).not.toContain("预览版包含新功能");
    expect(wrapper.text()).not.toContain("该选择会在重启后保持");
    expect(wrapper.text()).not.toContain("跟随 Windows 的应用颜色模式");
  });

  it("外观选择器提供系统、浅色、深色三档并立即保存", async () => {
    const wrapper = mount(SettingsPage, { props: { runtime } });
    const radios = wrapper.findAll<HTMLInputElement>('input[name="theme-preference"]');

    expect(radios.map((radio) => radio.attributes("value"))).toEqual([
      "system",
      "light",
      "dark",
    ]);
    expect(radios[0].element.checked).toBe(true);

    await radios[2].setValue(true);
    expect(setThemePreference).toHaveBeenCalledWith("dark");
  });

  it("外观设置失败时显示就地错误", () => {
    themeError.value = "外观设置保存失败，请稍后重试。";
    const wrapper = mount(SettingsPage, { props: { runtime } });
    expect(wrapper.get('[role="alert"]').text()).toContain("外观设置保存失败");
  });

  it("预览版更新开关默认关闭并保存用户选择", async () => {
    const wrapper = mount(SettingsPage, { props: { runtime } });
    await flushPromises();
    expect(loadUpdatePreferences).toHaveBeenCalledTimes(1);
    const toggle = wrapper.find<HTMLInputElement>('label[title*="预览版本"] input');
    expect(toggle.element.checked).toBe(false);

    await toggle.setValue(true);
    expect(setIncludePrereleases).toHaveBeenCalledWith(true);
  });

  it("设置页的 Switch 开关都在对应文字右侧（2026-10-03 用户要求）", async () => {
    const wrapper = mount(SettingsPage, { props: { runtime } });
    await flushPromises();

    // 顶部「检查预览版更新」：文字在前、开关在后（标签行内顺序即视觉顺序）。
    const prerelease = wrapper.get("label.prerelease-toggle");
    expect((prerelease.element.firstElementChild as HTMLElement).textContent).toContain(
      "检查预览版更新",
    );
    expect(prerelease.element.lastElementChild).toBe(prerelease.find("input").element);

    // 通用「登录时自动启动」：同样文字在前、开关在后。
    const launch = wrapper.get('label[title*="Windows 登录启动项"]');
    expect((launch.element.firstElementChild as HTMLElement).textContent).toContain(
      "登录时自动启动",
    );
    expect(launch.element.lastElementChild).toBe(
      launch.find('input[name="launch-at-login"]').element,
    );
  });

  it("初始状态显示手动检查入口", () => {
    const wrapper = mount(SettingsPage, { props: { runtime } });
    expect(wrapper.text()).toContain("手动检查是否有新版本");
    expect(wrapper.text()).not.toContain("开源许可");
    expect(wrapper.text()).not.toContain("GPL-3.0");
    const button = wrapper.findAll("button").find((b) => b.text().includes("检查更新"));
    expect(button).toBeDefined();
  });

  it("问题反馈模块提供官网与 GitHub 入口，官网带 Windows 来源标记", async () => {
    const open = vi.spyOn(window, "open").mockImplementation(() => null);
    const wrapper = mount(SettingsPage, { props: { runtime } });
    await flushPromises();

    const buttons = wrapper
      .findAll("button")
      .filter((button) => ["官网", "GitHub"].includes(button.text()));
    expect(buttons.map((button) => button.text())).toEqual(["官网", "GitHub"]);

    await buttons[0].trigger("click");
    await buttons[1].trigger("click");
    await flushPromises();

    expect(open.mock.calls.map(([url]) => url)).toEqual([
      "https://sayall.app/?from=win",
      "https://github.com/GetSayAll/remote-mic-app-windows",
    ]);
    // 2026-10-02 用户指定：成功不再显示“已在系统默认浏览器打开…”这类提示。
    expect(wrapper.find(".link-message").exists()).toBe(false);
    open.mockRestore();
  });

  it("入口打开失败时就地显示原因而不是静默", async () => {
    const open = vi
      .spyOn(window, "open")
      .mockImplementation(() => {
        throw new Error("Not allowed to open url https://sayall.app/?from=win");
      });
    const wrapper = mount(SettingsPage, { props: { runtime } });
    await flushPromises();

    const website = wrapper.findAll("button").find((button) => button.text() === "官网");
    await website!.trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("Not allowed to open url");
    open.mockRestore();
  });

  it("应用图标新装默认几何鸭，切换后保存并立即生效，顶部标识同步换图", async () => {
    bridge.setAppIcon.mockImplementation(async (identifier) => identifier);
    const wrapper = mount(SettingsPage, { props: { runtime } });
    await flushPromises();

    expect(bridge.getAppIcon).toHaveBeenCalledTimes(1);
    const radios = wrapper.findAll<HTMLInputElement>('input[name="app-icon"]');
    expect(radios.map((radio) => radio.attributes("value"))).toEqual([
      "standard",
      "faceted-duck",
    ]);
    // 2026-10-04 用户定稿：新装默认选中几何鸭（后端默认值经 getAppIcon 读回）。
    expect(radios[1].element.checked).toBe(true);
    expect(wrapper.text()).toContain("应用图标");
    expect(wrapper.text()).toContain("默认");
    expect(wrapper.text()).toContain("几何鸭");
    expect(wrapper.get("img.app-logo").attributes("src")).toBe(
      "/app-icon-faceted-duck.png",
    );

    await radios[0].setValue(true);
    expect(bridge.setAppIcon).toHaveBeenCalledWith("standard");
    expect(radios[0].element.checked).toBe(true);
    // 顶部标识与窗口/托盘用同一个选择：切回「默认」即换回水彩鸭。
    expect(wrapper.get("img.app-logo").attributes("src")).toBe("/app-logo.png");
  });

  it("应用图标保存失败时就地报错并回到实际生效的图标", async () => {
    bridge.setAppIcon.mockRejectedValue(new Error("保存应用图标设置失败"));
    bridge.getAppIcon.mockResolvedValue("standard");
    const wrapper = mount(SettingsPage, { props: { runtime } });
    await flushPromises();

    const radios = wrapper.findAll<HTMLInputElement>('input[name="app-icon"]');
    await radios[1].setValue(true);
    await flushPromises();

    expect(wrapper.get('[role="alert"]').text()).toContain("保存应用图标设置失败");
    expect(radios[0].element.checked).toBe(true);
  });

  it("发现新版本时展示版本、说明与安装入口", async () => {
    phase.value = "available";
    info.value = {
      currentVersion: "0.5.0",
      available: true,
      version: "0.6.0",
      notes: "修复若干问题",
      date: null,
    };
    const wrapper = mount(SettingsPage, { props: { runtime } });
    await flushPromises();
    expect(wrapper.text()).toContain("发现新版本");
    expect(wrapper.text()).toContain("0.6.0");
    expect(wrapper.text()).toContain("修复若干问题");
    const installButton = wrapper
      .findAll("button")
      .find((b) => b.text().includes("下载并安装"));
    expect(installButton).toBeDefined();
    await installButton!.trigger("click");
    expect(install).toHaveBeenCalledTimes(1);
  });

  it("下载中显示进度条与双值文案", async () => {
    phase.value = "downloading";
    progress.value = { downloaded: 1_048_576, contentLength: 4_194_304, finished: false };
    const wrapper = mount(SettingsPage, { props: { runtime } });
    await flushPromises();
    expect(wrapper.text()).toContain("1.0 MB / 4.0 MB");
    const bar = wrapper.find(".update-progress-bar");
    expect(bar.exists()).toBe(true);
    expect(bar.attributes("style")).toContain("width: 25%");
  });

  it("安装中提示自动重启且不提供可点击操作", async () => {
    phase.value = "installing";
    const wrapper = mount(SettingsPage, { props: { runtime } });
    await flushPromises();
    expect(wrapper.text()).toContain("正在安装更新，应用将自动重启");
    const installButton = wrapper
      .findAll("button")
      .find((b) => b.text().includes("下载并安装"));
    expect(installButton).toBeUndefined();
  });

  it("失败时展示错误并提供重试检查", async () => {
    phase.value = "failed";
    errorMessage.value = "网络连接失败，请稍后重试";
    const wrapper = mount(SettingsPage, { props: { runtime } });
    await flushPromises();
    expect(wrapper.text()).toContain("网络连接失败，请稍后重试");
    const retry = wrapper.findAll("button").find((b) => b.text().includes("重试检查"));
    expect(retry).toBeDefined();
    await retry!.trigger("click");
    expect(check).toHaveBeenCalledTimes(1);
  });

  it("服务器确认无更新时显示已经是最新版本", async () => {
    phase.value = "up-to-date";
    info.value = {
      currentVersion: "0.5.0",
      available: false,
      version: null,
      notes: null,
      date: null,
    };
    const wrapper = mount(SettingsPage, { props: { runtime } });
    await flushPromises();
    expect(wrapper.text()).toContain("已经是最新版本。");
    const installButton = wrapper
      .findAll("button")
      .find((b) => b.text().includes("下载并安装"));
    expect(installButton).toBeUndefined();
  });
});
