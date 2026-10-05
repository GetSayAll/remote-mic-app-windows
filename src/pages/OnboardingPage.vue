<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import {
  beginKeyObservation,
  completeOnboarding,
  connectRemote,
  disableRc003Capture,
  enableRc003Capture,
  endKeyObservation,
  getAudioSnapshot,
  getDiagnosticReport,
  getOnboardingState,
  getOtherVoiceHotkey,
  getRc003TaskStatus,
  getRuntimeSnapshot,
  getVokieInstallation,
  getVoiceHoldHotkey,
  getVoiceInputTool,
  isRecommendedVoiceEndpoint,
  launchVokie,
  listAudioEndpoints,
  openVbCableDownloadPage,
  openVokieHomepage,
  openWindowsSettings,
  saveOnboardingStep,
  scanPairedRemotes,
  selectAudioEndpoint,
  setMappingSuspension,
  setOtherVoiceHotkey,
  stageOnboardingVoiceBinding,
  subscribeButtonEdges,
  voiceHoldHotkeyLabel,
  type AudioEndpoint,
  type AudioSnapshot,
  type ButtonEdge,
  type KeyChord,
  type KeyCode,
  type PairedRemote,
  type Rc003TaskStatus,
  type RemoteButton,
  type RuntimeSnapshot,
  type VoiceInputTool,
  type VokieInstallation,
} from "../lib/bridge";
import { reportOnboardingEvent } from "../onboarding/diagnostics";
import {
  CONNECTED_PHASES,
  evaluateGate,
  nextStep,
  normalizeStep,
  phaseOf,
  previousStep,
  type OnboardingBlockCode,
  type OnboardingContext,
  type OnboardingStep,
} from "../onboarding/flow";
import { buildSidePanel } from "../onboarding/panel";
import { formatOnboardingDiagnostics } from "../onboarding/diagnostics-text";
import StepSide from "../components/onboarding/StepSide.vue";
import {
  createVoiceAttemptTracker,
  type VoiceAttemptFailureCode,
  type VoiceAttemptInput,
  type VoiceAttemptState,
  type VoiceAttemptTerminal,
  type VoiceAttemptTracker,
} from "../onboarding/voice-attempt";
import AudioStep from "../onboarding/steps/AudioStep.vue";
import CompleteStep from "../onboarding/steps/CompleteStep.vue";
import ControlsStep from "../onboarding/steps/ControlsStep.vue";
import RemoteStep from "../onboarding/steps/RemoteStep.vue";
import VoiceTestStep from "../onboarding/steps/VoiceTestStep.vue";
import VoiceToolStep from "../onboarding/steps/VoiceToolStep.vue";
import WelcomeStep from "../onboarding/steps/WelcomeStep.vue";
import EnhancedCaptureConfirmDialog from "../components/EnhancedCaptureConfirmDialog.vue";

const props = defineProps<{ runtime: RuntimeSnapshot | null }>();
const emit = defineEmits<{ completed: [] }>();

const step = ref<OnboardingStep>("welcome");
const stateReadFailed = ref("");
const saveMessage = ref("");
const wizardStartedAt = Math.round(performance.now());
const completing = ref(false);

// ---- 分步日志（2026-10-05 用户要求：每一步都可定位"卡在哪"）----
//
// 约定（详见 src/onboarding/diagnostics.ts）：
// - 外部调用/用户操作落 `kind=action` 对：begin=unknown → end=passed|failed；
//   begin 之后没有 end = 卡在该调用。
// - `kind=heartbeat` 每 30s 一条，携带当前 step / 门禁码 / 步骤内等待摘要，
//   是"状态未变化不重复刷"的显式例外（专门用于卡住定位）。
// - detail 一律脱敏：只允许稳定 token 与计数（设备/端点只用数量，不落 id/名称）。
function reportStepAction(
  target: OnboardingStep,
  reason: string,
  result: "passed" | "failed" | "unknown",
  details: { detail?: string; elapsedMs?: number } = {},
): void {
  reportOnboardingEvent({
    kind: "action",
    result,
    reason,
    step: target,
    ...(details.detail ? { detail: details.detail } : {}),
    ...(details.elapsedMs !== undefined ? { elapsedMs: details.elapsedMs } : {}),
  });
}

function actionElapsed(started: number): number {
  return Math.max(0, Math.round(performance.now() - started));
}

let heartbeatTimer: number | null = null;

/** 心跳里的步骤内等待摘要（稳定 token + 脱敏计数）。 */
function heartbeatDetail(): string | undefined {
  switch (step.value) {
    case "remote":
      return `obs_${remoteButtonObserved.value ? 1 : 0}_n${devices.value.length}`;
    case "controls":
      return `n${observedButtons.value.size}`;
    case "voice_test":
      return voicePhase.value;
    default:
      return undefined;
  }
}

function reportHeartbeat(): void {
  reportOnboardingEvent({
    kind: "heartbeat",
    result: "unknown",
    reason: "alive",
    step: step.value,
    code: gate.value.code ?? "ok",
    ...(heartbeatDetail() ? { detail: heartbeatDetail() } : {}),
  });
}

// ---- 遥控器步骤 ----
const devices = ref<PairedRemote[]>([]);
const scanning = ref(false);
const scanMessage = ref("");
const connectingDeviceId = ref("");
const operationMessage = ref("");
const remoteButtonObserved = ref(false);
const voiceKeyMistake = ref(false);
let buttonEdgeUnsubscribe: (() => void) | null = null;
let buttonEdgeSubscribed = false;

// ---- 语音设备步骤 ----
const audioEndpoints = ref<AudioEndpoint[]>([]);
const audioSnapshot = ref<AudioSnapshot | null>(null);
const scanningAudio = ref(false);
const audioMessage = ref("");
const selectingEndpointId = ref("");
const openingVbCablePage = ref(false);

// ---- 输入工具步骤 ----
const stagedTool = ref<VoiceInputTool | null>(null);
/** 完成页读到的正式配置（本会话未经过步骤④时的回退来源）。 */
const configuredTool = ref<VoiceInputTool | null>(null);
const currentHotkey = ref<KeyChord | null>(null);
const otherKeys = ref<KeyCode[] | null>(null);
const rc003Status = ref<Rc003TaskStatus | null>(null);
const captureBusy = ref(false);
const showCaptureConfirm = ref(false);
const captureHint = ref("");
const vokie = ref<VokieInstallation | null>(null);
const vokieBusy = ref(false);
const toolMessage = ref("");
const hotkeyLabel = computed(() =>
  currentHotkey.value ? voiceHoldHotkeyLabel(currentHotkey.value) : "不按键",
);

// ---- 普通按键体验 / 完成步骤 ----
const observedButtons = ref<Map<RemoteButton, number>>(new Map());
const observedList = computed(() =>
  [...observedButtons.value.entries()].map(([button, count]) => ({ button, count })),
);
/** 第⑤步真实验证通过（会话级事实；步骤⑤接入后由 attempt 终态置位）。 */
const voiceVerified = ref(false);
const mappingSuspended = ref(false);

/** 门禁上下文：全部来自运行快照与本向导会话内的观察，不发起任何调用。 */
const gateContext = computed<OnboardingContext>(() => {
  const platform = props.runtime?.platform;
  const recommended = audioEndpoints.value.filter(isRecommendedVoiceEndpoint);
  const selectedId =
    audioSnapshot.value?.selectedEndpointId ?? platform?.audio.selectedEndpointId ?? null;
  return {
    remote: {
      connectionPhase: platform?.connection.phase ?? "idle",
      bleVoiceReady: platform?.bleVoiceReady ?? false,
      rawInputPhase: platform?.rawInput.phase ?? "stopped",
      remoteButtonObserved: remoteButtonObserved.value,
    },
    audio: {
      recommendedEndpointAvailable: recommended.length > 0,
      selectedEndpointRecommended: recommended.some((endpoint) => endpoint.id === selectedId),
      wasapiReady:
        (audioSnapshot.value?.phase ?? platform?.audio.phase ?? "unconfigured") === "ready",
      lastError: audioSnapshot.value?.lastError ?? platform?.audio.lastError ?? null,
    },
    // 输入工具步骤：工具/授权/运行状态都来自本向导会话内的实时探测。
    voiceTool: {
      tool: stagedTool.value ?? configuredTool.value,
      doubaoCaptureEnabled: rc003Status.value?.enabled === true,
      vokieInstalled: vokie.value?.installed === true,
      vokieRunning: vokie.value?.running === true,
      otherHotkeyChosen: otherKeys.value !== null,
    },
    // 步骤⑤–⑦ 的上下文：①②④ 的真实验证在会话内由对应步骤置位。
    voiceTest: { verified: voiceVerified.value },
    controls: { distinctButtons: observedButtons.value.size },
  };
});

const gate = computed(() => evaluateGate(step.value, gateContext.value));
const blockMessage = computed(() => (gate.value.code ? BLOCK_COPY[gate.value.code] : ""));
const continueEnabled = computed(() => {
  if (!gate.value.ok) return false;
  if (step.value === "complete") return true;
  return nextStep(step.value) !== null;
});
const currentPhase = computed(() => phaseOf(step.value));

/** 阶段进度条（设计稿 2026-10-05）：只画位置感，不写步数与百分比。 */
const PROGRESS_PERCENT: Record<OnboardingStep, number> = {
  welcome: 4,
  remote: 18,
  audio: 34,
  voice_tool: 50,
  voice_test: 67,
  controls: 84,
  complete: 100,
};
const progressPercent = computed(() => PROGRESS_PERCENT[step.value]);

