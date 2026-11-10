<script setup lang="ts">
import { computed, onMounted, ref, useId, watch } from "vue";
import {
  getApplicationIcon,
  pickCustomApp,
  scanRegisteredApps,
  type CustomAppPick,
} from "../lib/bridge";
import { t } from "../lib/i18n";

type PickSource = "scan" | "file";

const props = defineProps<{
  knownApps: CustomAppPick[];
  saving: boolean;
  saveError: string | null;
}>();
const emit = defineEmits<{
  close: [];
  select: [app: CustomAppPick, source: PickSource];
}>();

const dialog = ref<HTMLDialogElement | null>(null);
const apps = ref<CustomAppPick[]>([]);
const selectedApp = ref<CustomAppPick | null>(null);
const selectedSource = ref<PickSource | null>(null);
const query = ref("");
const scanning = ref(false);
const error = ref<string | null>(null);
const radioGroup = useId();

const known = computed(() => new Set(props.knownApps.map((app) => app.path.toLowerCase())));
const filtered = computed(() =>
  apps.value.filter((app) =>
    app.name.toLocaleLowerCase().includes(query.value.trim().toLocaleLowerCase()),
  ),
);
const locked = computed(() => selectedApp.value !== null);
const selectedInitial = computed(() => {
  const name = selectedApp.value?.name.trim() ?? "";
  return name ? Array.from(name)[0]!.toLocaleUpperCase() : "?";
});
const sourceLabel = computed(() => {
  if (selectedSource.value === "file") return t("程序选择");
  if (selectedSource.value === "scan") return t("本机扫描");
  return "";
});
const selectedIcon = ref<string | null>(null);
let iconRequest = 0;

watch(selectedApp, async (app) => {
  const request = ++iconRequest;
  selectedIcon.value = null;
  if (!app) return;
  try {
    const icon = await getApplicationIcon(app.path);
    if (request === iconRequest) selectedIcon.value = icon;
  } catch {
    // 单个图标失败只回退到首字母，不影响选择。
  }
});

function isKnown(app: CustomAppPick): boolean {
  return known.value.has(app.path.toLowerCase());
}

function isSelected(app: CustomAppPick): boolean {
  return selectedApp.value?.path.toLowerCase() === app.path.toLowerCase();
}

function chooseFromScan(app: CustomAppPick): void {
  if (locked.value || scanning.value || props.saving) return;
  selectedApp.value = app;
  selectedSource.value = "scan";
  error.value = null;
}

/** 程序选择只更新“已选择”区域，不把文件塞进本机扫描列表。 */
async function pickProgram(): Promise<void> {
  if (locked.value || props.saving || scanning.value) return;
  const pick = await pickCustomApp();
  if (!pick) return;
  selectedApp.value = pick;
  selectedSource.value = "file";
  error.value = null;
}

function clearSelection(): void {
  if (props.saving) return;
  selectedApp.value = null;
  selectedSource.value = null;
}

function confirm(): void {
  if (!selectedApp.value || !selectedSource.value || props.saving || scanning.value) return;
  emit("select", selectedApp.value, selectedSource.value);
}

async function scan(): Promise<void> {
  if (scanning.value || locked.value) return;
  scanning.value = true;
  error.value = null;
  try {
    apps.value = await scanRegisteredApps();
    selectedApp.value = null;
    selectedSource.value = null;
  } catch (cause) {
    // 原始错误只进控制台；界面给固定文案，英文模式也能翻译。
    console.warn(`scan registered apps failed: ${cause instanceof Error ? cause.message : String(cause)}`);
    error.value = "无法读取本机应用列表，请稍后重试。";
  } finally {
    scanning.value = false;
  }
}

onMounted(() => {
  if (dialog.value?.showModal) dialog.value.showModal();
  else dialog.value?.setAttribute("open", "");
  void scan();
});
</script>

