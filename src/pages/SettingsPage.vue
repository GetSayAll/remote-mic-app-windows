<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import type {
  AppIconIdentifier,
  AppLanguagePreference,
  RuntimeSnapshot,
  ThemePreference,
} from "../lib/bridge";
import {
  getAppIcon,
  getLaunchAtLogin,
  getMembershipAccountSummary,
  getOpenWindowAtLaunch,
  openGitHubRepository,
  openOfficialWebsite,
  restartOnboarding,
  setAppIcon,
  setLaunchAtLogin,
  setOpenWindowAtLaunch,
} from "../lib/bridge";
import type { MembershipAccountSummary } from "../lib/bridge";
import CrownIcon from "../components/CrownIcon.vue";
import MembershipRequiredButton from "../components/MembershipRequiredButton.vue";
import { appUpdateProgressText, useAppUpdate } from "../lib/app-update";
import { useTheme } from "../lib/theme";
import { useLanguage } from "../lib/language";
import { effectiveLanguage } from "../lib/language-state";
import { t, tPage } from "../lib/i18n";

const props = defineProps<{ runtime: RuntimeSnapshot | null }>();
const emit = defineEmits<{ "restart-onboarding": []; navigate: [page: string] }>();

const {
  phase,
  info,
  errorMessage,
  progress,
  includePrereleases,
  preferenceBusy,
  preferenceError,
  check,
  install,
  loadUpdatePreferences,
  setIncludePrereleases,
} = useAppUpdate();
const {
  preference: themePreference,
  busy: themeBusy,
  errorMessage: themeError,
  setThemePreference,
} = useTheme();
const {
  preference: applicationLanguage,
  busy: languageBusy,
  errorMessage: languageError,
  setLanguagePreference,
} = useLanguage();

const themeOptions: Array<{ value: ThemePreference; label: string }> = [
  { value: "system", label: "系统" },
  { value: "light", label: "浅色" },
  { value: "dark", label: "深色" },
];

const languageOptions: Array<{ value: AppLanguagePreference; label: string }> = [
  { value: "system", label: "跟随系统" },
  { value: "zh-Hans", label: "简体中文" },
  { value: "en", label: "English" },
];

const appIconOptions: Array<{ value: AppIconIdentifier; label: string; preview: string }> = [
  { value: "standard", label: "默认", preview: "/app-logo.png" },
  { value: "faceted-duck", label: "几何鸭", preview: "/app-icon-faceted-duck.png" },
];

/** 版本号与安装包/更新器同源（package_info）；尚未读到时不编造版本。 */
const version = computed(() => props.runtime?.appVersion ?? "—");

const membershipSummary = ref<MembershipAccountSummary | null>(null);
const membershipAvailable = computed(
  () => membershipSummary.value?.availability === "available",
);
const membershipAccountText = computed(() => {
  const summary = membershipSummary.value;
  if (!summary) return "";
  if (summary.accountDisplayName) return summary.accountDisplayName;
  return t(summary.accountSignedIn ? "已登录账号" : "未登录");
});
const membershipStatusText = computed(() => {
  const summary = membershipSummary.value;
  if (!summary) return "";
  if (summary.plusActive) {
    return summary.syncState === "current"
      ? t("Plus 已开通，权益已同步。")
      : t("Plus 已开通，正在使用离线租约。");
  }
  return summary.accountSignedIn
    ? t("当前账号未开通 Plus；开通后可解锁会员功能。")
    : t("登录账号后可以开通或同步 Plus。");
});
const membershipActionLabel = computed(() => {
  const summary = membershipSummary.value;
  if (summary?.plusActive) return "管理会员";
  return summary?.accountSignedIn ? "开通 Plus 会员" : "登录并开通 Plus";
});

/**
 * 页面标题按页面语境取词：中文原文「设置」同时是首次向导的阶段名（onboarding
 * 片段译作 Set Up），i18n 以中文原文为 key，无法区分同形不同义。侧栏与
 * 本页标题分别走 `nav.*` / `page.*` 词表。
 */
const pageTitle = computed(() => tPage("设置"));

