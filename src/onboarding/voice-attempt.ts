/**
 * 向导第⑤步「按住说话验证」的一次 attempt 控制器（纯逻辑，设计稿 §5.5 / §8）。
 *
 * 证据来源：运行快照轮询（消费方约 150ms 喂一帧）。为什么不是 BLE 推送事件：
 * 探针记录 `docs/investigations/2026-10-04-windows-onboarding-feasibility.md` 允许
 * 「快照轮询 + 会话终态聚合」的等价方案；现有快照已提供全部所需事实：
 * - `connection.voiceState`：会话开始/结束的观测点（idle → streaming → idle）；
 * - `connection.decodedSamples`：累计解码采样（跨会话只增不减，用基准差值）；
 * - `audio.submittedSamples`：每次 `begin_session` 归零，会话结束后即本会话总量；
 * - `audio.queuedSamples / phase / lastError`：投递排空与失败分类。
 *
 * 规则（一次 attempt 一个终态；迟到回调与下一次会话不得改写已产生的终态）：
 * 1. 开始 = 观测到非 idle 的语音状态；基准在 arm 后的第一帧快照上固定；
 * 2. 结束 = 观测到回到 idle（成功路径上 audio 已先完成排空，queued 归零）；
 * 3. 结束后的失败优先级：音频错误 > 无解码采样 > 有采样未投递 > 排空异常；
 * 4. 通过 = 投递完成 + 转写文字在窗口内出现 + 未检测到物理键盘输入；
 * 5. 排水/转写各有一个截止时间，超时分别给 `session_not_ended` / `no_transcript`。
 */
import type { AudioPhase, VoiceSessionState } from "../lib/bridge";

export type VoiceAttemptFailureCode =
  | "voice.session_not_started"
  | "voice.no_samples"
  | "voice.audio_delivery_failed"
  | "voice.session_not_ended"
  | "voice.no_transcript"
  | "voice.manual_input"
  /** 输入框未聚焦（文字落不到窗口里，先让用户点回输入框）。 */
  | "voice.input_target_not_ready"
  /** 测试过程中输入框失焦。 */
  | "voice.focus_lost";

export type VoiceAttemptState =
  | "idle"
  | "waiting_start"
  | "streaming"
  | "waiting_end"
  | "waiting_transcript"
  | "terminal";

export interface VoiceAttemptInput {
  voiceState: VoiceSessionState;
  decodedSamples: number;
  audioPhase: AudioPhase;
  submittedSamples: number;
  queuedSamples: number;
  audioLastError: string | null;
  generation: number;
}

export interface VoiceAttemptEvidence {
  generation: number;
  /** 是否观测到会话开始（false = 会话落在两帧之间，结束证据仍然有效）。 */
  startObserved: boolean;
  /** 本会话解码采样增量（结束时的累计值 − arm 时基准）。 */
  decodedSamples: number;
  /** 本会话已提交采样（`begin_session` 归零后的最大值）。 */
  submittedSamples: number;
  /** 观测到的结束时刻队列长度（成功路径应为 0）。 */
  queuedEnd: number;
  /** 是否观测到"回到 idle 且队列已排空"的结束形态。 */
  drainObserved: boolean;
}

export interface VoiceAttemptTerminal {
  attemptId: number;
  result: "passed" | "failed";
  code: VoiceAttemptFailureCode | null;
  evidence: VoiceAttemptEvidence;
  elapsedMs: number;
}

export interface VoiceAttemptTrackerOptions {
  attemptId: number;
  /** 松开后等待转写文字的窗口（Mac 侧同款 3 秒）。 */
  transcriptWindowMs?: number;
  /** 观测到 draining 后等待收尾的上限；超时给 `session_not_ended`。 */
  drainTimeoutMs?: number;
}

export interface VoiceAttemptTracker {
  /** 重新开始一次 attempt，并以当前快照固定解码基准。 */
  arm(input: VoiceAttemptInput, now: number): void;
  /** 喂一帧运行快照；达到终态时返回终态（只返回一次）。 */
  observe(input: VoiceAttemptInput, now: number): VoiceAttemptTerminal | null;
  /** 测试输入框观测到转写文字。 */
  notifyTranscript(now: number): VoiceAttemptTerminal | null;
  /** 检测到物理键盘输入（手动输入不通过）。 */
  notifyManualInput(now: number): VoiceAttemptTerminal | null;
  /** 消费方已知原因的主动终止（如连接不可用、用户重试）。 */
  abort(code: VoiceAttemptFailureCode, now: number): VoiceAttemptTerminal | null;
  state(): VoiceAttemptState;
}

const DEFAULT_TRANSCRIPT_WINDOW_MS = 3_000;
const DEFAULT_DRAIN_TIMEOUT_MS = 8_000;

