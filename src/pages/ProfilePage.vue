<script setup lang="ts">
import type { CSSProperties } from "vue";
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import type { ButtonMappings, RuntimeSnapshot, UsageProfile } from "../lib/bridge";
import {
  getButtonMappings,
  getCombinedActions,
  getCommonPhrasePanelSnapshot,
  getUsageProfile,
  latestMacroDefinitions,
  listPresetApps,
  registerCommonPhraseAvailability,
  registerMacroNames,
  registerPresetAppNames,
} from "../lib/bridge";
import {
  buildActionRanking,
  buildButtonRanking,
  buildCalendarWeeks,
  formatCalendarDate,
  formatDuration,
  formatVoiceSessionDate,
  formatVoiceTotal,
  WEEKDAY_LABELS,
} from "../lib/profile";

defineProps<{ runtime?: RuntimeSnapshot | null }>();

const profile = ref<UsageProfile | null>(null);
const mappings = ref<ButtonMappings | null>(null);
const loading = ref(true);
const error = ref("");
const selectedDate = ref<string | null>(null);
const actionCacheVersion = ref(0);
const calendarScroll = ref<HTMLElement | null>(null);
const calendarCellSize = ref(20);
let pollTimer: ReturnType<typeof setInterval> | undefined;
let resizeObserver: ResizeObserver | null = null;
let refreshing = false;
let requestGeneration = 0;

const numberFormatter = new Intl.NumberFormat("zh-CN");

const metrics = computed(() => {
  const data = profile.value;
  return [
    {
      id: "total-buttons",
      title: "累计按键次数",
      value: data ? numberFormatter.format(data.totalButtonPresses) : "—",
      unit: "次",
      icon: "keyboard",
    },
    {
      id: "total-voice",
      title: "累计语音时长",
      value: data ? formatVoiceTotal(data.totalVoiceSeconds) : "—",
      unit: null,
      icon: "waveform",
    },
    {
      id: "week-buttons",
      title: "本周按键",
      value: data ? numberFormatter.format(data.weekButtonPresses) : "—",
      unit: "次",
      icon: "trend",
    },
    {
      id: "longest-voice",
      title: "最长单次语音",
      value: data ? formatDuration(data.longestVoiceSeconds) : "—",
      unit: null,
      icon: "clock",
    },
  ];
});

const actionRanking = computed(() => {
  // 动作显示名来自 bridge 的模块级缓存；缓存预热完成后主动重算一次。
  actionCacheVersion.value;
  return buildActionRanking(mappings.value, profile.value?.buttonPressesByControl ?? {});
});
const buttonRanking = computed(() =>
  buildButtonRanking(profile.value?.buttonPressesByControl ?? {}),
);
const voiceSessions = computed(() => profile.value?.voiceSessions ?? []);
const calendarWeeks = computed(() =>
  buildCalendarWeeks(profile.value?.recentDays ?? [], new Date()),
);
const flatCalendarDays = computed(() => calendarWeeks.value.flatMap((week) => week.days));
const monthLabels = computed(() =>
  calendarWeeks.value
    .map((week, index) => ({ index, label: week.monthLabel }))
    .filter((item): item is { index: number; label: string } => item.label !== null),
);
const selectedDay = computed(() => {
  if (!selectedDate.value) return null;
  return flatCalendarDays.value.find((day) => day.date === selectedDate.value) ?? null;
});
const selectedDayLabel = computed(() =>
  selectedDate.value ? formatCalendarDate(selectedDate.value) : "",
);

const calendarStyle = computed(
  () =>
    ({
      "--calendar-cell": `${calendarCellSize.value}px`,
    }) as CSSProperties,
);
const calendarMonthsStyle = computed(
  () =>
    ({
      gridTemplateColumns: `repeat(${Math.max(1, calendarWeeks.value.length)}, var(--calendar-cell))`,
    }) as CSSProperties,
);

