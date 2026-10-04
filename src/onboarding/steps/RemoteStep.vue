<script setup lang="ts">
import { computed } from "vue";
import {
  connectionPhaseLabel,
  remoteModelLabel,
  type PairedRemote,
  type RuntimeSnapshot,
} from "../../lib/bridge";

const props = defineProps<{
  runtime: RuntimeSnapshot | null;
  devices: PairedRemote[];
  scanning: boolean;
  scanMessage: string;
  connectingDeviceId: string;
  operationMessage: string;
  buttonObserved: boolean;
  voiceKeyMistake: boolean;
}>();

defineEmits<{
  scan: [];
  connect: [device: PairedRemote];
  openBluetoothSettings: [];
}>();

const CONNECTED_PHASES = ["ready", "streaming", "draining"] as const;

const connectionPhase = computed(() => props.runtime?.platform.connection.phase ?? "idle");
const connected = computed(() =>
  (CONNECTED_PHASES as readonly string[]).includes(connectionPhase.value),
);
const reconnecting = computed(() => connectionPhase.value === "reconnecting");
const reconnectAttempt = computed(() => props.runtime?.platform.connection.reconnectAttempt ?? 0);
const phaseLabel = computed(() => connectionPhaseLabel(connectionPhase.value));
const modelLabel = computed(() =>
  remoteModelLabel(props.runtime?.platform.connection.remoteModel ?? "unknown"),
);
</script>

<template>
  <section class="onboarding-step">
    <h1>连接小米蓝牙语音遥控器</h1>
    <p class="onboarding-lede">
      无线麦只支持小米蓝牙语音遥控器 2 和小米蓝牙语音遥控器 2 Pro（RC001 / RC003）。
    </p>

    <div v-if="!connected" class="onboarding-callout">
      <p>还没有连接遥控器。先在 Windows 里完成配对，再回到这里扫描：</p>
      <ol>
        <li>把遥控器放在电脑旁边。</li>
        <li>同时长按遥控器上的「菜单」和「主页」键，直到它进入配对模式。</li>
        <li>在 Windows 蓝牙设置里选择「小米蓝牙语音遥控器」，等待配对完成。</li>
      </ol>
      <div class="onboarding-actions">
        <button class="secondary-button" type="button" @click="$emit('openBluetoothSettings')">
          打开蓝牙设置
        </button>
        <button
          class="primary-button"
          type="button"
          :disabled="scanning"
          @click="$emit('scan')"
        >
          {{ scanning ? "正在扫描…" : "扫描已配对设备" }}
        </button>
      </div>
      <p v-if="scanMessage" class="onboarding-message">{{ scanMessage }}</p>
      <ul v-if="devices.length" class="onboarding-device-list">
        <li v-for="device in devices" :key="device.id">
          <span>{{ device.name }}</span>
          <button
            class="secondary-button"
            type="button"
            :disabled="connectingDeviceId === device.id"
            @click="$emit('connect', device)"
          >
            {{ connectingDeviceId === device.id ? "正在连接…" : "连接" }}
          </button>
        </li>
      </ul>
      <p v-if="operationMessage" class="onboarding-message">{{ operationMessage }}</p>
    </div>

    <div v-else class="onboarding-callout ok">
      <p class="onboarding-status-line">{{ modelLabel }} · {{ phaseLabel }}</p>
      <p v-if="!buttonObserved">再按一下遥控器上的普通按键（比如主页键），确认按键能用。</p>
      <p v-else class="onboarding-ok">已收到遥控器按键，这一项可以继续了。</p>
    </div>

    <div v-if="reconnecting" class="onboarding-callout warn">
      正在自动恢复连接（第 {{ reconnectAttempt }} 次尝试），最多等约 60 秒，不需要重新配对。
    </div>

    <div v-if="voiceKeyMistake && !buttonObserved" class="onboarding-callout warn">
      刚才按的是语音键。请按遥控器上除语音键以外的普通按键（比如主页键）。
    </div>
  </section>
</template>
