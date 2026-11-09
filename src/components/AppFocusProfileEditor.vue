<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import {
  activateAppForLearning,
  chordLabel,
  focusLearnErrorLabel,
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
import { effectiveLanguage } from "../lib/language-state";
import {
  CAPTURE_MODIFIER_SOURCES,
  MODIFIER_KEYS,
  codeToKeyCode,
} from "../lib/key-codes";

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
const pointState = ref<"idle" | "opening" | "recording" | "recorded" | "error">("idle");
const pointMessage = ref<string | null>(null);
let pointTimer: number | null = null;
let generation = 0;

const capturing = ref(false);
const captureStarting = ref(false);
const captureDisplay = ref<KeyCode[]>([]);
const capturedChord = ref<KeyCode[] | null>(null);
const selectedModifiers = reactive(new Set<KeyCode>());
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
const statusTone = computed(() => {
  switch (strategy.value) {
    case "recorded_element":
      return effective.value.recorded ? "success" : "pending";
    case "recorded_point":
      return effective.value.point ? "success" : "pending";
    case "app_shortcut":
      return effective.value.shortcut?.keys?.length ? "success" : "pending";
    case "auto_composer":
      return "success";
    default:
      return "neutral";
  }
});
const statusLabel = computed(() => {
  switch (strategy.value) {
    case "recorded_element":
      return effective.value.recorded ? t("已记录输入框") : t("尚未记录输入框");
    case "recorded_point":
      return effective.value.point ? t("已记录点击位置") : t("尚未记录点击位置");
    case "app_shortcut":
      return effective.value.shortcut?.keys?.length ? t("已录入快捷键") : t("尚未录入快捷键");
    case "auto_composer":
      return t("自动选择");
    default:
      return t("只打开");
  }
});
const primaryOptions = computed(() =>
  FOCUS_STRATEGY_OPTIONS.filter((option) => option.value !== "app_shortcut"),
);
const shortcut = computed(() => effective.value.shortcut);

function notice(message: string, tone: "success" | "error" = "success"): void {
  emit("notice", message, tone);
}

function clearLearnState(): void {
  // 改选策略/切换目标都会作废进行中的学习与点击位置记录，迟到结果不得覆盖新状态。
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
    if (previous.shortcut?.keys?.length) profile.shortcut = previous.shortcut;
  }
  if (next === "recorded_element" && previous.recorded) profile.recorded = previous.recorded;
  if (next === "recorded_point" && previous.point) profile.point = previous.point;
  return profile;
}

async function selectStrategy(next: FocusStrategy): Promise<void> {
  if (controlsDisabled.value || strategy.value === next) return;
  clearLearnState();
  busy.value = true;
  try {
    if (await props.save(profileForStrategy(next))) {
      notice(t("聚焦方式已更新"));
    } else {
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
      learnMessage.value = t("输入框学习成功。");
      notice(t("输入框学习成功。"));
    } else {
      learnState.value = "error";
      learnMessage.value = t("保存输入框记录失败，请重试。");
    }
  } catch (error) {
    if (requestGeneration !== generation || unmounted) return;
    const raw = error instanceof Error ? error.message : String(error);
    console.warn("learn focus target failed", raw);
    learnState.value = "error";
    learnMessage.value =
      effectiveLanguage.value === "zh-Hans"
        ? focusLearnErrorLabel(raw)
        : t("输入框学习失败，请重试。");
  }
}

async function startPointRecording(): Promise<void> {
  if (controlsDisabled.value || pointBusy.value) return;
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
      pointMessage.value = t("已记录位置（窗口大小变化超过 25% 时会拒绝点击，需要重新记录）");
      notice(t("已记录点击位置"));
    } else {
      pointState.value = "error";
      pointMessage.value = t("保存点击位置失败，请重试。");
    }
  } catch (error) {
    if (requestGeneration !== generation || unmounted) return;
    const raw = error instanceof Error ? error.message : String(error);
    console.warn("learn focus point failed", raw);
    pointState.value = "error";
    pointMessage.value =
      effectiveLanguage.value === "zh-Hans"
        ? `记录位置失败：${focusPointErrorLabel(raw)}`
        : t("记录位置失败，请重新记录。");
  }
}

async function clearRecorded(): Promise<void> {
  if (controlsDisabled.value) return;
  clearLearnState();
  const next: AppFocusProfile =
    strategy.value === "recorded_point"
      ? { strategy: "recorded_point" }
      : { strategy: "recorded_element" };
  if (await props.save(next)) notice(t("已清除记录"));
  else notice(t("清除记录失败，请重试。"), "error");
}

