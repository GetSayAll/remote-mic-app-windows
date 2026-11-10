<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from "vue";
import DraftRules from "./DraftRules.vue";
import { t } from "../lib/i18n";
import {
  armLocalWhisper,
  canUseLocalWhisper,
  cancelLiveLocalWhisper,
  cancelLocalWhisper,
  getLocalWhisperStatus,
  pickLocalWhisperFile,
  transcribeLocalWhisper,
  type WhisperDraft,
  type WhisperFileKind,
  type WhisperLanguage,
} from "../lib/local-whisper";

const available = canUseLocalWhisper();
const files = ref<Record<WhisperFileKind, string>>({ executable: "", model: "", audio: "" });
const language = ref<WhisperLanguage>("zh");
const picking = ref(false);
const job = ref<"file" | "remote" | null>(null);
const running = computed(() => job.value !== null);
const cancelling = ref(false);
const status = ref<"idle" | "ready" | "cancelled" | "expired" | "copied">("idle");
const error = ref("");
const draft = ref("");
const draftRevision = ref(0);
const lastResult = ref<WhisperDraft | null>(null);
const pendingDraft = ref<WhisperDraft | null>(null);
const livePhase = ref<"arming" | "armed" | "capturing" | "transcribing">("arming");

interface LiveRun {
  token: number | null;
  timer: ReturnType<typeof setTimeout> | null;
  polling: boolean;
  cancelRequested: boolean;
}

const STATUS_POLL_MS = 300;
let liveRun: LiveRun | null = null;
let disposed = false;

const fileOptions: ReadonlyArray<{ kind: WhisperFileKind; label: string }> = [
  { kind: "executable", label: "选择转写程序" },
  { kind: "model", label: "选择模型" },
  { kind: "audio", label: "选择 WAV 音频" },
];
const canArm = computed(() =>
  available && !picking.value && !running.value && !pendingDraft.value &&
  Boolean(files.value.executable && files.value.model),
);
const canStart = computed(() => canArm.value && Boolean(files.value.audio));

const errors: Record<string, string> = {
  whisper_access_denied: "请在 SayAll 主窗口打开本地转写。",
  whisper_busy: "已有转写在运行，请稍后再试。",
  whisper_picker_failed: "无法打开文件选择器。",
  whisper_invalid_path: "无法读取所选文件，请重新选择本机文件。",
  whisper_unverified_engine: "无法验证转写程序。请选择受支持的 Whisper 程序，并保留同包 DLL。",
  whisper_invalid_model: "无法验证 Whisper 模型，请重新选择受支持的模型文件。",
  whisper_invalid_wav: "请选择 16 kHz、16 位、单声道 PCM WAV 音频。",
  whisper_audio_too_long: "这段音频超出转写长度范围。请选择 0.2 至 60 秒的音频。",
  whisper_timed_out: "本地转写超时，请缩短音频后重试。",
  whisper_start_failed: "无法启动 Whisper 转写程序。",
  whisper_engine_failed: "Whisper 转写失败，请核对程序、模型和音频格式。",
  whisper_invalid_output: "Whisper 返回的文字无法读取。",
  whisper_empty_output: "没有识别出文字，请检查音频内容。",
  whisper_io_error: "无法准备本地转写所需的文件。",
  whisper_remote_unavailable: "遥控器语音会话不可用。请确认 Chromecast 遥控器已连接，然后重试。",
};
const errorText = computed(() =>
  error.value ? t(errors[error.value] ?? "本地转写失败，请稍后重试。") : "",
);

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

function publishDraft(result: WhisperDraft) {
  draft.value = result.text;
  lastResult.value = result;
  draftRevision.value += 1;
  status.value = "ready";
}

function receiveDraft(result: WhisperDraft) {
  if (lastResult.value && draft.value !== lastResult.value.text) {
    pendingDraft.value = result;
  } else {
    publishDraft(result);
  }
}

function resolvePendingDraft(replace: boolean) {
  const result = pendingDraft.value;
  pendingDraft.value = null;
  if (replace && result) publishDraft(result);
}

async function chooseFile(kind: WhisperFileKind) {
  if (picking.value || running.value) return;
  picking.value = true;
  error.value = "";
  try {
    const path = await pickLocalWhisperFile(kind);
    if (path && !disposed) files.value[kind] = path;
  } catch (cause) {
    if (!disposed) error.value = String(cause);
  } finally {
    picking.value = false;
  }
}

async function transcribe() {
  if (!canStart.value) return;
  job.value = "file";
  status.value = "idle";
  error.value = "";
  try {
    const result = await transcribeLocalWhisper({
      executable: files.value.executable,
      model: files.value.model,
      wav: files.value.audio,
      language: language.value,
    });
    if (disposed) return;
    receiveDraft(result);
  } catch (cause) {
    if (disposed) return;
    if (String(cause) === "whisper_cancelled") {
      status.value = "cancelled";
    } else {
      error.value = String(cause);
    }
  } finally {
    job.value = null;
    cancelling.value = false;
  }
}

function stopPolling(run: LiveRun) {
  if (run.timer === null) return;
  clearTimeout(run.timer);
  run.timer = null;
}

