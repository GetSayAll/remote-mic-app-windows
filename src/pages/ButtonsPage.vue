<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import {
  getButtonMappings,
  getRawInputSnapshot,
  getSendInputSnapshot,
  saveButtonMappings,
  startRawInput,
  stopRawInput,
  testButtonMapping,
  type ButtonAction,
  type ButtonMappings,
  type KeyCode,
  type RawInputSnapshot,
  type RemoteButton,
  type SendInputSnapshot,
} from "../lib/bridge";

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

const buttons = Object.keys(buttonLabels) as RemoteButton[];
const presets: Array<{ label: string; keys: KeyCode[] }> = [
  { label: "关闭", keys: [] },
  { label: "Enter", keys: ["enter"] },
  { label: "Esc", keys: ["escape"] },
  { label: "↑", keys: ["up"] },
  { label: "↓", keys: ["down"] },
  { label: "←", keys: ["left"] },
  { label: "→", keys: ["right"] },
  { label: "Home", keys: ["home"] },
  { label: "菜单", keys: ["apps"] },
  { label: "Ctrl+C", keys: ["control", "c"] },
  { label: "Ctrl+V", keys: ["control", "v"] },
  { label: "Win+D", keys: ["left_windows", "d"] },
];

const snapshot = ref<RawInputSnapshot | null>(null);
const errorMessage = ref<string | null>(null);
const busy = ref(false);
const mappings = ref<ButtonMappings>({ actions: {} });
const selectedButton = ref<RemoteButton>("up");
const mappingMessage = ref<string | null>(null);
const sendInput = ref<SendInputSnapshot | null>(null);
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

function selectedAction(): ButtonAction {
  return mappings.value.actions[selectedButton.value] ?? { type: "disabled" };
}

function applyPreset(keys: KeyCode[]): void {
  mappings.value.actions[selectedButton.value] = keys.length
    ? { type: "shortcut", chord: { keys: [...keys] } }
    : { type: "disabled" };
  mappingMessage.value = "映射已修改，点击保存后生效";
}

function actionLabel(button: RemoteButton): string {
  const action = mappings.value.actions[button];
  if (!action || action.type === "disabled") return "关闭";
  return action.chord.keys.join(" + ");
}

async function saveMappings(): Promise<void> {
  busy.value = true;
  mappingMessage.value = null;
  try {
    mappings.value = await saveButtonMappings(mappings.value);
    mappingMessage.value = "按键映射已保存并热加载";
  } catch (error) {
    mappingMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    busy.value = false;
  }
}

async function testMapping(): Promise<void> {
  busy.value = true;
  mappingMessage.value = null;
  try {
    sendInput.value = await testButtonMapping(selectedButton.value);
    mappingMessage.value = "测试快捷键已通过一次批量 SendInput 提交";
  } catch (error) {
    mappingMessage.value = error instanceof Error ? error.message : String(error);
    sendInput.value = await getSendInputSnapshot();
  } finally {
    busy.value = false;
  }
}

onMounted(async () => {
  const [loadedMappings, loadedSendInput] = await Promise.all([
    getButtonMappings(),
    getSendInputSnapshot(),
    refresh(),
  ]);
  mappings.value = loadedMappings;
  sendInput.value = loadedSendInput;
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
          <button class="primary-button" type="button" :disabled="busy" @click="saveMappings">
            保存映射
          </button>
        </div>
        <div class="mapping-button-grid">
          <button
            v-for="button in buttons"
            :key="button"
            type="button"
            :class="{ selected: selectedButton === button }"
            @click="selectedButton = button"
          >
            <strong>{{ buttonLabels[button] }}</strong>
            <span>{{ actionLabel(button) }}</span>
          </button>
        </div>
        <h3 class="mapping-subtitle">{{ buttonLabels[selectedButton] }} 的快捷键</h3>
        <div class="preset-grid">
          <button
            v-for="preset in presets"
            :key="preset.label"
            class="secondary-button"
            type="button"
            :disabled="busy"
            @click="applyPreset(preset.keys)"
          >
            {{ preset.label }}
          </button>
        </div>
        <div class="button-row mapping-actions">
          <button
            class="secondary-button"
            type="button"
            :disabled="busy || selectedAction().type === 'disabled' || !sendInput?.available"
            @click="testMapping"
          >
            测试当前快捷键
          </button>
        </div>
        <p v-if="mappingMessage" class="operation-message">{{ mappingMessage }}</p>
        <p class="muted mapping-note">
          当前只保存并显式测试快捷键。真实 RC003 尚未确认原始 Keyboard/HID 路径前，不会自动注入，避免方向键等被执行两次。
        </p>
        <div class="info-callout">
          语音键不会加入双击等待或长按阈值，避免增加首个响应延迟。
        </div>
      </article>
    </div>
  </section>
</template>
