<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import Sidebar from "./components/Sidebar.vue";
import { getOnboardingState, getRuntimeSnapshot, hideMainWindow, type RuntimeSnapshot } from "./lib/bridge";
import { reportFrontendEvent } from "./lib/frontend-diagnostics";
import { useAppUpdate } from "./lib/app-update";
import { ONBOARDING_WIZARD_ENABLED } from "./lib/feature-flags";
import {
  detectReloadRecovery,
  isBrowserReloadAccelerator,
  isWindowCloseAccelerator,
  loadPersistedPage,
  persistActivePage,
  touchLiveness,
  type PageId,
} from "./navigation";
import SettingsPage from "./pages/SettingsPage.vue";
import ButtonsPage from "./pages/ButtonsPage.vue";
import ConnectionPage from "./pages/ConnectionPage.vue";
import OnboardingPage from "./pages/OnboardingPage.vue";
import PermissionsPage from "./pages/PermissionsPage.vue";

const activePage = ref<PageId>(loadPersistedPage() ?? "buttons");
watch(activePage, (page) => persistActivePage(page));
const runtime = ref<RuntimeSnapshot | null>(null);
const loadError = ref("");
const { bannerVisible, info: updateInfo, dismissBanner, runStartupSilentCheck } = useAppUpdate();
let runtimePollTimer: ReturnType<typeof setInterval> | undefined;
let updateCheckTimer: ReturnType<typeof setTimeout> | undefined;
let initialRuntimeReported = false;

/** 未完成首次设置时进入向导（激活开关见 ONBOARDING_WIZARD_ENABLED）。 */
const showOnboarding = ref(false);

function onOnboardingCompleted(): void {
  showOnboarding.value = false;
  // 完成后落到「连接」页：先看到四项就绪状态，再开始日常使用。
  // 完成事件由向导壳在提交成功处上报（带耗时），这里不重复上报。
  activePage.value = "connection";
}

function handleWindowKeydown(event: KeyboardEvent): void {
  if (isWindowCloseAccelerator(event)) {
    if (event.repeat) return;
    // Ctrl+W 在 WebView2 里是否默认被当浏览器加速键取决于宿主配置，这里一律
    // 拦下：窗口已在 IPC 返回前隐藏，扩散出去只是把组合键喂给页面里的输入框。
    // 系统按键重复率下的连发也一并吞掉（见上面的 `repeat` 提前返回）——
    // 否则窗口重新显示时会收到一串本轮按住产生的关闭请求。
    event.preventDefault();
    event.stopPropagation();
    // 隐藏失败只可能是窗口已销毁/不存在，用户侧表现为"没反应"，不值得弹错误；
    // Rust 侧已落 window_close 日志可直接定位。
    void hideMainWindow().catch(() => undefined);
    return;
  }
  if (isBrowserReloadAccelerator(event)) {
    event.preventDefault();
    event.stopPropagation();
  }
}

const activeComponent = computed(() => ({
  buttons: ButtonsPage,
  connection: ConnectionPage,
  permissions: PermissionsPage,
  settings: SettingsPage,
})[activePage.value]);

// 横幅不在"设置"页重复显示（页面内已有完整更新面板）。
const updateBannerVisible = computed(
  () => bannerVisible.value && activePage.value !== "settings",
);

function showUpdatePage(): void {
  activePage.value = "settings";
}

onMounted(async () => {
  const refreshRuntime = async () => {
    try {
      runtime.value = await getRuntimeSnapshot();
      loadError.value = "";
      touchLiveness();
      if (!initialRuntimeReported) {
        reportFrontendEvent({
          event: "runtime_snapshot",
          phase: "completed",
          result: "passed",
          reason: "initial_ipc_ready",
        });
        initialRuntimeReported = true;
      }
    } catch (error) {
      loadError.value = error instanceof Error ? error.message : String(error);
      if (!initialRuntimeReported) {
        reportFrontendEvent({
          event: "runtime_snapshot",
          phase: "completed",
          result: "failed",
          reason: "initial_ipc_failed",
        });
        initialRuntimeReported = true;
      }
    }
  };
  // 心跳在前次会话仍新鲜 = 本次挂载是渲染进程崩溃后的重载恢复
  // （Bugs/2026-09-12）；上报后日志可区分冷启动与重载。
  if (detectReloadRecovery()) {
    reportFrontendEvent({
      event: "webview_reload_recovery",
      phase: "completed",
      result: "passed",
      reason: "fresh_liveness_heartbeat",
    });
  }
  window.addEventListener("keydown", handleWindowKeydown, true);
  // 首次使用向导：读取向导状态决定是否进入向导模式；读取失败不阻断启动
  // （Rust 侧已记录失败原因，用户仍可用主界面）。
  if (ONBOARDING_WIZARD_ENABLED) {
    try {
      const onboardingState = await getOnboardingState();
      showOnboarding.value = onboardingState.isActive;
    } catch {
      reportFrontendEvent({
        event: "onboarding",
        phase: "state_read",
        result: "failed",
        reason: "ipc_failed",
      });
    }
  }
  await refreshRuntime();
  runtimePollTimer = setInterval(() => {
    void refreshRuntime();
  }, 1_000);
  // 启动静默检查更新：延迟 3 秒避开 BLE 恢复/设置加载的启动高峰；
  // 失败完全无声（app-update.ts 内回落 idle，不打扰主功能）。
  updateCheckTimer = setTimeout(() => {
    void runStartupSilentCheck();
  }, 3_000);
});

onUnmounted(() => {
  if (runtimePollTimer) clearInterval(runtimePollTimer);
  if (updateCheckTimer) clearTimeout(updateCheckTimer);
  window.removeEventListener("keydown", handleWindowKeydown, true);
});
</script>

<template>
  <OnboardingPage
    v-if="showOnboarding"
    :runtime="runtime"
    @completed="onOnboardingCompleted"
  />
  <div v-else class="app-shell">
    <Sidebar :active-page="activePage" :version="runtime?.appVersion" @select="activePage = $event" />
    <main class="content">
      <div v-if="loadError" class="error-banner">无法读取运行状态：{{ loadError }}</div>
      <div v-if="updateBannerVisible" class="update-banner">
        <span>发现新版本 {{ updateInfo?.version }}</span>
        <button type="button" class="link-button" @click="showUpdatePage">查看</button>
        <button type="button" class="link-button" aria-label="忽略此提醒" @click="dismissBanner">
          ×
        </button>
      </div>
      <component :is="activeComponent" :runtime="runtime" />
    </main>
  </div>
</template>
