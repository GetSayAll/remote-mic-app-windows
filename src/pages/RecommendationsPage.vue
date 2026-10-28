<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  goBackRecommendationsPage,
  hideRecommendationsPage,
  isTauriRuntime,
  openRecommendationsWebsite,
  retryRecommendationsPage,
  showRecommendationsPage,
  type RecommendationsPageEvent,
} from "../lib/bridge";
import { reportFrontendEvent } from "../lib/frontend-diagnostics";
import { useTheme, initializeTheme } from "../lib/theme";
import { initializeLanguage, useLanguage } from "../lib/language";
import type { EffectiveLanguage } from "../lib/bridge";

/**
 * 推荐页只做官网页面宿主：
 * - 页面内容、筛选、详情、排序全部来自 https://sayall.app/recommendations/；
 * - 加载/失败状态由 Vue 提供（子 WebView 加载成功前隐藏，失败后也保持隐藏）；
 * - 浏览器预览没有子 WebView，给出明确说明和官网入口，不伪造推荐内容。
 */
const { effectiveTheme } = useTheme();
const { effectiveLanguage } = useLanguage();
const status = ref<"loading" | "ready" | "failed">("loading");
const canGoBack = ref(false);
const activeGeneration = ref(0);
let unlisten: UnlistenFn | undefined;
let disposed = false;
let appearanceReady = false;
let loadSequence = 0;
let bufferedEvent: RecommendationsPageEvent | null = null;
let loadTimer: ReturnType<typeof setTimeout> | undefined;

/**
 * 主文档加载超时。官网或网络偶发卡住时，WebView2 可能长时间不报告
 * NavigationCompleted；没有超时的话子 WebView 永远隐藏，用户只能看到
 * 一直转圈的「正在加载网页…」，也永远等不到失败/重试页（对齐 Mac）。
 */
const LOAD_TIMEOUT_MS = 20_000;
/** 固定文案与 Mac `RecommendationsView` 对齐；不要把底层错误拼进用户可见文本。 */
const COPY = {
  "zh-Hans": {
    title: "推荐",
    loading: "正在加载网页…",
    failure: "暂时无法加载此页面，你可以前往官网查看，或稍后重试。",
    openWebsite: "打开官网",
    retry: "重试",
    back: "返回上一页",
    browserPreview: "浏览器预览中无法内嵌官网推荐页面，请前往官网查看。",
    openFailed: "无法打开浏览器，请手动访问 sayall.app/recommendations/。",
  },
  en: {
    title: "Picks",
    loading: "Loading page…",
    failure:
      "This page is temporarily unavailable. Open the website or try again later.",
    openWebsite: "Open website",
    retry: "Retry",
    back: "Back",
    browserPreview:
      "The website cannot be embedded in the browser preview. Open the website instead.",
    openFailed:
      "Unable to open the browser. Please visit sayall.app/en/recommendations/ manually.",
  },
} as const;

const copy = computed(() => COPY[effectiveLanguage.value]);

function copyFor(language: EffectiveLanguage) {
  return COPY[language];
}

const message = ref<string>(copyFor(effectiveLanguage.value).loading);

function clearLoadTimer(): void {
  if (loadTimer !== undefined) {
    clearTimeout(loadTimer);
    loadTimer = undefined;
  }
}

function startLoadTimer(request: number): void {
  clearLoadTimer();
  loadTimer = setTimeout(() => {
    loadTimer = undefined;
    if (disposed || request !== loadSequence || status.value !== "loading") {
      return;
    }
    status.value = "failed";
    message.value = copy.value.failure;
    canGoBack.value = false;
    reportFrontendEvent({
      event: "recommendations_page",
      phase: "load",
      result: "failed",
      reason: "page_load_timeout",
      elapsedMs: LOAD_TIMEOUT_MS,
    });
    // 让原生侧放弃这一代页面：迟到成功的导航不得再把网页层盖回失败页。
    activeGeneration.value = 0;
    void Promise.resolve(hideRecommendationsPage()).catch(() => undefined);
  }, LOAD_TIMEOUT_MS);
}

function applyEvent(event: RecommendationsPageEvent): void {
  if (event.generation !== activeGeneration.value) {
    bufferedEvent = event;
    return;
  }
  clearLoadTimer();
  status.value = event.status;
  canGoBack.value =
    event.status === "ready" ? (event.canGoBack ?? false) : false;
  if (event.status === "failed") {
    message.value = copy.value.failure;
  }
}

