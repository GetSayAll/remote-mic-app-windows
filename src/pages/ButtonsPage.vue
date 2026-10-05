<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import RegisteredAppsDialog from "../components/RegisteredAppsDialog.vue";
import EnhancedCaptureConfirmDialog from "../components/EnhancedCaptureConfirmDialog.vue";
import BatteryIndicator from "../components/BatteryIndicator.vue";
import { reportFrontendEvent } from "../lib/frontend-diagnostics";
import { DEVICE_ACTION_SECTION_ENABLED } from "../lib/feature-flags";
import {
  actionSummary,
  buttonLabel,
  buttonLabels,
  buttonTriggerLabel,
  chordLabel,
  connectRemote,
  connectionPhaseLabel,
  exportButtonMappingConfiguration,
  getButtonMappingSnapshot,
  getButtonMappings,
  disableRc003Capture,
  enableRc003Capture,
  getRc003BridgeSnapshot,
  getRc003TaskStatus,
  identityShortcutByButton,
  importButtonMappingConfiguration,
  learnFocusTarget,
  listPresetApps,
  mouseClickLabels,
  mouseMoveLabels,
  pickCustomApp,
  registerPresetAppNames,
  remoteModelLabel,
  resetButtonMappings,
  saveButtonMappings,
  scanPairedRemotes,
  shortcutCapability,
  startRawInput,
  startShortcutCapture,
  stopRawInput,
  stopShortcutCapture,
  subscribeButtonEdges,
  subscribeButtonGestures,
  subscribeShortcutCaptureEdges,
  type ButtonAction,
  type ButtonActions,
  type ButtonEdge,
  type ButtonMappingSnapshot,
  type ButtonMappings,
  type ButtonTrigger,
  type CustomAppPick,
  type AppFocusProfile,
  type FiredGesture,
  type FocusStrategy,
  type KeyCode,
  type MoveDirection,
  type PairedRemote,
  type PresetAppInfo,
  type RawInputPhase,
  type Rc003BridgePhase,
  type Rc003TaskStatus,
  type RemoteButton,
  type RemoteModel,
  type RuntimeSnapshot,
  type ShortcutCaptureEdge,
  testAppFocus,
} from "../lib/bridge";

const props = defineProps<{ runtime: RuntimeSnapshot | null }>();

/** 画布几何：对齐 Mac RemoteMappingCanvas——高度固定 570，宽度流式
 * （占满容器，ResizeObserver 观测）；卡宽 = clamp((宽-260)/2, 270, 300)，
 * 与 Mac cardWidth(for:) 同公式；容器不足最小画布 800px 时由 CSS
 * --map-scale 连续缩放兜底（小于最小窗口的恢复态窗口）。 */
const CANVAS_MIN_WIDTH = 800;
const CANVAS_HEIGHT = 570;
const REMOTE_WIDTH = 202;
const REMOTE_HEIGHT = 410;
const CARD_HEIGHT = 72;
const REMOTE_TOP = (CANVAS_HEIGHT - REMOTE_HEIGHT) / 2;

const canvasEl = ref<HTMLElement | null>(null);
const canvasWidth = ref(CANVAS_MIN_WIDTH);
const cardWidth = computed(() =>
  Math.min(300, Math.max(270, (canvasWidth.value - 260) / 2)),
);
const remoteLeft = computed(() => (canvasWidth.value - REMOTE_WIDTH) / 2);

interface Placement {
  button: RemoteButton;
  side: "left" | "right";
  anchor: [number, number];
  targetY: number;
}

/** 按键卡片布局表：对齐 Mac RemoteMappingLayout.buttonPlacements。 */
const PLACEMENTS: Placement[] = [
  { button: "power", side: "left", anchor: [0.386, 0.099], targetY: 0.08 },
  { button: "up", side: "left", anchor: [0.502, 0.179], targetY: 0.23 },
  { button: "left", side: "left", anchor: [0.362, 0.246], targetY: 0.38 },
  { button: "back", side: "left", anchor: [0.406, 0.389], targetY: 0.53 },
  { button: "home", side: "left", anchor: [0.406, 0.479], targetY: 0.68 },
  { button: "menu", side: "left", anchor: [0.406, 0.569], targetY: 0.83 },
  { button: "right", side: "right", anchor: [0.638, 0.246], targetY: 0.215 },
  { button: "ok", side: "right", anchor: [0.502, 0.246], targetY: 0.36 },
  { button: "down", side: "right", anchor: [0.502, 0.317], targetY: 0.505 },
  { button: "volume_up", side: "right", anchor: [0.604, 0.39], targetY: 0.65 },
  { button: "volume_down", side: "right", anchor: [0.604, 0.48], targetY: 0.795 },
  { button: "tv", side: "right", anchor: [0.604, 0.569], targetY: 0.94 },
];
const VOICE_PLACEMENT: Placement = {
  button: "ok", // 语音卡不对应 RemoteButton；占位仅用于定位。
  side: "right",
  anchor: [0.63, 0.099],
  targetY: 0.07,
};
const TRIGGERS: ButtonTrigger[] = ["single", "double", "long"];

/** 当前连接的遥控器型号（未连接时 unknown，按 RC003 保守处理）。 */
const remoteModel = computed<RemoteModel>(
  () => props.runtime?.platform.connection.remoteModel ?? "unknown",
);

/**
 * 头部设备胶囊（2026-10-01）：优先显示已识别的型号（RC001 = 小米蓝牙语音遥控器 2、
 * RC003 = 小米蓝牙语音遥控器 2 Pro）；型号未读回时退回蓝牙广播名，未连接时提示未连接。
 */
const deviceLabel = computed(() => {
  if (remoteModel.value !== "unknown") return remoteModelLabel(remoteModel.value);
  return connectionInfo.value?.remoteName ?? "未连接遥控器";
});

function anchorPoint(placement: Placement): { x: number; y: number } {
  return {
    x: remoteLeft.value + REMOTE_WIDTH * placement.anchor[0],
    y: REMOTE_TOP + REMOTE_HEIGHT * placement.anchor[1],
  };
}

/** 照片容器内相对坐标（锚点橙点渲染在 .remote-photo 内部，坐标系是照片自身）。 */
function photoAnchorPoint(placement: Placement): { x: number; y: number } {
  return {
    x: REMOTE_WIDTH * placement.anchor[0],
    y: REMOTE_HEIGHT * placement.anchor[1],
  };
}

function cardTop(placement: Placement): number {
  return placement.targetY * CANVAS_HEIGHT - CARD_HEIGHT / 2;
}

/** 卡片朝向遥控器一侧的边缘中点（箭头/连线的落点基准）。 */
function cardEdgePoint(placement: Placement): { x: number; y: number } {
  return {
    x: placement.side === "left" ? cardWidth.value : canvasWidth.value - cardWidth.value,
    y: placement.targetY * CANVAS_HEIGHT,
  };
}

/** 连线与箭头一体化：线画到箭头底部（距卡边 13px），箭头补足到距卡边 7px，
 * 整体读作一条带箭头的连线（线不再穿过箭头延伸到卡片边缘）。 */
function lineEndPoint(placement: Placement): { x: number; y: number } {
  const edge = cardEdgePoint(placement);
  const direction = placement.side === "left" ? -1 : 1;
  return { x: edge.x - direction * 13, y: edge.y };
}

function connectionPath(placement: Placement): string {
  const start = anchorPoint(placement);
  const end = lineEndPoint(placement);
  const direction = placement.side === "left" ? -1 : 1;
  const distance = Math.min(70, Math.max(34, Math.abs(end.x - start.x) * 0.58));
  const endpointDistance = Math.min(42, Math.max(24, distance * 0.6));
  const control1 = { x: start.x + direction * distance, y: start.y };
  const control2 = { x: end.x - direction * endpointDistance, y: end.y };
  return `M ${start.x.toFixed(1)} ${start.y.toFixed(1)} C ${control1.x.toFixed(1)} ${control1.y.toFixed(1)}, ${control2.x.toFixed(1)} ${control2.y.toFixed(1)}, ${end.x.toFixed(1)} ${end.y.toFixed(1)}`;
}

/** 箭头（与连线同色同类）：尖端距卡片边 7px、底宽 12px/半高 4px，
 * 底部与连线终点重合（整体一条连线）。 */
function arrowPolygon(placement: Placement): string {
  const edge = cardEdgePoint(placement);
  const direction = placement.side === "left" ? -1 : 1;
  const tip = { x: edge.x - direction * 7, y: edge.y };
  const baseX = tip.x - direction * 6;
  return `${tip.x.toFixed(1)},${tip.y.toFixed(1)} ${baseX.toFixed(1)},${(tip.y - 4).toFixed(1)} ${baseX.toFixed(1)},${(tip.y + 4).toFixed(1)}`;
}

/** 按键图标：SVG path 组（24x24 视窗），形状对齐 Mac RemoteMappingCanvas
 * 的 SF Symbols（power/chevron/circle.circle/uturn/speaker/house/line.3/tv）。 */
const buttonIcons: Record<RemoteButton, string[]> = {
  power: ["M12 3v8", "M7.2 6.4a7 7 0 1 0 9.6 0"],
  up: ["M6 14.5l6-6 6 6"],
  down: ["M6 9.5l6 6 6-6"],
  left: ["M14.5 6l-6 6 6 6"],
  right: ["M9.5 6l6 6-6 6"],
  ok: ["M12 3.5a8.5 8.5 0 1 1 0 17 8.5 8.5 0 0 1 0-17z", "M12 10a2.4 2.4 0 1 1 0 4.8 2.4 2.4 0 0 1 0-4.8z"],
  back: ["M9 13.5L4 8.5l5-5", "M4 8.5h10.2a5.5 5.5 0 0 1 0 11H11"],
  home: ["M3.5 11.2L12 3.5l8.5 7.7", "M6 9.5V20.5h12V9.5", "M10 20.5v-5.5h4v5.5"],
  menu: ["M4 6h16", "M4 12h16", "M4 18h16"],
  tv: ["M3 7.5h18a1 1 0 0 1 1 1V18a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V8.5a1 1 0 0 1 1-1z", "M17 3l-5 4-5-4"],
  volume_up: ["M11 5.5L6.5 9H3v6h3.5L11 18.5z", "M15.5 9.5l5 5", "M20.5 9.5l-5 5"],
  volume_down: ["M11 5.5L6.5 9H3v6h3.5L11 18.5z", "M15 12h5.5"],
  volume_mute: ["M11 5.5L6.5 9H3v6h3.5L11 18.5z", "M15.5 9.5l5 5", "M20.5 9.5l-5 5"],
};
/** 语音键图标（Mac mic.fill：实心话筒）。 */
const VOICE_ICON_FILLED = "M12 2.8a3.4 3.4 0 0 1 3.4 3.4v5.6a3.4 3.4 0 0 1-6.8 0V6.2A3.4 3.4 0 0 1 12 2.8z";
const VOICE_ICON_STROKES = ["M6.3 11.5a5.7 5.7 0 0 0 11.4 0", "M12 17.2v3.8"];

