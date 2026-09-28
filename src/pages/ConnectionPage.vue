<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import BatteryIndicator from "../components/BatteryIndicator.vue";
import { reportFrontendEvent } from "../lib/frontend-diagnostics";
import type {
  AudioEndpoint,
  AudioSnapshot,
  ConnectionSnapshot,
  KeyChord,
  PairedRemote,
  RuntimeSnapshot,
} from "../lib/bridge";
import {
  audioPhaseLabel,
  chordLabel,
  connectRemote,
  connectionPhaseLabel,
  disconnectRemote,
  getAudioSnapshot,
  getConnectionSnapshot,
  getVoiceHoldHotkey,
  isRecommendedVoiceEndpoint,
  listAudioEndpoints,
  openVbCableDownloadPage,
  remoteModelLabel,
  scanPairedRemotes,
  selectAudioEndpoint,
  setVoiceHoldHotkey,
  startShortcutCapture,
  stopShortcutCapture,
  subscribeShortcutCaptureEdges,
  voiceHoldHotkeyLabel,
  type KeyCode,
  type ShortcutCaptureEdge,
} from "../lib/bridge";

const props = defineProps<{ runtime: RuntimeSnapshot | null }>();

const emptyConnection = (): ConnectionSnapshot => ({
  phase: "idle",
  remoteName: null,
  remoteModel: "unknown",
  capabilities: null,
  voiceState: "idle",
  decodedSamples: 0,
  generation: 0,
  reconnectAttempt: 0,
  powerNotificationsAvailable: false,
  lastError: null,
});

const emptyAudio = (): AudioSnapshot => ({
  phase: "unsupported",
  selectedEndpointId: null,
  selectedEndpointName: null,
  queuedSamples: 0,
  submittedSamples: 0,
  generation: 0,
  lastError: null,
});

const connection = ref<ConnectionSnapshot>(emptyConnection());
const audio = ref<AudioSnapshot>(emptyAudio());
const scanning = ref(false);
const connectingDeviceId = ref("");
const disconnecting = ref(false);
const devices = ref<PairedRemote[]>([]);
const scanMessage = ref("尚未扫描");
const operationMessage = ref("");
const audioEndpoints = ref<AudioEndpoint[]>([]);
const showEndpointList = ref(false);
const scanningAudio = ref(false);
const audioScanComplete = ref(false);
const selectingEndpointId = ref("");
const openingVbCablePage = ref(false);
const audioMessage = ref("尚未读取语音设备");
const voiceHotkey = ref<KeyChord | null>(null);
const savingVoiceHotkey = ref(false);
const voiceHotkeyMessage = ref("尚未读取快捷键设置");
const capturingVoiceHotkey = ref(false);
const captureStartingVoiceHotkey = ref(false);
/** 录入开始时仍有 preheld 键按住：后端吞键但不投递边沿，直到全部松开。 */
const waitingPreheldRelease = ref(false);
const voiceCaptureDisplay = ref<KeyCode[]>([]);
const voiceCapturePressedCount = ref(0);
let pollTimer: ReturnType<typeof setInterval> | undefined;
let unlistenVoiceCapture: (() => void) | null = null;
let voiceCaptureTimeout: number | null = null;
let voiceCaptureRequestId = 0;
let unmounted = false;
const voiceCapturePressed = new Set<KeyCode>();

/** 按住说话快捷键默认值（v1 固定，适配微信输入法的默认语音热键）。 */
const DEFAULT_VOICE_HOTKEY_KEYS: KeyCode[] = ["left_control", "left_windows"];

const activeVoiceHotkeyKeys = computed(() =>
  voiceHotkey.value ? [...voiceHotkey.value.keys].sort().join("+") : "",
);

function presetIsActive(keys: string[]): boolean {
  return [...keys].sort().join("+") === activeVoiceHotkeyKeys.value;
}