function finishLiveRun(run: LiveRun) {
  stopPolling(run);
  liveRun = null;
  job.value = null;
  cancelling.value = false;
}

function isCurrent(run: LiveRun): boolean {
  return !disposed && liveRun === run;
}

function schedulePoll(run: LiveRun) {
  if (!isCurrent(run) || run.cancelRequested || run.polling || run.timer !== null) return;
  run.timer = setTimeout(() => {
    run.timer = null;
    void pollLiveRun(run);
  }, STATUS_POLL_MS);
}

async function pollLiveRun(run: LiveRun) {
  if (!isCurrent(run) || run.cancelRequested || run.polling || run.token === null) return;
  run.polling = true;
  try {
    const result = await getLocalWhisperStatus(run.token);
    if (!isCurrent(run) || run.cancelRequested) return;
    switch (result.phase) {
      case "armed":
      case "capturing":
      case "transcribing":
        livePhase.value = result.phase;
        break;
      case "ready":
        if (!result.draft) {
          error.value = "whisper_invalid_output";
        } else {
          receiveDraft(result.draft);
        }
        finishLiveRun(run);
        break;
      case "failed":
        error.value = result.error ?? "whisper_engine_failed";
        finishLiveRun(run);
        break;
      case "cancelled":
      case "expired":
        status.value = result.phase;
        finishLiveRun(run);
        break;
    }
  } catch (cause) {
    if (!isCurrent(run) || run.cancelRequested) return;
    error.value = String(cause);
    finishLiveRun(run);
    void cancelLiveLocalWhisper(run.token).catch(() => {});
  } finally {
    run.polling = false;
    schedulePoll(run);
  }
}

async function cancelLiveRun(run: LiveRun) {
  if (run.token === null) return;
  try {
    // A false response means the token is already terminal or stale. Either
    // way, discard this result locally after the user asked to cancel.
    await cancelLiveLocalWhisper(run.token);
    if (!isCurrent(run)) return;
    status.value = "cancelled";
    finishLiveRun(run);
  } catch {
    if (!isCurrent(run)) return;
    run.cancelRequested = false;
    cancelling.value = false;
    error.value = "whisper_cancel_failed";
    schedulePoll(run);
  }
}

async function armRemote() {
  if (!canArm.value) return;
  const run: LiveRun = { token: null, timer: null, polling: false, cancelRequested: false };
  liveRun = run;
  job.value = "remote";
  livePhase.value = "arming";
  status.value = "idle";
  error.value = "";
  try {
    const { token } = await armLocalWhisper({
      executable: files.value.executable,
      model: files.value.model,
      language: language.value,
    });
    run.token = token;
    if (!isCurrent(run)) {
      void cancelLiveLocalWhisper(token).catch(() => {});
      return;
    }
    if (run.cancelRequested) {
      await cancelLiveRun(run);
    } else {
      livePhase.value = "armed";
      void pollLiveRun(run);
    }
  } catch (cause) {
    if (!isCurrent(run)) return;
    if (run.cancelRequested) {
      status.value = "cancelled";
    } else {
      error.value = String(cause);
    }
    finishLiveRun(run);
  }
}

async function cancel() {
  if (!running.value || cancelling.value) return;
  cancelling.value = true;
  if (liveRun) {
    liveRun.cancelRequested = true;
    stopPolling(liveRun);
    await cancelLiveRun(liveRun);
    return;
  }
  try {
    if (!await cancelLocalWhisper()) cancelling.value = false;
  } catch {
    cancelling.value = false;
    if (!disposed) error.value = "whisper_cancel_failed";
  }
}

function onCardToggle(event: Event) {
  if (!(event.currentTarget as HTMLDetailsElement).open && liveRun) void cancel();
}

async function copyDraft() {
  if (!draft.value.trim()) return;
  error.value = "";
  try {
    await navigator.clipboard.writeText(draft.value);
    if (!disposed) status.value = "copied";
  } catch {
    if (!disposed) error.value = "whisper_copy_failed";
  }
}

onBeforeUnmount(() => {
  disposed = true;
  if (liveRun) {
    stopPolling(liveRun);
    if (liveRun.token !== null && !liveRun.cancelRequested) {
      void cancelLiveLocalWhisper(liveRun.token).catch(() => {});
    }
    liveRun = null;
    return;
  }
  if (running.value && !cancelling.value) void cancelLocalWhisper().catch(() => {});
});
</script>