async function load(
  theme: "light" | "dark" = effectiveTheme.value,
  language: EffectiveLanguage = effectiveLanguage.value,
  retry = false,
): Promise<void> {
  const request = ++loadSequence;
  clearLoadTimer();
  status.value = "loading";
  message.value = copyFor(language).loading;
  canGoBack.value = false;
  bufferedEvent = null;

  if (!isTauriRuntime()) {
    if (request === loadSequence && !disposed) {
      status.value = "failed";
      message.value = copyFor(language).browserPreview;
    }
    return;
  }

  startLoadTimer(request);
  try {
    // 失败后重试必须回到“当前页”（含 ?item= 详情），与 Mac 容器一致。
    const generation = retry
      ? await retryRecommendationsPage(theme, language)
      : await showRecommendationsPage(theme, language);
    if (disposed || request !== loadSequence) return;
    activeGeneration.value = generation;
    const buffered = bufferedEvent as RecommendationsPageEvent | null;
    if (buffered?.generation === generation) {
      applyEvent(buffered);
    }
  } catch {
    if (disposed || request !== loadSequence) return;
    clearLoadTimer();
    status.value = "failed";
    // 与 Mac 一致：失败页只显示固定文案，底层错误只进结构化日志，不拼接给用户。
    message.value = copyFor(language).failure;
    reportFrontendEvent({
      event: "recommendations_page",
      phase: "show",
      result: "failed",
      reason: "show_command_failed",
    });
  }
}

async function openWebsite(): Promise<void> {
  try {
    await openRecommendationsWebsite(effectiveLanguage.value);
  } catch {
    message.value = copy.value.openFailed;
  }
}

async function retry(): Promise<void> {
  await load(effectiveTheme.value, effectiveLanguage.value, true);
}

async function goBack(): Promise<void> {
  try {
    await goBackRecommendationsPage();
  } catch {
    // 后退本身失败时保留当前页；Rust 侧已记录结构化日志。
  }
}

onMounted(async () => {
  if (isTauriRuntime()) {
    try {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<RecommendationsPageEvent>(
        "recommendations-page-state",
        (event) => {
          if (!disposed) applyEvent(event.payload);
        },
      );
    } catch {
      // 监听失败不阻塞加载；页面本身仍会显示，只是失败时没有原生事件可等。
    }
  }
  // 外观与语言设置都是异步 IPC。必须等它们完成再首次加载，否则深色偏好/
  // 英文偏好到达前会先用默认值加载一版，再切一次；主题切换还会把旧页面
  // 留在后退栈里，出现不该有的「返回上一页」。
  await Promise.all([initializeTheme(), initializeLanguage()]);
  if (disposed) return;
  appearanceReady = true;
  await load(effectiveTheme.value, effectiveLanguage.value);
});

// 官网嵌入模式通过 `theme` 与语言跟随 App 设置；变化时重新加载同一页面。
// 初始化期间的 effectiveTheme / effectiveLanguage 变化由上面的首次加载消化，
// 避免重复 show。
watch([effectiveTheme, effectiveLanguage], ([theme, language]) => {
  if (!disposed && appearanceReady && isTauriRuntime()) {
    void load(theme, language);
  }
});

onUnmounted(() => {
  disposed = true;
  clearLoadTimer();
  unlisten?.();
  void Promise.resolve(hideRecommendationsPage()).catch(() => undefined);
});
</script>

<template>
  <div class="recommendations-page" role="region" :aria-label="copy.title">
    <div
      v-if="status === 'ready' && canGoBack"
      class="recommendations-backbar"
    >
      <button
        class="secondary-button recommendations-back"
        type="button"
        @click="goBack"
      >
        {{ copy.back }}
      </button>
    </div>
    <div class="recommendations-body">
      <div
        v-if="status === 'loading'"
        class="recommendations-state"
        role="status"
        aria-live="polite"
      >
        <span class="recommendations-spinner" aria-hidden="true"></span>
        <p>{{ message }}</p>
      </div>
      <div
        v-else-if="status === 'failed'"
        class="recommendations-state"
        role="alert"
      >
        <h1>{{ copy.title }}</h1>
        <p>{{ message }}</p>
        <div class="recommendations-actions">
          <button class="primary-button" type="button" @click="openWebsite">
            {{ copy.openWebsite }}
          </button>
          <button class="secondary-button" type="button" @click="retry">
            {{ copy.retry }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.recommendations-page {
  min-height: calc(100vh - 48px);
  display: flex;
  flex-direction: column;
}

.recommendations-body {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}

/* 与子 WebView 顶部让出的 44px 对齐；只在 ready && 可后退时出现。 */
.recommendations-backbar {
  flex: none;
  height: 44px;
  display: flex;
  align-items: center;
  padding: 0 16px;
}

.recommendations-back {
  min-height: 32px;
  padding: 0 12px;
  font-size: 13px;
}

.recommendations-state {
  max-width: 440px;
  text-align: center;
  color: var(--text-subtle-strong);
}

.recommendations-state h1 {
  margin: 0 0 10px;
  font-size: 24px;
  color: var(--text-primary);
}

.recommendations-state p {
  margin: 0;
  font-size: 14px;
  line-height: 1.6;
}

.recommendations-actions {
  display: flex;
  justify-content: center;
  gap: 12px;
  margin-top: 20px;
}

.recommendations-spinner {
  display: block;
  width: 22px;
  height: 22px;
  margin: 0 auto 12px;
  border: 2px solid var(--border);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: recommendations-spin 0.8s linear infinite;
}

@keyframes recommendations-spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
