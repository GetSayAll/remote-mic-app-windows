// @vitest-environment jsdom

import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
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

  it("诊断摘要已迁往关于页，权限页不再提供生成或复制入口", () => {
    const wrapper = mount(PermissionsPage, { props: { runtime } });
    expect(wrapper.text()).not.toContain("诊断摘要");
    expect(wrapper.find(".diagnostic-output").exists()).toBe(false);
  });
});
