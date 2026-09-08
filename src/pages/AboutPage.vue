<script setup lang="ts">
import { computed, onMounted } from "vue";
import type { RuntimeSnapshot, ThemePreference } from "../lib/bridge";
import { appUpdateProgressText, useAppUpdate } from "../lib/app-update";
import { useTheme } from "../lib/theme";

defineProps<{ runtime: RuntimeSnapshot | null }>();

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

const checking = computed(() => phase.value === "checking");
const installing = computed(() => phase.value === "downloading" || phase.value === "installing");
const canCheck = computed(() => !checking.value && !installing.value);
const updateAvailable = computed(() => phase.value === "available" && info.value?.version != null);
const upToDate = computed(() => phase.value === "up-to-date");
const failed = computed(() => phase.value === "failed");
const notes = computed(() => info.value?.notes?.trim() || null);

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

onMounted(() => {
  void loadUpdatePreferences();
});
</script>

<template>
  <section>
    <header class="page-header">
      <div>
        <h1>关于</h1>
      </div>
    </header>

    <article class="card about-card">
      <img class="app-logo" src="/app-logo.png" alt="无线麦 SayAll 应用图标" />
      <div>
        <h2>无线麦 SayAll</h2>
        <p>版本 {{ runtime?.appVersion ?? "0.1.0" }}</p>
      </div>
    </article>

    <article class="card appearance-card">
      <h2>外观</h2>
      <p class="muted">选择应用的显示模式。</p>
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
      <p class="muted appearance-note">
        {{ themePreference === "system" ? "跟随 Windows 的应用颜色模式。" : "该选择会在重启后保持。" }}
      </p>
      <p v-if="themeError" class="error-text" role="alert">{{ themeError }}</p>
    </article>

    <article class="card">
      <h2>软件更新</h2>
      <p class="muted">更新包来自 GitHub Releases，下载后自动安装并重启应用。</p>
      <label class="toggle-row" title="开启后，检查更新时也会包含尚在测试中的预览版本。">
        <input
          type="checkbox"
          class="toggle-input"
          :checked="includePrereleases"
          :disabled="preferenceBusy || checking || installing"
          @change="onPreviewToggle"
        />
        检查预览版更新
      </label>
      <p class="muted">默认关闭。预览版包含新功能，但稳定性可能低于正式版。</p>
      <p v-if="preferenceError" class="update-error">{{ preferenceError }}</p>
      <div class="update-panel">
        <template v-if="updateAvailable">
          <p>
            发现新版本 <strong>{{ info?.version }}</strong
            >（当前 {{ info?.currentVersion }}）
          </p>
          <p v-if="notes" class="muted update-notes">{{ notes }}</p>
        </template>
        <p v-else-if="phase === 'installing'" class="muted">正在安装更新，应用将自动重启…</p>
        <p v-else-if="phase === 'downloading'" class="muted">
          正在下载更新… {{ appUpdateProgressText(progress) }}
        </p>
        <p v-else-if="checking" class="muted">正在检查更新…</p>
        <p v-else-if="upToDate" class="muted">已经是最新版本。</p>
        <p v-else-if="failed" class="update-error">{{ errorMessage }}</p>
        <p v-else class="muted">手动检查是否有新版本。</p>

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

        <div class="update-actions">
          <button v-if="updateAvailable" type="button" :disabled="installing" @click="onInstall">
            下载并安装
          </button>
          <button v-if="canCheck" type="button" :disabled="checking" @click="onCheck">
            {{ failed ? "重试检查" : "检查更新" }}
          </button>
        </div>
      </div>
    </article>
  </section>
</template>
