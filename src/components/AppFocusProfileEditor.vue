<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import {
  activateAppForLearning,
  chordLabel,
  learnFocusTarget,
  startShortcutCapture,
  stopShortcutCapture,
  subscribeShortcutCaptureEdges,
  testAppFocus,
  type AppFocusProfile,
  type FocusStrategy,
  type KeyCode,
  type ShortcutCaptureEdge,
} from "../lib/bridge";
import { FOCUS_STRATEGY_OPTIONS } from "../lib/focus-defaults";
import { focusStrategyLabel } from "../lib/app-adaptation";
import { t } from "../lib/i18n";
import { MODIFIER_KEYS, codeToKeyCode } from "../lib/key-codes";

const props = withDefaults(
  defineProps<{
    target: string;
    profile: AppFocusProfile | null;
    fallback?: AppFocusProfile | null;
    disabled?: boolean;
    save: (profile: AppFocusProfile | null) => Promise<boolean>;
  }>(),
  { fallback: null, disabled: false },
);

const emit = defineEmits<{ notice: [message: string, tone?: "success" | "error"] }>();

const effective = computed<AppFocusProfile>(
  () => props.profile ?? props.fallback ?? { strategy: "open_only" },
);
const strategy = computed(() => effective.value.strategy);
const busy = ref(false);
const learnState = ref<"idle" | "opening" | "learning" | "captured" | "error">("idle");
const learnMessage = ref<string | null>(null);
let generation = 0;

const capturing = ref(false);
const captureStarting = ref(false);
const captureDisplay = ref<KeyCode[]>([]);
const capturedChord = ref<KeyCode[] | null>(null);
const shortcutMessage = ref<string | null>(null);
const shortcutTone = ref<"success" | "error" | "info">("info");
const pressedModifiers = new Set<KeyCode>();
const pressedKeys = new Set<KeyCode>();
let captureTimeout: number | null = null;
let captureGeneration = 0;
let unlistenCapture: (() => void) | null = null;
let unmounted = false;

const learnBusy = computed(
  () => learnState.value === "opening" || learnState.value === "learning",
);
const controlsDisabled = computed(() => props.disabled || busy.value);
// 对齐 Mac：只保留「只打开 APP / 打开后自动聚焦」两个主选项；
// 旧版 auto_composer / recorded_point 档案仍可执行，但不再提供入口。
const primaryOptions = computed(() =>
  FOCUS_STRATEGY_OPTIONS.filter(
    (option) => option.value === "open_only" || option.value === "recorded_element",
  ),
);
const legacyStrategy = computed<string | null>(() => {
  const current = effective.value.strategy;
  return current === "auto_composer" || current === "recorded_point"
    ? t(focusStrategyLabel(current))
    : null;
});
const shortcut = computed(() => effective.value.shortcut);

function notice(message: string, tone: "success" | "error" = "success"): void {
  emit("notice", message, tone);
}

function setShortcutMessage(
  message: string | null,
  tone: "success" | "error" | "info" = "info",
): void {
  shortcutMessage.value = message;
  shortcutTone.value = tone;
}

function clearLearnState(): void {
  // 改选策略/切换目标都会作废进行中的学习，迟到结果不得覆盖新状态。
  generation += 1;
  learnState.value = "idle";
  learnMessage.value = null;
}

function profileForStrategy(next: FocusStrategy): AppFocusProfile {
  const previous = effective.value;
  const profile: AppFocusProfile = { strategy: next };
  if (next === "app_shortcut") {
    if (previous.shortcut?.keys?.length) profile.shortcut = previous.shortcut;
  }
  if (next === "recorded_element" && previous.recorded) profile.recorded = previous.recorded;
  return profile;
}

async function selectStrategy(next: FocusStrategy): Promise<void> {
  if (controlsDisabled.value || strategy.value === next) return;
  clearLearnState();
  setShortcutMessage(null);
  busy.value = true;
  try {
    if (!(await props.save(profileForStrategy(next)))) {
      notice(t("更新聚焦方式失败，请重试。"), "error");
    }
  } finally {
    busy.value = false;
  }
}

