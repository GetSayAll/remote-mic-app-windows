<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import {
  deleteAllTranscripts,
  deleteTranscript,
  deleteTranscriptApplication,
  deleteTranscriptRecording,
  exportTranscriptAudio,
  getTranscriptAudio,
  getTranscriptHistory,
  setTranscriptAudioRecordingEnabled,
  setTranscriptHistoryEnabled,
  subscribeTranscriptCaptured,
  subscribeTranscriptRecordingFinished,
  type TranscriptApplicationSummary,
  type TranscriptHistorySnapshot,
  type TranscriptRecordView,
  type TranscriptRecordingView,
} from "../lib/bridge";
import {
  displayApplicationName,
  formatCount,
  formatDayLabel,
  formatRecordTime,
  formatRecordingCount,
  formatRecordingDuration,
  formatTimestamp,
  groupTranscriptsByDay,
  REFLECTIONS_RECENT_MS,
  visibleTranscripts,
} from "./reflections-view";
import { t } from "../lib/i18n";

const UNKNOWN_APPLICATION_KEY = "__unknown__";

type DeleteRequest =
  | { kind: "record"; record: TranscriptRecordView }
  | { kind: "recording"; recording: TranscriptRecordingView }
  | { kind: "application"; application: TranscriptApplicationSummary }
  | { kind: "all" };

interface RecordingDayGroup {
  key: string;
  dateKey: string;
  recordings: TranscriptRecordingView[];
}

const snapshot = ref<TranscriptHistorySnapshot | null>(null);
const loading = ref(true);
const loadingCardVisible = ref(false);
const loadError = ref("");
const operationError = ref("");
const busy = ref(false);
const deleting = ref(false);
const selectedApplicationKey = ref<string | null>(null);
const applicationSwitcherExpanded = ref(false);
const expandedDayKeys = ref<Set<string>>(new Set());
const copiedRecordId = ref<string | null>(null);
const deleteRequest = ref<DeleteRequest | null>(null);
const deleteDialog = ref<HTMLDialogElement | null>(null);
const nowMs = ref(Date.now());
const togglesSettled = ref(false);

let copiedTimer: ReturnType<typeof setTimeout> | undefined;
let togglesTimer: ReturnType<typeof setTimeout> | undefined;
let loadingTimer: ReturnType<typeof setTimeout> | undefined;
let nowTimer: ReturnType<typeof setInterval> | undefined;
let unlistenTranscriptCaptured: (() => void) | undefined;
let unlistenTranscriptRecordingFinished: (() => void) | undefined;
let disposed = false;

const applications = computed(() => snapshot.value?.applications ?? []);
const selectedApplication = computed(
  () => applications.value.find((app) => app.key === selectedApplicationKey.value) ?? null,
);
const recordsForView = computed(() => {
  const records = snapshot.value?.records ?? [];
  return visibleTranscripts(records, selectedApplicationKey.value, nowMs.value);
});
const dayGroups = computed(() => groupTranscriptsByDay(recordsForView.value));
const textSessionIds = computed(
  () => new Set((snapshot.value?.records ?? []).map((record) => record.sessionId)),
);
const recordingOnlyForView = computed(() => {
  const cutoff = nowMs.value - REFLECTIONS_RECENT_MS;
  return (snapshot.value?.recordings ?? [])
    .filter((recording) => !textSessionIds.value.has(recording.sessionId))
    .filter((recording) =>
      selectedApplicationKey.value
        ? (recording.applicationKey ?? UNKNOWN_APPLICATION_KEY) === selectedApplicationKey.value
        : recording.endedAtMs >= cutoff,
    )
    .slice()
    .sort((left, right) => right.endedAtMs - left.endedAtMs);
});
const recordingDayGroups = computed<RecordingDayGroup[]>(() => {
  const groups = new Map<string, TranscriptRecordingView[]>();
  for (const recording of recordingOnlyForView.value) {
    const existing = groups.get(recording.localDateKey) ?? [];
    existing.push(recording);
    groups.set(recording.localDateKey, existing);
  }
  return [...groups.entries()]
    .map(([dateKey, recordings]) => ({
      key: `recording-${dateKey}`,
      dateKey,
      recordings: recordings.slice().sort((left, right) => right.endedAtMs - left.endedAtMs),
    }))
    .sort((left, right) => {
      const leftNewest = left.recordings[0]?.endedAtMs ?? 0;
      const rightNewest = right.recordings[0]?.endedAtMs ?? 0;
      return rightNewest - leftNewest;
    });
});
const visibleCount = computed(
  () => recordsForView.value.length + recordingOnlyForView.value.length,
);
const totalCount = computed(() => snapshot.value?.totalCount ?? 0);
const recentCount = computed(() => snapshot.value?.recentCount ?? 0);
const hasRecords = computed(() => totalCount.value > 0);
const hasEarlierEntries = computed(
  () => selectedApplicationKey.value === null && totalCount.value > recentCount.value,
);
const noRecentRecords = computed(
  () => hasRecords.value && visibleCount.value === 0 && selectedApplicationKey.value === null,
);
const enabled = computed(() => snapshot.value?.enabled ?? false);
const audioEnabled = computed(() => snapshot.value?.audioEnabled ?? false);
const recordingsBySession = computed(() => {
  const map = new Map<string, TranscriptRecordingView>();
  for (const recording of snapshot.value?.recordings ?? []) {
    map.set(recording.sessionId, recording);
  }
  return map;
});
const playingSessionId = ref<string | null>(null);
let playingAudio: HTMLAudioElement | null = null;
let playbackAttempt = 0;
let loadAttempt = 0;
const visibleApplications = computed(() => {
  const list = applications.value;
  return applicationSwitcherExpanded.value ? list : list.slice(0, 4);
});
const selectedTitle = computed(() =>
  selectedApplication.value
    ? displayApplicationName(selectedApplication.value.name)
    : t("全部记录"),
);
const selectedCountLabel = computed(() =>
  formatCount(selectedApplication.value?.count ?? visibleCount.value),
);
const privacyText = computed(
  () =>
    `${t("文字和原始录音仅在对应开关开启时保存在本机；不保存输入框全文、窗口标题、文档名或网址。")}\n${t("删除内容会移到回收站，可手动恢复。")}`,
);
const dialogTitle = computed(() => {
  switch (deleteRequest.value?.kind) {
    case "application":
      return t("删除这个应用的全部记录？");
    case "all":
      return t("删除全部语音记录？");
    case "recording":
      return t("删除这段录音？");
    default:
      return t("删除这条语音记录？");
  }
});
const dialogMessage = computed(() => {
  const request = deleteRequest.value;
  if (!request) return "";
  switch (request.kind) {
    case "application":
      return t("“{name}”的全部语音记录将移到回收站。", {
        name: displayApplicationName(request.application.name),
      });
    case "all":
      return t("所有应用的语音记录都将移到回收站。");
    case "recording":
      return t("录音将移到回收站，不会影响其他记录。");
    default:
      return recordingFor(request.record)
        ? t("记录和关联的录音将一起移到回收站，不会影响已经输入到目标应用中的文字。")
        : t("记录将移到回收站，不会影响已经输入到目标应用中的文字。");
  }
});