const ICON_PATHS: Record<string, string[]> = {
  keyboard: [
    "M3.2 6.8h17.6a1.7 1.7 0 0 1 1.7 1.7v7a1.7 1.7 0 0 1-1.7 1.7H3.2a1.7 1.7 0 0 1-1.7-1.7v-7a1.7 1.7 0 0 1 1.7-1.7z",
    "M7 10.2h.01M10.4 10.2h.01M13.8 10.2h.01M17.2 10.2h.01",
    "M7.6 13.6h8.8",
  ],
  waveform: [
    "M4 10v4M8 6v12M12 8v8M16 4v16M20 9v6",
  ],
  trend: [
    "M4 17l5-5 4 3 7-9",
    "M15 6h5v5",
  ],
  clock: [
    "M12 3.2a8.8 8.8 0 1 0 0 17.6 8.8 8.8 0 0 0 0-17.6z",
    "M12 7.2V12l3.4 2",
  ],
  list: [
    "M4 6.2h16M4 12h16M4 17.8h16",
  ],
};

function iconPaths(name: string): string[] {
  return ICON_PATHS[name] ?? [];
}

function calendarBackground(day: { date: string | null; intensity: number }): string {
  if (!day.date) return "transparent";
  if (day.intensity <= 0) return "color-mix(in srgb, var(--muted) 24%, transparent)";
  return `color-mix(in srgb, var(--accent) ${Math.round(22 + day.intensity * 78)}%, transparent)`;
}

function calendarDayLabel(day: {
  date: string | null;
  buttonPresses: number;
  voiceSeconds: number;
}): string {
  if (!day.date) return "";
  return `${formatCalendarDate(day.date)}，按键 ${day.buttonPresses} 次，语音 ${formatDuration(
    day.voiceSeconds,
  )}`;
}

function selectDay(day: { date: string | null }): void {
  if (day.date) selectedDate.value = day.date;
}

async function primeActionCaches(): Promise<void> {
  const [apps, macros, phrases] = await Promise.allSettled([
    listPresetApps(),
    getCombinedActions(),
    getCommonPhrasePanelSnapshot(),
  ]);
  if (apps.status === "fulfilled") registerPresetAppNames(apps.value);
  if (macros.status === "fulfilled") {
    registerMacroNames(latestMacroDefinitions(macros.value.definitions));
  }
  if (phrases.status === "fulfilled") {
    registerCommonPhraseAvailability(phrases.value.available);
  }
  actionCacheVersion.value += 1;
}

async function refresh(): Promise<void> {
  if (refreshing) return;
  refreshing = true;
  const generation = ++requestGeneration;
  try {
    const [nextProfile, nextMappings] = await Promise.all([
      getUsageProfile(),
      mappings.value ? Promise.resolve(mappings.value) : getButtonMappings().catch(() => null),
    ]);
    if (generation !== requestGeneration) return;
    profile.value = nextProfile;
    if (nextMappings) mappings.value = nextMappings;
    error.value = "";
  } catch {
    if (generation === requestGeneration) {
      // 读取失败时保留上一次成功的数据，但明确告诉用户当前不是实时值。
      error.value = "无法读取本机统计，请稍后重试。";
    }
  } finally {
    if (generation === requestGeneration) {
      loading.value = false;
      refreshing = false;
    }
  }
}

function retry(): void {
  loading.value = true;
  error.value = "";
  void refresh();
}

function updateCalendarSize(): void {
  const width = calendarScroll.value?.clientWidth ?? 0;
  const columns = Math.max(1, calendarWeeks.value.length);
  if (width <= 0) {
    calendarCellSize.value = 20;
    return;
  }
  const available = width - 30 - 8;
  const raw = (available - (columns - 1) * 3) / columns;
  calendarCellSize.value = Math.min(28, Math.max(14, Math.round(raw * 10) / 10));
}

watch(calendarWeeks, () => {
  nextTick(updateCalendarSize);
});
watch(profile, () => {
  if (selectedDate.value && !flatCalendarDays.value.some((day) => day.date === selectedDate.value)) {
    selectedDate.value = null;
  }
  nextTick(updateCalendarSize);
});

onMounted(() => {
  void primeActionCaches();
  void refresh();
  pollTimer = setInterval(() => {
    void refresh();
  }, 1_000);
  if (typeof ResizeObserver !== "undefined" && calendarScroll.value) {
    resizeObserver = new ResizeObserver(updateCalendarSize);
    resizeObserver.observe(calendarScroll.value);
  }
  nextTick(updateCalendarSize);
});

onUnmounted(() => {
  requestGeneration += 1;
  if (pollTimer) clearInterval(pollTimer);
  resizeObserver?.disconnect();
  resizeObserver = null;
});
</script>

