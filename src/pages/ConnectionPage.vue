<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import type {
  ConnectionSnapshot,
  PairedRemote,
  RuntimeSnapshot,
} from "../lib/bridge";
import {
  connectRemote,
  connectionPhaseLabel,
  disconnectRemote,
  getConnectionSnapshot,
  scanPairedRemotes,
} from "../lib/bridge";

const props = defineProps<{ runtime: RuntimeSnapshot | null }>();

const emptyConnection = (): ConnectionSnapshot => ({
  phase: "idle",
  remoteName: null,
  capabilities: null,
  voiceState: "idle",
  decodedSamples: 0,
  generation: 0,
  lastError: null,
});

const connection = ref<ConnectionSnapshot>(emptyConnection());
const scanning = ref(false);
const connectingDeviceId = ref("");
const disconnecting = ref(false);
const devices = ref<PairedRemote[]>([]);
const scanMessage = ref("尚未扫描");
const operationMessage = ref("");
let pollTimer: ReturnType<typeof setInterval> | undefined;

watch(
  () => props.runtime?.platform.connection,
  (snapshot) => {
    if (snapshot) connection.value = snapshot;
  },
  { immediate: true },
);

const connectionActive = computed(() =>
  ["connecting", "discovering", "awaiting_capabilities", "ready", "streaming", "draining"].includes(
    connection.value.phase,
  ),
);

const atvvReady = computed(() =>
  ["ready", "streaming", "draining"].includes(connection.value.phase),
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

async function refreshConnection() {
  try {
    connection.value = await getConnectionSnapshot();
  } catch (error) {
    operationMessage.value = error instanceof Error ? error.message : String(error);
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
    operationMessage.value = "RC003 连接已释放";
  } catch (error) {
    operationMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    disconnecting.value = false;
  }
}

onMounted(() => {
  void refreshConnection();
  pollTimer = setInterval(() => void refreshConnection(), 1_000);
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
        <h2>语音输出</h2>
        <div class="setting-list compact">
          <div class="setting-row">
            <strong>ATVV / 16 kHz</strong>
            <span>{{ atvvReady ? "BLE 会话已就绪" : "等待能力确认" }}</span>
          </div>
          <div class="setting-row"><strong>WASAPI 端点</strong><span>尚未实现</span></div>
          <div class="setting-row"><strong>本次会话代次</strong><span>{{ connection.generation }}</span></div>
        </div>
        <div class="info-callout warning">
          当前阶段只验证 RC003 到 PCM 的链路，尚未把音频写入 Windows 音频端点，因此不能作为系统麦克风使用。
        </div>
      </article>
    </div>
  </section>
</template>