async function testFocus(): Promise<void> {
  if (controlsDisabled.value) return;
  try {
    await testAppFocus(props.target);
    notice(t("已发送测试请求"));
  } catch (error) {
    console.warn("test app focus failed", error);
    notice(t("测试请求发送失败，请重试。"), "error");
  }
}

async function beginShortcutCapture(): Promise<void> {
  if (controlsDisabled.value || capturing.value || captureStarting.value) return;
  captureStarting.value = true;
  const requestGeneration = captureGeneration;
  try {
    await startShortcutCapture();
    if (unmounted || requestGeneration !== captureGeneration) {
      await stopShortcutCapture().catch(() => undefined);
      return;
    }
    pressedKeys.clear();
    capturedChord.value = null;
    selectedModifiers.clear();
    pressedModifiers.clear();
    captureDisplay.value = [];
    capturing.value = true;
    if (captureTimeout !== null) window.clearTimeout(captureTimeout);
    captureTimeout = window.setTimeout(() => {
      void finishShortcutCapture(t("录入已超时，请重新录入"));
    }, 15_000);
  } catch (error) {
    console.warn("start app focus shortcut capture failed", error);
    notice(t("没能开始录入快捷键，请重试。"), "error");
  } finally {
    captureStarting.value = false;
  }
}

async function finishShortcutCapture(message?: string): Promise<void> {
  captureGeneration += 1;
  capturing.value = false;
  if (captureTimeout !== null) window.clearTimeout(captureTimeout);
  captureTimeout = null;
  await stopShortcutCapture().catch(() => undefined);
  pressedKeys.clear();
  capturedChord.value = null;
  pressedModifiers.clear();
  captureDisplay.value = [];
  selectedModifiers.clear();
  if (message) notice(message, "error");
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
  if (code === "escape" && pressedModifiers.size === 0 && selectedModifiers.size === 0) {
    void finishShortcutCapture(t("已取消录入"));
    return;
  }
  const keys = [...new Set([...selectedModifiers, ...pressedModifiers, code])];
  capturedChord.value = keys;
  captureDisplay.value = keys;
}

async function saveShortcut(keys: KeyCode[]): Promise<void> {
  const profile: AppFocusProfile = { strategy: "app_shortcut", shortcut: { keys } };
  if (await props.save(profile)) notice(t("聚焦快捷键已录入"));
  else notice(t("保存聚焦快捷键失败，请重试。"), "error");
}

async function clearShortcut(): Promise<void> {
  if (controlsDisabled.value) return;
  if (await props.save({ strategy: "app_shortcut" })) {
    notice(t("已清除聚焦快捷键"));
  } else {
    notice(t("清除聚焦快捷键失败，请重试。"), "error");
  }
}

function toggleModifier(key: KeyCode): void {
  if (!capturing.value || capturedChord.value) return;
  if (selectedModifiers.has(key)) selectedModifiers.delete(key);
  else selectedModifiers.add(key);
  captureDisplay.value = [...selectedModifiers];
}

async function backToPrimary(): Promise<void> {
  if (controlsDisabled.value) return;
  const next: FocusStrategy = effective.value.recorded
    ? "recorded_element"
    : "auto_composer";
  if (await props.save(profileForStrategy(next))) {
    notice(t("已改回主聚焦方式"));
  }
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
    void finishShortcutCapture();
  },
);

