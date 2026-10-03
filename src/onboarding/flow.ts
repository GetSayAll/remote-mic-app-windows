/**
 * 首次使用向导（Onboarding）流程状态机（纯逻辑：无 UI、无 IPC）。
 *
 * 规范来源：Mac 仓 origin/main `feature/first-run-onboarding/PRODUCT_SPEC.md`
 * 与 Windows 设计稿 `artifacts/design/2026-10-04-onboarding-design.html`：
 * - 步骤顺序按 D1 确认的 Mac 可见顺序（欢迎 → 遥控器 → 语音设备 → 输入工具
 *   → 按住说话验证 → 普通按键体验 → 完成）；
 * - 每一步「能否继续」都由实时能力状态决定（页面访问与点击不是通过条件）；
 * - 阻断一律返回稳定失败码（小写 token），供页面显示与结构化日志使用。
 */
import type { ConnectionPhase, RawInputPhase, VoiceInputTool } from "../lib/bridge";

export type OnboardingStep =
  | "welcome"
  | "remote"
  | "audio"
  | "voice_tool"
  | "voice_test"
  | "controls"
  | "complete";

export type OnboardingPhase = "prepare" | "setup" | "try_it";

/** 顺序即导航顺序；不提供「跳过」，完成前没有进入主界面的旁路。 */
export const ONBOARDING_STEPS: readonly OnboardingStep[] = [
  "welcome",
  "remote",
  "audio",
  "voice_tool",
  "voice_test",
  "controls",
  "complete",
];

const PHASES: Record<OnboardingStep, OnboardingPhase> = {
  welcome: "prepare",
  remote: "setup",
  audio: "setup",
  voice_tool: "setup",
  voice_test: "try_it",
  controls: "try_it",
  complete: "try_it",
};

/** 稳定失败码（小写 token，日志安全；设计稿 §7）。 */
export type OnboardingBlockCode =
  | "remote.not_connected"
  | "remote.listener_not_ready"
  | "remote.button_not_ready"
  | "audio.install_required"
  | "audio.not_selected"
  | "audio.not_ready"
  | "tool.not_selected"
  | "tool.conflict.vokie_running"
  | "tool.doubao.authorization_required"
  | "tool.vokie.not_installed"
  | "tool.vokie.not_running"
  | "tool.hotkey_unset"
  | "voice_test.not_verified"
  | "controls.not_confirmed"
  | "complete.runtime_regressed";

export interface StepGate {
  ok: boolean;
  code: OnboardingBlockCode | null;
}

export function isOnboardingStep(value: unknown): value is OnboardingStep {
  return typeof value === "string" && (ONBOARDING_STEPS as readonly string[]).includes(value);
}

/** 未知步骤（更旧/更新版本写入）归一化为欢迎，不把用户留在空白页（设计稿 §6）。 */
export function normalizeStep(value: unknown): OnboardingStep {
  return isOnboardingStep(value) ? value : "welcome";
}

export function stepIndex(step: OnboardingStep): number {
  return ONBOARDING_STEPS.indexOf(step);
}

export function previousStep(step: OnboardingStep): OnboardingStep | null {
  const index = stepIndex(step);
  return index > 0 ? ONBOARDING_STEPS[index - 1] : null;
}

export function nextStep(step: OnboardingStep): OnboardingStep | null {
  const index = stepIndex(step);
  return index >= 0 && index < ONBOARDING_STEPS.length - 1 ? ONBOARDING_STEPS[index + 1] : null;
}

export function phaseOf(step: OnboardingStep): OnboardingPhase {
  return PHASES[step];
}