async function startFocusLearning(): Promise<void> {
  if (controlsDisabled.value || learnBusy.value) return;
  const requestGeneration = ++generation;
  learnState.value = "opening";
  learnMessage.value = t("正在打开目标应用…");
  try {
    await activateAppForLearning(props.target);
    if (requestGeneration !== generation || unmounted) return;
    learnState.value = "learning";
    learnMessage.value = t("正在等待你点击目标输入框……");
    const recorded = await learnFocusTarget();
    if (requestGeneration !== generation || unmounted) return;
    const saved = await props.save({
      ...profileForStrategy("recorded_element"),
      strategy: "recorded_element",
      recorded,
    });
    if (requestGeneration !== generation || unmounted) return;
    if (saved) {
      learnState.value = "captured";
      learnMessage.value = t("输入框学习成功，可以直接测试打开与聚焦。");
    } else {
      learnState.value = "error";
      learnMessage.value = t("保存输入框记录失败，请重试。");
    }
  } catch (error) {
    if (requestGeneration !== generation || unmounted) return;
    const raw = error instanceof Error ? error.message : String(error);
    console.warn("learn focus target failed", raw);
    learnState.value = "error";
    learnMessage.value = t("没有识别到可用输入框，请确认点击位置和辅助功能权限后重试。");
  }
}

async function testFocus(): Promise<void> {
  if (controlsDisabled.value) return;
  try {
    await testAppFocus(props.target);
  } catch (error) {
    console.warn("test app focus failed", error);
    notice(t("测试请求发送失败，请重试。"), "error");
  }
}

async function beginShortcutCapture(): Promise<void> {
  if (controlsDisabled.value || capturing.value || captureStarting.value) return;
  captureStarting.value = true;
  setShortcutMessage(null);
  const requestGeneration = captureGeneration;
  try {
    await startShortcutCapture();
    if (unmounted || requestGeneration !== captureGeneration) {
      await stopShortcutCapture().catch(() => undefined);
      return;
    }
    pressedKeys.clear();
    capturedChord.value = null;
    pressedModifiers.clear();
    captureDisplay.value = [];
    capturing.value = true;
    if (captureTimeout !== null) window.clearTimeout(captureTimeout);
    captureTimeout = window.setTimeout(() => {
      void finishShortcutCapture(t("录入已超时，请重新录入"), "error");
    }, 15_000);
  } catch (error) {
    console.warn("start app focus shortcut capture failed", error);
    setShortcutMessage(t("没能开始录入快捷键，请重试。"), "error");
  } finally {
    captureStarting.value = false;
  }
}

async function finishShortcutCapture(
  message?: string,
  tone: "success" | "error" | "info" = "info",
): Promise<void> {
  captureGeneration += 1;
  capturing.value = false;
  if (captureTimeout !== null) window.clearTimeout(captureTimeout);
  captureTimeout = null;
  await stopShortcutCapture().catch(() => undefined);
  pressedKeys.clear();
  capturedChord.value = null;
  pressedModifiers.clear();
  captureDisplay.value = [];
  if (message) setShortcutMessage(message, tone);
}

function acceptCapturedKey(code: KeyCode, isPressed: boolean): void {
  if (!capturing.value) return;
  if (!isPressed) {
    pressedKeys.delete(code);
    if (MODIFIER_KEYS.has(code)) pressedModifiers.delete(code);
    if (capturedChord.value) {
      captureDisplay.value = capturedChord.value;
      if (pressedKeys.size === 0) {
        const keys = capturedChord.value;
        capturedChord.value = null;
        void finishShortcutCapture();
        void saveShortcut(keys);
      }
    } else {
      captureDisplay.value = [...pressedModifiers];
    }
    return;
  }
  pressedKeys.add(code);
  if (capturedChord.value) return;
  if (MODIFIER_KEYS.has(code)) {
    pressedModifiers.add(code);
    captureDisplay.value = [...pressedModifiers];
    return;
  }
  if (code === "escape" && pressedModifiers.size === 0) {
    void finishShortcutCapture(t("已取消录入"));
    return;
  }
  const keys = [...new Set([...pressedModifiers, code])];
  capturedChord.value = keys;
  captureDisplay.value = keys;
}

