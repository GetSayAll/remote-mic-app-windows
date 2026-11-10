<script setup lang="ts">
import { ref, watch } from "vue";
import {
  applyDraftRules,
  draftRuleLimits,
  draftRulesStorageKey,
  normalizeDraftRules,
  parseDraftRules,
  type DraftRule,
} from "../lib/draft-rules";
import { reportFrontendEvent } from "../lib/frontend-diagnostics";
import { t } from "../lib/i18n";

const props = defineProps<{ modelValue: string; disabled?: boolean; draftRevision?: number }>();
const emit = defineEmits<{ "update:modelValue": [text: string] }>();

const rules = ref<DraftRule[]>([]);
const editingIndex = ref<number | null>(null);
const editor = ref<DraftRule>({ kind: "term", trigger: "", replacement: "" });
const undo = ref<{ before: string; after: string }>();
const result = ref<{ kind: "applied"; matches: number } | { kind: "no_match" | "restored" }>();
const storageError = ref(false);
const invalidEditor = ref(false);
const applyError = ref(false);

function report(phase: string, success: boolean, reason: string) {
  reportFrontendEvent({ event: "local_draft_rules", phase, result: success ? "passed" : "failed", reason });
}

try {
  const stored = parseDraftRules(localStorage.getItem(draftRulesStorageKey));
  if (stored) rules.value = stored;
  else {
    storageError.value = true;
    report("load", false, "invalid_rules");
  }
} catch {
  storageError.value = true;
  report("load", false, "storage_unavailable");
}

watch(() => [props.modelValue, props.draftRevision] as const, ([text, revision], [, previousRevision]) => {
  if (revision !== previousRevision || (undo.value && text !== undo.value.after)) {
    undo.value = undefined;
    result.value = undefined;
    applyError.value = false;
  }
});

function closeEditor() {
  editingIndex.value = null;
  invalidEditor.value = false;
}

function openEditor(index: number) {
  editingIndex.value = index;
  invalidEditor.value = false;
  editor.value = index === rules.value.length ? { kind: "term", trigger: "", replacement: "" } : { ...rules.value[index] };
}

function saveRules(candidate: DraftRule[]): boolean {
  const normalized = normalizeDraftRules(candidate);
  invalidEditor.value = normalized === null;
  if (!normalized) return false;
  try {
    localStorage.setItem(draftRulesStorageKey, JSON.stringify(normalized));
  } catch {
    storageError.value = true;
    report("save", false, "storage_unavailable");
    return false;
  }
  storageError.value = false;
  rules.value = normalized;
  report("save", true, "rules_saved");
  return true;
}

function saveEditor() {
  if (props.disabled || editingIndex.value === null) return;
  const candidate = [...rules.value];
  candidate[editingIndex.value] = editor.value;
  if (saveRules(candidate)) closeEditor();
}

function deleteRule(index: number) {
  if (props.disabled) return;
  const candidate = rules.value.filter((_, current) => current !== index);
  if (saveRules(candidate)) closeEditor();
}

function applyRules() {
  if (props.disabled || !props.modelValue.trim() || rules.value.length === 0) return;
  const applied = applyDraftRules(props.modelValue, rules.value);
  applyError.value = applied.status !== "ok";
  if (applied.status !== "ok") {
    result.value = undefined;
    report("apply", false, applied.status);
    return;
  }
  if (applied.text === props.modelValue) {
    result.value = { kind: "no_match" };
    report("apply", true, "no_change");
    return;
  }
  undo.value = { before: undo.value?.before ?? props.modelValue, after: applied.text };
  result.value = { kind: "applied", matches: applied.matches };
  emit("update:modelValue", applied.text);
  report("apply", true, "rules_applied");
}

function restoreDraft() {
  if (props.disabled || !undo.value || props.modelValue !== undo.value.after) return;
  const original = undo.value.before;
  undo.value = undefined;
  applyError.value = false;
  result.value = { kind: "restored" };
  emit("update:modelValue", original);
  report("undo", true, "draft_restored");
}
</script>

