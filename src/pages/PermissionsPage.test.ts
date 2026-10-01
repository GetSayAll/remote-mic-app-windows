// @vitest-environment jsdom

import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import type { RuntimeSnapshot } from "../lib/bridge";
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
    expect(report).toContain('"schemaVersion": 1');
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

    expect(wrapper.text()).toContain("当前是浏览器预览，无法打开日志目录");
  });
});