/** 右栏「检查卡」输入快照：全部来自既有状态，不发起调用、不改判据。 */
const sidePanel = computed(() => {
  const platform = props.runtime?.platform;
  const selectedId =
    audioSnapshot.value?.selectedEndpointId ?? platform?.audio.selectedEndpointId ?? null;
  const recommended = audioEndpoints.value.filter(isRecommendedVoiceEndpoint);
  const tool = stagedTool.value ?? configuredTool.value;
  return buildSidePanel(step.value, {
    connectionPhase: platform?.connection.phase ?? "idle",
    bleVoiceReady: platform?.bleVoiceReady ?? false,
    remoteButtonObserved: remoteButtonObserved.value,
    recommendedEndpointCount: recommended.length,
    selectedRecommended: recommended.some((endpoint) => endpoint.id === selectedId),
    wasapiReady:
      (audioSnapshot.value?.phase ?? platform?.audio.phase ?? "unconfigured") === "ready",
    audioLastError: audioSnapshot.value?.lastError ?? platform?.audio.lastError ?? null,
    scanningAudio: scanningAudio.value,
    tool,
    hotkeyReady: tool !== null && (tool !== "other" || otherKeys.value !== null),
    captureEnabled: rc003Status.value?.enabled === true,
    vokieRunning: vokie.value?.running === true,
    voicePhase: voicePhase.value,
    voiceTerminalCode: voiceResult.value?.code ?? null,
    voiceResultPassed: voiceResult.value?.result === "passed",
    voiceEvidence: voiceResult.value
      ? {
          decodedSamples: voiceResult.value.evidence.decodedSamples,
          submittedSamples: voiceResult.value.evidence.submittedSamples,
          drainObserved: voiceResult.value.evidence.drainObserved,
        }
      : null,
    distinctButtons: observedButtons.value.size,
    mappingSuspended: mappingSuspended.value,
  });
});

const BLOCK_COPY: Record<OnboardingBlockCode, string> = {
  "remote.not_connected": "先连接遥控器，再继续。",
  "remote.listener_not_ready": "按键监听正在启动，请稍等一下。",
  "remote.button_not_ready": "连接已就绪。按一下遥控器上的普通按键（比如主页键），不要按语音键。",
  "audio.install_required": "这台电脑还没有 VB-CABLE 语音设备，先安装再继续。",
  "audio.not_selected": "选择带「推荐」标记的语音设备，再继续。",
  "audio.not_ready": "语音设备没准备好；重新选择或刷新设备列表后再试。",
  "tool.not_selected": "先选择一个输入工具，再继续。",
  "tool.conflict.vokie_running": "Vokie 正在运行，它会抢先响应右 Alt；退出 Vokie 或改选 Vokie，再继续。",
  "tool.doubao.authorization_required":
    "豆包需要开启「支持更多输入工具」：打开开关并按提示完成系统授权。",
  "tool.vokie.not_installed": "没有检测到 Vokie；从官网安装后回来重新检测。",
  "tool.vokie.not_running": "Vokie 没有运行；打开它再重新检测。",
  "tool.hotkey_unset": "为「其他工具」选择语音键，再继续。",
  "voice_test.not_verified": "完成一次真实的语音上屏测试，再继续。",
  "controls.not_confirmed": "按 3 个不同的普通按键（推荐主页 / OK / 方向键），再继续。",
  "complete.runtime_regressed": "有设置不再可用，回到对应步骤修复后再完成。",
};

// 门禁日志去重：同一 (步骤, 失败码) 只在变化时各记一次；恢复只对「当前这一步」生效，
// 避免把上一步的阻断误记成新一步的恢复（LOGGING.md：状态未变化不重复刷）。
let blockedStep: OnboardingStep | null = null;
let blockedCode: string | null = null;
watch(
  () => `${step.value}|${gate.value.code ?? "ok"}`,
  (token) => {
    const [currentStep, status] = token.split("|") as [OnboardingStep, string];
    if (status === "ok") {
      if (blockedStep === currentStep) {
        blockedStep = null;
        blockedCode = null;
        reportOnboardingEvent({
          kind: "step_recovered",
          result: "passed",
          reason: "gate_satisfied",
          step: currentStep,
        });
      }
      return;
    }
    if (blockedStep === currentStep && blockedCode === status) return;
    blockedStep = currentStep;
    blockedCode = status;
    reportOnboardingEvent({
      kind: "step_blocked",
      result: "failed",
      reason: "gate_blocked",
      step: currentStep,
      code: status,
    });
  },
  { immediate: true },
);

// 步骤进入的副作用：遥控器/普通按键步骤订阅按键边沿；语音设备步骤刷新端点；
// 按键检测步骤（②遥控器 / ⑥普通按键）期间暂挂映射执行（离开恢复）；切换步骤时
// 清除误按提示。
watch(
  step,
  (current, previous) => {
    stopButtonObservation();
    voiceKeyMistake.value = false;
    // 按键检测只收集边沿，绝不能触发用户已配置的按键动作（2026-10-05 用户要求：
    // ②步原先不挂起，按下已映射的键会真的执行动作）。两处检测步骤统一挂起，
    // 离开后立即恢复。
    const observingButtons = current === "remote" || current === "controls";
    const observedButtons = previous === "remote" || previous === "controls";
    if (observedButtons && !observingButtons) {
      void setMappingSuspensionState(false);
    }
    if (previous === "voice_test" && current !== "voice_test") {
      stopVoiceTestSession();
    }
    if (current === "remote" || current === "controls") {
      void startButtonObservation();
    }
    if (current === "audio") {
      void refreshAudioDevices(true);
    }
    if (current === "voice_tool") {
      void prepareVoiceToolStep();
    }
    if (current === "voice_test") {
      void startVoiceTestSession();
    }
    if (observingButtons) {
      void setMappingSuspensionState(true);
    }
    if (current === "complete") {
      void refreshCompleteState();
    }
  },
  { immediate: true },
);

// 遥控器/普通按键步骤误按语音键的即时纠偏（语音会话状态由运行快照轮询带入）。
watch(
  () => props.runtime?.platform.connection.voiceState ?? "idle",
  (voiceState) => {
    if (voiceState === "idle") return;
    if (step.value !== "remote" && step.value !== "controls") return;
    if (voiceKeyMistake.value) return;
    if (step.value === "remote" && remoteButtonObserved.value) return;
    voiceKeyMistake.value = true;
    reportOnboardingEvent({
      kind: "remote_observed",
      result: "passed",
      reason: "voice_button_observed",
      step: step.value,
      detail: "voice",
    });
  },
);

onMounted(async () => {
  // 心跳是刻意保留的重复日志（卡住定位）；其余事件遵守"状态未变化不重复刷"。
  heartbeatTimer = window.setInterval(reportHeartbeat, 30_000);
  await restoreStep();
});

onUnmounted(() => {
  if (heartbeatTimer !== null) {
    window.clearInterval(heartbeatTimer);
    heartbeatTimer = null;
  }
  if (diagnosticsResetTimer !== null) {
    window.clearTimeout(diagnosticsResetTimer);
    diagnosticsResetTimer = null;
  }
  stopButtonObservation();
  stopVoiceTestSession();
  // 安全网：向导卸载（完成/退出）时恢复映射执行；进程退出后内存态自然复位。
  if (mappingSuspended.value) {
    void setMappingSuspension(false);
  }
});

async function restoreStep(): Promise<void> {
  stateReadFailed.value = "";
  const started = performance.now();
  reportOnboardingEvent({
    kind: "started",
    result: "unknown",
    reason: "state_read_requested",
  });
  try {
    const state = await getOnboardingState();
    if (!state.isActive) {
      reportOnboardingEvent({
        kind: "started",
        result: "passed",
        reason: "already_completed",
        elapsedMs: actionElapsed(started),
      });
      emit("completed");
      return;
    }
    const restored = normalizeStep(state.step);
    reportOnboardingEvent({
      kind: "started",
      result: "passed",
      reason: "restored",
      step: restored,
      elapsedMs: actionElapsed(started),
    });
    await setStep(restored, "restore");
  } catch (error) {
    stateReadFailed.value = error instanceof Error ? error.message : String(error);
    reportOnboardingEvent({
      kind: "started",
      result: "failed",
      reason: "state_read_failed",
      elapsedMs: actionElapsed(started),
    });
  }
}

async function setStep(
  target: OnboardingStep,
  reason: "continue" | "back" | "restore" | "fix",
): Promise<void> {
  step.value = target;
  reportOnboardingEvent({ kind: "step_entered", result: "passed", reason, step: target });
  try {
    await saveOnboardingStep(target);
    saveMessage.value = "";
  } catch (error) {
    saveMessage.value = error instanceof Error ? error.message : String(error);
    reportOnboardingEvent({
      kind: "persist",
      result: "failed",
      reason: "step_save_failed",
      step: target,
    });
  }
}

async function onContinue(): Promise<void> {
  if (!continueEnabled.value) return;
  reportOnboardingEvent({
    kind: "step_passed",
    result: "passed",
    reason: "user_continue",
    step: step.value,
  });
  const target = nextStep(step.value);
  if (target) {
    await setStep(target, "continue");
  }
}

async function onBack(): Promise<void> {
  const target = previousStep(step.value);
  if (!target) return;
  reportOnboardingEvent({
    kind: "navigation",
    result: "passed",
    reason: "user_back",
    step: step.value,
    detail: target,
  });
  await setStep(target, "back");
}

// ---- 遥控器 / 普通按键步骤：边沿观察 ----

async function startButtonObservation(): Promise<void> {
  if (buttonEdgeSubscribed) return;
  buttonEdgeSubscribed = true;
  const started = performance.now();
  reportStepAction(step.value, "button_observation", "unknown");
  try {
    const unsubscribe = await subscribeButtonEdges(handleButtonEdge);
    if (step.value !== "remote" && step.value !== "controls") {
      unsubscribe();
      buttonEdgeSubscribed = false;
      reportStepAction(step.value, "button_observation", "passed", {
        detail: "detached",
        elapsedMs: actionElapsed(started),
      });
      return;
    }
    buttonEdgeUnsubscribe = unsubscribe;
    reportStepAction(step.value, "button_observation", "passed", {
      elapsedMs: actionElapsed(started),
    });
  } catch {
    buttonEdgeSubscribed = false;
    reportStepAction(step.value, "button_observation", "failed", {
      elapsedMs: actionElapsed(started),
    });
  }
}

