// @vitest-environment jsdom

import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
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
    verificationStatus: "浏览器预览不代表 Windows 真机通过",
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

describe("permissions diagnostics", () => {
  const writeText = vi.fn<(text: string) => Promise<void>>();

  beforeEach(() => {
    writeText.mockReset();
    writeText.mockResolvedValue();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
  });

  it("shows truthful unsupported states and copies the generated privacy-safe report", async () => {
    const wrapper = mount(PermissionsPage, { props: { runtime } });
    expect(wrapper.text()).not.toContain("尚未实现");
    expect(wrapper.text()).toContain("当前电脑不支持");

    const buttons = wrapper.findAll(".diagnostics-card button");
    await buttons[0].trigger("click");
    await flushPromises();

    const report = wrapper.get(".diagnostic-output").text();
    expect(report).toContain('"schemaVersion": 1');
    expect(report).not.toContain("remoteName");
    expect(report).not.toContain("selectedEndpointName");
    expect(report).not.toContain("lastError");

    await buttons[1].trigger("click");
    await flushPromises();

    expect(writeText).toHaveBeenCalledOnce();
    expect(writeText).toHaveBeenCalledWith(report);
    expect(wrapper.text()).toContain("诊断摘要已复制到剪贴板");
  });
});
