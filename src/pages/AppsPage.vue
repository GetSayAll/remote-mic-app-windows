<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import AppFocusProfileEditor from "../components/AppFocusProfileEditor.vue";
import RegisteredAppsDialog from "../components/RegisteredAppsDialog.vue";
import {
  buttonLabel,
  buttonTriggerLabel,
  activateAppForLearning,
  getApplicationIcon,
  getButtonMappings,
  getCombinedActions,
  learnScrollRegion,
  listPresetApps,
  saveButtonMappings,
  testScrollRegion,
  type AppFocusProfile,
  type ButtonMappings,
  type ButtonTrigger,
  type MacroDefinition,
  type PresetAppInfo,
  type RemoteButton,
  type RuntimeSnapshot,
  type ScrollCommand,
  type ScrollRegionTarget,
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
  scrollRegionForTarget,
  type AppAdaptationRow,
} from "../lib/app-adaptation";
import { presetFocusDefault } from "../lib/focus-defaults";
import { t, tPage } from "../lib/i18n";
import type { NavigationRequest } from "../navigation";
import { appsViewSession } from "./apps-view-session";

const props = withDefaults(
  defineProps<{ runtime: RuntimeSnapshot | null; initialTarget?: string | null }>(),
  { initialTarget: null },
);
const emit = defineEmits<{ navigate: [request: string | NavigationRequest] }>();

const mappings = ref<ButtonMappings>({ enabled: true, actions: {} });
const presetApps = ref<PresetAppInfo[]>([]);
const macroDefinitions = ref<MacroDefinition[]>([]);
const selectedTarget = ref<string | null>(appsViewSession.selectedTarget);
const loading = ref(true);
const saving = ref(false);
const loadError = ref<string | null>(null);
const addOpen = ref(false);
const addError = ref<string | null>(null);
const confirmDelete = ref(false);
const notice = ref<{ text: string; tone: "success" | "error" } | null>(null);
const appIcons = ref<Record<string, string>>({});
const iconRequests = new Set<string>();
const scrollLearning = ref(false);
const scrollTesting = ref<ScrollCommand | null>(null);
const scrollMessage = ref<{
  text: string;
  tone: "success" | "error" | "info";
} | null>(null);
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
const selectedScrollRegion = computed<ScrollRegionTarget | null>(() =>
  selectedRow.value
    ? scrollRegionForTarget(mappings.value.scrollRegions, selectedRow.value.target)
    : null,
);
const scrollBusy = computed(() => scrollLearning.value || scrollTesting.value !== null || saving.value);
const fallbackProfile = computed<AppFocusProfile | null>(() => {
  const row = selectedRow.value;
  return row?.preset ? presetFocusDefault(row.target, true) : null;
});
/** 没有真实路径的预设只剩产品默认值，用「恢复默认」语义；有路径的按普通 APP 展示。 */
function isPresetPlaceholder(row: AppAdaptationRow | null): boolean {
  return Boolean(row?.preset && !row.path);
}
const bindings = computed(() => {
  const row = selectedRow.value;
  return row ? findAppBindings(mappings.value.actions, row.target) : [];
});
const macroReferences = computed(() => {
  const row = selectedRow.value;
  return row && row.path ? findMacroReferences(macroDefinitions.value, row.target) : [];
});
const deleteBlockedReason = computed<string | null>(() => {
  const row = selectedRow.value;
  if (!row || isPresetPlaceholder(row)) return null;
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

/** 列表摘要对齐 Mac：短聚焦名 + 翻页状态。 */
function rowStatus(row: AppAdaptationRow): string {
  const focus = t(compactFocusLabel(effectiveAppProfile(row, mappings.value.focusProfiles)));
  const scroll = scrollRegionForTarget(mappings.value.scrollRegions, row.target)
    ? t("已学习")
    : t("需学习");
  return `${focus} · ${scroll}`;
}

function scrollRegionErrorLabel(error: unknown, context: "learn" | "test"): string {
  // Tauri invoke 的拒绝值可能是字符串，也可能是 { message } 对象；统一按包含匹配，
  // 避免后端稳定 token 被错误归到通用失败文案。
  const raw =
    error instanceof Error
      ? error.message
      : typeof error === "string"
        ? error
        : JSON.stringify(error);
  const code = raw.toLowerCase();
  if (code.includes("target_exited")) return t("目标应用已关闭，请重试。");
  if (code.includes("own_process")) return t("请先切到目标 APP 再测试滚动。");
  if (code.includes("not_accessible")) return t("这个 APP 没有暴露可读取的滚动区域。");
  if (code.includes("no_candidate") || code.includes("outside_window")) {
    return t("没有识别到滚动区域，请点一下内容区后重试。");
  }
  if (code.includes("pattern_unavailable")) {
    return t("这个 APP 不支持结构翻页，已尝试滚轮回退。");
  }
  if (code.includes("not_moved")) return t("滚动区域没有移动，请重新学习。");
  return context === "learn"
    ? t("没有识别到滚动区域，请点一下内容区后重试。")
    : t("滚动请求失败，请重试。");
}

function scrollRegionMessage(
  text: string,
  tone: "success" | "error" | "info" = "success",
): void {
  scrollMessage.value = { text, tone };
}

async function loadIcons(): Promise<void> {
  for (const row of rows.value) {
    if (!row.path || appIcons.value[row.target] !== undefined || iconRequests.has(row.target)) {
      continue;
    }
    iconRequests.add(row.target);
    try {
      const icon = await getApplicationIcon(row.path);
      if (icon) appIcons.value = { ...appIcons.value, [row.target]: icon };
    } catch {
      // 单个图标失败只回退到通用图标，不影响列表与配置。
    }
  }
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
      props.initialTarget ?? appsViewSession.selectedTarget,
    );
    void loadIcons();
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
    scrollRegions:
      next.scrollRegions && Object.keys(next.scrollRegions).length
        ? next.scrollRegions
        : undefined,
  };
}

