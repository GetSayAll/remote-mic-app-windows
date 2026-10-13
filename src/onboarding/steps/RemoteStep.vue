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
  /** 本向导会话里连接成功的那台设备 id（启动恢复的连接拿不到，按目标状态即可）。 */
  connectedDeviceId: string;
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

/**
 * 连接后显示的名称（2026-10-13 现场：必须能分清 2 / 2 Pro）。
 * 优先用扫描列表里那台的系统名称（与设备行完全一致）；列表里没有（如启动恢复的连接）
 * 就用运行时上报的蓝牙名称；两者都拿不到才退回型号标签。
 */
const connectedName = computed(() => {
  const known = props.devices.find((device) => device.id === props.connectedDeviceId);
  return known?.name ?? props.runtime?.platform.connection.remoteName ?? modelLabel.value;
});

/**
 * 设备行是否就是当前已连接的那台（2026-10-13 现场要求：连接后给"已连接"状态）。
 * 用本向导会话记录到的设备 id：自动连接与手动连接都走 connectDevice，都会记录；
 * 启动恢复的连接页面拿不到 id，但那种情况下列表还没扫过、没有行可标记。
 */
function isConnectedDevice(device: PairedRemote): boolean {
  return connected.value && props.connectedDeviceId === device.id;
}

/**
 * 按键行的说明句（2026-10-10 现场反馈）：系统侧按键通道可用 ≠ 应用自己的会话
 * 已建立——遥控器与电脑配对完成后，按键就能被收到（第②步的按键行随之变绿），
 * 但"继续"还要求应用连上遥控器。未连接时不再写"这一项可以继续了"（会与"先连接
 * 遥控器，再继续"打架），改成指出还差的那一步。
 * 2026-10-13 现场：连接就绪、按键也收到时**整行不再写任何说明句**——行本身已是
 * 就绪状态、页脚门禁也放行，再写"这一项可以继续了。"是重复。
 */
const buttonRowDesc = computed(() => {
  if (!props.buttonObserved) {
    return "按一下遥控器上的普通按键（比如主页键），不要按语音键。";
  }
  if (connected.value) return "";
  // 说明句跟着用户当下能看到的下一步走：还没扫到设备 → 先扫；
  // 设备已列出（候选不唯一 / 自动连接没触发）→ 直接点设备行右侧的按钮。
  return props.devices.length === 0
    ? "按键已收到。还差一步：点“扫描已配对设备”选中遥控器连接。"
    : "按键已收到。还差一步：点遥控器一行右侧的“连接”。";
});

/** 扫描状态行：只在"扫描过 / 正在扫描"时出现，不新增空状态文案。 */
const scanRow = computed(() => {
  if (props.scanning) {
    // 2026-10-13 现场：去掉"保持遥控器在旁边…"那句（点击后不再出提示词）。
    return {
      state: "busy" as const,
      title: "正在寻找小米遥控器…",
      desc: "",
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

    <!-- 「扫描已配对设备」保持常显（2026-10-13 现场），下方留出间距；
         「打开蓝牙设置」入口已移到第①步的配对流程（失败修复卡里仍保留一份，见文件底部）。 -->
    <div class="onboarding-chips onboarding-chips-scan">
      <button
        class="onboarding-chip strong"
        type="button"
        :disabled="scanning"
        @click="$emit('scan')"
      >
        {{ scanning ? "正在扫描…" : "扫描已配对设备" }}
      </button>
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
        :selected="isConnectedDevice(device)"
        :disabled="connectingDeviceId === device.id || isConnectedDevice(device)"
        @select="$emit('connect', device)"
      >
        <strong>{{ device.name }}</strong>
        <small>已配对的小米蓝牙语音遥控器</small>
        <template #trailing>
          <span
            class="option-action"
            :class="{
              connecting: connectingDeviceId === device.id,
              connected: isConnectedDevice(device),
            }"
          >
            {{
              connectingDeviceId === device.id
                ? "正在连接…"
                : isConnectedDevice(device)
                  ? "已连接"
                  : "连接"
            }}
          </span>
        </template>
      </OptionCard>

      <StatusRow
        :state="buttonObserved ? 'ok' : 'pending'"
        :title="buttonObserved ? '已收到遥控器按键' : '等待控制按键'"
        :desc="buttonRowDesc"
      />
    </div>

    <p v-if="connected" class="onboarding-muted">{{ connectedName }} · {{ phaseLabel }}</p>
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
