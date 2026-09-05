<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
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
const audioMessage = ref("尚未读取 Windows 输出端点");
const voiceHotkey = ref<KeyChord | null>(null);
const savingVoiceHotkey = ref(false);
const voiceHotkeyMessage = ref("按住说话快捷键尚未读取");
let pollTimer: ReturnType<typeof setInterval> | undefined;

const voiceHotkeyPresets: Array<{ label: string; keys: string[] }> = [
  { label: "微信输入法（默认）", keys: ["left_control", "left_windows"] },
  { label: "关闭", keys: [] },
  { label: "Windows 语音输入", keys: ["left_windows", "h"] },
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
  if (connection.value.capabilities) {
    return `${connection.value.capabilities.sampleRate / 1000} kHz · 帧 ${connection.value.capabilities.frameSize} 字节 · 已解码 ${connection.value.decodedSamples.toLocaleString()} 个采样`;
  }
  return "必须经过服务发现、通知订阅和能力确认，才会进入 ATVV 就绪";
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
  if (audio.value.selectedEndpointName) {
    return `已提交 ${audio.value.submittedSamples.toLocaleString()} 个采样 · 队列 ${audio.value.queuedSamples.toLocaleString()}`;
  }
  return "无线麦不会自动使用或修改系统默认输出，必须在这里明确选择端点";
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
  scanMessage.value = "正在读取 Windows 已配对的 BLE 设备…";
  try {
    devices.value = await scanPairedRemotes();
    scanMessage.value = devices.value.length
      ? `找到 ${devices.value.length} 个已批准名称的候选设备`
      : "没有找到已配对且名称精确匹配的 RC001/RC003 候选设备";
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
    operationMessage.value = "已建立 BLE 会话，正在等待遥控器返回 ATVV 能力";
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
  audioMessage.value = "正在枚举 Windows 活动输出端点…";
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
      ? `已检测到 ${virtualCables.length} 个 VB-CABLE 输出端点`
      : "未检测到 VB-CABLE；安装完成后需要重启 Windows，再重新检测";
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
  audioMessage.value = "正在初始化所选 WASAPI 输出端点…";
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
    audioMessage.value = "已打开 VB-CABLE 官方下载页面；安装时需要管理员权限，完成后请重启 Windows";
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
  void refreshConnection();
  void initializeAudio();
  void refreshVoiceHotkey();
  pollTimer = setInterval(() => {
    void refreshConnection();
    void refreshAudio();
  }, 1_000);
});

onUnmounted(() => {
  if (pollTimer) clearInterval(pollTimer);
});
</script>