<template>
  <details class="card whisper-card" @toggle="onCardToggle">
    <summary>{{ t("本地 Whisper 转写") }}</summary>
    <div class="whisper-content">
      <p v-if="!available" role="status">{{ t("本地转写需要在 SayAll Windows 客户端中使用。") }}</p>
      <template v-else>
        <p>{{ t("用本机模型把 WAV 音频转成可编辑草稿。选好文件后，点击开始转写。") }}</p>
        <p>{{ t("也可以等待下一次 Chromecast 听写。点击等待后，按住遥控器语音键说话，松开生成草稿。") }}</p>
        <p>
          {{ t("支持 whisper.cpp v1.9.1 的 Windows x64 CPU 版和 ggml-base-q5_1.bin 模型。") }}
          {{ t("音频需为 16 kHz、16 位、单声道 PCM WAV，时长 0.2 至 60 秒。") }}
        </p>
        <div class="whisper-inputs">
          <div v-for="{ kind, label } in fileOptions" :key="kind" class="whisper-file">
            <button type="button" class="secondary-button" :data-whisper-file="kind"
              :disabled="picking || running" @click="chooseFile(kind)">{{ t(label) }}</button>
            <span :title="fileName(files[kind])">
              {{ files[kind] ? fileName(files[kind]) : t("尚未选择") }}
            </span>
          </div>
          <label class="whisper-language">
            {{ t("转写语言") }}
            <select v-model="language" :disabled="running">
              <option value="zh">{{ t("中文") }}</option>
              <option value="en">English</option>
              <option value="auto">{{ t("自动识别") }}</option>
            </select>
          </label>
        </div>
        <div class="whisper-actions">
          <button type="button" class="primary-button" data-whisper-start :disabled="!canStart" @click="transcribe">
            {{ t("开始转写") }}
          </button>
          <button type="button" class="secondary-button" data-whisper-arm :disabled="!canArm" @click="armRemote">
            {{ t("等待遥控器听写") }}
          </button>
          <button v-if="running" type="button" class="secondary-button" data-whisper-cancel
            :disabled="cancelling" @click="cancel">{{ t("取消转写") }}</button>
        </div>
        <p v-if="running" role="status">
          {{ cancelling ? t("正在取消转写…")
            : job === "file" || livePhase === "transcribing" ? t("正在本机转写…")
            : livePhase === "arming" ? t("正在验证本地模型和遥控器…")
            : livePhase === "capturing" ? t("正在接收遥控器语音。松开语音键结束。")
            : t("请按住遥控器语音键说话。") }}
        </p>
        <p v-else-if="status === 'cancelled'" role="status">{{ t("已取消转写。") }}</p>
        <p v-else-if="status === 'expired'" role="status">{{ t("本次遥控器听写已超时，请重新点击等待。") }}</p>
        <p v-else-if="status === 'copied'" role="status">{{ t("草稿已复制。") }}</p>
        <p v-else-if="status === 'ready' && lastResult" role="status">
          {{ t("转写完成。音频 {duration} 秒，耗时 {elapsed} 秒。", {
            duration: (lastResult.durationMs / 1000).toFixed(1),
            elapsed: (lastResult.elapsedMs / 1000).toFixed(1),
          }) }}
        </p>
        <p v-if="error" role="alert" class="whisper-error">
          {{ error === "whisper_copy_failed" ? t("无法复制草稿，你可以手动选中文字。")
            : error === "whisper_cancel_failed" ? t("取消请求失败，请重试。") : errorText }}
        </p>
        <label v-if="lastResult" class="whisper-draft">
          {{ t("转写草稿") }}
          <textarea v-model="draft" rows="5" data-whisper-draft
            :disabled="running" @input="status = 'idle'" />
        </label>
        <button v-if="lastResult" type="button" class="secondary-button" data-whisper-copy
          :disabled="running || !draft.trim()" @click="copyDraft">{{ t("复制草稿") }}</button>
        <div v-if="pendingDraft" class="whisper-pending">
          <p role="status">{{ t("新转写已完成。当前草稿有改动，请选择要保留的内容。") }}</p>
          <pre>{{ pendingDraft.text }}</pre>
          <div class="whisper-actions">
            <button type="button" class="secondary-button" data-whisper-keep
              @click="resolvePendingDraft(false)">{{ t("保留当前草稿") }}</button>
            <button type="button" class="primary-button" data-whisper-replace
              @click="resolvePendingDraft(true)">{{ t("使用新转写") }}</button>
          </div>
        </div>
        <DraftRules v-model="draft" :draft-revision="draftRevision"
          :disabled="running || Boolean(pendingDraft)" />
      </template>
    </div>
  </details>
</template>

<style scoped>
.whisper-card { padding: 14px 16px; }
.whisper-card summary { cursor: pointer; font-weight: 600; }
.whisper-content, .whisper-inputs, .whisper-draft { display: grid; gap: 10px; }
.whisper-content { margin-top: 12px; justify-items: start; }
.whisper-content p { margin: 0; color: var(--text-secondary); font-size: 13px; }
.whisper-file, .whisper-actions, .whisper-language { display: flex; gap: 10px; align-items: center; min-width: 0; }
.whisper-actions { flex-wrap: wrap; }
.whisper-inputs, .whisper-draft { width: 100%; }
.whisper-file button { flex: none; }
.whisper-file span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.whisper-draft textarea, .whisper-language select {
  padding: 8px;
  border: 1px solid var(--border);
  border-radius: 8px;
  color: var(--text-primary);
  background: var(--surface-control);
}
.whisper-draft textarea { resize: vertical; }
.whisper-content .whisper-error { color: var(--error-text); }
.whisper-pending { display: grid; gap: 8px; width: 100%; }
.whisper-pending pre { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
