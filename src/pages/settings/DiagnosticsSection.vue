<script setup lang="ts">
import { ref } from "vue";
import {
  formatDiagnosticReport,
  getDiagnosticReport,
  openGitHubRepository,
  openLogDirectory,
  openOfficialWebsite,
} from "../../lib/bridge";
import { t } from "../../lib/i18n";

/**
 * 诊断与日志（2026-10-31 从侧栏「权限」页并入设置页，对齐用户标注：
 * 诊断摘要、日志目录与问题反馈同一分类）。
 *
 * 摘要 = "本机运行状态 + 本机权限范围"的取证入口，状态文案存 key（而不是
 * 已翻译的字符串），由模板调用 `t()`：切换语言时页面上已有的提示会一起更新。
 * 原始错误只进控制台，界面给固定文案。
 */
type LocalizedMessage = { key: string; params?: Record<string, string> };

const diagnosticText = ref("");
const diagnosticMessage = ref<LocalizedMessage | null>({ key: "尚未生成诊断摘要" });
const generatingDiagnostic = ref(false);
const logDirectoryBusy = ref(false);
const logDirectoryMessage = ref<LocalizedMessage | null>(null);

const linkBusy = ref(false);
const linkMessage = ref("");

function logError(scope: string, error: unknown): void {
  console.warn(`${scope}: ${error instanceof Error ? error.message : String(error)}`);
}

async function generateDiagnostic(): Promise<void> {
  generatingDiagnostic.value = true;
  diagnosticMessage.value = { key: "正在读取当前运行状态…" };
  try {
    diagnosticText.value = formatDiagnosticReport(await getDiagnosticReport());
    diagnosticMessage.value = { key: "诊断摘要已生成；复制前可在页面内检查全部内容" };
  } catch (error) {
    logError("generate diagnostic failed", error);
    diagnosticText.value = "";
    diagnosticMessage.value = { key: "无法生成诊断摘要，请稍后重试。" };
  } finally {
    generatingDiagnostic.value = false;
  }
}

async function copyDiagnostic(): Promise<void> {
  if (!diagnosticText.value) await generateDiagnostic();
  if (!diagnosticText.value) return;
  if (!navigator.clipboard?.writeText) {
    diagnosticMessage.value = { key: "当前环境不支持剪贴板写入。" };
    return;
  }
  try {
    await navigator.clipboard.writeText(diagnosticText.value);
    diagnosticMessage.value = { key: "诊断摘要已复制到剪贴板" };
  } catch (error) {
    logError("copy diagnostic failed", error);
    diagnosticMessage.value = { key: "无法复制诊断摘要，请稍后重试。" };
  }
}

async function onOpenLogDirectory(): Promise<void> {
  logDirectoryBusy.value = true;
  logDirectoryMessage.value = { key: "正在打开日志目录…" };
  try {
    const directory = await openLogDirectory();
    logDirectoryMessage.value = {
      key: "已打开日志目录：{directory}",
      params: { directory },
    };
  } catch (error) {
    logError("open log directory failed", error);
    logDirectoryMessage.value = { key: "无法打开日志目录，请稍后重试。" };
  } finally {
    logDirectoryBusy.value = false;
  }
}

/**
 * 外部入口失败时不静默：capability 拒绝（Not allowed to open url）与系统
 * 没有默认浏览器是两类不同问题，压成"打开失败"会让现场无法归因。
 * 成功不显示任何提示。
 */
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
</script>

<template>
  <section class="settings-section">
    <h2 class="section-title">{{ t("诊断与日志") }}</h2>
    <article class="card diagnostics-card">
      <div class="card-title-row">
        <div>
          <h2>{{ t("诊断摘要") }}</h2>
          <p class="muted">{{ t("摘要不含设备地址、语音内容等隐私信息，可放心复制发给开发者排查问题。") }}</p>
        </div>
        <div class="button-row">
          <button
            class="secondary-button"
            type="button"
            :disabled="generatingDiagnostic"
            @click="generateDiagnostic"
          >
            {{ generatingDiagnostic ? t("生成中…") : t("生成摘要") }}
          </button>
          <button
            class="primary-button"
            type="button"
            :disabled="generatingDiagnostic"
            @click="copyDiagnostic"
          >
            {{ t("复制摘要") }}
          </button>
        </div>
      </div>
      <p class="operation-message" aria-live="polite">
        {{ diagnosticMessage ? t(diagnosticMessage.key, diagnosticMessage.params) : "" }}
      </p>
      <pre v-if="diagnosticText" class="diagnostic-output">{{ diagnosticText }}</pre>

      <div class="log-directory-block">
        <button
          class="secondary-button"
          type="button"
          :disabled="logDirectoryBusy"
          @click="onOpenLogDirectory"
        >
          {{ logDirectoryBusy ? t("正在打开…") : t("打开日志目录") }}
        </button>
        <p class="muted">{{ t("日志记录本机运行细节，遇到问题时连同摘要一起发给开发者。") }}</p>
      </div>
      <p class="operation-message log-directory-message" aria-live="polite">
        {{ logDirectoryMessage ? t(logDirectoryMessage.key, logDirectoryMessage.params) : "" }}
      </p>
    </article>

    <!-- 问题反馈与诊断摘要同属「诊断与日志」分类（2026-10-31 用户标注）。 -->
    <article class="card feedback-card">
      <div class="card-title-row">
        <div class="feedback-heading">
          <span class="settings-row-icon" aria-hidden="true">
            <svg viewBox="0 0 24 24" fill="none">
              <path
                d="M12 3.8c-4.8 0-8.7 3.1-8.7 7 0 2.3 1.3 4.3 3.3 5.6l-1 3.5 4-2.2c.8.2 1.6.3 2.4.3 4.8 0 8.7-3.1 8.7-7S16.8 3.8 12 3.8z"
                stroke="currentColor"
                stroke-width="1.9"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
          </span>
          <div>
            <h2>{{ t("问题反馈") }}</h2>
            <p class="muted">{{ t("反馈无线麦 SayAll 的问题，或提出改进建议。") }}</p>
          </div>
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
    </article>
  </section>
</template>

<style scoped>
/* 分组标题与第一张卡对齐其他设置分组（全局 .diagnostics-card 自带 14px 上边距）。 */
.settings-section > .diagnostics-card {
  margin-top: 0;
}

/* 反馈卡与上面的诊断卡同属一组：沿用设置页行内图标 + 标题的节奏。 */
.feedback-card {
  margin-top: 14px;
}

.feedback-heading {
  display: flex;
  align-items: flex-start;
  gap: 14px;
  min-width: 0;
}

.feedback-heading h2 {
  margin-bottom: 5px;
}

.feedback-heading .muted {
  margin: 0;
  font-size: 13px;
}

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

.log-directory-block .muted {
  margin: 0;
}
</style>
