<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import RegisteredAppsDialog from "./RegisteredAppsDialog.vue";
import {
  actionSummary,
  mouseClickLabels, mouseMoveLabels, registerPresetAppNames,
  startShortcutCapture, stopShortcutCapture, subscribeShortcutCaptureEdges,
  type MoveDirection, type AppLibraryEntry,
  chordLabel,
  pickCustomApp,
  type ButtonAction,
  type ButtonMappings,
  type ButtonTrigger,
  type KeyCode,
  type PresetAppInfo,
  type RemoteButton,
} from "../lib/bridge";

const props = defineProps<{
  mappings: ButtonMappings;
  button: RemoteButton;
  trigger: ButtonTrigger;
  presetApps: PresetAppInfo[];
  capabilityNote?: string | null;
  shortcutsOnly?: boolean;
}>();
const emit = defineEmits<{
  update: [action: ButtonAction];
  status: [message: string];
  applications: [apps: AppLibraryEntry[]];
}>();

const capturingShortcut = ref(false);
const captureStarting = ref(false);
const captureDisplay = ref<string[]>([]);
const safeCaptureMode = ref(false);
const capturePressedKeys = new Set<KeyCode>();
let capturedChord: KeyCode[] | null = null;
let captureTimeout: number | null = null;
let captureRequestId = 0;
let unmounted = false;
let unlistenCapture: (() => void) | null = null;
const statusMessage = ref<string | null>(null);
watch(statusMessage, message => { if (message) emit("status", message); });
const action = computed<ButtonAction>(() => props.mappings.actions[props.button]?.[props.trigger] ?? { type: "disabled" });
const presetIds = computed(() => new Set(props.presetApps.map((app) => app.id)));
const customApps = computed(() => {
  const found = new Map<string, string>((props.mappings.applications ?? []).map(app => [app.path, app.name]));
  for (const actions of Object.values(props.mappings.actions)) {
    for (const mapped of Object.values(actions)) {
      if (mapped.type === "open_app" && !presetIds.value.has(mapped.target)) {
        const base = mapped.target.split(/[\\/]/).pop() ?? mapped.target;
        found.set(mapped.target, base.replace(/\.(exe|lnk)$/i, "") || base);
      }
    }
  }
  return [...found].map(([path, name]) => ({ path, name }));
});

const groups: Array<{ label: string; items: Array<{ label: string; keys: KeyCode[] }> }> = [
  { label: "基础按键", items: [
    { label: "Enter", keys: ["enter"] }, { label: "Esc", keys: ["escape"] },
    { label: "空格", keys: ["space"] }, { label: "Tab", keys: ["tab"] },
    { label: "退格", keys: ["backspace"] }, { label: "删除", keys: ["delete"] },
    { label: "↑", keys: ["up"] }, { label: "↓", keys: ["down"] },
    { label: "←", keys: ["left"] }, { label: "→", keys: ["right"] },
    { label: "Home", keys: ["home"] }, { label: "复制", keys: ["control", "c"] },
    { label: "粘贴", keys: ["control", "v"] }, { label: "剪切", keys: ["control", "x"] },
    { label: "全选", keys: ["control", "a"] }, { label: "撤销", keys: ["control", "z"] },
    { label: "重做", keys: ["control", "y"] }, { label: "查找", keys: ["control", "f"] },
    { label: "保存", keys: ["control", "s"] }, { label: "发送", keys: ["control", "enter"] },
    { label: "换行", keys: ["shift", "enter"] }, { label: "右键菜单", keys: ["apps"] },
    { label: "刷新", keys: ["f5"] },
  ] },
  { label: "系统与媒体", items: [
    { label: "切换窗口", keys: ["alt", "tab"] }, { label: "显示桌面", keys: ["left_windows", "d"] },
    { label: "关闭窗口", keys: ["control", "w"] }, { label: "锁定", keys: ["left_windows", "l"] },
    { label: "搜索", keys: ["left_windows", "s"] }, { label: "截图", keys: ["left_windows", "shift", "s"] },
    { label: "静音", keys: ["volume_mute"] }, { label: "音量+", keys: ["volume_up"] },
    { label: "音量−", keys: ["volume_down"] }, { label: "播放/暂停", keys: ["media_play_pause"] },
    { label: "上一首", keys: ["media_prev"] }, { label: "下一首", keys: ["media_next"] },
  ] },
];