<template>
  <details class="draft-rules" data-draft-rules>
    <summary>{{ t("术语与口述短语") }}</summary>
    <p>{{ t("术语可纠正识别用词，口述短语可插入多行文字。规则仅保存在本机，点击应用后才会修改草稿。") }}</p>
    <p>{{ t("触发词区分大小写。英文触发词需要独立成词，Markdown 行内代码和顶层代码块保持原样。") }}</p>
    <ul v-if="rules.length" class="draft-rule-list">
      <li v-for="(rule, index) in rules" :key="rule.trigger">
        <span class="draft-rule-trigger">{{ rule.trigger }}</span>
        <span>{{ rule.kind === "term" ? t("术语") : t("口述短语") }}</span>
        <button type="button" class="secondary-button" data-draft-rule-edit :disabled="disabled || editingIndex !== null" @click="openEditor(index)">
          {{ t("编辑规则") }}
        </button>
        <button type="button" class="secondary-button" data-draft-rule-delete :disabled="disabled || editingIndex !== null" @click="deleteRule(index)">
          {{ t("删除规则") }}
        </button>
      </li>
    </ul>
    <div v-if="editingIndex !== null" class="draft-rule-editor">
      <label>
        {{ t("规则类型") }}
        <select v-model="editor.kind" data-draft-rule-kind :disabled="disabled">
          <option value="term">{{ t("术语") }}</option>
          <option value="snippet">{{ t("口述短语") }}</option>
        </select>
      </label>
      <label>
        {{ t("触发词") }}
        <input v-model="editor.trigger" data-draft-rule-trigger :maxlength="draftRuleLimits.trigger" :disabled="disabled" />
      </label>
      <label>
        {{ t("替换文字") }}
        <textarea v-model="editor.replacement" data-draft-rule-replacement rows="3" :maxlength="draftRuleLimits.replacement" :disabled="disabled" />
      </label>
      <p v-if="invalidEditor" role="alert" class="draft-rule-error">
        {{ t("请输入唯一的单行触发词和替换文字。多行替换请选择口述短语。") }}
      </p>
      <div class="draft-rule-actions">
        <button type="button" class="secondary-button" data-draft-rule-save :disabled="disabled" @click="saveEditor">{{ t("保存规则") }}</button>
        <button type="button" class="secondary-button" @click="closeEditor">{{ t("取消") }}</button>
      </div>
    </div>
    <div class="draft-rule-actions">
      <button v-if="editingIndex === null" type="button" class="secondary-button" data-draft-rule-add
        :disabled="disabled || rules.length >= draftRuleLimits.count" @click="openEditor(rules.length)">{{ t("添加规则") }}</button>
      <button type="button" class="secondary-button" data-draft-rule-apply
        :disabled="disabled || !modelValue.trim() || !rules.length" @click="applyRules">{{ t("应用到草稿") }}</button>
      <button v-if="undo && modelValue === undo.after" type="button" class="secondary-button" data-draft-rule-undo
        :disabled="disabled" @click="restoreDraft">{{ t("撤销规则修改") }}</button>
    </div>
    <p v-if="storageError" role="alert" class="draft-rule-error">{{ t("读取或保存本地规则失败。请检查本机存储后重试。") }}</p>
    <p v-if="applyError" role="alert" class="draft-rule-error">{{ t("无法应用规则。请缩短草稿或检查规则。") }}</p>
    <p v-if="result" role="status">
      <template v-if="result.kind === 'applied'">{{ t("已应用 {count} 处规则修改。", { count: result.matches }) }}</template>
      <template v-else-if="result.kind === 'no_match'">{{ t("规则没有更改当前草稿。") }}</template>
      <template v-else>{{ t("已恢复应用规则前的草稿。") }}</template>
    </p>
  </details>
</template>

<style scoped>
.draft-rules {
  color: var(--text-secondary);
}
.draft-rules summary {
  cursor: pointer;
  color: var(--text-primary);
}
.draft-rule-list {
  margin: 0;
  padding: 0;
  list-style: none;
}
.draft-rule-list li, .draft-rule-actions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 0.5rem;
  margin-block: 0.5rem;
}
.draft-rule-trigger {
  overflow-wrap: anywhere;
  color: var(--text-primary);
}
.draft-rule-editor, .draft-rule-editor label {
  display: grid;
  gap: 0.4rem;
}
.draft-rule-editor :is(input, select, textarea) {
  box-sizing: border-box;
  width: 100%;
  padding: 0.5rem;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface-control);
  color: var(--text-primary);
  font: inherit;
}
.draft-rule-error {
  color: var(--error-text);
}
</style>
