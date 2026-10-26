<script setup lang="ts">
import type { Component } from "vue";
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import Sidebar from "./components/Sidebar.vue";
import UpdateCompletedDialog from "./components/UpdateCompletedDialog.vue";
import {
  APP_ICON_CHANGED_EVENT,
  getAppIcon,
  getMembershipAccountSummary,
  getOnboardingState,
  getRuntimeSnapshot,
  hideMainWindow,
  takeUpdateCompletedVersion,
  type AppIconIdentifier,
  type RuntimeSnapshot,
} from "./lib/bridge";
import { profileAccountLabel } from "./lib/profile";
import { reportFrontendEvent } from "./lib/frontend-diagnostics";
import { useAppUpdate } from "./lib/app-update";
import { ONBOARDING_WIZARD_ENABLED } from "./lib/feature-flags";
import { t } from "./lib/i18n";
import { privatePageComponents } from "./private/feature-pages";
import {
  detectReloadRecovery,
  isBrowserReloadAccelerator,
  isWindowCloseAccelerator,
  loadPersistedPage,
  persistActivePage,
  touchLiveness,
} from "./navigation";
import SettingsPage from "./pages/SettingsPage.vue";
import ButtonsPage from "./pages/ButtonsPage.vue";
import ConnectionPage from "./pages/ConnectionPage.vue";
import MacrosPage from "./pages/MacrosPage.vue";
import OnboardingPage from "./pages/OnboardingPage.vue";
import PermissionsPage from "./pages/PermissionsPage.vue";
import RecommendationsPage from "./pages/RecommendationsPage.vue";
import ReflectionsPage from "./pages/ReflectionsPage.vue";
import ProfilePage from "./pages/ProfilePage.vue";

const activePage = ref<string>(loadPersistedPage() ?? "buttons");
watch(activePage, (page) => {
  persistActivePage(page);
  void refreshAppIcon();
  if (page === "profile") void refreshProfileLabel();
});
const runtime = ref<RuntimeSnapshot | null>(null);
const loadError = ref("");
const { bannerVisible, info: updateInfo, dismissBanner, runStartupSilentCheck } = useAppUpdate();
let runtimePollTimer: ReturnType<typeof setInterval> | undefined;
let updateCheckTimer: ReturnType<typeof setTimeout> | undefined;
let profileLabelTimer: ReturnType<typeof setInterval> | undefined;
let initialRuntimeReported = false;

/** 侧栏个人入口：未登录「个人中心」，已登录只显示邮箱 @ 前最多 10 个字符。 */
const profileLabel = ref("个人中心");
/** 与托盘图标同源；设置页切换后由事件和 5 秒轮询同步到侧栏。 */
const appIcon = ref<AppIconIdentifier>("faceted-duck");

async function refreshProfileLabel(): Promise<void> {
  try {
    const summary = await getMembershipAccountSummary();
    profileLabel.value =
      summary.availability === "available"
        ? profileAccountLabel(summary.accountDisplayName)
        : "个人中心";
  } catch {
    // 会员摘要读取失败不打扰主界面；保持上一次文案。
  }
}

async function refreshAppIcon(): Promise<void> {
  try {
    appIcon.value = await getAppIcon();
  } catch {
    // 读取失败保持上一次图标；托盘侧不受影响。
  }
}

function onAppIconChanged(event: Event): void {
  const identifier = (event as CustomEvent<AppIconIdentifier>).detail;
  if (identifier === "standard" || identifier === "faceted-duck") {
    appIcon.value = identifier;
  }
}

/** 未完成首次设置时进入向导（激活开关见 ONBOARDING_WIZARD_ENABLED）。 */
const showOnboarding = ref(false);

/**
 * 「无线麦已更新」（2026-10-19，对齐 Mac `showUpdateCompletedAlert`）：
 * 本次启动前刚完成更新时，版本号由 Rust 取走（取走即清空）并在这里提示一次。
 * 读取失败不打扰用户——更新提示不是功能路径。
 */
const updateCompletedVersion = ref<string | null>(null);

/**
 * 运行时仿真冒烟（构建期 `VITE_SAYALL_RUNTIME_SIMULATION=1`）跑的是**主界面**旅程
 * （`src/runtime-simulation.ts`：连接 / 按键 / 设置等页面 + IPC），不是首启体验。
 * 它用全新的隔离状态目录启动，状态判定必然走向导；向导一渲染，旅程等待的导航与
 * 页面就不存在（2026-10-08 CI 实测：`等待 导航"连接" 超时`）。因此该构建下跳过
 * 向导门禁。真机首启与硬件信号走查（`Testing/run-hardware-signal-script.ps1`，不带
 * 这个构建标志）不受影响，仍按状态进入向导。
 */
const runtimeSimulationBuild = import.meta.env.VITE_SAYALL_RUNTIME_SIMULATION === "1";

