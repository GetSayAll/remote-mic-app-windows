<script setup lang="ts">
/**
 * 组合动作页（对齐 Mac `RemoteMicMacroView` 的库浏览 + 三步向导交互）。
 *
 * 设计口径（2026-10-15 方案定稿）：
 * - 绑定遥控器按键在**按键页**设置；本页只管理动作库与编辑；
 * - 名称 20 字硬截；保存不是必须经过测试；删除为两段式内联确认；
 * - 浏览器预览下读演示库（bridge 里的 previewMacroLibrary），保存类操作报错。
 */
import { computed, onMounted, ref } from "vue";
import {
  buttonLabel,
  buttonTriggerLabel,
  cancelMacro,
  deleteMacro,
  exportCombinedActions,
  focusLearnErrorLabel,
  focusPointErrorLabel,
  getButtonMappings,
  getCombinedActions,
  getMacroRunStatus,
  importCombinedActions,
  learnFocusPoint,
  learnFocusTarget,
  latestMacroDefinitions,
  listPresetApps,
  macroActionLabel,
  macroChordLabel,
  macroReferences,
  macroStartErrorLabel,
  macroStepOutcomeLabel,
  macroStepSummary,
  macroTerminalLabel,
  newMacroId,
  newStepId,
  nextMacroVersion,
  registerMacroNames,
  registerPresetAppNames,
  saveButtonMappings,
  saveMacro,
  startShortcutCapture,
  stopShortcutCapture,
  subscribeShortcutCaptureEdges,
  testMacro,
  testMacroDefinition,
  testMacroStep,
  type KeyChord,
  type KeyCode,
  type MacroAction,
  type MacroChord,
  type MacroDefinition,
  type MacroLibraryDocument,
  type MacroModifier,
  type MacroReferenceReport,
  type MacroRunReport,
  type MacroStep,
  type ButtonTrigger,
  type PresetAppInfo,
  type RemoteButton,
  type RuntimeSnapshot,
  type ShortcutCaptureEdge,
} from "../lib/bridge";

defineProps<{ runtime: RuntimeSnapshot | null }>();

/** 向导三步（对齐 Mac：基本设置 → 编排步骤 → 测试与保存）。 */
type EditorStep = "basic" | "steps" | "review";
type WizardStepCard = "library" | "editor";

const MAX_NAME_CHARS = 20;
const MAX_STEPS = 20;

const loading = ref(true);
const definitions = ref<MacroDefinition[]>([]);
const selectedMacroId = ref<string | null>(null);
const search = ref("");
const statusMessage = ref("");
const statusKind = ref<"info" | "success" | "error">("info");

const wizardVisible = ref(false);
const editorStep = ref<EditorStep>("basic");
const draft = ref<MacroDefinition | null>(null);
const isNewMacro = ref(false);
const selectedStepId = ref<string | null>(null);
const pendingDeleteId = ref<string | null>(null);
/** 删除确认里的引用清单（按键 + 嵌套引用；查询中为 null）。 */
const deleteReferences = ref<MacroReferenceReport | null>(null);

const presetApps = ref<PresetAppInfo[]>([]);
const capturedChord = ref<KeyChord | null>(null);
const captureActive = ref(false);
const learningStepId = ref<string | null>(null);
const learningCountdown = ref(0);
/** 「记录点击位置」的倒计时（与学习输入框共用 `learningStepId`/`learningTimer`）。 */
const pointCountdown = ref(0);

const testRunId = ref<string | null>(null);
const testReport = ref<MacroRunReport | null>(null);
const testRunning = ref(false);
let testPollTimer: ReturnType<typeof setInterval> | undefined;
let learningTimer: ReturnType<typeof setInterval> | undefined;

const selectedDefinition = computed(
  () => definitions.value.find((item) => item.macroId === selectedMacroId.value) ?? null,
);

const filteredDefinitions = computed(() => {
  const keyword = search.value.trim().toLowerCase();
  if (!keyword) return definitions.value;
  return definitions.value.filter((item) => item.name.toLowerCase().includes(keyword));
});

const draftValid = computed(() => {
  const value = draft.value;
  if (!value) return false;
  if (!value.name.trim() || value.name.length > MAX_NAME_CHARS) return false;
  if (value.steps.length < 1 || value.steps.length > MAX_STEPS) return false;
  return true;
});

const selectedStepIndex = computed(() =>
  draft.value?.steps.findIndex((step) => step.stepId === selectedStepId.value) ?? -1,
);

const ACTION_ORDER: MacroAction[] = [
  "open_application",
  "send_shortcut",
  "focus_input",
  "wait_for_application",
  "open_url",
  "run_macro",
];

function setStatus(message: string, kind: "info" | "success" | "error" = "info"): void {
  statusMessage.value = message;
  statusKind.value = kind;
}