function handleButtonEdge(edge: ButtonEdge): void {
  if (!edge.isPressed) return;
  if (step.value === "remote") {
    if (remoteButtonObserved.value) return;
    remoteButtonObserved.value = true;
    voiceKeyMistake.value = false;
    reportOnboardingEvent({
      kind: "remote_observed",
      result: "passed",
      reason: "control_button",
      step: "remote",
      detail: edge.button,
    });
    return;
  }
  if (step.value === "controls") {
    const counts = observedButtons.value;
    const next = (counts.get(edge.button) ?? 0) + 1;
    counts.set(edge.button, next);
    if (voiceKeyMistake.value) voiceKeyMistake.value = false;
    // 每次观察都落日志：是否计入新的不同按键由 reason 区分（映射暂挂期间只观察、不注入）。
    reportOnboardingEvent({
      kind: "remote_observed",
      result: "passed",
      reason: next === 1 ? "control_button" : "control_button_repeat",
      step: "controls",
      detail: edge.button,
    });
  }
}

function stopButtonObservation(): void {
  buttonEdgeSubscribed = false;
  if (buttonEdgeUnsubscribe) {
    buttonEdgeUnsubscribe();
    buttonEdgeUnsubscribe = null;
  }
}

async function scanRemotes(): Promise<void> {
  scanning.value = true;
  scanMessage.value = "正在寻找小米遥控器…";
  const started = performance.now();
  reportStepAction("remote", "scan_requested", "unknown");
  try {
    devices.value = await scanPairedRemotes();
    scanMessage.value = devices.value.length
      ? `找到 ${devices.value.length} 个已配对的小米遥控器`
      : "没有找到已配对的小米遥控器。先在 Windows 里配对，再回来扫描。";
    reportStepAction("remote", "scan_requested", "passed", {
      detail: `found_${devices.value.length}`,
      elapsedMs: actionElapsed(started),
    });
  } catch (error) {
    devices.value = [];
    scanMessage.value = error instanceof Error ? error.message : String(error);
    reportStepAction("remote", "scan_requested", "failed", {
      elapsedMs: actionElapsed(started),
    });
  } finally {
    scanning.value = false;
  }
}

async function connectDevice(device: PairedRemote): Promise<void> {
  connectingDeviceId.value = device.id;
  operationMessage.value = "";
  const started = performance.now();
  // 脱敏：device.id / 名称不进日志，只记录连接请求与结果。
  reportStepAction("remote", "connect_requested", "unknown");
  try {
    await connectRemote(device.id);
    operationMessage.value = "正在确认语音功能，就绪后按一下普通按键。";
    reportStepAction("remote", "connect_requested", "passed", {
      elapsedMs: actionElapsed(started),
    });
  } catch (error) {
    operationMessage.value = error instanceof Error ? error.message : String(error);
    reportStepAction("remote", "connect_requested", "failed", {
      elapsedMs: actionElapsed(started),
    });
  } finally {
    connectingDeviceId.value = "";
  }
}

async function openBluetoothSettings(): Promise<void> {
  const started = performance.now();
  reportStepAction("remote", "open_bluetooth_settings", "unknown");
  try {
    await openWindowsSettings("bluetooth");
    operationMessage.value = "已打开系统蓝牙设置；配对完成后回来点「扫描已配对设备」。";
    reportStepAction("remote", "open_bluetooth_settings", "passed", {
      elapsedMs: actionElapsed(started),
    });
  } catch (error) {
    operationMessage.value = error instanceof Error ? error.message : String(error);
    reportStepAction("remote", "open_bluetooth_settings", "failed", {
      elapsedMs: actionElapsed(started),
    });
  }
}

// ---- 语音设备步骤 ----

/** 语音设备路由状态（脱敏：只有状态 token 与计数，不含端点 id/名称）。 */
let lastAudioRouteToken = "";
function reportAudioRoute(): void {
  const recommended = audioEndpoints.value.filter(isRecommendedVoiceEndpoint);
  const selectedId =
    audioSnapshot.value?.selectedEndpointId ??
    props.runtime?.platform.audio.selectedEndpointId ??
    null;
  const ready = (audioSnapshot.value?.phase ?? "unconfigured") === "ready";
  const reason =
    recommended.length === 0
      ? "missing"
      : !selectedId
        ? "not_selected"
        : ready
          ? "ready"
          : "selected";
  const token = `${reason}_${recommended.length}_${audioEndpoints.value.length}`;
  if (token === lastAudioRouteToken) return;
  lastAudioRouteToken = token;
  reportOnboardingEvent({
    kind: "audio_route",
    result: reason === "ready" ? "passed" : "unknown",
    reason,
    step: "audio",
    detail: `rec_${recommended.length}_total_${audioEndpoints.value.length}`,
  });
}

async function refreshAudioDevices(autoSelect: boolean): Promise<void> {
  scanningAudio.value = true;
  audioMessage.value = "正在读取语音设备…";
  const started = performance.now();
  const mode = autoSelect ? "auto" : "manual";
  reportStepAction("audio", "refresh_endpoints", "unknown", { detail: mode });
  try {
    audioEndpoints.value = await listAudioEndpoints();
    audioSnapshot.value = await getAudioSnapshot();
    const recommended = audioEndpoints.value.filter(isRecommendedVoiceEndpoint);
    const hasSelection = Boolean(audioSnapshot.value.selectedEndpointId);
    if (autoSelect && !hasSelection && recommended.length === 1) {
      await chooseEndpoint(recommended[0], true);
    } else {
      audioMessage.value = recommended.length
        ? `已检测到 ${recommended.length} 个 VB-CABLE 语音设备`
        : "没有检测到 VB-CABLE；安装完成后要重启电脑，再回来重新检测";
    }
    reportAudioRoute();
    reportStepAction("audio", "refresh_endpoints", "passed", {
      detail: `${mode}_total_${audioEndpoints.value.length}_rec_${recommended.length}`,
      elapsedMs: actionElapsed(started),
    });
  } catch (error) {
    audioEndpoints.value = [];
    audioMessage.value = error instanceof Error ? error.message : String(error);
    reportStepAction("audio", "refresh_endpoints", "failed", {
      elapsedMs: actionElapsed(started),
    });
  } finally {
    scanningAudio.value = false;
  }
}

async function chooseEndpoint(endpoint: AudioEndpoint, automatic = false): Promise<void> {
  selectingEndpointId.value = endpoint.id;
  audioMessage.value = "正在打开语音设备…";
  const started = performance.now();
  const mode = automatic ? "auto" : "manual";
  // 脱敏：endpoint.id / 名称不进日志；mode 与回退标记为稳定 token。
  reportStepAction("audio", "select_endpoint", "unknown", { detail: mode });
  try {
    const snapshot = await selectAudioEndpoint(endpoint.id);
    audioSnapshot.value = snapshot;
    const actualName = snapshot.selectedEndpointName ?? endpoint.name;
    audioMessage.value =
      actualName === endpoint.name
        ? automatic
          ? `已自动选择 ${actualName}`
          : `已选择 ${actualName}`
        : `已自动改用 ${actualName}（${endpoint.name} 暂时打不开）`;
    reportStepAction("audio", "select_endpoint", "passed", {
      detail: actualName === endpoint.name ? mode : "fallback",
      elapsedMs: actionElapsed(started),
    });
  } catch (error) {
    audioMessage.value = error instanceof Error ? error.message : String(error);
    try {
      audioSnapshot.value = await getAudioSnapshot();
    } catch {
      // 读取失败保持旧快照；错误已在上面显示。
    }
    reportStepAction("audio", "select_endpoint", "failed", {
      elapsedMs: actionElapsed(started),
    });
  } finally {
    selectingEndpointId.value = "";
    reportAudioRoute();
  }
}

async function openDownloadPage(): Promise<void> {
  openingVbCablePage.value = true;
  const started = performance.now();
  reportStepAction("audio", "open_download_page", "unknown");
  try {
    await openVbCableDownloadPage();
    audioMessage.value = "已打开官方下载页；安装需要管理员权限，完成后请重启电脑";
    reportStepAction("audio", "open_download_page", "passed", {
      elapsedMs: actionElapsed(started),
    });
  } catch (error) {
    audioMessage.value = error instanceof Error ? error.message : String(error);
    reportStepAction("audio", "open_download_page", "failed", {
      elapsedMs: actionElapsed(started),
    });
  } finally {
    openingVbCablePage.value = false;
  }
}

// ---- 输入工具步骤 ----

/** 各工具的固定语音键（与连接页同一口径：选卡即自动落该组合）。 */
function hotkeyForTool(tool: VoiceInputTool): KeyChord | null {
  switch (tool) {
    case "doubao":
    case "vokie":
      return { keys: ["right_alt"] };
    case "wechat":
      return { keys: ["left_control", "left_windows"] };
    case "other": {
      const keys = otherKeys.value ?? [];
      return keys.length ? { keys } : null;
    }
  }
}

async function prepareVoiceToolStep(): Promise<void> {
  toolMessage.value = "";
  const started = performance.now();
  // 该调用同时服务第④步与第⑤步（重启后直接落在第⑤步时补读配置），故按当前步骤记录。
  reportStepAction(step.value, "read_tool_state", "unknown");
  try {
    const [tool, hotkey, other, rc003, vokieState] = await Promise.all([
      getVoiceInputTool(),
      getVoiceHoldHotkey(),
      getOtherVoiceHotkey(),
      getRc003TaskStatus().catch(() => null),
      getVokieInstallation().catch(() => null),
    ]);
    stagedTool.value = tool;
    currentHotkey.value = hotkey;
    otherKeys.value = other;
    rc003Status.value = rc003;
    vokie.value = vokieState;
    reportStepAction(step.value, "read_tool_state", "passed", {
      elapsedMs: actionElapsed(started),
    });
  } catch (error) {
    toolMessage.value = error instanceof Error ? error.message : String(error);
    reportStepAction(step.value, "read_tool_state", "failed", {
      elapsedMs: actionElapsed(started),
    });
  }
}

/**
 * 选卡即暂存：落回滚快照 + 应用正式配置与运行时（Rust 事务），保证第⑤步
 * 真实验证可用；退出未完成流程 / 重跑向导会回滚（设计稿 §5.4）。
 */
