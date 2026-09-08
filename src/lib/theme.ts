import { readonly, ref } from "vue";
import {
  getThemePreference,
  isTauriRuntime,
  reportThemeResult,
  saveThemePreference,
  type ThemePreference,
} from "./bridge";

export type EffectiveTheme = "light" | "dark";

const preference = ref<ThemePreference>("system");
const effectiveTheme = ref<EffectiveTheme>("light");
const busy = ref(false);
const errorMessage = ref("");

let systemTheme: EffectiveTheme = "light";
let initialized = false;
let mediaQuery: MediaQueryList | null = null;
let removeMediaListener: (() => void) | null = null;
let removeTauriListener: (() => void) | null = null;
const preferenceCacheKey = "sayall-theme-preference";
let operationSequence = 0;

function nextOperationId(): string {
  operationSequence += 1;
  return `theme-${Date.now()}-${operationSequence}`;
}

async function recordTerminalResult(
  operationId: string,
  action: "initialize" | "change",
  terminalResult: "passed" | "failed",
  reason:
    | "applied"
    | "preference_load_failed"
    | "native_apply_failed"
    | "apply_or_save_failed",
  startedAt: number,
): Promise<void> {
  try {
    await reportThemeResult({
      operationId,
      action,
      preference: preference.value,
      resolvedTheme: effectiveTheme.value,
      terminalResult,
      reason,
      elapsedMs: Math.max(0, Math.round(performance.now() - startedAt)),
    });
  } catch {
    console.warn("feature=theme event=diagnostic_report result=error reason=ipc_failed");
  }
}

function cachedPreference(): ThemePreference | null {
  try {
    const value = localStorage.getItem(preferenceCacheKey);
    return value === "system" || value === "light" || value === "dark" ? value : null;
  } catch {
    return null;
  }
}

function cachePreference(value: ThemePreference): void {
  try {
    localStorage.setItem(preferenceCacheKey, value);
  } catch {
    console.warn("feature=theme event=cache result=error reason=storage_unavailable");
  }
}

function mediaTheme(): EffectiveTheme {
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function resolvedTheme(value: ThemePreference): EffectiveTheme {
  return value === "system" ? systemTheme : value;
}

function applyDocumentTheme(theme: EffectiveTheme): void {
  const previous = effectiveTheme.value;
  effectiveTheme.value = theme;
  document.documentElement.dataset.theme = theme;
  if (previous !== theme) {
    console.info(
      `feature=theme event=changed previous=${previous} resolved=${theme} preference=${preference.value}`,
    );
  }
}

async function applyNativePreference(
  value: ThemePreference,
): Promise<EffectiveTheme | null> {
  if (!isTauriRuntime()) return value === "system" ? systemTheme : value;
  const { setTheme } = await import("@tauri-apps/api/app");
  await setTheme(value === "system" ? null : value);
  if (value !== "system") return value;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  return (await getCurrentWindow().theme()) ?? mediaTheme();
}

async function subscribeToSystemTheme(): Promise<void> {
  mediaQuery = window.matchMedia?.("(prefers-color-scheme: dark)") ?? null;
  if (mediaQuery) {
    const onMediaChange = (event: MediaQueryListEvent): void => {
      systemTheme = event.matches ? "dark" : "light";
      if (preference.value === "system") applyDocumentTheme(systemTheme);
    };
    mediaQuery.addEventListener("change", onMediaChange);
    removeMediaListener = () => mediaQuery?.removeEventListener("change", onMediaChange);
  }

  if (!isTauriRuntime()) return;
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const currentWindow = getCurrentWindow();
    const current = await currentWindow.theme();
    if (current) systemTheme = current;
    removeTauriListener = await currentWindow.onThemeChanged(({ payload }) => {
      if (preference.value !== "system") return;
      systemTheme = payload;
      applyDocumentTheme(systemTheme);
    });
  } catch {
    console.warn("feature=theme event=fallback reason=listener_failed");
  }
}

export async function initializeTheme(): Promise<void> {
  if (initialized) return;
  const operationId = nextOperationId();
  const startedAt = performance.now();
  initialized = true;
  busy.value = true;
  systemTheme = mediaTheme();
  preference.value = cachedPreference() ?? "system";
  applyDocumentTheme(resolvedTheme(preference.value));
  try {
    await subscribeToSystemTheme();
  } catch {
    console.warn("feature=theme event=fallback reason=listener_failed");
  }

  let loadFailed = false;
  try {
    preference.value = await getThemePreference(operationId);
    cachePreference(preference.value);
  } catch {
    loadFailed = true;
    errorMessage.value = "无法读取外观设置，已暂时使用上次的选择。";
    console.warn("feature=theme event=fallback reason=preference_load_failed");
  }

  let nativeApplyFailed = false;
  try {
    const nativeTheme = await applyNativePreference(preference.value);
    if (preference.value === "system" && nativeTheme) systemTheme = nativeTheme;
  } catch {
    nativeApplyFailed = true;
    console.warn("feature=theme event=fallback reason=native_apply_failed");
  }
  applyDocumentTheme(resolvedTheme(preference.value));
  console.info(
    `feature=theme event=initialized preference=${preference.value} resolved=${effectiveTheme.value}`,
  );
  await recordTerminalResult(
    operationId,
    "initialize",
    loadFailed || nativeApplyFailed ? "failed" : "passed",
    loadFailed
      ? "preference_load_failed"
      : nativeApplyFailed
        ? "native_apply_failed"
        : "applied",
    startedAt,
  );
  busy.value = false;
}

export async function setThemePreference(value: ThemePreference): Promise<void> {
  if (busy.value || value === preference.value) return;
  const operationId = nextOperationId();
  const startedAt = performance.now();
  const previousPreference = preference.value;
  const previousTheme = effectiveTheme.value;
  busy.value = true;
  errorMessage.value = "";
  let saved = false;

  try {
    await saveThemePreference(value, operationId);
    saved = true;
    const nativeTheme = await applyNativePreference(value);
    if (value === "system" && nativeTheme) systemTheme = nativeTheme;
    preference.value = value;
    cachePreference(value);
    applyDocumentTheme(resolvedTheme(value));
    console.info(`feature=theme event=preference_changed preference=${value} result=ok`);
    await recordTerminalResult(operationId, "change", "passed", "applied", startedAt);
  } catch {
    if (saved) {
      try {
        await saveThemePreference(previousPreference, operationId);
      } catch {
        console.warn("feature=theme event=rollback result=error reason=settings_save_failed");
      }
    }
    try {
      await applyNativePreference(previousPreference);
    } catch {
      console.warn("feature=theme event=native_rollback result=error reason=native_apply_failed");
    }
    preference.value = previousPreference;
    cachePreference(previousPreference);
    applyDocumentTheme(previousTheme);
    errorMessage.value = "外观设置保存失败，请稍后重试。";
    console.warn(
      `feature=theme event=preference_changed preference=${value} result=error reason=apply_or_save_failed`,
    );
    await recordTerminalResult(
      operationId,
      "change",
      "failed",
      "apply_or_save_failed",
      startedAt,
    );
  } finally {
    busy.value = false;
  }
}

export function useTheme() {
  return {
    preference: readonly(preference),
    effectiveTheme: readonly(effectiveTheme),
    busy: readonly(busy),
    errorMessage: readonly(errorMessage),
    setThemePreference,
  };
}

export function disposeTheme(): void {
  removeMediaListener?.();
  removeTauriListener?.();
  removeMediaListener = null;
  removeTauriListener = null;
  mediaQuery = null;
  initialized = false;
}