async function reloadLibrary(): Promise<void> {
  try {
    const library = await getCombinedActions();
    applyLibrary(library);
    if (
      selectedMacroId.value &&
      !definitions.value.some((item) => item.macroId === selectedMacroId.value)
    ) {
      selectedMacroId.value = definitions.value[0]?.macroId ?? null;
    }
    if (!selectedMacroId.value) {
      selectedMacroId.value = definitions.value[0]?.macroId ?? null;
    }
  } catch (error) {
    setStatus(`组合动作读取失败：${messageOf(error)}`, "error");
  } finally {
    loading.value = false;
  }
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * 应用一次库响应：列表与选择器只保留每条动作的**最新版本**。
 *
 * 库文件按不可变版本累积（编辑一次 = 新版本追加），直接铺开会让同一条
 * 动作显示成多行——2026-10-15 现场「编辑后变成两个」即此。
 */
function applyLibrary(library: MacroLibraryDocument): void {
  const latest = latestMacroDefinitions(library.definitions);
  definitions.value = latest;
  registerMacroNames(latest);
}

/**
 * 深拷贝一条定义。
 *
 * 不能用 `structuredClone`：列表里的元素是 Vue 响应式代理，结构化克隆会
 * 抛 `DataCloneError`（2026-10-15 测试实测）；JSON 往返对纯数据定义足够。
 */
function cloneDefinition(definition: MacroDefinition): MacroDefinition {
  return JSON.parse(JSON.stringify(definition)) as MacroDefinition;
}

function cloneStep(step: MacroStep): MacroStep {
  return JSON.parse(JSON.stringify(step)) as MacroStep;
}

// ===== 库浏览 =====

function startCreate(): void {
  draft.value = {
    schemaVersion: "0.1",
    macroId: newMacroId(),
    version: "1.0.0",
    name: "",
    steps: [],
  };
  isNewMacro.value = true;
  wizardVisible.value = true;
  editorStep.value = "basic";
  selectedStepId.value = null;
  setStatus("组合动作已创建，请添加步骤");
}

function startEdit(definition: MacroDefinition): void {
  draft.value = cloneDefinition(definition);
  isNewMacro.value = false;
  wizardVisible.value = true;
  editorStep.value = "basic";
  selectedStepId.value = definition.steps[0]?.stepId ?? null;
}

function cancelEditing(): void {
  stopCapture();
  wizardVisible.value = false;
  draft.value = null;
  testReport.value = null;
  setStatus("已取消编辑");
}

function toggleDelete(macroId: string): void {
  if (pendingDeleteId.value === macroId) {
    pendingDeleteId.value = null;
    deleteReferences.value = null;
    return;
  }
  pendingDeleteId.value = macroId;
  deleteReferences.value = null;
  void macroReferences(macroId)
    .then((report) => {
      // 用户可能已取消或换了目标：只写当前目标的结果。
      if (pendingDeleteId.value === macroId) deleteReferences.value = report;
    })
    .catch(() => {
      if (pendingDeleteId.value === macroId) {
        deleteReferences.value = { buttons: [], nestedIn: [] };
      }
    });
}

/** 引用清单的用户文案（"确定·单击、主页·长按"）。 */
const deleteReferenceText = computed<string>(() => {
  const report = deleteReferences.value;
  if (!report || report.buttons.length === 0) return "";
  return report.buttons
    .map((item) => `${buttonLabel(item.button as RemoteButton)}·${buttonTriggerLabel(item.trigger as ButtonTrigger)}`)
    .join("、");
});

async function confirmDelete(macroId: string): Promise<void> {
  try {
    const library = await deleteMacro(macroId);
    applyLibrary(library);
    selectedMacroId.value = definitions.value[0]?.macroId ?? null;
    pendingDeleteId.value = null;
    deleteReferences.value = null;
    // 按键页每次进入都会重新读取映射；后端已把绑定该动作的槽位改成「未设置」。
    setStatus("组合动作已删除，绑定它的按键已改为未设置", "success");
  } catch (error) {
    setStatus(`删除失败：${messageOf(error)}`, "error");
  }
}

async function duplicateMacro(definition: MacroDefinition): Promise<void> {
  const copy: MacroDefinition = {
    ...cloneDefinition(definition),
    macroId: newMacroId(),
    version: "1.0.0",
    name: `${definition.name.slice(0, MAX_NAME_CHARS - 4)}（副本）`,
    steps: definition.steps.map((step, index) => ({
      ...cloneStep(step),
      stepId: `step.copy${index.toString().padStart(2, "0")}`,
    })),
  };
  try {
    const library = await saveMacro(copy);
    applyLibrary(library);
    selectedMacroId.value = copy.macroId;
    setStatus("组合动作副本已创建", "success");
  } catch (error) {
    setStatus(`复制失败：${messageOf(error)}`, "error");
  }
}

async function onExport(): Promise<void> {
  try {
    const exported = await exportCombinedActions();
    setStatus(exported ? "组合动作已导出" : "已取消导出", exported ? "success" : "info");
  } catch (error) {
    setStatus(`导出失败：${messageOf(error)}`, "error");
  }
}

async function onImport(): Promise<void> {
  try {
    const library = await importCombinedActions();
    if (!library) {
      setStatus("已取消导入");
      return;
    }
    applyLibrary(library);
    setStatus("组合动作已导入", "success");
  } catch (error) {
    setStatus(`导入失败：${messageOf(error)}`, "error");
  }
}

// ===== 编辑向导 =====

function renameDraft(value: string): void {
  if (!draft.value) return;
  draft.value.name = value.slice(0, MAX_NAME_CHARS);
}

function addStep(action: MacroAction): void {
  if (!draft.value) return;
  if (draft.value.steps.length >= MAX_STEPS) {
    setStatus(`最多 ${MAX_STEPS} 个步骤`, "error");
    return;
  }
  const step: MacroStep = {
    stepId: newStepId(),
    action,
    parameters: defaultParameters(action),
  };
  if (editorStep.value === "basic") {
    // 基本设置里选择第一个动作：**替换**而不是累加（对齐 Mac），停留在本步；
    // 名称与步骤在最后一起保存（早前版本在这里自动跳到第②步，绕过了
    // "名称为空不给下一步"的门禁，2026-10-15 现场实测）。
    draft.value.steps = [step];
  } else {
    draft.value.steps.push(step);
  }
  selectedStepId.value = step.stepId;
  capturedChord.value = null;
}

/** 向导步骤跳转门禁：名称非空才能进第②步，有步骤才能进第③步。 */
function goToStep(step: EditorStep): void {
  if (step === "basic") {
    editorStep.value = "basic";
    return;
  }
  if (!draft.value?.name.trim()) {
    setStatus("先填写组合动作名称", "error");
    return;
  }
  if (step === "review" && (draft.value?.steps.length ?? 0) === 0) {
    setStatus("先添加第一个步骤", "error");
    return;
  }
  editorStep.value = step;
}

function defaultParameters(action: MacroAction): MacroStep["parameters"] {
  const lastTarget = [...(draft.value?.steps ?? [])]
    .reverse()
    .find((step) => step.parameters.target)?.parameters.target;
  switch (action) {
    case "open_application":
      return { target: lastTarget ?? presetApps.value[0]?.id ?? "notepad" };
    case "wait_for_application":
      return { target: lastTarget ?? presetApps.value[0]?.id ?? "notepad", state: "visible_window" };
    case "send_shortcut":
      return {};
    case "focus_input":
      return { target: lastTarget ?? presetApps.value[0]?.id ?? "notepad" };
    case "open_url":
      return { url: "https://" };
    case "run_macro":
      return {};
  }
}

function removeStep(stepId: string): void {
  if (!draft.value) return;
  draft.value.steps = draft.value.steps.filter((step) => step.stepId !== stepId);
  if (selectedStepId.value === stepId) {
    selectedStepId.value = draft.value.steps[0]?.stepId ?? null;
  }
  setStatus("已删除步骤");
}

function moveStep(index: number, offset: number): void {
  if (!draft.value) return;
  const target = index + offset;
  if (target < 0 || target >= draft.value.steps.length) return;
  const steps = draft.value.steps;
  [steps[index], steps[target]] = [steps[target], steps[index]];
}

function selectStep(stepId: string): void {
  selectedStepId.value = stepId;
  capturedChord.value = null;
}

/** 「选择已有组合动作」候选：排除当前动作自身（对齐 Mac）。 */
const nestedCandidates = computed(() =>
  definitions.value.filter((item) => item.macroId !== draft.value?.macroId),
);

function updateSelectedStep(mutate: (step: MacroStep) => void): void {
  const step = draft.value?.steps.find((item) => item.stepId === selectedStepId.value);
  if (step) mutate(step);
}

/** 名称限长同步（20 字硬截）。 */
function renameStepTarget(target: string): void {
  updateSelectedStep((step) => {
    step.parameters.target = target;
  });
}

// ===== 快捷键录入（复用按键页的录入链路：边沿累积，全部松开后落盘）=====

const MODIFIER_KEY_CODES = new Set<KeyCode>([
  "control",
  "left_control",
  "right_control",
  "shift",
  "left_shift",
  "right_shift",
  "alt",
  "left_alt",
  "right_alt",
  "windows",
  "left_windows",
  "right_windows",
]);

let captureUnsubscribe: (() => void) | undefined;
const capturePressedKeys = new Set<KeyCode>();
let capturedKeys: KeyCode[] | null = null;

function onCaptureEdge(edge: ShortcutCaptureEdge): void {
  if (!captureActive.value) return;
  if (!edge.isPressed) {
    capturePressedKeys.delete(edge.key);
    if (capturedKeys && capturePressedKeys.size === 0) {
      finalizeCapture(capturedKeys);
    }
    return;
  }
  capturePressedKeys.add(edge.key);
  if (capturedKeys) return;
  if (MODIFIER_KEY_CODES.has(edge.key)) return;
  if (edge.key === "escape") {
    void stopCapture();
    setStatus("已取消录入");
    return;
  }
  // 主键按下：固定「当前按住的修饰键 + 主键」；等全部松开后落盘。
  capturedKeys = [...capturePressedKeys];
}

async function beginCapture(): Promise<void> {
  if (!selectedStepId.value || captureActive.value) return;
  capturedChord.value = null;
  capturedKeys = null;
  capturePressedKeys.clear();
  try {
    captureUnsubscribe?.();
    captureUnsubscribe = await subscribeShortcutCaptureEdges(onCaptureEdge);
    await startShortcutCapture();
    captureActive.value = true;
    setStatus("请按下一次按键");
  } catch (error) {
    captureActive.value = false;
    captureUnsubscribe?.();
    captureUnsubscribe = undefined;
    setStatus(`无法启动快捷键录入：${messageOf(error)}`, "error");
  }
}

async function stopCapture(): Promise<void> {
  captureActive.value = false;
  capturePressedKeys.clear();
  capturedKeys = null;
  captureUnsubscribe?.();
  captureUnsubscribe = undefined;
  try {
    await stopShortcutCapture();
  } catch {
    // 录入停止失败不影响已保存的步骤配置。
  }
}

/** 录入完成：把捕获到的组合键转换成组合动作的 `MacroChord` 写进当前步骤。 */
function finalizeCapture(keys: KeyCode[]): void {
  const step = draft.value?.steps.find((item) => item.stepId === selectedStepId.value);
  if (!step) {
    return;
  }
  const modifiers = keys
    .map(modifierOf)
    .filter((value): value is MacroModifier => value !== null);
  const ordered = MACRO_MODIFIER_ORDER.filter((modifier) => modifiers.includes(modifier));
  const keyCode = keys.find((code) => modifierOf(code) === null);
  const key = keyCode ? semanticKeyOf(keyCode) : null;
  if (!key) {
    setStatus("这个按键暂不支持写入组合动作", "error");
    void stopCapture();
    return;
  }
  const chord: MacroChord = { modifiers: ordered, key };
  step.parameters.chord = chord;
  capturedChord.value = { keys };
  setStatus(`快捷键录入成功：${macroChordLabel(chord)}`, "success");
  void stopCapture();
}

const MACRO_MODIFIER_ORDER: MacroModifier[] = ["control", "alt", "shift", "win"];

function modifierOf(code: KeyCode): MacroModifier | null {
  switch (code) {
    case "control":
    case "left_control":
    case "right_control":
      return "control";
    case "shift":
    case "left_shift":
    case "right_shift":
      return "shift";
    case "alt":
    case "left_alt":
    case "right_alt":
      return "alt";
    case "windows":
    case "left_windows":
    case "right_windows":
      return "win";
    default:
      return null;
  }
}

/** 边沿键码 → 语义键名（与 Rust `key_code_to_macro_key` 同一张表）。 */
function semanticKeyOf(code: KeyCode): string | null {
  const named: Record<string, string> = {
    enter: "return",
    escape: "escape",
    space: "space",
    tab: "tab",
    backspace: "backspace",
    delete: "delete",
    insert: "insert",
    home: "home",
    end: "end",
    page_up: "pageup",
    page_down: "pagedown",
    left: "left",
    right: "right",
    up: "up",
    down: "down",
    apps: "apps",
    volume_mute: "volume_mute",
    volume_up: "volume_up",
    volume_down: "volume_down",
    media_prev: "media_prev",
    media_next: "media_next",
    media_play_pause: "media_play_pause",
  };
  if (named[code]) return named[code];
  if (/^[a-z]$/.test(code)) return code;
  const digit = /^digit([0-9])$/.exec(code);
  if (digit) return digit[1];
  if (/^f([1-9]|1[0-2])$/.test(code)) return code;
  return null;
}

// ===== 学习输入框（复用按键页的学习链路；必须把结果写进聚焦档案）=====

/** 该 App 的聚焦档案策略（键 = 应用目标，与按键映射 `focusProfiles` 同源）。 */
const focusProfileStrategies = ref<Record<string, string>>({});

async function refreshFocusProfiles(): Promise<void> {
  try {
    const mappings = await getButtonMappings();
    const strategies: Record<string, string> = {};
    for (const [target, profile] of Object.entries(mappings.focusProfiles ?? {})) {
      strategies[target] = profile.strategy;
    }
    focusProfileStrategies.value = strategies;
  } catch {
    // 读取失败：状态显示为"未知"，不阻断页面其余功能。
  }
}

/** 该目标是否已学习过输入框（策略 = 聚焦已记录的输入框才可执行本步骤）。 */
function focusReady(target: string | undefined): boolean {
  if (!target) return false;
  return focusProfileStrategies.value[target] === "recorded_element";
}

/** 该目标是否已记录点击位置（策略 = 记录点击位置；面向没有 UIA 树的应用）。 */
function focusPointReady(target: string | undefined): boolean {
  if (!target) return false;
  return focusProfileStrategies.value[target] === "recorded_point";
}

/** 「记录点击位置」：6 秒后采集鼠标位置，写入该目标的 recorded_point 档案。 */
function startPointRecording(): void {
  const stepId = selectedStepId.value;
  const target = draft.value?.steps.find((item) => item.stepId === stepId)?.parameters.target;
  if (!stepId || !target) {
    setStatus("先给这一步选择目标 App", "error");
    return;
  }
  learningStepId.value = stepId;
  pointCountdown.value = 6;
  setStatus("6 秒后自动记录鼠标位置：请先把鼠标停在目标输入框上");
  learningTimer = setInterval(() => {
    pointCountdown.value -= 1;
    if (pointCountdown.value <= 0) {
      void finishPointRecording(target);
    }
  }, 1000);
}

async function finishPointRecording(target: string): Promise<void> {
  if (learningTimer) clearInterval(learningTimer);
  learningTimer = undefined;
  learningStepId.value = null;
  try {
    const point = await learnFocusPoint(target);
    const mappings = await getButtonMappings();
    const nextProfiles = {
      ...(mappings.focusProfiles ?? {}),
      [target]: { strategy: "recorded_point" as const, point },
    };
    await saveButtonMappings({ ...mappings, focusProfiles: nextProfiles });
    await refreshFocusProfiles();
    setStatus("点击位置记录成功", "success");
  } catch (error) {
    setStatus(`记录位置失败：${focusPointErrorLabel(messageOf(error))}`, "error");
  }
}

function startLearning(): void {
  const stepId = selectedStepId.value;
  const target = draft.value?.steps.find((item) => item.stepId === stepId)?.parameters.target;
  if (!stepId || !target) {
    setStatus("先给这一步选择目标 App", "error");
    return;
  }
  learningStepId.value = stepId;
  learningCountdown.value = 10;
  setStatus("10 秒后自动记录，请切换并点击目标输入框");
  learningTimer = setInterval(() => {
    learningCountdown.value -= 1;
    if (learningCountdown.value <= 0) {
      void finishLearning(target);
    }
  }, 1000);
}

async function finishLearning(target: string): Promise<void> {
  if (learningTimer) clearInterval(learningTimer);
  learningTimer = undefined;
  learningStepId.value = null;
  try {
    const recorded = await learnFocusTarget();
    // 与按键页**同一落盘口径**：写入该目标的聚焦档案（策略=聚焦已记录的输入框）。
    // 早前版本只显示"成功"却不写档案，导致这一步永远找不到输入框记录
    // （2026-10-15 现场：组合动作页学习"成功"、步骤仍报不可用）。
    const mappings = await getButtonMappings();
    const nextProfiles = {
      ...(mappings.focusProfiles ?? {}),
      [target]: { strategy: "recorded_element" as const, recorded },
    };
    await saveButtonMappings({ ...mappings, focusProfiles: nextProfiles });
    await refreshFocusProfiles();
    setStatus("输入框学习成功", "success");
  } catch (error) {
    setStatus(`输入框学习失败：${focusLearnErrorLabel(messageOf(error))}`, "error");
  }
}

// ===== 测试与保存 =====

function currentDraft(): MacroDefinition | null {
  return draft.value ? cloneDefinition(draft.value) : null;
}

async function runDraftTest(): Promise<void> {
  const definition = currentDraft();
  if (!definition) return;
  testReport.value = null;
  try {
    testRunId.value = await testMacroDefinition(definition);
    testRunning.value = true;
    setStatus("组合动作已开始执行");
    startPollingRun(testRunId.value);
  } catch (error) {
    setStatus(testFailureText(error), "error");
  }
}

async function runStepTest(stepId: string): Promise<void> {
  const definition = currentDraft();
  if (!definition) return;
  testReport.value = null;
  try {
    testRunId.value = await testMacroStep(definition, stepId);
    testRunning.value = true;
    startPollingRun(testRunId.value);
  } catch (error) {
    setStatus(testFailureText(error), "error");
  }
}

function startPollingRun(runId: string): void {
  if (testPollTimer) clearInterval(testPollTimer);
  testPollTimer = setInterval(async () => {
    try {
      const status = await getMacroRunStatus(runId);
      if (status.report) {
        testReport.value = status.report;
        testRunning.value = status.running;
        if (!status.running) {
          if (testPollTimer) clearInterval(testPollTimer);
          testPollTimer = undefined;
          setStatus(macroTerminalLabel(status.report.result), status.report.result.startsWith("completed") ? "success" : "error");
        }
      } else {
        testRunning.value = status.running;
      }
    } catch {
      if (testPollTimer) clearInterval(testPollTimer);
      testPollTimer = undefined;
      testRunning.value = false;
    }
  }, 400);
}

async function cancelTest(): Promise<void> {
  if (!testRunId.value) return;
  try {
    await cancelMacro(testRunId.value);
    setStatus("已取消运行");
  } catch (error) {
    setStatus(`取消失败：${messageOf(error)}`, "error");
  }
}

async function saveDraft(): Promise<void> {
  const definition = currentDraft();
  if (!definition) return;
  if (!definition.name.trim()) {
    setStatus("请先填写组合动作名称", "error");
    return;
  }
  if (definition.steps.length === 0) {
    setStatus("至少添加一个步骤", "error");
    return;
  }
  if (isNewMacro.value) {
    definition.version = "1.0.0";
  } else {
    definition.version = nextMacroVersion(definition.version);
  }
  try {
    const library = await saveMacro(definition);
    applyLibrary(library);
    selectedMacroId.value = definition.macroId;
    wizardVisible.value = false;
    draft.value = null;
    testReport.value = null;
    setStatus("组合动作已保存", "success");
  } catch (error) {
    setStatus(`保存失败：${messageOf(error)}`, "error");
  }
}

async function testSavedMacro(macroId: string): Promise<void> {
  testReport.value = null;
  try {
    await testMacro(macroId);
    testRunId.value = macroId;
    testRunning.value = true;
    setStatus("组合动作已开始执行");
    startPollingRun(macroId);
  } catch (error) {
    setStatus(testFailureText(error), "error");
  }
}

/**
 * 测试启动失败的文案：Rust 返回稳定 token（`invalid:name_empty` 等），
 * 映射成用户语言；无法识别的（传输层错误）保留原文。
 */
function testFailureText(error: unknown): string {
  const raw = messageOf(error);
  const mapped = macroStartErrorLabel(raw);
  return mapped ? `测试失败：${mapped}` : `测试失败：${raw}`;
}

function stepReportFor(stepId: string) {
  return testReport.value?.steps.find((report) => report.stepId === stepId) ?? null;
}

onMounted(async () => {
  try {
    presetApps.value = await listPresetApps();
    registerPresetAppNames(presetApps.value);
  } catch {
    presetApps.value = [];
  }
  await refreshFocusProfiles();
  await reloadLibrary();
});
</script>

<template>
  <div class="macros-page">
    <div class="page-header">
      <h1>组合动作</h1>
    </div>

    <!-- 库浏览 -->
    <div v-if="!wizardVisible" class="library-body">
      <aside class="library-pane">
        <div class="library-title-row">
          <span class="library-title">我的组合动作</span>
          <span class="count-badge">{{ definitions.length }}</span>
        </div>
        <input
          v-model="search"
          class="search-input"
          type="search"
          placeholder="搜索组合动作"
          aria-label="搜索组合动作"
        />
        <div class="macro-list" role="listbox" aria-label="组合动作列表">
          <button
            v-for="definition in filteredDefinitions"
            :key="definition.macroId"
            type="button"
            class="macro-item"
            :class="{ selected: definition.macroId === selectedMacroId }"
            :aria-selected="definition.macroId === selectedMacroId"
            role="option"
            @click="selectedMacroId = definition.macroId"
          >
            <span class="name">{{ definition.name }}</span>
            <span class="meta">{{ definition.steps.length }} 个步骤</span>
          </button>
          <p v-if="!loading && definitions.length === 0" class="empty-hint">
            还没有组合动作。创建一个，把多个操作合并成一次执行。
          </p>
        </div>
        <div class="library-actions">
          <button class="btn primary block" type="button" @click="startCreate">＋ 新建</button>
          <div class="btn-row">
            <button class="btn" type="button" @click="onImport">导入</button>
            <button class="btn" type="button" :disabled="definitions.length === 0" @click="onExport">
              导出
            </button>
          </div>
        </div>
      </aside>

      <section class="detail-pane">
        <template v-if="selectedDefinition">
          <div class="detail-head">
            <h2>{{ selectedDefinition.name }}</h2>
            <span class="spacer"></span>
            <button class="btn" type="button" @click="testSavedMacro(selectedDefinition.macroId)">
              测试动作
            </button>
            <button class="btn primary" type="button" @click="startEdit(selectedDefinition)">
              编辑
            </button>
          </div>
          <ol class="step-list">
            <li v-for="step in selectedDefinition.steps" :key="step.stepId" class="step-item">
              <span class="step-number">{{ selectedDefinition.steps.indexOf(step) + 1 }}</span>
              <span class="step-text">
                <span class="step-title">{{ macroActionLabel(step.action) }}</span>
                <span class="step-summary">{{ macroStepSummary(step) }}</span>
              </span>
              <span v-if="stepReportFor(step.stepId)" class="step-outcome">
                {{ macroStepOutcomeLabel(stepReportFor(step.stepId)!.outcome) }}
              </span>
            </li>
          </ol>
          <div class="detail-actions">
            <button class="btn" type="button" @click="duplicateMacro(selectedDefinition)">
              复制
            </button>
            <button class="btn danger" type="button" @click="toggleDelete(selectedDefinition.macroId)">
              删除
            </button>
          </div>
          <div v-if="pendingDeleteId === selectedDefinition.macroId" class="delete-confirm">
            <p class="delete-confirm-lead">删除后不可恢复。</p>
            <p v-if="deleteReferenceText" class="delete-confirm-impact">
              这些按键会改为<strong>未设置</strong>：{{ deleteReferenceText }}
            </p>
            <p
              v-if="deleteReferences && deleteReferences.nestedIn.length > 0"
              class="delete-confirm-impact"
            >
              这些组合动作嵌套引用了它，删除后会显示<strong>不可用</strong>：{{
                deleteReferences.nestedIn.join("、")
              }}
            </p>
            <p v-if="deleteReferences && !deleteReferenceText && deleteReferences.nestedIn.length === 0" class="delete-confirm-impact">
              没有按键或组合动作引用它。
            </p>
            <div class="btn-row">
              <button class="btn" type="button" @click="toggleDelete(selectedDefinition.macroId)">取消</button>
              <button
                class="btn danger"
                type="button"
                @click="confirmDelete(selectedDefinition.macroId)"
              >
                删除
              </button>
            </div>
          </div>
        </template>
        <div v-else class="empty-state">
          <span class="empty-icon" aria-hidden="true">▤</span>
          <h3>创建第一个组合动作</h3>
          <p>把打开 App、按键或输入框聚焦组合起来，再设到遥控器按键上。</p>
          <button class="btn primary" type="button" @click="startCreate">创建组合动作</button>
          <p class="hint">创建后可继续添加步骤并测试执行。</p>
        </div>
      </section>
    </div>

    <!-- 编辑向导 -->
    <template v-else>
      <div v-if="draft" class="wizard">
        <header class="wizard-head">
          <h2>{{ isNewMacro ? "新建组合动作" : "编辑组合动作" }}</h2>
          <div class="steps-indicator">
            <button
              class="step-pill"
              :class="{ active: editorStep === 'basic' }"
              type="button"
              @click="goToStep('basic')"
            >
              <span class="dot">1</span>基本设置
            </button>
            <span class="line"></span>
            <button
              class="step-pill"
              :class="{ active: editorStep === 'steps' }"
              type="button"
              @click="goToStep('steps')"
            >
              <span class="dot">2</span>编排步骤
            </button>
            <span class="line"></span>
            <button
              class="step-pill"
              :class="{ active: editorStep === 'review' }"
              type="button"
              @click="goToStep('review')"
            >
              <span class="dot">3</span>测试与保存
            </button>
          </div>
        </header>

        <!-- ① 基本设置 -->
        <div v-if="editorStep === 'basic'" class="wizard-body basic-body">
          <label class="field">
            <span class="field-label">组合动作名称</span>
            <input
              class="text-input"
              type="text"
              :value="draft.name"
              :maxlength="MAX_NAME_CHARS"
              placeholder="例如：打开 Codex 并聚焦输入框"
              @input="renameDraft(($event.target as HTMLInputElement).value)"
            />
            <span class="field-hint">最多 {{ MAX_NAME_CHARS }} 个字符。</span>
          </label>
          <div v-if="draft.steps.length === 0" class="first-action">
            <p class="field-label">选择第一个动作</p>
            <div class="action-grid">
              <button
                v-for="action in ACTION_ORDER"
                :key="action"
                class="action-card"
                type="button"
                @click="addStep(action)"
              >
                {{ macroActionLabel(action) }}
              </button>
            </div>
            <p class="field-hint">选择动作后，点击“下一步”配置；名称和步骤在最后一起保存。</p>
          </div>
          <p v-else class="field-hint">
            已有 {{ draft.steps.length }} 个步骤，可在下一步修改顺序和参数。
          </p>
        </div>

        <!-- ② 编排步骤 -->
        <div v-else-if="editorStep === 'steps'" class="wizard-body steps-body">
          <aside class="steps-pane">
            <div class="name-banner">{{ draft.name || "未命名" }}</div>
            <div class="step-edit-list">
              <div
                v-for="(step, index) in draft.steps"
                :key="step.stepId"
                class="step-edit-item"
                :class="{ selected: step.stepId === selectedStepId }"
                @click="selectStep(step.stepId)"
              >
                <span class="step-number">{{ index + 1 }}</span>
                <span class="grow">
                  <span class="step-title">{{ macroActionLabel(step.action) }}</span>
                  <span class="step-summary">{{ macroStepSummary(step) }}</span>
                </span>
                <span class="move-col">
                  <button
                    type="button"
                    :disabled="index === 0"
                    aria-label="上移步骤"
                    @click.stop="moveStep(index, -1)"
                  >
                    ▲
                  </button>
                  <button
                    type="button"
                    :disabled="index === draft.steps.length - 1"
                    aria-label="下移步骤"
                    @click.stop="moveStep(index, 1)"
                  >
                    ▼
                  </button>
                </span>
              </div>
              <p v-if="draft.steps.length === 0" class="empty-hint">请从下方添加第一个步骤。</p>
            </div>
            <div class="add-steps">
              <p class="field-label">添加步骤</p>
              <div class="action-grid">
                <button
                  v-for="action in ACTION_ORDER"
                  :key="action"
                  class="action-card"
                  type="button"
                  @click="addStep(action)"
                >
                  {{ macroActionLabel(action) }}
                </button>
              </div>
            </div>
          </aside>

          <section class="step-editor">
            <template v-if="selectedStepIndex >= 0">
              <article class="step-card">
                <header class="step-card-head">
                  <span class="label">步骤 {{ selectedStepIndex + 1 }}</span>
                  <span class="title">{{ macroActionLabel(draft.steps[selectedStepIndex].action) }}</span>
                  <span class="spacer"></span>
                  <button
                    class="btn"
                    type="button"
                    @click="runStepTest(draft.steps[selectedStepIndex].stepId)"
                  >
                    测试这步
                  </button>
                  <button
                    class="btn danger"
                    type="button"
                    @click="removeStep(draft.steps[selectedStepIndex].stepId)"
                  >
                    删除
                  </button>
                </header>

                <!-- 打开应用 -->
                <div
                  v-if="draft.steps[selectedStepIndex].action === 'open_application'"
                  class="step-card-body"
                >
                  <label class="field">
                    <span class="field-label">要使用的 App</span>
                    <select
                      class="text-input"
                      :value="draft.steps[selectedStepIndex].parameters.target ?? ''"
                      @change="renameStepTarget(($event.target as HTMLSelectElement).value)"
                    >
                      <option v-for="app in presetApps" :key="app.id" :value="app.id">
                        {{ app.name }}
                      </option>
                    </select>
                  </label>
                  <span class="field-hint">未运行时会先启动；已运行时直接切换到前台。</span>
                </div>

                <!-- 等待应用 -->
                <div
                  v-else-if="draft.steps[selectedStepIndex].action === 'wait_for_application'"
                  class="step-card-body"
                >
                  <label class="field">
                    <span class="field-label">要等待的 App</span>
                    <select
                      class="text-input"
                      :value="draft.steps[selectedStepIndex].parameters.target ?? ''"
                      @change="renameStepTarget(($event.target as HTMLSelectElement).value)"
                    >
                      <option v-for="app in presetApps" :key="app.id" :value="app.id">
                        {{ app.name }}
                      </option>
                    </select>
                  </label>
                  <div class="field">
                    <span class="field-label">等待到什么时候</span>
                    <div class="segmented">
                      <button
                        type="button"
                        :class="{ on: draft.steps[selectedStepIndex].parameters.state === 'running' }"
                        @click="updateSelectedStep((step) => { step.parameters.state = 'running'; })"
                      >
                        App 已打开
                      </button>
                      <button
                        type="button"
                        :class="{ on: draft.steps[selectedStepIndex].parameters.state !== 'running' }"
                        @click="updateSelectedStep((step) => { step.parameters.state = 'visible_window'; })"
                      >
                        App 窗口已出现
                      </button>
                    </div>
                  </div>
                  <span class="field-hint">最多 5 秒；超时后停止执行并提示。</span>
                </div>

                <!-- 单键或快捷键 -->
                <div
                  v-else-if="draft.steps[selectedStepIndex].action === 'send_shortcut'"
                  class="step-card-body"
                >
                  <div class="field">
                    <span class="field-label">已录入</span>
                    <span class="chord-display" data-testid="chord-display">
                      {{
                        draft.steps[selectedStepIndex].parameters.chord
                          ? macroChordLabel(draft.steps[selectedStepIndex].parameters.chord!)
                          : "尚未录入按键"
                      }}
                    </span>
                  </div>
                  <div class="btn-row">
                    <button class="btn primary" type="button" @click="beginCapture">开始录入</button>
                  </div>
                  <span class="field-hint">
                    点击“开始录入”后，只捕获下一次完整的单键或组合键，然后立即停止。
                  </span>
                </div>

                <!-- 聚焦输入框 -->
                <div
                  v-else-if="draft.steps[selectedStepIndex].action === 'focus_input'"
                  class="step-card-body"
                >
                  <div class="field">
                    <span class="field-label">目标 App</span>
                    <select
                      class="text-input"
                      :value="draft.steps[selectedStepIndex].parameters.target ?? ''"
                      @change="renameStepTarget(($event.target as HTMLSelectElement).value)"
                    >
                      <option v-for="app in presetApps" :key="app.id" :value="app.id">
                        {{ app.name }}
                      </option>
                    </select>
                    <span
                      class="focus-state"
                      :data-ready="
                        focusReady(draft.steps[selectedStepIndex].parameters.target) ||
                          focusPointReady(draft.steps[selectedStepIndex].parameters.target)
                      "
                      data-testid="focus-state"
                    >
                      {{
                        focusReady(draft.steps[selectedStepIndex].parameters.target)
                          ? "已学习这个 App 的输入框"
                          : focusPointReady(draft.steps[selectedStepIndex].parameters.target)
                            ? "已记录这个 App 的点击位置"
                            : "还没有学习过这个 App 的输入框"
                      }}
                    </span>
                  </div>
                  <div class="btn-row">
                    <button class="btn primary" type="button" @click="startLearning">
                      {{ focusReady(draft.steps[selectedStepIndex].parameters.target) ? "重新学习" : "学习输入框" }}
                    </button>
                    <button
                      class="btn"
                      type="button"
                      @click="startPointRecording"
                      title="目标应用在 UIA 里没有输入框时（例如微信 4.x）：把鼠标停在目标输入框上，按这个按钮记录位置"
                    >
                      {{ focusPointReady(draft.steps[selectedStepIndex].parameters.target) ? "重新记录位置" : "记录点击位置" }}
                    </button>
                    <span
                      v-if="learningStepId === draft.steps[selectedStepIndex].stepId"
                      class="countdown"
                    >
                      <template v-if="pointCountdown > 0">
                        {{ pointCountdown }} 秒后自动记录鼠标位置：请先把鼠标停在目标输入框上
                      </template>
                      <template v-else>
                        {{ learningCountdown }} 秒后自动记录，请切换并点击目标输入框
                      </template>
                    </span>
                  </div>
                  <span class="field-hint">
                    前置：① 目标 App 已经打开；② 倒计时结束前切到它、把光标点进要聚焦的输入框并保持在那里
                    （密码框、只读区域不会被记录；地址栏可以，因为你是主动点的）；③ 倒计时结束时自动采样并记录。
                    目标应用完全没有向系统暴露输入框时（微信 4.x 实测），改用「记录点击位置」：
                    把鼠标停在要点击的输入框上，6 秒后记录该相对位置，之后按这个位置点击聚焦。
                    记录只属于这台电脑；也可以在按键页记录，两边共用同一份档案。
                  </span>
                </div>

                <!-- 打开网址 -->
                <div
                  v-else-if="draft.steps[selectedStepIndex].action === 'open_url'"
                  class="step-card-body"
                >
                  <label class="field">
                    <span class="field-label">网址</span>
                    <input
                      class="text-input"
                      type="url"
                      :value="draft.steps[selectedStepIndex].parameters.url ?? ''"
                      placeholder="输入完整网址，例如 https://www.example.com"
                      @input="updateSelectedStep((step) => { step.parameters.url = ($event.target as HTMLInputElement).value; })"
                    />
                  </label>
                  <span class="field-hint">
                    会用默认浏览器打开。只能打开普通网页，不能打开电脑里的文件或 App 内的特殊页面。
                  </span>
                </div>

                <!-- 执行已有组合动作 -->
                <div
                  v-else-if="draft.steps[selectedStepIndex].action === 'run_macro'"
                  class="step-card-body"
                >
                  <label class="field">
                    <span class="field-label">选择组合动作</span>
                    <select
                      class="text-input"
                      :value="draft.steps[selectedStepIndex].parameters.nestedMacroId ?? ''"
                      @change="
                        updateSelectedStep((step) => {
                          const id = ($event.target as HTMLSelectElement).value;
                          step.parameters.nestedMacroId = id || undefined;
                          step.parameters.nestedMacroName =
                            nestedCandidates.find((item) => item.macroId === id)?.name ?? undefined;
                        })
                      "
                    >
                      <option value="">尚未选择</option>
                      <option v-for="item in nestedCandidates" :key="item.macroId" :value="item.macroId">
                        {{ item.name }}
                      </option>
                    </select>
                  </label>
                  <span class="field-hint">
                    这里会使用所选组合动作最后一次保存的内容。组合动作不能直接或间接执行自己，最多嵌套 8 层。
                  </span>
                </div>
              </article>
            </template>
            <p v-else class="empty-hint">请从左侧添加第一个步骤。</p>
          </section>
        </div>

        <!-- ③ 测试与保存 -->
        <div v-else class="wizard-body review-body">
          <div class="review-summary">
            <div>
              <p class="review-name">{{ draft.name || "未命名" }}</p>
              <p class="field-hint">
                {{ draft.steps.length }} 个步骤 · 可先测试，再保存。测试不是保存的必要条件。
              </p>
            </div>
            <span class="spacer"></span>
            <button v-if="testRunning" class="btn" type="button" @click="cancelTest">取消</button>
            <button v-else class="btn primary" type="button" @click="runDraftTest">▶ 测试动作</button>
          </div>
          <ol class="step-list">
            <li v-for="(step, index) in draft.steps" :key="step.stepId" class="step-item">
              <span class="step-number">{{ index + 1 }}</span>
              <span class="step-text">
                <span class="step-title">{{ macroActionLabel(step.action) }}</span>
                <span class="step-summary">{{ macroStepSummary(step) }}</span>
              </span>
              <span v-if="stepReportFor(step.stepId)" class="step-outcome">
                {{ macroStepOutcomeLabel(stepReportFor(step.stepId)!.outcome) }}
              </span>
              <span v-else class="step-outcome muted">未测试</span>
            </li>
          </ol>
        </div>

        <footer class="wizard-foot">
          <button class="btn" type="button" @click="cancelEditing">取消编辑</button>
          <span class="spacer"></span>
          <button
            v-if="editorStep !== 'basic'"
            class="btn"
            type="button"
            @click="goToStep(editorStep === 'review' ? 'steps' : 'basic')"
          >
            上一步
          </button>
          <button
            v-if="editorStep !== 'review'"
            class="btn primary"
            type="button"
            :disabled="editorStep === 'basic' && (!draft.name.trim() || draft.steps.length === 0)"
            @click="goToStep(editorStep === 'basic' ? 'steps' : 'review')"
          >
            下一步
          </button>
          <button v-else class="btn primary" type="button" :disabled="!draftValid" @click="saveDraft">
            保存修改
          </button>
        </footer>
      </div>
    </template>

    <p class="status-line" :data-kind="statusKind">{{ statusMessage }}</p>
    <p class="info-line">
      ⓘ “学习输入框”只负责把光标放到目标 App 的输入位置；组合动作里的快捷键只在这台电脑上生效，换电脑后需要重新录入。
    </p>
  </div>
</template>

<style scoped>
.macros-page {
  display: flex;
  flex-direction: column;
  min-height: 0;
  flex: 1;
}
.page-header {
  padding: 16px 22px 14px;
  border-bottom: 1px solid var(--border);
}
.page-header h1 {
  margin: 0;
  font-size: 24px;
  font-weight: 600;
}
.library-body {
  flex: 1;
  min-height: 0;
  display: flex;
}
.library-pane {
  width: 264px;
  flex: none;
  border-right: 1px solid var(--border);
  background: var(--surface-subtle);
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
}
.library-title-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.library-title {
  font-size: 16px;
  font-weight: 600;
}
.count-badge {
  margin-left: auto;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
  background: var(--pending-surface);
  border-radius: 99px;
  padding: 1px 8px;
}
.search-input,
.text-input {
  width: 100%;
  border: 1px solid var(--border-strong);
  border-radius: 6px;
  padding: 6px 10px;
  font-size: 13px;
  background: var(--card);
  color: var(--text-primary);
}
.macro-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.macro-item {
  text-align: left;
  border: 0;
  background: none;
  border-radius: 9px;
  padding: 9px 10px;
  display: flex;
  flex-direction: column;
  gap: 3px;
  cursor: pointer;
  color: inherit;
}
.macro-item:hover {
  background: var(--surface-hover);
}
.macro-item.selected {
  background: var(--accent-surface);
}
.macro-item .name {
  font-size: 13px;
  font-weight: 600;
}
.macro-item .meta {
  font-size: 12px;
  color: var(--text-secondary);
}
.library-actions {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.btn {
  border: 1px solid var(--border-strong);
  background: var(--surface-control);
  color: var(--text-control);
  border-radius: 7px;
  padding: 6px 12px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
}
.btn:hover {
  background: var(--surface-hover);
}
.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.btn.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--text-on-accent);
}
.btn.danger {
  color: var(--error-text);
}
.btn.block {
  width: 100%;
}
.btn-row {
  display: flex;
  gap: 8px;
  align-items: center;
}
.detail-pane {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 20px 22px;
}
.detail-head {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 14px;
}
.detail-head h2 {
  margin: 0;
  font-size: 20px;
  font-weight: 600;
}
.spacer {
  flex: 1;
}
.step-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.step-item {
  display: flex;
  gap: 12px;
  align-items: flex-start;
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 12px 14px;
}
.step-number {
  width: 24px;
  height: 24px;
  border-radius: 7px;
  background: var(--accent-surface);
  color: var(--accent-text);
  font-size: 12px;
  font-weight: 700;
  display: flex;
  align-items: center;
  justify-content: center;
  flex: none;
}
.step-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex: 1;
  min-width: 0;
}
.step-title {
  font-size: 13px;
  font-weight: 600;
}
.step-summary {
  font-size: 12px;
  color: var(--text-secondary);
  overflow-wrap: anywhere;
}
.step-outcome {
  font-size: 12px;
  font-weight: 600;
  flex: none;
}
.step-outcome.muted {
  color: var(--text-secondary);
  font-weight: 400;
}
.detail-actions {
  display: flex;
  gap: 8px;
  margin-top: 14px;
}
.delete-confirm {
  margin-top: 14px;
  border: 1px solid rgba(155, 49, 47, 0.35);
  background: var(--error-surface);
  color: var(--error-text);
  border-radius: 10px;
  padding: 12px 14px;
  font-size: 13px;
}
.delete-confirm-lead {
  margin: 0 0 6px;
  font-weight: 600;
}
.delete-confirm-impact {
  margin: 0 0 6px;
  line-height: 1.5;
}
.delete-confirm .btn-row {
  margin-top: 10px;
}
.empty-state {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  text-align: center;
}
.empty-icon {
  font-size: 40px;
  color: var(--accent);
}
.empty-state h3 {
  margin: 0;
  font-size: 20px;
  font-weight: 600;
}
.empty-state p {
  margin: 0;
  font-size: 14px;
  color: var(--text-secondary);
  max-width: 430px;
}
.empty-hint {
  font-size: 13px;
  color: var(--text-secondary);
  margin: 0;
}
.hint {
  font-size: 12px;
}
.wizard {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.wizard-head {
  padding: 14px 22px 12px;
  border-bottom: 1px solid var(--border);
}
.wizard-head h2 {
  margin: 0 0 12px;
  font-size: 20px;
  font-weight: 600;
}
.steps-indicator {
  display: flex;
  align-items: center;
  gap: 8px;
}
.step-pill {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary);
  background: none;
  border: 0;
  cursor: pointer;
}
.step-pill .dot {
  width: 22px;
  height: 22px;
  border-radius: 50%;
  background: var(--track);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
  font-weight: 700;
}
.step-pill.active {
  color: var(--accent-text);
}
.step-pill.active .dot {
  background: var(--accent);
  color: var(--text-on-accent);
}
.line {
  flex: 1;
  height: 1px;
  background: var(--border-strong);
  min-width: 24px;
}
.wizard-body {
  flex: 1;
  min-height: 0;
  display: flex;
}
.basic-body {
  flex-direction: column;
  gap: 20px;
  padding: 20px 22px;
  overflow-y: auto;
}
.review-body {
  flex-direction: column;
  gap: 14px;
  padding: 20px 22px;
  overflow-y: auto;
}
.steps-body {
  min-height: 0;
}
.steps-pane {
  width: 300px;
  flex: none;
  border-right: 1px solid var(--border);
  background: var(--surface-subtle);
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
}
.name-banner {
  font-size: 14px;
  font-weight: 600;
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 10px 12px;
}
.step-edit-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.step-edit-item {
  display: flex;
  gap: 8px;
  align-items: flex-start;
  border-radius: 9px;
  padding: 8px;
  cursor: pointer;
  border: 1px solid transparent;
}
.step-edit-item:hover {
  background: var(--surface-hover);
}
.step-edit-item.selected {
  background: var(--accent-surface);
  border-color: var(--accent-border);
}
.step-edit-item .grow {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.move-col {
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.move-col button {
  border: 0;
  background: none;
  color: var(--text-secondary);
  cursor: pointer;
  font-size: 11px;
  width: 20px;
  height: 16px;
  border-radius: 4px;
}
.move-col button:disabled {
  opacity: 0.3;
  cursor: default;
}
.add-steps {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.action-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 8px;
}
.first-action .action-grid {
  grid-template-columns: repeat(3, minmax(0, 1fr));
  margin-top: 10px;
}
.action-card {
  border: 1px solid var(--border-strong);
  background: var(--card);
  border-radius: 9px;
  padding: 10px;
  cursor: pointer;
  text-align: left;
  font-size: 12.5px;
  font-weight: 600;
  color: var(--text-control);
}
.action-card:hover {
  border-color: var(--accent-border);
  background: var(--accent-hover);
}
.step-editor {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 18px 22px;
}
.step-card {
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 12px;
  overflow: hidden;
}
.step-card-head {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border);
  background: var(--surface-subtle);
}
.step-card-head .label {
  font-size: 12px;
  color: var(--text-secondary);
  font-weight: 600;
}
.step-card-head .title {
  font-size: 15px;
  font-weight: 700;
  color: var(--accent-text);
}
.step-card-body {
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.field-label {
  font-size: 13px;
  font-weight: 600;
}
.field-hint {
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.5;
}
.chord-display {
  font-size: 15px;
  font-weight: 600;
  background: var(--surface-subtle);
  border: 1px dashed var(--border-strong);
  border-radius: 8px;
  padding: 8px 12px;
  display: inline-block;
  width: fit-content;
}
.segmented {
  display: flex;
  gap: 3px;
  background: var(--surface-subtle);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 3px;
  max-width: 420px;
}
.segmented button {
  flex: 1;
  border: 0;
  background: none;
  padding: 5px 10px;
  font-size: 12.5px;
  font-weight: 600;
  color: var(--text-secondary);
  border-radius: 6px;
  cursor: pointer;
}
.segmented button.on {
  background: var(--card);
  color: var(--accent-text);
}
.countdown {
  display: inline-flex;
  align-items: center;
  background: var(--warning-surface);
  color: var(--warning);
  border-radius: 8px;
  padding: 6px 10px;
  font-size: 12px;
  font-weight: 600;
}
.focus-state {
  font-size: 12px;
  font-weight: 600;
  color: var(--warning);
}
.focus-state[data-ready="true"] {
  color: var(--success-text);
}
.review-summary {
  display: flex;
  align-items: center;
  gap: 12px;
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 12px 16px;
}
.review-name {
  margin: 0;
  font-size: 16px;
  font-weight: 700;
}
.wizard-foot {
  flex: none;
  border-top: 1px solid var(--border);
  padding: 10px 22px;
  display: flex;
  align-items: center;
  gap: 10px;
}
.status-line {
  margin: 0;
  border-top: 1px solid var(--border);
  padding: 8px 22px;
  font-size: 12px;
  color: var(--text-secondary);
}
.status-line[data-kind="success"] {
  color: var(--success-text);
}
.status-line[data-kind="error"] {
  color: var(--error-text);
}
.info-line {
  margin: 0;
  border-top: 1px solid var(--border);
  background: var(--info-surface);
  color: var(--info-text);
  padding: 8px 22px;
  font-size: 12px;
  line-height: 1.5;
}
</style>
