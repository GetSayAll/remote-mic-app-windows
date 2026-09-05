<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import {
  actionSummary,
  buttonLabel,
  buttonLabels,
  buttonTriggerLabel,
  chordLabel,
  getButtonMappingSnapshot,
  getButtonMappings,
  resetButtonMappings,
  saveButtonMappings,
  startRawInput,
  stopRawInput,
  subscribeButtonEdges,
  subscribeButtonGestures,
  testButtonMapping,
  type ButtonAction,
  type ButtonActions,
  type ButtonEdge,
  type ButtonMappingSnapshot,
  type ButtonMappings,
  type ButtonTrigger,
  type FiredGesture,
  type KeyCode,
  type RawInputPhase,
  type RemoteButton,
  type RuntimeSnapshot,
} from "../lib/bridge";

const props = defineProps<{ runtime: RuntimeSnapshot | null }>();

/** 画布几何：与 Mac 原版 RemoteMappingCanvas 对齐（等比设计稿，组件内固定坐标系）。 */
const CANVAS_WIDTH = 780;
const CANVAS_HEIGHT = 570;
const REMOTE_WIDTH = 202;
const REMOTE_HEIGHT = 410;
const CARD_WIDTH = 262;
const CARD_HEIGHT = 64;
const REMOTE_LEFT = (CANVAS_WIDTH - REMOTE_WIDTH) / 2;
const REMOTE_TOP = (CANVAS_HEIGHT - REMOTE_HEIGHT) / 2;

interface Placement {
  button: RemoteButton;
  side: "left" | "right";
  anchor: [number, number];
  targetY: number;
}

/** 按键卡片布局表：对齐 Mac RemoteMappingLayout.buttonPlacements。 */
const PLACEMENTS: Placement[] = [
  { button: "power", side: "left", anchor: [0.386, 0.099], targetY: 0.08 },
  { button: "up", side: "left", anchor: [0.502, 0.179], targetY: 0.23 },
  { button: "left", side: "left", anchor: [0.362, 0.246], targetY: 0.38 },
  { button: "back", side: "left", anchor: [0.406, 0.389], targetY: 0.53 },
  { button: "home", side: "left", anchor: [0.406, 0.479], targetY: 0.68 },
  { button: "menu", side: "left", anchor: [0.406, 0.569], targetY: 0.83 },
  { button: "right", side: "right", anchor: [0.638, 0.246], targetY: 0.215 },
  { button: "ok", side: "right", anchor: [0.502, 0.246], targetY: 0.36 },
  { button: "down", side: "right", anchor: [0.502, 0.317], targetY: 0.505 },
  { button: "volume_up", side: "right", anchor: [0.604, 0.39], targetY: 0.65 },
  { button: "volume_down", side: "right", anchor: [0.604, 0.48], targetY: 0.795 },
  { button: "tv", side: "right", anchor: [0.604, 0.569], targetY: 0.94 },
];
const VOICE_PLACEMENT: Placement = {
  button: "ok", // 语音卡不对应 RemoteButton；占位仅用于定位。
  side: "right",
  anchor: [0.63, 0.099],
  targetY: 0.07,
};
const TRIGGERS: ButtonTrigger[] = ["single", "double", "long"];

function anchorPoint(placement: Placement): { x: number; y: number } {
  return {
    x: REMOTE_LEFT + REMOTE_WIDTH * placement.anchor[0],
    y: REMOTE_TOP + REMOTE_HEIGHT * placement.anchor[1],
  };
}

/** 照片容器内相对坐标（锚点橙点渲染在 .remote-photo 内部，坐标系是照片自身）。 */
function photoAnchorPoint(placement: Placement): { x: number; y: number } {
  return {
    x: REMOTE_WIDTH * placement.anchor[0],
    y: REMOTE_HEIGHT * placement.anchor[1],
  };
}

function cardTop(placement: Placement): number {
  return placement.targetY * CANVAS_HEIGHT - CARD_HEIGHT / 2;
}

/** 卡片朝向遥控器一侧的边缘中点（连线终点）。 */
function cardEdgePoint(placement: Placement): { x: number; y: number } {
  return {
    x: placement.side === "left" ? CARD_WIDTH : CANVAS_WIDTH - CARD_WIDTH,
    y: placement.targetY * CANVAS_HEIGHT,
  };
}