const mappings = ref<ButtonMappings>({ enabled: true, actions: {} });
const savedSnapshot = ref<ButtonMappings>({ enabled: true, actions: {} });
/** 已安装的预设应用（打开应用动作可选列表）。 */
const presetApps = ref<PresetAppInfo[]>([]);
const selectedButton = ref<RemoteButton | null>(null);
const editingTarget = ref<{ button: RemoteButton; trigger: ButtonTrigger } | null>(null);
const editorPanel = ref<HTMLElement | null>(null);
const lockSelection = ref(true);
const activeButtons = ref<Set<RemoteButton>>(new Set());
const lastFired = ref<FiredGesture | null>(null);
const firedFlash = ref<{ button: RemoteButton; trigger: ButtonTrigger } | null>(null);
const mappingSnapshot = ref<ButtonMappingSnapshot | null>(null);
const busy = ref(false);
const statusMessage = ref<string | null>(null);
const capturingShortcut = ref(false);
const captureStarting = ref(false);
const captureDisplay = ref<string[]>([]);
const safeCaptureMode = ref(false);
/** 录入快捷键的用途：动作本身，还是「打开应用」的聚焦快捷键。 */
const capturePurpose = ref<"action" | "focus_shortcut">("action");
/** 「学习输入框」状态机。 */
const focusLearnState = ref<"idle" | "learning" | "captured" | "error">("idle");
const focusLearnMessage = ref<string | null>(null);
const focusTestMessage = ref<string | null>(null);
const capturePressedKeys = new Set<KeyCode>();
let capturedChord: KeyCode[] | null = null;
let unlistenEdges: (() => void) | null = null;
let unlistenGestures: (() => void) | null = null;
let unlistenShortcutCapture: (() => void) | null = null;
let captureTimeout: number | null = null;
let captureRequestId = 0;
let snapshotTimer: number | null = null;
let flashTimer: number | null = null;
let resizeObserver: ResizeObserver | null = null;
let unmounted = false;
let resourcesReady = false;

function releasePageResources(): void {
  window.removeEventListener("keydown", handleCaptureKeydown, true);
  window.removeEventListener("keyup", handleCaptureKeyup, true);
  window.removeEventListener("blur", handleCaptureBlur);
  unlistenEdges?.();
  unlistenEdges = null;
  unlistenGestures?.();
  unlistenGestures = null;
  unlistenShortcutCapture?.();
  unlistenShortcutCapture = null;
  if (captureTimeout !== null) window.clearTimeout(captureTimeout);
  captureTimeout = null;
  void stopShortcutCapture();
  if (snapshotTimer !== null) window.clearInterval(snapshotTimer);
  snapshotTimer = null;
  if (flashTimer !== null) window.clearTimeout(flashTimer);
  flashTimer = null;
  resizeObserver?.disconnect();
  resizeObserver = null;
}

const dirty = computed(
  () => JSON.stringify(mappings.value) !== JSON.stringify(savedSnapshot.value),
);

const enabled = computed({
  get: () => mappings.value.enabled,
  set: (value: boolean) => {
    mappings.value = { ...mappings.value, enabled: value };
    void persist("总开关已更新");
  },
});

const voiceActive = computed(
  () => props.runtime?.platform.connection.voiceState === "streaming",
);

function actionsOf(button: RemoteButton): ButtonActions {
  return (
    mappings.value.actions[button] ?? {
      single: { type: "disabled" },
      double: { type: "disabled" },
      long: { type: "disabled" },
    }
  );
}

function actionOf(button: RemoteButton, trigger: ButtonTrigger): ButtonAction {
  return actionsOf(button)[trigger];
}

/** 当前编辑格的打开应用目标（非 open_app 动作返回 null，模板类型收窄用）。 */
function openAppTargetOf(button: RemoteButton, trigger: ButtonTrigger): string | null {
  const action = actionOf(button, trigger);
  return action.type === "open_app" ? action.target : null;
}

/** 当前编辑格的「打开应用」目标（聚焦档案按它键控）。 */
const focusTarget = computed(() =>
  editingTarget.value
    ? openAppTargetOf(editingTarget.value.button, editingTarget.value.trigger)
    : null,
);

/** 该目标的聚焦档案（未配置时 null ＝ 只打开应用）。 */
const focusProfile = computed<AppFocusProfile | null>(() => {
  const target = focusTarget.value;
  if (!target) return null;
  return mappings.value.focusProfiles?.[target] ?? null;
});

const focusStrategyOptions: Array<{ value: FocusStrategy; label: string }> = [
  { value: "open_only", label: "只打开应用" },
  { value: "app_shortcut", label: "用应用快捷键聚焦" },
  { value: "recorded_element", label: "聚焦已记录的输入框" },
];

// 2026-10-03 Andy：先隐藏「打开后聚焦方式」面板（后端能力与已存配置保留，UI 暂不暴露）。
// 需要恢复时把这个开关改回 true 即可；相关测试按同一开关跳过。
const showAppFocusStrategy = false;

const focusFailureLabels: Record<string, string> = {
  self_foreground: "当前就是无线麦自己的窗口，无需聚焦",
  no_candidate: "没有找到可用的输入框",
  not_foreground: "目标应用不在前台",
  target_exited: "目标应用已退出",
  not_accessible: "读不到该应用的界面（对方可能以管理员身份运行）",
  timeout: "等待超时",
  not_configured: "还没有配置聚焦方式",
  cancelled: "已取消",
};

/** 最近一次聚焦结果（失败时给用户可解释的原因）。 */
const lastFocusNotice = computed<string | null>(() => {
  const report = mappingSnapshot.value?.lastFocus;
  if (!report || report.focused) return null;
  return `上次聚焦失败：${focusFailureLabels[report.reason ?? ""] ?? report.reason ?? "未知原因"}`;
});

/** 聚焦档案写回：策略与字段保持自洽（对齐 Rust `AppFocusProfile::normalized`）。 */
function applyFocusStrategy(strategy: FocusStrategy): void {
  const target = focusTarget.value;
  if (!target) return;
  const previous = mappings.value.focusProfiles?.[target];
  const profile: AppFocusProfile = { strategy };
  if (strategy === "app_shortcut" && previous?.shortcut) profile.shortcut = previous.shortcut;
  if (strategy === "recorded_element" && previous?.recorded) profile.recorded = previous.recorded;
  const next = { ...(mappings.value.focusProfiles ?? {}), [target]: profile };
  mappings.value = { ...mappings.value, focusProfiles: next };
  focusLearnMessage.value = null;
  focusTestMessage.value = null;
  void persist("聚焦方式已更新");
}

/** 录入聚焦快捷键的落点（由 `capturePurpose` 分流）。 */
function applyFocusShortcut(keys: KeyCode[]): void {
  const target = focusTarget.value;
  if (!target) return;
  const next = {
    ...(mappings.value.focusProfiles ?? {}),
    [target]: { strategy: "app_shortcut" as const, shortcut: { keys } },
  };
  mappings.value = { ...mappings.value, focusProfiles: next };
  void persist("聚焦快捷键已录入");
}

function clearRecordedFocus(): void {
  const target = focusTarget.value;
  if (!target) return;
  const next = {
    ...(mappings.value.focusProfiles ?? {}),
    [target]: { strategy: "recorded_element" as const },
  };
  mappings.value = { ...mappings.value, focusProfiles: next };
  focusLearnState.value = "idle";
  focusLearnMessage.value = null;
  void persist("已清除记录的输入框");
}

async function beginFocusShortcutCapture(): Promise<void> {
  if (focusLearnState.value === "learning") return;
  capturePurpose.value = "focus_shortcut";
  await beginShortcutCapture();
}

/**
 * 「学习输入框」：3 秒窗口内轮询系统焦点，用户需要在此期间点击目标应用的输入框。
 * 采集只读控件语义（不含输入内容），写入该目标的聚焦档案。
 */
async function startFocusLearning(): Promise<void> {
  if (focusLearnState.value === "learning") return;
  const target = focusTarget.value;
  if (!target) return;
  focusLearnState.value = "learning";
  focusLearnMessage.value = "请在 3 秒内点击目标应用的输入框（点到别处会记录成别的控件）";
  try {
    const recorded = await learnFocusTarget();
    const next = {
      ...(mappings.value.focusProfiles ?? {}),
      [target]: { strategy: "recorded_element" as const, recorded },
    };
    mappings.value = { ...mappings.value, focusProfiles: next };
    await persist();
    focusLearnState.value = "captured";
    focusLearnMessage.value = "已记录该输入框（只记录控件特征，不含输入内容）";
  } catch (error) {
    focusLearnState.value = "error";
    focusLearnMessage.value =
      error instanceof Error ? error.message : String(error);
  }
}

/** 「测试打开与聚焦」：走生产路径（异步受理，结果稍后体现在聚焦结果提示里）。 */
async function testSelectedAppFocus(): Promise<void> {
  const target = focusTarget.value;
  if (!target) return;
  focusTestMessage.value = "已受理：正在打开/切换到该应用并执行聚焦…";
  try {
    await testAppFocus(target);
  } catch (error) {
    focusTestMessage.value = error instanceof Error ? error.message : String(error);
  }
}

/** 预设 id 集合（区分预设与自定义路径目标）。 */
const presetAppIds = computed(() => new Set(presetApps.value.map((app) => app.id)));

/** 已在映射中使用过的自定义应用（路径目标，去重；跨格可复选）。 */
const customApps = computed<Array<{ path: string; name: string }>>(() => {
  const seen = new Map<string, string>();
  for (const app of mappings.value.applications ?? []) {
    seen.set(app.path, app.name);
  }
  for (const actions of Object.values(mappings.value.actions)) {
    // 三列动作（不含「按住连续触发」槽位字段——它不是动作）。
    for (const action of [actions.single, actions.double, actions.long]) {
      if (action.type === "open_app" && !presetAppIds.value.has(action.target)) {
        const base = action.target.split(/[\\/]/).pop() ?? action.target;
        const name = base.replace(/\.(exe|lnk)$/i, "") || action.target;
        if (!seen.has(action.target)) {
          seen.set(action.target, name);
        }
      }
    }
  }
  return [...seen.entries()].map(([path, name]) => ({ path, name }));
});

const appPickerOpen = ref(false);
const appPickerError = ref<string | null>(null);
const appFilter = ref("");
const filteredCustomApps = computed(() => customApps.value.filter(app => app.name.toLocaleLowerCase().includes(appFilter.value.trim().toLocaleLowerCase())));
watch([presetApps, () => mappings.value.applications], () => {
  registerPresetAppNames([...presetApps.value, ...(mappings.value.applications ?? []).map(app => ({ id: app.path, name: app.name }))]);
}, { deep: true });

async function addScannedApps(apps: CustomAppPick[]): Promise<void> {
  if (busy.value) return;
  busy.value = true;
  appPickerError.value = null;
  try {
    const unique = new Map((mappings.value.applications ?? []).map(app => [app.path.toLowerCase(), app]));
    for (const app of apps) unique.set(app.path.toLowerCase(), app);
    const saved = await saveButtonMappings({ ...mappings.value, applications: [...unique.values()] });
    mappings.value = saved;
    savedSnapshot.value = JSON.parse(JSON.stringify(saved)) as ButtonMappings;
    appPickerOpen.value = false;
    statusMessage.value = `已添加 ${apps.length} 个应用，按键绑定未改变`;
  } catch (cause) {
    appPickerError.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    busy.value = false;
  }
}

/** 打开原生文件选择器添加自定义应用，并应用到当前编辑格。 */
async function addCustomApp(): Promise<void> {
  const pick = await pickCustomApp();
  if (!pick || !editingTarget.value) return;
  applyAction({ type: "open_app", target: pick.path });
}

function selectButton(button: RemoteButton): void {
  // 三键在「全按键支持」关闭时置灰禁用：合成事件绕过原生 disabled，这里再守一道。
  if (captureGated(button)) return;
  selectedButton.value = button;
}

function openEditor(button: RemoteButton, trigger: ButtonTrigger): void {
  if (captureGated(button)) return;
  selectedButton.value = button;
  editingTarget.value = { button, trigger };
  if (capturingShortcut.value) void finishShortcutCapture();
}

