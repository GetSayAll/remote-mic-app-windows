<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import AppFocusProfileEditor from "../components/AppFocusProfileEditor.vue";
import RegisteredAppsDialog from "../components/RegisteredAppsDialog.vue";
import {
  buttonLabel,
  buttonTriggerLabel,
  getButtonMappings,
  getCombinedActions,
  listPresetApps,
  saveButtonMappings,
  type AppFocusProfile,
  type ButtonMappings,
  type ButtonTrigger,
  type MacroDefinition,
  type PresetAppInfo,
  type RemoteButton,
  type RuntimeSnapshot,
} from "../lib/bridge";
import {
  buildAppRows,
  compactFocusLabel,
  effectiveAppProfile,
  findAppBindings,
  findMacroReferences,
  mergeApplications,
  nextSelectedTarget,
  profileForTarget,
  removeAppAdaptation,
  type AppAdaptationRow,
} from "../lib/app-adaptation";
import { presetFocusDefault } from "../lib/focus-defaults";
import { t, tPage } from "../lib/i18n";
import type { NavigationRequest } from "../navigation";

const props = withDefaults(
  defineProps<{ runtime: RuntimeSnapshot | null; initialTarget?: string | null }>(),
  { initialTarget: null },
);
const emit = defineEmits<{ navigate: [request: string | NavigationRequest] }>();

/** 选中项在页面卸载后仍保留到本次窗口关闭（对齐 Mac 的窗口内会话选择）。 */
let lastSelectedTarget: string | null = null;

const mappings = ref<ButtonMappings>({ enabled: true, actions: {} });
const presetApps = ref<PresetAppInfo[]>([]);
const macroDefinitions = ref<MacroDefinition[]>([]);
const selectedTarget = ref<string | null>(null);
const loading = ref(true);
const saving = ref(false);
const loadError = ref<string | null>(null);
const addOpen = ref(false);
const addError = ref<string | null>(null);
const confirmDelete = ref(false);
const notice = ref<{ text: string; tone: "success" | "error" } | null>(null);
let noticeTimer: number | null = null;

const rows = computed(() =>
  buildAppRows(
    mappings.value.applications ?? [],
    mappings.value.focusProfiles,
    presetApps.value,
    props.initialTarget ? [props.initialTarget] : [],
  ),
);
const selectedRow = computed<AppAdaptationRow | null>(
  () => rows.value.find((row) => row.target === selectedTarget.value) ?? null,
);
const selectedProfile = computed<AppFocusProfile | null>(() => {
  const row = selectedRow.value;
  return row ? profileForTarget(mappings.value.focusProfiles, row.target) : null;
});
const fallbackProfile = computed<AppFocusProfile | null>(() => {
  const row = selectedRow.value;
  return row?.preset ? presetFocusDefault(row.target, true) : null;
});
const bindings = computed(() => {
  const row = selectedRow.value;
  return row ? findAppBindings(mappings.value.actions, row.target) : [];
});
const macroReferences = computed(() => {
  const row = selectedRow.value;
  return row && !row.preset ? findMacroReferences(macroDefinitions.value, row.target) : [];
});
const deleteBlockedReason = computed<string | null>(() => {
  const row = selectedRow.value;
  if (!row || row.preset) return null;
  if (bindings.value.length) {
    return t("还有 {count} 处绑定在使用它，先解绑才能删除适配。", {
      count: bindings.value.length,
    });
  }
  if (macroReferences.value.length) {
    return t("还有 {count} 个组合动作在使用它，先移除引用才能删除适配。", {
      count: macroReferences.value.length,
    });
  }
  return null;
});

function showNotice(text: string, tone: "success" | "error" = "success"): void {
  notice.value = { text, tone };
  if (noticeTimer !== null) window.clearTimeout(noticeTimer);
  noticeTimer = window.setTimeout(() => {
    notice.value = null;
    noticeTimer = null;
  }, 3200);
}

function selectRow(target: string): void {
  selectedTarget.value = target;
}

async function load(): Promise<void> {
  loading.value = true;
  loadError.value = null;
  try {
    const [saved, presets, macros] = await Promise.all([
      getButtonMappings(),
      listPresetApps().catch(() => [] as PresetAppInfo[]),
      getCombinedActions()
        .then((document) => document.definitions)
        .catch(() => [] as MacroDefinition[]),
    ]);
    mappings.value = saved;
    presetApps.value = presets;
    macroDefinitions.value = macros;
    selectedTarget.value = nextSelectedTarget(
      rows.value,
      props.initialTarget ?? lastSelectedTarget,
    );
  } catch (error) {
    console.warn("load app adaptation page failed", error);
    loadError.value = t("无法读取 APP 适配设置，请稍后重试。");
  } finally {
    loading.value = false;
  }
}

