<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import ButtonActionEditor from "../components/ButtonActionEditor.vue";
import ApplicationTemplateBindings from "../components/ApplicationTemplateBindings.vue";
import SettingsDialog from "../components/SettingsDialog.vue";
import { reportFrontendEvent } from "../lib/frontend-diagnostics";
import { useUiPreference } from "../lib/ui-preferences";
import { useCurrentTemplate } from "../lib/current-template";
import {
  actionSummary, buttonLabels, buttonTriggerLabel, copyTemplateCatalogEntry,
  deleteMappingTemplate, resetBuiltinTemplate,
  getMappingConfiguration, listPresetApps, setMappingNoticeEnabled, setMenuTemplateSwitchEnabled,
  getTemplateCatalog, renameMappingTemplate, updateButtonMappingTemplate, setButtonMappingFollowEnabled,
  type ButtonAction, type ButtonMappingTemplate, type ButtonMappings,
  type ButtonTrigger, type MappingConfiguration, type PresetAppInfo, type RemoteButton,
  type TemplateCatalogEntry,
} from "../lib/bridge";

const configuration = ref<MappingConfiguration | null>(null);
const currentTemplate = useCurrentTemplate();
const templateExpansion = useUiPreference("templatesExpanded");
const catalog = ref<TemplateCatalogEntry[]>([]);
const presetApps = ref<PresetAppInfo[]>([]);
const editing = ref<ButtonMappingTemplate | null>(null);
const draft = ref<ButtonMappings | null>(null);
const editingAction = ref<{ button: RemoteButton; trigger: ButtonTrigger } | null>(null);
const error = ref<string | null>(null);
const status = ref<string | null>(null);
const ruleNames = { buttonMappingFollowEnabled: "按程序加载默认模板", menuTemplateSwitchEnabled: "菜单键选择完整模板", mappingNoticeEnabled: "模板切换提示" };
type RuleField = keyof typeof ruleNames;
const rulePending = ref<Record<RuleField, boolean>>({ buttonMappingFollowEnabled: false, menuTemplateSwitchEnabled: false, mappingNoticeEnabled: false });
const ruleErrors = ref<Partial<Record<RuleField, string>>>({});
const ruleError = computed(() => (Object.keys(ruleNames) as RuleField[])
  .filter(field => ruleErrors.value[field])
  .map(field => `${ruleNames[field]}：${ruleErrors.value[field]}`).join("；"));
const loading = ref(false);
const saving = ref(false);
const associationBusy = ref(false);
const busy = computed(() => saving.value || associationBusy.value || loading.value);
const nameDialog = ref<TemplateCatalogEntry | null>(null);
const nameDraft = ref("");
const nameError = ref<string | null>(null);
const triggers: ButtonTrigger[] = ["single", "double", "long"];
const buttons = Object.keys(buttonLabels) as RemoteButton[];

async function refresh(): Promise<void> {
  if (busy.value) return;
  loading.value = true;
  error.value = null;
  try {
    const [next, nextCatalog] = await Promise.all([getMappingConfiguration(), getTemplateCatalog()]);
    configuration.value = next;
    catalog.value = nextCatalog;
    status.value = "已从本机配置刷新模板、关联和开关；未改变开关或编辑草稿，实际前台命中仍需按键验证。";
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    loading.value = false;
  }
}

async function run(action: () => Promise<MappingConfiguration>, message: string): Promise<void> {
  if (busy.value) return;
  saving.value = true;
  error.value = null;
  status.value = null;
  try {
    configuration.value = await action();
    status.value = message;
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    saving.value = false;
  }
}

async function saveRule(field: RuleField, action: () => Promise<MappingConfiguration>, event: Event): Promise<void> {
  if (!configuration.value) return;
  // Keep the checkbox on its persisted value until the narrow save succeeds.
  const input = event.target as HTMLInputElement;
  input.checked = configuration.value[field];
  if (busy.value || rulePending.value[field]) return;
  rulePending.value[field] = true;
  delete ruleErrors.value[field];
  try {
    const saved = await action();
    // Replies contain the whole configuration; independent field saves can finish out of order.
    configuration.value = { ...configuration.value, [field]: saved[field] };
  } catch (reason) {
    ruleErrors.value[field] = reason instanceof Error ? reason.message : String(reason);
  } finally {
    rulePending.value[field] = false;
  }
}