function applicationFor(record: TranscriptRecordView): TranscriptApplicationSummary | null {
  return (
    applications.value.find((app) => app.key === record.applicationKey) ?? null
  );
}

function applicationForRecording(
  recording: TranscriptRecordingView,
): TranscriptApplicationSummary | null {
  return (
    applications.value.find(
      (app) => app.key === (recording.applicationKey ?? UNKNOWN_APPLICATION_KEY),
    ) ?? null
  );
}

function recordingApplicationName(recording: TranscriptRecordingView): string {
  return displayApplicationName(
    applicationForRecording(recording)?.name ??
      recording.applicationName ??
      t("未知应用"),
  );
}

function recordingTime(recording: TranscriptRecordingView): string {
  return formatTimestamp(recording.endedAtMs, recording.utcOffsetMinutes);
}

function newestExpandableDayKeys(): string[] {
  const keys: string[] = [];
  const newestText = dayGroups.value[0]?.key;
  if (newestText) keys.push(newestText);
  const newestRecording = recordingDayGroups.value[0]?.dateKey;
  if (newestRecording) keys.push(`recording-${newestRecording}`);
  return keys;
}

function normalizeSelection(): void {
  if (
    selectedApplicationKey.value &&
    !applications.value.some((app) => app.key === selectedApplicationKey.value)
  ) {
    selectedApplicationKey.value = null;
  }
}

function normalizeExpandedDays(): void {
  const valid = new Set([
    ...dayGroups.value.map((group) => group.key),
    ...recordingDayGroups.value.map((group) => group.key),
  ]);
  const next = new Set([...expandedDayKeys.value].filter((key) => valid.has(key)));
  if (next.size === 0) {
    for (const key of newestExpandableDayKeys()) next.add(key);
  }
  expandedDayKeys.value = next;
}

async function loadHistory(): Promise<void> {
  const attempt = ++loadAttempt;
  const initial = snapshot.value === null;
  // 首次进入才显示整页加载卡；后续（语音捕获事件触发的）刷新在后台静默完成，
  // 否则每次有数据变化都会闪一下加载态。首次读取通常几毫秒，延迟 150 ms 再显示
  // 加载卡，避免切回页面时闪一下。
  if (initial) {
    loading.value = true;
    if (loadingTimer) clearTimeout(loadingTimer);
    loadingTimer = setTimeout(() => {
      loadingCardVisible.value = true;
      loadingTimer = undefined;
    }, 150);
  }
  loadError.value = "";
  try {
    const next = await getTranscriptHistory();
    if (attempt !== loadAttempt) return;
    snapshot.value = next;
    nowMs.value = Date.now();
    normalizeSelection();
    normalizeExpandedDays();
  } catch (error) {
    if (attempt !== loadAttempt) return;
    loadError.value = error instanceof Error ? error.message : String(error);
  } finally {
    if (attempt === loadAttempt) {
      if (initial) {
        loading.value = false;
        if (loadingTimer) {
          clearTimeout(loadingTimer);
          loadingTimer = undefined;
        }
        loadingCardVisible.value = false;
      }
      if (!togglesSettled.value) {
        await nextTick();
        if (togglesTimer) clearTimeout(togglesTimer);
        togglesTimer = setTimeout(() => {
          togglesSettled.value = true;
          togglesTimer = undefined;
        }, 50);
      }
    }
  }
}

async function toggleEnabled(event: Event): Promise<void> {
  const input = event.target as HTMLInputElement;
  const next = input.checked;
  if (busy.value) return;
  busy.value = true;
  operationError.value = "";
  try {
    snapshot.value = await setTranscriptHistoryEnabled(next);
  } catch {
    input.checked = !next;
    operationError.value = t("保存回眸设置失败，请稍后重试。");
  } finally {
    busy.value = false;
  }
}