function connectionPath(placement: Placement): string {
  const start = anchorPoint(placement);
  const end = cardEdgePoint(placement);
  const direction = placement.side === "left" ? -1 : 1;
  const distance = Math.min(70, Math.max(34, Math.abs(end.x - start.x) * 0.58));
  const endpointDistance = Math.min(42, Math.max(24, distance * 0.6));
  const control1 = { x: start.x + direction * distance, y: start.y };
  const control2 = { x: end.x - direction * endpointDistance, y: end.y };
  return `M ${start.x.toFixed(1)} ${start.y.toFixed(1)} C ${control1.x.toFixed(1)} ${control1.y.toFixed(1)}, ${control2.x.toFixed(1)} ${control2.y.toFixed(1)}, ${end.x.toFixed(1)} ${end.y.toFixed(1)}`;
}

const buttonIcons: Record<RemoteButton, string> = {
  power: "⏻",
  up: "▲",
  down: "▼",
  left: "◀",
  right: "▶",
  ok: "◎",
  back: "↩",
  home: "⌂",
  menu: "☰",
  tv: "▭",
  volume_up: "＋",
  volume_down: "－",
  volume_mute: "∅",
};

const mappings = ref<ButtonMappings>({ enabled: true, actions: {} });
const savedSnapshot = ref<ButtonMappings>({ enabled: true, actions: {} });
const selectedButton = ref<RemoteButton | null>(null);
const editingTarget = ref<{ button: RemoteButton; trigger: ButtonTrigger } | null>(null);
const editorPanel = ref<HTMLElement | null>(null);
const lockSelection = ref(true);
const activeButtons = ref<Set<RemoteButton>>(new Set());
const lastFired = ref<FiredGesture | null>(null);
const firedFlash = ref<{ button: RemoteButton; trigger: ButtonTrigger } | null>(null);
const mappingSnapshot = ref<ButtonMappingSnapshot | null>(null);
const busy = ref(false);
const statusMessage = ref<string | null>(null);
const capturingShortcut = ref(false);
const captureDisplay = ref<string[]>([]);
let unlistenEdges: (() => void) | null = null;
let unlistenGestures: (() => void) | null = null;
let snapshotTimer: number | null = null;
let flashTimer: number | null = null;

const dirty = computed(
  () => JSON.stringify(mappings.value) !== JSON.stringify(savedSnapshot.value),
);

const enabled = computed({
  get: () => mappings.value.enabled,
  set: (value: boolean) => {
    mappings.value = { ...mappings.value, enabled: value };
    void persist("总开关已更新");
  },
});

const voiceActive = computed(
  () => props.runtime?.platform.connection.voiceState === "streaming",
);

function actionsOf(button: RemoteButton): ButtonActions {
  return (
    mappings.value.actions[button] ?? {
      single: { type: "disabled" },
      double: { type: "disabled" },
      long: { type: "disabled" },
    }
  );
}

function actionOf(button: RemoteButton, trigger: ButtonTrigger): ButtonAction {
  return actionsOf(button)[trigger];
}

function selectButton(button: RemoteButton): void {
  selectedButton.value = button;
}

function openEditor(button: RemoteButton, trigger: ButtonTrigger): void {
  selectedButton.value = button;
  editingTarget.value = { button, trigger };
  capturingShortcut.value = false;
}

function applyAction(action: ButtonAction): void {
  const target = editingTarget.value;
  if (!target) return;
  const next: ButtonMappings = {
    ...mappings.value,
    actions: { ...mappings.value.actions },
  };
  const actions = { ...actionsOf(target.button) };
  actions[target.trigger] = action;
  next.actions[target.button] = actions;
  mappings.value = next;
  statusMessage.value = "映射已修改，点击保存后生效";
}