function toggleButtonMappingFollow(event: Event): void {
  if (!configuration.value) return;
  const enabled = !configuration.value.buttonMappingFollowEnabled;
  reportFrontendEvent({ event: "button_template_follow", phase: "requested", result: "passed", reason: enabled ? "enable" : "disable" });
  void saveRule("buttonMappingFollowEnabled", () => setButtonMappingFollowEnabled(enabled), event);
}

function toggleMappingNotice(event: Event): void {
  if (!configuration.value) return;
  const enabled = !configuration.value.mappingNoticeEnabled;
  void saveRule("mappingNoticeEnabled", () => setMappingNoticeEnabled(enabled), event);
}
function toggleMenuTemplateSwitch(event: Event): void {
  if (!configuration.value) return;
  const enabled = !configuration.value.menuTemplateSwitchEnabled;
  void saveRule("menuTemplateSwitchEnabled", () => setMenuTemplateSwitchEnabled(enabled), event);
}

function openRename(template: TemplateCatalogEntry): void {
  if (template.builtIn) return;
  nameDialog.value = template;
  nameDraft.value = template.name;
  nameError.value = null;
}

async function confirmRename(): Promise<void> {
  if (busy.value || !configuration.value || !nameDialog.value) return;
  const name = nameDraft.value.trim();
  if (!name) { nameError.value = "请输入模板名称"; return; }
  if (catalog.value.some((item) => item.id !== nameDialog.value?.id && item.name === name)) {
    nameError.value = "模板名称已存在，请换一个名称";
    return;
  }
  saving.value = true;
  nameError.value = null;
  try {
    configuration.value = await renameMappingTemplate(nameDialog.value.id, name);
    catalog.value = await getTemplateCatalog();
    status.value = `已重命名为“${name}”`;
    nameDialog.value = null;
  } catch (reason) {
    nameError.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    saving.value = false;
  }
}

function removeDirect(template: ButtonMappingTemplate): void {
  if (!configuration.value) return;
  const bound = configuration.value.applicationBindings.filter((binding) => binding.templateId === template.id).length;
  if (!window.confirm(bound
    ? `该模板已关联 ${bound} 个程序。删除后这些程序恢复通用配置，是否继续？`
    : `删除完整按键模板“${template.name}”？`)) return;
  void run(
    async () => {
      const saved = await deleteMappingTemplate(template.id, null, true);
      catalog.value = await getTemplateCatalog();
      return saved;
    },
    `已删除完整按键模板“${template.name}”`,
  );
}

function edit(template: ButtonMappingTemplate): void {
  editing.value = template;
  draft.value = JSON.parse(JSON.stringify(template.mappings)) as ButtonMappings;
  editingAction.value = null;
  error.value = null;
}

function resetDefault(template: TemplateCatalogEntry): void {
  if (busy.value || !template.builtIn || !window.confirm(`只复位“${template.name}”为当前版本默认按键？该模板的修改将丢失，程序关联和其它模板保持不变。`)) return;
  void run(async () => {
    const saved = await resetBuiltinTemplate(template.id);
    catalog.value = await getTemplateCatalog();
    return saved;
  }, `已复位“${template.name}”`);
}

function copyCatalogEntry(template: TemplateCatalogEntry): void {
  void run(async () => {
    await copyTemplateCatalogEntry(template.id, `${template.name} 副本`);
    catalog.value = await getTemplateCatalog();
    return await getMappingConfiguration();
  }, `已复制“${template.name}”；副本可编辑`);
}

function closeEditor(): void {
  if (!busy.value) {
    editing.value = null;
    draft.value = null;
    editingAction.value = null;
  }
}

function actionOf(button: RemoteButton, trigger: ButtonTrigger): ButtonAction {
  return draft.value?.actions[button]?.[trigger] ?? { type: "disabled" };
}

