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
  resolvePresetTarget,
  saveButtonMappings,
  testScrollRegion,
  type AppFocusProfile,
  type ButtonMappings,
  type ButtonTrigger,
  type CustomAppPick,
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
  explicitProfileForTarget,
  findAppBindings,
  findMacroReferences,
  mergeApplications,
  nextSelectedTarget,
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
const resolvedPresetIds = ref<Record<string, string>>({});
const iconRequests = new Set<string>();
const scrollLearning = ref(false);
const scrollTesting = ref<ScrollCommand | null>(null);
const scrollMessage = ref<{
  text: string;
  tone: "success" | "error" | "info" | "warning";
} | null>(null);
let noticeTimer: number | null = null;

const rows = computed(() =>
  buildAppRows(
    mappings.value.applications ?? [],
    mappings.value.focusProfiles,
    presetApps.value,
    props.initialTarget ? [props.initialTarget] : [],
    resolvedPresetIds.value,
  ),
);
const selectedRow = computed<AppAdaptationRow | null>(
  () => rows.value.find((row) => row.target === selectedTarget.value) ?? null,
);
const selectedProfile = computed<AppFocusProfile | null>(() => {
  const row = selectedRow.value;
  return row
    ? explicitProfileForTarget(mappings.value.focusProfiles, row.target, row.presetId)
    : null;
});
const selectedScrollRegion = computed<ScrollRegionTarget | null>(() =>
  selectedRow.value
    ? scrollRegionForTarget(
        mappings.value.scrollRegions,
        selectedRow.value.target,
        selectedRow.value.presetId,
      )
    : null,
);
const scrollBusy = computed(() => scrollLearning.value || scrollTesting.value !== null || saving.value);
const fallbackProfile = computed<AppFocusProfile | null>(() => {
  const row = selectedRow.value;
  return row?.preset ? presetFocusDefault(row.presetId ?? row.target, true) : null;
});
const selectedIsPreset = computed(() => Boolean(selectedRow.value?.preset));
/** 预置应用的滚动区域已可用；用户重新学习后以显式档案为准。 */
const scrollStatusLabel = computed(() => {
  if (selectedScrollRegion.value) return t("已学习");
  return selectedIsPreset.value ? t("预置可用") : t("需学习");
});
/** 没有真实路径的预设只剩产品默认值，用「恢复默认」语义；有路径的按普通 APP 展示。 */
function isPresetPlaceholder(row: AppAdaptationRow | null): boolean {
  return Boolean(row?.preset && !row.path);
}
/** 只有用户手动添加过的应用才显示删除适配；显式导航来的条目删除后仍会留在当前页。 */
const canDeleteSelected = computed(
  () => Boolean(selectedRow.value?.library) && !isPresetPlaceholder(selectedRow.value),
);
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
    return t("还有 {count} 处绑定在使用它，先解绑才能删除设置。", {
      count: bindings.value.length,
    });
  }
  if (macroReferences.value.length) {
    return t("还有 {count} 个组合动作在使用它，先移除引用才能删除设置。", {
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

/** 列表摘要：预设整体预置可用；自定义 APP 显示短聚焦名 + 翻页状态。 */
function rowStatus(row: AppAdaptationRow): string {
  const focus = t(compactFocusLabel(effectiveAppProfile(row, mappings.value.focusProfiles)));
  const scroll = scrollRegionForTarget(
    mappings.value.scrollRegions,
    row.target,
    row.presetId,
  )
    ? t("已学习")
    : row.preset
      ? t("预置可用")
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
  if (code.includes("target_exited")) {
    return context === "learn"
      ? t("目标应用已关闭，请重试。")
      : t("翻页没有成功，请确认目标 App 在前台并已授予辅助功能权限。");
  }
  if (code.includes("not_accessible")) {
    return context === "learn"
      ? t("没有识别到可滚动区域，请点内容区（消息列表、正文或终端）后重试。")
      : t("翻页没有成功，请确认目标 App 在前台并已授予辅助功能权限。");
  }
  if (code.includes("own_process")) return t("请先切到目标 App 再测试翻页。");
  if (code.includes("pattern_unavailable")) {
    return t("已发送翻页；请目视确认内容是否移动。");
  }
  if (code.includes("not_moved")) {
    return t("已发送翻页，但没有检测到内容移动；可能已在顶部或底部，或该区域不可滚动。");
  }
  if (code.includes("no_candidate") || code.includes("outside_window")) {
    return context === "learn"
      ? t("没有识别到可滚动区域，请点内容区（消息列表、正文或终端）后重试。")
      : t("这个 App 没有可用的预置区域，请先学习。");
  }
  return context === "learn"
    ? t("没有识别到可滚动区域，请点内容区（消息列表、正文或终端）后重试。")
    : t("翻页没有成功，请确认目标 App 在前台并已授予辅助功能权限。");
}

function scrollRegionMessage(
  text: string,
  tone: "success" | "error" | "info" | "warning" = "success",
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

/** AppsFolder 目标无法在前端按路径匹配预设，加载时让后端按真实 exe 补齐。 */
async function resolvePresetIds(apps: readonly CustomAppPick[]): Promise<void> {
  const entries = await Promise.all(
    apps.map(async (app) => {
      try {
        return [app.path, await resolvePresetTarget(app.path)] as const;
      } catch {
        return [app.path, null] as const;
      }
    }),
  );
  const next: Record<string, string> = {};
  for (const [path, presetId] of entries) {
    if (presetId) next[path.toLocaleLowerCase()] = presetId;
  }
  resolvedPresetIds.value = next;
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
    void resolvePresetIds(saved.applications ?? []);
    selectedTarget.value = nextSelectedTarget(
      rows.value,
      props.initialTarget ?? appsViewSession.selectedTarget,
    );
    void loadIcons();
  } catch (error) {
    console.warn("load app adaptation page failed", error);
    loadError.value = t("无法读取聚焦与翻页设置，请稍后重试。");
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
    void resolvePresetIds(mappings.value.applications ?? []);
    return true;
  } catch (error) {
    console.warn("save app adaptation failed", error);
    showNotice(t("保存失败，请重试。"), "error");
    return false;
  } finally {
    saving.value = false;
  }
}

async function saveProfile(
  row: AppAdaptationRow,
  profile: AppFocusProfile | null,
): Promise<boolean> {
  const focusProfiles = { ...(mappings.value.focusProfiles ?? {}) };
  const keys = [row.target, row.presetId]
    .filter((key): key is string => Boolean(key))
    .map((key) => key.toLocaleLowerCase());
  for (const key of Object.keys(focusProfiles)) {
    if (keys.includes(key.toLocaleLowerCase())) delete focusProfiles[key];
  }
  // 预设统一存到预设 id：组合动作可能用 `wechat`，按键页/应用库可能用真实路径。
  const key = row.presetId ?? row.target;
  if (profile) focusProfiles[key] = profile;
  return persist({ ...mappings.value, focusProfiles });
}

async function saveScrollRegion(
  row: AppAdaptationRow,
  region: ScrollRegionTarget | null,
): Promise<boolean> {
  const scrollRegions = { ...(mappings.value.scrollRegions ?? {}) };
  const keys = [row.target, row.presetId]
    .filter((key): key is string => Boolean(key))
    .map((key) => key.toLocaleLowerCase());
  for (const key of Object.keys(scrollRegions)) {
    if (keys.includes(key.toLocaleLowerCase())) delete scrollRegions[key];
  }
  const key = row.presetId ?? row.target;
  if (region) scrollRegions[key] = region;
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
    if (await saveScrollRegion(row, region)) {
      scrollRegionMessage(t("滚动区域已记录，可以直接测试翻页。"));
    } else {
      scrollRegionMessage(t("保存滚动区域失败，请重试。"), "error");
    }
  } catch (error) {
    scrollRegionMessage(scrollRegionErrorLabel(error, "learn"), "warning");
  } finally {
    scrollLearning.value = false;
  }
}

async function clearScrollRegion(): Promise<void> {
  const row = selectedRow.value;
  if (!row || scrollBusy.value) return;
  scrollMessage.value = null;
  if (!(await saveScrollRegion(row, null))) {
    scrollRegionMessage(t("清除滚动区域失败，请重试。"), "error");
  }
}

async function testScroll(command: ScrollCommand): Promise<void> {
  const row = selectedRow.value;
  if (!row || !row.installed || scrollBusy.value) return;
  scrollTesting.value = command;
  scrollMessage.value = null;
  const regionKey =
    scrollRegionForTarget(mappings.value.scrollRegions, row.target)
      ? row.target
      : row.presetId &&
          scrollRegionForTarget(mappings.value.scrollRegions, row.presetId)
        ? row.presetId
        : (row.presetId ?? row.target);
  try {
    const outcome = await testScrollRegion(regionKey, command);
    if (command === "to_top" && outcome.atEdge) {
      scrollRegionMessage(t("已滚到顶部。"));
    } else if (command === "to_bottom" && outcome.atEdge) {
      scrollRegionMessage(t("已滚到底部。"));
    } else if (outcome.moved) {
      scrollRegionMessage(t("内容已移动，这个区域可以翻页。"));
    } else if (outcome.atEdge) {
      scrollRegionMessage(t("没有滚动；可能该区域不可滚，或已经在这一端。"), "warning");
    } else if (outcome.method === "wheel") {
      scrollRegionMessage(t("已发送翻页；请目视确认内容是否移动。"));
    } else {
      scrollRegionMessage(
        t("已发送翻页，但没有检测到内容移动；可能已在顶部或底部，或该区域不可滚动。"),
        "warning",
      );
    }
  } catch (error) {
    scrollRegionMessage(scrollRegionErrorLabel(error, "test"), "warning");
  } finally {
    scrollTesting.value = null;
  }
}

async function selectApp(app: CustomAppPick): Promise<void> {
  addError.value = null;
  const applications = mergeApplications(mappings.value.applications ?? [], [app]);
  if (await persist({ ...mappings.value, applications })) {
    addOpen.value = false;
    selectedTarget.value = app.path;
  } else {
    addError.value = t("保存应用列表失败，请重试。");
  }
}

async function deleteAdaptation(): Promise<void> {
  const row = selectedRow.value;
  if (!row || deleteBlockedReason.value) return;
  let next = removeAppAdaptation(mappings.value, row.target, isPresetPlaceholder(row));
  // 预设档案统一存在预设 id 下，删除适配时连同 id 键一起清理。
  if (row.presetId) {
    const expected = row.presetId.toLocaleLowerCase();
    const focusProfiles = { ...(next.focusProfiles ?? {}) };
    for (const key of Object.keys(focusProfiles)) {
      if (key.toLocaleLowerCase() === expected) delete focusProfiles[key];
    }
    const scrollRegions = { ...(next.scrollRegions ?? {}) };
    for (const key of Object.keys(scrollRegions)) {
      if (key.toLocaleLowerCase() === expected) delete scrollRegions[key];
    }
    next = {
      ...next,
      focusProfiles: Object.keys(focusProfiles).length ? focusProfiles : undefined,
      scrollRegions: Object.keys(scrollRegions).length ? scrollRegions : undefined,
    };
  }
  if (await persist(next)) {
    selectedTarget.value = nextSelectedTarget(rows.value, null);
    confirmDelete.value = false;
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
        <h1>{{ tPage("聚焦与翻页") }}</h1>
        <p class="page-subtitle">{{ t("按 App 单独设置输入框聚焦与翻页滚动；只要该 App 在前台就能用，不会改变按键绑定。") }}</p>
      </div>
    </header>

    <p v-if="notice" class="page-notice" :class="notice.tone" role="status">{{ notice.text }}</p>
    <div v-if="loadError" class="card load-error" role="alert">{{ loadError }}</div>

    <div v-if="loading" class="card loading-card" aria-busy="true">{{ t("正在读取聚焦与翻页设置…") }}</div>

    <article v-else-if="!rows.length" class="card empty-card">
      <span class="empty-icon" aria-hidden="true">
        <svg viewBox="0 0 24 24" fill="none">
          <rect x="3" y="3" width="7.4" height="7.4" rx="1.6" stroke="currentColor" stroke-width="1.8" />
          <rect x="13.6" y="3" width="7.4" height="7.4" rx="1.6" stroke="currentColor" stroke-width="1.8" />
          <rect x="3" y="13.6" width="7.4" height="7.4" rx="1.6" stroke="currentColor" stroke-width="1.8" />
          <path d="M17.3 13.8v7.4M13.6 17.5h7.4" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" />
        </svg>
      </span>
      <h2>{{ t("还没有添加任何 App") }}</h2>
      <p class="muted">
        {{ t("添加后可以为它单独设置输入框聚焦与翻页滚动；这两项只要该 App 在前台就能用。") }}
      </p>
      <button class="primary-button" type="button" @click="addError = null; addOpen = true">{{ t("添加 App") }}</button>
    </article>

    <div v-else class="master-detail">
      <section class="panel list-panel" :aria-label="t('已添加的 App')">
        <h2 class="list-title">{{ t("已添加的 App") }}</h2>
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
                <template v-if="!row.installed"> · {{ t("App 未找到") }}</template>
              </span>
            </span>
          </button>
        </div>
        <button class="add-app" type="button" @click="addError = null; addOpen = true">
          <span aria-hidden="true">＋</span>{{ t("添加 App") }}
        </button>
      </section>

      <section v-if="selectedRow" class="panel detail-panel" :aria-label="t('{name} 的设置', { name: selectedRow.name })">
        <div class="app-head">
          <div class="app-head-text">
            <h2>{{ selectedRow.name }}</h2>
          </div>
        </div>

        <p v-if="!selectedRow.installed" class="missing-note" role="status">
          {{ t("App 未找到，请重新添加。") }}
        </p>

        <p class="scope-note">
          {{ t("输入框聚焦和上下翻页按 App 单独设置：只要该 App 在前台就能用，与「打开 App」动作无关。") }}
        </p>

        <AppFocusProfileEditor
          :target="selectedRow.target"
          :profile="selectedProfile"
          :fallback="fallbackProfile"
          :allow-auto-composer="selectedIsPreset"
          :disabled="!selectedRow.installed"
          :save="(profile) => saveProfile(selectedRow!, profile)"
          @notice="showNotice"
        />

        <section class="section-card scroll-card">
          <div class="section-head">
            <h3>{{ t("滚动区域") }}</h3>
            <span
              class="scroll-status"
              :class="{ learned: selectedScrollRegion || selectedIsPreset }"
            >
              {{ scrollStatusLabel }}
            </span>
            <span class="spacer"></span>
          </div>
          <p class="hint">
            {{
              selectedIsPreset && !selectedScrollRegion
                ? t("预置滚动区域已可用；重新学习会覆盖预置区域。")
                : t("翻页键会滚动这个 App 的内容区。只读取区域结构，不读取内容，也不移动焦点；必要时会短暂移动鼠标指针。")
            }}
          </p>
          <div class="scroll-learn-row">
            <button
              class="primary-button scroll-learn"
              type="button"
              :disabled="!selectedRow.installed || scrollBusy"
              @click="learnScrollRegionForSelected"
            >
              {{ scrollLearning ? t("正在等待你点击滚动区域…") : t("学习滚动区域") }}
            </button>
            <span class="spacer"></span>
            <button
              class="secondary-button scroll-test"
              type="button"
              :disabled="!selectedRow.installed || scrollBusy"
              @click="testScroll('page_up')"
            >
              {{ scrollTesting === "page_up" ? t("执行中…") : t("测试向上翻页") }}
            </button>
            <button
              class="secondary-button scroll-test"
              type="button"
              :disabled="!selectedRow.installed || scrollBusy"
              @click="testScroll('page_down')"
            >
              {{ scrollTesting === "page_down" ? t("执行中…") : t("测试向下翻页") }}
            </button>
            <button
              v-if="selectedScrollRegion"
              class="secondary-button scroll-test"
              type="button"
              :disabled="scrollBusy"
              @click="clearScrollRegion"
            >
              {{ t("清除学习结果") }}
            </button>
          </div>
          <div class="scroll-edge-row">
            <span class="scroll-edge-label">{{ t("滚到两端：") }}</span>
            <button
              class="secondary-button scroll-test"
              type="button"
              :disabled="!selectedRow.installed || scrollBusy"
              @click="testScroll('to_top')"
            >
              {{ scrollTesting === "to_top" ? t("执行中…") : t("滚到顶部") }}
            </button>
            <button
              class="secondary-button scroll-test"
              type="button"
              :disabled="!selectedRow.installed || scrollBusy"
              @click="testScroll('to_bottom')"
            >
              {{ scrollTesting === "to_bottom" ? t("执行中…") : t("滚到底部") }}
            </button>
          </div>
          <p v-if="scrollMessage" class="scroll-message" :class="scrollMessage.tone">
            {{ scrollMessage.text }}
          </p>
        </section>

        <section class="section-card binding-card">
          <div class="section-head">
            <h3>{{ t("哪些按键会打开它") }}</h3>
            <span class="spacer"></span>
          </div>
          <p v-if="!bindings.length" class="hint">
            {{ t("还没有按键会打开这个 App，可在按键页的「打开 App」里选择它。") }}
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
              <span class="binding-action">{{ t("打开 App") }}</span>
              <span class="spacer"></span>
              <span class="binding-go">{{ t("去按键页") }}</span>
            </button>
          </div>

          <template v-if="canDeleteSelected">
            <div class="section-divider" aria-hidden="true"></div>
            <div class="delete-row">
              <p v-if="deleteBlockedReason" class="danger-line">{{ deleteBlockedReason }}</p>
              <template v-else>
                <button class="danger-link" type="button" :disabled="saving" @click="confirmDelete = true">
                  {{ t("删除这个 App 的设置") }}
                </button>
              </template>
            </div>
          </template>
        </section>
      </section>
    </div>

    <RegisteredAppsDialog
      v-if="addOpen"
      :known-apps="mappings.applications ?? []"
      :saving="saving"
      :save-error="addError"
      @close="addOpen = false"
      @select="selectApp"
    />

    <div v-if="confirmDelete && selectedRow" class="dialog-backdrop" @click.self="confirmDelete = false">
      <section class="confirm-dialog" role="dialog" aria-modal="true" :aria-labelledby="'delete-app-title'">
        <h2 id="delete-app-title">
          {{ isPresetPlaceholder(selectedRow) ? t("恢复「{name}」的默认聚焦？", { name: selectedRow.name }) : t("删除「{name}」的设置？", { name: selectedRow.name }) }}
        </h2>
        <p class="muted">
          {{
            isPresetPlaceholder(selectedRow)
              ? t("只会清除 SayAll 保存的显式聚焦设置；App 本身不会被修改或卸载。")
              : t("只会删除 SayAll 保存的这个 App 的设置（输入框学习记录、聚焦快捷键），不会卸载或改动 App 本身。已绑定的按键会保留，但会变成「尚未选择 App」。")
          }}
        </p>
        <div class="dialog-actions">
          <button class="secondary-button" type="button" :disabled="saving" @click="confirmDelete = false">{{ t("取消") }}</button>
          <button class="danger-button" type="button" :disabled="saving" @click="deleteAdaptation">
            {{ isPresetPlaceholder(selectedRow) ? t("恢复默认") : t("删除设置") }}
          </button>
        </div>
      </section>
    </div>
  </section>
</template>

<style scoped>
.apps-page {
  height: 100%;
  min-height: 0;
  padding-bottom: 12px;
  display: flex;
  flex-direction: column;
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
  align-items: stretch;
  gap: 14px;
  flex: 1 1 auto;
  min-height: 0;
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
  align-self: flex-start;
  height: fit-content;
  max-height: calc(100vh - 150px);
  min-height: 0;
}
.list-title {
  margin: 0 0 9px 2px;
  font-size: 16px;
}
.app-list {
  display: flex;
  flex-direction: column;
  gap: 5px;
  flex: 0 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding-right: 2px;
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
  margin-bottom: 0;
  flex: 0 0 auto;
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
  min-height: 0;
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  overflow-y: auto;
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
.scroll-learn-row,
.scroll-edge-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
}
.scroll-learn-row .spacer,
.scroll-edge-row .spacer {
  flex: 1 1 auto;
}
.scroll-learn {
  min-height: 28px;
  padding: 4px 12px;
  font-size: 12.5px;
}
.scroll-test {
  min-height: 28px;
  padding: 4px 10px;
  font-size: 12px;
}
.scroll-edge-label {
  color: var(--muted);
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
  cursor: default;
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
.scroll-message.warning {
  color: var(--warning-text);
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
  cursor: default;
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
  cursor: default;
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
