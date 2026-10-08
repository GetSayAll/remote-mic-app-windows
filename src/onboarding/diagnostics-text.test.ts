import { describe, expect, it } from "vitest";
import type { DiagnosticReport } from "../lib/bridge";
import {
  formatOnboardingDiagnostics,
  ONBOARDING_STEP_ORDER,
  type OnboardingDiagnosticsSnapshot,
} from "./diagnostics-text";

const report: DiagnosticReport = {
  schemaVersion: 2,
  appVersion: "0.5.0",
  appBuild: "1001",
  sourceRevision: "fe326f8a1b2c3d4e5f60718293a4b5c6d7e8f901",
  buildChannel: "local-test",
  windowsVersion: "10.0.26100",
  processArchitecture: "x86_64",
  platform: "windows",
  verificationStatus: "真机验证中",
  capabilities: {
    windowsApiAvailable: true,
    bleScanAvailable: true,
    bleVoiceReady: true,
    wasapiReady: true,
    rawInputReady: true,
    sendInputReady: true,
  },
  connection: {
    phase: "ready",
    capabilitiesConfirmed: true,
    sampleRate: 16000,
    frameSize: 40,
    decodedSamples: 320,
    generation: 3,
    reconnectAttempt: 0,
    powerNotificationsAvailable: true,
    errorPresent: false,
  },
  audio: {
    phase: "ready",
    endpointConfigured: true,
    queuedSamples: 0,
    submittedSamples: 320,
    generation: 1,
    errorPresent: false,
  },
  rawInput: {
    phase: "ready",
    matchedDeviceCount: 1,
    rawEventCount: 6,
    semanticEdgeCount: 3,
    lastButton: "home",
    lastIsPressed: false,
    errorPresent: false,
  },
  sendInput: {
    available: true,
    submittedBatches: 1,
    submittedEvents: 2,
    errorPresent: false,
  },
  buttonMapping: {
    enabled: true,
    gateActive: false,
    listenerActive: true,
    swallowedEdges: 0,
    leakedDowns: 0,
    firedGestures: 0,
    errorPresent: false,
  },
};

const snapshot: OnboardingDiagnosticsSnapshot = {
  step: "voice_test",
  gateCode: "voice_test.not_verified",
  blocked: true,
  model: "rc003",
  connectionPhase: "ready",
  bleVoiceReady: true,
  rawInputPhase: "ready",
  reconnectAttempt: 0,
  audioPhase: "ready",
  recommendedEndpointCount: 1,
  selectedRecommended: true,
  queuedSamples: 0,
  tool: "wechat",
  hotkey: "left_control+left_windows",
  captureEnabled: false,
  vokieInstalled: false,
  vokieRunning: false,
  distinctButtonCount: 2,
  mappingSuspended: true,
};

describe("formatOnboardingDiagnostics", () => {
  it("带出定位必需的版本与构建信息（Windows / App / Build / 架构）", () => {
    const text = formatOnboardingDiagnostics(
      report,
      snapshot,
      new Date("2026-10-05T06:52:00.000Z"),
    );

    expect(text).toContain("Generated at: 2026-10-05T06:52:00.000Z");
    expect(text).toContain("App version: 0.5.0");
    expect(text).toContain("Build: fe326f8a1b2c3d4e5f60718293a4b5c6d7e8f901");
    expect(text).toContain("Build channel: local-test");
    expect(text).toContain("Windows version: 10.0.26100");
    expect(text).toContain("Architecture: x86_64");
  });

  it("带出当前步骤（token + 序号）、门禁码与阻断状态", () => {
    const text = formatOnboardingDiagnostics(report, snapshot);

    expect(text).toContain("Wizard step: voice_test (5/7)");
    expect(text).toContain("Gate: voice_test.not_verified");
    expect(text).toContain("Blocked: yes");
    // 步骤顺序表覆盖全部七步（新增步骤时这里会红）。
    expect(ONBOARDING_STEP_ORDER).toHaveLength(7);
  });

  it("带出与 onboarding 相关的运行状态字段（全部为稳定 token）", () => {
    const text = formatOnboardingDiagnostics(report, snapshot);

    expect(text).toContain(
      "Remote: phase=ready model=rc003 voice_ready=yes input_phase=ready reconnects=0",
    );
    expect(text).toContain("Audio: phase=ready recommended=1 selected=yes queued_samples=0");
    expect(text).toContain(
      "Input tool: wechat hotkey=left_control+left_windows full_key_support=off",
    );
    expect(text).toContain("Vokie: not_detected");
    expect(text).toContain("Buttons observed: 2/3");
    expect(text).toContain("Mapping: suspended");
  });

  it("门禁通过时不输出门禁行，Blocked 记为 no", () => {
    const text = formatOnboardingDiagnostics(report, {
      ...snapshot,
      gateCode: null,
      blocked: false,
      mappingSuspended: false,
      vokieInstalled: true,
      vokieRunning: true,
    });

    expect(text).not.toContain("Gate: ");
    expect(text).toContain("Blocked: no");
    expect(text).toContain("Vokie: running");
    expect(text).toContain("Mapping: active");
  });

  it("内容全英文：不出现任何中日韩字符与全角标点", () => {
    const text = formatOnboardingDiagnostics(report, snapshot);

    expect(text).not.toMatch(/[\u3000-\u9fff\uff00-\uffef]/);
  });

  it("不含设备与端点身份信息（只有数量与布尔）", () => {
    const text = formatOnboardingDiagnostics(report, snapshot);

    // 端点 id/名称、蓝牙地址、HID 路径、用户路径一律不得出现。
    for (const secret of ["cable-input", "CABLE Input", "AA:BB", "\\\\?\\HID", "C:\\Users"]) {
      expect(text).not.toContain(secret);
    }
    expect(text).toContain("recommended=1");
  });
});