function applyAction(action: ButtonAction): void {
  const target = editingTarget.value;
  if (!target) return;
  const next: ButtonMappings = {
    ...mappings.value,
    actions: { ...mappings.value.actions },
  };
  const actions = { ...actionsOf(target.button) };
  actions[target.trigger] = action;
  const reconciled = reconcileHoldRepeat(target.button, actions);
  next.actions[target.button] = reconciled.actions;
  mappings.value = next;
  // 对齐 Mac：点击动作即自动保存生效（静默；失败时显示错误信息）。互斥收口
  // 产生的说明随保存一起展示（否则会被 persist 的清理覆盖）。
  void persist(reconciled.notice ?? undefined);
}

/**
 * 预设快捷键分组（对齐 Mac `ButtonActionCategory` 的 basicKeys/systemAndMedia，
 * 并按 Windows 语义适配：Home/End/PageUp/PageDown 属低频导航键、Mac 端基础
 * 按键列表亦无此四键，故移除；复制族从系统组移入基础组，对齐 Mac basicKeys）。
 */
const PRESET_GROUPS: Array<{ label: string; items: Array<{ label: string; keys: KeyCode[] }> }> = [
  {
    label: "基础按键",
    items: [
      { label: "Enter", keys: ["enter"] },
      { label: "Esc", keys: ["escape"] },
      { label: "空格", keys: ["space"] },
      { label: "Tab", keys: ["tab"] },
      { label: "退格", keys: ["backspace"] },
      { label: "删除", keys: ["delete"] },
      { label: "↑", keys: ["up"] },
      { label: "↓", keys: ["down"] },
      { label: "←", keys: ["left"] },
      { label: "→", keys: ["right"] },
      // Home 为"主页键同键映射"的必需预设（能力矩阵 identity 档的唯一
      // 合法目标；Mac 端基础键列表无此键，Windows 端因泄漏对冲需要保留）。
      { label: "Home", keys: ["home"] },
      { label: "复制", keys: ["control", "c"] },
      { label: "粘贴", keys: ["control", "v"] },
      { label: "剪切", keys: ["control", "x"] },
      { label: "全选", keys: ["control", "a"] },
      { label: "撤销", keys: ["control", "z"] },
      { label: "重做", keys: ["control", "y"] },
      { label: "查找", keys: ["control", "f"] },
      { label: "保存", keys: ["control", "s"] },
      { label: "发送", keys: ["control", "enter"] },
      { label: "换行", keys: ["shift", "enter"] },
      { label: "右键菜单", keys: ["apps"] },
      { label: "刷新", keys: ["f5"] },
    ],
  },
  {
    label: "系统与媒体",
    items: [
      { label: "切换窗口", keys: ["alt", "tab"] },
      { label: "显示桌面", keys: ["left_windows", "d"] },
      { label: "关闭窗口", keys: ["control", "w"] },
      { label: "锁定", keys: ["left_windows", "l"] },
      { label: "搜索", keys: ["left_windows", "s"] },
      { label: "截图", keys: ["left_windows", "shift", "s"] },
      { label: "静音", keys: ["volume_mute"] },
      { label: "音量+", keys: ["volume_up"] },
      { label: "音量−", keys: ["volume_down"] },
      { label: "播放/暂停", keys: ["media_play_pause"] },
      { label: "上一首", keys: ["media_prev"] },
      { label: "下一首", keys: ["media_next"] },
    ],
  },
];

const selectedAction = computed(() => editingTarget.value ? actionOf(editingTarget.value.button, editingTarget.value.trigger) : null);
const scrollSteps = computed(() => selectedAction.value?.type === "scroll" ? selectedAction.value.steps ?? 1 : 1);
const moveDistance = computed(() => selectedAction.value?.type === "mouse_move" ? selectedAction.value.distance : 30);
const moveSymbols: Record<MoveDirection, string> = { up: "↑", down: "↓", left: "←", right: "→" };

function updateMouseAmount(event: Event, kind: "scroll" | "mouse_move"): void {
  const input = event.target as HTMLInputElement;
  const value = input.valueAsNumber;
  const maximum = kind === "scroll" ? 100 : 2000;
  if (!Number.isInteger(value) || value < 1 || value > maximum) {
    statusMessage.value = `请输入 1 到 ${maximum} 之间的整数`;
    input.value = String(kind === "scroll" ? scrollSteps.value : moveDistance.value);
    return;
  }
  const action = selectedAction.value;
  if (action?.type === "scroll" && kind === "scroll") void applyAction({ ...action, steps: value });
  if (action?.type === "mouse_move" && kind === "mouse_move") void applyAction({ ...action, distance: value });
}

function isActiveScroll(direction: "up" | "down"): boolean {
  const target = editingTarget.value;
  if (!target) return false;
  const action = actionOf(target.button, target.trigger);
  return action.type === "scroll" && action.direction === direction;
}

function isActivePreset(keys: KeyCode[]): boolean {
  const target = editingTarget.value;
  if (!target) return false;
  const action = actionOf(target.button, target.trigger);
  if (action.type !== "shortcut") return false;
  return action.chord.keys.join("+") === keys.join("+");
}

/**
 * 「按住连续触发」的界面规则（2026-10-05）：
 * - 每键至多一个槽位（单击或长按；双击不参与），开关显示在单击/长按两个
 *   槽位的编辑区，是同一个按键级状态；
 * - 前提：按键在可连续触发范围内 + 该槽位动作「可连续执行」+ 开在单击时
 *   不得同时配置长按动作（互斥）；
 * - 前提被破坏（配置变化）时自动关闭并就地提示；不满足时的开启被阻止并说明。
 * 口径与 Rust `ButtonAction::allows_repeat` / `ButtonMappings::normalized` 同源。
 */
const REPEAT_CAPABLE_BUTTONS: RemoteButton[] = [
  "back",
  "up",
  "down",
  "left",
  "right",
  "volume_up",
  "volume_down",
];
/** 单独修饰键：不可作为「可连续执行」的单键动作（与 Rust `KeyCode::is_modifier` 一致）。 */
const REPEAT_MODIFIER_KEYS = new Set<KeyCode>([
  "control",
  "left_control",
  "right_control",
  "shift",
  "left_shift",
  "right_shift",
  "alt",
  "left_alt",
  "right_alt",
  "left_windows",
  "right_windows",
]);

/** 动作是否可连续执行（与 Rust `ButtonAction::allows_repeat` 同源口径）。 */
function actionAllowsRepeat(action: ButtonAction): boolean {
  if (action.type === "scroll" || action.type === "mouse_move") return true;
  if (action.type !== "shortcut") return false;
  return action.chord.keys.length === 1 && !REPEAT_MODIFIER_KEYS.has(action.chord.keys[0]);
}

function holdRepeatOf(button: RemoteButton): "single" | "long" | undefined {
  return actionsOf(button).holdRepeat;
}

const repeatSwitchVisible = computed(() => {
  const target = editingTarget.value;
  if (!target) return false;
  return (
    REPEAT_CAPABLE_BUTTONS.includes(target.button) &&
    (target.trigger === "single" || target.trigger === "long")
  );
});

/** 当前槽位是否被指定为连续触发槽位。 */
const repeatChecked = computed(() => {
  const target = editingTarget.value;
  return !!target && holdRepeatOf(target.button) === target.trigger;
});

/** 开启被阻止时的原因（null = 可开启；已开启时恒为 null）。 */
const repeatBlockedReason = computed<string | null>(() => {
  const target = editingTarget.value;
  if (!target) return null;
  const actions = actionsOf(target.button);
  if (target.trigger === "long") {
    if (actions.long.type === "disabled") return "请先为该按键配置长按动作";
    if (!actionAllowsRepeat(actions.long)) {
      return "该长按动作不支持按住连续触发（组合快捷键、打开应用等只执行一次）";
    }
    return null;
  }
  if (actions.single.type === "disabled") return "请先为该按键配置单击动作";
  if (!actionAllowsRepeat(actions.single)) {
    return "该动作不支持按住连续触发（组合快捷键、打开应用等只执行一次）";
  }
  if (actions.long.type !== "disabled") {
    return holdRepeatOf(target.button) === "long"
      ? "「按住连续触发」现在开在长按列（长按触发后继续连续）"
      : "该按键已配置长按动作：按住将触发长按；如需连续触发，请把开关开在长按列，或先清除长按动作";
  }
  return null;
});

const repeatSwitchTitle = computed(() => {
  const target = editingTarget.value;
  if (!target) return "";
  if (repeatBlockedReason.value) return repeatBlockedReason.value;
  return target.trigger === "long"
    ? "长按约 0.55 秒触发后，不松手会继续连续执行"
    : "按住不放会连续执行这个动作";
});

/**
 * 动作变更后的「按住连续触发」一致性收口：前提被破坏时自动关闭并返回提示
 * （互斥显式化，不留下用户看不到的无效状态）。提示经 `persist(message)`
 * 展示，避免被 persist 的清理逻辑覆盖。
 */
function reconcileHoldRepeat(
  button: RemoteButton,
  actions: ButtonActions,
): { actions: ButtonActions; notice: string | null } {
  const hold = actions.holdRepeat;
  if (!hold) return { actions, notice: null };
  const target = hold === "long" ? actions.long : actions.single;
  const valid =
    REPEAT_CAPABLE_BUTTONS.includes(button) &&
    target.type !== "disabled" &&
    actionAllowsRepeat(target) &&
    (hold !== "single" || actions.long.type === "disabled");
  if (valid) return { actions, notice: null };
  const next = { ...actions };
  delete next.holdRepeat;
  return {
    actions: next,
    notice:
      hold === "single" && actions.long.type !== "disabled"
        ? "该按键已配置长按动作：按住将触发长按；「按住连续触发」已关闭（如需长按后继续连续，请在长按列开启）"
        : "动作已变更，「按住连续触发」已自动关闭",
  };
}

function toggleHoldRepeat(): void {
  const target = editingTarget.value;
  if (!target) return;
  const next: ButtonMappings = { ...mappings.value, actions: { ...mappings.value.actions } };
  const actions = { ...actionsOf(target.button) };
  if (repeatChecked.value) {
    delete actions.holdRepeat;
    next.actions[target.button] = actions;
    mappings.value = next;
    void persist("已关闭「按住连续触发」");
    return;
  }
  const reason = repeatBlockedReason.value;
  if (reason) {
    statusMessage.value = reason;
    return;
  }
  const moving = holdRepeatOf(target.button) !== undefined;
  actions.holdRepeat = target.trigger === "long" ? "long" : "single";
  next.actions[target.button] = actions;
  mappings.value = next;
  void persist(
    moving
      ? "「按住连续触发」已移到当前列"
      : target.trigger === "long"
        ? "已开启：长按触发后不松手会继续连续"
        : "已开启：按住不放会连续触发",
  );
}

/**
 * 三键（返回/音量±）关闭态的统一说明（2026-09-28 Andy 定稿）：
 * 编辑器能力说明与卡片置灰禁用悬停提示共用，悬停提示在句尾补「开启后恢复正常」
 * （2026-10-03 Andy 定稿）。
 */
const TRI_KEY_CAPTURE_HINT = "返回 / 音量+ / 音量−需要开启全按键支持才能使用";
const TRI_KEY_GATED_TITLE = `${TRI_KEY_CAPTURE_HINT}，开启后恢复正常`;

