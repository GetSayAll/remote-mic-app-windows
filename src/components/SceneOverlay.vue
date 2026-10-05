<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getSceneSnapshot, subscribeSceneEvents, type MappingNotice, type SceneSnapshot } from "../lib/bridge";

const snapshot = ref<SceneSnapshot | null>(null);
const notice = ref<MappingNotice | null>(null);
const noticeElement = ref<HTMLElement | null>(null);
const menuList = ref<HTMLElement | null>(null);
let keyQueue = Promise.resolve();
const menuError = ref<string | null>(null);
function updateDefault(event: Event) {
  const generation = snapshot.value?.generation;
  if (generation === undefined) return;
  const enabled = (event.target as HTMLInputElement).checked;
  (event.target as HTMLInputElement).checked = snapshot.value?.updateDefault ?? false;
  if (snapshot.value?.preferencePending) return;
  menuError.value = null;
  keyQueue = keyQueue.then(async () => {
    await invoke("set_template_menu_update_default", { generation, enabled });
  }).catch(() => {
    if (snapshot.value?.generation !== generation) return;
    (event.target as HTMLInputElement).checked = snapshot.value?.updateDefault ?? false;
    menuError.value = "未能更新选择，请重新打开模板菜单。";
  });
}
const menuKeys = new Set(["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Enter", "Escape", "BrowserBack"]);
function menuKey(event: KeyboardEvent) {
  if (snapshot.value?.panel !== "template" || !menuKeys.has(event.key)) return;
  event.preventDefault();
  event.stopPropagation();
  const generation = snapshot.value.generation;
  const key = event.key;
  const down = event.type === "keydown";
  // Preserve rapid DOWN/UP order across asynchronous IPC.
  keyQueue = keyQueue.then(async () => { await invoke("template_menu_key", { generation, key, down }); }).catch(() => {});
}
function scrollSelectionIntoView() {
  const list = menuList.value;
  const selected = list?.querySelector<HTMLElement>("li.selected");
  if (!list || !selected) return;
  const viewport = list.getBoundingClientRect();
  const item = selected.getBoundingClientRect();
  if (item.top < viewport.top) list.scrollTop += item.top - viewport.top;
  else if (item.bottom > viewport.bottom) list.scrollTop += item.bottom - viewport.bottom;
}
let enabled = true;
let stop: (() => void) | null = null;
let timer: ReturnType<typeof setTimeout> | null = null;
let lastRevision = 0;
let disposed = false;

function clearNotice() {
  if (timer !== null) clearTimeout(timer);
  timer = null;
  notice.value = null;
}
function applyNotice(value: MappingNotice, revision: number) {
  if (revision <= lastRevision) return;
  lastRevision = revision;
  clearNotice();
  if (!enabled || snapshot.value?.panel) return;
  notice.value = value;
  void nextTick(() => {
    if (!notice.value || lastRevision !== revision || !enabled) return;
    const height = noticeElement.value?.getBoundingClientRect().height ?? 0;
    if (height > 0) void invoke("size_mapping_notice", { revision, height: Math.ceil(height + 16) }).catch(() => {});
  });
  timer = setTimeout(() => {
    clearNotice();
    void invoke("dismiss_mapping_notice", { revision }).catch(() => {});
  }, 2400);
}
function applySnapshot(value: SceneSnapshot) {
  const previous = snapshot.value;
  const selectionChanged = value.generation !== previous?.generation
    || value.selectedIndex !== previous?.selectedIndex
    || value.menuItems.length !== previous?.menuItems.length;
  if (value.generation !== snapshot.value?.generation) menuError.value = null;
  snapshot.value = value;
  enabled = value.mappingNoticeEnabled;
  if (!enabled) clearNotice();
  if (value.panel) clearNotice();
  if (value.mappingNotice) applyNotice(value.mappingNotice, value.mappingNoticeRevision);
  if (selectionChanged) void nextTick(scrollSelectionIntoView);
}
function noticeText(value: MappingNotice) {
  const name = value.kind === "common" ? "通用映射" : "当前模板：" + (value.name ?? "未命名模板");
  const applied = value.kind === "disabled" ? (value.name ? value.name + " · 映射已停用" : "按键映射已停用")
    : value.kind === "unconfigured" ? (value.name ? value.name + " · 未配置映射" : "当前窗口未配置映射")
    : value.actionsAvailable ? name : name + " · 按键暂不可用";
  if (value.defaultSaveStatus === "saved") return applied + " · 已更新程序默认";
  if (value.defaultSaveStatus === "failed") return applied + " · 默认保存失败，本次临时使用";
  if (value.defaultSaveStatus === "saving") return applied + " · 正在保存程序默认";
  return applied;
}
onMounted(async () => {
  window.addEventListener("keydown", menuKey, true);
  window.addEventListener("keyup", menuKey, true);
  stop = await subscribeSceneEvents(event => {
    if (disposed) return;
    if (event.type === "snapshot") applySnapshot(event.snapshot);
    else if (event.type === "mapping_notice_enabled") {
      enabled = event.enabled;
      if (!enabled) clearNotice();
    }
    else if (event.type === "mapping_applied") applyNotice(event.notice, event.revision);
  });
  if (disposed) { stop(); return; }
  const value = await getSceneSnapshot();
  if (!disposed && value) applySnapshot(value);
});
onUnmounted(() => {
  disposed = true; clearNotice(); stop?.();
  window.removeEventListener("keydown", menuKey, true);
  window.removeEventListener("keyup", menuKey, true);
});
</script>

