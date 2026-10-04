import { describe, expect, it } from "vitest";
import type { DiagnosticReport } from "../lib/bridge";
import {
  formatOnboardingDiagnostics,
  ONBOARDING_STEP_LABELS,
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
  blockMessage: "完成一次真实的语音上屏测试，再继续。",
  remoteModel: "小米蓝牙语音遥控器 2 Pro",
  connectionPhase: "ready",
  bleVoiceReady: true,
  rawInputPhase: "ready",
  reconnectAttempt: 0,
  audioPhase: "ready",
  recommendedEndpointCount: 1,
  selectedRecommended: true,
  queuedSamples: 0,
  toolLabel: "微信输入法",
  hotkeyLabel: "左 Ctrl + 左 Win",
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

    expect(text).toContain("生成时间: 2026-10-05T06:52:00.000Z");
    expect(text).toContain("App 版本: 0.5.0");
    expect(text).toContain("Build: fe326f8a1b2c3d4e5f60718293a4b5c6d7e8f901");
    expect(text).toContain("构建通道: local-test");
    expect(text).toContain("Windows 版本: 10.0.26100");
    expect(text).toContain("进程架构: x86_64");
  });

  it("带出当前步骤、门禁码与阻断提示", () => {
    const text = formatOnboardingDiagnostics(report, snapshot);

    expect(text).toContain("当前步骤: ⑤ 按住说话验证");
    expect(text).toContain("步骤标识: voice_test");
    expect(text).toContain("门禁: voice_test.not_verified");
    expect(text).toContain("阻断提示: 完成一次真实的语音上屏测试，再继续。");
    // 步骤标签覆盖全部七步（新增步骤时这里会红）。
    expect(Object.keys(ONBOARDING_STEP_LABELS)).toHaveLength(7);
  });

  it("带出与 onboarding 相关的运行状态字段", () => {
    const text = formatOnboardingDiagnostics(report, snapshot);

    expect(text).toContain(
      "遥控器: 连接=ready 型号=小米蓝牙语音遥控器 2 Pro 语音就绪=是 按键通道=ready 重连次数=0",
    );
    expect(text).toContain("音频: 状态=ready 推荐设备=1 已选推荐=是 待发送样本=0");
    expect(text).toContain("输入工具: 微信输入法 快捷键=左 Ctrl + 左 Win 全按键支持=未开启");
    expect(text).toContain("Vokie: 未检测到");
    expect(text).toContain("普通按键: 已按 2 / 3 个不同按键");
    expect(text).toContain("按键映射: 向导期间暂挂");
  });

  it("门禁通过时不输出门禁与阻断行，避免误读为失败", () => {
    const text = formatOnboardingDiagnostics(report, {
      ...snapshot,
      gateCode: null,
      blockMessage: "",
      mappingSuspended: false,
      vokieInstalled: true,
      vokieRunning: true,
    });

    expect(text).not.toContain("门禁: ");
    expect(text).not.toContain("阻断提示");
    expect(text).toContain("Vokie: 运行中");
    expect(text).toContain("按键映射: 运行中");
  });

  it("不含设备与端点身份信息（只有数量与布尔）", () => {
    const text = formatOnboardingDiagnostics(report, snapshot);

    // 端点 id/名称、蓝牙地址、HID 路径、用户路径一律不得出现。
    for (const secret of ["cable-input", "CABLE Input", "AA:BB", "\\\\?\\HID", "C:\\Users"]) {
      expect(text).not.toContain(secret);
    }
    expect(text).toContain("推荐设备=1");
  });
});