async function applyVoiceHotkey(keys: string[]): Promise<boolean> {
  savingVoiceHotkey.value = true;
  voiceHotkeyMessage.value = "";
  try {
    voiceHotkey.value = await setVoiceHoldHotkey(
      keys.length ? { keys: [...keys] } : null,
    );
    voiceHotkeyMessage.value = voiceHotkey.value
      ? `按住说话快捷键已设为 ${voiceHoldHotkeyLabel(voiceHotkey.value)}`
      : "按住说话快捷键已关闭，语音键仅输出语音";
    return true;
  } catch (error) {
    voiceHotkeyMessage.value = error instanceof Error ? error.message : String(error);
    await refreshVoiceHotkey();
    return false;
  } finally {
    savingVoiceHotkey.value = false;
  }
}

/**
 * 按住说话快捷键录入：录入门走 OS 级低级钩子（与按键映射页的自定义录入
 * 同一条链路）。按键持续加入可编辑草稿，松手不自动保存；用户可逐个补按
 * 被输入法吞掉的完成键，最后显式保存。Esc（未按其他键）取消，30 秒超时。
 */
async function beginVoiceHotkeyCapture(): Promise<void> {
  if (capturingVoiceHotkey.value || captureStartingVoiceHotkey.value) return;
  const requestId = ++voiceCaptureRequestId;
  captureStartingVoiceHotkey.value = true;
  voiceHotkeyMessage.value = "";
  try {
    const preheld = await startShortcutCapture();
    if (unmounted || requestId !== voiceCaptureRequestId) {
      await stopShortcutCapture().catch(() => undefined);
      return;
    }
    voiceCapturePressed.clear();
    voiceCapturePressedCount.value = 0;
    voiceCaptureDisplay.value = [];
    capturingVoiceHotkey.value = true;
    // preheld 键的边沿对录入不可见（其 DOWN 已进 OS，UP 必须放行），
    // 后端等它们全部松开后才开始投递边沿；提示用户先松手，避免把
    // "按住中打开录入"截断成半截组合。
    waitingPreheldRelease.value = preheld.length > 0;
    if (preheld.length > 0) {
      voiceHotkeyMessage.value =
        "检测到仍有按住的按键，请先松开所有按键；松开后即可按新组合，录入将自动开始";
    }
    if (voiceCaptureTimeout !== null) window.clearTimeout(voiceCaptureTimeout);
    voiceCaptureTimeout = window.setTimeout(() => {
      void cancelVoiceHotkeyCapture("录入已超时，请重新录入", "timeout");
    }, 30_000);
    reportFrontendEvent({
      event: "voice_hotkey_capture",
      phase: "started",
      result: "passed",
      reason: preheld.length > 0 ? "waiting_preheld_release" : "ready",
    });
  } catch (error) {
    voiceHotkeyMessage.value = error instanceof Error ? error.message : String(error);
    reportFrontendEvent({
      event: "voice_hotkey_capture",
      phase: "started",
      result: "failed",
      reason: "capture_start_failed",
    });
  } finally {
    if (requestId === voiceCaptureRequestId) captureStartingVoiceHotkey.value = false;
  }
}

/** 结束原生捕获会话并清理前端状态；调用方决定保存还是取消。 */
async function stopVoiceHotkeyCaptureSession(): Promise<KeyCode[]> {
  const draft = [...voiceCaptureDisplay.value];
  try {
    await stopShortcutCapture();
  } catch {
    voiceHotkeyMessage.value = "停止快捷键录入失败，请重试；若仍失败请重启应用";
    reportFrontendEvent({
      event: "voice_hotkey_capture",
      phase: "stopping",
      result: "failed",
      reason: "capture_stop_failed",
    });
    throw new Error("capture_stop_failed");
  }
  voiceCaptureRequestId += 1;
  captureStartingVoiceHotkey.value = false;
  capturingVoiceHotkey.value = false;
  waitingPreheldRelease.value = false;
  if (voiceCaptureTimeout !== null) window.clearTimeout(voiceCaptureTimeout);
  voiceCaptureTimeout = null;
  voiceCapturePressed.clear();
  voiceCapturePressedCount.value = 0;
  voiceCaptureDisplay.value = [];
  return draft;
}

