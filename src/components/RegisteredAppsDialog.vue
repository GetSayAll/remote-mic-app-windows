<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { scanRegisteredApps, type CustomAppPick } from "../lib/bridge";
const props = defineProps<{ knownApps: CustomAppPick[]; saving: boolean; saveError: string | null }>();
const emit = defineEmits<{ close: []; add: [apps: CustomAppPick[]] }>();
const dialog = ref<HTMLDialogElement | null>(null);
const apps = ref<CustomAppPick[]>([]);
const selected = ref(new Set<string>());
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
async function scan(): Promise<void> {
  if (scanning.value) return;
  scanning.value = true; error.value = null;
  try { apps.value = await scanRegisteredApps(); selected.value = new Set(); }
  catch (cause) { error.value = cause instanceof Error ? cause.message : String(cause); }
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
      <h3 id="registered-apps-title">本机应用</h3>
      <button type="button" class="secondary-button picker-close" aria-label="关闭应用选择" title="关闭" :disabled="saving" @click="emit('close')">×</button>
    </header>
    <div class="picker-toolbar">
      <input v-model="query" type="search" aria-label="搜索本机应用" placeholder="搜索应用" />
      <button type="button" class="secondary-button" :disabled="scanning || saving" @click="scan">重新扫描</button>
    </div>
    <div class="picker-select-row">
      <label><input type="checkbox" aria-label="全选当前结果" :checked="allSelected" :disabled="scanning || saving || !candidates.length" @change="toggleAll" />全选当前结果</label>
      <span>{{ filtered.length }} 个应用 · 已选 {{ selectedApps.length }}</span>
    </div>
    <div class="picker-list" :aria-busy="scanning">
      <p v-if="scanning" role="status">正在扫描…</p>
      <p v-else-if="!filtered.length && !error">未找到匹配的应用</p>
      <template v-else>
        <label v-for="app in filtered" :key="app.path" class="picker-app" :title="app.path">
          <input type="checkbox" :aria-label="app.name" :checked="known.has(app.path.toLowerCase()) || selected.has(app.path)" :disabled="saving || known.has(app.path.toLowerCase())" @change="toggle(app.path, ($event.target as HTMLInputElement).checked)" />
          <span>{{ app.name }}</span><small v-if="known.has(app.path.toLowerCase())">已添加</small>
        </label>
      </template>
    </div>
    <p v-if="error || saveError" class="error-text" role="alert">{{ error || saveError }}</p>
    <footer class="picker-footer">
      <button type="button" class="secondary-button" :disabled="saving" @click="emit('close')">取消</button>
      <button type="button" class="primary-button" :disabled="scanning || saving || !selectedApps.length" @click="emit('add', selectedApps)">{{ saving ? '正在添加…' : '添加所选' }}（{{ selectedApps.length }}）</button>
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
input[type="checkbox"] { flex-shrink: 0; width: 16px; height: 16px; }
</style>