/** 门禁上下文：由页面从运行快照/设备探测组装，本模块不发起任何调用。 */
export interface OnboardingContext {
  remote: {
    connectionPhase: ConnectionPhase;
    bleVoiceReady: boolean;
    rawInputPhase: RawInputPhase;
    /** 本向导会话里已观察到至少一个普通按键（生产链路）。 */
    remoteButtonObserved: boolean;
  };
  audio: {
    /** 系统里存在带「推荐」标记的 CABLE 播放端。 */
    recommendedEndpointAvailable: boolean;
    /** 当前选择的端点属于推荐集合。 */
    selectedEndpointRecommended: boolean;
    wasapiReady: boolean;
    lastError: string | null;
  };
  voiceTool: {
    tool: VoiceInputTool | null;
    /** 豆包路径必需：「支持更多输入工具」（全按键支持）已开启且授权完成。 */
    doubaoCaptureEnabled: boolean;
    vokieInstalled: boolean;
    vokieRunning: boolean;
    /** 「其他工具」的按键已选定（含明确的「不按键」）。 */
    otherHotkeyChosen: boolean;
  };
  voiceTest: {
    /** 已产生一次终态为通过的验证 attempt（不持久化的已通过标志除外）。 */
    verified: boolean;
  };
  controls: {
    /** 本向导会话里已观察到的不同普通按键个数。 */
    distinctButtons: number;
  };
}

/** 语音连接处于这些阶段即视为「已连接」（streaming/draining 是按住期间的瞬态）。 */
const CONNECTED_PHASES: readonly ConnectionPhase[] = ["ready", "streaming", "draining"];

function pass(): StepGate {
  return { ok: true, code: null };
}

function block(code: OnboardingBlockCode): StepGate {
  return { ok: false, code };
}

export function evaluateGate(step: OnboardingStep, context: OnboardingContext): StepGate {
  switch (step) {
    case "welcome":
      return pass();
    case "remote":
      return evaluateRemote(context.remote);
    case "audio":
      return evaluateAudio(context.audio);
    case "voice_tool":
      return evaluateVoiceTool(context.voiceTool);
    case "voice_test":
      return context.voiceTest.verified ? pass() : block("voice_test.not_verified");
    case "controls":
      return context.controls.distinctButtons >= 3 ? pass() : block("controls.not_confirmed");
    case "complete":
      return evaluateComplete(context);
  }
  // 未知步骤不应出现（normalizeStep 已兜底）；防御性阻断，并给出可诊断的失败码。
  return block("complete.runtime_regressed");
}

function evaluateRemote(remote: OnboardingContext["remote"]): StepGate {
  if (!CONNECTED_PHASES.includes(remote.connectionPhase) || !remote.bleVoiceReady) {
    return block("remote.not_connected");
  }
  if (remote.rawInputPhase !== "ready") {
    return block("remote.listener_not_ready");
  }
  if (!remote.remoteButtonObserved) {
    return block("remote.button_not_ready");
  }
  return pass();
}

function evaluateAudio(audio: OnboardingContext["audio"]): StepGate {
  if (!audio.recommendedEndpointAvailable) {
    return block("audio.install_required");
  }
  if (!audio.selectedEndpointRecommended) {
    return block("audio.not_selected");
  }
  if (!audio.wasapiReady || audio.lastError !== null) {
    return block("audio.not_ready");
  }
  return pass();
}

function evaluateVoiceTool(voiceTool: OnboardingContext["voiceTool"]): StepGate {
  const tool = voiceTool.tool;
  if (tool === null) {
    return block("tool.not_selected");
  }
  if (tool === "doubao") {
    // D4 已确认：同键冲突优先阻断（Vokie 会抢先响应右 Alt），再检查授权状态。
    if (voiceTool.vokieRunning) {
      return block("tool.conflict.vokie_running");
    }
    if (!voiceTool.doubaoCaptureEnabled) {
      return block("tool.doubao.authorization_required");
    }
    return pass();
  }
  if (tool === "vokie") {
    if (!voiceTool.vokieInstalled) {
      return block("tool.vokie.not_installed");
    }
    if (!voiceTool.vokieRunning) {
      return block("tool.vokie.not_running");
    }
    return pass();
  }
  if (tool === "other") {
    return voiceTool.otherHotkeyChosen ? pass() : block("tool.hotkey_unset");
  }
  // 微信输入法：无额外前置（按住约半秒与联网属于工具自身行为说明）。
  return pass();
}

function evaluateComplete(context: OnboardingContext): StepGate {
  const gates = [
    evaluateRemote(context.remote),
    evaluateAudio(context.audio),
    evaluateVoiceTool(context.voiceTool),
  ];
  return gates.find((gate) => !gate.ok) ?? pass();
}