async function selectTool(tool: VoiceInputTool): Promise<void> {
  if (tool === "other" && otherKeys.value === null) {
    stagedTool.value = "other";
    toolMessage.value = "先为「其他工具」选一个语音键。";
    void refreshVokieState();
    return;
  }
  const hotkey = hotkeyForTool(tool);
  const previous = stagedTool.value;
  stagedTool.value = tool;
  currentHotkey.value = hotkey;
  toolMessage.value = "";
  const started = performance.now();
  reportStepAction("voice_tool", "stage_binding", "unknown", { detail: tool });
  try {
    await stageOnboardingVoiceBinding(tool, hotkey);
    reportStepAction("voice_tool", "stage_binding", "passed", {
      detail: tool,
      elapsedMs: actionElapsed(started),
    });
    reportOnboardingEvent({
      kind: "tool_selected",
      result: "passed",
      reason: "staged",
      step: "voice_tool",
      detail: tool,
    });
  } catch (error) {
    stagedTool.value = previous;
    toolMessage.value = error instanceof Error ? error.message : String(error);
    reportStepAction("voice_tool", "stage_binding", "failed", {
      detail: tool,
      elapsedMs: actionElapsed(started),
    });
    return;
  }
  // 豆包/Vokie 同键（右 Alt）：选中即刷新 Vokie 运行状态，冲突要立刻可见。
  if (tool === "doubao" || tool === "vokie") {
    void refreshVokieState();
  }
}

async function chooseOtherKeys(keys: KeyCode[]): Promise<void> {
  otherKeys.value = keys;
  const keyDetail = keys.length ? "with_keys" : "disabled";
  const saveStarted = performance.now();
  reportStepAction("voice_tool", "save_other_keys", "unknown", { detail: keyDetail });
  try {
    await setOtherVoiceHotkey(keys);
    reportStepAction("voice_tool", "save_other_keys", "passed", {
      detail: keyDetail,
      elapsedMs: actionElapsed(saveStarted),
    });
  } catch (error) {
    toolMessage.value = error instanceof Error ? error.message : String(error);
    reportStepAction("voice_tool", "save_other_keys", "failed", {
      detail: keyDetail,
      elapsedMs: actionElapsed(saveStarted),
    });
  }
  const started = performance.now();
  reportStepAction("voice_tool", "stage_binding", "unknown", { detail: "other" });
  try {
    await stageOnboardingVoiceBinding("other", keys.length ? { keys } : null);
    currentHotkey.value = keys.length ? { keys } : null;
    reportStepAction("voice_tool", "stage_binding", "passed", {
      detail: "other",
      elapsedMs: actionElapsed(started),
    });
    reportOnboardingEvent({
      kind: "tool_selected",
      result: "passed",
      reason: "other_keys",
      step: "voice_tool",
      detail: keys.length ? "with_keys" : "disabled",
    });
  } catch (error) {
    toolMessage.value = error instanceof Error ? error.message : String(error);
    reportStepAction("voice_tool", "stage_binding", "failed", {
      detail: "other",
      elapsedMs: actionElapsed(started),
    });
  }
}

function requestCaptureToggle(): void {
  if (captureBusy.value) return;
  // 每次开启都先弹确认（2026-10-03 定稿）；关闭方向直接执行。
  if (rc003Status.value?.enabled !== true) {
    showCaptureConfirm.value = true;
    return;
  }
  void applyCaptureToggle();
}

async function applyCaptureToggle(): Promise<void> {
  if (captureBusy.value) return;
  captureBusy.value = true;
  const wasEnabled = rc003Status.value?.enabled === true;
  reportOnboardingEvent({
    kind: "authorization",
    result: "unknown",
    reason: wasEnabled ? "disable_requested" : "enable_requested",
    step: "voice_tool",
    detail: "doubao",
  });
  try {
    const next = wasEnabled ? await disableRc003Capture() : await enableRc003Capture();
    rc003Status.value = next;
    if (next.lastError) {
      captureHint.value = next.lastError;
      reportOnboardingEvent({
        kind: "authorization",
        result: "failed",
        reason: "switch_failed",
        step: "voice_tool",
        detail: "doubao",
      });
      return;
    }
    captureHint.value = "";
    reportOnboardingEvent({
      kind: "authorization",
      result: "passed",
      reason: next.enabled ? "enabled" : "disabled",
      step: "voice_tool",
      detail: "doubao",
    });
  } catch (error) {
    captureHint.value = error instanceof Error ? error.message : String(error);
    try {
      rc003Status.value = await getRc003TaskStatus();
    } catch {
      // 读取失败保持旧状态；错误已在上面显示。
    }
    reportOnboardingEvent({
      kind: "authorization",
      result: "failed",
      reason: "switch_error",
      step: "voice_tool",
      detail: "doubao",
    });
  } finally {
    captureBusy.value = false;
  }
}

function confirmCaptureDialog(): void {
  showCaptureConfirm.value = false;
  void applyCaptureToggle();
}

function closeCaptureDialog(): void {
  showCaptureConfirm.value = false;
}

async function refreshVokieState(): Promise<void> {
  const started = performance.now();
  reportStepAction("voice_tool", "vokie_detect", "unknown");
  try {
    vokie.value = await getVokieInstallation();
    reportStepAction("voice_tool", "vokie_detect", "passed", {
      detail: `installed_${vokie.value?.installed ? 1 : 0}_running_${vokie.value?.running ? 1 : 0}`,
      elapsedMs: actionElapsed(started),
    });
  } catch (error) {
    toolMessage.value = error instanceof Error ? error.message : String(error);
    reportStepAction("voice_tool", "vokie_detect", "failed", {
      elapsedMs: actionElapsed(started),
    });
  }
}

async function openVokieSite(): Promise<void> {
  const started = performance.now();
  reportStepAction("voice_tool", "open_vokie_site", "unknown");
  try {
    await openVokieHomepage();
    toolMessage.value = "已打开 Vokie 官网；安装后回来点「重新检测」。";
    reportStepAction("voice_tool", "open_vokie_site", "passed", {
      elapsedMs: actionElapsed(started),
    });
  } catch (error) {
    toolMessage.value = error instanceof Error ? error.message : String(error);
    reportStepAction("voice_tool", "open_vokie_site", "failed", {
      elapsedMs: actionElapsed(started),
    });
  }
}

async function launchVokieApp(): Promise<void> {
  vokieBusy.value = true;
  const started = performance.now();
  reportStepAction("voice_tool", "vokie_launch", "unknown");
  try {
    await launchVokie();
    toolMessage.value = "已发出启动请求；启动后点「重新检测」。";
    reportStepAction("voice_tool", "vokie_launch", "passed", {
      elapsedMs: actionElapsed(started),
    });
  } catch (error) {
    toolMessage.value = error instanceof Error ? error.message : String(error);
    reportStepAction("voice_tool", "vokie_launch", "failed", {
      elapsedMs: actionElapsed(started),
    });
  } finally {
    vokieBusy.value = false;
  }
}

// ---- 普通按键体验 / 完成步骤 ----

async function setMappingSuspensionState(suspended: boolean): Promise<void> {
  if (mappingSuspended.value === suspended) return;
  mappingSuspended.value = suspended;
  const started = performance.now();
  const reason = suspended ? "mapping_suspend" : "mapping_resume";
  reportStepAction(step.value, reason, "unknown");
  try {
    await setMappingSuspension(suspended);
    reportStepAction(step.value, reason, "passed", { elapsedMs: actionElapsed(started) });
  } catch (error) {
    saveMessage.value = error instanceof Error ? error.message : String(error);
    reportStepAction(step.value, reason, "failed", { elapsedMs: actionElapsed(started) });
  }
}

/** 完成页重查关键运行条件：正式输入工具、Vokie 运行态、全按键开关、音频端点。 */
async function refreshCompleteState(): Promise<void> {
  const started = performance.now();
  reportStepAction("complete", "complete_refresh", "unknown");
  let failedSource: string | null = null;
  const trackFailure = (source: string) => {
    if (!failedSource) failedSource = source;
  };
  try {
    configuredTool.value = await getVoiceInputTool();
  } catch {
    // 读取失败保持旧值；门禁会显示未就绪，不给用户虚假的「完成」。
    trackFailure("tool");
  }
  try {
    vokie.value = await getVokieInstallation();
  } catch {
    // 同上。
    trackFailure("vokie");
  }
  try {
    rc003Status.value = await getRc003TaskStatus();
  } catch {
    // 同上。
    trackFailure("capture");
  }
  try {
    audioEndpoints.value = await listAudioEndpoints();
    audioSnapshot.value = await getAudioSnapshot();
  } catch {
    // 同上。
    trackFailure("audio");
  }
  // detail=首个读取失败的来源 token（tool|vokie|capture|audio），不落错误原文。
  reportStepAction("complete", "complete_refresh", failedSource ? "failed" : "passed", {
    detail: failedSource ?? undefined,
    elapsedMs: actionElapsed(started),
  });
}

const completeChecks = computed(() => {
  const platform = props.runtime?.platform;
  const remoteReady =
    CONNECTED_PHASES.includes(platform?.connection.phase ?? "idle") &&
    (platform?.bleVoiceReady ?? false);
  return [
    { label: "遥控器已连接，语音按键可用", ok: remoteReady },
    { label: "语音设备已就绪", ok: evaluateGate("audio", gateContext.value).ok },
    { label: "输入工具已就绪", ok: evaluateGate("voice_tool", gateContext.value).ok },
    { label: "按住说话验证已通过", ok: voiceVerified.value },
  ];
});

const regressedStep = computed<OnboardingStep | null>(() => {
  if (step.value !== "complete") return null;
  const code = gate.value.code;
  if (!code) return null;
  if (code.startsWith("remote.")) return "remote";
  if (code.startsWith("audio.")) return "audio";
  if (code.startsWith("tool.")) return "voice_tool";
  if (code.startsWith("voice_test.")) return "voice_test";
  return null;
});

async function onFixStep(): Promise<void> {
  const target = regressedStep.value;
  if (!target) return;
  reportOnboardingEvent({
    kind: "navigation",
    result: "passed",
    reason: "fix_regression",
    step: "complete",
    detail: target,
  });
  await setStep(target, "fix");
}

async function onStartUsing(): Promise<void> {
  if (completing.value || step.value !== "complete" || !gate.value.ok) return;
  completing.value = true;
  try {
    // 提交 staged 语音绑定（Rust 事务）并标记完成；随后交回主界面。
    await completeOnboarding();
    reportOnboardingEvent({
      kind: "completed",
      result: "passed",
      reason: "wizard_finished",
      step: "complete",
      elapsedMs: Math.round(performance.now() - wizardStartedAt),
    });
    emit("completed");
  } catch (error) {
    saveMessage.value = error instanceof Error ? error.message : String(error);
    reportOnboardingEvent({
      kind: "completed",
      result: "failed",
      reason: "complete_save_failed",
    });
  } finally {
    completing.value = false;
  }
}