const PRESET_GROUPS: Array<{ label: string; items: Array<{ label: string; keys: KeyCode[] }> }> = [
  {
    label: "基础按键",
    items: [
      { label: "Enter", keys: ["enter"] },
      { label: "Esc", keys: ["escape"] },
      { label: "空格", keys: ["space"] },
      { label: "Tab", keys: ["tab"] },
      { label: "退格", keys: ["backspace"] },
      { label: "↑", keys: ["up"] },
      { label: "↓", keys: ["down"] },
      { label: "←", keys: ["left"] },
      { label: "→", keys: ["right"] },
      { label: "Home", keys: ["home"] },
      { label: "End", keys: ["end"] },
      { label: "PageUp", keys: ["page_up"] },
      { label: "PageDown", keys: ["page_down"] },
      { label: "Delete", keys: ["delete"] },
      { label: "菜单键", keys: ["apps"] },
    ],
  },
  {
    label: "系统与媒体",
    items: [
      { label: "显示桌面", keys: ["left_windows", "d"] },
      { label: "锁定", keys: ["left_windows", "l"] },
      { label: "复制", keys: ["control", "c"] },
      { label: "粘贴", keys: ["control", "v"] },
      { label: "剪切", keys: ["control", "x"] },
      { label: "撤销", keys: ["control", "z"] },
      { label: "全选", keys: ["control", "a"] },
      { label: "静音", keys: ["volume_mute"] },
      { label: "音量+", keys: ["volume_up"] },
      { label: "音量−", keys: ["volume_down"] },
      { label: "F5", keys: ["f5"] },
    ],
  },
];

function isActivePreset(keys: KeyCode[]): boolean {
  const target = editingTarget.value;
  if (!target) return false;
  const action = actionOf(target.button, target.trigger);
  if (action.type !== "shortcut") return false;
  return action.chord.keys.join("+") === keys.join("+");
}

async function persist(message: string): Promise<void> {
  busy.value = true;
  statusMessage.value = null;
  try {
    const saved = await saveButtonMappings(mappings.value);
    mappings.value = saved;
    savedSnapshot.value = JSON.parse(JSON.stringify(saved)) as ButtonMappings;
    statusMessage.value = message;
  } catch (error) {
    statusMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    busy.value = false;
  }
}

async function restoreDefaults(): Promise<void> {
  busy.value = true;
  statusMessage.value = null;
  try {
    const saved = await resetButtonMappings();
    mappings.value = saved;
    savedSnapshot.value = JSON.parse(JSON.stringify(saved)) as ButtonMappings;
    statusMessage.value = "已恢复默认（全部按键透传原始行为）";
  } catch (error) {
    statusMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    busy.value = false;
  }
}

async function testCurrentAction(): Promise<void> {
  const target = editingTarget.value;
  if (!target) return;
  busy.value = true;
  statusMessage.value = null;
  try {
    await testButtonMapping(target.button, target.trigger);
    statusMessage.value = "已通过 SendInput 提交一次测试";
  } catch (error) {
    statusMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    busy.value = false;
  }
}

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

const heldModifiers = reactive(new Set<KeyCode>());

function handleCaptureKeydown(event: KeyboardEvent): void {
  if (!capturingShortcut.value) return;
  event.preventDefault();
  event.stopPropagation();
  const code = codeToKeyCode(event.code);
  if (code === null) return;
  const isModifier = [
    "left_control",
    "right_control",
    "left_shift",
    "right_shift",
    "left_alt",
    "right_alt",
    "left_windows",
    "right_windows",
  ].includes(code);
  if (isModifier) {
    if (event.repeat) return;
    heldModifiers.add(code);
    captureDisplay.value = [...heldModifiers];
    return;
  }
  if (code === "escape" && heldModifiers.size === 0) {
    capturingShortcut.value = false;
    heldModifiers.clear();
    captureDisplay.value = [];
    statusMessage.value = "已取消录入";
    return;
  }
  const keys = [...heldModifiers, code];
  applyAction({ type: "shortcut", chord: { keys } });
  capturingShortcut.value = false;
  heldModifiers.clear();
  captureDisplay.value = [];
  statusMessage.value = `快捷键已录入：${chordLabel({ keys })}`;
}

function handleCaptureKeyup(event: KeyboardEvent): void {
  if (!capturingShortcut.value) return;
  const code = codeToKeyCode(event.code);
  if (code && heldModifiers.has(code)) {
    heldModifiers.delete(code);
    captureDisplay.value = [...heldModifiers];
  }
}