async function saveShortcut(keys: KeyCode[]): Promise<void> {
  const profile: AppFocusProfile = { strategy: "app_shortcut", shortcut: { keys } };
  if (await props.save(profile)) setShortcutMessage(t("聚焦快捷键已录入"), "success");
  else setShortcutMessage(t("保存聚焦快捷键失败，请重试。"), "error");
}

async function clearShortcut(): Promise<void> {
  if (controlsDisabled.value) return;
  if (await props.save({ strategy: "app_shortcut" })) {
    setShortcutMessage(t("已清除聚焦快捷键"), "success");
  } else {
    setShortcutMessage(t("清除聚焦快捷键失败，请重试。"), "error");
  }
}

async function backToPrimary(): Promise<void> {
  if (controlsDisabled.value) return;
  if (await props.save(profileForStrategy("recorded_element"))) setShortcutMessage(null);
  else setShortcutMessage(t("更新聚焦方式失败，请重试。"), "error");
}

function handleKeydown(event: KeyboardEvent): void {
  if (!capturing.value) return;
  event.preventDefault();
  event.stopPropagation();
  const code = codeToKeyCode(event.code);
  if (code) acceptCapturedKey(code, true);
}

function handleKeyup(event: KeyboardEvent): void {
  if (!capturing.value) return;
  const code = codeToKeyCode(event.code);
  if (code) acceptCapturedKey(code, false);
}

function handleBlur(): void {
  if (capturing.value || captureStarting.value) {
    void finishShortcutCapture(t("窗口失去焦点，已取消录入"));
  }
}

watch(
  () => props.target,
  () => {
    generation += 1;
    clearLearnState();
    setShortcutMessage(null);
    void finishShortcutCapture();
  },
);

watch(capturing, (active) => {
  if (!active) {
    pressedKeys.clear();
    pressedModifiers.clear();
    captureDisplay.value = [];
  }
});

onMounted(async () => {
  window.addEventListener("keydown", handleKeydown, true);
  window.addEventListener("keyup", handleKeyup, true);
  window.addEventListener("blur", handleBlur);
  unlistenCapture = await subscribeShortcutCaptureEdges((edge: ShortcutCaptureEdge) => {
    acceptCapturedKey(edge.key, edge.isPressed);
  });
  if (unmounted) {
    unlistenCapture?.();
    unlistenCapture = null;
  }
});

onUnmounted(() => {
  unmounted = true;
  generation += 1;
  window.removeEventListener("keydown", handleKeydown, true);
  window.removeEventListener("keyup", handleKeyup, true);
  window.removeEventListener("blur", handleBlur);
  unlistenCapture?.();
  unlistenCapture = null;
  if (captureTimeout !== null) window.clearTimeout(captureTimeout);
  captureTimeout = null;
  void stopShortcutCapture().catch(() => undefined);
});
</script>