<template>
  <main v-if="snapshot?.panel" class="scene-overlay">
    <h1>完整按键模板</h1>
    <label class="default-choice"><input :key="snapshot.generation" type="checkbox" :checked="snapshot.updateDefault" :aria-busy="snapshot.preferencePending" :aria-disabled="snapshot.preferencePending" @change="updateDefault">同时更新此程序的默认模板</label>
    <small class="default-hint">长按菜单键：切换此项并记住。确认模板后才更新关联。</small>
    <div class="preference-feedback">
      <small :class="{ visible: snapshot.preferencePending }" :role="snapshot.preferencePending ? 'status' : undefined">正在保存选项…</small>
      <small :class="{ visible: snapshot.preferenceError }" :role="snapshot.preferenceError ? 'alert' : undefined">选项保存失败，保留原值，请重试。</small>
      <small :class="{ visible: menuError }" :role="menuError ? 'alert' : undefined">{{ menuError ?? '未能更新选择，请重新打开模板菜单。' }}</small>
    </div>
    <ul ref="menuList"><li v-for="(item, index) in snapshot.menuItems" :key="item.label + '-' + index" :class="{ selected: index === snapshot.selectedIndex }"><span>{{ item.label }}</span><small>{{ item.running ? '当前模板' : '可选择' }}</small></li></ul>
    <p>{{ snapshot.status === 'template_menu_restore_failed' ? '未能返回原窗口，请点击目标窗口继续。' : '↑↓ 循环 · 确认松开后应用 · 返回 / 短菜单取消' }}</p>
  </main>
  <main v-else-if="notice" ref="noticeElement" class="mapping-notice" role="status">{{ noticeText(notice) }}</main>
  <main v-else class="scene-overlay-empty"></main>
</template>

<style scoped>
.scene-overlay {
  height: calc(100% - 16px);
  margin: 8px;
  padding: 16px;
  border-radius: 12px;
  background: rgba(30, 34, 44, .94);
  color: #fff;
  display: flex;
  flex-direction: column;
}
.scene-overlay h1 { font-size: 18px; margin: 0 0 12px; }
.default-choice { display: flex; align-items: center; gap: 8px; flex-shrink: 0; font-size: 14px; }
.default-choice input { width: auto; }
.default-hint { font-size: 12px; line-height: 1.5; margin: 5px 0 8px; flex-shrink: 0; }
.preference-feedback { display: grid; flex-shrink: 0; margin-bottom: 6px; }
.preference-feedback small { grid-area: 1 / 1; visibility: hidden; overflow-wrap: anywhere; }
.preference-feedback small.visible { visibility: visible; }
.scene-overlay ul { min-height: 0; overflow-y: auto; overflow-x: hidden; padding: 0; margin: 0; list-style: none; }
.scene-overlay li { padding: 10px; border-radius: 6px; overflow-wrap: anywhere; }
.scene-overlay li.selected { background: rgba(143, 146, 255, .25); }
.scene-overlay small { display: block; opacity: .65; }
.scene-overlay p { margin: 12px 0 0; font-size: 12px; opacity: .7; }

.mapping-notice {
  box-sizing: border-box;
  margin: 8px;
  padding: 14px 20px;
  border: 1px solid rgba(255, 255, 255, .14);
  border-radius: 12px;
  background: rgba(30, 34, 44, .94);
  color: #fff;
  font-size: 15px;
  line-height: 22px;
  overflow-wrap: anywhere;
  white-space: normal;
}
</style>