async function cancelVoiceHotkeyCapture(
  message = "已取消录入",
  reason = "user_cancelled",
): Promise<void> {
  try {
    await stopVoiceHotkeyCaptureSession();
  } catch {
    return;
  }
  voiceHotkeyMessage.value = message;
  reportFrontendEvent({
    event: "voice_hotkey_capture",
    phase: "completed",
    result: reason === "timeout" ? "failed" : "passed",
    reason,
  });
}

function reservedVoiceHotkeyReason(keys: KeyCode[]): string | null {
  const normalized = new Set(keys);
  const hasWindows = normalized.has("left_windows") || normalized.has("right_windows");
  const hasControl = normalized.has("left_control") || normalized.has("right_control");
  const hasAlt = normalized.has("left_alt") || normalized.has("right_alt");
  if (hasWindows && normalized.has("l")) return "reserved_win_l";
  if (hasControl && hasAlt && normalized.has("delete")) {
    return "reserved_ctrl_alt_delete";
  }
  return null;
}

async function saveVoiceHotkeyCapture(): Promise<void> {
  if (voiceCapturePressedCount.value > 0) {
    voiceHotkeyMessage.value = "请先松开所有按键再保存";
    return;
  }
  if (voiceCaptureDisplay.value.length === 0) {
    voiceHotkeyMessage.value = "请先按下要保存的快捷键";
    return;
  }
  const reservedReason = reservedVoiceHotkeyReason(voiceCaptureDisplay.value);
  if (reservedReason) {
    voiceHotkeyMessage.value = reservedReason === "reserved_win_l"
      ? "Win + L 是 Windows 保留组合，不能设为按住说话快捷键"
      : "Ctrl + Alt + Delete 是 Windows 安全组合，不能设为按住说话快捷键";
    reportFrontendEvent({
      event: "voice_hotkey_capture",
      phase: "validating",
      result: "failed",
      reason: reservedReason,
    });
    return;
  }
  let keys: KeyCode[];
  try {
    keys = await stopVoiceHotkeyCaptureSession();
  } catch {
    return;
  }
  const saved = await applyVoiceHotkey(keys);
  if (!saved) {
    reportFrontendEvent({
      event: "voice_hotkey_capture",
      phase: "completed",
      result: "failed",
      reason: "settings_save_failed",
    });
    return;
  }
  appendWetypeChordNotice(keys);
  reportFrontendEvent({
    event: "voice_hotkey_capture",
    phase: "completed",
    result: "passed",
    reason: `saved_key_count_${keys.length}`,
  });
}

function clearVoiceHotkeyCaptureDraft(): void {
  voiceCaptureDisplay.value = [];
  voiceHotkeyMessage.value = "草稿已清空，请重新按键";
  reportFrontendEvent({
    event: "voice_hotkey_capture",
    phase: "updated",
    result: "passed",
    reason: "draft_cleared",
  });
}

/**
 * 落盘组合不是微信输入法语音键（默认 左 Ctrl + 左 Win）时提醒：按住说话会把
 * 该组合注入系统来唤起微信输入法语音，组合不一致则按住说话无法生效——这正是
 * "能录上 Win+右 Ctrl 反而没用"的原因。默认组合与推断落盘不需要这句提醒。
 */
function appendWetypeChordNotice(keys: KeyCode[]): void {
  if (!voiceHotkeyMessage.value.startsWith("按住说话快捷键已设为")) return;
  const normalized = [...keys].sort().join("+");
  if ([...DEFAULT_VOICE_HOTKEY_KEYS].sort().join("+") === normalized) return;
  voiceHotkeyMessage.value +=
    "。注意：按住说话会把该组合注入系统来唤起微信输入法语音，若与微信输入法语音键不一致将无法生效";
}