function applyAction(action: ButtonAction): void {
  if (busy.value || !draft.value || !editingAction.value) return;
  const { button, trigger } = editingAction.value;
  const actions = draft.value.actions[button] ?? {
    single: { type: "disabled" }, double: { type: "disabled" }, long: { type: "disabled" },
  };
  draft.value = {
    ...draft.value,
    actions: { ...draft.value.actions, [button]: { ...actions, [trigger]: action } },
  };
}

async function saveDraft(): Promise<void> {
  if (busy.value || !editing.value || !draft.value || !configuration.value) return;
  saving.value = true;
  error.value = null;
  try {
    const saved = await updateButtonMappingTemplate(
      editing.value.id,
      JSON.parse(JSON.stringify(draft.value)) as ButtonMappings,
    );
    configuration.value = {
      ...configuration.value,
      templates: [...configuration.value.templates.filter(item => item.id !== saved.id), saved],
    };
    catalog.value = catalog.value.map((item) => item.id === saved.id
      ? { ...item, name: saved.name, buttonMappings: saved.mappings }
      : item);
    status.value = "完整按键模板已保存；若正在使用此模板，实际按键映射会同步更新";
    editing.value = null;
    draft.value = null;
    editingAction.value = null;
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    saving.value = false;
  }
}

const currentTitle = computed(() => editingAction.value
  ? `${buttonLabels[editingAction.value.button]} · ${buttonTriggerLabel(editingAction.value.trigger)}`
  : "选择一个动作");

function directTemplate(entry: TemplateCatalogEntry): ButtonMappingTemplate | null {
  return entry.kind === "direct" && entry.buttonMappings
    ? { id: entry.id, name: entry.name, mappings: entry.buttonMappings }
    : null;
}

function bindingCount(entry: TemplateCatalogEntry): number {
  return entry.kind === "direct"
    ? configuration.value?.applicationBindings.filter((binding) => binding.templateId === entry.id).length ?? 0
    : configuration.value?.applicationBindings.filter((binding) => binding.templateId === entry.id).length ?? 0;
}


onMounted(() => {
  void refresh();
  void listPresetApps().then((items) => { presetApps.value = items.filter((item) => item.installed); }).catch(() => {
    // 这里只影响编辑器中的“打开应用”候选；编辑器仍可使用自定义目标。
    presetApps.value = [];
  });
});
</script>

