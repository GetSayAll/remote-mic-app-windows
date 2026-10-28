import { readonly, ref } from "vue";
import {
  getApplicationLanguage,
  saveApplicationLanguage,
  type AppLanguagePreference,
} from "./bridge";
import {
  effectiveLanguage,
  type EffectiveLanguage,
} from "./language-state";
import { reasonToken, reportFrontendEvent } from "./frontend-diagnostics";

const preference = ref<AppLanguagePreference>("system");
const busy = ref(false);
const errorMessage = ref("");

let initialization: Promise<void> | null = null;
const preferenceCacheKey = "sayall-language-preference";
let operationSequence = 0;
/**
 * CI 的 runtime simulation 用中文选择器驱动真实 WebView；runner 的 Windows 显示语言
 * 可能是英文。仿真构建固定用中文，避免系统语言让整套旅程选择器失效。
 * 生产构建不受影响（`system` 仍按系统语言解析）。
 */
const runtimeSimulationBuild =
  import.meta.env.VITE_SAYALL_RUNTIME_SIMULATION === "1";

function nextOperationId(): string {
  operationSequence += 1;
  return `language-${Date.now()}-${operationSequence}`;
}

/** 与 Mac `LocalizationStore.resolvedLocale` 相同：只区分中文与英文。 */
export function resolveLanguage(
  value: AppLanguagePreference,
  systemLanguage: string | undefined = systemLanguageCode(),
): EffectiveLanguage {
  if (value === "zh-Hans" || value === "en") return value;
  return systemLanguage?.toLowerCase().startsWith("zh") ? "zh-Hans" : "en";
}

function systemLanguageCode(): string | undefined {
  if (typeof navigator === "undefined") return undefined;
  const candidates = [navigator.language, ...(navigator.languages ?? [])];
  return candidates.find((candidate) => candidate.length > 0);
}

function isLanguagePreference(value: string | null): value is AppLanguagePreference {
  return value === "system" || value === "zh-Hans" || value === "en";
}

function cachedPreference(): AppLanguagePreference | null {
  try {
    const value = localStorage.getItem(preferenceCacheKey);
    return isLanguagePreference(value) ? value : null;
  } catch {
    return null;
  }
}

function cachePreference(value: AppLanguagePreference): void {
  try {
    localStorage.setItem(preferenceCacheKey, value);
  } catch {
    // 缓存失败只影响首帧，原生设置仍是事实来源。
  }
}

function applyLanguage(language: EffectiveLanguage): void {
  effectiveLanguage.value = language;
  if (typeof document !== "undefined") {
    document.documentElement.lang = language;
  }
}

function effectiveLanguageFor(preference: AppLanguagePreference): EffectiveLanguage {
  return runtimeSimulationBuild ? "zh-Hans" : resolveLanguage(preference);
}

export function initializeLanguage(): Promise<void> {
  if (!initialization) {
    initialization = initializeLanguageOnce().catch(() => undefined);
  }
  return initialization;
}

async function initializeLanguageOnce(): Promise<void> {
  const operationId = nextOperationId();
  const startedAt = performance.now();
  busy.value = true;
  const cached = cachedPreference();
  if (cached) preference.value = cached;
  applyLanguage(effectiveLanguageFor(preference.value));

  let loadFailed = false;
  try {
    preference.value = await getApplicationLanguage(operationId);
    cachePreference(preference.value);
  } catch {
    loadFailed = true;
    errorMessage.value = "无法读取应用语言设置，已暂时使用上次的选择。";
  }
  applyLanguage(effectiveLanguageFor(preference.value));
  reportFrontendEvent({
    event: "language",
    phase: "initialize",
    result: loadFailed ? "failed" : "passed",
    reason: loadFailed ? "preference_load_failed" : "applied",
    detail: reasonToken("preference", preference.value, "effective", effectiveLanguage.value),
    elapsedMs: Math.max(0, Math.round(performance.now() - startedAt)),
  });
  busy.value = false;
}

export async function setLanguagePreference(value: AppLanguagePreference): Promise<void> {
  if (busy.value || value === preference.value) return;
  const operationId = nextOperationId();
  const startedAt = performance.now();
  const previousPreference = preference.value;
  const previousLanguage = effectiveLanguage.value;
  busy.value = true;
  errorMessage.value = "";

  try {
    await saveApplicationLanguage(value, operationId);
    preference.value = value;
    cachePreference(value);
    applyLanguage(effectiveLanguageFor(value));
    reportFrontendEvent({
      event: "language",
      phase: "change",
      result: "passed",
      reason: "applied",
      detail: reasonToken("preference", value, "effective", effectiveLanguage.value),
      elapsedMs: Math.max(0, Math.round(performance.now() - startedAt)),
    });
  } catch {
    try {
      await saveApplicationLanguage(previousPreference, operationId);
    } catch {
      // 回滚失败时保留原生设置原值；下次启动会以设置文件为准。
    }
    preference.value = previousPreference;
    cachePreference(previousPreference);
    applyLanguage(previousLanguage);
    errorMessage.value = "应用语言保存失败，请稍后重试。";
    reportFrontendEvent({
      event: "language",
      phase: "change",
      result: "failed",
      reason: "apply_or_save_failed",
      detail: reasonToken("preference", value),
      elapsedMs: Math.max(0, Math.round(performance.now() - startedAt)),
    });
  } finally {
    busy.value = false;
  }
}

export function useLanguage() {
  return {
    preference: readonly(preference),
    effectiveLanguage: readonly(effectiveLanguage),
    busy: readonly(busy),
    errorMessage: readonly(errorMessage),
    setLanguagePreference,
  };
}

export function disposeLanguage(): void {
  initialization = null;
}