const checking = computed(() => phase.value === "checking");
const installing = computed(() => phase.value === "downloading" || phase.value === "installing");
const canCheck = computed(() => !checking.value && !installing.value);
const updateAvailable = computed(() => phase.value === "available" && info.value?.version != null);
const upToDate = computed(() => phase.value === "up-to-date");
const failed = computed(() => phase.value === "failed");
const notes = computed(() => info.value?.notes?.trim() || null);

// 更新/偏好读取失败的原始错误只进控制台：界面用固定文案，避免把 Rust/插件
// 原始错误直接暴露给用户（诊断日志仍可在 Rust 侧查到）。
watch(errorMessage, (message) => {
  if (message) console.warn(`update check failed: ${message}`);
});
watch(preferenceError, (message) => {
  if (message) console.warn(`update preferences failed: ${message}`);
});

/**
 * 更新状态行（对齐 Mac 设置页的“已是最新版本 / 正在检查”一行）：语气由状态
 * 决定，成功给绿色，失败给错误色，进行中给中性文案。
 */
const updateStatus = computed<{ tone: "success" | "error" | "pending" | "idle"; text: string }>(
  () => {
    if (phase.value === "installing") {
      return { tone: "pending", text: t("正在安装更新，应用将自动重启…") };
    }
    if (phase.value === "downloading") {
      return {
        tone: "pending",
        text: t("正在下载更新… {progress}", {
          progress: appUpdateProgressText(progress.value),
        }),
      };
    }
    if (checking.value) return { tone: "pending", text: t("正在检查更新…") };
    if (upToDate.value) return { tone: "success", text: t("已经是最新版本。") };
    if (failed.value) {
      return { tone: "error", text: t("检查更新失败，请稍后重试。") };
    }
    return { tone: "idle", text: t("手动检查是否有新版本。") };
  },
);

const launchAtLogin = ref(false);
/**
 * 登录自启动的初始值来自异步 IPC。就绪前用同尺寸占位符顶位，就绪后才创建开关
 * 本体——元素“创建即带正确 checked”，从不存在属性变更，因此不会有关→开的滑动
 * 过渡（对照：检查预览版开关是模块级单例、创建时即为终值，故无此动画）。
 * 只禁用过渡（no-anim / nextTick / rAF）都不行：Vue 的 DOM 更新与 nextTick 都在
 * 同一批微任务内、早于浏览器绘制，浏览器看不到中间帧，过渡照常触发。
 */
const launchAtLoginReady = ref(false);
const launchAtLoginBusy = ref(false);
const launchAtLoginError = ref("");

/**
 * 「启动时自动打开主面板」（2026-10-19，对齐 Mac `openMainWindowAtLaunch`）。
 * 与登录自启动同样等 IPC 读回后再创建开关本体，避免先闪一下关闭态。
 */
const openWindowAtLaunch = ref(true);
const openWindowAtLaunchReady = ref(false);
const openWindowAtLaunchBusy = ref(false);
const openWindowAtLaunchError = ref("");

/**
 * 应用图标（2026-10-02）：占位初值取当前默认（2026-10-04 起 = 几何鸭，与后端
 * `AppSettings::default()` 同源，避免 IPC 读回前先闪一下水彩鸭）；顶级标识与
 * 窗口/任务栏、托盘用同一选择。
 */
const appIcon = ref<AppIconIdentifier>("faceted-duck");
const appIconReady = ref(false);
const appIconBusy = ref(false);
const appIconError = ref("");

/** 顶部标识跟随选择实时换图，与窗口/托盘用同一个 ID。 */
const appIconPreview = computed(
  () => appIconOptions.find((option) => option.value === appIcon.value)?.preview ?? "/app-logo.png",
);

/** 首次设置：重新运行向导（只重置向导进度，不清除设备/映射/音频/其他设置）。 */
const onboardingBusy = ref(false);
const onboardingError = ref("");

async function runOnboardingWizard(): Promise<void> {
  if (onboardingBusy.value) return;
  onboardingBusy.value = true;
  onboardingError.value = "";
  try {
    const state = await restartOnboarding();
    if (!state.isActive) {
      // 重置后仍不是进行中：保守报错，不把用户放进一个空向导。
      onboardingError.value = "无法开始设置向导，请稍后重试。";
      return;
    }
    emit("restart-onboarding");
  } catch (error) {
    console.warn(`restart onboarding failed: ${error instanceof Error ? error.message : String(error)}`);
    onboardingError.value = "无法开始设置向导，请稍后重试。";
  } finally {
    onboardingBusy.value = false;
  }
}

