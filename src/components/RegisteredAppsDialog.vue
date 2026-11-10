<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { pickCustomApp, scanRegisteredApps, type CustomAppPick } from "../lib/bridge";
import { t } from "../lib/i18n";
const props = defineProps<{ knownApps: CustomAppPick[]; saving: boolean; saveError: string | null }>();
const emit = defineEmits<{
  close: [];
  add: [apps: CustomAppPick[], source?: "scan" | "file"];
}>();
const dialog = ref<HTMLDialogElement | null>(null);
const apps = ref<CustomAppPick[]>([]);
const selected = ref(new Set<string>());
const pickedFromFile = new Set<string>();
const query = ref("");
const scanning = ref(false);
const error = ref<string | null>(null);
const known = computed(() => new Set(props.knownApps.map(app => app.path.toLowerCase())));
const filtered = computed(() => apps.value.filter(app => app.name.toLocaleLowerCase().includes(query.value.trim().toLocaleLowerCase())));
const candidates = computed(() => filtered.value.filter(app => !known.value.has(app.path.toLowerCase())));
const selectedApps = computed(() => apps.value.filter(app => selected.value.has(app.path) && !known.value.has(app.path.toLowerCase())));
const allSelected = computed(() => candidates.value.length > 0 && candidates.value.every(app => selected.value.has(app.path)));
function toggle(path: string, checked: boolean): void {
  const next = new Set(selected.value);
  if (checked) next.add(path); else next.delete(path);
  selected.value = next;
}
function toggleAll(event: Event): void {
  const checked = (event.target as HTMLInputElement).checked;
  const next = new Set(selected.value);
  for (const app of candidates.value) { if (checked) next.add(app.path); else next.delete(app.path); }
  selected.value = next;
}
/** 文件选择兜底：加入列表并勾选；提交时标记来源，按键页据此保持直接绑定。 */
async function pickProgram(): Promise<void> {
  if (props.saving || scanning.value) return;
  const pick = await pickCustomApp();
  if (!pick) return;
  const key = pick.path.toLowerCase();
  if (known.value.has(key)) {
    error.value = "这个应用已经在列表里。";
    return;
  }
  error.value = null;
  query.value = "";
  let entry = apps.value.find(app => app.path.toLowerCase() === key);
  if (!entry) {
    entry = pick;
    apps.value = [...apps.value, entry];
  }
  const next = new Set(selected.value);
  next.add(entry.path);
  selected.value = next;
  pickedFromFile.add(entry.path.toLowerCase());
}
function submit(): void {
  const appsToAdd = selectedApps.value;
  const source = appsToAdd.length === 1 && pickedFromFile.has(appsToAdd[0]!.path.toLowerCase())
    ? "file"
    : "scan";
  emit("add", appsToAdd, source);
}
async function scan(): Promise<void> {
  if (scanning.value) return;
  scanning.value = true; error.value = null;
  try {
    apps.value = await scanRegisteredApps();
    selected.value = new Set();
    pickedFromFile.clear();
  }
  catch (cause) {
    // 原始错误只进控制台；界面给固定文案，英文模式也能翻译。
    console.warn(`scan registered apps failed: ${cause instanceof Error ? cause.message : String(cause)}`);
    error.value = t("无法读取本机应用列表，请稍后重试。");
  }
  finally { scanning.value = false; }
}
onMounted(() => {
  if (dialog.value?.showModal) dialog.value.showModal(); else dialog.value?.setAttribute("open", "");
  void scan();
});
</script>