// ---- 复制诊断信息（2026-10-05 用户要求：过不去时一次复制即可报障）----

const diagnosticsCopyState = ref<"idle" | "busy" | "copied" | "failed">("idle");
const diagnosticsCopyMessage = ref("");
let diagnosticsResetTimer: number | null = null;

const diagnosticsCopyText = computed(() => {
  if (diagnosticsCopyState.value === "busy") return "正在整理…";
  if (diagnosticsCopyState.value === "copied") return "已复制到剪贴板";
  return "复制诊断信息";
});

/**
 * 复制给支持人员的诊断块：Rust 侧 `get_diagnostic_report`（版本 / Build / Windows
 * 版本 / 架构，全部真实读回）+ 向导当前状态（步骤、门禁、连接 / 音频 / 工具）。
 * 只含稳定 token、计数与布尔；设备与端点身份一律不进（见 diagnostics-text.ts）。
 */
async function copyDiagnostics(): Promise<void> {
  if (diagnosticsCopyState.value === "busy") return;
  const started = performance.now();
  const target = step.value;
  diagnosticsCopyState.value = "busy";
  diagnosticsCopyMessage.value = "";
  reportStepAction(target, "copy_diagnostics", "unknown");
  try {
    const report = await getDiagnosticReport();
    const platform = props.runtime?.platform;
    // 音频端点先做一次**只读**刷新（不触发自动选中）：向导只在进入第③步时
    // 加载过列表，沿用页面状态会让第①/②步复制出的「推荐设备」恒为 0，误导
    // 支持判断。读取失败退回页面在用状态——复制本身不因此失败。
    let endpoints = audioEndpoints.value;
    let audio = audioSnapshot.value;
    try {
      [endpoints, audio] = await Promise.all([listAudioEndpoints(), getAudioSnapshot()]);
    } catch {
      // 保持页面状态。
    }
    const recommended = endpoints.filter(isRecommendedVoiceEndpoint);
    const selectedId = audio?.selectedEndpointId ?? platform?.audio.selectedEndpointId ?? null;
    const chord =
      currentHotkey.value ?? (stagedTool.value ? hotkeyForTool(stagedTool.value) : null);
    const hotkeyTokens = chord
      ? chord.keys.length > 0
        ? chord.keys.join("+")
        : "none"
      : "unset";
    const text = formatOnboardingDiagnostics(report, {
      step: target,
      gateCode: gate.value.code,
      blocked: !gate.value.ok,
      model: platform?.connection.remoteModel ?? "unknown",
      connectionPhase: platform?.connection.phase ?? "idle",
      bleVoiceReady: platform?.bleVoiceReady ?? false,
      rawInputPhase: platform?.rawInput.phase ?? "stopped",
      reconnectAttempt: platform?.connection.reconnectAttempt ?? 0,
      audioPhase: audio?.phase ?? platform?.audio.phase ?? "unconfigured",
      recommendedEndpointCount: recommended.length,
      selectedRecommended: recommended.some((endpoint) => endpoint.id === selectedId),
      queuedSamples: audio?.queuedSamples ?? platform?.audio.queuedSamples ?? 0,
      tool: stagedTool.value ?? configuredTool.value ?? "unset",
      hotkey: hotkeyTokens,
      captureEnabled: rc003Status.value?.enabled === true,
      vokieInstalled: vokie.value?.installed === true,
      vokieRunning: vokie.value?.running === true,
      distinctButtonCount: observedButtons.value.size,
      mappingSuspended: mappingSuspended.value,
    });
    if (!navigator.clipboard?.writeText) throw new Error("当前环境不支持剪贴板写入");
    await navigator.clipboard.writeText(text);
    diagnosticsCopyState.value = "copied";
    diagnosticsCopyMessage.value = "已复制，可直接粘贴发给开发者";
    reportStepAction(target, "copy_diagnostics", "passed", {
      detail: `chars_${text.length}`,
      elapsedMs: actionElapsed(started),
    });
    if (diagnosticsResetTimer !== null) window.clearTimeout(diagnosticsResetTimer);
    diagnosticsResetTimer = window.setTimeout(() => {
      diagnosticsCopyState.value = "idle";
      diagnosticsCopyMessage.value = "";
      diagnosticsResetTimer = null;
    }, 4000);
  } catch (error) {
    diagnosticsCopyState.value = "failed";
    diagnosticsCopyMessage.value = `复制失败：${
      error instanceof Error ? error.message : String(error)
    }`;
    reportStepAction(target, "copy_diagnostics", "failed", {
      detail: "clipboard",
      elapsedMs: actionElapsed(started),
    });
  }
}

// ---- 按住说话验证（第⑤步）----

/** 键位名 → Windows 虚拟键码（观察窗口排除集；与捕获协议同口径）。 */
const VOICE_KEY_CODE_VKS: Record<string, number> = {
  control: 0x11,
  left_control: 0xa2,
  right_control: 0xa3,
  shift: 0x10,
  left_shift: 0xa0,
  right_shift: 0xa1,
  alt: 0x12,
  left_alt: 0xa4,
  right_alt: 0xa5,
  left_windows: 0x5b,
  right_windows: 0x5c,
  enter: 0x0d,
  escape: 0x1b,
  space: 0x20,
  tab: 0x09,
  apps: 0x5d,
};

/** 失败码 → 「发生了什么 + 你该怎么办」（设计稿 §7；不贴内部错误原文）。 */
const VOICE_FAILURE_COPY: Record<VoiceAttemptFailureCode, string> = {
  "voice.session_not_started":
    "没有检测到语音会话。确认遥控器已就绪，按住语音键不要放，再试一次。",
  "voice.no_samples": "收到了语音会话，但没有声音数据。对着遥控器正常说话，再试一次。",
  "voice.audio_delivery_failed":
    "声音没有完整送达语音设备。检查设备是否被其他程序占用，再试一次。",
  "voice.session_not_ended": "松开后这次语音没有正常结束。稍等一下或断开重连遥控器，再试一次。",
  "voice.no_transcript":
    "语音链路正常，但输入工具没有写出文字。检查工具麦克风是否选 CABLE Output、工具里的语音键是否和本次设置一致、工具是否在运行。",
  "voice.manual_input": "检测到键盘输入。这一步请只用遥控器语音键，不要用键盘打字。",
  "voice.input_target_not_ready": "输入框没有聚焦。先点一下输入框，再按住遥控器语音键。",
  "voice.focus_lost": "测试过程中输入框失去了焦点。点回输入框，再试一次。",
};

const VOICE_POLL_MS = 200;

const voiceTestRef = ref<InstanceType<typeof VoiceTestStep> | null>(null);
const voicePhase = ref<VoiceAttemptState>("waiting_start");
const voiceResult = ref<VoiceAttemptTerminal | null>(null);
const voiceFailureMessage = ref("");
const voiceFocused = ref(false);
let voiceTracker: VoiceAttemptTracker | null = null;
let voicePollTimer: number | null = null;
let voicePollBusy = false;
let voiceObservationId = 0;
let voiceAttemptSerial = 0;
let voiceAttemptCount = 0;
let voiceSnapshotErrorLogged = false;

const voiceToolLabel = computed(() => {
  switch (stagedTool.value) {
    case "doubao":
      return "豆包输入法";
    case "wechat":
      return "微信输入法";
    case "vokie":
      return "Vokie";
    case "other":
      return "其他工具";
    default:
      return "你选的输入工具";
  }
});

const voiceAudioText = computed(() => {
  const name =
    audioSnapshot.value?.selectedEndpointName ??
    props.runtime?.platform.audio.selectedEndpointName ??
    "";
  const phase = audioSnapshot.value?.phase ?? props.runtime?.platform.audio.phase ?? "unconfigured";
  if (!name) return "未选择";
  return `${name}（${phase === "ready" ? "就绪" : "未就绪"}）`;
});

function voiceAttemptInput(snapshot: RuntimeSnapshot): VoiceAttemptInput {
  const platform = snapshot.platform;
  return {
    voiceState: platform.connection.voiceState,
    decodedSamples: platform.connection.decodedSamples,
    audioPhase: platform.audio.phase,
    submittedSamples: platform.audio.submittedSamples,
    queuedSamples: platform.audio.queuedSamples,
    audioLastError: platform.audio.lastError,
    generation: platform.connection.generation,
  };
}

/** 报告层合成会把「按住说话」和弦以非注入形态送进 OS：它不算手动输入，排除。 */
function observationExcludeVks(): number[] {
  const chord = currentHotkey.value ?? (stagedTool.value ? hotkeyForTool(stagedTool.value) : null);
  return (chord?.keys ?? [])
    .map((key) => VOICE_KEY_CODE_VKS[key])
    .filter((vk): vk is number => vk !== undefined);
}

async function focusVoiceBox(): Promise<void> {
  await nextTick();
  voiceTestRef.value?.focusBox();
}

async function beginVoiceObservation(): Promise<void> {
  const serial = voiceAttemptSerial;
  try {
    const id = await beginKeyObservation(observationExcludeVks());
    if (serial !== voiceAttemptSerial) return;
    voiceObservationId = id;
    if (id === 0) {
      // 观察不可用：手动输入检测按未知处理（fail-open），不误判用户。
      reportOnboardingEvent({
        kind: "voice_attempt",
        result: "unknown",
        reason: "observation_unavailable",
        step: "voice_test",
      });
    }
  } catch {
    if (serial === voiceAttemptSerial) {
      voiceObservationId = 0;
      // 开窗 IPC 失败：手动输入检测按未知处理（fail-open），但必须留痕，
      // 否则"没有观察窗口"这类卡点无从定位。
      reportOnboardingEvent({
        kind: "voice_attempt",
        result: "unknown",
        reason: "observation_begin_failed",
        step: "voice_test",
      });
    }
  }
}