/**
 * 编辑器提示（信息性）：Home/TV 已落地"遥控器优先"（2026-09-07 方案 C）——
 * 已配置映射且遥控器连接期间原生按键被接管，任意按压（含闲置后首次）严格
 * 单响应；确定/方向的同键映射仍由泄漏对冲保证单响应，其余配置冷首按附带
 * 一次原生动作（结构性泄漏）。
 *
 * 2026-09-23 增补（返回/音量± 产品策略改为"启用"）：三键已恢复可配置，
 * 但能否真正收到边沿取决于型号——界面按型号如实说明，不做静默降级。
 *
 * 2026-10-03 增补：关闭态下三键卡片置灰禁用（captureGated），进不了编辑器；
 * 关闭态整句因此只在"开启后现场关闭开关"这一路径出现（防"界面说明说谎"）。
 */
const capabilityNote = computed<string | null>(() => {
  if (!editingTarget.value) return null;
  const button = editingTarget.value.button;
  // 2026-09-28 Andy 定稿：开启态按「三键 / 其他按键」分两句；关闭态三键
  // 一句话（与开关悬停提示同源），home/tv 与同键映射的关闭态说明保留原口径。
  if (rc003CaptureEnabled.value === true) {
    const isThreeKey =
      button === "back" || button === "volume_up" || button === "volume_down";
    return isThreeKey
      ? "全按键支持已启用，此按键的映射现在生效"
      : "全按键支持已启用，此按键的映射已优化";
  }
  if (button === "back" || button === "volume_up" || button === "volume_down") {
    // 三键的映射路径对两个型号一致（下游同为映射引擎），界面不做型号区分：
    // 文案只随开关状态走。RC001 的三键同样依赖增强捕获（2026-09-25 真机：
    // 开=可映射、关=不映射），不再按"RC001 免助手"设计。
    if (rc003CaptureEnabled.value === false) {
      return TRI_KEY_CAPTURE_HINT;
    }
    return "提示：正在确认按键状态…";
  }
  if (button === "home" || button === "tv") {
    return "提示：保存后，遥控器连接期间这个键只执行你配置的动作；同时物理键盘上的对应按键（Home / `）也会执行同样的动作。断开遥控器或删除本键映射即恢复原样。";
  }
  if (shortcutCapability(button, "single", remoteModel.value) === "identity") {
    const identity = identityShortcutByButton[button];
    const label = identity ? chordLabel({ keys: [identity] }) : "";
    return `提示：闲置约 4 秒后第一次按这个键，可能同时出现一次它原本的按键效果；这段时间内连按不受影响。单击动作若就是该键本身（${label}），多出的那一次会被自动合并。`;
  }
  return null;
});

/**
 * 全按键支持开关的悬停提示（2026-09-28 Andy 定稿）：随开关状态切换两句。
 * 状态未就绪（null）按关闭态口径显示——占位符阶段开关本体都不存在，
 * 真正可悬停时对账大概率已落地。
 */
const captureSwitchTitle = computed(() =>
  rc003CaptureEnabled.value === true
    ? "关闭后返回 / 音量+ / 音量−将不可映射"
    : "开启后支持使用返回 / 音量+ / 音量−，其他按键将同步优化",
);

let saveQueue: Promise<void> = Promise.resolve();
let saveRequest = 0;
async function persist(message?: string): Promise<void> {
  const request = ++saveRequest;
  const payload = JSON.parse(JSON.stringify(mappings.value)) as ButtonMappings;
  busy.value = true;
  statusMessage.value = null;
  const task = saveQueue.then(async () => {
    try {
      const saved = await saveButtonMappings(payload);
      savedSnapshot.value = JSON.parse(JSON.stringify(saved)) as ButtonMappings;
      if (request === saveRequest) {
        mappings.value = saved;
        if (message) statusMessage.value = message;
      }
    } catch (error) {
      if (request === saveRequest) statusMessage.value = error instanceof Error ? error.message : String(error);
    }
  });
  saveQueue = task;
  await task;
  if (request === saveRequest) busy.value = false;
}

async function restoreDefaults(): Promise<void> {
  busy.value = true;
  statusMessage.value = null;
  try {
    const saved = await resetButtonMappings();
    mappings.value = saved;
    savedSnapshot.value = JSON.parse(JSON.stringify(saved)) as ButtonMappings;
    statusMessage.value = "已恢复默认（全部按键保持原始行为）";
  } catch (error) {
    statusMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    busy.value = false;
  }
}

async function saveConfiguration(): Promise<void> {
  await persist("配置已保存并生效");
}

async function exportConfiguration(): Promise<void> {
  busy.value = true;
  statusMessage.value = null;
  try {
    const exported = await exportButtonMappingConfiguration();
    if (exported) statusMessage.value = "按键映射配置已导出";
  } catch (error) {
    statusMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    busy.value = false;
  }
}

async function importConfiguration(): Promise<void> {
  busy.value = true;
  statusMessage.value = null;
  try {
    const imported = await importButtonMappingConfiguration();
    if (!imported) return;
    mappings.value = imported;
    savedSnapshot.value = JSON.parse(JSON.stringify(imported)) as ButtonMappings;
    editingTarget.value = null;
    mappingSnapshot.value = await getButtonMappingSnapshot();
    statusMessage.value = "按键映射配置已导入并生效";
  } catch (error) {
    statusMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    busy.value = false;
  }
}

/** KeyboardEvent.code → KeyCode（serde snake_case）。 */
function codeToKeyCode(code: string): KeyCode | null {
  const modifierMap: Record<string, KeyCode> = {
    ControlLeft: "left_control",
    ControlRight: "right_control",
    ShiftLeft: "left_shift",
    ShiftRight: "right_shift",
    AltLeft: "left_alt",
    AltRight: "right_alt",
    MetaLeft: "left_windows",
    MetaRight: "right_windows",
  };
  if (modifierMap[code]) return modifierMap[code];
  const named: Record<string, KeyCode> = {
    Enter: "enter",
    Space: "space",
    Tab: "tab",
    Backspace: "backspace",
    Escape: "escape",
    ArrowLeft: "left",
    ArrowUp: "up",
    ArrowRight: "right",
    ArrowDown: "down",
    Home: "home",
    End: "end",
    PageUp: "page_up",
    PageDown: "page_down",
    Insert: "insert",
    Delete: "delete",
    ContextMenu: "apps",
    VolumeMute: "volume_mute",
    VolumeUp: "volume_up",
    VolumeDown: "volume_down",
  };
  if (named[code]) return named[code];
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return letter[1].toLowerCase();
  const digit = /^Digit([0-9])$/.exec(code);
  if (digit) return `digit${digit[1]}`;
  const functionKey = /^F([1-9]|1[0-2])$/.exec(code);
  if (functionKey) return `f${functionKey[1]}`;
  return null;
}

const selectedCaptureModifiers = reactive(new Set<KeyCode>());
const pressedCaptureModifiers = new Set<KeyCode>();
const MODIFIER_KEYS = new Set<KeyCode>([
  "left_control",
  "right_control",
  "left_shift",
  "right_shift",
  "left_alt",
  "right_alt",
  "left_windows",
  "right_windows",
]);
const CAPTURE_MODIFIER_OPTIONS: Array<{ key: KeyCode; label: string }> = [
  { key: "left_control", label: "左 Ctrl" },
  { key: "left_shift", label: "左 Shift" },
  { key: "left_alt", label: "左 Alt" },
  { key: "left_windows", label: "左 Win" },
  { key: "right_control", label: "右 Ctrl" },
  { key: "right_shift", label: "右 Shift" },
  { key: "right_alt", label: "右 Alt" },
  { key: "right_windows", label: "右 Win" },
];

function toggleCaptureModifier(key: KeyCode): void {
  if (!capturingShortcut.value || capturedChord) return;
  if (selectedCaptureModifiers.has(key)) selectedCaptureModifiers.delete(key);
  else selectedCaptureModifiers.add(key);
  captureDisplay.value = [...selectedCaptureModifiers];
}

async function beginShortcutCapture(): Promise<void> {
  if (capturingShortcut.value || captureStarting.value) return;
  const requestId = ++captureRequestId;
  captureStarting.value = true;
  statusMessage.value = null;
  try {
    await startShortcutCapture();
    if (unmounted || requestId !== captureRequestId) {
      await stopShortcutCapture().catch(() => undefined);
      return;
    }
    capturePressedKeys.clear();
    capturedChord = null;
    selectedCaptureModifiers.clear();
    pressedCaptureModifiers.clear();
    captureDisplay.value = [];
    capturingShortcut.value = true;
    if (captureTimeout !== null) window.clearTimeout(captureTimeout);
    captureTimeout = window.setTimeout(() => {
      void finishShortcutCapture("录入已超时，请重新录入");
    }, 15_000);
  } catch (error) {
    statusMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    if (requestId === captureRequestId) captureStarting.value = false;
  }
}

async function finishShortcutCapture(message?: string): Promise<void> {
  captureRequestId += 1;
  captureStarting.value = false;
  capturingShortcut.value = false;
  if (captureTimeout !== null) window.clearTimeout(captureTimeout);
  captureTimeout = null;
  await stopShortcutCapture().catch(() => undefined);
  capturePressedKeys.clear();
  capturedChord = null;
  pressedCaptureModifiers.clear();
  if (message) statusMessage.value = message;
}

function handleCaptureBlur(): void {
  if (capturingShortcut.value || captureStarting.value) {
    void finishShortcutCapture("窗口失去焦点，已取消录入");
  }
}

function acceptCapturedKey(code: KeyCode, isPressed: boolean, repeat = false): void {
  if (!capturingShortcut.value) return;
  if (!isPressed) {
    capturePressedKeys.delete(code);
    if (MODIFIER_KEYS.has(code)) pressedCaptureModifiers.delete(code);
    if (capturedChord) {
      captureDisplay.value = capturedChord;
      if (capturePressedKeys.size === 0) {
        const label = chordLabel({ keys: capturedChord });
        void finishShortcutCapture(`快捷键已录入：${label}`);
      }
    } else {
      captureDisplay.value = safeCaptureMode.value
        ? [...selectedCaptureModifiers]
        : [...pressedCaptureModifiers];
    }
    return;
  }
  if (!repeat) capturePressedKeys.add(code);
  // 已经拿到终止键后继续保持原生拦截，直到本次组合的所有 DOWN 都收到配对 UP。
  // 这避免 Win+L 在录入完成但物理键尚未松开时被 Windows 补执行。
  if (capturedChord) return;
  if (MODIFIER_KEYS.has(code)) {
    if (!repeat) pressedCaptureModifiers.add(code);
    if (safeCaptureMode.value) {
      statusMessage.value = "安全录入中：请松开键盘修饰键，并在界面中点击选择";
    } else {
      captureDisplay.value = [...pressedCaptureModifiers];
    }
    return;
  }
  if (safeCaptureMode.value && pressedCaptureModifiers.size > 0) {
    statusMessage.value = "未录入：请不要按住键盘修饰键；先在界面选择修饰键，再单独按主键";
    return;
  }
  const modifiers = safeCaptureMode.value
    ? [...selectedCaptureModifiers]
    : [...pressedCaptureModifiers];
  if (code === "escape" && modifiers.length === 0) {
    void finishShortcutCapture("已取消录入");
    return;
  }
  const keys = [...modifiers, code];
  capturedChord = keys;
  captureDisplay.value = keys;
  if (capturePurpose.value === "focus_shortcut") {
    applyFocusShortcut(keys);
  } else {
    applyAction({ type: "shortcut", chord: { keys } });
  }
  statusMessage.value = `已录入 ${chordLabel({ keys })}，松开全部按键后完成`;
}

