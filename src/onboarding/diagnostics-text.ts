/**
 * 向导「复制诊断信息」的文本生成（2026-10-05 用户要求）。
 *
 * 目标：onboarding 过不去时，用户一次复制即可给出定位所需的环境与状态，而不必
 * 教他导出日志。**内容一律英文**（2026-10-05 二次要求）；界面按钮与状态提示仍是
 * 中文（那是应用 UI）。分三段：
 * - 环境：App 版本 / Build（源码修订）/ 构建通道 / Windows 版本 / 进程架构 ——
 *   都来自 Rust `get_diagnostic_report`（真实读回，不是前端拼装）；
 * - 向导：当前步骤（英文 token + 序号）、门禁码、是否被阻断；
 * - 运行状态：连接 / 音频 / 输入工具 / 全按键支持 / Vokie / 普通按键观察。
 *
 * 脱敏红线（与 LOGGING.md 不同）：本块是**用户主动复制**发给支持人员的内容，
 * 允许出现稳定 token（`voice_test`、`rc003` 等）；但绝不包含设备 id/名称（蓝牙
 * 地址、HID 路径）、音频端点 id/名称、文件路径、用户名、任何输入文字。
 * 端点与设备一律只给数量/布尔，型号只给 token。
 */
import type { DiagnosticReport } from "../lib/bridge";
import type { OnboardingStep } from "./flow";

/** 步骤固定顺序（诊断里的 "n/7" 由它推导）。 */
export const ONBOARDING_STEP_ORDER: readonly OnboardingStep[] = [
  "welcome",
  "remote",
  "audio",
  "voice_tool",
  "voice_test",
  "controls",
  "complete",
];

export interface OnboardingDiagnosticsSnapshot {
  step: OnboardingStep;
  /** 当前门禁失败码（通过时 null）。 */
  gateCode: string | null;
  /** 门禁是否未通过（输出布尔，避免带入中文文案）。 */
  blocked: boolean;
  /** 遥控器型号 token（rc001 / rc003 / unknown）。 */
  model: string;
  connectionPhase: string;
  bleVoiceReady: boolean;
  rawInputPhase: string;
  reconnectAttempt: number;
  audioPhase: string;
  /** 带「推荐」标记的端点数量（只给数量，不给 id/名称）。 */
  recommendedEndpointCount: number;
  selectedRecommended: boolean;
  queuedSamples: number;
  /** 输入工具 token（doubao / wechat / vokie / other / unset）。 */
  tool: string;
  /** 按住说话快捷键 token（right_alt / left_control+left_windows / none）。 */
  hotkey: string;
  captureEnabled: boolean;
  vokieInstalled: boolean;
  vokieRunning: boolean;
  distinctButtonCount: number;
  mappingSuspended: boolean;
}

function yesNo(value: boolean): string {
  return value ? "yes" : "no";
}

export function formatOnboardingDiagnostics(
  report: DiagnosticReport,
  snapshot: OnboardingDiagnosticsSnapshot,
  now: Date = new Date(),
): string {
  const stepIndex = ONBOARDING_STEP_ORDER.indexOf(snapshot.step) + 1;
  const lines: string[] = [
    "SayAll diagnostics",
    `Generated at: ${now.toISOString()}`,
    `App version: ${report.appVersion}`,
    `Build: ${report.sourceRevision}`,
    `Build channel: ${report.buildChannel}`,
    `Windows version: ${report.windowsVersion}`,
    `Architecture: ${report.processArchitecture}`,
    `Wizard step: ${snapshot.step} (${stepIndex}/7)`,
  ];
  if (snapshot.gateCode) {
    lines.push(`Gate: ${snapshot.gateCode}`);
  }
  lines.push(
    `Blocked: ${yesNo(snapshot.blocked)}`,
    `Remote: phase=${snapshot.connectionPhase} model=${snapshot.model} voice_ready=${yesNo(
      snapshot.bleVoiceReady,
    )} input_phase=${snapshot.rawInputPhase} reconnects=${snapshot.reconnectAttempt}`,
    `Audio: phase=${snapshot.audioPhase} recommended=${snapshot.recommendedEndpointCount} selected=${yesNo(
      snapshot.selectedRecommended,
    )} queued_samples=${snapshot.queuedSamples}`,
    `Input tool: ${snapshot.tool} hotkey=${snapshot.hotkey} full_key_support=${
      snapshot.captureEnabled ? "on" : "off"
    }`,
    `Vokie: ${
      snapshot.vokieRunning ? "running" : snapshot.vokieInstalled ? "installed" : "not_detected"
    }`,
    `Buttons observed: ${snapshot.distinctButtonCount}/3`,
    `Mapping: ${snapshot.mappingSuspended ? "suspended" : "active"}`,
  );
  return lines.join("\n");
}