function selected(keys: KeyCode[]): boolean {
  return action.value.type === "shortcut" && action.value.chord.keys.join("+") === keys.join("+");
}
function apply(next: ButtonAction): void { emit("update", next); }
async function addCustomApp(): Promise<void> {
  const pick = await pickCustomApp();
  if (pick) apply({ type: "open_app", target: pick.path });
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

const selectedCaptureModifiers = reactive(new Set<KeyCode>());
const pressedCaptureModifiers = new Set<KeyCode>();
const MODIFIER_KEYS = new Set<KeyCode>([
  "left_control",
  "right_control",
  "left_shift",
  "right_shift",
  "left_alt",
  "right_alt",
  "left_windows",
  "right_windows",
]);
const CAPTURE_MODIFIER_OPTIONS: Array<{ key: KeyCode; label: string }> = [
  { key: "left_control", label: "左 Ctrl" },
  { key: "left_shift", label: "左 Shift" },
  { key: "left_alt", label: "左 Alt" },
  { key: "left_windows", label: "左 Win" },
  { key: "right_control", label: "右 Ctrl" },
  { key: "right_shift", label: "右 Shift" },
  { key: "right_alt", label: "右 Alt" },
  { key: "right_windows", label: "右 Win" },
];

function toggleCaptureModifier(key: KeyCode): void {
  if (!capturingShortcut.value || capturedChord) return;
  if (selectedCaptureModifiers.has(key)) selectedCaptureModifiers.delete(key);
  else selectedCaptureModifiers.add(key);
  captureDisplay.value = [...selectedCaptureModifiers];
}

async function beginShortcutCapture(): Promise<void> {
  if (capturingShortcut.value || captureStarting.value) return;
  const requestId = ++captureRequestId;
  captureStarting.value = true;
  statusMessage.value = null;
  try {
    await startShortcutCapture();
    if (unmounted || requestId !== captureRequestId) {
      await stopShortcutCapture().catch(() => undefined);
      return;
    }
    capturePressedKeys.clear();
    capturedChord = null;
    selectedCaptureModifiers.clear();
    pressedCaptureModifiers.clear();
    captureDisplay.value = [];
    capturingShortcut.value = true;
    if (captureTimeout !== null) window.clearTimeout(captureTimeout);
    captureTimeout = window.setTimeout(() => {
      void finishShortcutCapture("录入已超时，请重新录入");
    }, 15_000);
  } catch (error) {
    statusMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    if (requestId === captureRequestId) captureStarting.value = false;
  }
}

async function finishShortcutCapture(message?: string): Promise<void> {
  captureRequestId += 1;
  captureStarting.value = false;
  capturingShortcut.value = false;
  if (captureTimeout !== null) window.clearTimeout(captureTimeout);
  captureTimeout = null;
  await stopShortcutCapture().catch(() => undefined);
  capturePressedKeys.clear();
  capturedChord = null;
  pressedCaptureModifiers.clear();
  if (message) statusMessage.value = message;
}

function handleCaptureBlur(): void {
  if (capturingShortcut.value || captureStarting.value) {
    void finishShortcutCapture("窗口失去焦点，已取消录入");
  }
}

function acceptCapturedKey(code: KeyCode, isPressed: boolean, repeat = false): void {
  if (!capturingShortcut.value) return;
  if (!isPressed) {
    capturePressedKeys.delete(code);
    if (MODIFIER_KEYS.has(code)) pressedCaptureModifiers.delete(code);
    if (capturedChord) {
      captureDisplay.value = capturedChord;
      if (capturePressedKeys.size === 0) {
        const label = chordLabel({ keys: capturedChord });
        void finishShortcutCapture(`快捷键已录入：${label}`);
      }
    } else {
      captureDisplay.value = safeCaptureMode.value
        ? [...selectedCaptureModifiers]
        : [...pressedCaptureModifiers];
    }
    return;
  }
  if (!repeat) capturePressedKeys.add(code);
  // 已经拿到终止键后继续保持原生拦截，直到本次组合的所有 DOWN 都收到配对 UP。
  // 这避免 Win+L 在录入完成但物理键尚未松开时被 Windows 补执行。
  if (capturedChord) return;
  if (MODIFIER_KEYS.has(code)) {
    if (!repeat) pressedCaptureModifiers.add(code);
    if (safeCaptureMode.value) {
      statusMessage.value = "安全录入中：请松开键盘修饰键，并在界面中点击选择";
    } else {
      captureDisplay.value = [...pressedCaptureModifiers];
    }
    return;
  }
  if (safeCaptureMode.value && pressedCaptureModifiers.size > 0) {
    statusMessage.value = "未录入：请不要按住键盘修饰键；先在界面选择修饰键，再单独按主键";
    return;
  }
  const modifiers = safeCaptureMode.value
    ? [...selectedCaptureModifiers]
    : [...pressedCaptureModifiers];
  if (code === "escape" && modifiers.length === 0) {
    void finishShortcutCapture("已取消录入");
    return;
  }
  const keys = [...modifiers, code];
  capturedChord = keys;
  captureDisplay.value = keys;
  apply({ type: "shortcut", chord: { keys } });
  statusMessage.value = `已录入 ${chordLabel({ keys })}，松开全部按键后完成`;
}

function handleCaptureKeydown(event: KeyboardEvent): void {
  if (!capturingShortcut.value) return;
  event.preventDefault();
  event.stopPropagation();
  const code = codeToKeyCode(event.code);
  if (code === null) return;
  acceptCapturedKey(code, true, event.repeat);
}

function handleCaptureKeyup(event: KeyboardEvent): void {
  if (!capturingShortcut.value) return;
  const code = codeToKeyCode(event.code);
  if (code) acceptCapturedKey(code, false);
}

watch(capturingShortcut, (active) => {
  if (!active) {
    selectedCaptureModifiers.clear();
    pressedCaptureModifiers.clear();
    captureDisplay.value = [];
  }
});

const selectedAction = action;
const scrollSteps = computed(() => selectedAction.value?.type === "scroll" ? selectedAction.value.steps ?? 1 : 1);
const moveDistance = computed(() => selectedAction.value?.type === "mouse_move" ? selectedAction.value.distance : 30);
const moveSymbols: Record<MoveDirection, string> = { up: "↑", down: "↓", left: "←", right: "→" };

function updateMouseAmount(event: Event, kind: "scroll" | "mouse_move"): void {
  const input = event.target as HTMLInputElement;
  const value = input.valueAsNumber;
  const maximum = kind === "scroll" ? 100 : 2000;
  if (!Number.isInteger(value) || value < 1 || value > maximum) {
    statusMessage.value = `请输入 1 到 ${maximum} 之间的整数`;
    input.value = String(kind === "scroll" ? scrollSteps.value : moveDistance.value);
    return;
  }
  const action = selectedAction.value;
  if (action?.type === "scroll" && kind === "scroll") void apply({ ...action, steps: value });
  if (action?.type === "mouse_move" && kind === "mouse_move") void apply({ ...action, distance: value });
}

function isActiveScroll(direction: "up" | "down"): boolean {
  const current = action.value;
  return current.type === "scroll" && current.direction === direction;
}


const appPickerOpen = ref(false);
const appFilter = ref("");
const filteredCustomApps = computed(() => customApps.value.filter(app => app.name.toLocaleLowerCase().includes(appFilter.value.trim().toLocaleLowerCase())));
watch([() => props.presetApps, () => props.mappings.applications], () => {
  registerPresetAppNames([...props.presetApps, ...(props.mappings.applications ?? []).map(app => ({id: app.path, name: app.name}))]);
}, {deep: true, immediate: true});
function addScannedApps(apps: AppLibraryEntry[]): void {
  const unique = new Map((props.mappings.applications ?? []).map(app => [app.path.toLowerCase(), app]));
  for (const app of apps) unique.set(app.path.toLowerCase(), app);
  emit("applications", [...unique.values()]);
  appPickerOpen.value = false;
  emit("status", `已添加 ${apps.length} 个应用到草稿；保存当前配置后生效，按键绑定未改变`);
}
onMounted(async () => {
  window.addEventListener("keydown", handleCaptureKeydown, true);
  window.addEventListener("keyup", handleCaptureKeyup, true);
  window.addEventListener("blur", handleCaptureBlur);
  try {
    const stop = await subscribeShortcutCaptureEdges(edge => acceptCapturedKey(edge.key, edge.isPressed));
    if (unmounted) stop(); else unlistenCapture = stop;
  } catch (cause) { statusMessage.value = String(cause); }
});
onUnmounted(() => {
  unmounted = true;
  window.removeEventListener("keydown", handleCaptureKeydown, true);
  window.removeEventListener("keyup", handleCaptureKeyup, true);
  window.removeEventListener("blur", handleCaptureBlur);
  unlistenCapture?.();
  if (capturingShortcut.value || captureStarting.value) void finishShortcutCapture();
});
</script>

<template>
  <div class="button-action-editor">
    <p class="muted">当前：{{ actionSummary(action) }}</p>
<div class="action-sections">
<p v-if="capabilityNote" class="muted editor-note capability-note">{{ capabilityNote }}</p>
        <p v-if="button === 'ok' && (trigger !== 'single' || action.type !== 'shortcut' || action.chord.keys.length !== 1 || action.chord.keys[0] !== 'enter')" class="muted editor-note" role="note">确认键的首次原生 Enter 可能先到达目标窗口；其他组合键、双击或长按不能保证阻止这次原生确认。需要即时确认时请使用单击 Enter。</p>
        <section class="action-section">
          <h4 class="action-section-title">系统任务选择</h4>
          <div class="preset-grid">
            <button class="chip" type="button" :class="{ selected: action.type === 'task_switch' && action.view === 'applications' }" @click="apply({ type: 'task_switch', view: 'applications' })">任务切换</button>
            <button class="chip" type="button" :class="{ selected: action.type === 'task_switch' && action.view === 'desktops' }" @click="apply({ type: 'task_switch', view: 'desktops' })">任务视图</button>
          </div>
          <p class="muted editor-note">方向选择，确认切换，返回取消；TV 按各手势配置执行。不持续按住 Alt。</p>
        </section>
        <p v-if="capabilityNote" class="muted editor-note capability-note">{{ capabilityNote }}</p>
        <section v-for="group in groups" :key="group.label" class="action-section">
          <h4 class="action-section-title">{{ group.label }}</h4>
          <div class="preset-grid">
            <!-- 芯片显示实际按键组合（组合在不同 App 里语义不同，功能描述
                 只作悬停提示，避免把 Ctrl+C 一类写成"复制"造成误判）。 -->
            <button
              v-for="preset in group.items"
              :key="preset.label"
              class="chip"
              :class="{ selected: selected(preset.keys) }"
              type="button"
              :title="preset.label"
              @click="apply({ type: 'shortcut', chord: { keys: [...preset.keys] } })"
            >
              {{ chordLabel({ keys: preset.keys }) }}
            </button>
          </div>
        </section>

        <section v-if="!shortcutsOnly" class="action-section">
          <h4 class="action-section-title">鼠标滚轮</h4>
          <div class="preset-grid">
            <button v-for="direction in (['up', 'down'] as const)" :key="direction" class="chip"
              :class="{ selected: isActiveScroll(direction) }" type="button" title="在鼠标当前位置滚动"
              @click="apply({ type: 'scroll', direction, steps: scrollSteps })">{{ direction === "up" ? "滚轮向上" : "滚轮向下" }}</button>
          </div>
          <label v-if="selectedAction?.type === 'scroll'" class="mouse-amount">
            <span>每次滚动</span>
            <input aria-label="每次滚动格数" type="number" min="1" max="100" step="1" :value="scrollSteps" @change="updateMouseAmount($event, 'scroll')" />
            <span>格</span>
          </label>
        </section>

        <section v-if="!shortcutsOnly" class="action-section">
          <h4 class="action-section-title">鼠标点击</h4>
          <div class="preset-grid">
            <button v-for="(label, kind) in mouseClickLabels" :key="kind" class="chip" type="button"
              :class="{ selected: selectedAction?.type === 'mouse_click' && selectedAction.kind === kind }"
              title="点击鼠标当前位置" @click="apply({ type: 'mouse_click', kind })">{{ label }}</button>
          </div>
        </section>

        <section v-if="!shortcutsOnly" class="action-section">
          <h4 class="action-section-title">鼠标移动</h4>
          <div class="preset-grid">
            <button v-for="(label, direction) in mouseMoveLabels" :key="direction" class="chip mouse-direction" type="button"
              :aria-label="label" :title="label" :class="{ selected: selectedAction?.type === 'mouse_move' && selectedAction.direction === direction }"
              @click="apply({ type: 'mouse_move', direction, distance: moveDistance })">{{ moveSymbols[direction] }}</button>
          </div>
          <label v-if="selectedAction?.type === 'mouse_move'" class="mouse-amount">
            <span>每次移动</span>
            <input aria-label="每次移动像素" type="number" min="1" max="2000" step="1" :value="moveDistance" @change="updateMouseAmount($event, 'mouse_move')" />
            <span>像素</span>
          </label>
        </section>

        <section v-if="!shortcutsOnly" class="action-section">
          <h4 class="action-section-title">打开应用</h4>
          <div class="preset-grid app-preset-grid">
            <button
              v-for="app in presetApps"
              :key="app.id"
              class="chip"
              :class="{ selected: (action.type === 'open_app' ? action.target : null) === app.id }"
              type="button"
              title="已运行则切到该应用窗口，未运行则启动"
              @click="apply({ type: 'open_app', target: app.id })"
            >
              {{ app.name }}
            </button>
            <button v-for="app in filteredCustomApps" :key="app.path" class="chip" type="button"
              :class="{ selected: (action.type === 'open_app' ? action.target : null) === app.path }"
              :title="app.name" @click="apply({ type: 'open_app', target: app.path })">{{ app.name }}</button>
            <button class="chip" type="button" @click="appPickerOpen = true">扫描本机应用</button>
            <button
              class="chip add-app"
              type="button"
              title="从本机选择任意程序或快捷方式"
              @click="addCustomApp"
            >
              ＋ 添加应用
            </button>
          </div>
        </section>

        <section class="action-section">
          <h4 class="action-section-title">自定义</h4>
          <div class="custom-shortcut-row">
            <button
              class="chip"
              :class="{ selected: capturingShortcut }"
              type="button"
              :disabled="captureStarting"
              @click="capturingShortcut ? finishShortcutCapture('已取消录入') : beginShortcutCapture()"
            >
              {{ capturingShortcut ? "录入中…（按 Esc 取消）" : "录入自定义快捷键" }}
            </button>
            <span v-if="capturingShortcut" class="capture-display">
              {{ captureDisplay.length ? chordLabel({ keys: captureDisplay }) : (safeCaptureMode ? "先选择修饰键" : "请按下快捷键组合") }}
            </span>
          </div>
          <label
            class="toggle-row safe-capture-toggle"
            title="开启后，通过界面选择修饰键，键盘只需按主键。"
          >
            <span>安全录入模式</span>
            <input
              v-model="safeCaptureMode"
              type="checkbox"
              class="toggle-input"
              :disabled="capturingShortcut || captureStarting"
            />
            <small class="muted safe-capture-hint">
              直接录入无法完成或会触发系统动作时再开启。
            </small>
          </label>
          <template v-if="capturingShortcut && safeCaptureMode">
            <p class="muted editor-note capture-guide">
              请用鼠标选择修饰键，再只按一次主键。不要在键盘上按完整组合，系统快捷键不会被执行。
            </p>
            <div class="preset-grid capture-modifiers">
              <button
                v-for="modifier in CAPTURE_MODIFIER_OPTIONS"
                :key="modifier.key"
                class="chip"
                :class="{ selected: selectedCaptureModifiers.has(modifier.key) }"
                type="button"
                @click="toggleCaptureModifier(modifier.key)"
              >
                {{ modifier.label }}
              </button>
            </div>
            <p class="capture-display">然后单独按主键（例如选择“左 Win”后，只按 L）</p>
          </template>
        </section>
      </div>

    <RegisteredAppsDialog v-if="appPickerOpen" :known-apps="mappings.applications ?? []" :saving="false" :save-error="null" @close="appPickerOpen = false" @add="addScannedApps" />
  </div>
</template>

<style scoped>
.button-action-editor { display: grid; gap: 14px; }
.action-sections { display: grid; gap: 18px; }
.mouse-amount { display: flex; align-items: center; gap: 8px; }
.mouse-amount input { width: 84px; }
.saved-app-grid { max-height: 200px; overflow-y: auto; }
.app-library-search { max-width: 280px; margin: 8px 0; }
.action-section { display: grid; gap: 8px; }
.action-section h4 { margin: 0; }
.preset-grid { display: flex; flex-wrap: wrap; gap: 8px; }
.capture-display { margin-left: 10px; color: var(--text-secondary); }
</style>