async function acceptVoiceCaptureEdge(edge: ShortcutCaptureEdge): Promise<void> {
  if (!capturingVoiceHotkey.value) return;
  // 后端只在 preheld 键全部松开后才开始投递边沿：第一条边沿即已武装。
  if (waitingPreheldRelease.value) {
    waitingPreheldRelease.value = false;
    voiceHotkeyMessage.value = "";
  }
  const { key, isPressed } = edge;
  if (!isPressed) {
    voiceCapturePressed.delete(key);
    voiceCapturePressedCount.value = voiceCapturePressed.size;
    return;
  }
  if (key === "escape" && voiceCapturePressed.size === 0) {
    await cancelVoiceHotkeyCapture();
    return;
  }
  voiceCapturePressed.add(key);
  voiceCapturePressedCount.value = voiceCapturePressed.size;
  // 持续编辑草稿：松手不自动定稿。输入法只放行半截组合时，用户可以在同一
  // 会话里单独补按缺少的键；第三方重放副本也只会去重合并。
  if (!voiceCaptureDisplay.value.includes(key)) {
    voiceCaptureDisplay.value = [...voiceCaptureDisplay.value, key];
  }
}

function handleVoiceCaptureBlur(): void {
  if (capturingVoiceHotkey.value || captureStartingVoiceHotkey.value) {
    void cancelVoiceHotkeyCapture("窗口失去焦点，已取消录入", "window_blurred");
  }
}

async function refreshVoiceHotkey() {
  try {
    voiceHotkey.value = await getVoiceHoldHotkey();
  } catch (error) {
    voiceHotkeyMessage.value = error instanceof Error ? error.message : String(error);
  }
}

watch(
  () => props.runtime?.platform.connection,
  (snapshot) => {
    if (snapshot) connection.value = snapshot;
  },
  { immediate: true },
);

watch(
  () => props.runtime?.platform.audio,
  (snapshot) => {
    if (snapshot) audio.value = snapshot;
  },
  { immediate: true },
);

const connectionActive = computed(() =>
  [
    "connecting",
    "discovering",
    "awaiting_capabilities",
    "ready",
    "streaming",
    "draining",
    "reconnecting",
    "suspended",
  ].includes(connection.value.phase),
);

const atvvReady = computed(() =>
  ["ready", "streaming", "draining"].includes(connection.value.phase),
);

const audioBusy = computed(() => ["streaming", "draining"].includes(audio.value.phase));

const wasapiReady = computed(() =>
  ["ready", "streaming", "draining"].includes(audio.value.phase),
);

const virtualCableEndpoints = computed(() =>
  audioEndpoints.value.filter((endpoint) => endpoint.isVirtualCableCandidate),
);

const virtualCableInstalled = computed(() => virtualCableEndpoints.value.length > 0);

const phaseTone = computed(() => {
  if (connection.value.phase === "failed") return "error";
  if (connection.value.phase === "streaming") return "active";
  if (connection.value.phase === "ready") return "success";
  if (connectionActive.value) return "warning";
  return "pending";
});

const phaseDetail = computed(() => {
  if (connection.value.lastError) return connection.value.lastError;
  if (connection.value.capabilities) return "语音功能已确认，可以按住遥控器语音键说话";
  return "连接后即可使用遥控器语音键";
});

const audioTone = computed(() => {
  if (audio.value.phase === "failed") return "error";
  if (audio.value.phase === "streaming") return "active";
  if (audio.value.phase === "ready") return "success";
  if (audio.value.phase === "draining") return "warning";
  return "pending";
});

const audioDetail = computed(() => {
  if (audio.value.lastError) return audio.value.lastError;
  if (audio.value.selectedEndpointName) return "语音会写入选中的设备";
  return "不会自动改动系统默认设备，需要在这里明确选择";
});

