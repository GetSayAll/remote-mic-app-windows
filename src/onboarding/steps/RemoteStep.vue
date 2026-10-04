<script setup lang="ts">
import { computed } from "vue";
import {
  connectionPhaseLabel,
  remoteModelLabel,
  type PairedRemote,
  type RuntimeSnapshot,
} from "../../lib/bridge";
import OptionCard from "../../components/onboarding/OptionCard.vue";
import StatusRow from "../../components/onboarding/StatusRow.vue";

const props = defineProps<{
  runtime: RuntimeSnapshot | null;
  devices: PairedRemote[];
  scanning: boolean;
  scanMessage: string;
  connectingDeviceId: string;
  operationMessage: string;
  buttonObserved: boolean;
  voiceKeyMistake: boolean;
  /** 当前门禁失败码（只用于显示修复卡；判据仍由 flow.evaluateGate 独裁）。 */
  gateCode: string | null;
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

/** 扫描状态行：只在"扫描过 / 正在扫描"时出现，不新增空状态文案。 */
const scanRow = computed(() => {
  if (props.scanning) {
    return {
      state: "busy" as const,
      title: "正在寻找小米遥控器…",
      desc: "保持遥控器在旁边；已配对的设备会出现在下方。",
    };
  }
  if (props.devices.length > 0) {
    return {
      state: "ok" as const,
      title: `找到 ${props.devices.length} 个已配对的小米遥控器`,
      desc: "",
    };
  }
  if (props.scanMessage) {
    return { state: "warn" as const, title: props.scanMessage, desc: "" };
  }
  return null;
});

/** 修复卡（对应失败矩阵的主要修复动作）；文案为短路标，判据与恢复动作不变。 */
const FIX_TITLES: Record<string, string> = {
  "bluetooth.unavailable": "蓝牙当前不可用",
  "remote.pairing_required": "还没有已配对的遥控器",
  "remote.not_found": "没有找到遥控器",
  "remote.connect_failed": "连接没有成功",
};

const fixTitle = computed(() =>
  props.gateCode ? (FIX_TITLES[props.gateCode] ?? null) : null,
);
const fixPrefersSettings = computed(
  () =>
    props.gateCode === "bluetooth.unavailable" ||
    props.gateCode === "remote.pairing_required",
);
const fixShowsSettings = computed(() => props.gateCode !== "remote.connect_failed");
</script>

<template>
  <section class="onboarding-step">
    <h1>连接遥控器</h1>
    <p class="onboarding-lede">
      无线麦只支持小米蓝牙语音遥控器 2 和小米蓝牙语音遥控器 2 Pro（RC001 / RC003）。
    </p>

    <div v-if="!connected" class="onboarding-card accent">
      <h4>首次连接遥控器</h4>
      <ul class="onboarding-numlist">
        <li><span class="onboarding-num">1</span><span>把遥控器放在电脑旁边。</span></li>
        <li>
          <span class="onboarding-num">2</span>
          <span>同时长按「菜单」和「主页」键，直到它进入配对模式。</span>
        </li>
        <li>
          <span class="onboarding-num">3</span>
          <span>在 Windows 蓝牙设置里选择「小米蓝牙语音遥控器」，等待配对完成。</span>
        </li>
      </ul>
      <div class="onboarding-chips">
        <button
          class="onboarding-chip strong"
          type="button"
          :disabled="scanning"
          @click="$emit('scan')"
        >
          {{ scanning ? "正在扫描…" : "扫描已配对设备" }}
        </button>
        <button class="onboarding-chip" type="button" @click="$emit('openBluetoothSettings')">
          打开蓝牙设置
        </button>
      </div>
    </div>

    <div class="onboarding-rows">
      <StatusRow
        v-if="scanRow"
        :state="scanRow.state"
        :title="scanRow.title"
        :desc="scanRow.desc || undefined"
      />
      <OptionCard
        v-for="device in devices"
        :key="device.id"
        indicator="none"
        :disabled="connectingDeviceId === device.id"
        @select="$emit('connect', device)"
      >
        <strong>{{ device.name }}</strong>
        <small>已配对的小米蓝牙语音遥控器</small>
        <template #trailing>
          {{ connectingDeviceId === device.id ? "正在连接…" : "连接" }}
        </template>
      </OptionCard>

      <StatusRow
        :state="buttonObserved ? 'ok' : 'pending'"
        :title="buttonObserved ? '已收到遥控器按键' : '等待控制按键'"
        :desc="
          buttonObserved
            ? '这一项可以继续了。'
            : '按一下遥控器上的普通按键（比如主页键），不要按语音键。'
        "
      />
    </div>

    <p v-if="connected" class="onboarding-muted">
      {{ modelLabel }} · {{ phaseLabel }}
    </p>
    <p v-if="reconnecting" class="onboarding-muted">
      正在自动恢复连接（第 {{ reconnectAttempt }} 次尝试），最多等约 60 秒，不需要重新配对。
    </p>

    <div v-if="voiceKeyMistake && !buttonObserved" class="onboarding-card warn">
      <h4>刚才按的是语音键</h4>
      <p>请按遥控器上除语音键以外的普通按键（比如主页键）。</p>
    </div>

    <div v-if="fixTitle" class="onboarding-card warn">
      <h4>{{ fixTitle }}</h4>
      <div class="onboarding-chips">
        <button
          v-if="fixShowsSettings"
          class="onboarding-chip"
          :class="{ strong: fixPrefersSettings }"
          type="button"
          @click="$emit('openBluetoothSettings')"
        >
          打开蓝牙设置
        </button>
        <button
          class="onboarding-chip"
          :class="{ strong: !fixPrefersSettings }"
          type="button"
          :disabled="scanning"
          @click="$emit('scan')"
        >
          {{ scanning ? "正在扫描…" : "重新扫描" }}
        </button>
      </div>
    </div>

    <p v-if="operationMessage" class="onboarding-muted">{{ operationMessage }}</p>
  </section>
</template>