function mappingPayload(next: ButtonMappings): ButtonMappings {
  return {
    ...next,
    focusProfiles:
      next.focusProfiles && Object.keys(next.focusProfiles).length
        ? next.focusProfiles
        : undefined,
  };
}

async function persist(next: ButtonMappings): Promise<boolean> {
  if (saving.value) return false;
  saving.value = true;
  try {
    mappings.value = await saveButtonMappings(mappingPayload(next));
    return true;
  } catch (error) {
    console.warn("save app adaptation failed", error);
    showNotice(t("保存失败，请重试。"), "error");
    return false;
  } finally {
    saving.value = false;
  }
}

async function saveProfile(target: string, profile: AppFocusProfile | null): Promise<boolean> {
  const focusProfiles = { ...(mappings.value.focusProfiles ?? {}) };
  const expected = target.toLocaleLowerCase();
  for (const key of Object.keys(focusProfiles)) {
    if (key.toLocaleLowerCase() === expected) delete focusProfiles[key];
  }
  if (profile) focusProfiles[target] = profile;
  return persist({ ...mappings.value, focusProfiles });
}

async function addApps(apps: Array<{ name: string; path: string }>): Promise<void> {
  addError.value = null;
  if (!apps.length) {
    addOpen.value = false;
    return;
  }
  const applications = mergeApplications(mappings.value.applications ?? [], apps);
  if (await persist({ ...mappings.value, applications })) {
    addOpen.value = false;
    selectedTarget.value = apps[0].path;
    showNotice(t("已添加 {count} 个应用，按键绑定未改变。", { count: apps.length }));
  } else {
    addError.value = t("保存应用列表失败，请重试。");
  }
}

async function deleteAdaptation(): Promise<void> {
  const row = selectedRow.value;
  if (!row || deleteBlockedReason.value) return;
  const next = removeAppAdaptation(mappings.value, row.target, row.preset);
  if (await persist(next)) {
    selectedTarget.value = nextSelectedTarget(rows.value, null);
    confirmDelete.value = false;
    showNotice(row.preset ? t("已恢复默认聚焦方式") : t("已删除这个 APP 的适配"));
  }
}

function goToBinding(button: RemoteButton, trigger: ButtonTrigger): void {
  emit("navigate", {
    page: "buttons",
    params: { button, trigger },
  });
}

function tileInitial(name: string): string {
  const trimmed = name.trim();
  if (!trimmed) return "?";
  const latin = /^[A-Za-z]/.exec(trimmed);
  return (latin ? latin[0] : trimmed[0]).toLocaleUpperCase();
}

const TILE_COLORS = [
  "linear-gradient(145deg, #3eb575, #16834b)",
  "linear-gradient(145deg, #4d5565, #20242d)",
  "linear-gradient(145deg, #4aa8e8, #1667a8)",
  "linear-gradient(145deg, #e08a4a, #a84f16)",
  "linear-gradient(145deg, #9a6ad6, #6236a8)",
];
function tileStyle(name: string): { background: string } {
  let hash = 0;
  for (const char of name) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return { background: TILE_COLORS[hash % TILE_COLORS.length] };
}

watch(selectedTarget, (target) => {
  lastSelectedTarget = target;
  confirmDelete.value = false;
});

watch(
  () => props.initialTarget,
  (target) => {
    if (target) {
      selectedTarget.value = nextSelectedTarget(rows.value, target);
    }
  },
);

onMounted(() => void load());

onUnmounted(() => {
  if (noticeTimer !== null) window.clearTimeout(noticeTimer);
  noticeTimer = null;
});
</script>

