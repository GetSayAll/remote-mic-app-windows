import { computed, ref } from "vue";
import {
  checkAppUpdate,
  getAppUpdatePreferences,
  installAppUpdate,
  setAppUpdatePreferences,
  subscribeAppUpdateProgress,
  type AppUpdateInfo,
  type AppUpdateProgress,
} from "./bridge";

/**
 * 应用内更新流程的共享状态（App.vue 启动静默检查 + AboutPage 手动操作共用）。
 * 模块级单例：整个应用只有一条更新流水线。
 *
 * 状态机：idle → checking → {up-to-date | available} → downloading → installing
 * （Windows 上安装成功后进程退出并由安装器重启，installing 为终态）；
 * 任何失败回落 failed（手动检查显示错误，静默检查完全无声——更新失败
 * 不打扰用户，符合"检查更新失败不影响主功能"边界）。
 */
export type AppUpdatePhase =
  | "idle"
  | "checking"
  | "up-to-date"
  | "available"
  | "downloading"
  | "installing"
  | "failed";

const phase = ref<AppUpdatePhase>("idle");
const info = ref<AppUpdateInfo | null>(null);
const errorMessage = ref("");
const progress = ref<AppUpdateProgress>({ downloaded: 0, contentLength: null, finished: false });
const bannerDismissed = ref(false);
const includePrereleases = ref(false);
const preferenceBusy = ref(false);
const preferenceError = ref("");
let progressUnsubscribe: (() => void) | null = null;
let autoCheckDone = false;
let preferenceLoaded = false;

/** 顶部横幅可见：仅"有更新且用户未忽略"，且当前不在关于页时由 App.vue 自行隐藏。 */
const bannerVisible = computed(
  () => phase.value === "available" && !bannerDismissed.value && info.value?.version != null,
);

async function check(manual: boolean): Promise<void> {
  if (phase.value === "checking" || phase.value === "downloading" || phase.value === "installing") {
    return;
  }
  phase.value = "checking";
  errorMessage.value = "";
  try {
    const result = await checkAppUpdate();
    info.value = result;
    if (result.available) {
      phase.value = "available";
      bannerDismissed.value = false;
    } else {
      phase.value = "up-to-date";
    }
  } catch (error) {
    info.value = null;
    // 静默检查失败完全无声（回落 idle）；手动检查展示错误供重试。
    if (manual) {
      phase.value = "failed";
      errorMessage.value = error instanceof Error ? error.message : String(error);
    } else {
      phase.value = "idle";
    }
  }
}

async function install(): Promise<void> {
  if (phase.value !== "available") {
    return;
  }
  phase.value = "downloading";
  errorMessage.value = "";
  progress.value = { downloaded: 0, contentLength: null, finished: false };
  try {
    if (!progressUnsubscribe) {
      progressUnsubscribe = await subscribeAppUpdateProgress((event) => {
        progress.value = event;
        if (event.finished) {
          phase.value = "installing";
        }
      });
    }
    await installAppUpdate();
    // Windows 上安装成功时进程在 Rust 侧退出，不会走到这里；
    // 走到这里说明安装器启动失败或行为变化。
    phase.value = "installing";
  } catch (error) {
    phase.value = "failed";
    errorMessage.value = error instanceof Error ? error.message : String(error);
  }
}

function dismissBanner(): void {
  bannerDismissed.value = true;
}

async function loadUpdatePreferences(): Promise<void> {
  if (preferenceLoaded) return;
  preferenceLoaded = true;
  preferenceBusy.value = true;
  preferenceError.value = "";
  try {
    const saved = await getAppUpdatePreferences();
    includePrereleases.value = saved.includePrereleases;
  } catch (error) {
    preferenceLoaded = false;
    preferenceError.value = error instanceof Error ? error.message : String(error);
  } finally {
    preferenceBusy.value = false;
  }
}

async function setIncludePrereleases(enabled: boolean): Promise<void> {
  const previous = includePrereleases.value;
  includePrereleases.value = enabled;
  preferenceBusy.value = true;
  preferenceError.value = "";
  try {
    const saved = await setAppUpdatePreferences(enabled);
    includePrereleases.value = saved.includePrereleases;
    info.value = null;
    bannerDismissed.value = false;
    if (phase.value !== "downloading" && phase.value !== "installing") {
      phase.value = "idle";
    }
  } catch (error) {
    includePrereleases.value = previous;
    preferenceError.value = error instanceof Error ? error.message : String(error);
  } finally {
    preferenceBusy.value = false;
  }
}

/** 启动静默检查（每次应用生命周期至多一次；失败无声）。 */
async function runStartupSilentCheck(): Promise<void> {
  if (autoCheckDone) {
    return;
  }
  autoCheckDone = true;
  await check(false);
}

/** 测试专用：重置模块级状态（vitest 模块隔离已覆盖，保留以防未来复用）。 */
export function resetAppUpdateForTests(): void {
  phase.value = "idle";
  info.value = null;
  errorMessage.value = "";
  progress.value = { downloaded: 0, contentLength: null, finished: false };
  bannerDismissed.value = false;
  includePrereleases.value = false;
  preferenceBusy.value = false;
  preferenceError.value = "";
  progressUnsubscribe?.();
  progressUnsubscribe = null;
  autoCheckDone = false;
  preferenceLoaded = false;
}

export function useAppUpdate() {
  return {
    phase,
    info,
    errorMessage,
    progress,
    bannerVisible,
    includePrereleases,
    preferenceBusy,
    preferenceError,
    check,
    install,
    dismissBanner,
    loadUpdatePreferences,
    setIncludePrereleases,
    runStartupSilentCheck,
  };
}

/** 下载进度百分比文案（如 "3.2 MB / 10.5 MB"、"10.5 MB"）。 */
export function appUpdateProgressText(progress: AppUpdateProgress): string {
  const downloadedMiB = progress.downloaded / (1024 * 1024);
  if (progress.contentLength == null || progress.contentLength <= 0) {
    return `${downloadedMiB.toFixed(1)} MB`;
  }
  const totalMiB = progress.contentLength / (1024 * 1024);
  return `${downloadedMiB.toFixed(1)} MB / ${totalMiB.toFixed(1)} MB`;
}
