// @vitest-environment jsdom

import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Ref } from "vue";
import { ref } from "vue";
import type { RuntimeSnapshot } from "../lib/bridge";
import { useAppUpdate, type AppUpdatePhase } from "../lib/app-update";
import AboutPage from "./AboutPage.vue";

const phase: Ref<AppUpdatePhase> = ref("idle");
const info = ref<Awaited<ReturnType<typeof useAppUpdate>>["info"]["value"]>(null);
const errorMessage = ref("");
const progress = ref({ downloaded: 0, contentLength: null as number | null, finished: false });
const check = vi.fn<() => Promise<void>>();
const install = vi.fn<() => Promise<void>>();

vi.mock("../lib/app-update", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/app-update")>();
  return {
    ...actual,
    useAppUpdate: () => ({
      phase,
      info,
      errorMessage,
      progress,
      check,
      install,
    }),
  };
});

const runtime: RuntimeSnapshot = {
  appVersion: "0.1.0",
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
    },
    buttonMapping: {
      enabled: true,
      gateActive: false,
      listenerActive: false,
      swallowedEdges: 0,
      leakedDowns: 0,
      firedGestures: 0,
      lastFired: null,
      lastError: null,
    },
  },
};

describe("about page update panel", () => {
  beforeEach(() => {
    phase.value = "idle";
    info.value = null;
    errorMessage.value = "";
    progress.value = { downloaded: 0, contentLength: null, finished: false };
    check.mockReset();
    install.mockReset();
  });

  it("初始状态显示手动检查入口", () => {
    const wrapper = mount(AboutPage, { props: { runtime } });
    expect(wrapper.text()).toContain("软件更新");
    expect(wrapper.text()).toContain("手动检查是否有新版本");
    const button = wrapper.findAll("button").find((b) => b.text().includes("检查更新"));
    expect(button).toBeDefined();
  });

  it("发现新版本时展示版本、说明与安装入口", async () => {
    phase.value = "available";
    info.value = {
      currentVersion: "0.1.0",
      available: true,
      version: "0.2.0",
      notes: "修复若干问题",
      date: null,
    };
    const wrapper = mount(AboutPage, { props: { runtime } });
    await flushPromises();
    expect(wrapper.text()).toContain("发现新版本");
    expect(wrapper.text()).toContain("0.2.0");
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
    const wrapper = mount(AboutPage, { props: { runtime } });
    await flushPromises();
    expect(wrapper.text()).toContain("1.0 MB / 4.0 MB");
    const bar = wrapper.find(".update-progress-bar");
    expect(bar.exists()).toBe(true);
    expect(bar.attributes("style")).toContain("width: 25%");
  });

  it("安装中提示自动重启且不提供可点击操作", async () => {
    phase.value = "installing";
    const wrapper = mount(AboutPage, { props: { runtime } });
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
    const wrapper = mount(AboutPage, { props: { runtime } });
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
      currentVersion: "0.2.0",
      available: false,
      version: null,
      notes: null,
      date: null,
    };
    const wrapper = mount(AboutPage, { props: { runtime } });
    await flushPromises();
    expect(wrapper.text()).toContain("已经是最新版本。");
    const installButton = wrapper
      .findAll("button")
      .find((b) => b.text().includes("下载并安装"));
    expect(installButton).toBeUndefined();
  });
});