<template>
  <section class="apps-page">
    <header class="page-header">
      <div>
        <h1>{{ tPage("APP 适配") }}</h1>
        <p class="page-subtitle">{{ t("输入框聚焦按 APP 生效，APP 在前台即可使用。") }}</p>
      </div>
    </header>

    <p v-if="notice" class="page-notice" :class="notice.tone" role="status">{{ notice.text }}</p>
    <div v-if="loadError" class="card load-error" role="alert">{{ loadError }}</div>

    <div v-if="loading" class="card loading-card" aria-busy="true">{{ t("正在读取 APP 适配设置…") }}</div>

    <article v-else-if="!rows.length" class="card empty-card">
      <span class="empty-icon" aria-hidden="true">
        <svg viewBox="0 0 24 24" fill="none">
          <rect x="3" y="3" width="7.4" height="7.4" rx="1.6" stroke="currentColor" stroke-width="1.8" />
          <rect x="13.6" y="3" width="7.4" height="7.4" rx="1.6" stroke="currentColor" stroke-width="1.8" />
          <rect x="3" y="13.6" width="7.4" height="7.4" rx="1.6" stroke="currentColor" stroke-width="1.8" />
          <path d="M17.3 13.8v7.4M13.6 17.5h7.4" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" />
        </svg>
      </span>
      <h2>{{ t("还没有添加任何 APP") }}</h2>
      <p class="muted">
        {{ t("添加后可以为它单独设置输入框聚焦；只要该 APP 在前台就能用。") }}
      </p>
      <button class="primary-button" type="button" @click="addOpen = true">{{ t("添加 APP") }}</button>
    </article>

    <div v-else class="master-detail">
      <section class="panel list-panel" :aria-label="t('已添加的 APP')">
        <h2 class="list-title">{{ t("已添加的 APP") }}</h2>
        <div class="app-list">
          <button
            v-for="row in rows"
            :key="row.target"
            type="button"
            class="app-row"
            :class="{ selected: selectedTarget === row.target }"
            :aria-current="selectedTarget === row.target ? 'true' : undefined"
            @click="selectRow(row.target)"
          >
            <span class="app-tile" :style="tileStyle(row.name)" aria-hidden="true">{{ tileInitial(row.name) }}</span>
            <span class="app-row-text">
              <span class="app-row-name">
                {{ row.name }}
                <span v-if="row.preset" class="inline-badge">{{ t("内置") }}</span>
              </span>
              <span class="app-row-status">
                {{ t(compactFocusLabel(effectiveAppProfile(row, mappings.focusProfiles))) }}
                <template v-if="!row.installed"> · {{ t("应用未找到") }}</template>
              </span>
            </span>
          </button>
        </div>
        <button class="add-app" type="button" @click="addOpen = true">
          <span aria-hidden="true">＋</span>{{ t("添加 APP") }}
        </button>
      </section>

      <section v-if="selectedRow" class="panel detail-panel" :aria-label="t('{name} 的适配设置', { name: selectedRow.name })">
        <div class="app-head">
          <span class="app-tile large" :style="tileStyle(selectedRow.name)" aria-hidden="true">{{ tileInitial(selectedRow.name) }}</span>
          <div class="app-head-text">
            <h2>
              {{ selectedRow.name }}
              <span v-if="selectedRow.preset" class="inline-badge">{{ t("内置预设") }}</span>
            </h2>
            <span class="app-path">{{ selectedRow.path ?? t("内置预设应用") }}</span>
          </div>
        </div>

        <p v-if="!selectedRow.installed" class="missing-note" role="status">
          {{ t("应用未找到，请重新添加。") }}
        </p>

        <p class="scope-note">
          {{ t("输入框聚焦按 APP 单独设置：只要该 APP 在前台就能用，与“打开应用”动作无关。") }}
        </p>

        <AppFocusProfileEditor
          :target="selectedRow.target"
          :profile="selectedProfile"
          :fallback="fallbackProfile"
          :disabled="!selectedRow.installed || saving"
          :save="(profile) => saveProfile(selectedRow!.target, profile)"
          @notice="showNotice"
        />

        <section class="section-card binding-card">
          <div class="section-head">
            <h3>{{ t("绑定到它的按键") }}</h3>
            <span class="badge neutral">{{ t("{count} 处", { count: bindings.length }) }}</span>
            <span class="spacer"></span>
          </div>
          <p v-if="!bindings.length" class="hint">
            {{ t("还没有按键绑定这个 APP，可在按键页的“打开应用”里选择它。") }}
          </p>
          <div v-else class="binding-list">
            <button
              v-for="binding in bindings"
              :key="`${binding.button}-${binding.trigger}`"
              class="binding-row"
              type="button"
              @click="goToBinding(binding.button, binding.trigger)"
            >
              <span class="binding-key">{{ buttonLabel(binding.button) }} · {{ buttonTriggerLabel(binding.trigger) }}</span>
              <span class="binding-action">{{ t("打开应用") }}</span>
              <span class="spacer"></span>
              <span class="binding-go">{{ t("去按键页") }}</span>
            </button>
          </div>
        </section>

        <div class="delete-row">
          <p v-if="deleteBlockedReason" class="danger-line">{{ deleteBlockedReason }}</p>
          <template v-else>
            <button class="danger-button" type="button" :disabled="saving" @click="confirmDelete = true">
              {{ selectedRow.preset ? t("恢复默认聚焦方式") : t("删除这个 APP 的适配") }}
            </button>
            <span class="hint">
              {{ selectedRow.preset ? t("只清除 SayAll 保存的显式聚焦设置，不会修改 APP。") : t("只删除 SayAll 保存的适配，不会卸载 APP。") }}
            </span>
          </template>
        </div>
      </section>
    </div>

    <RegisteredAppsDialog
      v-if="addOpen"
      :known-apps="mappings.applications ?? []"
      :saving="saving"
      :save-error="addError"
      @close="addOpen = false"
      @add="addApps"
    />

    <div v-if="confirmDelete && selectedRow" class="dialog-backdrop" @click.self="confirmDelete = false">
      <section class="confirm-dialog" role="dialog" aria-modal="true" :aria-labelledby="'delete-app-title'">
        <h2 id="delete-app-title">
          {{ selectedRow.preset ? t("恢复「{name}」的默认聚焦？", { name: selectedRow.name }) : t("删除「{name}」的适配？", { name: selectedRow.name }) }}
        </h2>
        <p class="muted">
          {{
            selectedRow.preset
              ? t("只会清除 SayAll 保存的显式聚焦设置；APP 本身不会被修改或卸载。")
              : t("只会删除 SayAll 保存的适配设置（输入框学习记录、聚焦快捷键），不会卸载或改动 APP 本身。")
          }}
        </p>
        <div class="dialog-actions">
          <button class="secondary-button" type="button" :disabled="saving" @click="confirmDelete = false">{{ t("取消") }}</button>
          <button class="danger-button" type="button" :disabled="saving" @click="deleteAdaptation">
            {{ selectedRow.preset ? t("恢复默认") : t("删除适配") }}
          </button>
        </div>
      </section>
    </div>
  </section>