async function closeVoiceObservation(): Promise<void> {
  const id = voiceObservationId;
  if (id === 0) return;
  voiceObservationId = 0;
  try {
    await endKeyObservation(id);
  } catch {
    // 关闭失败无后续影响：窗口是进程内状态，重试会重开。
  }
}

async function startVoiceTestSession(): Promise<void> {
  stopVoiceTestSession();
  voiceAttemptSerial += 1;
  voiceResult.value = null;
  voiceFailureMessage.value = "";
  voicePhase.value = "waiting_start";
  voiceSnapshotErrorLogged = false;
  if (!stagedTool.value) {
    // 应用重启后直接落在第⑤步：读一次正式配置，供核对卡与排除集使用。
    await prepareVoiceToolStep();
  }
  voiceAttemptCount += 1;
  let snapshot: RuntimeSnapshot;
  try {
    snapshot = await getRuntimeSnapshot();
  } catch (error) {
    voiceFailureMessage.value = error instanceof Error ? error.message : String(error);
    reportOnboardingEvent({
      kind: "voice_attempt",
      result: "failed",
      reason: "snapshot_read_failed",
      step: "voice_test",
      detail: "session_start",
    });
    return;
  }
  const tracker = createVoiceAttemptTracker({ attemptId: voiceAttemptCount });
  tracker.arm(voiceAttemptInput(snapshot), Date.now());
  voiceTracker = tracker;
  reportOnboardingEvent({
    kind: "voice_attempt",
    result: "unknown",
    reason: "armed",
    step: "voice_test",
    detail: `id_${voiceAttemptCount}`,
  });
  await beginVoiceObservation();
  if (voiceTracker !== tracker) return;
  voicePollTimer = window.setInterval(() => void pollVoiceTest(), VOICE_POLL_MS);
  void focusVoiceBox();
}

function stopVoiceTestSession(): void {
  // 只把"尚未终结的 attempt"记成中止；已到终态的会话离开步骤不算中止。
  const hadActiveSession = voiceTracker !== null && voiceTracker.state() !== "terminal";
  voiceAttemptSerial += 1;
  if (voicePollTimer !== null) {
    window.clearInterval(voicePollTimer);
    voicePollTimer = null;
  }
  if (voiceObservationId !== 0) {
    void endKeyObservation(voiceObservationId);
    voiceObservationId = 0;
  }
  voiceTracker = null;
  voicePollBusy = false;
  if (hadActiveSession) {
    // 会话被中止（离开第⑤步/重开）：attempt 无终态就结束，日志记 unknown 便于定位。
    reportOnboardingEvent({
      kind: "voice_attempt",
      result: "unknown",
      reason: "session_stopped",
      step: "voice_test",
    });
  }
}

/**
 * 在任何「通过」判定前结算手动输入检测：关闭当前观察窗口取回计数。
 * 计数 > 0 → 手动输入（终止 attempt）；None = 计量不可靠（fail-open）。
 * 尝试未终结时重开新窗口，覆盖后续阶段（转写窗口与会话收尾）。
 */
async function settleVoiceManualInput(now: number): Promise<void> {
  const tracker = voiceTracker;
  const id = voiceObservationId;
  if (!tracker || id === 0) return;
  voiceObservationId = 0;
  const serial = voiceAttemptSerial;
  let count: number | null = null;
  try {
    count = await endKeyObservation(id);
  } catch {
    count = null;
  }
  if (serial !== voiceAttemptSerial || voiceTracker !== tracker) return;
  if (count === null) {
    reportOnboardingEvent({
      kind: "voice_attempt",
      result: "unknown",
      reason: "observation_unreliable",
      step: "voice_test",
    });
  } else if (count > 0) {
    const terminal = tracker.notifyManualInput(now);
    if (terminal) onVoiceTerminal(terminal);
  }
  if (tracker.state() !== "terminal") {
    await beginVoiceObservation();
  }
}

async function pollVoiceTest(): Promise<void> {
  if (voicePollBusy) return;
  const tracker = voiceTracker;
  if (!tracker) return;
  voicePollBusy = true;
  const serial = voiceAttemptSerial;
  try {
    const snapshot = await getRuntimeSnapshot();
    if (serial !== voiceAttemptSerial || voiceTracker !== tracker || step.value !== "voice_test") {
      return;
    }
    const frame = voiceAttemptInput(snapshot);
    const now = Date.now();
    const stateNow = tracker.state();
    if (stateNow !== "terminal") {
      if (
        stateNow !== "waiting_start" &&
        !CONNECTED_PHASES.includes(snapshot.platform.connection.phase)
      ) {
        // 会话进行中连接掉线：按已知原因终止，不谎报通过。
        const terminal = tracker.abort("voice.session_not_started", now);
        if (terminal) {
          onVoiceTerminal(terminal);
          void closeVoiceObservation();
        }
        return;
      }
      if (stateNow === "waiting_start" && frame.voiceState !== "idle" && !voiceFocused.value) {
        // 会话开始了但输入框没聚焦：文字落不到输入框，先让用户点回去。
        const terminal = tracker.abort("voice.input_target_not_ready", now);
        if (terminal) {
          onVoiceTerminal(terminal);
          void closeVoiceObservation();
          void focusVoiceBox();
        }
        return;
      }
      if (frame.voiceState === "idle" && (stateNow === "streaming" || stateNow === "waiting_end")) {
        await settleVoiceManualInput(now);
        if (serial !== voiceAttemptSerial || voiceTracker !== tracker) return;
      }
      const terminal = tracker.observe(frame, now);
      if (terminal) {
        onVoiceTerminal(terminal);
      } else {
        voicePhase.value = tracker.state();
      }
    }
  } catch {
    if (serial === voiceAttemptSerial && !voiceSnapshotErrorLogged) {
      voiceSnapshotErrorLogged = true;
      reportOnboardingEvent({
        kind: "voice_attempt",
        result: "unknown",
        reason: "snapshot_read_failed",
        step: "voice_test",
      });
    }
  } finally {
    voicePollBusy = false;
  }
}

function onVoiceTerminal(terminal: VoiceAttemptTerminal): void {
  voiceResult.value = terminal;
  voicePhase.value = "terminal";
  if (terminal.result === "passed") {
    voiceVerified.value = true;
  } else {
    voiceFailureMessage.value = terminal.code
      ? VOICE_FAILURE_COPY[terminal.code]
      : "这次测试没有通过，再试一次。";
  }
  reportOnboardingEvent({
    kind: "voice_attempt",
    result: terminal.result,
    reason: terminal.code ?? "passed",
    step: "voice_test",
    detail: `d${terminal.evidence.decodedSamples}_s${terminal.evidence.submittedSamples}_q${terminal.evidence.queuedEnd}_drain${terminal.evidence.drainObserved ? 1 : 0}`,
    elapsedMs: terminal.elapsedMs,
  });
  if (voicePollTimer !== null) {
    window.clearInterval(voicePollTimer);
    voicePollTimer = null;
  }
  void closeVoiceObservation();
}

async function onVoiceInput(): Promise<void> {
  const tracker = voiceTracker;
  if (!tracker || tracker.state() === "terminal" || tracker.state() === "idle") return;
  const serial = voiceAttemptSerial;
  await settleVoiceManualInput(Date.now());
  if (serial !== voiceAttemptSerial || voiceTracker !== tracker || tracker.state() === "terminal") {
    return;
  }
  const terminal = tracker.notifyTranscript(Date.now());
  if (terminal) onVoiceTerminal(terminal);
}

function onVoiceBoxFocus(): void {
  voiceFocused.value = true;
}

function onVoiceBoxBlur(): void {
  voiceFocused.value = false;
  const tracker = voiceTracker;
  if (!tracker) return;
  const state = tracker.state();
  if (state === "streaming" || state === "waiting_end" || state === "waiting_transcript") {
    const terminal = tracker.abort("voice.focus_lost", Date.now());
    if (terminal) {
      onVoiceTerminal(terminal);
      void closeVoiceObservation();
    }
  }
}

async function retryVoiceTest(): Promise<void> {
  reportOnboardingEvent({
    kind: "step_retry",
    result: "passed",
    reason: "user_retry",
    step: "voice_test",
  });
  voiceTestRef.value?.clearBox();
  await startVoiceTestSession();
}
</script>