watch(capturingShortcut, (active) => {
  if (!active) {
    heldModifiers.clear();
    captureDisplay.value = [];
  }
});

// 打开编辑面板后滚动到可见位置（Mac ScrollViewReader 同款行为）。
watch(editingTarget, async (target) => {
  if (!target) return;
  await nextTick();
  editorPanel.value?.scrollIntoView?.({ behavior: "smooth", block: "nearest" });
});

function phaseLabel(phase: RawInputPhase | undefined): string {
  switch (phase) {
    case "ready":
      return "按键监听已就绪";
    case "starting":
      return "正在启动监听";
    case "failed":
      return "监听启动失败（自动重试中）";
    case "stopped":
      return "监听已停止";
    case "unsupported":
      return "当前环境不可用";
    default:
      return "正在读取状态";
  }
}

/** 手动启停监听：自动启动之外保留显式控制（停止后自动重试不生效）。 */
async function toggleListener(): Promise<void> {
  busy.value = true;
  statusMessage.value = null;
  try {
    if (rawInput.value?.phase === "ready") {
      await stopRawInput();
      statusMessage.value = "监听已停止；映射与高亮暂停（按住说话不受影响）";
    } else {
      await startRawInput();
      statusMessage.value = "监听已启动";
    }
  } catch (error) {
    statusMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    busy.value = false;
  }
}

const rawInput = computed(() => props.runtime?.platform.rawInput);
const connectionInfo = computed(() => props.runtime?.platform.connection);

onMounted(async () => {
  window.addEventListener("keydown", handleCaptureKeydown, true);
  window.addEventListener("keyup", handleCaptureKeyup, true);
  const [loaded, snapshot] = await Promise.all([
    getButtonMappings(),
    getButtonMappingSnapshot(),
  ]);
  mappings.value = loaded;
  savedSnapshot.value = JSON.parse(JSON.stringify(loaded)) as ButtonMappings;
  mappingSnapshot.value = snapshot;
  if (rawInput.value?.activeButtons) {
    activeButtons.value = new Set(rawInput.value.activeButtons);
  }

  unlistenEdges = await subscribeButtonEdges((edge: ButtonEdge) => {
    const next = new Set(activeButtons.value);
    if (edge.isPressed) {
      next.add(edge.button);
    } else {
      next.delete(edge.button);
    }
    activeButtons.value = next;
    if (!lockSelection.value && edge.isPressed) {
      selectedButton.value = edge.button;
    }
  });
  unlistenGestures = await subscribeButtonGestures((gesture: FiredGesture) => {
    lastFired.value = gesture;
    firedFlash.value = { button: gesture.button, trigger: gesture.trigger };
    if (flashTimer !== null) window.clearTimeout(flashTimer);
    flashTimer = window.setTimeout(() => {
      firedFlash.value = null;
    }, 600);
  });

  snapshotTimer = window.setInterval(async () => {
    mappingSnapshot.value = await getButtonMappingSnapshot();
    // 按住集合对账：快照是并集真值（覆盖漏事件漂移）。
    if (rawInput.value?.activeButtons) {
      activeButtons.value = new Set(rawInput.value.activeButtons);
    }
  }, 1_000);
});

onUnmounted(() => {
  window.removeEventListener("keydown", handleCaptureKeydown, true);
  window.removeEventListener("keyup", handleCaptureKeyup, true);
  unlistenEdges?.();
  unlistenGestures?.();
  if (snapshotTimer !== null) window.clearInterval(snapshotTimer);
  if (flashTimer !== null) window.clearTimeout(flashTimer);
});
</script>