<template>
  <section class="section-card focus-editor" :aria-busy="busy || learnBusy">
    <div class="section-head">
      <h3>{{ t("聚焦方式") }}</h3>
      <span class="spacer"></span>
    </div>

    <div class="option-grid">
      <button
        v-for="option in primaryOptions"
        :key="option.value"
        class="focus-option"
        :class="{ selected: strategy === option.value }"
        type="button"
        :aria-pressed="strategy === option.value"
        :disabled="props.disabled"
        @click="selectStrategy(option.value)"
      >
        <span class="radio" aria-hidden="true"></span>
        <span>{{ t(option.label) }}</span>
      </button>
    </div>

    <p v-if="legacyStrategy" class="hint legacy-hint">
      {{ t("当前为旧版聚焦方式：{strategy}。选择上方任一方式后生效。", { strategy: legacyStrategy }) }}
    </p>

    <template v-if="strategy === 'recorded_element'">
      <p class="hint">
        {{ t("点击后会立即打开目标 APP。请在 3 秒内点击需要聚焦的输入框；无线麦 SayAll 只记录辅助功能特征，不读取输入内容。") }}
      </p>
      <div class="actions">
        <button class="primary" type="button" :disabled="controlsDisabled || learnBusy" @click="startFocusLearning">
          {{
            learnState === "opening"
              ? t("正在打开应用…")
              : learnState === "learning"
                ? t("正在等待你点击目标输入框…")
                : t("开始学习输入框")
          }}
        </button>
        <button
          class="secondary"
          type="button"
          :disabled="controlsDisabled || !effective.recorded"
          @click="testFocus"
        >
          {{ t("测试输入框") }}
        </button>
        <span class="inline-status" :class="{ pending: !effective.recorded }">
          {{ effective.recorded ? t("已记录目标输入框") : t("尚未记录目标输入框") }}
        </span>
      </div>
      <p v-if="learnMessage" class="state-message" :class="learnState">{{ learnMessage }}</p>
      <p v-else-if="!effective.recorded" class="hint">
        {{ t("学习只需要在目标 APP 里点一下输入框，比录快捷键更快。") }}
      </p>
    </template>

    <template v-else-if="strategy === 'app_shortcut'">
      <p class="hint">
        {{ t("先点击“录入快捷键”，再按下该 APP 用于聚焦输入框的单个按键或组合键。执行时会先激活 APP，再发送此按键。") }}
      </p>
      <div class="shortcut-row">
        <span class="shortcut-value" :class="{ empty: !shortcut?.keys?.length }">
          {{ shortcut?.keys?.length ? chordLabel(shortcut) : t("尚未录入") }}
        </span>
        <button
          class="primary"
          type="button"
          :disabled="controlsDisabled || captureStarting"
          @click="capturing ? finishShortcutCapture(t('已取消录入')) : beginShortcutCapture()"
        >
          {{ capturing ? t("录入中…（按 Esc 取消）") : t("录入快捷键") }}
        </button>
        <button class="secondary" type="button" :disabled="controlsDisabled || !shortcut?.keys?.length" @click="clearShortcut">
          {{ t("清除") }}
        </button>
      </div>
      <div v-if="capturing" class="capture-display">
        {{ captureDisplay.length ? chordLabel({ keys: captureDisplay }) : t("请按下快捷键组合") }}
      </div>
      <div class="shortcut-footer">
        <span v-if="shortcutMessage" class="shortcut-message" :class="shortcutTone">
          {{ shortcutMessage }}
        </span>
        <span class="spacer"></span>
        <button class="quiet" type="button" :disabled="controlsDisabled" @click="backToPrimary">
          {{ t("改回打开后自动聚焦") }}
        </button>
      </div>
    </template>

    <div v-if="strategy !== 'app_shortcut'" class="fallback-row">
      <span>{{ t("这个 APP 有自带的聚焦快捷键？也可以用它代替学习。") }}</span>
      <button class="quiet" type="button" :disabled="controlsDisabled" @click="selectStrategy('app_shortcut')">
        {{ t("改用 APP 快捷键聚焦") }}
      </button>
    </div>

    <div class="focus-footer">
      <button class="primary" type="button" :disabled="props.disabled" @click="testFocus">
        {{ t("测试打开与聚焦") }}
      </button>
    </div>
  </section>
</template>