<template>
  <section class="profile-page" :aria-busy="loading">
    <header class="page-header profile-header">
      <h1>个人中心</h1>
      <span class="badge success">数据仅存本机</span>
    </header>

    <div v-if="error" class="profile-error" role="alert">
      <span>{{ error }}</span>
      <button type="button" class="secondary-button" @click="retry">重试</button>
    </div>

    <div class="profile-kpis">
      <article v-for="metric in metrics" :key="metric.id" class="profile-kpi">
        <span class="profile-kpi-icon" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none">
            <path
              v-for="(path, index) in iconPaths(metric.icon)"
              :key="index"
              :d="path"
              stroke="currentColor"
              stroke-width="1.9"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          </svg>
        </span>
        <div class="profile-kpi-text">
          <span class="profile-kpi-label">{{ metric.title }}</span>
          <div class="profile-kpi-value">
            <strong>{{ metric.value }}</strong>
            <small v-if="metric.unit">{{ metric.unit }}</small>
          </div>
        </div>
      </article>
    </div>

    <div class="profile-columns">
      <section class="profile-panel profile-ranking">
        <h2>排行榜</h2>

        <div class="ranking-section">
          <h3>
            <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <path
                v-for="(path, index) in iconPaths('keyboard')"
                :key="index"
                :d="path"
                stroke="currentColor"
                stroke-width="1.9"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
            <span>常用快捷键 / 动作</span>
          </h3>
          <ol v-if="actionRanking.length" class="ranking-list">
            <li v-for="(entry, index) in actionRanking" :key="entry.title" class="ranking-row">
              <span class="rank" :class="{ top: index < 3 }">{{ index + 1 }}</span>
              <span class="ranking-title">{{ entry.title }}</span>
              <span class="ranking-count">{{ numberFormatter.format(entry.count) }} <small>次</small></span>
            </li>
          </ol>
          <p v-else class="profile-empty">还没有可展示的数据。</p>
        </div>

        <div class="ranking-section">
          <h3>
            <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <path
                v-for="(path, index) in iconPaths('list')"
                :key="index"
                :d="path"
                stroke="currentColor"
                stroke-width="1.9"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
            <span>实体按键排行</span>
          </h3>
          <ol v-if="buttonRanking.length" class="ranking-list">
            <li v-for="(entry, index) in buttonRanking" :key="entry.title" class="ranking-row">
              <span class="rank" :class="{ top: index < 3 }">{{ index + 1 }}</span>
              <span class="ranking-title">{{ entry.title }}</span>
              <span class="ranking-count">{{ numberFormatter.format(entry.count) }} <small>次</small></span>
            </li>
          </ol>
          <p v-else class="profile-empty">还没有可展示的数据。</p>
        </div>
      </section>

      <div class="profile-right">
        <section class="profile-panel profile-calendar">
          <div class="profile-panel-title-row">
            <h2>使用日历</h2>
            <span class="profile-hint">最近半年，颜色越深使用越多</span>
          </div>

          <div ref="calendarScroll" class="calendar-scroll" :style="calendarStyle">
            <div class="calendar-inner">
              <div class="calendar-months" :style="calendarMonthsStyle" aria-hidden="true">
                <span
                  v-for="label in monthLabels"
                  :key="`${label.index}-${label.label}`"
                  class="calendar-month"
                  :style="{ gridColumn: `${label.index + 1} / span 2` }"
                >
                  {{ label.label }}
                </span>
              </div>
              <div class="calendar-body">
                <div class="calendar-weekdays" aria-hidden="true">
                  <span v-for="label in WEEKDAY_LABELS" :key="label">{{ label }}</span>
                </div>
                <div class="calendar-grid">
                  <template v-for="(day, index) in flatCalendarDays" :key="day.date ?? `empty-${index}`">
                    <button
                      v-if="day.date"
                      type="button"
                      class="calendar-day"
                      :class="{ today: day.isToday, selected: selectedDate === day.date }"
                      :style="{ background: calendarBackground(day) }"
                      :aria-label="calendarDayLabel(day)"
                      :title="calendarDayLabel(day)"
                      @click="selectDay(day)"
                    />
                    <span v-else class="calendar-day empty" aria-hidden="true" />
                  </template>
                </div>
              </div>
            </div>
          </div>

          <div v-if="selectedDay" class="calendar-detail">
            <strong>{{ selectedDayLabel }}</strong>
            <span>按键 {{ numberFormatter.format(selectedDay.buttonPresses) }} 次</span>
            <span>语音 {{ formatDuration(selectedDay.voiceSeconds) }}</span>
          </div>
        </section>

        <section class="profile-panel profile-voice">
          <h2 class="profile-voice-title">
            <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <path
                v-for="(path, index) in iconPaths('waveform')"
                :key="index"
                :d="path"
                stroke="currentColor"
                stroke-width="1.9"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
            <span>单次语音时长排行</span>
          </h2>

          <ol v-if="voiceSessions.length" class="voice-list">
            <li
              v-for="(session, index) in voiceSessions"
              :key="`${session.endedAtMs}-${index}`"
              class="voice-row"
            >
              <span class="voice-rank">{{ index + 1 }}</span>
              <span class="voice-duration">{{ formatDuration(session.durationSeconds) }}</span>
              <span class="voice-dot">·</span>
              <span class="voice-app">{{ session.applicationName ?? "未知应用" }}</span>
              <span class="voice-date">{{ formatVoiceSessionDate(session.endedAtMs) }}</span>
            </li>
          </ol>
          <p v-else class="profile-empty">完成一次语音输入后，这里会显示本机排行。</p>
        </section>
      </div>
    </div>
  </section>