function handleCaptureKeydown(event: KeyboardEvent): void {
  if (!capturingShortcut.value) return;
  event.preventDefault();
  event.stopPropagation();
  const code = codeToKeyCode(event.code);
  if (code === null) return;
  acceptCapturedKey(code, true, event.repeat);
}

function handleCaptureKeyup(event: KeyboardEvent): void {
  if (!capturingShortcut.value) return;
  const code = codeToKeyCode(event.code);
  if (code) acceptCapturedKey(code, false);
}

watch(capturingShortcut, (active) => {
  if (!active) {
    selectedCaptureModifiers.clear();
    pressedCaptureModifiers.clear();
    captureDisplay.value = [];
  }
});

// 打开编辑面板后滚动到可见位置（Mac ScrollViewReader 同款行为）。
watch(editingTarget, async (target) => {
  if (!target) return;
  await nextTick();
  editorPanel.value?.scrollIntoView?.({ behavior: "smooth", block: "nearest" });
});

function phaseLabel(phase: RawInputPhase | undefined): string {
  switch (phase) {
    case "ready":
      return "按键监听已就绪";
    case "starting":
      return "正在启动监听";
    case "failed":
      return "监听启动失败（自动重试中）";
    case "stopped":
      return "监听已停止";
    case "awaiting":
      return "等待遥控器连接";
    case "unsupported":
      return "当前环境暂不支持";
    default:
      return "正在读取状态";
  }
}

/** 手动启停监听：自动启动之外保留显式控制（停止后自动重试不生效）。 */
async function toggleListener(): Promise<void> {
  busy.value = true;
  statusMessage.value = null;
  try {
    if (rawInput.value?.phase === "ready") {
      await stopRawInput();
      statusMessage.value = "监听已停止；映射与高亮暂停（按住说话不受影响）";
    } else {
      await startRawInput();
      statusMessage.value = "监听已启动";
    }
  } catch (error) {
    statusMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    busy.value = false;
  }
}

const rawInput = computed(() => props.runtime?.platform.rawInput);
const connectionInfo = computed(() => props.runtime?.platform.connection);

/** 头部遥控器信息卡片（2026-10-04）：连接态、电量与「重新连接」按钮。 */
const connectionReady = computed(() => {
  const phase = connectionInfo.value?.phase;
  return phase === "ready" || phase === "streaming";
});
/** 圆点配色沿用改造前设备胶囊里的同一套映射（streaming=active / ready=success / 其余 pending）。 */
const connectionTone = computed(() =>
  connectionInfo.value?.phase === "streaming"
    ? "active"
    : connectionInfo.value?.phase === "ready"
      ? "success"
      : "pending",
);
const reconnectBusy = ref(false);
const reconnectLabel = computed(() =>
  reconnectBusy.value ? "连接中…" : connectionReady.value ? "重新连接" : "立即连接",
);
const reconnectDisabled = computed(
  () => reconnectBusy.value || props.runtime?.platform.bleScanAvailable === false,
);
const reconnectTitle = computed(() => {
  if (reconnectBusy.value) return "正在连接遥控器…";
  if (props.runtime?.platform.bleScanAvailable === false) {
    return "蓝牙不可用：先在连接页确认蓝牙已打开，再回来重试";
  }
  return connectionReady.value ? "断开后连回当前这支遥控器" : "连接已配对的遥控器";
});

/**
 * 选哪支遥控器来重连：优先型号一致，其次蓝牙广播名一致，最后退回第一个候选。
 * 头部拿不到设备 id（ConnectionSnapshot 里没有），只能按型号/名字匹配。
 */
function pickReconnectTarget(
  remotes: PairedRemote[],
  current: { model: RemoteModel; name: string | null },
): PairedRemote | null {
  const supported = remotes.filter((remote) => remote.isSupportedCandidate);
  const pool = supported.length > 0 ? supported : remotes;
  if (pool.length === 0) return null;
  if (current.model !== "unknown") {
    const byModel = pool.find((remote) => remote.model === current.model);
    if (byModel) return byModel;
  }
  if (current.name) {
    const byName = pool.find((remote) => remote.name === current.name);
    if (byName) return byName;
  }
  return pool[0]!;
}

async function reconnectRemote(): Promise<void> {
  if (reconnectBusy.value) return;
  reconnectBusy.value = true;
  statusMessage.value = null;
  try {
    const remotes = await scanPairedRemotes();
    const target = pickReconnectTarget(remotes, {
      model: remoteModel.value,
      name: connectionInfo.value?.remoteName ?? null,
    });
    if (!target) {
      statusMessage.value = "没有找到已配对的遥控器；到连接页扫描后再试。";
      return;
    }
    await connectRemote(target.id);
  } catch (error) {
    statusMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    reconnectBusy.value = false;
  }
}

/**
 * RC003 三键的传输桥接状态（捕获链第 ② 段）。
 *
 * 三个键按不动时，这一行直接说出卡在哪一段：主程序已就绪 = 在等提权助手；
 * 助手已连接 = 传输段通了（再没反应就该查第 ① 段的捕获）。
 *
 * 两个型号统一展示（产品决策 2026-09-24：三键映射逻辑不分型号，
 * 界面不做区分；RC001 开着增强捕获只是多一个不参与其按键路径的助手）。
 */
/** 三键捕获的授权状态（= 系统里的计划任务）。 */
/**
 * 开关与圆点的「上次已知状态」缓存。只服务**首帧**（2026-09-28 Andy 要求：
 * 进按键页不要「开关延迟出现 + 圆点灰→绿」）：setup 阶段同步读出终值渲染，
 * 挂载后的对账以权威状态为准，不一致时采信对账并回写（卸载后重装回落关闭
 * 会在约一次 IPC 内纠正首帧，属可接受的最终一致）。
 */
const RC003_UI_CACHE_KEY = "sayall.rc003Capture.uiCache";
const RC003_BRIDGE_PHASES: readonly Rc003BridgePhase[] = [
  "stopped",
  "listening",
  "connected",
  "failed",
];

function readRc003UiCache(): { enabled: boolean | null; phase: Rc003BridgePhase | null } {
  try {
    const raw = localStorage.getItem(RC003_UI_CACHE_KEY);
    if (!raw) return { enabled: null, phase: null };
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return { enabled: null, phase: null };
    const record = parsed as Record<string, unknown>;
    return {
      enabled: typeof record.enabled === "boolean" ? record.enabled : null,
      phase:
        typeof record.phase === "string" &&
        RC003_BRIDGE_PHASES.includes(record.phase as Rc003BridgePhase)
          ? (record.phase as Rc003BridgePhase)
          : null,
    };
  } catch {
    // 解析失败（版本垃圾/手工改动）：忽略缓存，回落旧的对账行为。
    return { enabled: null, phase: null };
  }
}

function writeRc003UiCache(enabled: boolean | null, phase: Rc003BridgePhase | null): void {
  try {
    localStorage.setItem(RC003_UI_CACHE_KEY, JSON.stringify({ enabled, phase }));
  } catch {
    // localStorage 满/被禁：缓存只是首帧优化，静默降级。
  }
}

const rc003UiCache = readRc003UiCache();
const rc003Task = ref<Rc003TaskStatus | null>(null);
const rc003CaptureBusy = ref(false);
/**
 * 开关的显示状态：**事件驱动**，`null` = 尚未初始化。
 *
 * 为什么不用「任务是否存在」当真相源：关闭开关只结束助手、**任务保留**
 * （提权任务普通权限删不掉；授权视为作废，下次开启必弹 UAC）——任务还在，
 * 若读任务，开关会立刻弹回开启，「已停用」的提示与三键恢复原生行为全都
 * 对不上（2026-09-23 首次 UI 验收正是这个形状）。因此轮询只在首次对账一次，
 * 之后以用户的开关操作为准。
 */
const rc003CaptureEnabled = ref<boolean | null>(rc003UiCache.enabled);
/** 桥接相位（从快照中提取）：首帧可由缓存给出终值，后续随对账刷新。 */
const rc003BridgePhase = ref<Rc003BridgePhase | null>(rc003UiCache.phase);
const rc003BridgeText = computed(() => {
  const phase = rc003BridgePhase.value;
  if (!phase || phase === "stopped") return null;
  switch (phase) {
    case "listening":
      return "全按键支持已开启，正在启动";
    case "connected":
      return "全按键支持已开启";
    case "failed":
      return "全按键支持开启失败";
    default:
      return null;
  }
});
const rc003BridgeTone = computed(() => {
  switch (rc003BridgePhase.value) {
    case "connected":
      return "success";
    case "failed":
      return "error";
    default:
      // listening 及未知相位：等待中（黄点）。文案仍可从开关圆点的 title 读到。
      return "pending";
  }
});

/**
 * 返回/音量± 在「全按键支持」关闭时不可用：卡片置灰禁用并给悬停提示
 * （2026-10-03 Andy 定稿：置灰禁用 +「…需要开启全按键支持才能使用，开启后恢复正常」）。
 * 状态未就绪（null）按关闭态处理——与开关悬停提示、能力说明同一口径；开启后
 * 卡片与命中路径立即恢复（响应式，无需重进页面）。
 * 注意：不能只靠模板的 disabled 属性——合成事件会绕过原生 disabled，动作函数
 * （selectButton/openEditor）必须同样自守。
 */
function captureGated(button: RemoteButton): boolean {
  return (
    rc003CaptureEnabled.value !== true &&
    (button === "back" || button === "volume_up" || button === "volume_down")
  );
}

// 开启成功但桥接段异步失败（助手起不来等）：开关已是开启态、不会再走
// applyCaptureToggle 的失败分支，必须在这里把失败送进底部提示条，
// 否则用户只看到一个黄点永远不变绿（2026-09-28 状态行移除后的唯一显性告警）。
watch(rc003BridgePhase, (phase, previous) => {
  if (phase === "failed" && previous !== "failed") {
    statusMessage.value = "全按键支持开启失败：请关闭全按键支持后重新开启；若反复失败请联系开发者。";
    reportFrontendEvent({
      event: "rc003_capture_bridge_failed",
      phase: "completed",
      result: "failed",
      reason: `bridge_phase=${String(phase)}`,
    });
  }
});

// 缓存回写：状态或圆点相位每次落定都记录，供下次进页首帧直接渲染终值。
watch([rc003CaptureEnabled, rc003BridgePhase], ([enabled, phase]) => {
  writeRc003UiCache(enabled, phase);
});

/**
 * 授权与桥接状态对账（挂载开头启动一次 + 每秒一次）。不能只靠 interval：
 * 首跳在挂载 1 秒后才跑，期间开关一直 disabled——真机上用户进页面头 1 秒
 * 点不动、无解释；CI 慢机上 waitFor 默认 1000ms 被压线超时（PR #132 实测）。
 *
 * 首次对账以权威状态为准：缓存只是首帧优化（见 rc003UiCache），授权变化
 * （卸载后重装回落关闭、启动对账回落）必须采信；之后以用户的开关操作为准
 * （理由见 rc003CaptureEnabled 注释）。桥接快照在此一并拉取（内存读，开销
 * 可忽略），圆点相位因此不再依赖 1 秒轮询首跳——消除「灰点→绿点」跳变。
 */
let rc003FirstReconcile = true;
const reconcileRc003Task = async (): Promise<void> => {
  try {
    const [task, bridge] = await Promise.all([
      getRc003TaskStatus(),
      getRc003BridgeSnapshot().catch(() => null),
    ]);
    rc003Task.value = task;
    if (rc003FirstReconcile) {
      rc003FirstReconcile = false;
      if (!rc003CaptureBusy.value) {
        rc003CaptureEnabled.value = task.enabled;
      }
    }
    if (bridge) {
      rc003BridgePhase.value = bridge.phase;
    }
  } catch {
    // 对账失败：状态保持当前值（缓存或 null），等下一秒轮询重试
    //（与既有 interval 行为一致）。
  }
};