<style scoped>
.section-card {
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface-subtle);
}
.section-head {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 20px;
}
.section-head h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
}
.section-head .spacer {
  flex: 1 1 auto;
}
.option-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 7px;
  margin-top: 8px;
}
.focus-option {
  min-height: 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 6px 9px;
  border: 1px solid var(--border-strong);
  border-radius: 8px;
  background: var(--surface-raised);
  color: var(--text-primary);
  text-align: center;
  font-size: 12.5px;
}
.focus-option:hover:not(:disabled) {
  border-color: var(--accent-border);
}
.focus-option.selected {
  border-color: rgba(95, 98, 219, 0.5);
  background: var(--accent-surface);
  color: var(--accent-text);
  font-weight: 600;
}
.radio {
  width: 15px;
  height: 15px;
  flex: 0 0 15px;
  display: grid;
  place-items: center;
  border: 1.4px solid #9aa0ad;
  border-radius: 50%;
}
.focus-option.selected .radio {
  border-color: var(--accent);
}
.focus-option.selected .radio::after {
  content: "";
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--accent);
}
.hint {
  margin: 6px 0 0;
  color: var(--muted);
  font-size: 11.5px;
  line-height: 1.4;
}
.actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 7px;
  margin-top: 8px;
}
.primary,
.secondary,
.quiet {
  min-height: 28px;
  padding: 4px 10px;
  border: 1px solid transparent;
  border-radius: 7px;
  font-size: 12px;
  font-weight: 600;
  white-space: nowrap;
}
.primary {
  color: var(--text-on-accent);
  background: var(--accent);
}
.primary:hover:not(:disabled) {
  background: var(--accent-dark);
}
.secondary {
  color: var(--text-control);
  border-color: var(--border);
  background: var(--surface-control);
}
.secondary:hover:not(:disabled) {
  border-color: var(--border-strong);
  background: var(--surface-raised);
}
.quiet {
  padding-inline: 4px;
  color: var(--accent-text);
  background: transparent;
}
.quiet:hover:not(:disabled) {
  background: var(--accent-surface);
}
button:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}
.inline-status {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  margin-left: 2px;
  color: var(--success-text);
  font-size: 11.5px;
  font-weight: 600;
}
.inline-status::before {
  content: "";
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--dot-success);
}
.inline-status.pending {
  color: var(--muted);
}
.inline-status.pending::before {
  background: var(--dot-pending);
}
.state-message {
  margin: 6px 0 0;
  color: var(--muted);
  font-size: 11.5px;
  font-weight: 600;
}
.state-message.captured,
.state-message.recorded {
  color: var(--success-text);
}
.state-message.error {
  color: var(--error-text);
}
.shortcut-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 7px;
  margin-top: 8px;
}
.shortcut-value {
  min-height: 30px;
  display: inline-flex;
  align-items: center;
  padding: 4px 10px;
  border: 1px solid var(--border);
  border-radius: 7px;
  background: var(--surface-raised);
  font-family: Consolas, monospace;
  font-size: 12px;
}
.shortcut-value.empty {
  color: var(--muted);
  font-family: inherit;
  font-weight: 400;
}
.capture-display {
  margin-top: 7px;
  padding: 6px 9px;
  border: 1px dashed var(--accent-border);
  border-radius: 7px;
  color: var(--accent-text);
  background: var(--accent-surface);
  font-family: Consolas, monospace;
  font-size: 12px;
}
.shortcut-footer {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 30px;
  margin-top: 8px;
}
.shortcut-footer .spacer {
  flex: 1 1 auto;
}
.shortcut-message {
  font-size: 12px;
  font-weight: 600;
  color: var(--muted);
}
.shortcut-message.success {
  color: var(--success-text);
}
.shortcut-message.error {
  color: var(--error-text);
}
.fallback-row {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 6px;
  margin-top: 7px;
  color: var(--muted);
  font-size: 11.5px;
  line-height: 1.35;
}
.fallback-row .quiet {
  white-space: normal;
  text-align: left;
}
.focus-footer {
  display: flex;
  justify-content: flex-end;
  margin-top: 10px;
}
</style>
