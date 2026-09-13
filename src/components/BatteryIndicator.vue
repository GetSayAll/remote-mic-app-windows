<script setup lang="ts">
import { computed } from "vue";
import type { ConnectionSnapshot } from "../lib/bridge";

const props = defineProps<{ connection?: ConnectionSnapshot | null }>();
const connected = computed(() => ["awaiting_capabilities", "ready", "streaming", "draining"]
  .includes(props.connection?.phase ?? ""));
const level = computed(() => {
  const value = props.connection?.batteryLevel;
  return connected.value && typeof value === "number" && Number.isInteger(value)
    && value >= 0 && value <= 100 ? value : null;
});
const label = computed(() => level.value === null ? "电量未知" : `${level.value}%`);
const title = computed(() => !connected.value ? "遥控器未连接"
  : level.value === null ? "Windows 暂未提供遥控器电量"
  : `遥控器电量 ${level.value}%（Windows 缓存，随设备上报更新）`);
</script>

<template>
  <span class="battery-indicator" :class="{ low: level !== null && level <= 20 }"
    :title="title" :aria-label="title" role="status">
    <svg viewBox="0 0 26 16" width="26" height="16" aria-hidden="true">
      <rect x="1" y="2" width="21" height="12" rx="2" fill="none" stroke="currentColor" stroke-width="1.5" />
      <path d="M24 6v4" stroke="currentColor" stroke-width="2" stroke-linecap="round" />
      <rect v-if="level !== null && level > 0" x="3.5" y="4.5" :width="16 * level / 100" height="7" rx="0.5" fill="currentColor" />
      <path v-if="level === null" d="M8 8h7" stroke="currentColor" stroke-width="1.5" />
    </svg>
    <span>{{ label }}</span>
  </span>
</template>

<style scoped>
.battery-indicator { display: inline-flex; align-items: center; gap: 5px; min-width: 72px; flex-shrink: 0; color: var(--text-secondary); font-size: 12px; line-height: 20px; white-space: nowrap; font-variant-numeric: tabular-nums; }
.battery-indicator svg { flex-shrink: 0; }
.battery-indicator.low { color: var(--warning-text); }
</style>