<template>
  <section class="buttons-page">
    <header class="page-header mapping-header">
      <div>
        <h1>按键映射</h1>
        <p>遥控器除语音键外的 12 个按键可自定义单击、双击与长按动作；语音键保持按住说话，不参与映射。</p>
      </div>
      <div class="mapping-header-controls">
        <div class="device-chip" :class="{ connected: connectionInfo?.phase === 'ready' || connectionInfo?.phase === 'streaming' }">
          <span class="status-dot" :class="connectionInfo?.phase === 'streaming' ? 'active' : connectionInfo?.phase === 'ready' ? 'success' : 'pending'"></span>
          <span>{{ connectionInfo?.remoteName ?? "未连接遥控器" }}</span>
        </div>
        <label class="toggle-row">
          <span>启用自定义按键功能</span>
          <input v-model="enabled" type="checkbox" class="toggle-input" :disabled="busy" />
        </label>
        <button
          class="primary-button"
          type="button"
          :disabled="busy || !dirty"
          @click="persist('按键映射已保存并即时生效')"
        >
          {{ dirty ? "保存映射" : "已保存" }}
        </button>
      </div>
    </header>

    <div class="mapping-canvas" :style="{ width: `${CANVAS_WIDTH}px`, height: `${CANVAS_HEIGHT}px` }">
      <svg class="mapping-connections" :width="CANVAS_WIDTH" :height="CANVAS_HEIGHT" aria-hidden="true">
        <path
          v-for="placement in PLACEMENTS"
          :key="placement.button"
          :d="connectionPath(placement)"
          :class="{ selected: selectedButton === placement.button, active: activeButtons.has(placement.button) }"
          fill="none"
        />
        <path
          :d="connectionPath(VOICE_PLACEMENT)"
          :class="{ active: voiceActive }"
          fill="none"
        />
      </svg>

      <figure class="remote-photo">
        <img src="/RC003-remote-photo.png" alt="小米蓝牙遥控器 2 Pro（RC003）示意图" draggable="false" />
        <span
          v-for="placement in PLACEMENTS"
          :key="placement.button"
          class="anchor-dot"
          :class="{ visible: activeButtons.has(placement.button) }"
          :style="{
            left: `${photoAnchorPoint(placement).x - 5}px`,
            top: `${photoAnchorPoint(placement).y - 5}px`,
          }"
        ></span>
        <span
          class="anchor-dot voice"
          :class="{ visible: voiceActive }"
          :style="{
            left: `${photoAnchorPoint(VOICE_PLACEMENT).x - 5}px`,
            top: `${photoAnchorPoint(VOICE_PLACEMENT).y - 5}px`,
          }"
        ></span>
      </figure>

      <article
        v-for="placement in PLACEMENTS"
        :key="placement.button"
        class="mapping-card"
        :class="{
          left: placement.side === 'left',
          right: placement.side === 'right',
          selected: selectedButton === placement.button,
          active: activeButtons.has(placement.button),
          flashed: firedFlash?.button === placement.button,
        }"
        :style="{ top: `${cardTop(placement)}px` }"
        @click="selectButton(placement.button)"
      >
        <div class="mapping-card-title">
          <span class="mapping-icon" aria-hidden="true">{{ buttonIcons[placement.button] }}</span>
          <strong>{{ buttonLabels[placement.button] }}</strong>
        </div>
        <div class="mapping-cells">
          <button
            v-for="trigger in TRIGGERS"
            :key="trigger"
            type="button"
            class="mapping-cell"
            :class="{
              set: actionOf(placement.button, trigger).type !== 'disabled',
              editing:
                editingTarget?.button === placement.button && editingTarget?.trigger === trigger,
              flashed: firedFlash?.button === placement.button && firedFlash?.trigger === trigger,
            }"
            @click.stop="openEditor(placement.button, trigger)"
          >
            <small>{{ buttonTriggerLabel(trigger) }}</small>
            <span>{{ actionSummary(actionOf(placement.button, trigger)) }}</span>
          </button>
        </div>
      </article>

      <article
        class="mapping-card voice-card right"
        :class="{ active: voiceActive }"
        :style="{ top: `${cardTop(VOICE_PLACEMENT)}px` }"
      >
        <div class="mapping-card-title">
          <span class="mapping-icon" aria-hidden="true">🎤</span>
          <strong>语音键</strong>
          <span class="badge pending voice-badge">按住说话</span>
        </div>
        <p class="voice-note">按下开始、松开结束；不参与自定义映射，不加双击/长按延迟。</p>
      </article>
    </div>

    <article v-if="editingTarget" ref="editorPanel" class="card mapping-editor">
      <div class="card-title-row">
        <div>
          <h2>{{ buttonLabel(editingTarget.button) }} · {{ buttonTriggerLabel(editingTarget.trigger) }}</h2>
          <p class="muted">
            当前：{{ actionSummary(actionOf(editingTarget.button, editingTarget.trigger)) }} ·
            点击选择新动作，保存后立即生效
          </p>
        </div>
        <div class="button-row">
          <button class="secondary-button" type="button" :disabled="busy" @click="testCurrentAction">
            测试一次
          </button>
          <button class="secondary-button" type="button" @click="editingTarget = null">关闭</button>
        </div>
      </div>
      <div class="preset-groups">
        <button
          class="chip"
          :class="{ selected: actionOf(editingTarget.button, editingTarget.trigger).type === 'disabled' }"
          type="button"
          @click="applyAction({ type: 'disabled' })"
        >
          关闭（透传原始按键）
        </button>
        <button
          v-for="group in PRESET_GROUPS"
          :key="group.label"
          class="chip group-label"
          type="button"
          disabled
        >
          {{ group.label }}
        </button>
      </div>
      <div v-for="group in PRESET_GROUPS" :key="group.label" class="preset-grid">
        <button
          v-for="preset in group.items"
          :key="preset.label"
          class="chip"
          :class="{ selected: isActivePreset(preset.keys) }"
          type="button"
          @click="applyAction({ type: 'shortcut', chord: { keys: [...preset.keys] } })"
        >
          {{ preset.label }}
        </button>
      </div>
      <div class="custom-shortcut-row">
        <button
          class="chip"
          :class="{ selected: capturingShortcut }"
          type="button"
          @click="capturingShortcut = !capturingShortcut"
        >
          {{ capturingShortcut ? "录入中…（按 Esc 取消）" : "录入自定义快捷键" }}
        </button>
        <span v-if="capturingShortcut" class="capture-display">
          {{ captureDisplay.length ? captureDisplay.join(" + ") : "请按下快捷键组合" }}
        </span>
      </div>
      <p v-if="editingTarget.trigger === 'single'" class="muted editor-note">
        未配置双击与长按时，单击在按下瞬间触发（零延迟）；返回/方向/音量键按住会连续触发。
      </p>
      <p v-else class="muted editor-note">
        {{ editingTarget.trigger === "double" ? "双击判定窗口约 0.3 秒：配置后单击会稍等片刻以区分双击。" : "长按约 0.55 秒触发；配置后按住连发停用。" }}
      </p>
    </article>

    <footer class="card mapping-footer">
      <div class="mapping-footer-status">
        <span class="status-dot" :class="rawInput?.phase === 'ready' ? 'success' : 'pending'"></span>
        <span>{{ phaseLabel(rawInput?.phase) }}</span>
        <button
          v-if="rawInput?.phase === 'ready' || rawInput?.phase === 'stopped' || rawInput?.phase === 'failed'"
          class="secondary-button footer-listener-toggle"
          type="button"
          :disabled="busy"
          @click="toggleListener"
        >
          {{ rawInput?.phase === "ready" ? "停止监听" : "启动监听" }}
        </button>
        <small v-if="mappingSnapshot">
          · 已吞 {{ mappingSnapshot.swallowedEdges }} / 泄漏 {{ mappingSnapshot.leakedDowns }} / 触发
          {{ mappingSnapshot.firedGestures }} 次
        </small>
        <small v-if="mappingSnapshot && !mappings.enabled" class="muted"> · 总开关关闭（全部透传）</small>
      </div>
      <label class="toggle-row">
        <span>锁定当前按键</span>
        <input v-model="lockSelection" type="checkbox" class="toggle-input" />
      </label>
      <span class="muted lock-hint">按遥控器时保持当前编辑项</span>
      <div class="button-row">
        <button class="secondary-button" type="button" :disabled="busy" @click="restoreDefaults">
          恢复默认
        </button>
      </div>
    </footer>

    <p v-if="statusMessage" class="operation-message mapping-status">{{ statusMessage }}</p>
    <p v-if="mappingSnapshot?.lastError" class="error-text">{{ mappingSnapshot.lastError }}</p>
  </section>
</template>
