<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import type {
  AudioEndpoint,
  AudioSnapshot,
  ConnectionSnapshot,
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
  listAudioEndpoints,
  scanPairedRemotes,
  selectAudioEndpoint,
} from "../lib/bridge";

const props = defineProps<{ runtime: RuntimeSnapshot | null }>();

const emptyConnection = (): ConnectionSnapshot => ({
  phase: "idle",
  remoteName: null,
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
const scanningAudio = ref(false);
const selectingEndpointId = ref("");
const audioMessage = ref("尚未读取 Windows 输出端点");
let pollTimer: ReturnType<typeof setInterval> | undefined;

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
  } catch (error) {
    audioMessage.value = error instanceof Error ? error.message : String(error);
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
      : "没有找到已配对且名称精确匹配的 RC003 候选设备";
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
    operationMessage.value = "已建立 BLE 会话，正在等待 RC003 返回 ATVV 能力";
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
    operationMessage.value = "RC003 连接已释放，本次运行已停止自动重连";
  } catch (error) {
    operationMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    disconnecting.value = false;
  }
}

async function scanAudio() {
  scanningAudio.value = true;
  audioMessage.value = "正在枚举 Windows 活动输出端点…";
  try {
    audioEndpoints.value = await listAudioEndpoints();
    audioMessage.value = audioEndpoints.value.length
      ? `找到 ${audioEndpoints.value.length} 个活动输出端点`
      : "没有找到可用的 Windows 输出端点";
  } catch (error) {
    audioEndpoints.value = [];
    audioMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    scanningAudio.value = false;
  }
}

async function chooseAudioEndpoint(endpoint: AudioEndpoint) {
  selectingEndpointId.value = endpoint.id;
  audioMessage.value = "正在初始化所选 WASAPI 输出端点…";
  try {
    audio.value = await selectAudioEndpoint(endpoint.id);
    audioMessage.value = `已选择 ${endpoint.name}`;
  } catch (error) {
    audioMessage.value = error instanceof Error ? error.message : String(error);
    await refreshAudio();
  } finally {
    selectingEndpointId.value = "";
  }
}

onMounted(() => {
  void refreshConnection();
  void refreshAudio();
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
        <p>连接状态来自实际 WinRT GATT 会话；BLE / ATVV 就绪不等于系统麦克风已经可用。</p>
      </div>
      <span class="badge" :class="phaseTone">{{ connectionPhaseLabel(connection.phase) }}</span>
    </header>

    <div class="two-column">
      <article class="card">
        <div class="card-title-row">
          <div>
            <h2>RC003 蓝牙连接</h2>
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
            <div><strong>{{ device.name }}</strong><small>已配对候选设备</small></div>
            <button
              type="button"
              :disabled="connectionActive || Boolean(connectingDeviceId)"
              @click="connect(device)"
            >
              {{ connectingDeviceId === device.id ? "连接中…" : "连接" }}
            </button>
          </li>
        </ul>
      </article>

      <article class="card">
        <div class="card-title-row">
          <div>
            <h2>Windows 语音输出</h2>
            <p class="muted">选择要接收 RC003 PCM 的输出端点，例如 CABLE Input。</p>
          </div>
          <button
            class="secondary-button"
            type="button"
            :disabled="scanningAudio || audioBusy || !runtime?.platform.windowsApiAvailable"
            @click="scanAudio"
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
        <ul v-if="audioEndpoints.length" class="device-list endpoint-list">
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

        <div class="setting-list compact">
          <div class="setting-row">
            <strong>ATVV / 16 kHz</strong>
            <span>{{ atvvReady ? "BLE 会话已就绪" : "等待能力确认" }}</span>
          </div>
          <div class="setting-row">
            <strong>WASAPI 端点</strong>
            <span>{{ wasapiReady ? audioPhaseLabel(audio.phase) : "等待明确选择" }}</span>
          </div>
          <div class="setting-row"><strong>本次会话代次</strong><span>{{ connection.generation }}</span></div>
          <div class="setting-row">
            <strong>睡眠恢复通知</strong>
            <span>{{ connection.powerNotificationsAvailable ? "Windows API 已注册" : "当前不可用" }}</span>
          </div>
        </div>
        <div class="info-callout" :class="{ warning: !wasapiReady }">
          {{
            wasapiReady
              ? "所选端点已通过 WASAPI 初始化；仍需在 Windows 上完成真实 RC003 与 VB-CABLE 回环验收。"
              : "请明确选择输出端点。无线麦不会静默安装 VB-CABLE，也不会修改系统默认输入或输出。"
          }}
        </div>
      </article>
    </div>
  </section>
</template>