watch(capturing, (active) => {
  if (!active) {
    pressedKeys.clear();
    pressedModifiers.clear();
    selectedModifiers.clear();
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
      <span class="badge" :class="statusTone">{{ statusLabel }}</span>
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
        :disabled="controlsDisabled"
        @click="selectStrategy(option.value)"
      >
        <span class="radio" aria-hidden="true"></span>
        <span>
          {{ t(option.label) }}
          <small v-if="option.value === 'auto_composer'">{{ t("Windows 默认") }}</small>
          <small v-else-if="option.value === 'recorded_point'">{{ t("界面不暴露输入框时") }}</small>
        </span>
      </button>
    </div>

    <template v-if="strategy === 'recorded_element'">
      <p class="hint">
        {{ t("执行时会先打开 APP，再把焦点放回已记录的输入框。只记录控件特征，不读取输入内容。") }}
      </p>
      <div class="actions">
        <button class="primary" type="button" :disabled="controlsDisabled || learnBusy" @click="startFocusLearning">
          {{
            learnState === "opening"
              ? t("正在打开应用…")
              : learnState === "learning"
                ? t("等待点击输入框…")
                : effective.recorded
                  ? t("重新学习输入框")
                  : t("开始学习输入框")
          }}
        </button>
        <button class="secondary" type="button" :disabled="controlsDisabled" @click="testFocus">
          {{ t("测试打开与聚焦") }}
        </button>
        <span class="inline-status" :class="{ pending: !effective.recorded }">
          {{ effective.recorded ? t("已记录目标输入框") : t("尚未记录目标输入框") }}
        </span>
        <button v-if="effective.recorded" class="quiet" type="button" :disabled="controlsDisabled" @click="clearRecorded">
          {{ t("清除") }}
        </button>
      </div>
      <p v-if="learnMessage" class="state-message" :class="learnState">{{ learnMessage }}</p>
    </template>

    <template v-else-if="strategy === 'recorded_point'">
      <p class="hint">
        {{ t("「记录点击位置」会先打开目标应用；把鼠标停在要点击的位置上，6 秒后自动记录，之后按这个位置点击聚焦。") }}
      </p>
      <div class="actions">
        <button class="primary" type="button" :disabled="controlsDisabled || pointBusy" @click="startPointRecording">
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
        <button class="secondary" type="button" :disabled="controlsDisabled" @click="testFocus">
          {{ t("测试打开与聚焦") }}
        </button>
        <span class="inline-status" :class="{ pending: !effective.point }">
          {{ effective.point ? t("已记录点击位置") : t("尚未记录点击位置") }}
        </span>
        <button v-if="effective.point" class="quiet" type="button" :disabled="controlsDisabled" @click="clearRecorded">
          {{ t("清除") }}
        </button>
      </div>
      <p v-if="pointMessage" class="state-message" :class="pointState">{{ pointMessage }}</p>
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
        <button class="secondary" type="button" :disabled="controlsDisabled || !shortcut?.keys?.length" @click="testFocus">
          {{ t("测试打开与聚焦") }}
        </button>
      </div>
      <div v-if="capturing" class="capture-display">
        {{ captureDisplay.length ? chordLabel({ keys: captureDisplay }) : t("请按下快捷键组合") }}
      </div>
      <div v-if="capturing" class="modifier-grid">
        <button
          v-for="modifier in CAPTURE_MODIFIER_SOURCES"
          :key="modifier.key"
          class="chip"
          :class="{ selected: selectedModifiers.has(modifier.key) }"
          type="button"
          @click="toggleModifier(modifier.key)"
        >
          {{ t(modifier.label) }}
        </button>
      </div>
      <div class="fallback-row">
        <button class="quiet" type="button" :disabled="controlsDisabled" @click="backToPrimary">
          {{ t("改回主聚焦方式") }}
        </button>
      </div>
    </template>

    <template v-else>
      <p class="hint">
        {{
          strategy === "auto_composer"
            ? t("打开 APP 后由 Windows 自动寻找可编辑的输入框；找不到时再改用学习或点击位置。")
            : t("只打开 APP，不移动输入焦点。")
        }}
      </p>
      <div class="actions">
        <button class="secondary" type="button" :disabled="controlsDisabled" @click="testFocus">
          {{ t("测试打开与聚焦") }}
        </button>
      </div>
    </template>

    <div v-if="strategy !== 'app_shortcut'" class="fallback-row">
      <span>{{ t("这个 APP 有自带的聚焦快捷键？也可以用它代替学习。") }}</span>
      <button class="quiet" type="button" :disabled="controlsDisabled" @click="selectStrategy('app_shortcut')">
        {{ t("改用 APP 快捷键聚焦") }}
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
.badge {
  display: inline-flex;
  align-items: center;
  min-height: 20px;
  padding: 2px 7px;
  border-radius: 999px;
  font-size: 11px;
  font-weight: 600;
  white-space: nowrap;
}
.badge.success {
  color: var(--success-text);
  background: var(--success-surface);
}
.badge.pending {
  color: var(--warning-text);
  background: var(--warning-surface);
}
.badge.neutral {
  color: var(--text-secondary);
  background: rgba(31, 36, 48, 0.06);
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
  gap: 8px;
  padding: 6px 9px;
  border: 1px solid var(--border-strong);
  border-radius: 8px;
  background: var(--surface-raised);
  color: var(--text-primary);
  text-align: left;
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
.focus-option small {
  display: block;
  margin-top: 1px;
  color: var(--muted);
  font-size: 10px;
  font-weight: 400;
  line-height: 1.25;
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
.modifier-grid {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 7px;
}
.chip {
  min-height: 26px;
  padding: 3px 9px;
  border: 1px solid var(--border-strong);
  border-radius: 999px;
  background: var(--surface-chip);
  color: var(--text-primary);
  font-size: 11.5px;
}
.chip.selected {
  border-color: var(--accent-border);
  background: var(--accent-surface);
  color: var(--accent-text);
}
.fallback-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 7px;
  color: var(--muted);
  font-size: 11.5px;
  line-height: 1.35;
}
</style>
