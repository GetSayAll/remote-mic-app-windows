/**
 * 向导「复制诊断信息」的文本生成（2026-10-05 用户要求；2026-10-10 扩字段）。
 *
 * 目标：onboarding 过不去时，用户一次复制即可给出定位所需的环境与状态，而不必
 * 教他导出日志。**内容一律英文**（2026-10-05 二次要求）；界面按钮与状态提示仍是
 * 中文（那是应用 UI）。分四段：
 * - 环境：App 版本 / Build（源码修订）/ 构建通道 / Windows 版本 / 进程架构 ——
 *   都来自 Rust `get_diagnostic_report`（真实读回，不是前端拼装）；
 * - 向导：当前步骤（英文 token + 序号）、门禁码、是否被阻断；
 * - 链路：连接（是否有错误记录、②步是否已收到按键）、输入通道计数、音频
 *   （是否有错误记录）、输入工具、Vokie、语音测试；
 * - 会话内计数：普通按键观察数（⑥步 3 键门禁）、按键映射是否被暂挂。
 *
 * 2026-10-10 增补（现场"按键已收到，但遥控器已连接没选上"）：
 * - `button_observed`：②步是否已观察到普通按键（原来只在界面与日志里，块里没有，
 *   那次报障只能靠截图补形状）；
 * - `Raw input:` 行：输入通道的阶段与四个计数（绑定接口数 / 原始报文 / 语义边沿 /
 *   被丢弃报文）——"按键收不到"与"绑定失效"的分界靠这几个数；
 * - `error=`（连接、音频各一处）：只给布尔，区分"从未尝试"与"试过且失败"，
 *   错误原文一律不进（脱敏红线）；
 * - `Voice test:` 行：第⑤步 attempt 的当前阶段与终态码（无终态时写 `none`；
 *   迟到接受后终态码写 `late_transcript`）与 `late=` 布尔。
 *
 * 2026-10-14 增补（现场"全按键支持开着却没生效"）：
 * - `capture=`：`full_key_support=` 是开关**设置**（向导只在④/⑤/⑦步读过它）；
 *   这里并列写实际捕获状态（读回 `CaptureHealth`：off / starting / active /
 *   inactive；读不到写 unknown）。`active` 才算生效；`inactive`（过宽限仍无
 *   所有权）与可能长期停留的 `starting`（助手/桥未就绪）都是没生效——用户
 *   复制一次即自证。
 *
 * 脱敏红线（与 LOGGING.md 不同）：本块是**用户主动复制**发给支持人员的内容，
 * 允许出现稳定 token（`voice_test`、`rc003` 等）；但绝不包含设备 id/名称（蓝牙
 * 地址、HID 路径）、音频端点 id/名称、文件路径、用户名、任何输入文字。
 * 端点与设备一律只给数量/布尔，型号只给 token。
 */
