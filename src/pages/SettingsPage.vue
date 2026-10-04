<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import type { AppIconIdentifier, RuntimeSnapshot, ThemePreference } from "../lib/bridge";
import {
  getAppIcon,
  getLaunchAtLogin,
  openGitHubRepository,
  openOfficialWebsite,
  setAppIcon,
  setLaunchAtLogin,
} from "../lib/bridge";
import { appUpdateProgressText, useAppUpdate } from "../lib/app-update";
import { useTheme } from "../lib/theme";

const props = defineProps<{ runtime: RuntimeSnapshot | null }>();

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

const themeOptions: Array<{ value: ThemePreference; label: string }> = [
  { value: "system", label: "系统" },
  { value: "light", label: "浅色" },
  { value: "dark", label: "深色" },
];

const appIconOptions: Array<{ value: AppIconIdentifier; label: string; preview: string }> = [
  { value: "standard", label: "默认", preview: "/app-logo.png" },
  { value: "faceted-duck", label: "几何鸭", preview: "/app-icon-faceted-duck.png" },
];

/** 版本号与安装包/更新器同源（package_info）；尚未读到时不编造版本。 */
const version = computed(() => props.runtime?.appVersion ?? "—");

const checking = computed(() => phase.value === "checking");
const installing = computed(() => phase.value === "downloading" || phase.value === "installing");
const canCheck = computed(() => !checking.value && !installing.value);
const updateAvailable = computed(() => phase.value === "available" && info.value?.version != null);
const upToDate = computed(() => phase.value === "up-to-date");
const failed = computed(() => phase.value === "failed");
const notes = computed(() => info.value?.notes?.trim() || null);

/**
 * 更新状态行（对齐 Mac 设置页的“已是最新版本 / 正在检查”一行）：语气由状态
 * 决定，成功给绿色，失败给错误色，进行中给中性文案。
 */
const updateStatus = computed<{ tone: "success" | "error" | "pending" | "idle"; text: string }>(
  () => {
    if (phase.value === "installing") {
      return { tone: "pending", text: "正在安装更新，应用将自动重启…" };
    }
    if (phase.value === "downloading") {
      return {
        tone: "pending",
        text: `正在下载更新… ${appUpdateProgressText(progress.value)}`,
      };
    }
    if (checking.value) return { tone: "pending", text: "正在检查更新…" };
    if (upToDate.value) return { tone: "success", text: "已经是最新版本。" };
    if (failed.value) {
      return { tone: "error", text: errorMessage.value || "检查更新失败，请稍后重试。" };
    }
    return { tone: "idle", text: "手动检查是否有新版本。" };
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

async function onLaunchAtLoginChange(event: Event): Promise<void> {
  const enabled = (event.target as HTMLInputElement).checked;
  launchAtLoginBusy.value = true;
  launchAtLoginError.value = "";
  try {
    launchAtLogin.value = await setLaunchAtLogin(enabled);
  } catch (error) {
    launchAtLoginError.value = error instanceof Error ? error.message : String(error);
  } finally {
    launchAtLoginBusy.value = false;
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
    appIconError.value = error instanceof Error ? error.message : String(error);
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
  void getLaunchAtLogin()
    .then((enabled) => { launchAtLogin.value = enabled; })
    .catch((error) => { launchAtLoginError.value = error instanceof Error ? error.message : String(error); })
    // 就绪标志与初值在同一渲染批次生效：开关此时才被创建，创建即带正确
    // checked，不产生属性变更，故无过渡可触发（无需禁用过渡或等待绘制）。
    .finally(() => { launchAtLoginReady.value = true; });
  void getAppIcon()
    .then((identifier) => { appIcon.value = identifier; })
    .catch((error) => { appIconError.value = error instanceof Error ? error.message : String(error); })
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
};
</script>

<template>
  <section class="settings-page">
    <header class="page-header">
      <div>
        <h1>设置</h1>
      </div>
    </header>

    <!-- 顶部模块（对齐 Mac 设置页：左边应用标识，右边版本与检查更新）。 -->
    <article class="card settings-overview">
      <div class="overview-identity">
        <img class="app-logo" :src="appIconPreview" alt="无线麦 SayAll 应用图标" />
        <h2>无线麦 SayAll</h2>
        <p class="muted">让语音触手可及</p>
      </div>

      <div class="overview-update">
        <div class="version-row">
          <span class="muted">当前版本</span>
          <strong class="version-value">{{ version }}</strong>
          <span v-if="updateAvailable" class="badge success">可更新</span>
          <label
            class="toggle-row prerelease-toggle"
            title="开启后，检查更新时也会包含尚在测试中的预览版本。"
          >
            <span>检查预览版更新</span>
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
            {{ failed ? "重试检查" : "检查更新…" }}
          </button>
        </div>
        <p v-if="preferenceError" class="update-error">{{ preferenceError }}</p>

        <hr class="settings-divider" />

        <div class="update-panel" aria-live="polite">
          <template v-if="updateAvailable">
            <p class="update-status success">
              发现新版本 <strong>{{ info?.version }}</strong>（当前 {{ info?.currentVersion }}）
            </p>
            <p v-if="notes" class="muted update-notes">{{ notes }}</p>
            <div class="update-actions">
              <button class="primary-button" type="button" :disabled="installing" @click="onInstall">
                下载并安装
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
      <h2 class="section-title">通用</h2>
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
            <strong>外观</strong>
            <p class="muted">选择应用的显示模式。</p>
          </div>
          <div class="theme-selector" role="radiogroup" aria-label="显示模式">
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
              <span>{{ option.label }}</span>
            </label>
          </div>
        </div>
        <p v-if="themeError" class="error-text" role="alert">{{ themeError }}</p>

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
            <strong>启动行为</strong>
            <p class="muted">登录 Windows 后自动启动无线麦 SayAll。</p>
          </div>
          <label class="toggle-row" title="使用当前用户的 Windows 登录启动项，不需要管理员权限。">
            <span>登录时自动启动</span>
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
        </div>
        <p v-if="launchAtLoginError" class="error-text" role="alert">{{ launchAtLoginError }}</p>

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
            <strong>应用图标</strong>
          </div>
          <div class="app-icon-selector" role="radiogroup" aria-label="应用图标">
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
              <span>{{ option.label }}</span>
            </label>
          </div>
        </div>
        <p v-if="appIconError" class="error-text" role="alert">{{ appIconError }}</p>
      </div>
    </section>

    <!-- 问题反馈（对齐 Mac 设置页的 settingsSupportSection）。 -->
    <section class="settings-section">
      <h2 class="section-title">问题反馈</h2>
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
            <strong>问题反馈</strong>
            <p class="muted">反馈无线麦 SayAll 的问题，或提出改进建议。</p>
          </div>
          <div class="button-row">
            <button
              class="secondary-button"
              type="button"
              :disabled="linkBusy"
              @click="onOpenOfficialWebsite"
            >
              官网
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
/* 行内错误与行内说明共用左缩进，读起来仍属于上一行。 */
.settings-group > .error-text { margin: 0 0 8px 50px; }

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