<template>
  <div class="onboarding-shell">
    <header class="onboarding-header">
      <span class="onboarding-phase" aria-label="设置阶段">
        <span :class="{ on: currentPhase === 'prepare' }">准备</span>
        <span class="sep">›</span>
        <span :class="{ on: currentPhase === 'setup' }">设置</span>
        <span class="sep">›</span>
        <span :class="{ on: currentPhase === 'try_it' }">试一下</span>
      </span>
      <span class="onboarding-progress" aria-hidden="true">
        <span class="onboarding-progress-fill" :style="{ width: `${progressPercent}%` }"></span>
      </span>
    </header>

    <main class="onboarding-body">
      <div class="onboarding-main">
        <button
          class="onboarding-back"
          :class="{ 'onboarding-back--placeholder': !previousStep(step) }"
          type="button"
          :aria-hidden="!previousStep(step)"
          :tabindex="previousStep(step) ? 0 : -1"
          @click="onBack"
        >
          <svg class="onboarding-back-icon" viewBox="0 0 16 16" aria-hidden="true">
            <path
              d="M9.8 3.6 5.4 8l4.4 4.4"
              fill="none"
              stroke="currentColor"
              stroke-width="1.6"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          </svg>
          <span>返回</span>
        </button>

        <div class="onboarding-content">
          <div v-if="stateReadFailed" class="onboarding-error-block">
            <p>无法读取设置进度：{{ stateReadFailed }}</p>
            <button class="secondary-button" type="button" @click="restoreStep">重试</button>
          </div>

          <WelcomeStep v-if="step === 'welcome'" />
          <RemoteStep
            v-else-if="step === 'remote'"
            :runtime="runtime"
            :devices="devices"
            :scanning="scanning"
            :scan-message="scanMessage"
            :connecting-device-id="connectingDeviceId"
            :operation-message="operationMessage"
            :button-observed="remoteButtonObserved"
            :voice-key-mistake="voiceKeyMistake"
            :gate-code="gate.code"
            @scan="scanRemotes"
            @connect="connectDevice"
            @open-bluetooth-settings="openBluetoothSettings"
          />
          <AudioStep
            v-else-if="step === 'audio'"
            :endpoints="audioEndpoints"
            :audio="audioSnapshot"
            :scanning="scanningAudio"
            :message="audioMessage"
            :selecting-endpoint-id="selectingEndpointId"
            :opening-vb-cable-page="openingVbCablePage"
            :gate-code="gate.code"
            @select="chooseEndpoint"
            @refresh="refreshAudioDevices(false)"
            @open-download="openDownloadPage"
          />
          <VoiceToolStep
            v-else-if="step === 'voice_tool'"
            :tool="stagedTool"
            :hotkey-label="hotkeyLabel"
            :capture-enabled="rc003Status?.enabled === true"
            :capture-busy="captureBusy"
            :capture-hint="captureHint"
            :conflict="stagedTool === 'doubao' && vokie?.running === true"
            :vokie="vokie"
            :vokie-busy="vokieBusy"
            :other-keys="otherKeys"
            :message="toolMessage"
            @select-tool="selectTool"
            @toggle-capture="requestCaptureToggle"
            @open-vokie-site="openVokieSite"
            @launch-vokie="launchVokieApp"
            @refresh-vokie="refreshVokieState"
            @choose-other-keys="chooseOtherKeys"
          />
          <VoiceTestStep
            v-else-if="step === 'voice_test'"
            ref="voiceTestRef"
            :phase="voicePhase"
            :result="voiceResult?.result ?? null"
            :failure-message="voiceFailureMessage"
            :focused="voiceFocused"
            :tool-label="voiceToolLabel"
            :hotkey-text="hotkeyLabel"
            :audio-text="voiceAudioText"
            @input="onVoiceInput"
            @focus="onVoiceBoxFocus"
            @blur="onVoiceBoxBlur"
            @retry="retryVoiceTest"
          />
          <ControlsStep
            v-else-if="step === 'controls'"
            :observed="observedList"
            :voice-key-mistake="voiceKeyMistake"
          />
          <CompleteStep
            v-else-if="step === 'complete'"
            :checks="completeChecks"
            @fix="onFixStep"
          />
        </div>

        <div class="onboarding-actions">
          <div class="onboarding-actions-left">
            <button
              class="onboarding-diagnostics-button"
              type="button"
              :disabled="diagnosticsCopyState === 'busy'"
              @click="copyDiagnostics"
            >
              {{ diagnosticsCopyText }}
            </button>
            <span
              v-if="diagnosticsCopyMessage"
              class="onboarding-diagnostics-message"
              :class="{ ok: diagnosticsCopyState === 'copied', bad: diagnosticsCopyState === 'failed' }"
              role="status"
            >
              {{ diagnosticsCopyMessage }}
            </span>
          </div>

          <div class="onboarding-actions-right">
            <span v-if="!continueEnabled && blockMessage" class="onboarding-block-hint">{{ blockMessage }}</span>
            <button
              class="primary-button"
              type="button"
              :disabled="!continueEnabled"
              :title="continueEnabled ? '' : blockMessage"
              :data-gate-code="gate.code ?? 'ok'"
              :data-gate-ready="gate.ok ? 'true' : 'false'"
              @click="step === 'complete' ? onStartUsing() : onContinue()"
            >
              {{ step === "complete" ? "开始使用" : "继续" }}
            </button>
          </div>
        </div>
      </div>

      <StepSide :panel="sidePanel" />
    </main>

    <p v-if="saveMessage" class="onboarding-error">进度保存失败：{{ saveMessage }}</p>

    <EnhancedCaptureConfirmDialog
      v-if="showCaptureConfirm"
      @confirm="confirmCaptureDialog"
      @close="closeCaptureDialog"
    />
  </div>
</template>

<style>
/* 向导壳与步骤共用样式（2026-10-05 重设计：参考 Mac，两栏 + 右栏检查卡）。
   类名统一 .onboarding- 前缀，避免与主界面样式冲突。 */
.onboarding-shell {
  display: flex;
  flex-direction: column;
  min-height: 100vh;
  box-sizing: border-box;
  background: var(--surface-canvas);
  color: var(--text-primary);
}
.onboarding-header {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 46px;
  border-bottom: 1px solid var(--border);
}
.onboarding-phase {
  display: inline-flex;
  gap: 10px;
  align-items: center;
  font-size: 14px;
  color: var(--text-secondary);
}
.onboarding-phase .on {
  color: var(--text-primary);
  font-weight: 700;
}
.onboarding-phase .sep {
  opacity: 0.55;
}
.onboarding-progress {
  position: absolute;
  left: 0;
  right: 0;
  bottom: -1px;
  height: 3px;
  background: var(--track);
}
.onboarding-progress-fill {
  display: block;
  height: 100%;
  background: var(--accent);
  border-radius: 0 2px 2px 0;
  transition: width 0.25s ease;
}
.onboarding-body {
  flex: 1 1 auto;
  display: grid;
  grid-template-columns: minmax(0, 60fr) minmax(360px, 40fr);
  min-height: 0;
}
.onboarding-main {
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 26px 44px 18px;
  overflow: hidden;
  min-height: 0;
}
.onboarding-content {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding-right: 4px;
}
/* 无上一步时保留同高占位：各步标题固定在内容列左上角同一位置（2026-10-05 要求）。 */
.onboarding-back--placeholder {
  visibility: hidden;
  pointer-events: none;
}
/* 底部动作行（2026-10-05 用户要求）：去掉整幅页脚横幅；复制诊断在左、主按钮在右。 */
.onboarding-actions {
  flex: 0 0 auto;
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 18px;
  padding-top: 4px;
}
.onboarding-actions-left {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 4px;
  min-width: 0;
}
.onboarding-actions-right {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 6px;
}
.onboarding-back {
  width: fit-content;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  margin-left: -5px;
  padding: 0;
  border: 0;
  background: none;
  color: var(--text-control);
  font: inherit;
  font-size: 14px;
  line-height: 1.4;
  cursor: pointer;
}
.onboarding-back-icon {
  width: 16px;
  height: 16px;
  flex: 0 0 16px;
}
.onboarding-back:hover {
  color: var(--accent-text);
}