async function refreshConnection() {
  try {
    connection.value = await getConnectionSnapshot();
  } catch (error) {
    operationMessage.value = error instanceof Error ? error.message : String(error);
  }
}

async function refreshAudio() {
  try {
    audio.value = await getAudioSnapshot();
    return true;
  } catch (error) {
    audioMessage.value = error instanceof Error ? error.message : String(error);
    return false;
  }
}

async function scan() {
  scanning.value = true;
  operationMessage.value = "";
  scanMessage.value = "正在寻找小米遥控器…";
  try {
    devices.value = await scanPairedRemotes();
    scanMessage.value = devices.value.length
      ? `找到 ${devices.value.length} 个已配对的小米遥控器`
      : "没有找到已配对的小米遥控器";
  } catch (error) {
    devices.value = [];
    scanMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    scanning.value = false;
  }
}

async function connect(device: PairedRemote) {
  connectingDeviceId.value = device.id;
  operationMessage.value = "";
  try {
    connection.value = await connectRemote(device.id);
    operationMessage.value = "已连接，正在确认语音功能";
  } catch (error) {
    operationMessage.value = error instanceof Error ? error.message : String(error);
    await refreshConnection();
  } finally {
    connectingDeviceId.value = "";
  }
}

async function disconnect() {
  disconnecting.value = true;
  operationMessage.value = "";
  try {
    connection.value = await disconnectRemote();
    operationMessage.value = "遥控器连接已释放，本次运行已停止自动重连";
  } catch (error) {
    operationMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    disconnecting.value = false;
  }
}

async function detectAudioEndpoints(autoSelectVirtualCable: boolean) {
  scanningAudio.value = true;
  audioMessage.value = "正在读取语音设备…";
  try {
    audioEndpoints.value = await listAudioEndpoints();
    audioScanComplete.value = true;
    const virtualCables = audioEndpoints.value.filter(
      (endpoint) => endpoint.isVirtualCableCandidate,
    );
    if (virtualCables.length === 1 && autoSelectVirtualCable && !audio.value.selectedEndpointId) {
      await chooseAudioEndpoint(virtualCables[0], true);
      return;
    }
    audioMessage.value = virtualCables.length
      ? `已检测到 ${virtualCables.length} 个 VB-CABLE 语音设备`
      : "未检测到 VB-CABLE；安装完成后需要重启电脑，再重新检测";
  } catch (error) {
    audioEndpoints.value = [];
    audioScanComplete.value = true;
    audioMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    scanningAudio.value = false;
  }
}

async function scanAudio() {
  await detectAudioEndpoints(false);
  // 用户主动读取端点 = 想看列表；选好即收起（每次只用一个端点）。
  showEndpointList.value = audioEndpoints.value.length > 0;
}

async function chooseAudioEndpoint(endpoint: AudioEndpoint, automatic = false) {
  selectingEndpointId.value = endpoint.id;
  audioMessage.value = "正在打开语音设备…";
  try {
    audio.value = await selectAudioEndpoint(endpoint.id);
    audioMessage.value = automatic
      ? `已自动选择 ${endpoint.name}`
      : `已选择 ${endpoint.name}`;
    showEndpointList.value = false;
  } catch (error) {
    audioMessage.value = error instanceof Error ? error.message : String(error);
    await refreshAudio();
  } finally {
    selectingEndpointId.value = "";
  }
}

async function openVbCablePage() {
  openingVbCablePage.value = true;
  try {
    await openVbCableDownloadPage();
    audioMessage.value = "已打开 VB-CABLE 官方下载页面；安装时需要管理员权限，完成后请重启电脑";
  } catch (error) {
    audioMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    openingVbCablePage.value = false;
  }
}

async function initializeAudio() {
  const restoredAudio = await refreshAudio();
  await detectAudioEndpoints(restoredAudio);
}