import type { DiagnosticReport, Rc003CaptureHealth } from "../lib/bridge";
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
  /** 连接层是否有错误记录（只给布尔；错误原文不进）。 */
  connectionErrorPresent: boolean;
  /** 本向导会话第②步是否已观察到普通按键。 */
  remoteButtonObserved: boolean;
  /** 输入通道：绑定的接口数（0 = HID 接口不在位）。 */
  rawInputMatchedDeviceCount: number;
  /** 输入通道：原始报文数。 */
  rawInputRawEventCount: number;
  /** 输入通道：语义按键边沿数。 */
  rawInputSemanticEdgeCount: number;
  /** 输入通道：来自遥控器但被路径过滤丢弃的报文数（绑定失效的直接证据）。 */
  rawInputStaleEventCount: number;
  audioPhase: string;
  /** 带「推荐」标记的端点数量（只给数量，不给 id/名称）。 */
  recommendedEndpointCount: number;
  selectedRecommended: boolean;
  queuedSamples: number;
  /** 音频层是否有错误记录（只给布尔；错误原文不进）。 */
  audioErrorPresent: boolean;
  /** 输入工具 token（doubao / wechat / vokie / other / unset）。 */
  tool: string;
  /** 按住说话快捷键 token（right_alt / left_control+left_windows / none）。 */
  hotkey: string;
  captureEnabled: boolean;
  /**
   * 「全按键支持」的**实际捕获状态**（2026-10-14）：读回 `CaptureHealth`
   * （off / starting / active / inactive），读不到时写 unknown。
   * 与 `captureEnabled`（开关设置）并列——只有 `active` 才算生效；`inactive`
   * （过宽限仍无所有权）与可能长期停留的 `starting`（助手/桥未就绪）都是没
   * 生效，不用日志就能在块里分流（对照 rc003-capture-silent-inactive 现场）。
   */
  captureHealth: Rc003CaptureHealth | "unknown";
  vokieInstalled: boolean;
  vokieRunning: boolean;
  /** 第⑤步 attempt 的当前阶段（VoiceAttemptState token）。 */
  voiceTestPhase: string;
  /** 第⑤步 attempt 的终态码（如 voice.no_transcript；无终态时 null；迟到接受后为 late_transcript）。 */
  voiceTerminalCode: string | null;
  /** 第⑤步是否走了「迟到接受」（3 秒窗口失败后文字仍出现；对齐 Mac 现值门禁）。 */
  voiceLateAccepted: boolean;
  distinctButtonCount: number;
  mappingSuspended: boolean;
  /**
   * 输入法（2026-10-13 现场）：③诊断"语音没出字是不是因为没切输入法"。
   * 复制块**保持全英文**（既有约定）——不放输入法显示名（含中文），名字留在本机日志里；
   * 这里只给"读没读到 / 是不是所选工具 / 系统里装了几个"。
   */
  imeDetected: boolean;
  /** 当前会话输入法是否就是所选工具对应的那个（判定不出来为 null）。 */
  imeMatchesExpected: boolean | null;
  /** 系统里可用的输入法数量。 */
  imeInstalledCount: number;
}

function yesNo(value: boolean): string {
  return value ? "yes" : "no";
}

/** 三态（yes / no / unknown）：输入法是否匹配允许"判定不出来"。 */
function threeState(value: boolean | null): string {
  return value === null ? "unknown" : yesNo(value);
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
    )} reconnects=${snapshot.reconnectAttempt} error=${yesNo(
      snapshot.connectionErrorPresent,
    )} button_observed=${yesNo(snapshot.remoteButtonObserved)}`,
    `Raw input: phase=${snapshot.rawInputPhase} matched=${snapshot.rawInputMatchedDeviceCount} raw_events=${snapshot.rawInputRawEventCount} semantic_edges=${snapshot.rawInputSemanticEdgeCount} stale_events=${snapshot.rawInputStaleEventCount}`,
    `Audio: phase=${snapshot.audioPhase} recommended=${snapshot.recommendedEndpointCount} selected=${yesNo(
      snapshot.selectedRecommended,
    )} queued_samples=${snapshot.queuedSamples} error=${yesNo(snapshot.audioErrorPresent)}`,
    `Input tool: ${snapshot.tool} hotkey=${snapshot.hotkey} full_key_support=${
      snapshot.captureEnabled ? "on" : "off"
    } capture=${snapshot.captureHealth}`,
    `Vokie: ${
      snapshot.vokieRunning ? "running" : snapshot.vokieInstalled ? "installed" : "not_detected"
    }`,
    `Voice test: phase=${snapshot.voiceTestPhase} terminal=${
      snapshot.voiceTerminalCode ?? "none"
    } late=${yesNo(snapshot.voiceLateAccepted)}`,
    `Buttons observed: ${snapshot.distinctButtonCount}/3`,
    // 输入法（2026-10-13 现场）：只给英文 token 与布尔/计数——"是不是没切输入法"看 match，
    // 输入法显示名留在本机日志（复制块保持全英文是既有约定）。
    `IME: detected=${yesNo(snapshot.imeDetected)} match=${threeState(
      snapshot.imeMatchesExpected,
    )} installed_count=${snapshot.imeInstalledCount} expected_tool=${snapshot.tool}`,
    `Mapping: ${snapshot.mappingSuspended ? "suspended" : "active"}`,
  );
  return lines.join("\n");
}