function onOnboardingCompleted(): void {
  showOnboarding.value = false;
  // 完成后落到「连接」页：先看到四项就绪状态，再开始日常使用。
  // 完成事件由向导壳在提交成功处上报（带耗时），这里不重复上报。
  activePage.value = "connection";
}

/** 设置页「重新运行向导」：向导状态已由设置页重置为进行中，这里切回向导。 */
function onRestartOnboarding(): void {
  showOnboarding.value = true;
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

/** 公开页面 + 私有页面（通常为空）的组件表。私有页面缺席时保持原行为。 */
const pageComponents: Record<string, Component> = {
  buttons: ButtonsPage,
  recommendations: RecommendationsPage,
  macros: MacrosPage,
  connection: ConnectionPage,
  reflections: ReflectionsPage,
  permissions: PermissionsPage,
  settings: SettingsPage,
  profile: ProfilePage,
  ...privatePageComponents(),
};

const activeComponent = computed(() => pageComponents[activePage.value] ?? ButtonsPage);

// 横幅不在"设置"页重复显示（页面内已有完整更新面板）；推荐页由原生子 WebView
// 覆盖内容区，横幅即使渲染也看不到，直接跳过。
const updateBannerVisible = computed(
  () =>
    bannerVisible.value &&
    activePage.value !== "settings" &&
    activePage.value !== "recommendations",
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
      // 原始 IPC/Rust 错误只进控制台；界面给固定文案（英文模式可翻译）。
      console.error(
        `runtime snapshot failed: ${error instanceof Error ? error.message : String(error)}`,
      );
      loadError.value = "无法读取运行状态，请稍后重试。";
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
  window.addEventListener(APP_ICON_CHANGED_EVENT, onAppIconChanged);
  // 首次使用向导：读取向导状态决定是否进入向导模式；读取失败不阻断启动
  // （Rust 侧已记录失败原因，用户仍可用主界面）。仿真冒烟构建跳过（见上方说明）。
  if (ONBOARDING_WIZARD_ENABLED && !runtimeSimulationBuild) {
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
  // 更新完成提示（对齐 Mac `showUpdateCompletedAlert`）：只在「本次启动前刚完成更新」
  // 时出现一次。Mac 在窗口显示后立即弹（向导未完成时同样弹），这里也照做；
  // 版本号由 Rust 取走即清空，所以重载页面不会重复弹。
  if (!runtimeSimulationBuild) {
    void takeUpdateCompletedVersion()
      .then((version) => {
        if (version) updateCompletedVersion.value = version;
      })
      .catch(() => undefined);
  }
  await refreshRuntime();
  void refreshProfileLabel();
  void refreshAppIcon();
  runtimePollTimer = setInterval(() => {
    void refreshRuntime();
  }, 1_000);
  profileLabelTimer = setInterval(() => {
    void refreshProfileLabel();
    void refreshAppIcon();
  }, 5_000);
  // 启动静默检查更新：延迟 3 秒避开 BLE 恢复/设置加载的启动高峰；
  // 失败完全无声（app-update.ts 内回落 idle，不打扰主功能）。
  updateCheckTimer = setTimeout(() => {
    void runStartupSilentCheck();
  }, 3_000);
});

onUnmounted(() => {
  if (runtimePollTimer) clearInterval(runtimePollTimer);
  if (updateCheckTimer) clearTimeout(updateCheckTimer);
  if (profileLabelTimer) clearInterval(profileLabelTimer);
  window.removeEventListener("keydown", handleWindowKeydown, true);
  window.removeEventListener(APP_ICON_CHANGED_EVENT, onAppIconChanged);
});
</script>

<template>
  <OnboardingPage
    v-if="showOnboarding"
    :runtime="runtime"
    @completed="onOnboardingCompleted"
  />
  <div v-else class="app-shell">
    <Sidebar
      :active-page="activePage"
      :profile-label="profileLabel"
      :app-icon="appIcon"
      @select="activePage = $event"
    />
    <main class="content">
      <div v-if="loadError" class="error-banner">{{ t(loadError) }}</div>
      <div v-if="updateBannerVisible" class="update-banner">
        <span>{{ t("发现新版本 {version}", { version: updateInfo?.version ?? "" }) }}</span>
        <button type="button" class="link-button" @click="showUpdatePage">{{ t("查看") }}</button>
        <button type="button" class="link-button" :aria-label="t('忽略此提醒')" @click="dismissBanner">
          ×
        </button>
      </div>
      <component
        :is="activeComponent"
        :runtime="runtime"
        @restart-onboarding="onRestartOnboarding"
        @navigate="activePage = $event"
      />
    </main>
  </div>
  <UpdateCompletedDialog
    v-if="updateCompletedVersion"
    :version="updateCompletedVersion"
    @close="updateCompletedVersion = null"
  />
</template>