<template>
  <dialog ref="dialog" class="registered-apps-dialog" aria-labelledby="registered-apps-title" @cancel.prevent="!saving && emit('close')">
    <header class="picker-header">
      <h3 id="registered-apps-title">{{ t("本机应用") }}</h3>
      <button type="button" class="secondary-button picker-close" :aria-label="t('关闭应用选择')" :title="t('关闭')" :disabled="saving" @click="emit('close')">×</button>
    </header>
    <div class="picker-toolbar">
      <input v-model="query" type="search" :aria-label="t('搜索本机应用')" :placeholder="t('搜索应用')" />
      <button type="button" class="secondary-button" :disabled="scanning || saving" @click="scan">{{ t("重新扫描") }}</button>
    </div>
    <div class="picker-select-row">
      <label><input type="checkbox" :aria-label="t('全选当前结果')" :checked="allSelected" :disabled="scanning || saving || !candidates.length" @change="toggleAll" />{{ t("全选当前结果") }}</label>
      <span>{{ t("{count} 个应用 · 已选 {selected}", { count: filtered.length, selected: selectedApps.length }) }}</span>
    </div>
    <div class="picker-list" :aria-busy="scanning">
      <p v-if="scanning" role="status">{{ t("正在扫描…") }}</p>
      <p v-else-if="!filtered.length && !error">{{ t("未找到匹配的应用") }}</p>
      <template v-else>
        <label v-for="app in filtered" :key="app.path" class="picker-app" :title="app.path">
          <input type="checkbox" :aria-label="app.name" :checked="known.has(app.path.toLowerCase()) || selected.has(app.path)" :disabled="saving || known.has(app.path.toLowerCase())" @change="toggle(app.path, ($event.target as HTMLInputElement).checked)" />
          <span>{{ app.name }}</span><small v-if="known.has(app.path.toLowerCase())">{{ t("已添加") }}</small>
        </label>
      </template>
    </div>
    <p v-if="error || saveError" class="error-text" role="alert">{{ error ? t(error) : saveError }}</p>
    <footer class="picker-footer">
      <button
        type="button"
        class="secondary-button picker-pick"
        :disabled="scanning || saving"
        :title="t('从本机选择任意程序或快捷方式')"
        @click="pickProgram"
      >
        {{ t("选择程序或快捷方式") }}
      </button>
      <button type="button" class="secondary-button" :disabled="saving" @click="emit('close')">{{ t("取消") }}</button>
      <button type="button" class="primary-button" :disabled="scanning || saving || !selectedApps.length" @click="submit">{{ saving ? t('正在添加…') : t('添加所选') }}{{ t("（{count}）", { count: selectedApps.length }) }}</button>
    </footer>
  </dialog>
</template>

<style scoped>
.registered-apps-dialog { width: min(620px, calc(100vw - 40px)); max-height: calc(100vh - 40px); box-sizing: border-box; padding: 20px; border: 1px solid var(--border-strong); border-radius: 8px; background: var(--surface-canvas); color: var(--text-primary); overflow: auto; }
.registered-apps-dialog::backdrop { background: #0008; }
.picker-header, .picker-toolbar, .picker-select-row, .picker-footer { display: flex; align-items: center; gap: 12px; }
.picker-header { justify-content: space-between; margin-bottom: 14px; }
.picker-header h3 { font-size: 18px; margin: 0; }
.picker-close { width: 32px; height: 32px; padding: 0; font-size: 22px; }
.picker-toolbar input { flex: 1; min-width: 0; padding: 8px; border: 1px solid #888; border-radius: 4px; color: inherit; background: transparent; font: inherit; }
.picker-select-row { justify-content: space-between; flex-wrap: wrap; margin: 14px 0; font-size: 13px; }
.picker-select-row label, .picker-app { display: flex; align-items: center; gap: 10px; }
.picker-list { height: min(360px, 48vh); overflow-y: auto; border-block: 1px solid #8885; }
.picker-app { min-height: 38px; padding: 6px 8px; box-sizing: border-box; border-bottom: 1px solid #8883; }
.picker-app span { flex: 1; min-width: 0; overflow-wrap: anywhere; font-size: 14px; }
.picker-app small { flex-shrink: 0; opacity: .65; }
.picker-footer { justify-content: flex-end; margin-top: 16px; }
.picker-pick { margin-right: auto; }
input[type="checkbox"] { flex-shrink: 0; width: 16px; height: 16px; }
</style>