/**
 * “问题反馈”的外部入口（2026-10-01 用户指定：官网与 GitHub 入口；2026-10-02
 * 官网地址带 `?from=win` 来源标记）。打开动作一律走 bridge：浏览器预览开新标签，
 * Tauri 运行时的 URL 白名单在 capabilities/default.json。
 */
/**
 * 外部入口（2026-10-01 用户指定：官网与 GitHub 入口；2026-10-02 官网地址带
 * `?from=win` 来源标记）。打开动作一律走 bridge：浏览器预览开新标签，Tauri
 * 运行时的 URL 白名单在 capabilities/default.json。
 *
 * 2026-10-02 用户指定：成功不再显示任何提示（“已在系统默认浏览器打开…”已去掉），
 * 只有失败就地给原因——失败文案不能省：capability 拒绝（Not allowed to open url）
 * 与系统没有默认浏览器是两类不同问题，压成"打开失败"会让现场无法归因。
 */
const linkBusy = ref(false);
const linkMessage = ref("");

async function openEntry(open: () => Promise<void>): Promise<void> {
  linkBusy.value = true;
  linkMessage.value = "";
  try {
    await open();
  } catch (error) {
    linkMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    linkBusy.value = false;
  }
}

async function onOpenOfficialWebsite(): Promise<void> {
  await openEntry(openOfficialWebsite);
}

async function onOpenGitHubRepository(): Promise<void> {
  await openEntry(openGitHubRepository);
}

async function onCheck(): Promise<void> {
  await check(true);
}

async function onInstall(): Promise<void> {
  await install();
}

async function onPreviewToggle(event: Event): Promise<void> {
  await setIncludePrereleases((event.target as HTMLInputElement).checked);
}

async function onThemeChange(event: Event): Promise<void> {
  await setThemePreference((event.target as HTMLInputElement).value as ThemePreference);
}

async function onLanguageChange(event: Event): Promise<void> {
  await setLanguagePreference(
    (event.target as HTMLInputElement).value as AppLanguagePreference,
  );
}

async function onLaunchAtLoginChange(event: Event): Promise<void> {
  const enabled = (event.target as HTMLInputElement).checked;
  launchAtLoginBusy.value = true;
  launchAtLoginError.value = "";
  try {
    launchAtLogin.value = await setLaunchAtLogin(enabled);
  } catch (error) {
    // 原始错误只进控制台；界面给固定文案，英文模式也能翻译。
    console.warn(`launch at login update failed: ${error instanceof Error ? error.message : String(error)}`);
    launchAtLoginError.value = "无法保存登录启动设置，请稍后重试。";
  } finally {
    launchAtLoginBusy.value = false;
  }
}

async function onOpenWindowAtLaunchChange(event: Event): Promise<void> {
  const input = event.target as HTMLInputElement;
  const enabled = input.checked;
  openWindowAtLaunchBusy.value = true;
  openWindowAtLaunchError.value = "";
  try {
    openWindowAtLaunch.value = await setOpenWindowAtLaunch(enabled);
  } catch (error) {
    console.warn(`open window at launch update failed: ${error instanceof Error ? error.message : String(error)}`);
    openWindowAtLaunchError.value = "无法保存启动页面设置，请稍后重试。";
    // 浏览器已经把复选框翻成用户点击的值；保存失败必须把可见状态拨回实际
    // 生效值，否则界面会显示一个并没有保存的选择。
    input.checked = openWindowAtLaunch.value;
  } finally {
    openWindowAtLaunchBusy.value = false;
  }
}

async function onAppIconChange(event: Event): Promise<void> {
  const desired = (event.target as HTMLInputElement).value as AppIconIdentifier;
  const previous = appIcon.value;
  // 乐观显示：先让界面与用户点击一致（与连接页选输入工具同款），失败再回到
  // 实际生效的图标——否则失败时单选按钮会停在"点击过但没保存"的位置。
  appIcon.value = desired;
  appIconBusy.value = true;
  appIconError.value = "";
  try {
    appIcon.value = await setAppIcon(desired);
  } catch (error) {
    console.warn(`app icon update failed: ${error instanceof Error ? error.message : String(error)}`);
    appIconError.value = "无法保存应用图标设置，请稍后重试。";
    try {
      appIcon.value = await getAppIcon();
    } catch {
      // 读回也失败时退回最后一次已知生效值，错误信息已经给出原因。
      appIcon.value = previous;
    }
  } finally {
    appIconBusy.value = false;
  }
}