<template>
  <section>
    <header class="page-header">
      <div>
        <h1>连接与语音</h1>
        <p>连接状态来自实际 WinRT GATT 会话；语音输出走你明确选择的端点，不改动系统默认设备。</p>
      </div>
      <span class="badge" :class="phaseTone">{{ connectionPhaseLabel(connection.phase) }}</span>
    </header>

    <div class="two-column">
      <article class="card">
        <div class="card-title-row">
          <div>
            <h2>RC001 / RC003 蓝牙连接</h2>
            <p class="muted">只扫描 Windows 中已经配对、且名称在白名单内的 BLE 设备。</p>
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
          <span class="status-dot" :class="phaseTone"></span>
          <div>
            <strong>{{ connection.remoteName ?? connectionPhaseLabel(connection.phase) }}</strong>
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
            <strong>ATVV / 16 kHz</strong>
            <span>{{ atvvReady ? "BLE 会话已就绪" : "等待能力确认" }}</span>
          </div>
          <div class="setting-row">
            <strong>睡眠恢复通知</strong>
            <span>{{ connection.powerNotificationsAvailable ? "已注册" : "当前不可用" }}</span>
          </div>
          <div class="setting-row">
            <strong>按住说话快捷键</strong>
            <span>{{ voiceHoldHotkeyLabel(voiceHotkey) }}</span>
          </div>
        </div>
        <p class="muted voice-hotkey-row">按住说话：按下语音键 = 按下该快捷键并开始传声，松开 = 释放；语音写入右侧所选输出端点，由目标程序识别成文字。v1 默认适配微信输入法（左 Ctrl+左 Win）。</p>
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
        <p class="muted scan-summary">{{ voiceHotkeyMessage }}</p>
        <details class="usage-hint-details">
          <summary>微信输入法使用步骤（点开查看）</summary>
          <ol>
            <li>输出端点选择 CABLE Input；</li>
            <li>微信输入法的麦克风设为 CABLE Output（在其"语音输入"设置里；若无此项，把系统默认录音设备设为 CABLE Output）；</li>
            <li>在目标应用的文本框内切换到微信输入法（看任务栏输入指示器确认）；</li>
            <li>按住遥控器语音键约半秒以上说话，松开等待文字上屏（需联网，云端识别）。快按不出文字是微信输入法自身的最短按住要求，不是故障。应用会自动屏蔽遥控器语音键附带的 F5 键盘事件，物理键盘的 F5 不受影响。</li>
          </ol>
        </details>
      </article>

      <article class="card">
        <div class="card-title-row">
          <div>
            <h2>Windows 语音输出</h2>
            <p class="muted">选择要接收遥控器 PCM 的输出端点，例如 CABLE Input。</p>
          </div>
          <button
            class="secondary-button"
            type="button"
            :disabled="scanningAudio || audioBusy || !runtime?.platform.windowsApiAvailable"
            @click="scanAudio()"
          >
            {{ scanningAudio ? "读取中…" : "读取输出端点" }}
          </button>
        </div>

        <div class="status-panel" aria-live="polite">
          <span class="status-dot" :class="audioTone"></span>
          <div>
            <strong>{{ audio.selectedEndpointName ?? audioPhaseLabel(audio.phase) }}</strong>
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
            {{ showEndpointList ? "收起端点列表" : audio.selectedEndpointId ? "更换端点" : "选择端点" }}
          </button>
          <span v-if="!showEndpointList" class="muted endpoint-count">
            共 {{ audioEndpoints.length }} 个端点可选
          </span>
        </div>
        <ul v-if="showEndpointList && audioEndpoints.length" class="device-list endpoint-list">
          <li v-for="endpoint in audioEndpoints" :key="endpoint.id">
            <div>
              <strong>{{ endpoint.name }}</strong>
              <small>{{ endpoint.isVirtualCableCandidate ? "虚拟麦克风候选端点" : "普通 Windows 输出端点" }}</small>
            </div>
            <button
              type="button"
              :disabled="audioBusy || Boolean(selectingEndpointId) || audio.selectedEndpointId === endpoint.id"
              @click="chooseAudioEndpoint(endpoint)"
            >
              {{
                selectingEndpointId === endpoint.id
                  ? "初始化中…"
                  : audio.selectedEndpointId === endpoint.id
                    ? "当前端点"
                    : "选择"
              }}
            </button>
          </li>
        </ul>

        <div class="setting-list compact two-col">
          <div class="setting-row">
            <strong>WASAPI 端点</strong>
            <span>{{ wasapiReady ? audioPhaseLabel(audio.phase) : "等待明确选择" }}</span>
          </div>
          <div class="setting-row"><strong>会话代次</strong><span>{{ connection.generation }}</span></div>
        </div>

        <div v-if="audioScanComplete && !virtualCableInstalled" class="info-callout warning vb-cable-callout">
          <div>
            <strong>需要安装 VB-CABLE</strong>
            <p>它由 VB-Audio 提供，属于 Donationware。安装需要管理员权限，完成后必须重启 Windows。</p>
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
              ? "所选端点已通过 WASAPI 初始化；仍需在 Windows 上分别完成真实 RC001、RC003 与 VB-CABLE 回环验收。"
              : virtualCableInstalled
                ? "已检测到 VB-CABLE。请选择 CABLE Input；在输入法或语音软件中选择 CABLE Output。"
                : "正在检测 VB-CABLE 音频端点。"
          }}
        </div>
      </article>
    </div>
  </section>
</template>
