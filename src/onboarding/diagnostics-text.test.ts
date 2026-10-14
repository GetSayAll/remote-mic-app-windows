import { describe, expect, it } from "vitest";
import type { DiagnosticReport } from "../lib/bridge";
import {
  formatOnboardingDiagnostics,
  ONBOARDING_STEP_ORDER,
  type OnboardingDiagnosticsSnapshot,
} from "./diagnostics-text";

const report: DiagnosticReport = {
  schemaVersion: 3,
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
  ime: {
    detected: false,
    activeName: null,
    installedNames: [],
    expectedName: null,
    matchesExpected: null,
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
  connectionErrorPresent: false,
  remoteButtonObserved: true,
  rawInputMatchedDeviceCount: 1,
  rawInputRawEventCount: 6,
  rawInputSemanticEdgeCount: 3,
  rawInputStaleEventCount: 0,
  audioPhase: "ready",
  recommendedEndpointCount: 1,
  selectedRecommended: true,
  queuedSamples: 0,
  audioErrorPresent: false,
  tool: "wechat",
  hotkey: "left_control+left_windows",
  captureEnabled: false,
  captureHealth: "off",
  vokieInstalled: false,
  vokieRunning: false,
  voiceTestPhase: "terminal",
  voiceTerminalCode: "voice.no_transcript",
  voiceLateAccepted: false,
  distinctButtonCount: 2,
  mappingSuspended: true,
  imeDetected: true,
  imeMatchesExpected: false,
  imeInstalledCount: 3,
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
      "Remote: phase=ready model=rc003 voice_ready=yes reconnects=0 error=no button_observed=yes",
    );
    expect(text).toContain(
      "Raw input: phase=ready matched=1 raw_events=6 semantic_edges=3 stale_events=0",
    );
    expect(text).toContain("Audio: phase=ready recommended=1 selected=yes queued_samples=0 error=no");
    expect(text).toContain(
      "Input tool: wechat hotkey=left_control+left_windows full_key_support=off capture=off",
    );
    expect(text).toContain("Vokie: not_detected");
    expect(text).toContain("Voice test: phase=terminal terminal=voice.no_transcript late=no");
    expect(text).toContain("Buttons observed: 2/3");
    expect(text).toContain(
      "IME: detected=yes match=no installed_count=3 expected_tool=wechat",
    );
    expect(text).toContain("Mapping: suspended");
  });

  it("读不到输入法时只写 detected=no，不臆测（2026-10-13 现场）", () => {
    const text = formatOnboardingDiagnostics(report, {
      ...snapshot,
      imeDetected: false,
      imeMatchesExpected: null,
      imeInstalledCount: 0,
    });

    expect(text).toContain(
      "IME: detected=no match=unknown installed_count=0 expected_tool=wechat",
    );
  });

  it("「全按键支持」开关开着但捕获没生效时在块里自证（2026-10-14）", () => {
    // 现场场景（rc003-capture-silent-inactive）：开关显示开启，实际没有所有权。
    // 单看 full_key_support=on 会误导支持；capture= 与它并列后不用日志就能分流。
    const text = formatOnboardingDiagnostics(report, {
      ...snapshot,
      captureEnabled: true,
      captureHealth: "inactive",
    });

    expect(text).toContain("full_key_support=on capture=inactive");
    // `starting`（助手/桥未就绪、可能长期停留）同样不是生效——不能只认 inactive。
    expect(
      formatOnboardingDiagnostics(report, {
        ...snapshot,
        captureEnabled: true,
        captureHealth: "starting",
      }),
    ).toContain("full_key_support=on capture=starting");
  });

  it("捕获状态读不到时写 capture=unknown，不臆测（2026-10-14）", () => {
    const text = formatOnboardingDiagnostics(report, {
      ...snapshot,
      captureHealth: "unknown",
    });

    expect(text).toContain("full_key_support=off capture=unknown");
  });

  it("迟到接受在文本里自证（terminal=late_transcript late=yes）", () => {
    // 2026-10-13 现场报告：3 秒窗口失败后文字仍出现（对齐 Mac 现值门禁）。
    const text = formatOnboardingDiagnostics(report, {
      ...snapshot,
      voiceTerminalCode: "late_transcript",
      voiceLateAccepted: true,
    });

    expect(text).toContain("Voice test: phase=terminal terminal=late_transcript late=yes");
  });

  it("②步现场（按键已收到但应用未连接）在文本里能自证", () => {
    // 2026-10-10 用户报障的现场形状：系统侧输入通道在位、按键已收到，
    // 但应用自己的会话从未建立（idle / 型号未读回 / 无重连）。
    const text = formatOnboardingDiagnostics(report, {
      ...snapshot,
      step: "remote",
      model: "unknown",
      connectionPhase: "idle",
      bleVoiceReady: false,
      rawInputPhase: "ready",
      remoteButtonObserved: true,
      rawInputMatchedDeviceCount: 1,
      rawInputSemanticEdgeCount: 1,
      voiceTestPhase: "waiting_start",
      voiceTerminalCode: null,
    });

    expect(text).toContain("Wizard step: remote (2/7)");
    expect(text).toContain("phase=idle model=unknown voice_ready=no");
    expect(text).toContain("button_observed=yes");
    expect(text).toContain("Raw input: phase=ready matched=1");
    // 没有产生过 attempt 终态时写 none，不写内部字段名。
    expect(text).toContain("Voice test: phase=waiting_start terminal=none");
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
