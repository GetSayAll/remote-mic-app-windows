<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from "vue";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  hideRecommendationsPage,
  isTauriRuntime,
  openRecommendationsWebsite,
  showRecommendationsPage,
  type RecommendationsPageEvent,
} from "../lib/bridge";
import { useTheme } from "../lib/theme";

/**
 * 推荐页只做官网页面宿主：
 * - 页面内容、筛选、详情、排序全部来自 https://sayall.app/recommendations/；
 * - 加载/失败状态由 Vue 提供（子 WebView 加载成功前隐藏，失败后也保持隐藏）；
 * - 浏览器预览没有子 WebView，给出明确说明和官网入口，不伪造推荐内容。
 */
const { effectiveTheme } = useTheme();
const status = ref<"loading" | "ready" | "failed">("loading");
const message = ref("正在加载网页…");
const activeGeneration = ref(0);
let unlisten: UnlistenFn | undefined;
let disposed = false;
let loadSequence = 0;
let bufferedEvent: RecommendationsPageEvent | null = null;

function applyEvent(event: RecommendationsPageEvent): void {
  if (event.generation !== activeGeneration.value) {
    bufferedEvent = event;
    return;
  }
  status.value = event.status;
  if (event.status === "failed") {
    message.value =
      "暂时无法加载此页面，你可以前往官网查看，或稍后重试。";
  }
}

async function load(theme: "light" | "dark" = effectiveTheme.value): Promise<void> {
  const request = ++loadSequence;
  status.value = "loading";
  message.value = "正在加载网页…";
  bufferedEvent = null;

  if (!isTauriRuntime()) {
    if (request === loadSequence && !disposed) {
      status.value = "failed";
      message.value = "浏览器预览中无法内嵌官网推荐页面，请前往官网查看。";
    }
    return;
  }

  try {
    const generation = await showRecommendationsPage(theme);
    if (disposed || request !== loadSequence) return;
    activeGeneration.value = generation;
    const buffered = bufferedEvent as RecommendationsPageEvent | null;
    if (buffered?.generation === generation) {
      applyEvent(buffered);
    }
  } catch (error) {
    if (disposed || request !== loadSequence) return;
    status.value = "failed";
    message.value =
      error instanceof Error && error.message
        ? `暂时无法加载此页面：${error.message}`
        : "暂时无法加载此页面，你可以前往官网查看，或稍后重试。";
  }
}

async function openWebsite(): Promise<void> {
  try {
    await openRecommendationsWebsite();
  } catch {
    message.value = "无法打开浏览器，请手动访问 sayall.app/recommendations/。";
  }
}

async function retry(): Promise<void> {
  await load();
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
  await load();
});

// 官网嵌入模式通过 `theme` 参数跟随 App 外观；变化时重新加载同一页面。
watch(effectiveTheme, (theme) => {
  if (!disposed && isTauriRuntime()) {
    void load(theme);
  }
});

onUnmounted(() => {
  disposed = true;
  unlisten?.();
  void Promise.resolve(hideRecommendationsPage()).catch(() => undefined);
});
</script>

<template>
  <div class="recommendations-page" role="region" aria-label="推荐">
    <div
      v-if="status === 'loading'"
      class="recommendations-state"
      role="status"
      aria-live="polite"
    >
      <span class="recommendations-spinner" aria-hidden="true"></span>
      <p>{{ message }}</p>
    </div>
    <div v-else-if="status === 'failed'" class="recommendations-state" role="alert">
      <h1>推荐</h1>
      <p>{{ message }}</p>
      <div class="recommendations-actions">
        <button class="primary-button" type="button" @click="openWebsite">
          打开官网
        </button>
        <button class="secondary-button" type="button" @click="retry">重试</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.recommendations-page {
  min-height: calc(100vh - 48px);
  display: flex;
  align-items: center;
  justify-content: center;
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
