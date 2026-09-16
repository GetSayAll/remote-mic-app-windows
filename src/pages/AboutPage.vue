<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import type { RuntimeSnapshot, ThemePreference } from "../lib/bridge";
import {
  formatDiagnosticReport,
  getDiagnosticReport,
  getLaunchAtLogin,
  openLogDirectory,
  setLaunchAtLogin,
} from "../lib/bridge";
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
const launchAtLogin = ref(false);
/**
 * 登录自启动的初始值来自异步 IPC。就绪前用同尺寸占位符顶位，就绪后才创建开关
 * 本体——元素"创建即带正确 checked"，从不存在属性变更，因此不会有关→开的滑动
 * 过渡（对照：检查预览版开关是模块级单例、创建时即为终值，故无此动画）。
 * 只禁用过渡（no-anim / nextTick / rAF）都不行：Vue 的 DOM 更新与 nextTick 都在
 * 同一批微任务内、早于浏览器绘制，浏览器看不到中间帧，过渡照常触发。
 */
const launchAtLoginReady = ref(false);
const launchAtLoginBusy = ref(false);
const launchAtLoginError = ref("");

/**
 * 诊断摘要与日志目录（2026-09-16 从权限页迁到关于页）。
 *
 * 为什么在关于页：诊断摘要是"给开发者看本机运行状态"的支持入口，跟着版本、
 * 启动行为、更新一起属于应用级信息；权限页只回答"蓝牙/按键/音频有没有权限"，
 * 两者混在一页会让用户以为生成摘要需要额外授权。
 */
const diagnosticText = ref("");
const diagnosticMessage = ref("尚未生成诊断摘要");
const generatingDiagnostic = ref(false);
const logDirectoryBusy = ref(false);
const logDirectoryMessage = ref("");

async function generateDiagnostic(): Promise<void> {
  generatingDiagnostic.value = true;
  diagnosticMessage.value = "正在读取当前运行状态…";
  try {
    diagnosticText.value = formatDiagnosticReport(await getDiagnosticReport());
    diagnosticMessage.value = "诊断摘要已生成；复制前可在页面内检查全部内容";
  } catch (error) {
    diagnosticText.value = "";
    diagnosticMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    generatingDiagnostic.value = false;
  }
}

async function copyDiagnostic(): Promise<void> {
  if (!diagnosticText.value) await generateDiagnostic();
  if (!diagnosticText.value) return;
  try {
    if (!navigator.clipboard?.writeText) throw new Error("当前环境不支持剪贴板写入");
    await navigator.clipboard.writeText(diagnosticText.value);
    diagnosticMessage.value = "诊断摘要已复制到剪贴板";
  } catch (error) {
    diagnosticMessage.value = error instanceof Error ? error.message : String(error);
  }
}

async function onOpenLogDirectory(): Promise<void> {
  logDirectoryBusy.value = true;
  logDirectoryMessage.value = "正在打开日志目录…";
  try {
    const directory = await openLogDirectory();
    logDirectoryMessage.value = `已打开日志目录：${directory}`;
  } catch (error) {
    logDirectoryMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    logDirectoryBusy.value = false;
  }
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

onMounted(() => {
  void loadUpdatePreferences();
  void getLaunchAtLogin()
    .then((enabled) => { launchAtLogin.value = enabled; })
    .catch((error) => { launchAtLoginError.value = error instanceof Error ? error.message : String(error); })
    // 就绪标志与初值在同一渲染批次生效：开关此时才被创建，创建即带正确
    // checked，不产生属性变更，故无过渡可触发（无需禁用过渡或等待绘制）。
    .finally(() => { launchAtLoginReady.value = true; });
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
      <h2>启动行为</h2>
      <p class="muted">登录 Windows 后自动启动无线麦 SayAll。</p>
      <label class="toggle-row" title="使用当前用户的 Windows 登录启动项，不需要管理员权限。">
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
        登录时自动启动
      </label>
      <p v-if="launchAtLoginError" class="error-text" role="alert">{{ launchAtLoginError }}</p>
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
    <article class="card diagnostics-card">
      <div class="card-title-row">
        <div>
          <h2>诊断摘要</h2>
          <p class="muted">摘要不含设备地址、语音内容等隐私信息，可放心复制发给开发者排查问题。</p>
        </div>
        <div class="button-row">
          <button
            class="secondary-button"
            type="button"
            :disabled="generatingDiagnostic"
            @click="generateDiagnostic"
          >
            {{ generatingDiagnostic ? "生成中…" : "生成摘要" }}
          </button>
          <button
            class="primary-button"
            type="button"
            :disabled="generatingDiagnostic"
            @click="copyDiagnostic"
          >
            复制摘要
          </button>
        </div>
      </div>
      <p class="operation-message" aria-live="polite">{{ diagnosticMessage }}</p>
      <pre v-if="diagnosticText" class="diagnostic-output">{{ diagnosticText }}</pre>

      <div class="log-directory-block">
        <button
          class="secondary-button"
          type="button"
          :disabled="logDirectoryBusy"
          @click="onOpenLogDirectory"
        >
          {{ logDirectoryBusy ? "正在打开…" : "打开日志目录" }}
        </button>
        <p class="muted">日志记录本机运行细节，遇到问题时连同摘要一起发给开发者。</p>
      </div>
      <p class="operation-message log-directory-message" aria-live="polite">
        {{ logDirectoryMessage }}
      </p>
    </article>
  </section>
</template>

<style scoped>
/* 初值就绪前用同尺寸占位符顶位，避免开关出现时布局跳动；开关本体仅在
   终值就绪后创建，创建即带正确 checked，不产生关→开滑动过渡。 */
.toggle-placeholder { width: 34px; height: 20px; flex: none; }
/* 日志目录入口与上面的摘要动作分组：摘要随时可生成，日志目录是"已经出问题、
   要取证"时才走的路，靠分隔线和一行说明避免被当成同一组按钮。 */
.log-directory-block {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
  margin-top: 14px;
  padding-top: 12px;
  border-top: 1px solid var(--border);
}
.log-directory-block .muted { margin: 0; }
</style>
