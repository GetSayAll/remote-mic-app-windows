<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import {
  getRawInputSnapshot,
  startRawInput,
  stopRawInput,
  type RawInputSnapshot,
  type RemoteButton,
} from "../lib/bridge";

const mappings = [
  ["方向键", "Raw Input 已建立，等待 RC003 真机确认"],
  ["OK / 首页 / 菜单", "语义边沿已建立，尚未配置动作"],
  ["电视 / 电源", "实验能力，不进入首版承诺"],
  ["返回 / 音量", "Windows 公共 API 可用性待验证"],
];

const buttonLabels: Record<RemoteButton, string> = {
  back: "返回",
  ok: "OK",
  tv: "电视",
  home: "首页",
  right: "右",
  left: "左",
  down: "下",
  up: "上",
  menu: "菜单",
  power: "电源",
  volume_mute: "静音",
  volume_up: "音量加",
  volume_down: "音量减",
};

const snapshot = ref<RawInputSnapshot | null>(null);
const errorMessage = ref<string | null>(null);
const busy = ref(false);
let refreshTimer: number | null = null;

const phaseLabel = computed(() => {
  switch (snapshot.value?.phase) {
    case "ready":
      return "Raw Input 已监听";
    case "starting":
      return "正在启动监听";
    case "failed":
      return "Raw Input 启动失败";
    case "stopped":
      return "Raw Input 未启动";
    default:
      return "当前环境不可用";
  }
});

const lastEdgeLabel = computed(() => {
  const current = snapshot.value;
  if (!current?.lastButton || current.lastIsPressed === null) {
    return "尚未收到普通按键边沿";
  }
  return `${buttonLabels[current.lastButton]} · ${current.lastIsPressed ? "按下" : "释放"}`;
});

async function refresh(): Promise<void> {
  snapshot.value = await getRawInputSnapshot();
}

async function startListener(): Promise<void> {
  busy.value = true;
  errorMessage.value = null;
  try {
    snapshot.value = await startRawInput();
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : String(error);
    await refresh();
  } finally {
    busy.value = false;
  }
}

async function stopListener(): Promise<void> {
  busy.value = true;
  errorMessage.value = null;
  try {
    snapshot.value = await stopRawInput();
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : String(error);
    await refresh();
  } finally {
    busy.value = false;
  }
}

onMounted(async () => {
  await refresh();
  refreshTimer = window.setInterval(() => {
    if (snapshot.value?.phase === "ready") {
      void refresh();
    }
  }, 500);
});

onUnmounted(() => {
  if (refreshTimer !== null) {
    window.clearInterval(refreshTimer);
  }
});
</script>

<template>
  <section>
    <header class="page-header">
      <div>
        <h1>按键</h1>
        <p>按键映射将以真实 Windows Raw Input 报告为准，不继承未经复验的 13 键结论。</p>
      </div>
      <span class="badge" :class="snapshot?.phase === 'ready' ? 'success' : 'pending'">
        {{ phaseLabel }}
      </span>
    </header>

    <div class="two-column remote-layout">
      <article class="card remote-card">
        <div class="remote-photo-wrap">
          <img src="/RC003-remote-photo.png" alt="小米蓝牙遥控器 2 Pro" />
        </div>
        <div>
          <h2>小米蓝牙遥控器 2 Pro</h2>
          <p class="muted">型号 RC003 · 语音键始终是按下开始、释放结束</p>
        </div>
      </article>

      <article class="card">
        <div class="card-title-row">
          <h2>Raw Input 状态</h2>
          <div class="button-row">
            <button
              class="secondary-button"
              type="button"
              :disabled="busy || snapshot?.phase === 'unsupported'"
              @click="refresh"
            >
              刷新
            </button>
            <button
              v-if="snapshot?.phase !== 'ready'"
              class="primary-button"
              type="button"
              :disabled="busy || snapshot?.phase === 'unsupported' || snapshot?.phase === 'starting'"
              @click="startListener"
            >
              启动监听
            </button>
            <button
              v-else
              class="secondary-button"
              type="button"
              :disabled="busy"
              @click="stopListener"
            >
              停止监听
            </button>
          </div>
        </div>
        <div class="setting-list">
          <div class="setting-row">
            <strong>匹配设备路径</strong>
            <span>{{ snapshot?.matchedDeviceCount ?? 0 }} 个</span>
          </div>
          <div class="setting-row">
            <strong>最近按键</strong>
            <span>{{ lastEdgeLabel }}</span>
          </div>
          <div class="setting-row">
            <strong>原始事件 / 语义边沿</strong>
            <span>{{ snapshot?.rawEventCount ?? 0 }} / {{ snapshot?.semanticEdgeCount ?? 0 }}</span>
          </div>
        </div>
        <p v-if="errorMessage || snapshot?.lastError" class="error-text">
          {{ errorMessage || snapshot?.lastError }}
        </p>

        <div class="card-title-row mapping-heading">
          <h2>映射状态</h2>
          <button class="secondary-button" type="button" disabled>编辑映射</button>
        </div>
        <div class="setting-list">
          <div v-for="mapping in mappings" :key="mapping[0]" class="setting-row">
            <strong>{{ mapping[0] }}</strong>
            <span>{{ mapping[1] }}</span>
          </div>
        </div>
        <div class="info-callout">
          语音键不会加入双击等待或长按阈值，避免增加首个响应延迟。
        </div>
      </article>
    </div>
  </section>
</template>