<template>
  <section class="settings-page templates-page">
    <header class="page-header">
      <div>
        <h1>模板</h1>
        <p>为程序选择按键模板，保存普通按键的实际动作。语音键始终按下开始、释放结束。</p>
      </div>
      <button type="button" class="secondary-button" :disabled="loading || busy" @click="refresh">刷新关联状态</button>
    </header>
    <p v-if="error && !editing" class="error-text">{{ error }}</p>
    <p v-if="status" class="operation-message">{{ status }}</p>
    <section v-if="loading && !configuration" class="card loading-card"><p class="muted">正在读取模板配置…</p></section>
    <section v-else-if="!configuration" class="card loading-card">
      <p class="error-text">模板配置加载失败。请重试。</p>
      <button type="button" class="secondary-button" @click="refresh">重试</button>
    </section>

    <template v-else>
      <section class="card run-modes">
        <h2>模板切换规则</h2>
        <p class="muted">按程序自动加载，也可用菜单键手动选择。</p>
        <label class="mode-row">
          <input type="checkbox" aria-label="按程序加载默认模板" aria-describedby="program-default-help" :checked="configuration.buttonMappingFollowEnabled" :disabled="busy" :aria-busy="rulePending.buttonMappingFollowEnabled" :aria-disabled="busy || rulePending.buttonMappingFollowEnabled" @change="toggleButtonMappingFollow">
          <span><strong>按程序加载默认模板</strong><small id="program-default-help">切换程序时加载默认；同程序换窗不变。</small></span>
        </label>
        <label class="mode-row">
          <input type="checkbox" aria-label="菜单键选择完整模板" aria-describedby="menu-template-help" :checked="configuration.menuTemplateSwitchEnabled" :disabled="busy" :aria-busy="rulePending.menuTemplateSwitchEnabled" :aria-disabled="busy || rulePending.menuTemplateSwitchEnabled" @change="toggleMenuTemplateSwitch">
          <span><strong>菜单键选择完整模板</strong><small id="menu-template-help">短按打开选择；面板可选本次使用或保存为程序默认。</small></span>
        </label>
        <label class="mode-row">
          <input type="checkbox" aria-label="模板切换提示" aria-describedby="mapping-notice-help" :checked="configuration.mappingNoticeEnabled" :disabled="busy" :aria-busy="rulePending.mappingNoticeEnabled" :aria-disabled="busy || rulePending.mappingNoticeEnabled" @change="toggleMappingNotice">
          <span><strong>模板切换提示</strong><small id="mapping-notice-help">仅生效模板变化时短暂提示。</small></span>
        </label>
        <details class="template-help"><summary>使用说明</summary><p class="muted">关闭自动加载后，手动选中的模板在本次运行中持续使用，切换程序不会清除；重启后恢复通用配置。开启自动加载后，菜单未勾选更新默认时仅覆盖当前程序，切到其他程序后清除；勾选并确认才保存该程序默认。按键页切换仅改变当前使用模板。关闭菜单选择后，菜单键按当前映射执行。提示开关不影响按键动作。</p></details>
        <p class="rule-feedback error-text" role="status" aria-live="polite">{{ ruleError ? `保存失败：${ruleError}` : "" }}</p>
      </section>

      <details class="card collapsible-card complete-template-panel" :open="templateExpansion.value.value" :aria-busy="templateExpansion.pending.value" @toggle="templateExpansion.toggleDetails">
        <summary><h2>完整按键模板</h2><span class="section-count">{{ catalog.length }} 个模板</span></summary>
        <div class="collapsible-body">
        <p class="muted">所有模板统一发送固定按键或组合键，不读取第三方界面。前三项可直接编辑或单独复位；未配置的动作不执行映射。</p>
        <p v-if="currentTemplate.error.value" class="error-text" role="status">{{ currentTemplate.error.value }}</p>
        <div v-for="template in catalog" :key="template.id" class="status-panel template-row" :class="{ 'current-template': currentTemplate.applied.value && currentTemplate.templateId.value === template.id }" :aria-current="currentTemplate.applied.value && currentTemplate.templateId.value === template.id ? 'true' : undefined">
          <span>
            <strong>{{ template.name }}</strong>
            <small>固定按键与组合键 · 已关联 {{ bindingCount(template) }} 个程序<span v-if="template.builtIn"> · 默认模板</span></small>
          </span>
          <span class="button-row">
            <button type="button" class="secondary-button" :disabled="busy" @click="directTemplate(template) && edit(directTemplate(template)!)">编辑</button>
            <button v-if="template.builtIn" type="button" class="secondary-button" :disabled="busy" @click="resetDefault(template)">复位模板</button>
            <button type="button" class="secondary-button" :disabled="busy" @click="copyCatalogEntry(template)">复制</button>
            <button v-if="!template.builtIn" type="button" class="secondary-button" :disabled="busy" @click="openRename(template)">重命名</button>
            <button v-if="!template.builtIn && template.kind === 'direct'" type="button" class="secondary-button" :disabled="busy" @click="directTemplate(template) && removeDirect(directTemplate(template)!)">删除</button>
          </span>
        </div>
        <p v-if="!catalog.length" class="muted">尚未保存完整按键模板。</p>
        </div>
      </details>

      <p v-if="templateExpansion.error.value" role="alert" class="error-text">{{ templateExpansion.error.value }}</p>
      <ApplicationTemplateBindings
        :configuration="configuration"
        :catalog="catalog"
        :disabled="saving || loading"
        @busy-change="associationBusy = $event"
        @saved="configuration = $event"
        @status="status = $event"
      />
    </template>
  </section>

  <SettingsDialog
    v-if="editing && draft"
    :title="`编辑完整按键模板“${editing.name}”`"
    description="此处只修改当前模板。运行中的前台切换不会改变编辑目标或草稿。"
    :busy="busy"
    @close="closeEditor"
  >
    <label class="toggle-row"><span>启用此模板</span><input v-model="draft.enabled" :disabled="busy" type="checkbox" class="toggle-input" /></label>
    <div class="template-grid">
      <strong>按键</strong><strong v-for="trigger in triggers" :key="trigger">{{ buttonTriggerLabel(trigger) }}</strong>
      <template v-for="button in buttons" :key="button">
        <span>{{ buttonLabels[button] }}</span>
        <button v-for="trigger in triggers" :key="trigger" type="button" class="mapping-cell" :disabled="busy" :class="{ selected: editingAction?.button === button && editingAction?.trigger === trigger }" @click="editingAction = { button, trigger }">{{ actionSummary(actionOf(button, trigger)) }}</button>
      </template>
    </div>
    <section v-if="editingAction" class="card action-detail">
      <h3>{{ currentTitle }}</h3>
      <ButtonActionEditor shortcuts-only :mappings="draft" :button="editingAction.button" :trigger="editingAction.trigger" :preset-apps="presetApps" @update="applyAction" @status="status = $event" />
    </section>
    <p v-if="error" class="error-text">保存失败：{{ error }}</p>
    <template #actions>
      <button type="button" :disabled="busy" @click="saveDraft">{{ busy ? "正在保存…" : "保存模板" }}</button>
      <button type="button" class="secondary-button" :disabled="busy" @click="closeEditor">取消</button>
    </template>
  </SettingsDialog>


  <SettingsDialog
    v-if="nameDialog"
    title="重命名完整按键模板"
    description="只修改当前模板名称。"
    :busy="busy"
    @close="nameDialog = null"
  >
    <label class="name-field">模板名称<input v-model="nameDraft" :disabled="busy" @keyup.enter="confirmRename" /></label>
    <p v-if="nameError" class="error-text">{{ nameError }}</p>
    <template #actions>
      <button type="button" :disabled="busy" @click="confirmRename">{{ busy ? "正在保存…" : "保存名称" }}</button>
      <button type="button" class="secondary-button" :disabled="busy" @click="nameDialog = null">取消</button>
    </template>
  </SettingsDialog>