async function toggleAudioEnabled(event: Event): Promise<void> {
  const input = event.target as HTMLInputElement;
  const next = input.checked;
  if (busy.value) return;
  busy.value = true;
  operationError.value = "";
  try {
    snapshot.value = await setTranscriptAudioRecordingEnabled(next);
  } catch {
    input.checked = !next;
    operationError.value = t("保存原始录音设置失败，请稍后重试。");
  } finally {
    busy.value = false;
  }
}

function recordingFor(record: TranscriptRecordView): TranscriptRecordingView | null {
  return recordingsBySession.value.get(record.sessionId) ?? null;
}

function stopPlayback(): void {
  playbackAttempt += 1;
  const audio = playingAudio;
  playingAudio = null;
  playingSessionId.value = null;
  if (audio) {
    // Clearing src fires an error in WebView2. Detach first so a normal end
    // (or the user stopping playback) is never reported as a failed play.
    audio.onended = null;
    audio.onerror = null;
    audio.pause();
    audio.src = "";
  }
}

async function togglePlaybackSession(sessionId: string): Promise<void> {
  if (playingSessionId.value === sessionId) {
    stopPlayback();
    return;
  }
  stopPlayback();
  const attempt = playbackAttempt;
  operationError.value = "";
  try {
    const source = await getTranscriptAudio(sessionId);
    if (attempt !== playbackAttempt) return;
    const audio = new Audio(source);
    audio.onended = () => {
      if (playingAudio !== audio) return;
      stopPlayback();
    };
    audio.onerror = () => {
      if (playingAudio !== audio) return;
      stopPlayback();
      operationError.value = t("无法播放这段录音，请稍后重试。");
    };
    playingAudio = audio;
    playingSessionId.value = sessionId;
    await audio.play();
  } catch {
    if (attempt !== playbackAttempt) return;
    stopPlayback();
    operationError.value = t("无法播放这段录音，请稍后重试。");
  }
}

function togglePlayback(record: TranscriptRecordView): Promise<void> {
  return togglePlaybackSession(record.sessionId);
}

function toggleRecordingPlayback(recording: TranscriptRecordingView): Promise<void> {
  return togglePlaybackSession(recording.sessionId);
}

async function exportRecordingSession(sessionId: string): Promise<void> {
  operationError.value = "";
  try {
    await exportTranscriptAudio(sessionId);
  } catch {
    operationError.value = t("导出录音失败，请稍后重试。");
  }
}

function exportRecording(record: TranscriptRecordView): Promise<void> {
  return exportRecordingSession(record.sessionId);
}

function exportRecordingAsset(recording: TranscriptRecordingView): Promise<void> {
  return exportRecordingSession(recording.sessionId);
}

function selectApplication(key: string | null): void {
  selectedApplicationKey.value = key;
  expandedDayKeys.value = new Set();
  normalizeExpandedDays();
}

function toggleDay(key: string): void {
  const next = new Set(expandedDayKeys.value);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  expandedDayKeys.value = next;
}

function isDayExpanded(key: string): boolean {
  return expandedDayKeys.value.has(key);
}

async function copyRecord(record: TranscriptRecordView): Promise<void> {
  operationError.value = "";
  try {
    if (!navigator.clipboard?.writeText) {
      throw new Error("clipboard unavailable");
    }
    await navigator.clipboard.writeText(record.text);
    copiedRecordId.value = record.id;
    if (copiedTimer) clearTimeout(copiedTimer);
    copiedTimer = setTimeout(() => {
      copiedRecordId.value = null;
      copiedTimer = undefined;
    }, 1_500);
  } catch {
    operationError.value = t("复制失败，请检查剪贴板权限后重试。");
  }
}

function openDelete(request: DeleteRequest): void {
  deleteRequest.value = request;
  void nextTick(() => {
    if (!deleteDialog.value?.open) {
      deleteDialog.value?.showModal();
    }
  });
}

function closeDelete(): void {
  deleteRequest.value = null;
  if (deleteDialog.value?.open) {
    deleteDialog.value.close();
  }
}

async function confirmDelete(): Promise<void> {
  const request = deleteRequest.value;
  if (!request || deleting.value) return;
  deleting.value = true;
  operationError.value = "";
  stopPlayback();
  try {
    if (request.kind === "record") {
      snapshot.value = await deleteTranscript(request.record.id);
    } else if (request.kind === "recording") {
      snapshot.value = await deleteTranscriptRecording(request.recording.sessionId);
    } else if (request.kind === "application") {
      snapshot.value = await deleteTranscriptApplication(request.application.key);
    } else {
      snapshot.value = await deleteAllTranscripts();
    }
    closeDelete();
    normalizeSelection();
    normalizeExpandedDays();
  } catch {
    operationError.value =
      request.kind === "recording"
        ? t("删除录音失败，录音仍保留。请稍后重试。")
        : t("删除失败，记录仍保留。请稍后重试。");
    closeDelete();
  } finally {
    deleting.value = false;
  }
}

watch(selectedApplicationKey, () => {
  expandedDayKeys.value = new Set();
});

watch(dayGroups, () => {
  normalizeExpandedDays();
});

watch(recordingDayGroups, () => {
  normalizeExpandedDays();
});

