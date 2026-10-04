<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import {
  buttonLabel,
  completeOnboarding,
  connectRemote,
  disableRc003Capture,
  enableRc003Capture,
  getAudioSnapshot,
  getOnboardingState,
  getOtherVoiceHotkey,
  getRc003TaskStatus,
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
import AudioStep from "../onboarding/steps/AudioStep.vue";
import CompleteStep from "../onboarding/steps/CompleteStep.vue";
import ControlsStep from "../onboarding/steps/ControlsStep.vue";
import RemoteStep from "../onboarding/steps/RemoteStep.vue";
import VoiceToolStep from "../onboarding/steps/VoiceToolStep.vue";
import WelcomeStep from "../onboarding/steps/WelcomeStep.vue";
import EnhancedCaptureConfirmDialog from "../components/EnhancedCaptureConfirmDialog.vue";

const props = defineProps<{ runtime: RuntimeSnapshot | null }>();
const emit = defineEmits<{ completed: [] }>();

/**
 * 步骤⑤（按住说话验证）依赖现场探针①：未接入前，「继续」到它止步；
 * 其余步骤（含⑥⑦）已全部接入。探针通过、步骤⑤落地后删除本守卫。
 */
function isImplemented(candidate: OnboardingStep): boolean {
  return candidate !== "voice_test";
}

const step = ref<OnboardingStep>("welcome");
const stateReadFailed = ref("");
const saveMessage = ref("");
const wizardStartedAt = Math.round(performance.now());
const completing = ref(false);

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
let mappingSuspended = false;

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
  const target = nextStep(step.value);
  return target !== null && isImplemented(target);
});
const currentPhase = computed(() => phaseOf(step.value));

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
// 普通按键体验期间暂挂映射执行（离开恢复）；切换步骤时清除误按提示。
watch(
  step,
  (current, previous) => {
    stopButtonObservation();
    voiceKeyMistake.value = false;
    if (previous === "controls" && current !== "controls") {
      void setMappingSuspensionState(false);
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
    if (current === "controls") {
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
  await restoreStep();
});

onUnmounted(() => {
  stopButtonObservation();
  // 安全网：向导卸载（完成/退出）时恢复映射执行；进程退出后内存态自然复位。
  if (mappingSuspended) {
    void setMappingSuspension(false);
  }
});

async function restoreStep(): Promise<void> {
  stateReadFailed.value = "";
  try {
    const state = await getOnboardingState();
    if (!state.isActive) {
      reportOnboardingEvent({
        kind: "started",
        result: "passed",
        reason: "already_completed",
        elapsedMs: 0,
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
    });
    await setStep(restored, "restore");
  } catch (error) {
    stateReadFailed.value = error instanceof Error ? error.message : String(error);
    reportOnboardingEvent({
      kind: "started",
      result: "failed",
      reason: "state_read_failed",
      elapsedMs: 0,
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
  if (target && isImplemented(target)) {
    await setStep(target, "continue");
  }
}

async function onBack(): Promise<void> {
  const target = previousStep(step.value);
  if (!target) return;
  // 未接入的步骤（当前只有⑤）不进入，继续往回退一步（临时守卫，与 isImplemented 同批删除）。
  const resolved = !isImplemented(target) ? previousStep(target) : target;
  if (!resolved) return;
  reportOnboardingEvent({
    kind: "navigation",
    result: "passed",
    reason: "user_back",
    step: step.value,
    detail: resolved,
  });
  await setStep(resolved, "back");
}

// ---- 遥控器 / 普通按键步骤：边沿观察 ----

async function startButtonObservation(): Promise<void> {
  if (buttonEdgeSubscribed) return;
  buttonEdgeSubscribed = true;
  try {
    const unsubscribe = await subscribeButtonEdges(handleButtonEdge);
    if (step.value !== "remote" && step.value !== "controls") {
      unsubscribe();
      buttonEdgeSubscribed = false;
      return;
    }
    buttonEdgeUnsubscribe = unsubscribe;
  } catch {
    buttonEdgeSubscribed = false;
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
  try {
    devices.value = await scanPairedRemotes();
    scanMessage.value = devices.value.length
      ? `找到 ${devices.value.length} 个已配对的小米遥控器`
      : "没有找到已配对的小米遥控器。先在 Windows 里配对，再回来扫描。";
  } catch (error) {
    devices.value = [];
    scanMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    scanning.value = false;
  }
}

async function connectDevice(device: PairedRemote): Promise<void> {
  connectingDeviceId.value = device.id;
  operationMessage.value = "";
  try {
    await connectRemote(device.id);
    operationMessage.value = "正在确认语音功能，就绪后按一下普通按键。";
  } catch (error) {
    operationMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    connectingDeviceId.value = "";
  }
}

async function openBluetoothSettings(): Promise<void> {
  try {
    await openWindowsSettings("bluetooth");
    operationMessage.value = "已打开系统蓝牙设置；配对完成后回来点「扫描已配对设备」。";
  } catch (error) {
    operationMessage.value = error instanceof Error ? error.message : String(error);
  }
}

// ---- 语音设备步骤 ----

async function refreshAudioDevices(autoSelect: boolean): Promise<void> {
  scanningAudio.value = true;
  audioMessage.value = "正在读取语音设备…";
  try {
    audioEndpoints.value = await listAudioEndpoints();
    audioSnapshot.value = await getAudioSnapshot();
    const recommended = audioEndpoints.value.filter(isRecommendedVoiceEndpoint);
    const hasSelection = Boolean(audioSnapshot.value.selectedEndpointId);
    if (autoSelect && !hasSelection && recommended.length === 1) {
      await chooseEndpoint(recommended[0], true);
      return;
    }
    audioMessage.value = recommended.length
      ? `已检测到 ${recommended.length} 个 VB-CABLE 语音设备`
      : "没有检测到 VB-CABLE；安装完成后要重启电脑，再回来重新检测";
  } catch (error) {
    audioEndpoints.value = [];
    audioMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    scanningAudio.value = false;
  }
}

async function chooseEndpoint(endpoint: AudioEndpoint, automatic = false): Promise<void> {
  selectingEndpointId.value = endpoint.id;
  audioMessage.value = "正在打开语音设备…";
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
  } catch (error) {
    audioMessage.value = error instanceof Error ? error.message : String(error);
    try {
      audioSnapshot.value = await getAudioSnapshot();
    } catch {
      // 读取失败保持旧快照；错误已在上面显示。
    }
  } finally {
    selectingEndpointId.value = "";
  }
}

async function openDownloadPage(): Promise<void> {
  openingVbCablePage.value = true;
  try {
    await openVbCableDownloadPage();
    audioMessage.value = "已打开官方下载页；安装需要管理员权限，完成后请重启电脑";
  } catch (error) {
    audioMessage.value = error instanceof Error ? error.message : String(error);
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
  } catch (error) {
    toolMessage.value = error instanceof Error ? error.message : String(error);
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
  try {
    await stageOnboardingVoiceBinding(tool, hotkey);
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
    return;
  }
  // 豆包/Vokie 同键（右 Alt）：选中即刷新 Vokie 运行状态，冲突要立刻可见。
  if (tool === "doubao" || tool === "vokie") {
    void refreshVokieState();
  }
}

async function chooseOtherKeys(keys: KeyCode[]): Promise<void> {
  otherKeys.value = keys;
  try {
    await setOtherVoiceHotkey(keys);
  } catch (error) {
    toolMessage.value = error instanceof Error ? error.message : String(error);
  }
  try {
    await stageOnboardingVoiceBinding("other", keys.length ? { keys } : null);
    currentHotkey.value = keys.length ? { keys } : null;
    reportOnboardingEvent({
      kind: "tool_selected",
      result: "passed",
      reason: "other_keys",
      step: "voice_tool",
      detail: keys.length ? "with_keys" : "disabled",
    });
  } catch (error) {
    toolMessage.value = error instanceof Error ? error.message : String(error);
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
  try {
    vokie.value = await getVokieInstallation();
  } catch (error) {
    toolMessage.value = error instanceof Error ? error.message : String(error);
  }
}

async function openVokieSite(): Promise<void> {
  try {
    await openVokieHomepage();
    toolMessage.value = "已打开 Vokie 官网；安装后回来点「重新检测」。";
  } catch (error) {
    toolMessage.value = error instanceof Error ? error.message : String(error);
  }
}

async function launchVokieApp(): Promise<void> {
  vokieBusy.value = true;
  try {
    await launchVokie();
    toolMessage.value = "已发出启动请求；启动后点「重新检测」。";
  } catch (error) {
    toolMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    vokieBusy.value = false;
  }
}

// ---- 普通按键体验 / 完成步骤 ----

async function setMappingSuspensionState(suspended: boolean): Promise<void> {
  if (mappingSuspended === suspended) return;
  mappingSuspended = suspended;
  try {
    await setMappingSuspension(suspended);
  } catch (error) {
    saveMessage.value = error instanceof Error ? error.message : String(error);
  }
}

/** 完成页重查关键运行条件：正式输入工具、Vokie 运行态、全按键开关、音频端点。 */
async function refreshCompleteState(): Promise<void> {
  try {
    configuredTool.value = await getVoiceInputTool();
  } catch {
    // 读取失败保持旧值；门禁会显示未就绪，不给用户虚假的「完成」。
  }
  try {
    vokie.value = await getVokieInstallation();
  } catch {
    // 同上。
  }
  try {
    rc003Status.value = await getRc003TaskStatus();
  } catch {
    // 同上。
  }
  try {
    audioEndpoints.value = await listAudioEndpoints();
    audioSnapshot.value = await getAudioSnapshot();
  } catch {
    // 同上。
  }
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
  if (!isImplemented(target)) {
    // 第⑤步未接入前无法跳转（激活开关关闭，真实用户不受影响）。
    reportOnboardingEvent({
      kind: "navigation",
      result: "failed",
      reason: "fix_blocked",
      step: "complete",
      detail: target,
    });
    return;
  }
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
      <span class="onboarding-title">无线麦 SayAll · 首次设置</span>
    </header>

    <main class="onboarding-body">
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
    </main>

    <p v-if="saveMessage" class="onboarding-error">进度保存失败：{{ saveMessage }}</p>

    <footer class="onboarding-footer">
      <button
        v-if="previousStep(step)"
        class="secondary-button"
        type="button"
        @click="onBack"
      >
        返回
      </button>
      <span class="spacer"></span>
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
    </footer>

    <EnhancedCaptureConfirmDialog
      v-if="showCaptureConfirm"
      @confirm="confirmCaptureDialog"
      @close="closeCaptureDialog"
    />
  </div>
</template>

<style>
/* 向导壳与步骤共用样式。类名统一 .onboarding- 前缀，避免与主界面样式冲突。 */
.onboarding-shell {
  display: flex;
  flex-direction: column;
  min-height: 100vh;
  box-sizing: border-box;
  padding: 18px 26px 20px;
  background: var(--surface-canvas);
  color: var(--text-primary);
}
.onboarding-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding-bottom: 12px;
  border-bottom: 1px solid var(--border);
}
.onboarding-phase {
  display: inline-flex;
  gap: 8px;
  align-items: center;
  font-size: 12.5px;
  color: var(--text-secondary);
}
.onboarding-phase .on {
  color: var(--accent-text);
  font-weight: 700;
}
.onboarding-phase .sep {
  opacity: 0.6;
}
.onboarding-title {
  font-size: 12.5px;
  color: var(--text-secondary);
}
.onboarding-body {
  flex: 1 1 auto;
  padding: 18px 2px 12px;
}
.onboarding-footer {
  display: flex;
  align-items: center;
  gap: 12px;
  padding-top: 14px;
  border-top: 1px solid var(--border);
}
.onboarding-footer .spacer {
  margin-left: auto;
}
.onboarding-block-hint {
  max-width: 560px;
  font-size: 12.5px;
  color: var(--text-secondary);
  text-align: right;
}
.onboarding-step {
  max-width: 720px;
}
.onboarding-step h1 {
  margin: 0 0 8px;
  font-size: 18px;
}
.onboarding-lede {
  margin: 0 0 14px;
  color: var(--text-control);
}
.onboarding-muted {
  color: var(--text-secondary);
  font-size: 12.5px;
}
.onboarding-callout {
  margin: 0 0 12px;
  padding: 12px 14px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--card);
}
.onboarding-callout.ok {
  border-color: rgba(46, 160, 96, 0.4);
}
.onboarding-callout.warn {
  border-color: rgba(240, 173, 78, 0.55);
}
.onboarding-callout ol,
.onboarding-callout ul {
  margin: 8px 0 0;
  padding-left: 22px;
}
.onboarding-callout li {
  margin: 3px 0;
}
.onboarding-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-top: 10px;
}
.onboarding-message {
  margin: 8px 0 0;
  font-size: 12.5px;
  color: var(--text-secondary);
}
.onboarding-status-line {
  margin: 0 0 6px;
  font-weight: 600;
}
.onboarding-ok {
  margin: 0;
  color: var(--success-text, #1a7f4b);
}
.onboarding-device-list {
  display: grid;
  gap: 8px;
  margin: 8px 0 0;
  padding: 0;
  list-style: none;
}
.onboarding-device-list li {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.onboarding-badge {
  margin-left: 6px;
  padding: 1px 8px;
  border-radius: 999px;
  background: var(--accent-surface);
  color: var(--accent-text);
  font-size: 11.5px;
  font-style: normal;
}
.onboarding-tool-list {
  display: grid;
  gap: 8px;
  max-width: 560px;
  margin: 0 0 12px;
  padding: 0;
  list-style: none;
}
.onboarding-tool-card {
  display: flex;
  gap: 10px;
  align-items: flex-start;
  width: 100%;
  padding: 9px 12px;
  border: 1px solid var(--border-strong, #c9cbd6);
  border-radius: 10px;
  background: var(--surface-control, #f7f7fa);
  color: var(--text-primary);
  text-align: left;
  cursor: pointer;
}
.onboarding-tool-card.selected {
  border-color: var(--accent-border);
  background: var(--accent-surface);
}
.onboarding-tool-card .radio {
  flex: 0 0 auto;
  width: 14px;
  height: 14px;
  margin-top: 3px;
  border: 1.5px solid var(--border-strong, #c9cbd6);
  border-radius: 50%;
  background: var(--surface-raised, #fff);
}
.onboarding-tool-card.selected .radio {
  border-color: var(--accent);
  box-shadow:
    inset 0 0 0 3px var(--surface-raised, #fff),
    inset 0 0 0 8px var(--accent);
}
.onboarding-tool-card strong {
  display: block;
  font-size: 13.5px;
}
.onboarding-tool-card small {
  display: block;
  margin-top: 2px;
  color: var(--text-secondary);
  font-size: 12px;
}
.onboarding-switch-row {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 10px;
  padding-top: 10px;
  border-top: 1px solid var(--border);
}
.onboarding-switch-row .switch-state {
  font-size: 12.5px;
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
  padding: 5px 12px;
  border: 1px solid var(--border-strong, #c9cbd6);
  border-radius: 8px;
  background: var(--surface-raised, #fff);
  color: var(--text-control);
  cursor: pointer;
}
.onboarding-chip-select button.selected {
  border-color: var(--accent-border);
  background: var(--accent-surface);
  color: var(--accent-text);
  font-weight: 600;
}
.onboarding-error {
  margin: 6px 0;
  font-size: 12.5px;
  color: var(--danger-text, #b3261e);
}
.onboarding-error-block {
  margin: 0 0 14px;
  padding: 10px 14px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--card);
}
.onboarding-error-block p {
  margin: 0 0 8px;
  font-size: 12.5px;
}
.onboarding-check-list {
  display: grid;
  gap: 6px;
  margin: 0;
  padding: 0;
  list-style: none;
}
.onboarding-check-list li {
  display: flex;
  gap: 8px;
  align-items: baseline;
}
.onboarding-check-list .onboarding-ok,
.onboarding-check-list .onboarding-error {
  margin: 0;
}
</style>