export function createVoiceAttemptTracker(
  options: VoiceAttemptTrackerOptions,
): VoiceAttemptTracker {
  const transcriptWindowMs = options.transcriptWindowMs ?? DEFAULT_TRANSCRIPT_WINDOW_MS;
  const drainTimeoutMs = options.drainTimeoutMs ?? DEFAULT_DRAIN_TIMEOUT_MS;

  let state: VoiceAttemptState = "idle";
  let startedAt = 0;
  let decodedBaseline = 0;
  let generation = 0;
  let maxSubmitted = 0;
  let maxDecoded = 0;
  let queuedEnd = 0;
  let transcriptSeen = false;
  let manualInputSeen = false;
  let sessionStartObserved = false;
  let sessionEndAt: number | null = null;
  let drainingSince: number | null = null;
  let terminal: VoiceAttemptTerminal | null = null;

  function evidence(drainObserved: boolean): VoiceAttemptEvidence {
    return {
      generation,
      startObserved: sessionStartObserved,
      decodedSamples: Math.max(0, maxDecoded - decodedBaseline),
      submittedSamples: maxSubmitted,
      queuedEnd,
      drainObserved,
    };
  }

  function finish(
    result: "passed" | "failed",
    code: VoiceAttemptFailureCode | null,
    drainObserved: boolean,
    now: number,
  ): VoiceAttemptTerminal {
    terminal = {
      attemptId: options.attemptId,
      result,
      code,
      evidence: evidence(drainObserved),
      elapsedMs: Math.max(0, now - startedAt),
    };
    state = "terminal";
    return terminal;
  }

  function arm(input: VoiceAttemptInput, now: number): void {
    state = "waiting_start";
    startedAt = now;
    decodedBaseline = input.decodedSamples;
    generation = input.generation;
    maxSubmitted = 0;
    maxDecoded = input.decodedSamples;
    queuedEnd = 0;
    transcriptSeen = false;
    manualInputSeen = false;
    sessionStartObserved = false;
    sessionEndAt = null;
    drainingSince = null;
    terminal = null;
  }

  function observe(input: VoiceAttemptInput, now: number): VoiceAttemptTerminal | null {
    if (state === "idle" || state === "terminal") return null;

    // 采样计数只增不减；始终记录最大值用于结束时的差值。
    maxDecoded = Math.max(maxDecoded, input.decodedSamples);

    if (state === "waiting_start") {
      const idleFrame =
        input.voiceState === "idle" && input.audioPhase !== "streaming" && input.audioPhase !== "draining";
      if (idleFrame) {
        // 极短按压：整个会话落在两帧之间。解码计数已增长 → 仍按"会话已结束"分类，
        // 证据里标记 startObserved=false，不谎报观测到了开始。
        if (maxDecoded > decodedBaseline) {
          sessionStartObserved = false;
          state = "waiting_end";
        } else {
          return null;
        }
      } else {
        sessionStartObserved = true;
        generation = input.generation || generation;
        state = "streaming";
      }
    }

    if (state === "streaming" || state === "waiting_end") {
      maxSubmitted = Math.max(maxSubmitted, input.submittedSamples);
      const draining = input.voiceState === "draining" || input.audioPhase === "draining";
      if (draining && drainingSince === null) {
        drainingSince = now;
      }
      if (draining && drainingSince !== null && now - drainingSince > drainTimeoutMs) {
        return finish("failed", "voice.session_not_ended", false, now);
      }

      const ended = input.voiceState === "idle";
      if (!ended) {
        state = "streaming";
        return null;
      }

      // 会话结束：按设计稿 §5.5 的优先级分类。
      queuedEnd = input.queuedSamples;
      sessionEndAt = now;
      const drainObserved = input.audioLastError === null && input.audioPhase === "ready" && input.queuedSamples === 0;
      const decodedDelta = Math.max(0, maxDecoded - decodedBaseline);

      if (input.audioLastError !== null || input.audioPhase === "failed") {
        return finish("failed", "voice.audio_delivery_failed", false, now);
      }
      if (decodedDelta === 0 && maxSubmitted === 0) {
        return finish("failed", "voice.no_samples", false, now);
      }
      if (maxSubmitted === 0 || !drainObserved) {
        return finish("failed", "voice.audio_delivery_failed", drainObserved, now);
      }
      if (manualInputSeen) {
        return finish("failed", "voice.manual_input", drainObserved, now);
      }
      if (transcriptSeen) {
        return finish("passed", null, drainObserved, now);
      }
      state = "waiting_transcript";
      return null;
    }

    if (state === "waiting_transcript") {
      // 结束后的窗口期内只需要截止时间检查；等待 notifyTranscript / notifyManualInput。
      if (sessionEndAt !== null && now - sessionEndAt > transcriptWindowMs) {
        return finish("failed", "voice.no_transcript", true, now);
      }
      return null;
    }

    return null;
  }

  function notifyTranscript(now: number): VoiceAttemptTerminal | null {
    if (terminal) return null;
    transcriptSeen = true;
    if (manualInputSeen) {
      // 手动输入优先：转写内容不能来自键盘。
      if (state !== "idle") {
        return finish("failed", "voice.manual_input", state === "waiting_transcript", now);
      }
      return null;
    }
    if (state === "waiting_transcript") {
      return finish("passed", null, true, now);
    }
    // 结束前先出现文字：等结束分类时再决定（见 observe 的 transcriptSeen 分支）。
    return null;
  }

  function notifyManualInput(now: number): VoiceAttemptTerminal | null {
    if (terminal) return null;
    manualInputSeen = true;
    if (state === "waiting_transcript" || state === "streaming" || state === "waiting_end") {
      return finish("failed", "voice.manual_input", state === "waiting_transcript", now);
    }
    return null;
  }

  function abort(code: VoiceAttemptFailureCode, now: number): VoiceAttemptTerminal | null {
    if (terminal) return null;
    if (state === "idle") return null;
    return finish("failed", code, false, now);
  }

  return {
    arm,
    observe,
    notifyTranscript,
    notifyManualInput,
    abort,
    state: () => state,
  };
}
