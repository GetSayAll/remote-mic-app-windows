// @vitest-environment jsdom

import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RuntimeSnapshot } from "../lib/bridge";
import { effectiveLanguage } from "../lib/language-state";
import PermissionsPage from "./PermissionsPage.vue";

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
      lastIsPressed: null,
      activeButtons: [],
      lastError: null,
      staleRemoteEventCount: 0,
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

describe("permissions page", () => {
  beforeEach(() => {
    effectiveLanguage.value = "zh-Hans";
  });

  it("shows truthful unsupported states", () => {
    const wrapper = mount(PermissionsPage, { props: { runtime } });
    expect(wrapper.text()).not.toContain("尚未实现");
    expect(wrapper.text()).toContain("当前电脑不支持");
  });

  it("诊断摘要入口落在权限页（2026-10-01 从关于页迁回）", async () => {
    const writeText = vi.fn<(text: string) => Promise<void>>();
    writeText.mockResolvedValue();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    const wrapper = mount(PermissionsPage, { props: { runtime } });
    expect(wrapper.find(".diagnostics-card").exists()).toBe(true);
    const buttons = wrapper.findAll<HTMLButtonElement>(".diagnostics-card button");
    expect(buttons.map((button) => button.text())).toEqual([
      "生成摘要",
      "复制摘要",
      "打开日志目录",
    ]);

    await buttons[0].trigger("click");
    await flushPromises();

    const report = wrapper.get(".diagnostic-output").text();
    expect(report).toContain('"schemaVersion": 4');
    expect(report).toContain('"windowsVersion"');
    expect(report).toContain('"sourceRevision"');
    // 开机上下文（2026-10-20）：设置页诊断摘要同样带出，报障时无需另拉日志。
    expect(report).toContain('"bootType"');
    expect(report).toContain('"fastStartupEnabled"');
    expect(report).not.toContain("remoteName");
    expect(report).not.toContain("selectedEndpointName");
    expect(report).not.toContain("lastError");
    expect(wrapper.text()).toContain("诊断摘要已生成");

    await buttons[1].trigger("click");
    await flushPromises();

    expect(writeText).toHaveBeenCalledOnce();
    expect(writeText).toHaveBeenCalledWith(report);
    expect(wrapper.text()).toContain("诊断摘要已复制到剪贴板");
  });

  it("浏览器预览下打开日志目录给出明确不可用提示而不是静默失败", async () => {
    const wrapper = mount(PermissionsPage, { props: { runtime } });
    const button = wrapper
      .findAll("button")
      .find((candidate) => candidate.text().includes("打开日志目录"));
    expect(button).toBeDefined();

    await button!.trigger("click");
    await flushPromises();

    // 原始 IPC 错误只进控制台，界面给固定文案（英文模式同样可翻译）。
    expect(wrapper.text()).toContain("无法打开日志目录，请稍后重试。");
  });

  it("英文模式下页面标题、权限条目与诊断按钮使用英文", async () => {
    effectiveLanguage.value = "en";
    const wrapper = mount(PermissionsPage, { props: { runtime } });

    expect(wrapper.get("h1").text()).toBe("Permissions");
    expect(wrapper.text()).toContain("Bluetooth");
    expect(wrapper.text()).toContain("Button Listening & Mapping");
    expect(wrapper.text()).toContain("Audio Device");
    expect(wrapper.text()).toContain("Not supported on this PC");
    expect(wrapper.text()).toContain("Diagnostic Summary");
    expect(
      wrapper.findAll(".diagnostics-card button").map((button) => button.text()),
    ).toEqual(["Generate Summary", "Copy Summary", "Open Log Folder"]);
  });
});