/* ---- 右栏（插图 + 检查卡） ---- */
.onboarding-side {
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 22px 26px;
  border-left: 1px solid var(--border);
  background: var(--accent-surface);
  min-height: 0;
  overflow: hidden;
}
.side-visual {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 14px;
  min-height: 0;
}
.side-photo {
  width: 138px;
  max-height: 100%;
  object-fit: contain;
  filter: drop-shadow(0 14px 22px rgba(20, 24, 36, 0.22));
}
.side-appicon {
  width: 76px;
  height: 76px;
  border-radius: 18px;
  box-shadow: 0 10px 22px rgba(20, 24, 36, 0.18);
}
.side-wave {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
.side-wave i {
  display: block;
  width: 5px;
  border-radius: 3px;
  background: var(--accent);
}
.side-logo-done {
  position: relative;
  display: inline-flex;
}
.side-logo-done .side-appicon {
  width: 84px;
  height: 84px;
  border-radius: 20px;
  box-shadow: 0 10px 22px rgba(20, 24, 36, 0.18);
}
.side-done-badge {
  position: absolute;
  right: -7px;
  bottom: -7px;
  width: 30px;
  height: 30px;
  border-radius: 50%;
  border: 2px solid var(--card);
  background: var(--success-surface);
  color: var(--success-text);
  display: grid;
  place-items: center;
  font-size: 16px;
  font-weight: 800;
}
.side-caption {
  font-size: 14px;
  color: var(--text-secondary);
}
.onboarding-check-card {
  border: 1px solid var(--border);
  border-radius: 12px;
  background: var(--card);
  padding: 16px;
  box-shadow: 0 10px 24px var(--shadow-card);
}
.onboarding-check-card h4 {
  margin: 0 0 10px;
  font-size: 15px;
  color: var(--text-subtle-strong);
}
.onboarding-crow {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 9px 12px;
  border-radius: 8px;
  background: var(--surface-subtle);
  font-size: 14.5px;
  color: var(--text-control);
}
.onboarding-crow + .onboarding-crow {
  margin-top: 7px;
}
.onboarding-crow .crow-state {
  margin-left: auto;
  font-weight: 700;
}
.onboarding-crow .crow-value {
  margin-left: auto;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  color: var(--text-secondary);
}
.onboarding-crow[data-state="ok"] .crow-state {
  color: var(--success-text);
}
.onboarding-crow[data-state="busy"] .crow-state {
  color: var(--accent-text);
}
.onboarding-crow[data-state="warn"] .crow-state {
  color: var(--warning-text);
}
.onboarding-crow[data-state="pending"] .crow-state {
  color: var(--pending-text);
}

/* ---- 步骤内容 ---- */
.onboarding-step {
  max-width: 720px;
}
.onboarding-step h1 {
  margin: 0 0 12px;
  font-size: 27px;
  letter-spacing: -0.3px;
  line-height: 1.25;
}
.onboarding-lede {
  margin: 0 0 16px;
  font-size: 15.5px;
  color: var(--text-control);
  line-height: 1.7;
}
.onboarding-muted {
  color: var(--text-secondary);
  font-size: 14px;
}
.onboarding-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin: 6px 0 0;
  padding-left: 20px;
  font-size: 14.5px;
  line-height: 1.65;
}
.onboarding-rows {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.onboarding-rows .onboarding-status-row + .onboarding-status-row {
  margin-top: 0;
}
.onboarding-rows .onboarding-option + .onboarding-option {
  margin-top: 0;
}
.onboarding-card {
  padding: 16px 18px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--card);
}
.onboarding-card + .onboarding-card {
  margin-top: 14px;
}
.onboarding-card.accent {
  border-color: var(--accent-border);
  background: var(--accent-surface);
}
.onboarding-card.warn {
  border-color: rgba(182, 109, 20, 0.35);
  background: var(--warning-surface-soft);
}
.onboarding-card h4 {
  margin: 0 0 12px;
  font-size: 16px;
}
.onboarding-card p {
  margin: 12px 0 0;
  font-size: 14.5px;
  line-height: 1.65;
  color: var(--text-secondary);
}
.onboarding-card.success {
  border-color: rgba(38, 113, 72, 0.35);
  background: var(--success-surface);
}
.onboarding-success {
  display: flex;
  align-items: center;
  gap: 9px;
  margin: 0;
  color: var(--success-text, #1a7f4b);
  font-weight: 700;
}
.onboarding-success .ok-mark {
  flex: 0 0 20px;
  width: 20px;
  height: 20px;
  border-radius: 50%;
  background: var(--success-text, #1a7f4b);
  color: #fff;
  display: grid;
  place-items: center;
  font-size: 12px;
  font-weight: 800;
}
.onboarding-numlist {
  display: flex;
  flex-direction: column;
  gap: 7px;
  margin: 6px 0 0;
  padding: 0;
  list-style: none;
}
.onboarding-numlist li {
  display: flex;
  gap: 9px;
  font-size: 14px;
  line-height: 1.6;
}
.onboarding-num {
  flex: 0 0 18px;
  width: 18px;
  height: 18px;
  margin-top: 1px;
  border-radius: 50%;
  background: var(--accent);
  color: var(--text-on-accent);
  font-size: 11.5px;
  font-weight: 700;
  display: grid;
  place-items: center;
}
.onboarding-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 12px;
}
.onboarding-chip {
  padding: 8px 14px;
  border: 1px solid var(--border-strong);
  border-radius: 8px;
  background: var(--surface-control);
  color: var(--text-control);
  font: inherit;
  font-size: 14px;
  cursor: pointer;
}
.onboarding-chip.strong {
  border-color: var(--accent-border);
  background: var(--accent-surface-strong);
  color: var(--accent-text);
  font-weight: 600;
}
.onboarding-chip:disabled {
  opacity: 0.55;
  cursor: default;
}

/* ---- 状态行 / 选择卡 ---- */
.onboarding-status-row {
  display: flex;
  align-items: center;
  gap: 11px;
  padding: 12px 14px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface-subtle);
}
.onboarding-status-row + .onboarding-status-row {
  margin-top: 10px;
}
.onboarding-status-row .status-icon {
  flex: 0 0 28px;
  width: 28px;
  height: 28px;
  border-radius: 8px;
  display: grid;
  place-items: center;
  font-size: 14px;
  background: var(--accent-surface);
  color: var(--accent-text);
}
.onboarding-status-row[data-state="pending"] .status-icon {
  background: var(--pending-surface);
  color: var(--pending-text);
}
.onboarding-status-row[data-state="ok"] .status-icon {
  background: var(--success-surface);
  color: var(--success-text);
}
.onboarding-status-row[data-state="warn"] .status-icon {
  background: var(--warning-surface);
  color: var(--warning-text);
}
.onboarding-status-row .status-text {
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.onboarding-status-row .status-text strong {
  font-size: 15px;
}
.onboarding-status-row .status-text small {
  margin-top: 2px;
  font-size: 14px;
  color: var(--text-secondary);
}
.onboarding-status-row .status-state {
  margin-left: auto;
  font-size: 14px;
  font-weight: 700;
  white-space: nowrap;
}
.onboarding-status-row .status-value {
  margin-left: auto;
  font-size: 14px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  color: var(--text-secondary);
  white-space: nowrap;
}
.onboarding-status-row[data-state="pending"] .status-state {
  color: var(--pending-text);
}
.onboarding-status-row[data-state="busy"] .status-state {
  color: var(--accent-text);
}
.onboarding-status-row[data-state="ok"] .status-state {
  color: var(--success-text);
}
.onboarding-status-row[data-state="warn"] .status-state {
  color: var(--warning-text);
}

.onboarding-option {
  display: flex;
  align-items: center;
  gap: 11px;
  width: 100%;
  padding: 13px 14px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--card);
  color: var(--text-primary);
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.onboarding-option + .onboarding-option {
  margin-top: 9px;
}
.onboarding-option.selected {
  border-color: var(--accent-border);
  background: var(--accent-surface);
}
.onboarding-option:disabled {
  opacity: 0.6;
  cursor: default;
}
.onboarding-option .option-radio {
  flex: 0 0 16px;
  width: 16px;
  height: 16px;
  border: 1.5px solid var(--border-strong);
  border-radius: 50%;
  position: relative;
}
.onboarding-option.selected .option-radio {
  border-color: var(--accent);
}
.onboarding-option.selected .option-radio::after {
  content: "";
  position: absolute;
  inset: 3px;
  border-radius: 50%;
  background: var(--accent);
}
.onboarding-option .option-body {
  min-width: 0;
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.onboarding-option .option-body strong {
  font-size: 15px;
  display: flex;
  align-items: center;
  gap: 8px;
}
.onboarding-option .option-body small {
  font-size: 14px;
  color: var(--text-secondary);
}
.onboarding-option .option-trailing {
  margin-left: auto;
  font-size: 13.5px;
  font-weight: 600;
  color: var(--text-secondary);
  white-space: nowrap;
}
.onboarding-option.selected .option-trailing {
  color: var(--accent-text);
}
.onboarding-tag {
  font-size: 12px;
  font-weight: 700;
  color: var(--accent-text);
}
.onboarding-option .option-state.ok {
  color: var(--success-text);
}
.onboarding-grid2 {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
  margin-bottom: 16px;
}
.onboarding-grid2 .onboarding-option {
  margin-top: 0;
}

/* ---- 按键 chip 网格 / 进度点 ---- */
.onboarding-btnchips {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 8px;
}
.onboarding-bchip {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface-subtle);
  color: var(--text-control);
  font-size: 14px;
}
.onboarding-bchip .dot {
  flex: 0 0 14px;
  width: 14px;
  height: 14px;
  border: 1.5px solid var(--border-strong);
  border-radius: 50%;
}
.onboarding-bchip.hit {
  border-color: var(--accent-border);
  background: var(--accent-surface);
  color: var(--accent-text);
  font-weight: 700;
}
.onboarding-bchip.hit .dot {
  border-color: var(--accent);
  background: var(--accent);
}
.onboarding-dots {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 12px;
  font-size: 14.5px;
  color: var(--text-secondary);
}
.onboarding-dots .dot {
  width: 11px;
  height: 11px;
  border: 1.5px solid var(--border-strong);
  border-radius: 50%;
}
.onboarding-dots .dot.on {
  border-color: var(--accent);
  background: var(--accent);
}

/* ---- 语音验证页 ---- */
.onboarding-status-line {
  margin: 0;
  font-size: 14.5px;
  font-weight: 600;
}
.onboarding-wave-mark {
  margin-right: 6px;
  color: var(--accent);
  letter-spacing: 1px;
}
.onboarding-ok {
  margin: 0;
  color: var(--success-text, #1a7f4b);
}
.onboarding-voice-input {
  width: 100%;
  box-sizing: border-box;
  min-height: 64px;
  margin: 20px 0 14px;
  padding: 16px 16px;
  border: 1.5px solid var(--accent-border);
  border-radius: 10px;
  background: var(--card);
  color: var(--text-primary);
  font: inherit;
  font-size: 16px;
  line-height: 1.5;
}
.onboarding-voice-input:focus {
  outline: 2px solid var(--accent-border);
  outline-offset: 1px;
}

/* ---- 输入工具页 ---- */
.onboarding-switch-row {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 16px;
  padding-top: 16px;
  border-top: 1px solid var(--border);
}
.onboarding-switch-row .switch-state {
  font-size: 14px;
  font-weight: 600;
}
.onboarding-switch-row .switch-state.ok {
  color: var(--success-text, #1a7f4b);
}
.onboarding-switch-row .switch-state.warn {
  color: var(--warning-text, #8a5a00);
}
.onboarding-chip-select {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 8px;
}
.onboarding-chip-select button {
  padding: 7px 14px;
  border: 1px solid var(--border-strong, #c9cbd6);
  border-radius: 8px;
  background: var(--surface-raised, #fff);
  color: var(--text-control);
  cursor: pointer;
  font: inherit;
  font-size: 14px;
}
.onboarding-chip-select button.selected {
  border-color: var(--accent-border);
  background: var(--accent-surface);
  color: var(--accent-text);
  font-weight: 600;
}

/* ---- 保存失败提示 ---- */
.onboarding-error {
  margin: 0 44px 10px;
  font-size: 14px;
  color: var(--error-text, #b3261e);
}
.onboarding-error-block {
  margin: 0 0 12px;
  padding: 12px 16px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--card);
}
.onboarding-error-block p {
  margin: 0 0 8px;
  font-size: 14px;
}

/* 底部动作行里的门禁提示（右对齐，紧贴主按钮上方）。 */
.onboarding-block-hint {
  max-width: 420px;
  font-size: 14px;
  color: var(--warning-text);
  text-align: right;
}
/* 「复制诊断信息」：过不去时的报障入口，低频动作，弱化为文字链接（无下划线）。 */
.onboarding-diagnostics-button {
  padding: 0;
  border: 0;
  background: none;
  color: var(--text-secondary);
  font: inherit;
  font-size: 13.5px;
  text-decoration: none;
  cursor: pointer;
}
.onboarding-diagnostics-button:hover {
  color: var(--accent-text);
}
.onboarding-diagnostics-button:disabled {
  cursor: wait;
  opacity: 0.6;
}
.onboarding-diagnostics-message {
  font-size: 13.5px;
  color: var(--text-secondary);
}
.onboarding-diagnostics-message.ok {
  color: var(--success-text, #1a7f4b);
}
.onboarding-diagnostics-message.bad {
  color: var(--warning-text);
}

/* 窄窗或矮窗（含 150% 缩放下的较小可用高度）：右栏收进内容列底部，
   只保留检查卡，插图隐藏。 */
@media (max-width: 1024px), (max-height: 660px) {
  .onboarding-body {
    grid-template-columns: minmax(0, 1fr);
    overflow-y: auto;
  }
  .onboarding-main {
    overflow: visible;
  }
  .onboarding-content {
    overflow: visible;
  }
  .onboarding-side {
    flex-direction: row;
    align-items: center;
    gap: 16px;
    padding: 14px 26px;
    border-left: 0;
    border-top: 1px solid var(--border);
    overflow: visible;
  }
  .side-visual {
    display: none;
  }
  .onboarding-check-card {
    flex: 1;
  }
}
</style>