</template>

<style scoped>
.profile-page {
  display: flex;
  flex-direction: column;
  gap: 14px;
  min-width: 0;
  padding-bottom: 8px;
}

.profile-header {
  align-items: center;
  margin-bottom: 0;
}

.profile-header h1 {
  font-size: 25px;
  font-weight: 600;
  letter-spacing: -0.5px;
}

/* Mac StatusPill：12 semibold、胶囊 tint；它是状态文字，不是按钮。 */
.profile-header .badge {
  min-height: 0;
  padding: 5px 9px;
  color: #34c759;
  background: color-mix(in srgb, #34c759 14%, transparent);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.2;
  cursor: default;
}

:global(html[data-theme="dark"] .profile-header .badge) {
  color: #30d158;
}

.profile-error {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin: 0 0 0 14px;
  padding: 9px 12px;
  border: 1px solid color-mix(in srgb, var(--error-text) 24%, transparent);
  border-radius: 10px;
  color: var(--error-text);
  background: var(--error-surface);
  font-size: 13px;
}

.profile-kpis {
  display: grid;
  grid-template-columns: repeat(4, minmax(170px, 1fr));
  gap: 12px;
}

.profile-kpi {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 88px;
  padding: 12px;
  border: 1px solid color-mix(in srgb, var(--accent) 10%, var(--border));
  border-radius: 16px;
  background: color-mix(in srgb, var(--accent) 6%, var(--card));
  box-shadow: 0 8px 30px var(--shadow-card);
}

.profile-kpi-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 42px;
  height: 42px;
  flex: 0 0 auto;
  border-radius: 11px;
  color: var(--accent);
  background: var(--accent-surface);
}

.profile-kpi-icon svg {
  width: 22px;
  height: 22px;
}

.profile-kpi-text {
  min-width: 0;
}