/**
 * 切换三键捕获。打开**每次都会弹一次 UAC**（2026-10-03 起每次开启都重新授权，
 * IPC 会等授权流程结束）；关闭只结束助手、任务留在系统里但授权作废。
 * 完成后用返回的状态刷新，而不是假设成功；lastError 走页面既有的提示条。
 */
const captureSwitchEl = ref<HTMLInputElement | null>(null);
/**
 * 把开关的**DOM 状态**写回成绑定值。
 *
 * 为什么必须手动写：原生复选框在被点击的瞬间由浏览器先翻了 `checked`，
 * 而 `:checked` 的绑定值若前后都是 false（操作失败 = 状态没变），Vue 判定
 * props 无变化、**不会**生成 DOM 补丁——于是出现「状态是关、界面是开」
 * （2026-09-25 真机：后端日志判 failed，开关却仍显示勾选）。
 */
function syncCaptureSwitchDom(): void {
  const el = captureSwitchEl.value;
  if (el) {
    el.checked = rc003CaptureEnabled.value === true;
  }
}

async function toggleRc003Capture() {
  if (rc003CaptureBusy.value) return;
  // 开启方向：**每次都先弹确认**（2026-10-03 Andy 定稿）——每次开启都要重新
  // 授权：IPC 里的 enable 会重新注册任务并弹一次 Windows 授权窗口，弹窗文案
  // 与之逐句对齐（「每次开启都会弹出 Windows 授权窗口」）。
  // 也不再记「已读过」：一次性 localStorage 标记在重装/升级后仍然存活，
  // 正是 2026-09-27「重装后弹窗消失」的根因。
  // 关闭方向永远直接执行，不弹。
  if (rc003CaptureEnabled.value !== true) {
    showCaptureConfirm.value = true;
    // 开关 DOM 在点击瞬间已被浏览器翻转，先写回关闭，等确认后再真正执行。
    syncCaptureSwitchDom();
    return;
  }
  await applyCaptureToggle();
}

const showCaptureConfirm = ref(false);

function confirmCaptureDialog(): void {
  showCaptureConfirm.value = false;
  void applyCaptureToggle();
}

function closeCaptureDialog(): void {
  showCaptureConfirm.value = false;
  // 不开启：开关 DOM 已被浏览器翻转，必须显式写回（见 syncCaptureSwitchDom）。
  syncCaptureSwitchDom();
}

async function applyCaptureToggle() {
  if (rc003CaptureBusy.value) return;
  rc003CaptureBusy.value = true;
  const wasEnabled = rc003CaptureEnabled.value;
  try {
    const next = wasEnabled ? await disableRc003Capture() : await enableRc003Capture();
    rc003Task.value = next;
    if (next.lastError) {
      // 操作失败（UAC 取消 / 停止被拒）：**保持开关原状态**。
      // 实际系统状态没变，开关却翻过去，就是又一次"证据说谎"。
      statusMessage.value = next.lastError;
      return;
    }
    // 成功后的反馈由面板里的常驻能力说明承担（随状态实时切换），
    // 提示条不再重复一条一次性的话；失败仍走提示条（见上）。
    rc003CaptureEnabled.value = next.enabled;
    reportFrontendEvent({
      event: "rc003_capture_toggle",
      phase: "completed",
      result: "passed",
      reason: `enabled=${String(next.enabled)}`,
    });
  } catch (error) {
    statusMessage.value = error instanceof Error ? error.message : String(error);
    // **同步回退到点击前的状态**——不依赖"我没动过它"，也不等下一次异步
    // 对账：原生复选框在 click 时 DOM 已经先翻了，只有显式写回才确定。
    rc003CaptureEnabled.value = wasEnabled === true;
    reportFrontendEvent({
      event: "rc003_capture_toggle",
      phase: "completed",
      result: "failed",
      reason: `was_enabled=${String(wasEnabled === true)} switch_reverted`,
    });
    // 再用权威状态（设置里的意图）校正一次，防止本地值与系统状态漂移。
    void getRc003TaskStatus()
      .then((status) => {
        rc003CaptureEnabled.value = status.enabled;
        reportFrontendEvent({
          event: "rc003_capture_toggle",
          phase: "reconciled",
          result: "unknown",
          reason: `authoritative_enabled=${String(status.enabled)}`,
        });
      })
      .catch(() => {});
  } finally {
    rc003CaptureBusy.value = false;
    // 无论成败都把 DOM 校正成绑定值：失败时尤其关键（见 syncCaptureSwitchDom）。
    syncCaptureSwitchDom();
  }
}

/** 页面统一展示全按键支持；开关关闭时，非三键仍沿用原有输入路径。 */
onMounted(async () => {
  const setupStarted = performance.now();
  // 画布宽度必须在首个 await 之前同步测量。canvasWidth 初值是
  // CANVAS_MIN_WIDTH(800)，而容器实际宽 926（1100 窗口）；若等挂载首批
  // IPC（下方 Promise.all）返回后再测，首帧会以 800 布局、随后跳到 926，
  // 画布内所有元素（遥控器图/左右卡片/连线箭头）整体水平重排——真机 IPC
  // 跨进程有延迟必然跨帧，用户看到"点按键页整页左右抖一下"（2026-09-28
  // 实测：注入 300ms 延迟后遥控器图 +63px、右列卡片 −30px、连线箭头
  // +96px；浏览器预览因 Promise 同帧 resolve 不复现）。同步测量后首帧
  // 即真实宽度，后续窗口变化仍由 ResizeObserver 接管。
  if (canvasEl.value) {
    canvasWidth.value = Math.max(CANVAS_MIN_WIDTH, canvasEl.value.clientWidth);
  }
  window.addEventListener("keydown", handleCaptureKeydown, true);
  window.addEventListener("keyup", handleCaptureKeyup, true);
  window.addEventListener("blur", handleCaptureBlur);
  // 授权/桥接对账**尽早启动**（与映射加载并行）：缓存只负责首帧渲染，
  // 权威纠正（卸载后重装回落、授权变化）越早落地越好。
  const rc003ReconcileStarted = reconcileRc003Task();
  const [loaded, snapshot, apps] = await Promise.all([
    getButtonMappings(),
    getButtonMappingSnapshot(),
    listPresetApps().catch(() => [] as PresetAppInfo[]),
  ]);
  if (unmounted) {
    return;
  }
  presetApps.value = apps.filter((app) => app.installed);
  registerPresetAppNames(presetApps.value);
  mappings.value = loaded;
  savedSnapshot.value = JSON.parse(JSON.stringify(loaded)) as ButtonMappings;
  mappingSnapshot.value = snapshot;
  if (rawInput.value?.activeButtons) {
    activeButtons.value = new Set(rawInput.value.activeButtons);
  }

  const stopEdges = await subscribeButtonEdges((edge: ButtonEdge) => {
    const next = new Set(activeButtons.value);
    if (edge.isPressed) {
      next.add(edge.button);
    } else {
      next.delete(edge.button);
    }
    activeButtons.value = next;
    if (!lockSelection.value && edge.isPressed) {
      selectedButton.value = edge.button;
    }
  });
  if (unmounted) {
    stopEdges();
    return;
  }
  unlistenEdges = stopEdges;

  const stopGestures = await subscribeButtonGestures((gesture: FiredGesture) => {
    lastFired.value = gesture;
    firedFlash.value = { button: gesture.button, trigger: gesture.trigger };
    if (flashTimer !== null) window.clearTimeout(flashTimer);
    flashTimer = window.setTimeout(() => {
      firedFlash.value = null;
    }, 600);
  });
  if (unmounted) {
    stopGestures();
    return;
  }
  unlistenGestures = stopGestures;

  const stopShortcutCaptureEvents = await subscribeShortcutCaptureEdges(
    (edge: ShortcutCaptureEdge) => acceptCapturedKey(edge.key, edge.isPressed),
  );
  if (unmounted) {
    stopShortcutCaptureEvents();
    return;
  }
  unlistenShortcutCapture = stopShortcutCaptureEvents;

  // 对账函数定义在 onMounted 之前（挂载开头就启动它，与映射加载并行）。
  await rc003ReconcileStarted;

  snapshotTimer = window.setInterval(async () => {
    mappingSnapshot.value = await getButtonMappingSnapshot();
    await reconcileRc003Task();
    // 按住集合对账：快照是并集真值（覆盖漏事件漂移）。
    if (rawInput.value?.activeButtons) {
      activeButtons.value = new Set(rawInput.value.activeButtons);
    }
  }, 1_000);

  // 流式画布：观测容器宽（不足最小画布 800px 时保持 800 由 CSS 缩放兜底）。
  // 首次宽度已在 onMounted 同步段测过（见函数开头），此处只订阅后续变化。
  // jsdom 测试环境无 ResizeObserver，跳过观测。
  if (canvasEl.value && typeof ResizeObserver !== "undefined") {
    resizeObserver = new ResizeObserver((entries) => {
      for (const entry of entries) {
        canvasWidth.value = Math.max(CANVAS_MIN_WIDTH, Math.round(entry.contentRect.width));
      }
    });
    resizeObserver.observe(canvasEl.value);
  }
  resourcesReady = true;
  reportFrontendEvent({
    event: "buttons_page_resource_setup",
    phase: "completed",
    result: "passed",
    reason: "listeners_and_polling_ready",
    elapsedMs: Math.max(0, Math.round(performance.now() - setupStarted)),
  });
});

onUnmounted(() => {
  unmounted = true;
  releasePageResources();
  reportFrontendEvent({
    event: "buttons_page_resource_cleanup",
    phase: "completed",
    result: "passed",
    reason: resourcesReady ? "unmounted_after_cleanup" : "unmounted_before_setup_completed",
  });
});
</script>