</template>

<style scoped>
.templates-page { display: grid; gap: 14px; }
.templates-page > .page-header { margin-bottom: -2px; }
.loading-card { display: flex; align-items: center; justify-content: space-between; }
.run-modes { display: grid; gap: 0; min-width: 0; }
.run-modes > p { margin: 0; }
.run-modes > .mode-row:first-of-type { margin-top: 14px; }
.mode-row { display: grid; grid-template-columns: 18px minmax(0, 1fr); align-items: start; gap: 12px; padding: 14px 0; border-top: 1px solid var(--border); cursor: pointer; }
.mode-row:last-of-type { padding-bottom: 0; }
.mode-row > span { min-width: 0; overflow-wrap: anywhere; }
.mode-row strong, .mode-row small { display: block; }
.mode-row strong { font-size: 14px; line-height: 20px; }
.mode-row small { margin-top: 5px; color: var(--muted); font-size: 12.5px; line-height: 1.65; }
.mode-row input[type="checkbox"] { box-sizing: border-box; width: 18px; height: 18px; padding: 0; margin: 1px 0 0; accent-color: var(--accent); cursor: inherit; }
.mode-row input:focus-visible { outline: 2px solid var(--accent); outline-offset: 3px; }
.mode-row:has(input:disabled) { cursor: default; opacity: .65; }
.run-modes > .rule-feedback { height: 2.8em; line-height: 1.4; margin: 8px 0 0 30px; overflow: auto; overflow-wrap: anywhere; }
.complete-template-panel { margin-top: 0; }
.template-row { align-items: center; justify-content: space-between; border: 2px solid transparent; }
.template-row.current-template { border-color: var(--accent); }
.template-grid { display: grid; grid-template-columns: minmax(90px, .7fr) repeat(3, minmax(150px, 1fr)); gap: 8px; align-items: stretch; min-width: 620px; margin-top: 14px; }
.template-grid > strong, .template-grid > span { align-self: center; padding: 9px; }
.template-grid .mapping-cell { min-height: 48px; text-align: left; }
.template-grid .mapping-cell.selected { outline: 2px solid var(--accent); }
.action-detail { margin: 16px 0 0; }
.name-field { display: grid; gap: 7px; font-size: 13px; font-weight: 600; }
</style>