onMounted(() => {
  void loadHistory();
  void subscribeTranscriptCaptured(() => {
    if (!disposed) void loadHistory();
  })
    .then((unlisten) => {
      if (disposed) unlisten();
      else unlistenTranscriptCaptured = unlisten;
    })
    .catch(() => undefined);
  void subscribeTranscriptRecordingFinished(() => {
    if (!disposed) void loadHistory();
  })
    .then((unlisten) => {
      if (disposed) unlisten();
      else unlistenTranscriptRecordingFinished = unlisten;
    })
    .catch(() => undefined);
  nowTimer = setInterval(() => {
    nowMs.value = Date.now();
  }, 60_000);
});

onUnmounted(() => {
  disposed = true;
  stopPlayback();
  if (copiedTimer) clearTimeout(copiedTimer);
  if (togglesTimer) clearTimeout(togglesTimer);
  if (loadingTimer) clearTimeout(loadingTimer);
  if (nowTimer) clearInterval(nowTimer);
  unlistenTranscriptCaptured?.();
  unlistenTranscriptRecordingFinished?.();
});
</script>

<template>
  <section class="reflections-page">
    <header class="page-header reflections-header">
      <div class="reflections-title">
        <h1>{{ t("回眸") }}</h1>
        <p class="privacy-note">{{ privacyText }}</p>
      </div>
      <div class="record-toggles" :class="{ settled: togglesSettled }">
        <label class="toggle-row record-toggle">
          <span>{{ t("记录回眸") }}</span>
          <input
            class="toggle-input"
            type="checkbox"
            :checked="enabled"
            :disabled="busy || loading"
            @change="toggleEnabled"
          />
        </label>
        <label
          class="toggle-row record-toggle"
          :class="{ disabled: !enabled }"
          :title="!enabled ? t('先开启记录回眸') : undefined"
        >
          <span>{{ t("保存原始录音") }}</span>
          <input
            class="toggle-input"
            type="checkbox"
            :checked="audioEnabled"
            :disabled="busy || loading || !enabled"
            @change="toggleAudioEnabled"
          />
        </label>
      </div>
    </header>

    <p v-if="loadError" class="page-error" role="alert">
      {{ t("读取回眸记录失败：{error}", { error: loadError }) }}
      <button class="link-button" type="button" @click="loadHistory">{{ t("重试") }}</button>
    </p>
    <p v-if="operationError" class="page-error" role="alert">{{ operationError }}</p>
    <p v-if="snapshot?.archiveError" class="page-error" role="alert">
      {{ snapshot.archiveError }}
    </p>

    <div v-if="loadingCardVisible" class="card reflections-loading">{{ t("正在读取回眸记录…") }}</div>
    <div
      v-else-if="loading && snapshot === null"
      class="reflections-loading-placeholder"
      aria-hidden="true"
    ></div>

    <div v-else-if="!hasRecords" class="card empty-card">
      <div class="empty-inner">
        <svg class="empty-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
          <path d="M4.2 4.2h15.6a2 2 0 0 1 2 2v8.9a2 2 0 0 1-2 2h-6.9L8 20.6v-3.5H4.2a2 2 0 0 1-2-2V6.2a2 2 0 0 1 2-2z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>
          <path d="M6.4 8.3h11.2M6.4 11.7h7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
        </svg>
        <p v-if="enabled">{{ t("还没有语音记录。下一次完成语音输入后会显示在这里。") }}</p>
        <p v-else>
          {{ t("开启后，新的语音转文字结果会保存在本机。") }}<br />{{ t("已有记录不会因关闭开关而删除。") }}
        </p>
      </div>
    </div>

    <template v-else>
      <div class="history-main">
        <div class="history-summary">
          <div class="history-summary-copy">
            <h2>{{ selectedTitle }}</h2>
            <p>{{ selectedCountLabel }}</p>
          </div>
          <div class="history-summary-actions">
            <button
              v-if="selectedApplication"
              class="danger-link"
              type="button"
              @click="openDelete({ kind: 'application', application: selectedApplication })"
            >
              {{ t("删除当前应用") }}
            </button>
            <button
              class="link-button"
              type="button"
              @click="applicationSwitcherExpanded = !applicationSwitcherExpanded"
            >
              {{ applicationSwitcherExpanded ? t("收起") : t("展开") }}
              <svg
                class="chevron"
                :class="{ up: applicationSwitcherExpanded }"
                viewBox="0 0 24 24"
                fill="none"
                aria-hidden="true"
              >
                <path d="m6 9 6 6 6-6" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
              </svg>
            </button>
          </div>
        </div>

        <div class="card app-switcher">
          <div :class="applicationSwitcherExpanded ? 'app-switcher-grid' : 'app-switcher-row'">
            <button
              class="app-chip"
              :class="{ selected: selectedApplicationKey === null, 'app-chip-all': !applicationSwitcherExpanded }"
              type="button"
              @click="selectApplication(null)"
            >
              <span class="app-icon-all" aria-hidden="true">
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none">
                  <rect x="3" y="3" width="7.2" height="7.2" rx="2" fill="currentColor"/>
                  <rect x="13.8" y="3" width="7.2" height="7.2" rx="2" fill="currentColor"/>
                  <rect x="3" y="13.8" width="7.2" height="7.2" rx="2" fill="currentColor"/>
                  <rect x="13.8" y="13.8" width="7.2" height="7.2" rx="2" fill="currentColor"/>
                </svg>
              </span>
              <span class="app-chip-label">
                <strong>{{ t("全部应用") }}</strong>
                <span>{{ recentCount }}</span>
              </span>
            </button>
            <span
              v-if="!applicationSwitcherExpanded"
              class="app-divider"
              aria-hidden="true"
            ></span>
            <button
              v-for="application in visibleApplications"
              :key="application.key"
              class="app-chip"
              :class="{ selected: selectedApplicationKey === application.key, 'app-chip-app': !applicationSwitcherExpanded }"
              type="button"
              @click="selectApplication(application.key)"
            >
              <span class="app-icon-frame">
                <img
                  v-if="application.iconDataUrl"
                  :src="application.iconDataUrl"
                  alt=""
                />
                <span v-else class="app-icon-fallback" aria-hidden="true">
                  <svg width="17" height="17" viewBox="0 0 24 24" fill="none">
                    <rect x="4" y="4" width="16" height="16" rx="4" stroke="currentColor" stroke-width="1.6"/>
                    <path d="M8.5 9.5h7M8.5 13h4.5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
                  </svg>
                </span>
              </span>
              <span class="app-chip-label">
                <strong>{{ displayApplicationName(application.name) }}</strong>
                <span>{{ application.count }}</span>
              </span>
            </button>
          </div>
        </div>

        <button
          v-if="hasEarlierEntries"
          class="link-button more-by-app"
          type="button"
          @click="applicationSwitcherExpanded = true"
        >
          <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path d="M3.2 12a8.8 8.8 0 1 0 2.6-6.2M3.2 4.6v5h5M12 7.2v5.1l3.2 1.9" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>
          </svg>
          {{ t("按应用查看更多记录") }}
        </button>

        <div v-if="noRecentRecords" class="card no-recent-card">
          {{ t("最近 7 天没有记录。请在应用栏选择具体应用查看更早记录。") }}
        </div>

        <div v-if="dayGroups.length > 0" class="timeline">
          <section
            v-for="group in dayGroups"
            :key="group.key"
            class="day-group"
          >
            <button
              class="day-header"
              :class="{ open: isDayExpanded(group.key) }"
              type="button"
              @click="toggleDay(group.key)"
            >
              <svg class="chevron" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <path
                  v-if="isDayExpanded(group.key)"
                  d="m6 9 6 6 6-6"
                  stroke="currentColor"
                  stroke-width="2"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                />
                <path
                  v-else
                  d="m9 6 6 6-6 6"
                  stroke="currentColor"
                  stroke-width="2"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                />
              </svg>
              <span class="day-title">{{ formatDayLabel(group.key) }}</span>
              <span class="day-separator">·</span>
              <span class="day-count">{{ formatCount(group.records.length) }}</span>
            </button>
            <div class="day-rule"></div>
            <div v-if="isDayExpanded(group.key)" class="timeline-rows">
              <div
                v-for="record in group.records"
                :key="record.id"
                class="transcript-row"
              >
                <span class="row-time">{{ formatRecordTime(record) }}</span>
                <span class="row-app">
                  <span class="row-app-icon">
                    <img
                      v-if="applicationFor(record)?.iconDataUrl"
                      :src="applicationFor(record)?.iconDataUrl ?? ''"
                      alt=""
                    />
                    <span v-else class="app-icon-fallback small" aria-hidden="true">
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none">
                        <rect x="4" y="4" width="16" height="16" rx="4" stroke="currentColor" stroke-width="1.6"/>
                        <path d="M8.5 9.5h7M8.5 13h4.5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
                      </svg>
                    </span>
                  </span>
                  <span class="row-app-name">{{ displayApplicationName(record.applicationName) }}</span>
                </span>
                <p class="row-text">{{ record.text }}</p>
                <span class="row-actions">
                  <template v-if="recordingFor(record)">
                    <button
                      class="icon-button"
                      type="button"
                      :title="playingSessionId === record.sessionId ? t('停止播放') : t('播放录音')"
                      :aria-label="playingSessionId === record.sessionId ? t('停止播放') : t('播放录音')"
                      @click="togglePlayback(record)"
                    >
                      <svg v-if="playingSessionId === record.sessionId" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                        <path d="M8 5.5v13M16 5.5v13" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"/>
                      </svg>
                      <svg v-else viewBox="0 0 24 24" fill="none" aria-hidden="true">
                        <path d="M8 5.2 18.5 12 8 18.8V5.2Z" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"/>
                      </svg>
                    </button>
                    <button
                      class="icon-button"
                      type="button"
                      :title="t('导出录音')"
                      :aria-label="t('导出录音')"
                      @click="exportRecording(record)"
                    >
                      <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
                        <path d="M12 4v10m0 0 4-4m-4 4-4-4M5 18.5h14" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>
                      </svg>
                    </button>
                  </template>
                  <button
                    class="icon-button"
                    type="button"
                    :title="copiedRecordId === record.id ? t('已复制') : t('复制这条记录')"
                    :aria-label="copiedRecordId === record.id ? t('已复制') : t('复制这条记录')"
                    @click="copyRecord(record)"
                  >
                    <svg v-if="copiedRecordId === record.id" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                      <path d="m5 12 4.2 4.2L19 6.5" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/>
                    </svg>
                    <svg v-else viewBox="0 0 24 24" fill="none" aria-hidden="true">
                      <rect x="8.5" y="8.5" width="11" height="11" rx="2" stroke="currentColor" stroke-width="1.7"/>
                      <path d="M15.5 8.5V6.8A2.3 2.3 0 0 0 13.2 4.5H6.8A2.3 2.3 0 0 0 4.5 6.8v6.4a2.3 2.3 0 0 0 2.3 2.3h1.7" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>
                    </svg>
                  </button>
                  <button
                    class="icon-button danger"
                    type="button"
                    :title="t('删除这条记录')"
                    :aria-label="t('删除这条记录')"
                    @click="openDelete({ kind: 'record', record })"
                  >
                    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
                      <path d="M4.5 7h15M9.5 7V4.8h5V7M7 7l.8 12.2h8.4L17 7M10 10.5v5.5M14 10.5v5.5" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>
                    </svg>
                  </button>
                </span>
              </div>
            </div>
          </section>
        </div>

        <div v-if="recordingDayGroups.length > 0" class="timeline recording-timeline">
          <div class="recording-section-heading">
            <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <path d="M4 12h2.2l1.8-5 3 10 2.4-7 1.8 4H20" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>
            </svg>
            <span>{{ t("仅录音") }}</span>
          </div>
          <section
            v-for="group in recordingDayGroups"
            :key="group.key"
            class="day-group"
          >
            <button
              class="day-header"
              :class="{ open: isDayExpanded(group.key) }"
              type="button"
              @click="toggleDay(group.key)"
            >
              <svg class="chevron" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <path
                  v-if="isDayExpanded(group.key)"
                  d="m6 9 6 6 6-6"
                  stroke="currentColor"
                  stroke-width="2"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                />
                <path
                  v-else
                  d="m9 6 6 6-6 6"
                  stroke="currentColor"
                  stroke-width="2"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                />
              </svg>
              <span class="day-title">{{ formatDayLabel(group.dateKey) }}</span>
              <span class="day-separator">·</span>
              <span class="day-count">{{ formatRecordingCount(group.recordings.length) }}</span>
            </button>
            <div class="day-rule"></div>
            <div v-if="isDayExpanded(group.key)" class="timeline-rows">
              <div
                v-for="recording in group.recordings"
                :key="recording.sessionId"
                class="transcript-row recording-row"
              >
                <span class="row-time">{{ recordingTime(recording) }}</span>
                <span class="row-app">
                  <span class="row-app-icon">
                    <img
                      v-if="applicationForRecording(recording)?.iconDataUrl"
                      :src="applicationForRecording(recording)?.iconDataUrl ?? ''"
                      alt=""
                    />
                    <span v-else class="app-icon-fallback small" aria-hidden="true">
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none">
                        <rect x="4" y="4" width="16" height="16" rx="4" stroke="currentColor" stroke-width="1.6"/>
                        <path d="M8.5 9.5h7M8.5 13h4.5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
                      </svg>
                    </span>
                  </span>
                  <span class="row-app-name">{{ recordingApplicationName(recording) }}</span>
                </span>
                <span class="row-recording">
                  <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
                    <path d="M4 12h2.2l1.8-5 3 10 2.4-7 1.8 4H20" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>
                  </svg>
                  {{ t("录音") }} · {{ formatRecordingDuration(recording.durationMs) }}
                </span>
                <span class="row-actions">
                  <button
                    class="icon-button"
                    type="button"
                    :title="playingSessionId === recording.sessionId ? t('停止播放') : t('播放录音')"
                    :aria-label="playingSessionId === recording.sessionId ? t('停止播放') : t('播放录音')"
                    @click="toggleRecordingPlayback(recording)"
                  >
                    <svg v-if="playingSessionId === recording.sessionId" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                      <path d="M8 5.5v13M16 5.5v13" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"/>
                    </svg>
                    <svg v-else viewBox="0 0 24 24" fill="none" aria-hidden="true">
                      <path d="M8 5.2 18.5 12 8 18.8V5.2Z" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"/>
                    </svg>
                  </button>
                  <button
                    class="icon-button"
                    type="button"
                    :title="t('导出录音')"
                    :aria-label="t('导出录音')"
                    @click="exportRecordingAsset(recording)"
                  >
                    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
                      <path d="M12 4v10m0 0 4-4m-4 4-4-4M5 18.5h14" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>
                    </svg>
                  </button>
                  <button
                    class="icon-button danger"
                    type="button"
                    :title="t('删除录音')"
                    :aria-label="t('删除录音')"
                    @click="openDelete({ kind: 'recording', recording })"
                  >
                    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
                      <path d="M4.5 7h15M9.5 7V4.8h5V7M7 7l.8 12.2h8.4L17 7M10 10.5v5.5M14 10.5v5.5" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>
                    </svg>
                  </button>
                </span>
              </div>
            </div>
          </section>
        </div>
      </div>

      <footer class="history-footer">
        <p>{{ t("把语音实际写入目标输入框的文字和原始录音保存在本机，并按应用和日期整理。") }}</p>
        <button class="danger-link" type="button" @click="openDelete({ kind: 'all' })">
          {{ t("全部删除") }}
        </button>
      </footer>
    </template>

    <dialog
      ref="deleteDialog"
      class="delete-dialog"
      aria-labelledby="reflections-delete-title"
      @cancel.prevent="closeDelete"
    >
      <h3 id="reflections-delete-title">{{ dialogTitle }}</h3>
      <p>{{ dialogMessage }}</p>
      <div class="dialog-actions">
        <button class="secondary-button" type="button" :disabled="deleting" @click="closeDelete">
          {{ t("取消") }}
        </button>
        <button class="danger-button" type="button" :disabled="deleting" @click="confirmDelete">
          {{ deleting ? t("删除中…") : t("删除") }}
        </button>
      </div>
    </dialog>
  </section>
