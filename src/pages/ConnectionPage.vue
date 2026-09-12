<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import type {
  AudioEndpoint,
  AudioSnapshot,
  ConnectionSnapshot,
  KeyChord,
  KeyCode,
  PairedRemote,
  RuntimeSnapshot,
  ShortcutCaptureEdge,
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
let pollTimer: ReturnType<typeof setInterval> | undefined;

const voiceHotkeyPresets: Array<{ label: string; keys: string[] }> = [
  { label: "微信输入法（默认）", keys: ["left_control", "left_windows"] },
  { label: "Windows 听写（Win + H）", keys: ["left_windows", "h"] },
  { label: "关闭", keys: [] },
];

const activeVoiceHotkeyKeys = computed(() =>
  voiceHotkey.value ? [...voiceHotkey.value.keys].sort().join("+") : "",
);

function presetIsActive(keys: string[]): boolean {
  return [...keys].sort().join("+") === activeVoiceHotkeyKeys.value;
}

async function applyVoiceHotkey(keys: string[]) {
  savingVoiceHotkey.value = true;
  voiceHotkeyMessage.value = "";
  try {
    voiceHotkey.value = await setVoiceHoldHotkey(
      keys.length ? { keys: [...keys] } : null,
    );
    voiceHotkeyMessage.value = voiceHotkey.value
      ? `按住说话快捷键已设为 ${voiceHoldHotkeyLabel(voiceHotkey.value)}`
      : "按住说话快捷键已关闭，语音键仅输出语音";
  } catch (error) {
    voiceHotkeyMessage.value = error instanceof Error ? error.message : String(error);
    await refreshVoiceHotkey();
  } finally {
    savingVoiceHotkey.value = false;
  }
}

async function refreshVoiceHotkey() {
  try {
    voiceHotkey.value = await getVoiceHoldHotkey();
  } catch (error) {
    voiceHotkeyMessage.value = error instanceof Error ? error.message : String(error);
  }
}

// —— 自定义快捷键录入：与「按键映射」页同一套捕获机制（原生钩子 + 前端双源）。 ——

/** KeyboardEvent.code → KeyCode（serde snake_case）。 */
function codeToKeyCode(code: string): KeyCode | null {
  const modifierMap: Record<string, KeyCode> = {
    ControlLeft: "left_control",
    ControlRight: "right_control",
    ShiftLeft: "left_shift",
    ShiftRight: "right_shift",
    AltLeft: "left_alt",
    AltRight: "right_alt",
    MetaLeft: "left_windows",
    MetaRight: "right_windows",
  };
  if (modifierMap[code]) return modifierMap[code];
  const named: Record<string, KeyCode> = {
    Enter: "enter",
    Space: "space",
    Tab: "tab",
    Backspace: "backspace",
    Escape: "escape",
    ArrowLeft: "left",
    ArrowUp: "up",
    ArrowRight: "right",
    ArrowDown: "down",
    Home: "home",
    End: "end",
    PageUp: "page_up",
    PageDown: "page_down",
    Insert: "insert",
    Delete: "delete",
    ContextMenu: "apps",
    VolumeMute: "volume_mute",
    VolumeUp: "volume_up",
    VolumeDown: "volume_down",
  };
  if (named[code]) return named[code];
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return letter[1].toLowerCase();
  const digit = /^Digit([0-9])$/.exec(code);
  if (digit) return `digit${digit[1]}`;
  const functionKey = /^F([1-9]|1[0-2])$/.exec(code);
  if (functionKey) return `f${functionKey[1]}`;
  return null;
}

const MODIFIER_KEYS = new Set<KeyCode>([
  "left_control",
  "right_control",
  "left_shift",
  "right_shift",
  "left_alt",
  "right_alt",
  "left_windows",
  "right_windows",
]);
const CAPTURE_MODIFIER_OPTIONS: Array<{ key: KeyCode; label: string }> = [
  { key: "left_control", label: "左 Ctrl" },
  { key: "left_shift", label: "左 Shift" },
  { key: "left_alt", label: "左 Alt" },
  { key: "left_windows", label: "左 Win" },
  { key: "right_control", label: "右 Ctrl" },
  { key: "right_shift", label: "右 Shift" },
  { key: "right_alt", label: "右 Alt" },
  { key: "right_windows", label: "右 Win" },
];

const capturingHotkey = ref(false);
const captureStarting = ref(false);
const captureDisplay = ref<string[]>([]);
const safeCaptureMode = ref(false);
const capturePressedKeys = new Set<KeyCode>();
const selectedCaptureModifiers = reactive(new Set<KeyCode>());
const pressedCaptureModifiers = new Set<KeyCode>();
let capturedChord: KeyCode[] | null = null;
let captureTimeout: number | null = null;
let captureRequestId = 0;
let unlistenShortcutCapture: (() => void) | null = null;
let pageUnmounted = false;

function toggleCaptureModifier(key: KeyCode): void {
  if (!capturingHotkey.value || capturedChord) return;
  if (selectedCaptureModifiers.has(key)) selectedCaptureModifiers.delete(key);
  else selectedCaptureModifiers.add(key);
  captureDisplay.value = [...selectedCaptureModifiers];
}

async function beginHotkeyCapture(): Promise<void> {
  if (capturingHotkey.value || captureStarting.value) return;
  const requestId = ++captureRequestId;
  captureStarting.value = true;
  voiceHotkeyMessage.value = "";
  try {
    await startShortcutCapture();
    if (pageUnmounted || requestId !== captureRequestId) {
      await stopShortcutCapture().catch(() => undefined);
      return;
    }
    capturePressedKeys.clear();
    capturedChord = null;
    selectedCaptureModifiers.clear();
    pressedCaptureModifiers.clear();
    captureDisplay.value = [];
    capturingHotkey.value = true;
    if (captureTimeout !== null) window.clearTimeout(captureTimeout);
    captureTimeout = window.setTimeout(() => {
      void finishHotkeyCapture("录入已超时，快捷键未修改");
    }, 15_000);
  } catch (error) {
    voiceHotkeyMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    if (requestId === captureRequestId) captureStarting.value = false;
  }
}

async function finishHotkeyCapture(message?: string): Promise<void> {
  captureRequestId += 1;
  captureStarting.value = false;
  capturingHotkey.value = false;
  if (captureTimeout !== null) window.clearTimeout(captureTimeout);
  captureTimeout = null;
  await stopShortcutCapture().catch(() => undefined);
  capturePressedKeys.clear();
  capturedChord = null;
  pressedCaptureModifiers.clear();
  if (message) voiceHotkeyMessage.value = message;
}

function handleCaptureBlur(): void {
  if (capturingHotkey.value || captureStarting.value) {
    void finishHotkeyCapture("窗口失去焦点，已取消录入，快捷键未修改");
  }
}

function acceptCapturedKey(code: KeyCode, isPressed: boolean, repeat = false): void {
  if (!capturingHotkey.value) return;
  if (!isPressed) {
    capturePressedKeys.delete(code);
    if (MODIFIER_KEYS.has(code)) pressedCaptureModifiers.delete(code);
    if (capturedChord) {
      captureDisplay.value = capturedChord;
      if (capturePressedKeys.size === 0) {
        const keys = capturedChord;
        void finishHotkeyCapture();
        void applyVoiceHotkey(keys);
      }
    } else {
      captureDisplay.value = safeCaptureMode.value
        ? [...selectedCaptureModifiers]
        : [...pressedCaptureModifiers];
    }
    return;
  }
  if (!repeat) capturePressedKeys.add(code);
  // 终止键确定后继续拦截，直到本次组合全部 UP 到齐，避免 Win+L 等
  // 系统快捷键在录入完成但物理键未松开时被 Windows 补执行。
  if (capturedChord) return;
  if (MODIFIER_KEYS.has(code)) {
    if (!repeat) pressedCaptureModifiers.add(code);
    if (safeCaptureMode.value) {
      voiceHotkeyMessage.value = "安全录入中：请松开键盘修饰键，并在界面中点击选择";
    } else {
      captureDisplay.value = [...pressedCaptureModifiers];
    }
    return;
  }
  if (safeCaptureMode.value && pressedCaptureModifiers.size > 0) {
    voiceHotkeyMessage.value = "未录入：请不要按住键盘修饰键；先在界面选择修饰键，再单独按主键";
    return;
  }
  const modifiers = safeCaptureMode.value
    ? [...selectedCaptureModifiers]
    : [...pressedCaptureModifiers];
  if (code === "escape" && modifiers.length === 0) {
    void finishHotkeyCapture("已取消录入，快捷键未修改");
    return;
  }
  const keys = [...modifiers, code];
  capturedChord = keys;
  captureDisplay.value = keys;
  voiceHotkeyMessage.value = `已录入 ${chordLabel({ keys })}，松开全部按键后保存`;
}

function handleCaptureKeydown(event: KeyboardEvent): void {
  if (!capturingHotkey.value) return;
  event.preventDefault();
  event.stopPropagation();
  const code = codeToKeyCode(event.code);
  if (code === null) return;
  acceptCapturedKey(code, true, event.repeat);
}

function handleCaptureKeyup(event: KeyboardEvent): void {
  if (!capturingHotkey.value) return;
  const code = codeToKeyCode(event.code);
  if (code) acceptCapturedKey(code, false);
}

watch(capturingHotkey, (active) => {
  if (!active) {
    selectedCaptureModifiers.clear();
    pressedCaptureModifiers.clear();
    captureDisplay.value = [];
  }
});

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

onMounted(() => {
  window.addEventListener("keydown", handleCaptureKeydown, true);
  window.addEventListener("keyup", handleCaptureKeyup, true);
  window.addEventListener("blur", handleCaptureBlur);
  void subscribeShortcutCaptureEdges(
    (edge: ShortcutCaptureEdge) => acceptCapturedKey(edge.key, edge.isPressed),
  ).then((stop) => {
    if (pageUnmounted) {
      stop();
      return;
    }
    unlistenShortcutCapture = stop;
  });
  void refreshConnection();
  void initializeAudio();
  void refreshVoiceHotkey();
  pollTimer = setInterval(() => {
    void refreshConnection();
    void refreshAudio();
  }, 1_000);
});

onUnmounted(() => {
  pageUnmounted = true;
  window.removeEventListener("keydown", handleCaptureKeydown, true);
  window.removeEventListener("keyup", handleCaptureKeyup, true);
  window.removeEventListener("blur", handleCaptureBlur);
  unlistenShortcutCapture?.();
  unlistenShortcutCapture = null;
  if (captureTimeout !== null) window.clearTimeout(captureTimeout);
  captureTimeout = null;
  void stopShortcutCapture();
  if (pollTimer) clearInterval(pollTimer);
});
</script>

<template>
  <section>
    <header class="page-header">
      <div>
        <h1>连接与语音</h1>
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
        <p class="muted voice-hotkey-row">按住遥控器语音键说话，松开即停止；语音会送入右侧选中的设备，由微信输入法、Windows 听写等工具转成文字。使用其他软件时选择与其一致的快捷键，或用「自定义录入…」录制。</p>
        <div class="button-row voice-hotkey-presets">
          <button
            v-for="preset in voiceHotkeyPresets"
            :key="preset.label"
            :class="presetIsActive(preset.keys) ? 'primary-button' : 'secondary-button'"
            type="button"
            :disabled="savingVoiceHotkey || !runtime?.platform.windowsApiAvailable || presetIsActive(preset.keys)"
            @click="applyVoiceHotkey(preset.keys)"
          >
            {{ preset.label }}
          </button>
        </div>
        <div class="custom-shortcut-row">
          <button
            class="chip"
            :class="{ selected: capturingHotkey }"
            type="button"
            :disabled="captureStarting || savingVoiceHotkey || !runtime?.platform.windowsApiAvailable"
            @click="capturingHotkey ? finishHotkeyCapture('已取消录入，快捷键未修改') : beginHotkeyCapture()"
          >
            {{ capturingHotkey ? "录入中…（按 Esc 取消）" : "自定义录入…" }}
          </button>
          <span v-if="capturingHotkey" class="capture-display">
            {{
              captureDisplay.length
                ? chordLabel({ keys: captureDisplay })
                : safeCaptureMode
                  ? "先选择修饰键"
                  : "请按下快捷键组合"
            }}
          </span>
        </div>
        <label
          class="toggle-row safe-capture-toggle"
          title="开启后，通过界面选择修饰键，键盘只需按主键。"
        >
          <span>安全录入模式</span>
          <input
            v-model="safeCaptureMode"
            type="checkbox"
            class="toggle-input"
            :disabled="capturingHotkey || captureStarting"
          />
          <small class="muted">直接录入无法完成或会触发系统动作时再开启。</small>
        </label>
        <template v-if="capturingHotkey && safeCaptureMode">
          <p class="muted scan-summary">请用鼠标选择修饰键，再只按一次主键。例如 Windows 听写：选「左 Win」后只按 H。</p>
          <div class="preset-grid">
            <button
              v-for="modifier in CAPTURE_MODIFIER_OPTIONS"
              :key="modifier.key"
              class="chip"
              :class="{ selected: selectedCaptureModifiers.has(modifier.key) }"
              type="button"
              @click="toggleCaptureModifier(modifier.key)"
            >
              {{ modifier.label }}
            </button>
          </div>
        </template>
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
        <details class="usage-hint-details">
          <summary>其他输入法 / Windows 听写使用步骤（点开查看）</summary>
          <ol>
            <li>语音设备保持 CABLE Input；把所用软件（或系统默认录音设备）的麦克风设为 CABLE Output。Windows 听写使用系统默认录音设备；</li>
            <li>把「按住说话快捷键」设为该软件的语音热键：Windows 听写可点上面的「Windows 听写（Win + H）」预设，其他软件用「自定义录入…」录制一致的组合；</li>
            <li>在目标应用的文本框内点入光标（Windows 听写需要文本框获得焦点后才会启动）；</li>
            <li>按住遥控器语音键说话，松开后等待文字出现。使用非微信输入法默认组合时不做输入法自动激活，属正常行为。</li>
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
              <small>{{ endpoint.isVirtualCableCandidate ? "推荐（微信输入法等语音工具使用）" : "其他音频设备" }}</small>
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