<template>
  <section class="buttons-page">
    <!-- 头部对齐 Mac mappingPage：标题 + 启用开关相邻居左，遥控器状态最右
         （保存按钮移入编辑面板，与"测试一次/关闭"同排）。 -->
    <header class="page-header mapping-header">
      <div>
        <div class="mapping-title-row">
          <h1>按键映射</h1>
          <label class="toggle-row" title="开启后，遥控器按键按本页配置执行动作；关闭时，遥控器保持原始按键行为。">
            <span>启用自定义按键功能</span>
            <input v-model="enabled" type="checkbox" class="toggle-input" :disabled="busy" />
          </label>
          <!-- 全按键支持开关常驻页面头部（2026-09-27 Andy 要求）：不依赖
               选中某个按键的编辑面板，任何时刻都能开启/关闭。
               样式与「启用自定义按键功能」一致（toggle-input Switch，2026-09-28）；
               桥接状态是开关右侧的行内圆点——不能再用独立状态行：v-if 插行会把
               下方画布整体顶下去（页面抖动），胶囊底色+状态光晕也把标题区染了色
               （2026-09-28 Andy 报告）。 -->
          <!-- 悬停提示随开关状态切换（2026-09-28 Andy 定稿）：关闭态指向
               「开启后支持使用…」，开启态指向「关闭后…将不可映射」。 -->
          <label
            class="toggle-row"
            :title="captureSwitchTitle"
          >
            <span>全按键支持</span>
            <!-- 终值就绪前用同尺寸占位符顶位、就绪后才创建开关本体——与
                 「启动行为」页登录自启动开关同法（2026-09-28 Andy 要求）：
                 开关创建即带正确 checked，不产生"状态回来后关→开"的滑动动画。 -->
            <span
              v-if="rc003CaptureEnabled === null"
              class="toggle-placeholder"
              aria-hidden="true"
            ></span>
            <input
              v-else
              ref="captureSwitchEl"
              type="checkbox"
              class="toggle-input"
              :checked="rc003CaptureEnabled === true"
              :disabled="rc003CaptureBusy"
              @change="toggleRc003Capture"
            />
            <!-- 状态圆点只在开关**打开**时出现（2026-09-28 Andy 要求），颜色随
                 桥接相位变化：绿=助手已连接、黄=启动中/未知、红=开启失败；
                 关闭时不占位，避免"关着还亮个点"读成已启用。 -->
            <span
              v-if="rc003CaptureEnabled === true"
              class="status-dot"
              :class="rc003BridgeTone"
              :title="rc003BridgeText ?? '全按键支持已开启'"
            ></span>
          </label>
        </div>
        <p class="page-subtitle">点击按键进行自定义配置</p>
      </div>
      <div class="mapping-header-controls">
        <!-- 遥控器信息卡片（2026-10-04，对标 Mac mappingPage 右上卡片）：图标 + 型号 +
             状态/电量 + 重新连接。状态行与按钮文案都是常驻的（不靠 v-if 插行），
             避免头部高度变化把画布顶下去（2026-09-28 抖动教训）。 -->
        <div class="remote-info-card">
          <div class="remote-info-main">
            <strong>{{ deviceLabel }}</strong>
            <span class="remote-info-status">
              <span class="status-dot" :class="connectionTone"></span>{{ connectionPhaseLabel(connectionInfo?.phase ?? "idle") }}
              <BatteryIndicator v-if="connectionReady" :connection="connectionInfo" />
            </span>
          </div>
          <button
            class="secondary-button"
            type="button"
            :disabled="reconnectDisabled"
            :title="reconnectTitle"
            @click="reconnectRemote"
          >
            {{ reconnectLabel }}
          </button>
        </div>
      </div>
    </header>

    <!-- 桥接状态不再有独立行（2026-09-28）：状态收进头部开关右侧的行内圆点
         （title 悬停看文案），开启失败走底部提示条——独立行的 v-if 插入 /
         移除会引起整页回流（抖动），胶囊底色与状态光晕还会染到标题区。 -->

    <div ref="canvasEl" class="mapping-canvas" :style="{ height: `${CANVAS_HEIGHT}px` }">
      <svg
        class="mapping-connections"
        :width="canvasWidth"
        :height="CANVAS_HEIGHT"
        aria-hidden="true"
      >
        <template v-for="placement in PLACEMENTS" :key="placement.button">
          <path
            :d="connectionPath(placement)"
            :class="{ selected: selectedButton === placement.button, active: activeButtons.has(placement.button) }"
            fill="none"
          />
          <polygon
            :points="arrowPolygon(placement)"
            :class="{ selected: selectedButton === placement.button, active: activeButtons.has(placement.button) }"
          />
        </template>
        <path
          :d="connectionPath(VOICE_PLACEMENT)"
          :class="{ active: voiceActive }"
          fill="none"
        />
        <polygon :points="arrowPolygon(VOICE_PLACEMENT)" :class="{ active: voiceActive }" />
      </svg>

      <figure class="remote-photo" :style="{ left: `${remoteLeft}px` }">
        <img src="/RC003-remote-photo@2x.png" alt="小米蓝牙语音遥控器 2 Pro 示意图" draggable="false" />
        <span
          v-for="placement in PLACEMENTS"
          :key="placement.button"
          class="anchor-dot"
          :class="{ visible: activeButtons.has(placement.button) }"
          :style="{
            left: `${photoAnchorPoint(placement).x - 4}px`,
            top: `${photoAnchorPoint(placement).y - 4}px`,
          }"
        ></span>
        <span
          class="anchor-dot voice"
          :class="{ visible: voiceActive }"
          :style="{
            left: `${photoAnchorPoint(VOICE_PLACEMENT).x - 4}px`,
            top: `${photoAnchorPoint(VOICE_PLACEMENT).y - 4}px`,
          }"
        ></span>
      </figure>

      <article
        v-for="placement in PLACEMENTS"
        :key="placement.button"
        class="mapping-card"
        :class="{
          left: placement.side === 'left',
          right: placement.side === 'right',
          selected: selectedButton === placement.button,
          active: activeButtons.has(placement.button),
          flashed: firedFlash?.button === placement.button,
          'is-locked': captureGated(placement.button),
        }"
        :style="{ top: `${cardTop(placement)}px`, width: `${cardWidth}px` }"
        :title="captureGated(placement.button) ? TRI_KEY_GATED_TITLE : undefined"
        :aria-disabled="captureGated(placement.button) || undefined"
        @click="selectButton(placement.button)"
      >
        <div class="mapping-card-title">
          <svg class="mapping-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path
              v-for="(path, index) in buttonIcons[placement.button]"
              :key="index"
              :d="path"
              stroke="currentColor"
              stroke-width="1.9"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          </svg>
          <strong>{{ buttonLabels[placement.button] }}</strong>
        </div>
        <div class="mapping-cells">
          <button
            v-for="trigger in TRIGGERS"
            :key="trigger"
            type="button"
            class="mapping-cell"
            :class="{
              set: actionOf(placement.button, trigger).type !== 'disabled',
              editing:
                editingTarget?.button === placement.button && editingTarget?.trigger === trigger,
              flashed: firedFlash?.button === placement.button && firedFlash?.trigger === trigger,
            }"
            :disabled="captureGated(placement.button)"
            :title="
              captureGated(placement.button)
                ? TRI_KEY_GATED_TITLE
                : `${buttonLabels[placement.button]} · ${buttonTriggerLabel(trigger)}：${actionSummary(actionOf(placement.button, trigger))}`
            "
            @click.stop="openEditor(placement.button, trigger)"
          >
            <small>{{ buttonTriggerLabel(trigger) }}</small>
            <span>{{ actionSummary(actionOf(placement.button, trigger)) }}</span>
          </button>
        </div>
      </article>

      <article
        class="mapping-card voice-card right"
        :class="{ active: voiceActive }"
        :style="{ top: `${cardTop(VOICE_PLACEMENT)}px`, width: `${cardWidth}px` }"
      >
        <div class="mapping-card-title">
          <svg class="mapping-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path :d="VOICE_ICON_FILLED" fill="currentColor" />
            <path
              v-for="(path, index) in VOICE_ICON_STROKES"
              :key="index"
              :d="path"
              stroke="currentColor"
              stroke-width="1.9"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          </svg>
          <strong>语音键</strong>
          <span class="badge pending voice-badge" :class="{ active: voiceActive }">按住说话</span>
        </div>
        <p class="voice-note">按下开始、松开结束；不参与自定义映射，不加双击/长按延迟。</p>
      </article>
    </div>

    <article v-if="editingTarget" ref="editorPanel" class="card mapping-editor">
      <div class="card-title-row">
        <div>
          <h2>{{ buttonLabel(editingTarget.button) }} · {{ buttonTriggerLabel(editingTarget.trigger) }}</h2>
          <p class="muted">当前：{{ actionSummary(actionOf(editingTarget.button, editingTarget.trigger)) }}</p>
        </div>
        <div class="button-row">
          <!-- 全按键支持开关已移至页面头部（与「启用自定义按键功能」并排，
               2026-09-27），编辑面板只保留按键级操作。 -->
          <button
            class="secondary-button editor-disable-btn"
            :class="{ 'is-active': actionOf(editingTarget.button, editingTarget.trigger).type === 'disabled' }"
            type="button"
            :disabled="busy"
            title="只禁用当前格子的映射，此按键恢复原始行为"
            @click="applyAction({ type: 'disabled' })"
          >
            禁用按键
          </button>
          <button class="secondary-button" type="button" @click="editingTarget = null">关闭</button>
        </div>
      </div>
      <div class="action-sections">
        <p v-if="capabilityNote" class="muted editor-note capability-note">{{ capabilityNote }}</p>
        <section v-for="group in PRESET_GROUPS" :key="group.label" class="action-section">
          <h4 class="action-section-title">{{ group.label }}</h4>
          <div class="preset-grid">
            <!-- 芯片显示实际按键组合（组合在不同 App 里语义不同，功能描述
                 只作悬停提示，避免把 Ctrl+C 一类写成"复制"造成误判）。 -->
            <button
              v-for="preset in group.items"
              :key="preset.label"
              class="chip"
              :class="{ selected: isActivePreset(preset.keys) }"
              type="button"
              :title="preset.label"
              @click="applyAction({ type: 'shortcut', chord: { keys: [...preset.keys] } })"
            >
              {{ chordLabel({ keys: preset.keys }) }}
            </button>
          </div>
        </section>

        <section class="action-section">
          <h4 class="action-section-title">鼠标滚轮</h4>
          <div class="preset-grid">
            <button v-for="direction in (['up', 'down'] as const)" :key="direction" class="chip"
              :class="{ selected: isActiveScroll(direction) }" type="button" title="在鼠标当前位置滚动"
              @click="applyAction({ type: 'scroll', direction, steps: scrollSteps })">{{ direction === "up" ? "滚轮向上" : "滚轮向下" }}</button>
          </div>
          <label v-if="selectedAction?.type === 'scroll'" class="mouse-amount">
            <span>每次滚动</span>
            <input aria-label="每次滚动格数" type="number" min="1" max="100" step="1" :value="scrollSteps" @change="updateMouseAmount($event, 'scroll')" />
            <span>格</span>
          </label>
        </section>

        <section class="action-section">
          <h4 class="action-section-title">鼠标点击</h4>
          <div class="preset-grid">
            <button v-for="(label, kind) in mouseClickLabels" :key="kind" class="chip" type="button"
              :class="{ selected: selectedAction?.type === 'mouse_click' && selectedAction.kind === kind }"
              title="点击鼠标当前位置" @click="applyAction({ type: 'mouse_click', kind })">{{ label }}</button>
          </div>
        </section>

        <section class="action-section">
          <h4 class="action-section-title">鼠标移动</h4>
          <div class="preset-grid">
            <button v-for="(label, direction) in mouseMoveLabels" :key="direction" class="chip mouse-direction" type="button"
              :aria-label="label" :title="label" :class="{ selected: selectedAction?.type === 'mouse_move' && selectedAction.direction === direction }"
              @click="applyAction({ type: 'mouse_move', direction, distance: moveDistance })">{{ moveSymbols[direction] }}</button>
          </div>
          <label v-if="selectedAction?.type === 'mouse_move'" class="mouse-amount">
            <span>每次移动</span>
            <input aria-label="每次移动像素" type="number" min="1" max="2000" step="1" :value="moveDistance" @change="updateMouseAmount($event, 'mouse_move')" />
            <span>像素</span>
          </label>
        </section>

        <!-- 「设备操作 / 聚焦输入框」入口先隐藏（2026-10-03 用户要求）：动作类型与平台
             受理路径保留（已有映射照常生效），只是 UI 暂不暴露入口。 -->
        <section v-if="DEVICE_ACTION_SECTION_ENABLED" class="action-section">
          <h4 class="action-section-title">设备操作</h4>
          <div class="preset-grid">
            <button
              class="chip"
              :class="{ selected: selectedAction?.type === 'focus_input' }"
              type="button"
              title="把键盘焦点放到当前前台应用的输入框（不切换应用、不输入文字）"
              @click="applyAction({ type: 'focus_input' })"
            >
              聚焦输入框
            </button>
          </div>
        </section>

        <section class="action-section">
          <h4 class="action-section-title">打开应用</h4>
          <input v-if="customApps.length > 12" v-model="appFilter" class="app-library-search" type="search" aria-label="筛选已添加应用" placeholder="筛选已添加应用" />
          <!-- 预设应用与已添加应用共用一个换行网格（2026-10-03 用户要求）：
               两组名字连续排布，不再在中间强制换行；长名字的换行规则见 .chip.custom-app。 -->
          <div class="preset-grid app-library">
            <button
              v-for="app in presetApps"
              :key="app.id"
              class="chip"
              :class="{ selected: openAppTargetOf(editingTarget.button, editingTarget.trigger) === app.id }"
              type="button"
              title="已运行则切到该应用窗口，未运行则启动"
              @click="applyAction({ type: 'open_app', target: app.id })"
            >
              {{ app.name }}
            </button>
            <button v-for="app in filteredCustomApps" :key="app.path" class="chip custom-app" type="button"
              :class="{ selected: openAppTargetOf(editingTarget.button, editingTarget.trigger) === app.path }"
              :title="app.name" @click="applyAction({ type: 'open_app', target: app.path })">{{ app.name }}</button>
          </div>
          <!-- 两个动作入口排在**所有**应用名称（预设 + 自定义）之后（2026-10-03 用户要求）：
               用户挑应用时先扫完名字，再看到「扫描 / 添加」——不再插在名字中间。 -->
          <div class="preset-grid app-library-actions">
            <button class="chip" type="button" @click="appPickerError = null; appPickerOpen = true">扫描本机应用</button>
            <button
              class="chip add-app"
              type="button"
              title="从本机选择任意程序或快捷方式"
              @click="addCustomApp"
            >
              ＋ 添加应用
            </button>
          </div>
          <div v-if="showAppFocusStrategy && focusTarget" class="focus-profile">
            <p class="muted focus-profile-title">打开后聚焦方式</p>
            <div class="preset-grid">
              <button
                v-for="option in focusStrategyOptions"
                :key="option.value"
                class="chip"
                :class="{ selected: (focusProfile?.strategy ?? 'open_only') === option.value }"
                type="button"
                @click="applyFocusStrategy(option.value)"
              >
                {{ option.label }}
              </button>
            </div>
            <div v-if="focusProfile?.strategy === 'app_shortcut'" class="focus-shortcut-row">
              <button
                class="chip"
                :class="{ selected: capturingShortcut && capturePurpose === 'focus_shortcut' }"
                type="button"
                :disabled="captureStarting"
                @click="
                  capturingShortcut && capturePurpose === 'focus_shortcut'
                    ? finishShortcutCapture('已取消录入')
                    : beginFocusShortcutCapture()
                "
              >
                {{ capturingShortcut && capturePurpose === "focus_shortcut" ? "录入中…（按 Esc 取消）" : "录入聚焦快捷键" }}
              </button>
              <span class="muted">
                {{
                  focusProfile?.shortcut?.keys?.length
                    ? `当前：${chordLabel(focusProfile.shortcut)}`
                    : "尚未录入"
                }}
              </span>
            </div>
            <div v-if="focusProfile?.strategy === 'recorded_element'" class="focus-learn-row">
              <button
                class="chip"
                type="button"
                :disabled="focusLearnState === 'learning'"
                @click="startFocusLearning"
              >
                {{ focusLearnState === "learning" ? "学习中…" : "开始学习输入框" }}
              </button>
              <button
                v-if="focusProfile?.recorded"
                class="chip"
                type="button"
                @click="clearRecordedFocus"
              >
                清除记录
              </button>
              <span class="muted">
                {{ focusProfile?.recorded ? "已记录输入框" : "尚未记录" }}
              </span>
            </div>
            <div class="focus-profile-actions">
              <button class="chip" type="button" :disabled="!focusProfile?.strategy || focusProfile.strategy === 'open_only'" @click="testSelectedAppFocus">
                测试打开与聚焦
              </button>
            </div>
            <p v-if="focusLearnMessage" class="muted focus-learn-message">{{ focusLearnMessage }}</p>
            <p v-if="focusTestMessage" class="muted focus-test-message">{{ focusTestMessage }}</p>
            <p v-if="lastFocusNotice" class="muted focus-result-message">{{ lastFocusNotice }}</p>
          </div>
        </section>

        <section class="action-section">
          <h4 class="action-section-title">自定义</h4>
          <div class="custom-shortcut-row">
            <button
              class="chip"
              :class="{ selected: capturingShortcut }"
              type="button"
              :disabled="captureStarting"
              @click="capturingShortcut ? finishShortcutCapture('已取消录入') : beginShortcutCapture()"
            >
              {{ capturingShortcut ? "录入中…（按 Esc 取消）" : "录入自定义快捷键" }}
            </button>
            <span v-if="capturingShortcut" class="capture-display">
              {{ captureDisplay.length ? chordLabel({ keys: captureDisplay }) : (safeCaptureMode ? "先选择修饰键" : "请按下快捷键组合") }}
            </span>
          </div>
          <label
            class="toggle-row safe-capture-toggle"
            title="开启后，通过界面选择修饰键，键盘只需按主键。"
          >
            <span>安全录入模式</span>
            <input
              v-model="safeCaptureMode"
              type="checkbox"
              class="toggle-input"
              :disabled="capturingShortcut || captureStarting"
            />
            <small class="muted safe-capture-hint">
              直接录入无法完成或会触发系统动作时再开启。
            </small>
          </label>
          <template v-if="capturingShortcut && safeCaptureMode">
            <p class="muted editor-note capture-guide">
              请用鼠标选择修饰键，再只按一次主键。不要在键盘上按完整组合，系统快捷键不会被执行。
            </p>
            <div class="preset-grid capture-modifiers">
              <button
                v-for="modifier in CAPTURE_MODIFIER_OPTIONS"
                :key="modifier.key"
                class="chip"
                :class="{ selected: selectedCaptureModifiers.has(modifier.key) }"
                type="button"
                @click="toggleCaptureModifier(modifier.key)"
              >
                {{ modifier.label }}
              </button>
            </div>
            <p class="capture-display">然后单独按主键（例如选择“左 Win”后，只按 L）</p>
          </template>
        </section>
      </div>
      <label v-if="repeatSwitchVisible" class="toggle-row repeat-toggle" :title="repeatSwitchTitle">
        <span>按住连续触发</span>
        <input
          class="toggle-input"
          type="checkbox"
          :checked="repeatChecked"
          :disabled="busy || (!!repeatBlockedReason && !repeatChecked)"
          @change="toggleHoldRepeat"
        />
      </label>
      <p v-if="repeatSwitchVisible && repeatBlockedReason" class="muted repeat-hint">
        {{ repeatBlockedReason }}
      </p>
      <p v-if="editingTarget.trigger === 'single'" class="muted editor-note">
        未配置双击与长按时，单击在按下瞬间触发；返回/方向/音量键开启「按住连续触发」后，按住不放会连续执行（组合快捷键只触发一次）。配置双击时，按住超过约 0.3 秒即按单击处理并开始连续触发。
      </p>
      <p v-else class="muted editor-note">
        {{ editingTarget.trigger === "double" ? "双击判定窗口约 0.3 秒：配置后单击会稍等片刻以区分双击。" : "长按约 0.55 秒触发；「按住连续触发」开在长按时，触发后不松手会继续连续执行（组合快捷键只触发一次）。它与开在单击上的连续触发互斥。" }}
      </p>
    </article>

    <footer class="card mapping-footer">
      <div class="mapping-footer-status">
        <span class="status-dot" :class="rawInput?.phase === 'ready' ? 'success' : 'pending'"></span>
        <span>{{ phaseLabel(rawInput?.phase) }}</span>
        <button
          v-if="rawInput?.phase === 'ready' || rawInput?.phase === 'stopped' || rawInput?.phase === 'failed'"
          class="secondary-button footer-listener-toggle"
          type="button"
          :disabled="busy"
          @click="toggleListener"
        >
          {{ rawInput?.phase === "ready" ? "停止监听" : "启动监听" }}
        </button>
        <small v-if="mappingSnapshot && !mappings.enabled" class="muted"> · 总开关关闭（按键保持原样）</small>
      </div>
      <label class="toggle-row" title="开启后，操作实体遥控器不会切换正在编辑的按键。">
        <span>锁定当前按键</span>
        <input v-model="lockSelection" type="checkbox" class="toggle-input" />
      </label>
      <span class="muted lock-hint">按遥控器时保持当前编辑项</span>
      <div class="button-row">
        <button class="secondary-button" type="button" :disabled="busy" @click="saveConfiguration">
          保存配置
        </button>
        <button class="secondary-button" type="button" :disabled="busy" @click="importConfiguration">
          导入配置…
        </button>
        <button class="secondary-button" type="button" :disabled="busy" @click="exportConfiguration">
          导出配置…
        </button>
        <button class="secondary-button" type="button" :disabled="busy" @click="restoreDefaults">
          恢复默认
        </button>
      </div>
    </footer>

    <p v-if="statusMessage" class="operation-message mapping-status">{{ statusMessage }}</p>
    <p v-if="mappingSnapshot?.lastError" class="error-text">{{ mappingSnapshot.lastError }}</p>
    <RegisteredAppsDialog v-if="appPickerOpen" :known-apps="mappings.applications ?? []" :saving="busy" :save-error="appPickerError" @close="appPickerOpen = false" @add="addScannedApps" />
    <EnhancedCaptureConfirmDialog v-if="showCaptureConfirm" @confirm="confirmCaptureDialog" @close="closeCaptureDialog" />
  </section>
