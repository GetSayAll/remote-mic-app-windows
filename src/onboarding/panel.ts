/**
 * 向导右栏「检查卡」的状态推导（设计稿 2026-10-05：右栏每一行 = 当前步骤的一条
 * 「继续」条件）。纯函数：输入全部来自页面既有状态（运行快照、门禁上下文、
 * attempt 终态），输出可直接渲染的行；不改任何判据——`flow.evaluateGate` 仍是
 * 「继续」的唯一裁决者，这里只是把同一批事实画出来。
 */
import type { VoiceInputTool } from "../lib/bridge";
import { CONNECTED_PHASES, type OnboardingStep } from "./flow";
import type { ConnectionPhase } from "../lib/bridge";
import type { VoiceAttemptState } from "./voice-attempt";

/** 状态四态：待满足 / 进行中 / 已通过 / 需处理（与状态行组件一致）。 */
export type StatusRowState = "pending" | "busy" | "ok" | "warn";

export interface SideCheckRow {
  label: string;
  state: StatusRowState;
  /** 右侧短值（如 "2 / 3"）；缺省显示状态词。 */
  value?: string;
}

export interface SidePanel {
  visual: "remote" | "app" | "done";
  caption?: string;
  cardTitle?: string;
  rows?: SideCheckRow[];
}

/** 右栏推导的全部输入（页面统一收集后调用；全部为只读事实）。 */
export interface SidePanelContext {
  connectionPhase: ConnectionPhase;
  bleVoiceReady: boolean;
  remoteButtonObserved: boolean;
  recommendedEndpointCount: number;
  selectedRecommended: boolean;
  wasapiReady: boolean;
  audioLastError: string | null;
  scanningAudio: boolean;
  tool: VoiceInputTool | null;
  hotkeyReady: boolean;
  captureEnabled: boolean;
  vokieRunning: boolean;
  voicePhase: VoiceAttemptState;
  voiceTerminalCode: string | null;
  voiceResultPassed: boolean;
  voiceEvidence: {
    decodedSamples: number;
    submittedSamples: number;
    drainObserved: boolean;
  } | null;
  distinctButtons: number;
  mappingSuspended: boolean;
}

export function buildSidePanel(step: OnboardingStep, ctx: SidePanelContext): SidePanel {
  switch (step) {
    case "welcome":
      return { visual: "app", caption: "无线说话，也能随手控制" };

    case "remote": {
      const connected =
        CONNECTED_PHASES.includes(ctx.connectionPhase) && ctx.bleVoiceReady;
      const connecting =
        !connected &&
        (ctx.connectionPhase === "connecting" || ctx.connectionPhase === "reconnecting");
      return {
        visual: "remote",
        cardTitle: "连接检查",
        rows: [
          { label: "遥控器已连接", state: connected ? "ok" : connecting ? "busy" : "pending" },
          {
            label: "控制按键已收到",
            state: ctx.remoteButtonObserved ? "ok" : "pending",
          },
        ],
      };
    }

    case "audio":
      return {
        visual: "remote",
        cardTitle: "音频检查",
        rows: [
          {
            label: "已检测到带「推荐」标记的 CABLE 设备",
            state:
              ctx.recommendedEndpointCount > 0
                ? "ok"
                : ctx.scanningAudio
                  ? "busy"
                  : "warn",
          },
          {
            label: "已选中推荐设备",
            state: ctx.selectedRecommended ? "ok" : "pending",
          },
          {
            label: "音频输出正常",
            state: ctx.audioLastError
              ? "warn"
              : ctx.wasapiReady
                ? "ok"
                : ctx.scanningAudio
                  ? "busy"
                  : "pending",
          },
        ],
      };

    case "voice_tool": {
      const rows: SideCheckRow[] = [
        { label: "已选择输入工具", state: ctx.tool ? "ok" : "pending" },
        { label: "语音键已适配", state: ctx.hotkeyReady ? "ok" : "pending" },
      ];
      switch (ctx.tool) {
        case "doubao":
          rows.push({
            label: "「支持更多输入工具」已开启",
            state: ctx.captureEnabled ? "ok" : "warn",
          });
          break;
        case "vokie":
          rows.push({
            label: "Vokie 已运行",
            state: ctx.vokieRunning ? "ok" : "warn",
          });
          break;
        case "wechat":
          rows.push({ label: "无需额外设置", state: "ok" });
          break;
        case "other":
          rows.push({
            label: "已选择语音键",
            state: ctx.hotkeyReady ? "ok" : "pending",
          });
          break;
        default:
          rows.push({ label: "等待选择输入工具", state: "pending" });
      }
      return { visual: "remote", cardTitle: "工具检查", rows };
    }

    case "voice_test": {
      const terminal = ctx.voiceTerminalCode;
      const evidence = ctx.voiceEvidence;
      const inSession =
        ctx.voicePhase === "streaming" ||
        ctx.voicePhase === "waiting_end" ||
        ctx.voicePhase === "waiting_transcript";
      const sessionState: StatusRowState =
        terminal === "voice.session_not_started"
          ? "warn"
          : ctx.voicePhase !== "waiting_start" || ctx.voiceResultPassed
            ? "ok"
            : "pending";
      const samplesState: StatusRowState =
        evidence && evidence.decodedSamples > 0
          ? "ok"
          : terminal === "voice.no_samples"
            ? "warn"
            : inSession
              ? "busy"
              : "pending";
      const deliveryState: StatusRowState =
        evidence && evidence.submittedSamples > 0 && evidence.drainObserved
          ? "ok"
          : terminal === "voice.audio_delivery_failed"
            ? "warn"
            : inSession
              ? "busy"
              : "pending";
      const transcriptState: StatusRowState = ctx.voiceResultPassed
        ? "ok"
        : terminal === "voice.no_transcript"
          ? "warn"
          : ctx.voicePhase === "waiting_transcript"
            ? "busy"
            : "pending";
      return {
        visual: "remote",
        cardTitle: "实时检查",
        rows: [
          { label: "语音键已按下并松开", state: sessionState },
          { label: "语音声音已收到", state: samplesState },
          { label: "音频输出正常", state: deliveryState },
          { label: "文字已经出现", state: transcriptState },
        ],
      };
    }

    case "controls": {
      const n = ctx.distinctButtons;
      return {
        visual: "remote",
        cardTitle: "按键检查",
        rows: [
          {
            label: "已按 3 个不同的普通按键",
            state: n >= 3 ? "ok" : n > 0 ? "busy" : "pending",
            value: `${n} / 3`,
          },
          {
            label: "按键映射暂时暂停，完成后恢复",
            state: ctx.mappingSuspended ? "ok" : "pending",
          },
        ],
      };
    }

    case "complete":
      return { visual: "done", caption: "开始使用无线麦" };
  }
}