onMounted(() => {
  void loadUpdatePreferences();
  void getMembershipAccountSummary()
    .then((summary) => {
      membershipSummary.value = summary;
    })
    .catch(() => {
      // 公开构建或服务不可用时，设置页不显示会员账号区。
      membershipSummary.value = null;
    });
  void getLaunchAtLogin()
    .then((enabled) => { launchAtLogin.value = enabled; })
    .catch((error) => {
      console.warn(`read launch at login failed: ${error instanceof Error ? error.message : String(error)}`);
      launchAtLoginError.value = "无法读取登录启动设置，请稍后重试。";
    })
    // 就绪标志与初值在同一渲染批次生效：开关此时才被创建，创建即带正确
    // checked，不产生属性变更，故无过渡可触发（无需禁用过渡或等待绘制）。
    .finally(() => { launchAtLoginReady.value = true; });
  void getOpenWindowAtLaunch()
    .then((enabled) => { openWindowAtLaunch.value = enabled; })
    .catch((error) => {
      console.warn(`read open window at launch failed: ${error instanceof Error ? error.message : String(error)}`);
      openWindowAtLaunchError.value = "无法读取启动页面设置，请稍后重试。";
    })
    .finally(() => { openWindowAtLaunchReady.value = true; });
  void getAppIcon()
    .then((identifier) => { appIcon.value = identifier; })
    .catch((error) => {
      console.warn(`read app icon failed: ${error instanceof Error ? error.message : String(error)}`);
      appIconError.value = "无法读取应用图标设置，请稍后重试。";
    })
    .finally(() => { appIconReady.value = true; });
});

/** 行内图标：形状对齐 Mac 设置页各行的 SF Symbols（Windows 无 SF Symbols）。 */
const ROW_ICONS: Record<string, { strokes: string[]; fills?: string[] }> = {
  appearance: {
    // SF "circle.lefthalf.filled"：圆 + 右半填充
    strokes: ["M12 3.2a8.8 8.8 0 1 0 0 17.6 8.8 8.8 0 0 0 0-17.6z"],
    fills: ["M12 3.2a8.8 8.8 0 0 1 0 17.6z"],
  },
  power: {
    // SF "power"：电源圆环 + 竖线
    strokes: ["M12 3.6v7.4", "M7.3 6.4a7 7 0 1 0 9.4 0"],
  },
  language: {
    // SF "globe"：圆 + 赤道 + 经线
    strokes: [
      "M12 3.2a8.8 8.8 0 1 0 0 17.6 8.8 8.8 0 0 0 0-17.6z",
      "M3.4 12h17.2",
      "M12 3.2c2.4 2.5 3.6 5.5 3.6 8.8s-1.2 6.3-3.6 8.8c-2.4-2.5-3.6-5.5-3.6-8.8S9.6 5.7 12 3.2z",
    ],
  },
  app_icon: {
    // SF "app.badge" 近似：应用方块 + 右下角标
    strokes: [
      "M4.6 3.4h10.4a1.6 1.6 0 0 1 1.6 1.6v10.4a1.6 1.6 0 0 1-1.6 1.6H4.6A1.6 1.6 0 0 1 3 15.4V5a1.6 1.6 0 0 1 1.6-1.6z",
      "M17.6 13.8a3.6 3.6 0 1 1 0 7.2 3.6 3.6 0 0 1 0-7.2z",
    ],
    fills: ["M17.6 15.9a1.5 1.5 0 1 1 0 3 1.5 1.5 0 0 1 0-3z"],
  },
  feedback: {
    // SF "bubble.left.and.bubble.right" 近似：单个圆角对话气泡
    strokes: [
      "M12 3.8c-4.8 0-8.7 3.1-8.7 7 0 2.3 1.3 4.3 3.3 5.6l-1 3.5 4-2.2c.8.2 1.6.3 2.4.3 4.8 0 8.7-3.1 8.7-7S16.8 3.8 12 3.8z",
    ],
  },
  onboarding: {
    // SF "arrow.triangle.2.circlepath" 近似：环形箭头（重新走一遍）
    strokes: ["M19.6 12a7.6 7.6 0 1 1-2.3-5.4", "M17.6 2.8v4h-4"],
  },
};
</script>