.profile-kpi-label {
  display: block;
  overflow: hidden;
  color: var(--muted);
  font-size: 12px;
  line-height: 1.3;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.profile-kpi-value {
  display: flex;
  align-items: baseline;
  gap: 5px;
  margin-top: 4px;
}

.profile-kpi-value strong {
  overflow: hidden;
  font-size: 22px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  line-height: 1.15;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.profile-kpi-value small {
  flex: 0 0 auto;
  color: var(--accent);
  font-size: 12px;
  font-weight: 600;
}

.profile-columns {
  display: grid;
  grid-template-columns: minmax(360px, 42%) minmax(0, 1fr);
  gap: 14px;
  align-items: stretch;
}

.profile-right {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 14px;
}

.profile-panel {
  min-width: 0;
  padding: 16px;
  border: 1px solid var(--border);
  border-radius: 20px;
  background: var(--card);
  box-shadow: 0 8px 30px var(--shadow-card);
}

.profile-panel h2 {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
  line-height: 1.25;
}

.profile-ranking {
  display: flex;
  flex-direction: column;
}

.ranking-section {
  margin-top: 12px;
}

.ranking-section h3 {
  display: flex;
  align-items: center;
  gap: 7px;
  margin: 0 0 7px;
  font-size: 12px;
  font-weight: 600;
}

.ranking-section h3 svg {
  width: 16px;
  height: 16px;
  flex: 0 0 auto;
  color: var(--text-control);
}

.ranking-list,
.voice-list {
  margin: 0;
  padding: 0;
  list-style: none;
}

.ranking-row {
  display: grid;
  grid-template-columns: 18px minmax(0, 1fr) auto;
  align-items: center;
  gap: 8px;
  min-height: 29px;
  border-bottom: 1px solid var(--border);
  font-size: 13px;
}

.ranking-row:last-child {
  border-bottom: 0;
}

.rank {
  color: var(--muted);
  font-size: 12px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.rank.top {
  color: var(--accent);
}

.ranking-title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ranking-count {
  color: var(--muted);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.ranking-count small {
  font-size: 12px;
}

.profile-empty {
  margin: 8px 0 4px;
  color: var(--muted);
  font-size: 12px;
  line-height: 1.5;
}

.profile-panel-title-row {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 10px;
  margin-bottom: 10px;
}

.profile-hint {
  max-width: 160px;
  color: var(--muted);
  font-size: 12px;
  line-height: 1.35;
  text-align: right;
}

.calendar-scroll {
  overflow-x: auto;
  overflow-y: hidden;
  padding-bottom: 2px;
}

.calendar-inner {
  min-width: min-content;
}

.calendar-months {
  display: grid;
  gap: 3px;
  height: 18px;
  margin-left: 38px;
}

.calendar-month {
  overflow: visible;
  color: var(--muted);
  font-size: 12px;
  line-height: 18px;
  white-space: nowrap;
}

.calendar-body {
  display: flex;
  gap: 8px;
}

.calendar-weekdays {
  display: grid;
  flex: 0 0 30px;
  grid-template-rows: repeat(7, var(--calendar-cell));
  gap: 3px;
  width: 30px;
}

.calendar-weekdays span {
  display: flex;
  align-items: center;
  color: var(--muted);
  font-size: 12px;
  line-height: 1;
  white-space: nowrap;
}

.calendar-grid {
  display: grid;
  grid-auto-flow: column;
  grid-auto-columns: var(--calendar-cell);
  grid-template-rows: repeat(7, var(--calendar-cell));
  gap: 3px;
}

.calendar-day {
  width: var(--calendar-cell);
  height: var(--calendar-cell);
  padding: 0;
  border: 0;
  border-radius: 2px;
  cursor: pointer;
}

.calendar-day.empty {
  background: transparent;
  cursor: default;
}

.calendar-day.today {
  outline: 1px solid var(--accent);
  outline-offset: 0;
}

.calendar-day.selected {
  box-shadow: 0 0 0 2px var(--accent);
}

.calendar-day:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

.calendar-detail {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 10px;
  padding: 9px 10px;
  border-radius: 12px;
  color: var(--text-control);
  background: var(--surface-subtle);
  font-size: 12px;
}

.calendar-detail strong {
  font-size: 13px;
}

.profile-voice-title {
  display: flex;
  align-items: center;
  gap: 7px;
  color: var(--pressed-text);
  font-size: 15px;
  font-weight: 600;
  line-height: 1.25;
}

.profile-voice-title svg {
  width: 18px;
  height: 18px;
  flex: 0 0 auto;
}

.voice-list {
  margin-top: 7px;
}

.voice-row {
  display: grid;
  grid-template-columns: 18px auto auto minmax(0, 1fr) auto;
  align-items: center;
  gap: 6px;
  min-height: 30px;
  border-bottom: 1px solid var(--border);
  font-size: 13px;
}

.voice-row:last-child {
  border-bottom: 0;
}

.voice-rank {
  color: var(--pressed-text);
  font-size: 12px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.voice-duration {
  font-variant-numeric: tabular-nums;
}

.voice-dot {
  color: var(--muted);
}

.voice-app {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.voice-date {
  overflow: hidden;
  color: var(--muted);
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

@media (max-width: 980px) {
  .profile-kpis {
    grid-template-columns: repeat(2, minmax(170px, 1fr));
  }

  .profile-columns {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