<template>
  <dialog
    ref="dialog"
    class="registered-apps-dialog"
    aria-labelledby="registered-apps-title"
    @cancel.prevent="!saving && emit('close')"
  >
    <header class="picker-header">
      <div>
        <h3 id="registered-apps-title">{{ t("选择应用") }}</h3>
        <p class="picker-subtitle">{{ t("从本机扫描或手动选择程序。一次只能选择一个应用。") }}</p>
      </div>
      <button
        type="button"
        class="secondary-button picker-close"
        :aria-label="t('关闭应用选择')"
        :title="t('关闭')"
        :disabled="saving"
        @click="emit('close')"
      >×</button>
    </header>

    <div class="picker-methods" :class="{ locked }">
      <section class="picker-method picker-scan" :aria-label="t('本机扫描')">
        <div class="picker-method-head">
          <h4>{{ t("本机扫描") }}</h4>
          <span class="picker-count">{{ t("{count} 个应用", { count: filtered.length }) }}</span>
        </div>
        <div class="picker-search">
          <input
            v-model="query"
            type="search"
            :aria-label="t('搜索本机应用')"
            :placeholder="t('搜索应用')"
            :disabled="locked || scanning || saving"
          />
          <button
            type="button"
            class="secondary-button picker-rescan"
            :disabled="locked || scanning || saving"
            @click="scan"
          >{{ t("重新扫描") }}</button>
        </div>
        <div class="picker-list" :aria-busy="scanning" role="radiogroup" :aria-label="t('本机扫描')">
          <p v-if="scanning" class="picker-list-note" role="status">{{ t("正在扫描…") }}</p>
          <p v-else-if="!filtered.length && !error" class="picker-list-note">{{ t("未找到匹配的应用") }}</p>
          <label
            v-for="app in filtered"
            :key="app.path"
            class="picker-row"
            :class="{ 'is-selected': isSelected(app) }"
            :title="app.path"
          >
            <input
              type="radio"
              :name="radioGroup"
              :aria-label="app.name"
              :checked="isSelected(app)"
              :disabled="locked || saving"
              @change="chooseFromScan(app)"
            />
            <span class="picker-app-name">{{ app.name }}</span>
            <small v-if="isKnown(app)" class="picker-added">{{ t("已添加") }}</small>
          </label>
        </div>
      </section>

      <section class="picker-method picker-program" :aria-label="t('程序选择')">
        <div class="picker-method-head">
          <h4>{{ t("程序选择") }}</h4>
          <span class="picker-micro">{{ t("手动") }}</span>
        </div>
        <button
          type="button"
          class="picker-program-pick"
          :disabled="locked || saving || scanning"
          @click="pickProgram"
        >
          <span class="picker-program-icon" aria-hidden="true">+</span>
          <strong>{{ t("选择程序或快捷方式") }}</strong>
          <small>{{ t("支持 .exe 和 .lnk") }}</small>
        </button>
        <p class="picker-program-note">{{ t("适用于便携应用，或扫描列表里没有的程序。") }}</p>
      </section>
    </div>

    <div class="picker-hint" :class="{ 'is-locked': locked }">
      <span class="picker-hint-dot" aria-hidden="true"></span>
      <span v-if="locked">{{ t("已选择应用。要重新选择，请先取消下方已选择的应用。") }}</span>
      <span v-else>{{ t("尚未选择应用。请从上方两个入口中选择一个。") }}</span>
    </div>

    <section class="picker-selection" :class="{ 'is-filled': selectedApp }" :aria-label="t('已选择')">
      <template v-if="selectedApp">
        <img v-if="selectedIcon" :src="selectedIcon" alt="" class="picker-app-icon" />
        <span v-else class="picker-app-letter" aria-hidden="true">{{ selectedInitial }}</span>
        <div class="picker-selection-copy">
          <div class="picker-selection-name">{{ selectedApp.name }}</div>
          <div class="picker-selection-path" :title="selectedApp.path">{{ selectedApp.path }}</div>
        </div>
        <span class="picker-source">{{ sourceLabel }}</span>
        <button type="button" class="picker-clear" :disabled="saving" @click="clearSelection">
          {{ t("取消选择") }}
        </button>
      </template>
      <template v-else>
        <span class="picker-selection-empty-icon" aria-hidden="true">+</span>
        <div class="picker-selection-copy">
          <div class="picker-selection-title">{{ t("还没有选择应用") }}</div>
          <div class="picker-selection-sub">{{ t("选中后会在这里显示名称、来源和路径。") }}</div>
        </div>
      </template>
    </section>

    <p v-if="error || saveError" class="error-text" role="alert">{{ error ? t(error) : saveError }}</p>

    <footer class="picker-footer">
      <span class="picker-foot-hint">
        {{ locked ? t("确认后会把当前配置切换到这个应用") : t("一次只能选择一个应用") }}
      </span>
      <div class="picker-actions">
        <button type="button" class="secondary-button" :disabled="saving" @click="emit('close')">
          {{ t("取消") }}
        </button>
        <button
          type="button"
          class="primary-button"
          :disabled="!selectedApp || saving || scanning"
          @click="confirm"
        >
          {{ saving ? t("正在保存…") : t("使用这个应用") }}
        </button>
      </div>
    </footer>
  </dialog>