</template>

<style scoped>
.reflections-page {
  min-height: calc(100vh - 48px);
  display: flex;
  flex-direction: column;
}
.reflections-header {
  align-items: center;
  gap: 14px;
}
.reflections-title {
  display: flex;
  align-items: center;
  gap: 14px;
  min-width: 0;
  flex: 1 1 auto;
}
.reflections-title h1 {
  flex: none;
  margin: 0;
}
.privacy-note {
  max-width: 566px;
  margin: 0;
  color: var(--muted);
  font-size: 12.5px;
  line-height: 1.4;
  letter-spacing: 0.01em;
  white-space: pre-line;
}
.record-toggles {
  display: flex;
  flex: none;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 6px 14px;
  margin-left: 6px;
}
.record-toggle {
  flex: none;
  font-size: 13.5px;
  font-weight: 600;
}
.record-toggle.disabled {
  opacity: 0.55;
}
.record-toggles:not(.settled) .toggle-input,
.record-toggles:not(.settled) .toggle-input::after {
  transition: none;
}
.page-error {
  margin: 0 0 12px;
  padding: 9px 12px;
  border: 1px solid var(--border);
  border-radius: 9px;
  color: var(--error-text);
  background: var(--error-surface);
  font-size: 13px;
}
.page-error .link-button {
  margin-left: 8px;
}
.reflections-loading {
  min-height: 120px;
  display: grid;
  place-items: center;
  color: var(--muted);
  font-size: 13.5px;
}
.reflections-loading-placeholder {
  min-height: 120px;
}
.history-main {
  display: flex;
  flex-direction: column;
  gap: 14px;
  flex: 1 1 auto;
}
.history-summary {
  display: flex;
  align-items: flex-start;
  gap: 16px;
  padding: 2px 4px 0;
}
.history-summary-copy {
  /* 页头 h1 有全局 14px 左内边距；这里补上父级 4px 之外的 10px，保持「回眸」与「全部记录」左对齐。 */
  padding-left: 10px;
}
.history-summary-copy h2 {
  margin: 0;
  font-size: 20px;
  line-height: 1.2;
  letter-spacing: -0.3px;
}
.history-summary-copy p {
  margin: 3px 0 0;
  color: var(--muted);
  font-size: 12.5px;
}
.history-summary-actions {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 16px;
  min-height: 26px;
  padding-top: 2px;
}
.link-button,
.danger-link {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  min-height: 26px;
  padding: 0;
  border: 0;
  background: transparent;
  cursor: pointer;
  font-size: 12.5px;
  font-weight: 600;
  white-space: nowrap;
}
.link-button {
  color: var(--accent-text);
}
.link-button:hover {
  color: var(--accent-dark);
}
.danger-link {
  color: var(--danger-text, var(--error-text));
}
.danger-link:hover {
  color: var(--danger-hover, var(--error-text));
}
.chevron {
  width: 14px;
  height: 14px;
  flex: none;
}
.chevron.up {
  transform: rotate(180deg);
}
.app-switcher {
  padding: 12px 14px;
  border-radius: 13px;
}
.app-switcher-row {
  display: flex;
  align-items: stretch;
  gap: 8px;
  min-width: 0;
  overflow: hidden;
}
.app-switcher-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 8px;
}
.app-chip {
  min-width: 0;
  height: 48px;
  display: flex;
  align-items: center;
  gap: 9px;
  flex: 0 0 auto;
  padding: 7px 9px;
  border: 0;
  border-radius: 10px;
  color: var(--text-primary);
  background: transparent;
  cursor: pointer;
  text-align: left;
}
.app-chip:hover {
  background: var(--surface-subtle);
}
.app-chip.selected {
  color: var(--accent-text);
  background: var(--accent-surface);
}
.app-chip-all {
  width: 156px;
}
.app-chip-app {
  width: 148px;
}
.app-switcher-grid .app-chip {
  width: auto;
}
.app-divider {
  width: 1px;
  flex: none;
  align-self: center;
  height: 40px;
  background: var(--border);
}
.app-icon-frame,
.row-app-icon {
  width: 32px;
  height: 32px;
  flex: none;
  display: grid;
  place-items: center;
}
.app-icon-frame img,
.row-app-icon img {
  width: 100%;
  height: 100%;
  object-fit: contain;
  display: block;
}
.row-app-icon {
  width: 24px;
  height: 24px;
}
.app-icon-fallback {
  width: 32px;
  height: 32px;
  display: grid;
  place-items: center;
  border-radius: 9px;
  color: var(--muted);
  background: var(--surface-subtle);
  border: 1px solid var(--border);
}
.app-icon-fallback.small {
  width: 24px;
  height: 24px;
  border-radius: 7px;
}
.app-icon-all {
  width: 32px;
  height: 32px;
  border-radius: 8px;
  display: grid;
  place-items: center;
  color: var(--accent-text);
  background: var(--accent-surface-strong);
}
.app-chip-label {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.app-chip-label strong {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
  font-weight: 600;
  line-height: 1.15;
}
.app-chip-label span {
  color: var(--muted);
  font-size: 12px;
  line-height: 1.15;
}
.app-chip.selected .app-chip-label span {
  color: var(--accent-muted-text);
}
.more-by-app {
  align-self: flex-start;
  margin: -2px 0 0 6px;
}
.no-recent-card {
  color: var(--muted);
  font-size: 13px;
}
.timeline {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-width: 0;
}
.day-group {
  min-width: 0;
}
.day-header {
  width: 100%;
  min-height: 36px;
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 8px 4px;
  border: 0;
  background: transparent;
  color: var(--muted);
  cursor: pointer;
  text-align: left;
}
.day-header:hover {
  color: var(--text-primary);
}
.day-header.open {
  color: var(--accent-text);
}
.day-header .chevron {
  width: 14px;
  height: 14px;
}
.day-title {
  font-size: 14px;
  font-weight: 600;
  line-height: 1.2;
}
.day-separator {
  color: var(--text-secondary);
  opacity: 0.7;
  font-size: 12px;
}
.day-count {
  font-size: 12px;
  font-weight: 500;
  color: var(--muted);
}
.day-header.open .day-count {
  color: var(--accent-muted-text);
}
.day-rule {
  height: 1px;
  background: var(--border);
}
.recording-section-heading {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 8px 4px 2px;
  color: var(--warning-text);
  font-size: 12.5px;
  font-weight: 600;
}
.recording-section-heading svg {
  width: 16px;
  height: 16px;
}
.recording-timeline .timeline-rows::before {
  background: var(--warning);
}
.recording-timeline .day-header.open {
  color: var(--warning-text);
}
.recording-timeline .day-header.open .day-count {
  color: var(--warning-text);
}
.recording-row::before {
  background: var(--warning);
}
.row-recording {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  color: var(--warning-text);
  font-size: 13px;
  line-height: 1.55;
}
.row-recording svg {
  width: 15px;
  height: 15px;
  flex: none;
}
.timeline-rows {
  position: relative;
  padding-left: 28px;
}
.timeline-rows::before {
  content: "";
  position: absolute;
  left: 13px;
  top: 0;
  bottom: 0;
  width: 1px;
  background: var(--accent);
  opacity: 0.72;
}
.transcript-row {
  position: relative;
  display: grid;
  grid-template-columns: 54px 126px minmax(0, 1fr) auto;
  align-items: start;
  gap: 12px;
  padding: 9px 0;
  border-bottom: 1px solid var(--border);
}
.transcript-row:last-child {
  border-bottom: 0;
}
.transcript-row::before {
  content: "";
  position: absolute;
  left: -19px;
  top: 17px;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--accent);
}
.row-time {
  color: var(--muted);
  font-size: 12px;
  line-height: 1.5;
  font-variant-numeric: tabular-nums;
  letter-spacing: 0.02em;
  white-space: nowrap;
}
.row-app {
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding-top: 1px;
}
.row-app-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12.5px;
  font-weight: 600;
  line-height: 1.35;
}
.row-text {
  margin: 0;
  min-width: 0;
  color: var(--text-primary);
  font-size: 13.5px;
  line-height: 1.55;
  overflow-wrap: anywhere;
  white-space: pre-wrap;
  user-select: text;
}
.row-actions {
  display: flex;
  align-items: center;
  gap: 3px;
  padding-top: 1px;
  opacity: 0.72;
}
.transcript-row:hover .row-actions {
  opacity: 1;
}
.icon-button {
  width: 28px;
  height: 28px;
  display: grid;
  place-items: center;
  padding: 0;
  border: 0;
  border-radius: 7px;
  color: var(--muted);
  background: transparent;
  cursor: pointer;
}
.icon-button:hover {
  color: var(--text-primary);
  background: var(--surface-subtle);
}
.icon-button.danger:hover {
  color: var(--danger-text, var(--error-text));
  background: var(--error-surface);
}
.icon-button svg {
  width: 16px;
  height: 16px;
  display: block;
}
.empty-card {
  min-height: 176px;
  display: grid;
  place-items: center;
  padding: 24px;
  border-radius: 13px;
}
.empty-inner {
  max-width: 460px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  text-align: center;
}
.empty-icon {
  width: 38px;
  height: 38px;
  color: var(--empty-icon);
}
.empty-inner p {
  margin: 0;
  color: var(--muted);
  font-size: 13px;
  line-height: 1.5;
}
.history-footer {
  display: flex;
  align-items: flex-start;
  gap: 18px;
  margin-top: auto;
  padding: 14px 8px 0;
}
.history-footer p {
  max-width: 690px;
  margin: 0;
  color: var(--muted);
  font-size: 12.5px;
  line-height: 1.45;
}
.history-footer .danger-link {
  margin-left: auto;
  flex: none;
  padding-top: 1px;
}
.delete-dialog {
  width: min(420px, calc(100vw - 48px));
  padding: 20px;
  border: 1px solid var(--border-strong);
  border-radius: 14px;
  color: var(--text-primary);
  background: var(--surface-canvas);
  box-shadow: 0 20px 60px rgba(10, 12, 20, 0.26);
}
.delete-dialog::backdrop {
  background: rgba(0, 0, 0, 0.53);
}
.delete-dialog h3 {
  margin: 0;
  font-size: 16px;
  line-height: 1.3;
}
.delete-dialog p {
  margin: 9px 0 0;
  color: var(--muted);
  font-size: 13px;
  line-height: 1.55;
}
.dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 20px;
}
.danger-button {
  min-height: 30px;
  padding: 5px 12px;
  border: 1px solid transparent;
  border-radius: 8px;
  color: #fff;
  background: var(--danger, #b3261e);
  cursor: pointer;
  font-size: 13.5px;
}
.danger-button:hover {
  background: var(--danger-hover, #8c1d18);
}
@media (max-width: 1000px) {
  .privacy-note {
    max-width: 430px;
  }
  .app-switcher-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .transcript-row {
    grid-template-columns: 54px 112px minmax(0, 1fr) auto;
  }
}
</style>