<template>
  <section class="settings-page">
    <header class="page-header">
      <div>
        <h1>{{ pageTitle }}</h1>
      </div>
    </header>

    <!-- 顶部模块（对齐 Mac 设置页：左边应用标识，右边版本与检查更新）。 -->
    <article class="card settings-overview">
      <div class="overview-identity">
        <img class="app-logo" :src="appIconPreview" :alt="t('无线麦 SayAll 应用图标')" />
        <h2>{{ t("无线麦 SayAll") }}</h2>
        <p class="muted">{{ t("让语音触手可及") }}</p>
      </div>

      <div class="overview-update">
        <div class="version-row">
          <span class="muted">{{ t("当前版本") }}</span>
          <strong class="version-value">{{ version }}</strong>
          <span v-if="updateAvailable" class="badge success">{{ t("可更新") }}</span>
          <label
            class="toggle-row prerelease-toggle"
            :title="t('开启后，检查更新时也会包含尚在测试中的预览版本。')"
          >
            <span>{{ t("检查预览版更新") }}</span>
            <input
              type="checkbox"
              class="toggle-input"
              :checked="includePrereleases"
              :disabled="preferenceBusy || checking || installing"
              @change="onPreviewToggle"
            />
          </label>
        </div>

        <div class="check-row">
          <button
            class="secondary-button check-button"
            type="button"
            :disabled="!canCheck"
            @click="onCheck"
          >
            <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <path
                d="M20 12a8 8 0 1 1-2.3-5.6"
                stroke="currentColor"
                stroke-width="1.9"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
              <path
                d="M20.4 3.6v4.8h-4.8"
                stroke="currentColor"
                stroke-width="1.9"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
            {{ failed ? t("重试检查") : t("检查更新…") }}
          </button>
        </div>
        <p v-if="preferenceError" class="update-error">
          {{ t("无法读取或保存更新设置，请稍后重试。") }}
        </p>

        <hr class="settings-divider" />

        <div class="update-panel" aria-live="polite">
          <template v-if="updateAvailable">
            <p class="update-status success">
              {{ t("发现新版本") }} <strong>{{ info?.version }}</strong>{{ t("（当前 {current}）", { current: info?.currentVersion ?? "" }) }}
            </p>
            <p v-if="notes" class="muted update-notes">{{ notes }}</p>
            <div class="update-actions">
              <button class="primary-button" type="button" :disabled="installing" @click="onInstall">
                {{ t("下载并安装") }}
              </button>
            </div>
          </template>
          <p v-else class="update-status" :class="updateStatus.tone">{{ updateStatus.text }}</p>

          <div
            v-if="phase === 'downloading' && progress.contentLength"
            class="update-progress"
            role="progressbar"
            :aria-valuenow="Math.min(100, (progress.downloaded / progress.contentLength) * 100)"
            aria-valuemin="0"
            aria-valuemax="100"
          >
            <div
              class="update-progress-bar"
              :style="{
                width: `${Math.min(100, (progress.downloaded / progress.contentLength) * 100)}%`,
              }"
            ></div>
          </div>
        </div>
      </div>
    </article>

    <!-- 通用（对齐 Mac 设置页的 settings.general 分组：标题 + 一张卡内多行 + 细分隔线）。 -->
    <section class="settings-section">
      <h2 class="section-title">{{ t("通用") }}</h2>
      <div class="card settings-group">
        <div class="settings-row">
          <span class="settings-row-icon" aria-hidden="true">
            <svg viewBox="0 0 24 24" fill="none">
              <path
                v-for="(path, index) in ROW_ICONS.appearance.fills"
                :key="`af${index}`"
                :d="path"
                fill="currentColor"
              />
              <path
                v-for="(path, index) in ROW_ICONS.appearance.strokes"
                :key="`as${index}`"
                :d="path"
                stroke="currentColor"
                stroke-width="1.9"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
          </span>
          <div class="settings-row-text">
            <strong>{{ t("外观") }}</strong>
          </div>
          <div class="theme-selector" role="radiogroup" :aria-label="t('显示模式')">
            <label
              v-for="option in themeOptions"
              :key="option.value"
              class="theme-option"
              :class="{ selected: themePreference === option.value }"
            >
              <input
                type="radio"
                name="theme-preference"
                :value="option.value"
                :checked="themePreference === option.value"
                :disabled="themeBusy"
                @change="onThemeChange"
              />
              <span>{{ t(option.label) }}</span>
            </label>
          </div>
        </div>
        <p v-if="themeError" class="error-text" role="alert">{{ t(themeError) }}</p>

        <div class="settings-row">
          <span class="settings-row-icon" aria-hidden="true">
            <svg viewBox="0 0 24 24" fill="none">
              <path
                v-for="(path, index) in ROW_ICONS.power.strokes"
                :key="`ps${index}`"
                :d="path"
                stroke="currentColor"
                stroke-width="1.9"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
          </span>
          <div class="settings-row-text">
            <strong>{{ t("启动行为") }}</strong>
          </div>
          <div class="launch-behavior-toggles">
            <label class="toggle-row launch-toggle" :title="t('使用当前用户的 Windows 登录启动项，不需要管理员权限。')">
              <span>{{ t("登录时自动启动") }}</span>
              <input
                v-if="launchAtLoginReady"
                type="checkbox"
                class="toggle-input"
                name="launch-at-login"
                :checked="launchAtLogin"
                :disabled="launchAtLoginBusy"
                @change="onLaunchAtLoginChange"
              />
              <span v-else class="toggle-placeholder" aria-hidden="true"></span>
            </label>
            <span class="launch-toggle-divider" aria-hidden="true"></span>
            <label
              class="toggle-row launch-toggle"
              :title="t('关闭后普通启动仅保留托盘入口；更新完成后仍会显示主面板。')"
            >
              <span>{{ t("启动时自动打开主面板") }}</span>
              <input
                v-if="openWindowAtLaunchReady"
                type="checkbox"
                class="toggle-input"
                name="open-window-at-launch"
                :checked="openWindowAtLaunch"
                :disabled="openWindowAtLaunchBusy"
                @change="onOpenWindowAtLaunchChange"
              />
              <span v-else class="toggle-placeholder" aria-hidden="true"></span>
            </label>
          </div>
        </div>
        <p v-if="launchAtLoginError" class="error-text" role="alert">
          {{ t(launchAtLoginError) }}
        </p>
        <p v-if="openWindowAtLaunchError" class="error-text" role="alert">
          {{ t(openWindowAtLaunchError) }}
        </p>

        <div class="settings-row">
          <span class="settings-row-icon" aria-hidden="true">
            <svg viewBox="0 0 24 24" fill="none">
              <path
                v-for="(path, index) in ROW_ICONS.language.strokes"
                :key="`ls${index}`"
                :d="path"
                stroke="currentColor"
                stroke-width="1.9"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
          </span>
          <div class="settings-row-text">
            <strong>{{ t("应用语言") }}</strong>
          </div>
          <div class="theme-selector" role="radiogroup" :aria-label="t('应用语言')">
            <label
              v-for="option in languageOptions"
              :key="option.value"
              class="theme-option"
              :class="{ selected: applicationLanguage === option.value }"
            >
              <input
                type="radio"
                name="application-language"
                :value="option.value"
                :checked="applicationLanguage === option.value"
                :disabled="languageBusy"
                @change="onLanguageChange"
              />
              <span>{{ t(option.label) }}</span>
            </label>
          </div>
        </div>
        <p v-if="languageError" class="error-text" role="alert">{{ t(languageError) }}</p>

        <div class="settings-row">
          <span class="settings-row-icon" aria-hidden="true">
            <svg viewBox="0 0 24 24" fill="none">
              <path
                v-for="(path, index) in ROW_ICONS.app_icon.fills"
                :key="`tf${index}`"
                :d="path"
                fill="currentColor"
              />
              <path
                v-for="(path, index) in ROW_ICONS.app_icon.strokes"
                :key="`ts${index}`"
                :d="path"
                stroke="currentColor"
                stroke-width="1.9"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
          </span>
          <div class="settings-row-text">
            <strong>{{ t("应用图标") }}</strong>
          </div>
          <div class="app-icon-selector" role="radiogroup" :aria-label="t('应用图标')">
            <label
              v-for="option in appIconOptions"
              :key="option.value"
              class="app-icon-option"
              :class="{ selected: appIcon === option.value }"
            >
              <input
                type="radio"
                name="app-icon"
                :value="option.value"
                :checked="appIcon === option.value"
                :disabled="!appIconReady || appIconBusy"
                @change="onAppIconChange"
              />
              <img :src="option.preview" alt="" aria-hidden="true" />
              <span>{{ t(option.label) }}</span>
            </label>
          </div>
        </div>
        <p v-if="appIconError" class="error-text" role="alert">{{ t(appIconError) }}</p>
      </div>
    </section>

    <!-- 会员与账号（对齐 Mac 设置页的 membership section）。 -->
    <section v-if="membershipAvailable" class="settings-section">
      <h2 class="section-title">{{ t("会员与账号") }}</h2>
      <div class="card settings-group">
        <div class="settings-row settings-membership-row">
          <span class="settings-row-icon membership-settings-icon" aria-hidden="true">
            <CrownIcon />
          </span>
          <div class="settings-row-text">
            <strong>{{ membershipAccountText }}</strong>
            <p class="muted">{{ membershipStatusText }}</p>
          </div>
          <div class="settings-membership-actions">
            <MembershipRequiredButton
              v-if="!membershipSummary?.plusActive"
              :label="membershipActionLabel"
              @open="emit('navigate', 'membership')"
            />
            <button
              v-else
              class="secondary-button"
              type="button"
              @click="emit('navigate', 'membership')"
            >
              {{ t(membershipActionLabel) }}
            </button>
          </div>
        </div>
      </div>
    </section>

    <!-- 「调整常用语」已移到按键页的二级视图（2026-10-17 用户反馈）。 -->

    <!-- 首次设置（向导重跑入口）：老用户机器已标记完成，需要入口才能重走一遍。 -->
    <section class="settings-section">
      <h2 class="section-title">{{ t("首次设置") }}</h2>
      <div class="card settings-group">
        <div class="settings-row">
          <span class="settings-row-icon" aria-hidden="true">
            <svg viewBox="0 0 24 24" fill="none">
              <path
                v-for="(path, index) in ROW_ICONS.onboarding.strokes"
                :key="`os${index}`"
                :d="path"
                stroke="currentColor"
                stroke-width="1.9"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
          </span>
          <div class="settings-row-text">
            <strong>{{ t("重新运行设置向导") }}</strong>
            <p class="muted">{{ t("重新走一遍首次设置；不会清除按键映射、设备与语音设置。") }}</p>
          </div>
          <button
            class="secondary-button"
            type="button"
            :disabled="onboardingBusy"
            @click="runOnboardingWizard"
          >
            {{ t("重新运行向导") }}
          </button>
        </div>
        <p v-if="onboardingError" class="error-text" role="alert">{{ t(onboardingError) }}</p>
      </div>
    </section>

    <!-- 问题反馈（对齐 Mac 设置页的 settingsSupportSection）。 -->
    <section class="settings-section">
      <h2 class="section-title">{{ t("问题反馈") }}</h2>
      <div class="card settings-group">
        <div class="settings-row">
          <span class="settings-row-icon" aria-hidden="true">
            <svg viewBox="0 0 24 24" fill="none">
              <path
                v-for="(path, index) in ROW_ICONS.feedback.strokes"
                :key="`fs${index}`"
                :d="path"
                stroke="currentColor"
                stroke-width="1.9"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
          </span>
          <div class="settings-row-text">
            <strong>{{ t("问题反馈") }}</strong>
            <p class="muted">{{ t("反馈无线麦 SayAll 的问题，或提出改进建议。") }}</p>
          </div>
          <div class="button-row">
            <button
              class="secondary-button"
              type="button"
              :disabled="linkBusy"
              @click="onOpenOfficialWebsite"
            >
              {{ t("官网") }}
            </button>
            <button
              class="secondary-button"
              type="button"
              :disabled="linkBusy"
              @click="onOpenGitHubRepository"
            >
              GitHub
            </button>
          </div>
        </div>
        <p v-if="linkMessage" class="operation-message link-message" aria-live="polite">
          {{ linkMessage }}
        </p>
      </div>
    </section>
  </section>