</template>

<style scoped>
.registered-apps-dialog {
  width: min(820px, calc(100vw - 40px));
  max-height: calc(100vh - 40px);
  box-sizing: border-box;
  padding: 20px 22px 18px;
  border: 1px solid var(--border-strong);
  border-radius: 12px;
  background: var(--surface-canvas);
  color: var(--text-primary);
  overflow-y: auto;
  display: flex;
  flex-direction: column;
}
.registered-apps-dialog::backdrop { background: #0008; }

.picker-header { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; }
.picker-header h3 { margin: 0; font-size: 21px; line-height: 1.2; letter-spacing: -0.3px; }
.picker-subtitle { margin: 5px 0 0; color: var(--muted); font-size: 12.5px; }
.picker-close { width: 32px; height: 32px; padding: 0; flex: 0 0 auto; font-size: 22px; }

.picker-methods {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 258px;
  gap: 14px;
  margin-top: 16px;
}
.picker-method {
  min-width: 0;
  padding: 13px;
  border: 1px solid var(--border);
  border-radius: 12px;
  background: var(--card);
}
.picker-program { border-style: dashed; border-color: var(--border-strong); background: var(--surface-subtle); }
.picker-method-head { height: 22px; display: flex; align-items: center; justify-content: space-between; gap: 10px; }
.picker-method-head h4 { margin: 0; font-size: 14px; font-weight: 700; }
.picker-count, .picker-micro { color: var(--muted); font-size: 10.5px; font-weight: 500; }

.picker-search { display: flex; gap: 8px; margin-top: 10px; }
.picker-search input {
  flex: 1;
  min-width: 0;
  height: 34px;
  padding: 0 10px;
  border: 1px solid var(--border-strong);
  border-radius: 8px;
  background: var(--surface-control);
  color: var(--text-primary);
  font: inherit;
  font-size: 12.5px;
}
.picker-search input::placeholder { color: var(--muted); }
.picker-search input:disabled { opacity: 0.55; }
.picker-rescan { height: 34px; padding: 0 10px; font-size: 11.5px; white-space: nowrap; }

.picker-list {
  max-height: 270px;
  margin-top: 9px;
  overflow-y: auto;
  border-top: 1px solid var(--border);
}
.picker-list-note { margin: 0; padding: 12px 8px; color: var(--muted); font-size: 12.5px; }
.picker-row {
  min-height: 39px;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 9px;
  border-bottom: 1px solid var(--border);
  color: var(--text-primary);
  cursor: pointer;
  font-size: 12.5px;
}
.picker-row.is-selected {
  margin: 1px 0;
  border: 1px solid var(--accent-border);
  border-radius: 8px;
  background: var(--accent-surface);
}
.picker-row input[type="radio"] {
  width: 15px;
  height: 15px;
  flex: 0 0 auto;
  margin: 0;
  accent-color: var(--accent);
}
.picker-app-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.picker-added {
  flex-shrink: 0;
  padding: 2px 6px;
  border-radius: 999px;
  background: var(--surface-subtle);
  color: var(--muted);
  font-size: 9.5px;
}

.picker-program { display: flex; flex-direction: column; }
.picker-program-pick {
  width: 100%;
  min-height: 156px;
  margin-top: 12px;
  padding: 16px 12px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 7px;
  border: 1.5px dashed var(--accent-border);
  border-radius: 11px;
  background: var(--accent-surface);
  color: var(--accent-text);
  font: inherit;
  text-align: center;
  cursor: pointer;
}
.picker-program-pick:disabled { cursor: default; }
.picker-program-icon {
  width: 44px;
  height: 44px;
  display: grid;
  place-items: center;
  border: 1px solid var(--accent-border);
  border-radius: 13px;
  background: var(--card);
  color: var(--accent);
  box-shadow: 0 5px 14px var(--shadow-control);
  font-size: 25px;
  font-weight: 300;
}
.picker-program-pick strong { font-size: 13px; line-height: 1.3; }
.picker-program-pick small { color: var(--muted); font-size: 10.5px; }
.picker-program-note { margin: 11px 2px 0; color: var(--muted); font-size: 10.5px; line-height: 1.55; }

.picker-hint {
  min-height: 32px;
  margin-top: 12px;
  padding: 7px 11px;
  display: flex;
  align-items: center;
  gap: 8px;
  border: 1px solid var(--border);
  border-radius: 9px;
  background: var(--surface-subtle);
  color: var(--muted);
  font-size: 11.5px;
  line-height: 1.35;
}
.picker-hint.is-locked {
  border-color: var(--warning-surface);
  background: var(--warning-surface-soft);
  color: var(--warning-text);
}
.picker-hint-dot { width: 7px; height: 7px; flex: 0 0 auto; border-radius: 50%; background: var(--muted); }
.picker-hint.is-locked .picker-hint-dot { background: var(--warning); }

.picker-selection {
  min-height: 88px;
  margin-top: 10px;
  padding: 12px 14px;
  display: flex;
  align-items: center;
  gap: 12px;
  border: 1.5px dashed var(--border-strong);
  border-radius: 11px;
  background: var(--surface-subtle);
}
.picker-selection.is-filled {
  border-style: solid;
  border-color: var(--accent-border);
  background: var(--accent-surface);
}
.picker-selection-copy { flex: 1; min-width: 0; }
.picker-selection-title { color: var(--text-subtle-strong); font-size: 12.5px; font-weight: 700; }
.picker-selection-sub { margin-top: 3px; color: var(--muted); font-size: 10.5px; }
.picker-selection-empty-icon {
  width: 34px;
  height: 34px;
  flex: 0 0 auto;
  display: grid;
  place-items: center;
  border: 1px dashed var(--border-strong);
  border-radius: 10px;
  background: var(--card);
  color: var(--muted);
  font-size: 17px;
}
.picker-app-icon {
  width: 36px;
  height: 36px;
  flex: 0 0 auto;
  box-sizing: border-box;
  padding: 3px;
  border: 1px solid var(--accent-border);
  border-radius: 10px;
  background: var(--card);
  object-fit: contain;
}
.picker-app-letter {
  width: 36px;
  height: 36px;
  flex: 0 0 auto;
  display: grid;
  place-items: center;
  border: 1px solid var(--accent-border);
  border-radius: 10px;
  background: var(--accent-surface-strong);
  color: var(--accent-text);
  font-size: 15px;
  font-weight: 700;
}
.picker-selection-name { color: var(--text-primary); font-size: 14px; font-weight: 700; }
.picker-selection-path {
  margin-top: 3px;
  overflow: hidden;
  color: var(--muted);
  font-size: 10.5px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.picker-source {
  display: inline-flex;
  align-items: center;
  height: 20px;
  padding: 0 8px;
  flex: 0 0 auto;
  border-radius: 999px;
  background: var(--accent-surface);
  color: var(--accent-text);
  font-size: 9.5px;
  font-weight: 600;
  white-space: nowrap;
}
.picker-clear {
  height: 29px;
  padding: 0 10px;
  flex: 0 0 auto;
  border: 1px solid var(--accent-border);
  border-radius: 8px;
  background: var(--surface-control);
  color: var(--accent-text);
  font: inherit;
  font-size: 11px;
  font-weight: 600;
  white-space: nowrap;
  cursor: pointer;
}

.picker-footer {
  margin-top: auto;
  padding-top: 16px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.picker-foot-hint { color: var(--muted); font-size: 10.5px; }
.picker-actions { display: flex; gap: 9px; }
.picker-actions .secondary-button, .picker-actions .primary-button {
  height: 36px;
  padding: 0 17px;
  font-size: 12px;
}
.picker-actions .primary-button:disabled {
  opacity: 1;
  border: 1px solid var(--border);
  background: var(--surface-subtle);
  color: var(--muted);
  box-shadow: none;
}

.picker-methods.locked .picker-method-head,
.picker-methods.locked .picker-search,
.picker-methods.locked .picker-row:not(.is-selected),
.picker-methods.locked .picker-program-pick,
.picker-methods.locked .picker-program-note {
  opacity: 0.55;
}
.picker-methods.locked .picker-row { cursor: default; }

@media (max-width: 720px) {
  .picker-methods { grid-template-columns: 1fr; }
}
</style>
