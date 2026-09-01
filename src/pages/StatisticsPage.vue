<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import {
  formatUsageDuration,
  getUsageStatistics,
  type UsageStatisticsSummary,
  type UsageTotals,
} from "../lib/bridge";

type UsagePeriod = "today" | "thisWeek" | "total";

const periods: Array<{ id: UsagePeriod; label: string }> = [
  { id: "today", label: "今日" },
  { id: "thisWeek", label: "本周" },
  { id: "total", label: "全部" },
];

const summary = ref<UsageStatisticsSummary | null>(null);
const selectedPeriod = ref<UsagePeriod>("today");
const loading = ref(true);
const loadError = ref("");
let refreshTimer: number | null = null;
let refreshing = false;

const selectedUsage = computed<UsageTotals>(() =>
  summary.value?.[selectedPeriod.value] ?? {
    buttonPresses: 0,
    voiceSessions: 0,
    voiceSeconds: 0,
  },
);

const hasUsage = computed(() => {
  const usage = summary.value?.total;
  return Boolean(
    usage && (usage.buttonPresses > 0 || usage.voiceSessions > 0 || usage.voiceSeconds > 0),
  );
});

const maximumDailyButtonPresses = computed(() =>
  Math.max(1, ...(summary.value?.recentDays.map((day) => day.usage.buttonPresses) ?? [0])),
);

const maximumDailyVoiceSeconds = computed(() =>
  Math.max(1, ...(summary.value?.recentDays.map((day) => day.usage.voiceSeconds) ?? [0])),
);

function dayLabel(localDate: string): string {
  const date = new Date(`${localDate}T12:00:00`);
  return new Intl.DateTimeFormat("zh-CN", {
    month: "numeric",
    day: "numeric",
    weekday: "short",
  }).format(date);
}

function barWidth(value: number, maximum: number): string {
  if (value <= 0) return "0%";
  return `${Math.max(4, (value / maximum) * 100)}%`;
}

async function refreshStatistics(): Promise<void> {
  if (refreshing) return;
  refreshing = true;
  try {
    summary.value = await getUsageStatistics();
    loadError.value = "";
  } catch (error) {
    loadError.value = error instanceof Error ? error.message : String(error);
  } finally {
    loading.value = false;
    refreshing = false;
  }
}

onMounted(() => {
  void refreshStatistics();
  refreshTimer = window.setInterval(() => {
    void refreshStatistics();
  }, 1_000);
});

onUnmounted(() => {
  if (refreshTimer !== null) {
    window.clearInterval(refreshTimer);
  }
});
</script>

<template>
  <section>
    <header class="page-header statistics-header">
      <div>
        <h1>统计</h1>
        <p>本页统计只保存每日汇总，不包含语音内容、识别文字、窗口标题、蓝牙身份或个人文件路径。</p>
      </div>
      <div class="statistics-header-actions">
        <div class="period-switch" aria-label="统计时间范围">
          <button
            v-for="period in periods"
            :key="period.id"
            type="button"
            :class="{ active: selectedPeriod === period.id }"
            :aria-pressed="selectedPeriod === period.id"
            @click="selectedPeriod = period.id"
          >
            {{ period.label }}
          </button>
        </div>
        <span class="badge success">仅保存在本机</span>
      </div>
    </header>

    <p v-if="loadError" class="error-banner">无法读取本机统计：{{ loadError }}</p>

    <div class="metric-grid" :aria-busy="loading">
      <article class="card metric-card">
        <span>按键次数</span><strong>{{ selectedUsage.buttonPresses.toLocaleString() }}</strong><small>次</small>
      </article>
      <article class="card metric-card">
        <span>语音次数</span><strong>{{ selectedUsage.voiceSessions.toLocaleString() }}</strong><small>次</small>
      </article>
      <article class="card metric-card duration-card">
        <span>语音时长</span><strong>{{ formatUsageDuration(selectedUsage.voiceSeconds) }}</strong>
      </article>
    </div>

    <article v-if="hasUsage && summary" class="card usage-chart-card">
      <div class="card-title-row">
        <div>
          <h2>最近 7 天</h2>
          <p class="muted">按本机日期聚合；语音时长只统计成功排空的完整会话。</p>
        </div>
        <div class="chart-legend" aria-label="图例">
          <span><i class="button-dot"></i>按键</span>
          <span><i class="voice-dot"></i>语音</span>
        </div>
      </div>
      <div class="usage-day-list">
        <div v-for="day in summary.recentDays" :key="day.localDate" class="usage-day-row">
          <span class="usage-day-label">{{ dayLabel(day.localDate) }}</span>
          <div class="usage-bars">
            <div class="usage-bar-track">
              <span
                class="usage-bar button-bar"
                :style="{ width: barWidth(day.usage.buttonPresses, maximumDailyButtonPresses) }"
              ></span>
            </div>
            <div class="usage-bar-track">
              <span
                class="usage-bar voice-bar"
                :style="{ width: barWidth(day.usage.voiceSeconds, maximumDailyVoiceSeconds) }"
              ></span>
            </div>
          </div>
          <span class="usage-day-value">
            {{ day.usage.buttonPresses }} 次 · {{ formatUsageDuration(day.usage.voiceSeconds) }}
          </span>
        </div>
      </div>
    </article>

    <article v-else-if="!loading" class="card empty-state">
      <div class="empty-icon">▥</div>
      <h2>暂无可展示的数据</h2>
      <p>连接 RC003 并完成真实语音会话或普通按键监听后，这里会显示仅保存在本机的每日汇总。</p>
    </article>
  </section>
</template>