</template>

<style scoped>
/* 顶部模块：左标识 / 右版本与更新，中间一条竖分隔线（对齐 Mac 设置页顶部）。 */
.settings-overview {
  display: grid;
  grid-template-columns: minmax(200px, 260px) minmax(0, 1fr);
  gap: 24px;
  margin-bottom: 18px;
}
.overview-identity { display: flex; flex-direction: column; gap: 8px; align-items: flex-start; }
.overview-identity h2 { margin: 0; font-size: 16px; }
.overview-identity p { margin: 0; font-size: 13px; }
.overview-update {
  min-width: 0;
  padding-left: 24px;
  border-left: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.version-row { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; font-size: 13.5px; }
.version-value { font-size: 14px; font-variant-numeric: tabular-nums; }
.prerelease-toggle { margin-left: auto; }
.check-row { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; }
.check-button { display: inline-flex; align-items: center; gap: 7px; }
.check-button svg { width: 15px; height: 15px; }
.settings-divider { width: 100%; height: 1px; margin: 2px 0; border: 0; background: var(--border); }
.update-status { margin: 0; font-size: 13.5px; line-height: 1.5; }
.update-status.success { color: var(--success-text); font-weight: 600; }
.update-status.error { color: var(--error-text); }
.update-status.pending { color: var(--muted); }
.update-status.idle { color: var(--muted); }

/* 分组：标题在卡外（Mac settings.general.title），卡内每行左侧图标、右侧操作。 */
.settings-section { margin-bottom: 18px; }
.section-title { margin: 0 0 8px; font-size: 16px; }
.settings-group { padding: 4px 16px; }
.settings-row {
  display: grid;
  grid-template-columns: 36px minmax(0, 1fr) auto;
  align-items: center;
  gap: 14px;
  padding: 12px 0;
  border-bottom: 1px solid var(--border);
}
.settings-row:last-child { border-bottom: 0; }
.settings-row-icon {
  width: 36px;
  height: 36px;
  display: grid;
  place-items: center;
  border-radius: 10px;
  color: var(--accent-text);
  background: var(--accent-surface);
}
.settings-row-icon svg { width: 20px; height: 20px; }
.settings-row-text { min-width: 0; }
.settings-row-text strong { font-size: 14px; }
.settings-row-text p { margin: 3px 0 0; font-size: 13px; }
.settings-membership-actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  flex-wrap: wrap;
  gap: 8px;
}
.membership-settings-icon {
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 12%, transparent);
}
/* 行内错误与行内说明共用左缩进，读起来仍属于上一行。 */
.settings-group > .error-text { margin: 0 0 8px 50px; }