onMounted(async () => {
  window.addEventListener("blur", handleVoiceCaptureBlur);
  void refreshConnection();
  void initializeAudio();
  void refreshVoiceHotkey();
  pollTimer = setInterval(() => {
    void refreshConnection();
    void refreshAudio();
  }, 1_000);
  const stopCaptureEdges = await subscribeShortcutCaptureEdges((edge) => {
    void acceptVoiceCaptureEdge(edge);
  });
  if (unmounted) {
    stopCaptureEdges();
    return;
  }
  unlistenVoiceCapture = stopCaptureEdges;
});

onUnmounted(() => {
  unmounted = true;
  window.removeEventListener("blur", handleVoiceCaptureBlur);
  if (pollTimer) clearInterval(pollTimer);
  if (voiceCaptureTimeout !== null) window.clearTimeout(voiceCaptureTimeout);
  voiceCaptureTimeout = null;
  unlistenVoiceCapture?.();
  unlistenVoiceCapture = null;
  void stopShortcutCapture().catch(() => undefined);
});
</script>

<template>
  <section>
    <header class="page-header">
      <div>
        <h1>连接</h1>
      </div>
      <span class="badge" :class="phaseTone">{{ connectionPhaseLabel(connection.phase) }}</span>
    </header>

    <div class="two-column">
      <article class="card">
        <div class="card-title-row">
          <div>
            <h2>遥控器连接</h2>
            <p class="muted">连接已配对的小米遥控器。</p>
          </div>
          <button
            class="primary-button"
            type="button"
            :disabled="scanning || connectionActive || !runtime?.platform.bleScanAvailable"
            @click="scan"
          >
            {{ scanning ? "扫描中…" : "扫描已配对设备" }}
          </button>
        </div>

        <div class="status-panel" aria-live="polite">
          <div class="status-copy">
            <div class="status-heading">
              <span class="status-dot" :class="phaseTone"></span>
              <strong>{{ connection.remoteName ?? connectionPhaseLabel(connection.phase) }}</strong>
            </div>
            <small>{{ phaseDetail }}</small>
          </div>
          <button
            v-if="connectionActive"
            class="secondary-button status-action"
            type="button"
            :disabled="disconnecting"
            @click="disconnect"
          >
            {{ disconnecting ? "断开中…" : "断开" }}
          </button>
        </div>

        <p class="muted scan-summary">{{ scanMessage }}</p>
        <p v-if="operationMessage" class="operation-message">{{ operationMessage }}</p>

        <ul v-if="devices.length" class="device-list">
          <li v-for="device in devices" :key="device.id">
            <div><strong>{{ device.name }}</strong><small>{{ remoteModelLabel(device.model) }}</small></div>
            <button
              type="button"
              :disabled="connectionActive || Boolean(connectingDeviceId)"
              @click="connect(device)"
            >
              {{ connectingDeviceId === device.id ? "连接中…" : "连接" }}
            </button>
          </li>
        </ul>

        <div class="setting-list compact two-col">
          <div class="setting-row">
            <strong>设备型号</strong>
            <span>{{ remoteModelLabel(connection.remoteModel) }}</span>
          </div>
          <div class="setting-row">
            <strong>电池电量</strong>
            <BatteryIndicator :connection="connection" />
          </div>
          <div class="setting-row">
            <strong>语音按键</strong>
            <span>{{ atvvReady ? "已就绪" : "正在确认" }}</span>
          </div>
          <div class="setting-row">
            <strong>睡眠唤醒自动重连</strong>
            <span>{{ connection.powerNotificationsAvailable ? "已启用" : "暂不可用" }}</span>
          </div>
          <div class="setting-row">
            <strong>按住说话快捷键</strong>
            <span>{{ voiceHoldHotkeyLabel(voiceHotkey) }}</span>
          </div>
        </div>
        <p class="muted voice-hotkey-row">按住遥控器语音键说话，松开即停止；语音会送入右侧选中的设备，由微信输入法等工具转成文字。说话时按的是遥控器语音键，此快捷键是应用替它向系统注入的组合，用于唤起微信输入法语音——因此必须与微信输入法设置的语音键一致，否则按住说话无法生效。默认快捷键：{{ chordLabel({ keys: DEFAULT_VOICE_HOTKEY_KEYS }) }}。</p>
        <div class="button-row voice-hotkey-presets">
          <button
            class="secondary-button"
            type="button"
            :disabled="
              savingVoiceHotkey ||
              captureStartingVoiceHotkey ||
              !runtime?.platform.windowsApiAvailable
            "
            @click="
              capturingVoiceHotkey
                ? cancelVoiceHotkeyCapture()
                : beginVoiceHotkeyCapture()
            "
          >
            {{ capturingVoiceHotkey ? "录入中…（按 Esc 取消）" : "修改快捷键" }}
          </button>
          <button
            :class="
              presetIsActive(DEFAULT_VOICE_HOTKEY_KEYS) ? 'primary-button' : 'secondary-button'
            "
            type="button"
            :disabled="
              savingVoiceHotkey ||
              capturingVoiceHotkey ||
              !runtime?.platform.windowsApiAvailable ||
              presetIsActive(DEFAULT_VOICE_HOTKEY_KEYS)
            "
            @click="applyVoiceHotkey(DEFAULT_VOICE_HOTKEY_KEYS)"
          >
            {{ chordLabel({ keys: DEFAULT_VOICE_HOTKEY_KEYS }) }}（默认）
          </button>
          <button
            :class="activeVoiceHotkeyKeys ? 'secondary-button' : 'primary-button'"
            type="button"
            :disabled="
              savingVoiceHotkey ||
              capturingVoiceHotkey ||
              !runtime?.platform.windowsApiAvailable ||
              !activeVoiceHotkeyKeys
            "
            @click="applyVoiceHotkey([])"
          >
            关闭
          </button>
        </div>
        <div v-if="capturingVoiceHotkey" class="voice-hotkey-recorder">
          <p class="capture-display voice-hotkey-capture">
            {{
              voiceCaptureDisplay.length
                ? chordLabel({ keys: voiceCaptureDisplay })
                : waitingPreheldRelease
                  ? "检测到仍有按住的按键，请先松开所有按键"
                  : "请按下快捷键"
            }}
          </p>
          <p class="muted scan-summary">
            可以同时按下组合，也可以逐个轻按每个键；松手不会自动保存，确认无误后点“保存”。
          </p>
          <div class="button-row voice-hotkey-capture-actions">
            <button
              class="secondary-button"
              type="button"
              :disabled="voiceCaptureDisplay.length === 0"
              @click="clearVoiceHotkeyCaptureDraft"
            >
              清空
            </button>
            <button class="secondary-button" type="button" @click="cancelVoiceHotkeyCapture()">
              取消
            </button>
            <button
              class="primary-button"
              type="button"
              :disabled="
                voiceCaptureDisplay.length === 0 ||
                waitingPreheldRelease ||
                voiceCapturePressedCount > 0
              "
              @click="saveVoiceHotkeyCapture"
            >
              保存
            </button>
          </div>
        </div>
        <p class="muted scan-summary">{{ voiceHotkeyMessage }}</p>
        <details class="usage-hint-details">
          <summary>微信输入法使用步骤（点开查看）</summary>
          <ol>
            <li>语音设备选择 CABLE Input；</li>
            <li>在微信输入法的语音设置里，把麦克风设为 CABLE Output；若没有这个选项，把系统默认录音设备设为 CABLE Output；</li>
            <li>在目标应用的文本框内切换到微信输入法（看任务栏输入指示器确认）；</li>
            <li>按住遥控器语音键约半秒以上再说话，松开后等待文字出现（需要联网）。快速点按不出文字是微信输入法自己的最短按住要求，不是故障。遥控器语音键自带的 F5 按键会被应用自动屏蔽，物理键盘的 F5 不受影响。</li>
          </ol>
        </details>
      </article>

      <article class="card">
        <div class="card-title-row">
          <div>
            <h2>语音设备</h2>
            <p class="muted">选择语音写入的设备。使用微信输入法请选 CABLE Input。</p>
          </div>
          <button
            class="secondary-button"
            type="button"
            :disabled="scanningAudio || audioBusy || !runtime?.platform.windowsApiAvailable"
            @click="scanAudio()"
          >
            {{ scanningAudio ? "读取中…" : "刷新设备列表" }}
          </button>
        </div>

        <div class="status-panel" aria-live="polite">
          <div class="status-copy">
            <div class="status-heading">
              <span class="status-dot" :class="audioTone"></span>
              <strong>{{ audio.selectedEndpointName ?? audioPhaseLabel(audio.phase) }}</strong>
            </div>
            <small>{{ audioDetail }}</small>
          </div>
        </div>

        <p class="muted scan-summary">{{ audioMessage }}</p>
        <div v-if="audioEndpoints.length" class="endpoint-select-row">
          <button
            class="secondary-button"
            type="button"
            @click="showEndpointList = !showEndpointList"
          >
            {{ showEndpointList ? "收起列表" : audio.selectedEndpointId ? "更换设备" : "选择设备" }}
          </button>
          <span v-if="!showEndpointList" class="muted endpoint-count">
            共 {{ audioEndpoints.length }} 个设备可选
          </span>
        </div>
        <ul v-if="showEndpointList && audioEndpoints.length" class="device-list endpoint-list">
          <li v-for="endpoint in audioEndpoints" :key="endpoint.id">
            <div>
              <strong>{{ endpoint.name }}</strong>
              <strong
                v-if="isRecommendedVoiceEndpoint(endpoint)"
                class="endpoint-recommend"
              >
                推荐
              </strong>
              <small v-else>其他音频设备</small>
            </div>
            <button
              type="button"
              :disabled="audioBusy || Boolean(selectingEndpointId) || audio.selectedEndpointId === endpoint.id"
              @click="chooseAudioEndpoint(endpoint)"
            >
              {{
                selectingEndpointId === endpoint.id
                  ? "正在启用…"
                  : audio.selectedEndpointId === endpoint.id
                    ? "当前设备"
                    : "选择"
              }}
            </button>
          </li>
        </ul>

        <div class="setting-list compact two-col">
          <div class="setting-row">
            <strong>语音设备</strong>
            <span>{{ wasapiReady ? audioPhaseLabel(audio.phase) : "待选择" }}</span>
          </div>
        </div>

        <div v-if="audioScanComplete && !virtualCableInstalled" class="info-callout warning vb-cable-callout">
          <div>
            <strong>需要安装 VB-CABLE</strong>
            <p>由 VB-Audio 提供的免费虚拟声卡。安装需要管理员权限，完成后需重启电脑。</p>
          </div>
          <div class="button-row">
            <button class="primary-button" type="button" :disabled="openingVbCablePage" @click="openVbCablePage">
              {{ openingVbCablePage ? "正在打开…" : "打开官方下载页" }}
            </button>
            <button class="secondary-button" type="button" :disabled="scanningAudio" @click="scanAudio()">
              重新检测
            </button>
          </div>
        </div>
        <div v-else class="info-callout" :class="{ warning: !wasapiReady }">
          {{
            wasapiReady
              ? "语音设备已就绪。"
              : virtualCableInstalled
                ? "已检测到 VB-CABLE。这里选择 CABLE Input；在微信输入法的语音设置里选择 CABLE Output。"
                : "正在检测 VB-CABLE…"
          }}
        </div>
      </article>
    </div>
  </section>
</template>