async function persist(next: ButtonMappings): Promise<boolean> {
  if (saving.value) return false;
  saving.value = true;
  try {
    mappings.value = await saveButtonMappings(mappingPayload(next));
    void loadIcons();
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

async function saveScrollRegion(
  target: string,
  region: ScrollRegionTarget | null,
): Promise<boolean> {
  const scrollRegions = { ...(mappings.value.scrollRegions ?? {}) };
  const expected = target.toLocaleLowerCase();
  for (const key of Object.keys(scrollRegions)) {
    if (key.toLocaleLowerCase() === expected) delete scrollRegions[key];
  }
  if (region) scrollRegions[target] = region;
  return persist({ ...mappings.value, scrollRegions });
}

async function learnScrollRegionForSelected(): Promise<void> {
  const row = selectedRow.value;
  if (!row || !row.installed || scrollBusy.value) return;
  scrollLearning.value = true;
  scrollMessage.value = null;
  try {
    await activateAppForLearning(row.target);
    const region = await learnScrollRegion();
    if (await saveScrollRegion(row.target, region)) {
      scrollRegionMessage(t("滚动区域已记录。"));
    } else {
      scrollRegionMessage(t("保存滚动区域失败，请重试。"), "error");
    }
  } catch (error) {
    scrollRegionMessage(scrollRegionErrorLabel(error, "learn"), "error");
  } finally {
    scrollLearning.value = false;
  }
}

async function clearScrollRegion(): Promise<void> {
  const row = selectedRow.value;
  if (!row || scrollBusy.value) return;
  scrollMessage.value = null;
  if (await saveScrollRegion(row.target, null)) {
    scrollRegionMessage(t("已清除滚动区域记录。"));
  } else {
    scrollRegionMessage(t("清除滚动区域失败，请重试。"), "error");
  }
}

async function testScroll(command: ScrollCommand): Promise<void> {
  const row = selectedRow.value;
  if (!row || !row.installed || !selectedScrollRegion.value || scrollBusy.value) return;
  scrollTesting.value = command;
  scrollMessage.value = null;
  try {
    const outcome = await testScrollRegion(row.target, command);
    if (outcome.moved && outcome.atEdge) {
      scrollRegionMessage(t("已翻页，并到达滚动区域的这一端。"), "info");
    } else if (outcome.atEdge) {
      scrollRegionMessage(t("已经到达滚动区域的这一端。"), "info");
    } else if (outcome.moved) {
      scrollRegionMessage(t("滚动区域已移动。"));
    } else if (outcome.method === "wheel") {
      scrollRegionMessage(t("已发送滚动请求，但无法读回是否移动。"), "info");
    } else {
      scrollRegionMessage(t("没有检测到滚动，请重新学习。"), "error");
    }
  } catch (error) {
    scrollRegionMessage(scrollRegionErrorLabel(error, "test"), "error");
  } finally {
    scrollTesting.value = null;
  }
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

watch(selectedTarget, (target) => {
  appsViewSession.selectedTarget = target;
  confirmDelete.value = false;
  scrollMessage.value = null;
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
            <span class="app-tile" aria-hidden="true">
              <img v-if="appIcons[row.target]" :src="appIcons[row.target]" alt="" />
              <svg v-else viewBox="0 0 24 24" fill="none">
                <rect x="4" y="5" width="16" height="14" rx="2.5" stroke="currentColor" stroke-width="1.6" />
                <path d="M4 9h16" stroke="currentColor" stroke-width="1.6" />
              </svg>
            </span>
            <span class="app-row-text">
              <span class="app-row-name">{{ row.name }}</span>
              <span class="app-row-status">
                {{ rowStatus(row) }}
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
          <div class="app-head-text">
            <h2>{{ selectedRow.name }}</h2>
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
          :disabled="!selectedRow.installed"
          :save="(profile) => saveProfile(selectedRow!.target, profile)"
          @notice="showNotice"
        />

        <section class="section-card scroll-card">
          <div class="section-head">
            <h3>{{ t("滚动区域") }}</h3>
            <span class="scroll-status" :class="{ learned: selectedScrollRegion }">
              {{ selectedScrollRegion ? t("已学习") : t("需学习") }}
            </span>
            <span class="spacer"></span>
          </div>
          <p class="hint">
            {{ t("翻页键会滚动这个 APP 的内容区。只读取区域结构，不读取内容，也不移动焦点或鼠标。") }}
          </p>
          <div class="scroll-actions">
            <button
              class="secondary-button"
              type="button"
              :disabled="!selectedRow.installed || scrollBusy"
              @click="learnScrollRegionForSelected"
            >
              {{ scrollLearning ? t("正在等待你点击滚动区域…") : t("学习滚动区域") }}
            </button>
            <button
              v-if="selectedScrollRegion"
              class="quiet-button"
              type="button"
              :disabled="scrollBusy"
              @click="clearScrollRegion"
            >
              {{ t("清除滚动区域") }}
            </button>
          </div>
          <div class="scroll-test-row">
            <button
              v-for="item in [
                { command: 'page_up' as ScrollCommand, label: '测试向上翻页' },
                { command: 'page_down' as ScrollCommand, label: '测试向下翻页' },
                { command: 'to_top' as ScrollCommand, label: '滚到顶部' },
                { command: 'to_bottom' as ScrollCommand, label: '滚到底部' },
              ]"
              :key="item.command"
              class="quiet-button"
              type="button"
              :disabled="!selectedRow.installed || !selectedScrollRegion || scrollBusy"
              @click="testScroll(item.command)"
            >
              {{ scrollTesting === item.command ? t("执行中…") : t(item.label) }}
            </button>
          </div>
          <p v-if="scrollMessage" class="scroll-message" :class="scrollMessage.tone">
            {{ scrollMessage.text }}
          </p>
        </section>

        <section class="section-card binding-card">
          <div class="section-head">
            <h3>{{ t("绑定到它的按键") }}</h3>
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

          <div class="section-divider" aria-hidden="true"></div>
          <div class="delete-row">
            <p v-if="deleteBlockedReason" class="danger-line">{{ deleteBlockedReason }}</p>
            <template v-else>
              <button class="danger-link" type="button" :disabled="saving" @click="confirmDelete = true">
                {{ isPresetPlaceholder(selectedRow) ? t("恢复默认聚焦方式") : t("删除这个 APP 的适配") }}
              </button>
            </template>
          </div>
        </section>
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
          {{ isPresetPlaceholder(selectedRow) ? t("恢复「{name}」的默认聚焦？", { name: selectedRow.name }) : t("删除「{name}」的适配？", { name: selectedRow.name }) }}
        </h2>
        <p class="muted">
          {{
            isPresetPlaceholder(selectedRow)
              ? t("只会清除 SayAll 保存的显式聚焦设置；APP 本身不会被修改或卸载。")
              : t("只会删除 SayAll 保存的适配设置（输入框学习记录、聚焦快捷键），不会卸载或改动 APP 本身。")
          }}
        </p>
        <div class="dialog-actions">
          <button class="secondary-button" type="button" :disabled="saving" @click="confirmDelete = false">{{ t("取消") }}</button>
          <button class="danger-button" type="button" :disabled="saving" @click="deleteAdaptation">
            {{ isPresetPlaceholder(selectedRow) ? t("恢复默认") : t("删除适配") }}
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
  color: var(--muted);
  background: transparent;
}
.app-tile img {
  width: 100%;
  height: 100%;
  object-fit: contain;
}
.app-tile svg {
  width: 17px;
  height: 17px;
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
.scroll-card {
  display: grid;
  gap: 8px;
}
.scroll-status {
  font-size: 11.5px;
  font-weight: 600;
  color: var(--warning-text);
}
.scroll-status.learned {
  color: var(--success-text);
}
.scroll-actions,
.scroll-test-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
}
.scroll-actions .secondary-button {
  min-height: 26px;
  padding: 3px 10px;
  font-size: 12px;
}
.quiet-button {
  min-height: 24px;
  padding: 0 2px;
  border: 0;
  background: transparent;
  color: var(--accent-text);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
}
.quiet-button:hover:not(:disabled) {
  color: var(--accent-dark);
  text-decoration: underline;
}
.quiet-button:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}
.scroll-message {
  margin: 0;
  font-size: 12px;
  font-weight: 600;
}
.scroll-message.success {
  color: var(--success-text);
}
.scroll-message.error {
  color: var(--error-text);
}
.scroll-message.info {
  color: var(--text-secondary);
}
.binding-card {
  padding-bottom: 8px;
}
.section-divider {
  height: 1px;
  margin: 8px 0 6px;
  background: var(--border);
}
.delete-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
  min-height: 22px;
}
.danger-line {
  margin: 0;
  color: var(--muted);
  font-size: 11.5px;
  line-height: 1.4;
}
.danger-link {
  display: inline-flex;
  align-items: center;
  min-height: 22px;
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--delete-text);
  font-size: 12.5px;
  font-weight: 600;
  cursor: pointer;
}
.danger-link:hover:not(:disabled) {
  color: var(--delete-text-hover);
}
.danger-link:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}
.danger-button {
  min-height: 28px;
  padding: 4px 10px;
  border: 0;
  border-radius: 7px;
  color: var(--delete-text);
  background: var(--delete-surface);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
}
.danger-button:hover:not(:disabled) {
  color: var(--delete-text-hover);
  background: color-mix(in srgb, var(--delete-text) 16%, transparent);
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
