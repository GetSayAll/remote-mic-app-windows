<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import {
  activateAppForLearning,
  chordLabel,
  focusPointErrorLabel,
  learnFocusPoint,
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
import { t } from "../lib/i18n";
import { MODIFIER_KEYS, codeToKeyCode } from "../lib/key-codes";

const props = withDefaults(
  defineProps<{
    target: string;
    profile: AppFocusProfile | null;
    fallback?: AppFocusProfile | null;
    disabled?: boolean;
    /** 只有预设应用才提供「自动选择输入框」；自定义应用隐藏。 */
    allowAutoComposer?: boolean;
    save: (profile: AppFocusProfile | null) => Promise<boolean>;
  }>(),
  { fallback: null, disabled: false, allowAutoComposer: false },
);

const emit = defineEmits<{ notice: [message: string, tone?: "success" | "error"] }>();

const effective = computed<AppFocusProfile>(
  () => props.profile ?? props.fallback ?? { strategy: "open_only" },
);
const strategy = computed(() => effective.value.strategy);
const busy = ref(false);
const learnState = ref<"idle" | "opening" | "learning" | "captured" | "error">("idle");
const learnMessage = ref<string | null>(null);
const pointState = ref<"idle" | "opening" | "recording" | "recorded" | "error">("idle");
const pointMessage = ref<string | null>(null);
let pointTimer: number | null = null;
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
const pointBusy = computed(
  () => pointState.value === "opening" || pointState.value === "recording",
);
const controlsDisabled = computed(() => props.disabled || busy.value);
// 顺序按产品编号：自动选择(0) → 只打开(1) → 打开后自动聚焦(2) →
// APP 快捷键(3) → 记录点击位置(4)。自动选择只给预设应用。
const primaryOptions = computed(() =>
  FOCUS_STRATEGY_OPTIONS.filter(
    (option) =>
      option.value !== "auto_composer" ||
      props.allowAutoComposer ||
      strategy.value === "auto_composer",
  ),
);
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
  // 改选策略/切换目标都会作废进行中的学习与位置记录，迟到结果不得覆盖新状态。
  generation += 1;
  learnState.value = "idle";
  learnMessage.value = null;
  pointState.value = "idle";
  pointMessage.value = null;
  if (pointTimer !== null) window.clearTimeout(pointTimer);
  pointTimer = null;
}

function profileForStrategy(next: FocusStrategy): AppFocusProfile {
  const previous = effective.value;
  const profile: AppFocusProfile = { strategy: next };
  if (next === "app_shortcut") {
    const shortcut = previous.shortcut ?? props.fallback?.shortcut;
    if (shortcut?.keys?.length) profile.shortcut = shortcut;
  }
  if (next === "recorded_element" && previous.recorded) profile.recorded = previous.recorded;
  if (next === "recorded_point" && previous.point) profile.point = previous.point;
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
  if (controlsDisabled.value || learnBusy.value || pointBusy.value) return;
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

async function startPointRecording(): Promise<void> {
  if (controlsDisabled.value || pointBusy.value || learnBusy.value) return;
  const requestGeneration = ++generation;
  pointState.value = "opening";
  pointMessage.value = t("正在打开目标应用…");
  try {
    await activateAppForLearning(props.target);
  } catch {
    if (requestGeneration !== generation || unmounted) return;
    pointState.value = "error";
    pointMessage.value = t("没能打开目标应用，请确认它已安装后再试。");
    return;
  }
  if (requestGeneration !== generation || unmounted) return;
  pointState.value = "recording";
  pointMessage.value = t("正在等待你把鼠标停在目标输入框上……（6 秒后自动记录）");
  if (pointTimer !== null) window.clearTimeout(pointTimer);
  pointTimer = window.setTimeout(() => {
    pointTimer = null;
    void finishPointRecording(requestGeneration);
  }, 6000);
}

async function finishPointRecording(requestGeneration: number): Promise<void> {
  if (requestGeneration !== generation || unmounted) return;
  try {
    const point = await learnFocusPoint(props.target);
    if (requestGeneration !== generation || unmounted) return;
    const saved = await props.save({
      ...profileForStrategy("recorded_point"),
      strategy: "recorded_point",
      point,
    });
    if (requestGeneration !== generation || unmounted) return;
    if (saved) {
      pointState.value = "recorded";
      pointMessage.value = t(
        "已记录位置（窗口大小变化超过 25% 时会拒绝点击，需要重新记录）",
      );
    } else {
      pointState.value = "error";
      pointMessage.value = t("保存点击位置失败，请重试。");
    }
  } catch (error) {
    if (requestGeneration !== generation || unmounted) return;
    const raw = error instanceof Error ? error.message : String(error);
    console.warn("learn focus point failed", raw);
    pointState.value = "error";
    pointMessage.value = t("记录位置失败：{reason}", {
      reason: focusPointErrorLabel(raw),
    });
  }
}

async function clearRecordedPoint(): Promise<void> {
  if (controlsDisabled.value) return;
  clearLearnState();
  const saved = await props.save({ strategy: "recorded_point" });
  pointMessage.value = saved ? t("已清除记录") : t("清除记录失败，请重试。");
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
  if (pointTimer !== null) window.clearTimeout(pointTimer);
  pointTimer = null;
  if (captureTimeout !== null) window.clearTimeout(captureTimeout);
  captureTimeout = null;
  void stopShortcutCapture().catch(() => undefined);
});
</script>