/* 启动行为：对齐 Mac `settings.general.launch_behavior` 右侧两组「文字在上、开关在下」，
   两组之间一条竖分隔线；窗口窄时允许折行，但不挤压左侧说明。 */
.launch-behavior-toggles {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-start;
  justify-content: flex-end;
  gap: 12px;
}
.launch-toggle {
  flex-direction: column;
  align-items: flex-end;
  gap: 4px;
}
.launch-toggle > span:first-child {
  font-size: 12px;
  font-weight: 500;
  line-height: 1.2;
}
.launch-toggle-divider {
  width: 1px;
  height: 34px;
  background: var(--border);
  align-self: center;
}

/* 应用图标：两个带预览图的选项（选中态用强调色描边，对齐 Mac appIconPreferenceRow）。 */
.app-icon-selector { display: flex; gap: 8px; }
.app-icon-option {
  position: relative;
  width: 96px;
  display: grid;
  justify-items: center;
  gap: 5px;
  padding: 8px 6px 7px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface-control);
  color: var(--text-secondary);
  cursor: pointer;
  font-size: 12.5px;
  font-weight: 600;
  text-align: center;
  transition: border-color 0.15s, background-color 0.15s, color 0.15s;
}
.app-icon-option:hover { border-color: var(--accent-border); }
.app-icon-option.selected { color: var(--accent-text); border-color: var(--accent); background: var(--accent-surface); }
.app-icon-option:has(input:disabled) { cursor: wait; opacity: 0.6; }
.app-icon-option input { position: absolute; width: 1px; height: 1px; opacity: 0; pointer-events: none; }
.app-icon-option img { width: 28px; height: 28px; border-radius: 7px; }
</style>
