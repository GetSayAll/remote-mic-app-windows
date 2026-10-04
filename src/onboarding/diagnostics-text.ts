/**
 * 向导「复制诊断信息」的文本生成（2026-10-05 用户要求）。
 *
 * 目标：onboarding 过不去时，用户一次复制即可给出定位所需的环境与状态，而不必
 * 教他导出日志。内容分三段：
 * - 环境：App 版本 / Build（源码修订）/ 构建通道 / Windows 版本 / 进程架构 ——
 *   都来自 Rust `get_diagnostic_report`（真实读回，不是前端拼装）；
 * - 向导：当前步骤、门禁码、阻断提示；
 * - 运行状态：连接 / 音频 / 输入工具 / 全按键支持 / Vokie / 普通按键观察。
 *
 * 脱敏红线（与 LOGGING.md 不同）：本块是**用户主动复制**发给支持人员的内容，
 * 允许出现稳定 token（`voice_test`、`rc003` 等）；但绝不包含设备 id/名称（蓝牙
 * 地址、HID 路径）、音频端点 id/名称、文件路径、用户名、任何输入文字。
 * 端点与设备一律只给数量/布尔，型号只给产品名。
 */
import type { DiagnosticReport } from "../lib/bridge";
import type { OnboardingStep } from "./flow";

/** 步骤序号 + 名称（诊断文本里显示"卡在哪一步"）。 */
export const ONBOARDING_STEP_LABELS: Record<OnboardingStep, string> = {
  welcome: "① 欢迎使用",
  remote: "② 连接遥控器",
  audio: "③ 选择语音设备",
  voice_tool: "④ 选择输入工具",
  voice_test: "⑤ 按住说话验证",
  controls: "⑥ 普通按键体验",
  complete: "⑦ 设置完成",
};

export interface OnboardingDiagnosticsSnapshot {
  step: OnboardingStep;
  /** 当前门禁失败码（通过时 null）。 */
  gateCode: string | null;
  /** 页脚给用户看的阻断提示（无则空串）。 */
  blockMessage: string;
  /** 已本地化的型号名（remoteModelLabel）。 */
  remoteModel: string;
  connectionPhase: string;
  bleVoiceReady: boolean;
  rawInputPhase: string;
  reconnectAttempt: number;
  audioPhase: string;
  /** 带「推荐」标记的端点数量（只给数量，不给 id/名称）。 */
  recommendedEndpointCount: number;
  selectedRecommended: boolean;
  queuedSamples: number;
  /** 已本地化的工具名（豆包输入法 / 微信输入法 / Vokie / 其他工具）。 */
  toolLabel: string;
  hotkeyLabel: string;
  captureEnabled: boolean;
  vokieInstalled: boolean;
  vokieRunning: boolean;
  distinctButtonCount: number;
  mappingSuspended: boolean;
}

function yesNo(value: boolean): string {
  return value ? "是" : "否";
}

export function formatOnboardingDiagnostics(
  report: DiagnosticReport,
  snapshot: OnboardingDiagnosticsSnapshot,
  now: Date = new Date(),
): string {
  const lines: string[] = [
    "无线麦 SayAll 诊断信息",
    `生成时间: ${now.toISOString()}`,
    "—— 环境 ——",
    `App 版本: ${report.appVersion}`,
    `Build: ${report.sourceRevision}`,
    `构建通道: ${report.buildChannel}`,
    `Windows 版本: ${report.windowsVersion}`,
    `进程架构: ${report.processArchitecture}`,
    "—— 向导 ——",
    `当前步骤: ${ONBOARDING_STEP_LABELS[snapshot.step]}`,
    `步骤标识: ${snapshot.step}`,
  ];
  if (snapshot.gateCode) {
    lines.push(`门禁: ${snapshot.gateCode}`);
  }
  if (snapshot.blockMessage) {
    lines.push(`阻断提示: ${snapshot.blockMessage}`);
  }
  lines.push(
    "—— 运行状态 ——",
    `遥控器: 连接=${snapshot.connectionPhase} 型号=${snapshot.remoteModel} 语音就绪=${yesNo(
      snapshot.bleVoiceReady,
    )} 按键通道=${snapshot.rawInputPhase} 重连次数=${snapshot.reconnectAttempt}`,
    `音频: 状态=${snapshot.audioPhase} 推荐设备=${snapshot.recommendedEndpointCount} 已选推荐=${yesNo(
      snapshot.selectedRecommended,
    )} 待发送样本=${snapshot.queuedSamples}`,
    `输入工具: ${snapshot.toolLabel} 快捷键=${snapshot.hotkeyLabel} 全按键支持=${
      snapshot.captureEnabled ? "已开启" : "未开启"
    }`,
    `Vokie: ${
      snapshot.vokieRunning ? "运行中" : snapshot.vokieInstalled ? "已安装未运行" : "未检测到"
    }`,
    `普通按键: 已按 ${snapshot.distinctButtonCount} / 3 个不同按键`,
    `按键映射: ${snapshot.mappingSuspended ? "向导期间暂挂" : "运行中"}`,
  );
  return lines.join("\n");
}