</template>

<style scoped>
/* 全按键支持开关的终值就绪前占位符：与 toggle-input 同尺寸（34x20），
   避免就绪后开关创建时标题行宽度跳动（同「启动行为」页做法）。 */
.toggle-placeholder { width: 34px; height: 20px; flex: none; }
/* 「按住连续触发」：编辑面板内的按键级开关（2026-10-05），与动作分组留出间距。 */
.repeat-toggle { margin-top: 12px; }
.repeat-hint { margin: 6px 0 0; font-size: 12px; }
.mouse-amount { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; margin-top: 10px; font-size: 13px; }
.mouse-amount input { width: 88px; max-width: 100%; padding: 5px 8px; font: inherit; color: inherit; background: transparent; border: 1px solid currentColor; border-radius: 4px; }
.mouse-direction { width: 40px; height: 30px; padding: 0; font-size: 17px; }
/* 滚动容器顶部会裁掉向上溢出（transform 不产生可滚动区域）：
   chip:hover 上浮 1px + 阴影会在容器顶边被切，故用内边距留出
   上浮空间，负外边距补偿保持原网格位置不变。 */
/* 打开应用：预设 + 已添加应用共用一个换行网格（2026-10-03 用户要求）。
   芯片内换行只给自定义应用名（预设名保持单行，避免撑高整行）。 */
.app-library .chip.custom-app { max-width: 100%; white-space: normal; overflow-wrap: anywhere; }
/* 「扫描本机应用 / ＋添加应用」排在所有应用名称之后（2026-10-03 用户要求）：
   与上面的应用列表（含可滚动列表）之间留出分组间距；选择器要压过
   `.action-section .preset-grid` 的 6px。 */
.action-section .preset-grid.app-library-actions { margin-top: 9px; }
.app-library-search { display: block; width: min(300px, 100%); box-sizing: border-box; margin-top: 10px; padding: 6px 8px; font: inherit; color: inherit; background: transparent; border: 1px solid #888; border-radius: 4px; }
</style>