</template>

<style scoped>
.apps-page {
  padding-bottom: 24px;
}
.apps-page .page-header h1 {
  font-size: 24px;
}
.page-notice {
  margin: 0 0 10px;
  padding: 8px 12px;
  border-radius: 8px;
  font-size: 13px;
}
.page-notice.success {
  color: var(--success-text);
  background: var(--success-surface);
}
.page-notice.error {
  color: var(--error-text);
  background: var(--error-surface);
}
.load-error {
  color: var(--error-text);
  font-size: 13px;
}
.loading-card {
  color: var(--muted);
  font-size: 13px;
}
.empty-card {
  min-height: 320px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  text-align: center;
}
.empty-card h2 {
  margin: 2px 0 0;
}
.empty-card p {
  max-width: 520px;
  margin: 0;
  font-size: 13px;
}
.empty-icon {
  width: 48px;
  height: 48px;
  display: grid;
  place-items: center;
  border-radius: 13px;
  color: var(--accent-text);
  background: var(--accent-surface);
}
.empty-icon svg {
  width: 26px;
  height: 26px;
}
.master-detail {
  display: flex;
  align-items: flex-start;
  gap: 14px;
}
.panel {
  border: 1px solid var(--border);
  border-radius: 12px;
  background: var(--card);
  box-shadow: 0 8px 30px var(--shadow-card);
}
.list-panel {
  width: 228px;
  flex: 0 0 228px;
  padding: 12px;
  display: flex;
  flex-direction: column;
}
.list-title {
  margin: 0 0 9px 2px;
  font-size: 16px;
}
.app-list {
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.app-row {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 7px 8px;
  border: 1px solid transparent;
  border-radius: 8px;
  background: rgba(31, 36, 48, 0.035);
  color: var(--text-primary);
  text-align: left;
  cursor: pointer;
}
.app-row:hover {
  border-color: var(--border-strong);
}
.app-row.selected {
  border-color: rgba(95, 98, 219, 0.5);
  background: rgba(95, 98, 219, 0.11);
}
.app-tile {
  width: 26px;
  height: 26px;
  flex: 0 0 26px;
  display: grid;
  place-items: center;
  border-radius: 7px;
  color: #fff;
  font-size: 12px;
  font-weight: 700;
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.22);
}
.app-tile.large {
  width: 36px;
  height: 36px;
  flex-basis: 36px;
  border-radius: 9px;
  font-size: 15px;
}
.app-row-text {
  min-width: 0;
  flex: 1 1 auto;
  display: grid;
  gap: 1px;
}
.app-row-name {
  overflow: hidden;
  font-size: 12px;
  font-weight: 600;
  line-height: 1.25;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.app-row-status {
  overflow: hidden;
  color: var(--muted);
  font-size: 10px;
  line-height: 1.25;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.app-row.selected .app-row-status {
  color: var(--accent-text);
  opacity: 0.78;
}
.inline-badge {
  display: inline-flex;
  margin-left: 4px;
  padding: 0 5px;
  border-radius: 999px;
  color: var(--text-secondary);
  background: rgba(31, 36, 48, 0.06);
  font-size: 9px;
  font-weight: 600;
  line-height: 15px;
  vertical-align: 1px;
}
.add-app {
  min-height: 32px;
  margin-top: 8px;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  border: 1px dashed var(--border-strong);
  border-radius: 8px;
  background: transparent;
  color: var(--text-control);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
}
.add-app:hover {
  border-color: var(--accent-border);
  color: var(--accent-text);
  background: var(--accent-surface);
}
.detail-panel {
  flex: 1 1 auto;
  min-width: 0;
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.app-head {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 36px;
}
.app-head-text {
  min-width: 0;
  display: grid;
  gap: 2px;
}
.app-head-text h2 {
  margin: 0;
  font-size: 19px;
  line-height: 1.2;
}
.app-path {
  overflow: hidden;
  color: var(--muted);
  font-size: 11px;
  line-height: 1.35;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.missing-note {
  margin: 0;
  padding: 7px 10px;
  border-radius: 7px;
  color: var(--warning-text);
  background: var(--warning-surface);
  font-size: 12px;
}
.scope-note {
  margin: 0;
  color: var(--muted);
  font-size: 12px;
  line-height: 1.45;
}
.section-card {
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface-subtle);
}
.section-head {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 20px;
}
.section-head h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
}
.section-head .spacer {
  flex: 1 1 auto;
}
.badge {
  display: inline-flex;
  align-items: center;
  min-height: 20px;
  padding: 2px 7px;
  border-radius: 999px;
  font-size: 11px;
  font-weight: 600;
  white-space: nowrap;
}
.badge.neutral {
  color: var(--text-secondary);
  background: rgba(31, 36, 48, 0.06);
}
.hint {
  margin: 6px 0 0;
  color: var(--muted);
  font-size: 11.5px;
  line-height: 1.4;
}
.binding-list {
  display: flex;
  flex-direction: column;
  gap: 5px;
  margin-top: 7px;
}
.binding-row {
  min-height: 32px;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 9px;
  border: 0;
  border-radius: 7px;
  background: var(--surface-raised);
  color: var(--text-primary);
  text-align: left;
  cursor: pointer;
}
.binding-row:hover {
  background: var(--accent-surface);
}
.binding-key {
  font-size: 12px;
  font-weight: 600;
  white-space: nowrap;
}
.binding-action {
  color: var(--muted);
  font-size: 11.5px;
  white-space: nowrap;
}
.binding-row .spacer {
  flex: 1 1 auto;
}
.binding-go {
  color: var(--accent-text);
  font-size: 11.5px;
  font-weight: 600;
  white-space: nowrap;
}
.delete-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
  min-height: 30px;
}
.danger-line {
  margin: 0;
  color: var(--muted);
  font-size: 11.5px;
  line-height: 1.4;
}
.danger-button {
  min-height: 28px;
  padding: 4px 10px;
  border: 0;
  border-radius: 7px;
  color: var(--error-text);
  background: var(--error-surface);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
}
.danger-button:hover:not(:disabled) {
  filter: brightness(0.97);
}
.danger-button:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}
.dialog-backdrop {
  position: fixed;
  inset: 0;
  z-index: 20;
  display: grid;
  place-items: center;
  padding: 20px;
  background: rgba(0, 0, 0, 0.45);
}
.confirm-dialog {
  width: min(440px, calc(100vw - 40px));
  padding: 18px;
  border: 1px solid var(--border-strong);
  border-radius: 10px;
  background: var(--surface-canvas);
  box-shadow: 0 18px 50px rgba(0, 0, 0, 0.24);
}
.confirm-dialog h2 {
  margin: 0 0 8px;
  font-size: 17px;
}
.confirm-dialog p {
  margin: 0;
  font-size: 13px;
  line-height: 1.5;
}
.dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 16px;
}
</style>
