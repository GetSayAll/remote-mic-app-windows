<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import Sidebar from "./components/Sidebar.vue";
import { getRuntimeSnapshot, type RuntimeSnapshot } from "./lib/bridge";
import { reportFrontendEvent } from "./lib/frontend-diagnostics";
import { useAppUpdate } from "./lib/app-update";
import type { PageId } from "./navigation";
import AboutPage from "./pages/AboutPage.vue";
import ButtonsPage from "./pages/ButtonsPage.vue";
import ConnectionPage from "./pages/ConnectionPage.vue";
import PermissionsPage from "./pages/PermissionsPage.vue";

const activePage = ref<PageId>("buttons");
const runtime = ref<RuntimeSnapshot | null>(null);
const loadError = ref("");
const { bannerVisible, info: updateInfo, dismissBanner, runStartupSilentCheck } = useAppUpdate();
let runtimePollTimer: ReturnType<typeof setInterval> | undefined;
let updateCheckTimer: ReturnType<typeof setTimeout> | undefined;
let initialRuntimeReported = false;

const activeComponent = computed(() => ({
  buttons: ButtonsPage,
  connection: ConnectionPage,
  permissions: PermissionsPage,
  about: AboutPage,
})[activePage.value]);

// 横幅不在"关于"页重复显示（页面内已有完整更新面板）。
const updateBannerVisible = computed(
  () => bannerVisible.value && activePage.value !== "about",
);

function showUpdatePage(): void {
  activePage.value = "about";
}

onMounted(async () => {
  const refreshRuntime = async () => {
    try {
      runtime.value = await getRuntimeSnapshot();
      loadError.value = "";
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
});
</script>

<template>
  <div class="app-shell">
    <Sidebar :active-page="activePage" @select="activePage = $event" />
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