<template>
  <section class="section-card focus-editor" :aria-busy="busy || learnBusy || pointBusy">
    <div class="section-head">
      <h3>{{ t("聚焦方式") }}</h3>
      <span class="spacer"></span>
    </div>

    <!-- 选中态对齐 Mac SettingsView：checkmark.circle.fill + accent 50% 边框、
         12% 背景，未选中为空心 circle。 -->
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
        <svg
          v-if="strategy === option.value"
          class="radio-icon"
          viewBox="0 0 16 16"
          aria-hidden="true"
        >
          <path
            fill="currentColor"
            fill-rule="evenodd"
            d="M8 1a7 7 0 1 0 0 14A7 7 0 0 0 8 1Zm-3.6 6.2 2.3 2.3 4.8-4.8.9.9-5.7 5.7-3.2-3.2.9-.9Z"
          />
        </svg>
        <svg v-else class="radio-icon" viewBox="0 0 16 16" aria-hidden="true">
          <circle cx="8" cy="8" r="6.4" fill="none" stroke="currentColor" stroke-width="1.2" />
        </svg>
        <span class="option-label">
          {{ t(option.label) }}
          <small v-if="option.value === 'auto_composer'">{{ t("预置可用") }}</small>
          <small v-else-if="option.value === 'recorded_point'">{{ t("界面不暴露输入框时") }}</small>
        </span>
      </button>
    </div>

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

    <template v-else-if="strategy === 'recorded_point'">
      <p class="hint">
        {{ t("「记录点击位置」会先打开目标应用；把鼠标停在要点击的位置上，6 秒后自动记录，之后按这个位置点击聚焦。") }}
      </p>
      <div class="actions">
        <button class="primary" type="button" :disabled="controlsDisabled || pointBusy || learnBusy" @click="startPointRecording">
          {{
            pointState === "opening"
              ? t("正在打开应用…")
              : pointState === "recording"
                ? t("等待鼠标停留…")
                : effective.point
                  ? t("重新记录位置")
                  : t("开始记录位置")
          }}
        </button>
        <button
          class="secondary"
          type="button"
          :disabled="controlsDisabled || !effective.point"
          @click="testFocus"
        >
          {{ t("测试打开与聚焦") }}
        </button>
        <span class="inline-status" :class="{ pending: !effective.point }">
          {{ effective.point ? t("已记录点击位置") : t("尚未记录点击位置") }}
        </span>
        <button
          v-if="effective.point"
          class="quiet"
          type="button"
          :disabled="controlsDisabled"
          @click="clearRecordedPoint"
        >
          {{ t("清除") }}
        </button>
      </div>
      <p v-if="pointMessage" class="state-message" :class="pointState">{{ pointMessage }}</p>
    </template>

    <template v-else-if="strategy === 'auto_composer'">
      <p class="hint">
        {{ t("打开 APP 后由 Windows 自动寻找可编辑的输入框；找不到时再改用学习或点击位置。") }}
      </p>
    </template>

    <template v-else-if="strategy === 'open_only'">
      <p class="hint">{{ t("只打开 APP，不移动输入焦点。") }}</p>
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
.focus-option:hover:not(:disabled):not(.selected) {
  border-color: var(--accent-border);
}
.focus-option.selected {
  border-color: color-mix(in srgb, var(--accent) 50%, transparent);
  background: color-mix(in srgb, var(--accent) 12%, transparent);
  color: var(--accent);
  font-weight: 600;
}
.focus-option.selected:hover:not(:disabled) {
  border-color: color-mix(in srgb, var(--accent) 50%, transparent);
}
.option-label {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.option-label small {
  color: var(--muted);
  font-size: 10px;
  font-weight: 400;
}
.focus-option.selected .option-label small {
  color: color-mix(in srgb, var(--accent) 70%, var(--muted));
}
.radio-icon {
  width: 15px;
  height: 15px;
  flex: 0 0 15px;
  color: var(--text-secondary);
}
.focus-option.selected .radio-icon {
  color: var(--accent);
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
  cursor: default;
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
.focus-footer {
  display: flex;
  justify-content: flex-end;
  margin-top: 10px;
}
</style>
