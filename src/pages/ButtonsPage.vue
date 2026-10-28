<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import RegisteredAppsDialog from "../components/RegisteredAppsDialog.vue";
import EnhancedCaptureConfirmDialog from "../components/EnhancedCaptureConfirmDialog.vue";
import BatteryIndicator from "../components/BatteryIndicator.vue";
import MembershipRequiredButton from "../components/MembershipRequiredButton.vue";
import { SHORTCUT_PRESET_GROUPS } from "../lib/shortcut-presets";
import { reportFrontendEvent, reasonToken } from "../lib/frontend-diagnostics";
import { t, tPage } from "../lib/i18n";
import { effectiveLanguage } from "../lib/language-state";
import { COMBINATION_ACTIONS_ENABLED } from "../lib/feature-flags";
import {
  FOCUS_STRATEGY_OPTIONS,
  effectiveFocusProfile,
  presetFocusDefault,
} from "../lib/focus-defaults";
import {
  captureFailureText,
  captureInactiveText,
  captureRevokedText,
  captureSwitchState as captureSwitchStateFn,
  normalizeCaptureRevoked,
  normalizeCaptureUnavailable,
  shouldShowCaptureInactive,
  shouldShowCaptureRevoked,
} from "../lib/capture-switch";
import {
  actionSummary,
  activateAppForLearning,
  buttonLabel,
  buttonLabels,
  buttonTriggerLabel,
  chordLabel,
  connectRemote,
  connectionPhaseLabel,
  exportButtonMappingConfiguration,
  getButtonMappingSnapshot,
  getButtonMappings,
  getCombinedActions,
  disableRc003Capture,
  enableRc003Capture,
  getCaptureSupport,
  getCommonPhrasePanelSnapshot,
  getLockMappingSelection,
  getRc003BridgeSnapshot,
  getRc003TaskStatus,
  identityShortcutByButton,
  importButtonMappingConfiguration,
  focusLearnErrorLabel,
  focusPointErrorLabel,
  learnFocusPoint,
  learnFocusTarget,
  listPresetApps,
  mouseClickLabels,
  mouseMoveLabels,
  openAppLabel,
  openCommonPhraseExample,
  pickCustomApp,
  registerCommonPhraseAvailability,
  registerPresetAppNames,
  registerMacroNames,
  latestMacroDefinitions,
  remoteModelLabel,
  resetButtonMappings,
  saveButtonMappings,
  saveLockMappingSelection,
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
  type CaptureSupport,
  type CommonPhraseAccess,
  type CustomAppPick,
  type AppFocusProfile,
  type FiredGesture,
  type FocusStrategy,
  type KeyCode,
  type MacroDefinition,
  type PairedRemote,
  type PresetAppInfo,
  type RawInputPhase,
  type Rc003BridgePhase,
  type Rc003CaptureHealth,
  type Rc003CaptureRevoked,
  type Rc003CaptureUnavailable,
  type Rc003TaskStatus,
  type RemoteButton,
  type RemoteModel,
  type RuntimeSnapshot,
  type ShortcutCaptureEdge,
} from "../lib/bridge";
import { settingsComponent as commonPhraseSettingsComponent } from "../private/windows-common-phrases";

const props = defineProps<{ runtime: RuntimeSnapshot | null }>();
const emit = defineEmits<{ navigate: [page: string] }>();

/**
 * 「调整常用语」二级视图（对齐 macOS：按键页 → 调整常用语 → 返回按键页）。
 * 编辑器来自私有 UI（公开构建返回 null → 入口不渲染）。
 */
const commonPhraseEditor = commonPhraseSettingsComponent();
const showCommonPhraseEditor = ref(false);

/** 画布几何：对齐 Mac RemoteMappingCanvas——高度固定 570，宽度流式
 * （占满容器，ResizeObserver 观测）；卡宽 = clamp((宽-260)/2, 270, 300)，
 * 与 Mac cardWidth(for:) 同公式；容器不足最小画布 800px 时由 CSS
 * --map-scale 连续缩放兜底（小于最小窗口的恢复态窗口）。 */
const CANVAS_MIN_WIDTH = 800;
const CANVAS_HEIGHT = 570;
// 遥控器实物图尺寸（2026-10-07 放大到可见约 115×480）：PNG 为 404×820，含透明边
// ——可见遥控器只占图内 187×768（46.3%×93.7%），所以容器比可见尺寸大：
// 250×508 ⇒ 可见 ≈ 115.9×475.8（缩放约 1.24×，图与容器同比例，object-fit: cover
// 不裁掉可见像素）。锚点/橙点/连线一律用归一化比例 × 本尺寸，故全部按比例跟随。
const REMOTE_WIDTH = 250;
const REMOTE_HEIGHT = 508;
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
  return connectionInfo.value?.remoteName ?? t("未连接遥控器");
});

/**
 * 操作失败文案：中文界面保留既有原始错误（既有行为与测试口径）；英文界面只给
 * 一句短的用户语言，原始错误写进控制台/诊断，不把中文原文摊到英文 UI 上。
 */
function operationErrorText(
  context: string,
  error: unknown,
  englishFallback: string,
): string {
  console.warn(context, error);
  if (effectiveLanguage.value === "zh-Hans") {
    return error instanceof Error ? error.message : String(error);
  }
  return t(englishFallback);
}

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

/**
 * 两个总开关的上次已知值（2026-10-28）。
 *
 * 为什么要单独缓存：这两只开关的值来自 IPC，而 IPC 必然异步——首帧只能先渲染占位，
 * 等数据回来再挂开关本体，用户看到的就是"先空白、再跳出来"（2026-10-28 反馈）。
 * 把上次的值同步写进 localStorage，首帧就能直接渲染成正确状态；IPC 返回的权威值
 * 通常与缓存一致，于是没有可见跳变。首次安装没有缓存时开关仍会从默认值改成真实值，
 * 那一次跳变无法避免。
 *
 * 只存两个布尔值，不含任何用户数据。
 */
const SWITCH_CACHE_KEY = "sayall.buttons.switches.v1";

function readSwitchCache(): { enabled: boolean; lockSelection: boolean } | null {
  try {
    const raw = localStorage.getItem(SWITCH_CACHE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as { enabled?: unknown; lockSelection?: unknown };
    if (typeof parsed.enabled !== "boolean" || typeof parsed.lockSelection !== "boolean") {
      return null;
    }
    return { enabled: parsed.enabled, lockSelection: parsed.lockSelection };
  } catch {
    return null;
  }
}

function writeSwitchCache(enabled: boolean, lockSelection: boolean): void {
  try {
    localStorage.setItem(SWITCH_CACHE_KEY, JSON.stringify({ enabled, lockSelection }));
  } catch {
    // 存储不可用时只是少一次首帧加速，不影响功能与保存。
  }
}

const switchCache = readSwitchCache();
const mappings = ref<ButtonMappings>({ enabled: switchCache?.enabled ?? true, actions: {} });
/** 映射是否已从设置读回：「启用自定义按键功能」开关就绪前用占位符顶位（防滑入）。 */
const mappingsLoaded = ref(false);
const savedSnapshot = ref<ButtonMappings>({ enabled: true, actions: {} });
/** 已安装的预设应用（打开应用动作可选列表）。 */
const presetApps = ref<PresetAppInfo[]>([]);
/** 组合动作库（「组合动作」动作分组的候选；2026-10-15）。 */
const macroDefinitions = ref<MacroDefinition[]>([]);
/** 常用语组件是否可用（页面加载快照后更新；不可用时不显示动作入口）。 */
const commonPhrasesAvailable = ref(false);
/** 常用语会员门禁：只有 allowed 才允许选择「打开常用语」；denied/unavailable 显示禁用。 */
const commonPhraseAccess = ref<CommonPhraseAccess>("unavailable");
const commonPhraseAllowed = computed(() => commonPhraseAccess.value === "allowed");
const selectedButton = ref<RemoteButton | null>(null);
const editingTarget = ref<{ button: RemoteButton; trigger: ButtonTrigger } | null>(null);
const editorPanel = ref<HTMLElement | null>(null);
const lockSelection = ref(switchCache?.lockSelection ?? true);
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
/** 「学习输入框」状态机：opening = 正在打开目标应用；learning = 已进入学习窗口。 */
const focusLearnState = ref<"idle" | "opening" | "learning" | "captured" | "error">("idle");
const focusLearnBusy = computed(
  () => focusLearnState.value === "opening" || focusLearnState.value === "learning",
);
const focusLearnMessage = ref<string | null>(null);
const capturePressedKeys = new Set<KeyCode>();
let capturedChord: KeyCode[] | null = null;
let unlistenEdges: (() => void) | null = null;
let unlistenGestures: (() => void) | null = null;
let unlistenShortcutCapture: (() => void) | null = null;
let captureTimeout: number | null = null;
let captureRequestId = 0;
let snapshotTimer: number | null = null;
let flashTimer: number | null = null;
/** 本次闪烁反馈的起点（性能时钟，用于"松开即灭、最短 150ms"的补足计算）。 */
let flashStartedAt = 0;
/** 闪烁反馈的最短可见时间：极短按也要看得见这一下反馈。 */
const FLASH_MIN_MS = 150;
/** 闪烁反馈的兜底时长：松开沿丢失（断连/异常）时不让它一直亮着。 */
const FLASH_FALLBACK_MS = 600;
let resizeObserver: ResizeObserver | null = null;
let unmounted = false;
let resourcesReady = false;

/** 「记录点击位置」的倒计时句柄与状态（6 秒后采集鼠标位置）。 */
let pointLearnTimer: number | null = null;
const pointLearnState = ref<"idle" | "opening" | "recording" | "recorded" | "error">("idle");
const pointLearnMessage = ref<string | null>(null);

/**
 * 「锁定当前按键」持久化（2026-10-18，用户定稿：跨页面切换与跨重启保持）。
 *
 * 背景：本开关此前只是页面局部状态，切页重建组件后回到默认「开启」，用户侧表现
 * 为"关不掉"。现在落进应用设置文件（`lock_mapping_selection`）：挂载时读回、
 * 变更时保存；保存失败回退到最近落盘值并把错误显示在页脚提示里。
 *
 * 2026-10-18 追加（Andy 报"每次进页面开关都滑动"）：就绪前用同尺寸占位符顶位、
 * 就绪后才创建开关本体——元素创建即带正确 checked，从不存在属性变更，因此不会
 * 有关→开的滑动过渡（同设置页「登录时自动启动」与同页「全按键支持」，见
 * SettingsPage.vue 2026-09-28 定稿注释：只禁用过渡不行，Vue 的 DOM 更新与
 * nextTick 同批早于绘制，过渡照常触发）。
 */
let lockSelectionPersisted = true;
/** 开关值是否已读回：false 期间渲染占位符，不渲染开关本体（防滑入动画）。 */
const lockSelectionReady = ref(false);
/** 保存串行队列：执行时取当前值，快速往返（关→开）收敛到最后一次选择。 */
let lockSelectionSaveQueue: Promise<void> = Promise.resolve();

async function persistLockSelection(): Promise<void> {
  const task = lockSelectionSaveQueue.then(async () => {
    const value = lockSelection.value;
    if (value === lockSelectionPersisted) return; // 加载回填 / 失败回退的重入：无需落盘
    try {
      lockSelectionPersisted = await saveLockMappingSelection(value);
      writeSwitchCache(mappings.value.enabled, lockSelectionPersisted);
    } catch (error) {
      // 只有用户没有再改时才回退开关，避免覆盖更新的选择。
      if (lockSelection.value === value) {
        lockSelection.value = lockSelectionPersisted;
        statusMessage.value = operationErrorText(
          "lock mapping selection save failed",
          error,
          "保存锁定当前按键设置失败，请重试。",
        );
      }
    }
  });
  lockSelectionSaveQueue = task;
  await task;
}

watch(lockSelection, () => {
  // 每次变更都过一遍持久化（回填/回退的重入在执行时被"值与落盘值相等"拦下），
  // 这样"关闭中又马上打开"也不会停在中间值。
  void persistLockSelection();
});

/**
 * 松开沿结束本次按压的闪烁反馈（与按下高亮、连线**同灭**）。
 *
 * 现场（2026-10-13）：未配映射的键"同亮同灭"，配了映射的反而不行——固定 600ms 的
 * 闪烁比按下高亮多亮约 400ms。现在闪烁跟随这一次按压：按下手势触发时起算，
 * 松开沿到达即结束；不足 150ms 的极短按补足到 150ms（保证看得见）。
 */
function endFiredFlash(button: RemoteButton): void {
  const flash = firedFlash.value;
  if (!flash || flash.button !== button) return;
  if (flashTimer !== null) {
    window.clearTimeout(flashTimer);
    flashTimer = null;
  }
  const remaining = FLASH_MIN_MS - (performance.now() - flashStartedAt);
  if (remaining <= 0) {
    firedFlash.value = null;
    return;
  }
  flashTimer = window.setTimeout(() => {
    firedFlash.value = null;
    flashTimer = null;
  }, remaining);
}

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
  if (pointLearnTimer !== null) window.clearTimeout(pointLearnTimer);
  pointLearnTimer = null;
  void stopShortcutCapture();
  if (snapshotTimer !== null) window.clearInterval(snapshotTimer);
  snapshotTimer = null;
  if (flashTimer !== null) window.clearTimeout(flashTimer);
  flashTimer = null;
  resizeObserver?.disconnect();
  resizeObserver = null;
}

/** 连续执行分组的「关闭」选项：同一中文原文在编辑面板里是 Close，这里需要 Off。 */
const repeatOffLabel = computed(() =>
  effectiveLanguage.value === "zh-Hans" ? "关闭" : "Off",
);

const dirty = computed(
  () => JSON.stringify(mappings.value) !== JSON.stringify(savedSnapshot.value),
);

const enabled = computed({
  get: () => mappings.value.enabled,
  set: (value: boolean) => {
    mappings.value = { ...mappings.value, enabled: value };
    void persist(t("总开关已更新"));
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

/** 卡片格子的悬停标题：随界面语言实时生成（含全角冒号的原文作为字典 key）。 */
function actionCellTitle(button: RemoteButton, trigger: ButtonTrigger): string {
  return t("{button} · {trigger}：{summary}", {
    button: buttonLabels[button],
    trigger: buttonTriggerLabel(trigger),
    summary: actionSummary(actionOf(button, trigger)),
  });
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

function isPresetTarget(target: string): boolean {
  return presetApps.value.some((app) => app.id === target);
}

/** 该目标实际会执行的聚焦档案：显式档案优先，预设应用回落到产品默认值。 */
const focusProfile = computed<AppFocusProfile | null>(() => {
  const target = focusTarget.value;
  if (!target) return null;
  return effectiveFocusProfile(target, mappings.value.focusProfiles, isPresetTarget);
});

/** 预设默认档案（只用于「恢复默认」入口与默认选中）。 */
const presetFocusDefaultProfile = computed<AppFocusProfile | null>(() => {
  const target = focusTarget.value;
  if (!target || !isPresetTarget(target)) return null;
  return presetFocusDefault(target, true);
});

/** 预设应用是否已有用户显式档案（与默认值区分）。 */
const hasPresetFocusOverride = computed(() => {
  const target = focusTarget.value;
  return Boolean(
    target && isPresetTarget(target) && mappings.value.focusProfiles?.[target],
  );
});

const focusStrategyOptions = computed(() =>
  FOCUS_STRATEGY_OPTIONS.map((option) => ({ ...option, label: t(option.label) })),
);

/** 录入 / 学习 / 记录位置期间，切换策略会干扰当前会话。 */
const focusBusy = computed(
  () =>
    focusLearnBusy.value ||
    capturingShortcut.value ||
    pointLearnState.value === "opening" ||
    pointLearnState.value === "recording",
);

// 2026-10-03 Andy：先隐藏「打开后聚焦方式」面板（后端能力与已存配置保留，UI 暂不暴露）。
// 2026-10-15 Andy：要求恢复，并参考 Mac 端补上「聚焦输入框」入口。
// 2026-10-23 Andy：预设应用也显示聚焦方式，并默认选中其实际使用的策略。
const showAppFocusStrategy = computed(() => Boolean(focusTarget.value));

/** 聚焦失败原因的用户文案（按当前语言实时求值，不在 setup 阶段缓存）。 */
function focusFailureLabel(reason: string): string {
  switch (reason) {
    case "self_foreground":
      return t("当前就是无线麦自己的窗口，无需聚焦");
    case "no_candidate":
      return t("没有找到可用的输入框");
    case "not_foreground":
      return t("目标应用不在前台");
    case "target_exited":
      return t("目标应用已退出");
    case "not_accessible":
      return t("读不到该应用的界面（对方可能以管理员身份运行）");
    case "timeout":
      return t("等待超时");
    case "not_configured":
      return t("还没有配置聚焦方式");
    case "cancelled":
      return t("已取消");
    default:
      // 未知 token 只在中文界面回显原文；英文界面不暴露内部 token。
      if (!reason) return t("未知原因");
      return effectiveLanguage.value === "zh-Hans" ? reason : t("未知原因");
  }
}

/** 最近一次聚焦结果（失败时给用户可解释的原因）。 */
const lastFocusNotice = computed<string | null>(() => {
  // 聚焦服务只服务「聚焦已记录的输入框」；「记录点击位置」不走这个服务，
  // 继续显示旧的失败会让用户以为新配置仍然失败（2026-10-19 现场）。
  if (focusProfile.value?.strategy !== "recorded_element") return null;
  const report = mappingSnapshot.value?.lastFocus;
  if (!report || report.focused) return null;
  return t("上次聚焦失败：{reason}", {
    reason: focusFailureLabel(report.reason ?? ""),
  });
});

/** 聚焦档案写回：策略与字段保持自洽（对齐 Rust `AppFocusProfile::normalized`）。 */
function applyFocusStrategy(strategy: FocusStrategy): void {
  const target = focusTarget.value;
  if (!target) return;
  const explicit = mappings.value.focusProfiles?.[target];
  const fallback = presetFocusDefaultProfile.value ?? undefined;
  const previous = explicit ?? fallback;
  const profile: AppFocusProfile = { strategy };
  if (strategy === "app_shortcut") {
    // 预设应用首次切到快捷键档时带上产品默认组合键（Chrome Ctrl+L / 网易云 Ctrl+F）。
    const shortcut = previous?.shortcut ?? fallback?.shortcut;
    if (shortcut) profile.shortcut = shortcut;
  }
  if (strategy === "recorded_element" && previous?.recorded) profile.recorded = previous.recorded;
  if (strategy === "recorded_point" && previous?.point) profile.point = previous.point;
  const next = { ...(mappings.value.focusProfiles ?? {}), [target]: profile };
  mappings.value = { ...mappings.value, focusProfiles: next };
  // 切走策略时清掉上一策略的提示，避免“已记录点击位置”出现在学习输入框下面。
  if (!focusLearnBusy.value) {
    focusLearnState.value = "idle";
    focusLearnMessage.value = null;
  }
  if (pointLearnState.value !== "opening" && pointLearnState.value !== "recording") {
    pointLearnState.value = "idle";
    pointLearnMessage.value = null;
  }
  void persist(t("聚焦方式已更新"));
}

/** 预设应用恢复产品默认聚焦方式（删除显式档案）。 */
function resetPresetFocus(): void {
  const target = focusTarget.value;
  if (!target) return;
  const next = { ...(mappings.value.focusProfiles ?? {}) };
  delete next[target];
  mappings.value = { ...mappings.value, focusProfiles: next };
  focusLearnState.value = "idle";
  focusLearnMessage.value = null;
  pointLearnState.value = "idle";
  pointLearnMessage.value = null;
  void persist(t("已恢复默认聚焦方式"));
}

/**
 * 「记录点击位置」：把鼠标所在的相对位置记到目标应用上。
 *
 * 面向完全不暴露无障碍接口的应用（2026-10-15 实测：微信 4.x 的 UIA 树里没有任何
 * 可聚焦元素），只能按位置点击。
 */
function recordFocusPoint(): void {
  const target = focusTarget.value;
  if (!target) return;
  if (pointLearnState.value === "recording" || pointLearnState.value === "opening") return;
  void openAppThenRecordPoint(target);
}

/** 对齐 Mac 的学习流程：先打开/切到目标应用，再开始记录。 */
async function openAppThenRecordPoint(target: string): Promise<void> {
  pointLearnState.value = "opening";
  pointLearnMessage.value = t("正在打开目标应用…");
  try {
    await activateAppForLearning(target);
  } catch {
    pointLearnState.value = "error";
    pointLearnMessage.value = t("没能打开目标应用，请确认它已安装后再试。");
    return;
  }
  pointLearnState.value = "recording";
  pointLearnMessage.value = t("正在等待你把鼠标停在目标输入框上……（6 秒后自动记录）");
  if (pointLearnTimer) window.clearTimeout(pointLearnTimer);
  pointLearnTimer = window.setTimeout(() => {
    pointLearnTimer = null;
    void finishPointRecording(target);
  }, 6000);
}

async function finishPointRecording(target: string): Promise<void> {
  try {
    const point = await learnFocusPoint(target);
    const next = {
      ...(mappings.value.focusProfiles ?? {}),
      [target]: { strategy: "recorded_point" as const, point },
    };
    mappings.value = { ...mappings.value, focusProfiles: next };
    pointLearnState.value = "recorded";
    pointLearnMessage.value = t("已记录位置（窗口大小变化超过 25% 时会拒绝点击，需要重新记录）");
    await persist();
  } catch (error) {
    const raw = error instanceof Error ? error.message : String(error);
    console.warn("learn focus point failed", raw);
    pointLearnState.value = "error";
    pointLearnMessage.value =
      effectiveLanguage.value === "zh-Hans"
        ? `记录位置失败：${focusPointErrorLabel(raw)}`
        : t("记录位置失败，请重新记录。");
  }
}

function clearRecordedPoint(): void {
  const target = focusTarget.value;
  if (!target) return;
  const next = {
    ...(mappings.value.focusProfiles ?? {}),
    [target]: { strategy: "recorded_point" as const },
  };
  mappings.value = { ...mappings.value, focusProfiles: next };
  pointLearnState.value = "idle";
  pointLearnMessage.value = null;
  void persist(t("已清除记录的位置"));
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
  void persist(t("聚焦快捷键已录入"));
}

function clearFocusShortcut(): void {
  const target = focusTarget.value;
  if (!target) return;
  // 预设应用的快捷键可能来自产品默认值（还没有显式档案），清除时也要落一条
  // 显式“快捷键档但未录入”的档案，否则按钮看起来可点却没有任何效果。
  const profile = mappings.value.focusProfiles?.[target] ?? focusProfile.value;
  if (profile?.strategy !== "app_shortcut") return;
  const next = {
    ...(mappings.value.focusProfiles ?? {}),
    [target]: { strategy: "app_shortcut" as const },
  };
  mappings.value = { ...mappings.value, focusProfiles: next };
  void persist(t("已清除聚焦快捷键"));
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
  void persist(t("已清除记录的输入框"));
}

async function beginFocusShortcutCapture(): Promise<void> {
  if (focusLearnBusy.value) return;
  capturePurpose.value = "focus_shortcut";
  await beginShortcutCapture();
}

/**
 * 「学习输入框」：先打开/切到目标应用，再在最多 6 秒的窗口内轮询系统焦点
 * （焦点不可用时取鼠标命中的输入候选）。用户在此期间点击目标输入框。
 * 采集只读控件语义（不含输入内容），写入该目标的聚焦档案。
 */
async function startFocusLearning(): Promise<void> {
  if (focusLearnBusy.value) return;
  const target = focusTarget.value;
  if (!target) return;
  focusLearnState.value = "opening";
  focusLearnMessage.value = t("正在打开目标应用…");
  try {
    await activateAppForLearning(target);
  } catch {
    focusLearnState.value = "error";
    focusLearnMessage.value = t("没能打开目标应用，请确认它已安装后再试。");
    return;
  }

  focusLearnState.value = "learning";
  focusLearnMessage.value = t("正在等待你点击目标输入框……");
  try {
    const recorded = await learnFocusTarget();
    const next = {
      ...(mappings.value.focusProfiles ?? {}),
      [target]: { strategy: "recorded_element" as const, recorded },
    };
    mappings.value = { ...mappings.value, focusProfiles: next };
    await persist();
    focusLearnState.value = "captured";
    focusLearnMessage.value = t("输入框学习成功。");
  } catch (error) {
    const raw = error instanceof Error ? error.message : String(error);
    console.warn("learn focus target failed", raw);
    focusLearnState.value = "error";
    focusLearnMessage.value =
      effectiveLanguage.value === "zh-Hans"
        ? focusLearnErrorLabel(raw)
        : t("输入框学习失败，请重试。");
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
    statusMessage.value = t("已添加 {count} 个应用，按键绑定未改变", { count: apps.length });
  } catch (cause) {
    appPickerError.value = operationErrorText(
      "add scanned apps failed",
      cause,
      "没能添加应用，请重试。",
    );
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

/** 「常用语示例」：打开常用语面板（会员=普通、非会员=只读预览；判定在宿主层）。 */
async function openExample(): Promise<void> {
  try {
    await openCommonPhraseExample();
  } catch {
    // 未接通私有组件（公开构建）：入口不渲染，正常不会到这里；留静默兜底。
  }
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
  const reconciled = reconcileHoldRepeat(actions);
  next.actions[target.button] = reconciled.actions;
  mappings.value = next;
  // 对齐 Mac：点击动作即自动保存生效（静默；失败时显示错误信息）。互斥收口
  // 产生的说明随保存一起展示（否则会被 persist 的清理覆盖）。
  void persist(reconciled.notice ?? undefined);
}

const selectedAction = computed(() => editingTarget.value ? actionOf(editingTarget.value.button, editingTarget.value.trigger) : null);
const MOUSE_AMOUNT_DEFAULT = 20;
const scrollSteps = computed(() => selectedAction.value?.type === "scroll" ? selectedAction.value.steps ?? MOUSE_AMOUNT_DEFAULT : MOUSE_AMOUNT_DEFAULT);
const moveDistance = computed(() => selectedAction.value?.type === "mouse_move" ? selectedAction.value.distance : MOUSE_AMOUNT_DEFAULT);

/**
 * 鼠标数量范围（2026-10-22 用户要求：滚轮格数与移动像素统一为 1–100，
 * 默认 20）。输入框是普通文本框，不再带数字框的上下加减按钮。
 */
const MAX_MOUSE_AMOUNT = 100;

/**
 * 输入中的草稿值。**不能**把输入框直接绑到 `scrollSteps` / `moveDistance`：
 * App 每秒刷新运行状态会让页面重渲染，Vue 的 value patch 会把正在输入的内容
 * 重置回配置值（2026-10-22 用户现场：“修改后变回默认值”）。
 */
const scrollDraft = ref(String(MOUSE_AMOUNT_DEFAULT));
const moveDraft = ref(String(MOUSE_AMOUNT_DEFAULT));
const mouseAmountError = ref<string | null>(null);
const MOUSE_AMOUNT_HINT = computed(() =>
  t("请输入 1 到 100 之间的整数，并按Enter或鼠标点击空白处保存配置。"),
);

/** 点击/重新聚焦输入框时收回错误，恢复输入提示。 */
function clearMouseAmountError(): void {
  mouseAmountError.value = null;
}

/** 鼠标动作大类的数量输入：只有滚轮/移动动作显示，按动作类型取对应草稿。 */
const mouseAmountVisible = computed(
  () =>
    selectedAction.value?.type === "scroll" ||
    selectedAction.value?.type === "mouse_move",
);
const mouseAmountKind = computed<"scroll" | "mouse_move">(
  () => (selectedAction.value?.type === "mouse_move" ? "mouse_move" : "scroll"),
);
const mouseAmountDraft = computed({
  get: () =>
    selectedAction.value?.type === "mouse_move" ? moveDraft.value : scrollDraft.value,
  set: (value: string) => {
    if (selectedAction.value?.type === "mouse_move") moveDraft.value = value;
    else scrollDraft.value = value;
  },
});

function syncMouseAmountDrafts(): void {
  const action = selectedAction.value;
  clearMouseAmountError();
  scrollDraft.value = String(
    action?.type === "scroll" ? action.steps ?? MOUSE_AMOUNT_DEFAULT : MOUSE_AMOUNT_DEFAULT,
  );
  moveDraft.value = String(
    action?.type === "mouse_move" ? action.distance : MOUSE_AMOUNT_DEFAULT,
  );
}

// 只在动作类型/方向/数值变化时同步草稿；运行状态轮询导致的重渲染不会触碰草稿。
watch(
  () => {
    const action = selectedAction.value;
    if (action?.type === "scroll") {
      return `scroll:${action.direction}:${action.steps ?? MOUSE_AMOUNT_DEFAULT}`;
    }
    if (action?.type === "mouse_move") {
      return `move:${action.direction}:${action.distance}`;
    }
    return "none";
  },
  syncMouseAmountDrafts,
  { immediate: true },
);

/**
 * 鼠标数量输入的提交入口：Enter（`keydown.enter`）与失焦共用。
 * 滚轮与移动距离都只要求 1–100 的整数，且共用同一保存与校验口径。
 * 值未变化时直接返回，避免 Enter 后再失焦重复触发一次保存。
 */
function updateMouseAmount(): void {
  const action = selectedAction.value;
  if (action?.type !== "scroll" && action?.type !== "mouse_move") return;
  const kind = action.type === "mouse_move" ? "mouse_move" : "scroll";
  const value = Number(mouseAmountDraft.value.trim());
  if (!Number.isInteger(value) || value < 1 || value > MAX_MOUSE_AMOUNT) {
    // 2026-10-23 用户要求：错误提示贴在输入框旁，不再出现在页面最下方。
    mouseAmountError.value = t("请输入 1 到 {max} 之间的整数", { max: MAX_MOUSE_AMOUNT });
    mouseAmountDraft.value = String(kind === "scroll" ? scrollSteps.value : moveDistance.value);
    return;
  }
  clearMouseAmountError();
  if (action.type === "scroll") {
    // 显式输入默认值也要落盘（旧配置可能没有 steps 字段）。
    if (action.steps === value) return;
    void applyAction({ ...action, steps: value });
  } else {
    if (action.distance === value) return;
    void applyAction({ ...action, distance: value });
  }
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
 * 「按住时连续执行」的界面规则（2026-10-05 radio 三选一定稿）：
 * - 按键级三态：关闭 / 重复单击动作 / 重复长按动作（双击不参与），显示在
 *   编辑区底部的「连续执行」分组（鼠标动作之后），所有按键的单击/长按页都显示，
 *   两页是同一个状态；
 * - 选项可用性由配置决定：未配置动作 / 动作只执行一次 / 单击与长按并存
 *   （长按会接管按住，单击不能连续）；不可选时给「原因 + 恢复方式」；
 * - 已选目标不再可用（配置变化）时自动回到「关闭」并提示，不偷改选择。
 * 定稿依据：docs/plan/2026-10-05-button-hold-repeat-radio.md。
 * 口径与 Rust `ButtonAction::allows_repeat` / `ButtonMappings::normalized` 同源。
 */
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

const repeatGroupVisible = computed(() => {
  const target = editingTarget.value;
  if (!target) return false;
  return target.trigger === "single" || target.trigger === "long";
});

type HoldRepeatChoice = "off" | "single" | "long";

/** 选项不可选的原因（null = 可选）；radio 展示与一致性收口共用同一判据。 */
function holdRepeatBlockedReason(
  actions: ButtonActions,
  slot: "single" | "long",
): string | null {
  if (slot === "long") {
    if (actions.long.type === "disabled") return t("先给这个按键配置长按动作");
    if (!actionAllowsRepeat(actions.long)) {
      return t("“长按”的动作只执行一次（组合快捷键、打开应用等），不能连续执行；把它改成单个按键后可选");
    }
    return null;
  }
  if (actions.single.type === "disabled") return t("先给这个按键配置单击动作");
  if (!actionAllowsRepeat(actions.single)) {
    return t("“单击”的动作只执行一次（组合快捷键、打开应用等），不能连续执行；改成单个按键后可选");
  }
  if (actions.long.type !== "disabled") {
    // 指路前先确认「重复长按动作」真的可选：长按动作只执行一次时它同样是
    // 灰的，原句会指向一个点不到的选项（2026-10-05 用户反馈）。
    return holdRepeatBlockedReason(actions, "long") === null
      ? t("这个按键已有长按动作，按住会先执行长按；要连续执行，请选“重复长按动作”")
      : t("这个按键已有长按动作：按住会执行长按。要连续执行单击动作，请先清除长按动作");
  }
  return null;
}

/** 当前按键的「按住时连续执行」状态（按键级，两页同源）。 */
const repeatChoice = computed<HoldRepeatChoice>(() => {
  const target = editingTarget.value;
  const hold = target ? actionsOf(target.button).holdRepeat : undefined;
  return hold === "long" ? "long" : hold === "single" ? "single" : "off";
});

const repeatSingleReason = computed<string | null>(() => {
  const target = editingTarget.value;
  return target ? holdRepeatBlockedReason(actionsOf(target.button), "single") : null;
});

const repeatLongReason = computed<string | null>(() => {
  const target = editingTarget.value;
  return target ? holdRepeatBlockedReason(actionsOf(target.button), "long") : null;
});

/** 常驻原因行：只解释当前页面对应的选项。 */
const repeatPageReason = computed<string | null>(() => {
  const target = editingTarget.value;
  if (!target) return null;
  if (target.trigger === "single") return repeatSingleReason.value;
  if (target.trigger === "long") return repeatLongReason.value;
  return null;
});

/** 当前编辑按键是否配置了双击（「重复单击动作」悬停据此切换口径）。 */
const doubleConfiguredOnEditingButton = computed(() => {
  const target = editingTarget.value;
  return !!target && actionsOf(target.button).double.type !== "disabled";
});

/** 「重复单击动作」的悬停说明（不可选时为原因；配双击时按压住确认口径）。 */
const repeatSingleTitle = computed(() => {
  if (repeatSingleReason.value) return repeatSingleReason.value;
  return doubleConfiguredOnEditingButton.value
    ? t("按住超过约 0.3 秒开始，单击动作会不断重复，直到松手")
    : t("按住不放，单击动作会不断重复，直到松手");
});

/** 「重复长按动作」的悬停说明（不可选时为原因）。 */
const repeatLongTitle = computed(
  () => repeatLongReason.value ?? t("按住约 0.55 秒后，长按动作会不断重复，直到松手"),
);

/**
 * OK 键「移动光标后按 OK 点击」（2026-10-06 定稿，方案见
 * `docs/plan/2026-10-06-ok-context-click.md`、产品逻辑见
 * `docs/product/button-behavior.md` §2）：按键级开关、默认关、只对 OK 键提供，
 * 三个槽位页都在同一位置显示（与当前编辑哪个槽位无关）。
 */
const okClickVisible = computed(
  () => editingTarget.value?.button === "ok" && editingTarget.value.trigger !== "double",
);

const okClickChecked = computed(() => actionsOf("ok").okContextClick === true);

function toggleOkContextClick(): void {
  const target = editingTarget.value;
  if (!target || target.button !== "ok") return;
  const actions = { ...actionsOf("ok") };
  if (actions.okContextClick) {
    delete actions.okContextClick;
  } else {
    actions.okContextClick = true;
  }
  mappings.value = {
    ...mappings.value,
    actions: { ...mappings.value.actions, [target.button]: actions },
  };
  void persist(
    actions.okContextClick
      ? t("已开启：用遥控器移动光标后，按 OK 会点击光标位置")
      : t("已关闭：移动光标后按 OK 点击"),
  );
}

/**
 * 动作变更后的「按住时连续执行」一致性收口：已选目标不再可用时自动回到
 * 「关闭」并返回提示（不偷改选择到另一个目标，保留用户的显式选择）。
 * 提示经 `persist(message)` 展示，避免被 persist 的清理逻辑覆盖。
 */
function reconcileHoldRepeat(
  actions: ButtonActions,
): { actions: ButtonActions; notice: string | null } {
  const hold = actions.holdRepeat;
  if (!hold) return { actions, notice: null };
  const slot = hold === "long" ? "long" : "single";
  if (holdRepeatBlockedReason(actions, slot) === null) return { actions, notice: null };
  const next = { ...actions };
  delete next.holdRepeat;
  // 自动关闭的提示同样不得指向灰掉的选项（长按动作只执行一次时它不可选）。
  const notice =
    slot === "single" && actions.long.type !== "disabled"
      ? holdRepeatBlockedReason(actions, "long") === null
        ? t("这个按键已配置长按动作：按住会先执行长按，“按住时连续执行”已关闭（要连续执行，请选“重复长按动作”）")
        : t("这个按键已配置长按动作：按住会执行长按，而它只执行一次，“按住时连续执行”已关闭")
      : t("动作已变更，“按住时连续执行”已关闭");
  return { actions: next, notice };
}

function selectHoldRepeat(choice: HoldRepeatChoice): void {
  const target = editingTarget.value;
  if (!target) return;
  const reason =
    choice === "single"
      ? repeatSingleReason.value
      : choice === "long"
        ? repeatLongReason.value
        : null;
  if (reason) {
    statusMessage.value = reason;
    return;
  }
  if (repeatChoice.value === choice) return;
  const actions = { ...actionsOf(target.button) };
  if (choice === "off") {
    delete actions.holdRepeat;
  } else {
    actions.holdRepeat = choice;
  }
  mappings.value = {
    ...mappings.value,
    actions: { ...mappings.value.actions, [target.button]: actions },
  };
  void persist(
    choice === "off"
      ? t("已关闭“按住时连续执行”")
      : choice === "long"
        ? t("已开启：长按后不松手会继续连续执行")
        : t("已开启：按住不放会连续执行"),
  );
}

/**
 * 三键（返回/音量±）关闭态的统一说明（2026-09-28 Andy 定稿）：
 * 编辑器能力说明与卡片置灰禁用悬停提示共用，悬停提示在句尾补「开启后恢复正常」
 * （2026-10-03 Andy 定稿）。
 */
const triKeyCaptureHint = computed(() =>
  t("返回 / 音量+ / 音量−需要开启全按键支持才能使用"),
);
const triKeyGatedTitle = computed(() =>
  captureUnsupported.value
    ? t("返回 / 音量+ / 音量−需要全按键支持；这台电脑暂不支持全按键支持，新版本支持后可配置。")
    : t("{hint}，开启后恢复正常", { hint: triKeyCaptureHint.value }),
);

/**
 * 本机不可用（ARM64，2026-10-07 issue #206）时说不得「开启后恢复正常」——
 * 这台电脑根本开不了。三句都只讲用户能懂的事：哪个开关、为什么、怎么办。
 * 用户可见文案的规范来源是 docs/product-copy.md（§2 名称表 / §7 决定索引）。
 */
const captureUnsupportedTitle = computed(() =>
  t("全按键支持在这台电脑上暂不可用；新版本支持后会恢复正常。"),
);
const captureUnsupportedNote = computed(() =>
  t("这台电脑暂不支持全按键支持，其他按键不受影响。"),
);
/**
 * 失败文案（2026-10-10 Andy 定稿）：只有失败才显示；其余情况一个字都不显示。
 * 本页名称不带引号，与既有短说明同口径（见 docs/product-copy.md §7 2026-10-07 与
 * 2026-10-10 两条）。
 */
const CAPTURE_FAILURE_NAME = "全按键支持";
const captureBridgeFailedText = computed(() =>
  t("全按键支持没有启动成功。请关闭后重新开启；仍不行就先断开遥控器再重新配对。"),
);

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
      ? t("全按键支持已启用，此按键的映射现在生效")
      : t("全按键支持已启用，此按键的映射已优化");
  }
  if (button === "back" || button === "volume_up" || button === "volume_down") {
    // 三键的映射路径对两个型号一致（下游同为映射引擎），界面不做型号区分：
    // 文案只随开关状态走。RC001 的三键同样依赖增强捕获（2026-09-25 真机：
    // 开=可映射、关=不映射），不再按"RC001 免助手"设计。
    if (rc003CaptureEnabled.value === false) {
      return triKeyCaptureHint.value;
    }
    return t("提示：正在确认按键状态…");
  }
  if (button === "home" || button === "tv") {
    return t("提示：保存后，遥控器连接期间这个键只执行你配置的动作；同时物理键盘上的对应按键（Home / `）也会执行同样的动作。断开遥控器或删除本键映射即恢复原样。");
  }
  if (shortcutCapability(button, "single", remoteModel.value) === "identity") {
    const identity = identityShortcutByButton[button];
    const label = identity ? chordLabel({ keys: [identity] }) : "";
    return t("提示：闲置约 4 秒后第一次按这个键，可能同时出现一次它原本的按键效果；这段时间内连按不受影响。单击动作若就是该键本身（{label}），多出的那一次会被自动合并。", { label });
  }
  return null;
});

/**
 * 全按键支持开关的悬停提示（2026-09-28 Andy 定稿）：随开关状态切换两句。
 * 状态未就绪（null）按关闭态口径显示——占位符阶段开关本体都不存在，
 * 真正可悬停时对账大概率已落地。
 *
 * 2026-10-07 增补：本机不可用（ARM64）时固定为不可用口径——因为本机开不了，
 * 「开启后支持使用…」这句在这类机器上是谎话。
 */
const captureSwitchTitle = computed(() =>
  captureUnsupported.value
    ? captureUnsupportedTitle.value
    : rc003CaptureEnabled.value === true
      ? t("关闭后返回 / 音量+ / 音量−将不可映射")
      : t("开启后支持使用返回 / 音量+ / 音量−，其他按键将同步优化"),
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
      writeSwitchCache(saved.enabled, lockSelection.value);
      if (request === saveRequest) {
        mappings.value = saved;
        if (message) statusMessage.value = message;
      }
    } catch (error) {
      if (request === saveRequest) {
        statusMessage.value = operationErrorText(
          "save button mappings failed",
          error,
          "没能保存按键映射，请重试。",
        );
      }
    }
  });
  saveQueue = task;
  await task;
  if (request === saveRequest) busy.value = false;
}

// 页面底部的快照错误只显示固定一句用户语言；原始错误留在控制台/后端日志。
watch(
  () => mappingSnapshot.value?.lastError,
  (raw) => {
    if (raw) console.warn("button mapping snapshot error", raw);
  },
);

async function restoreDefaults(): Promise<void> {
  busy.value = true;
  statusMessage.value = null;
  try {
    const saved = await resetButtonMappings();
    mappings.value = saved;
    savedSnapshot.value = JSON.parse(JSON.stringify(saved)) as ButtonMappings;
    statusMessage.value = t("已恢复默认（全部按键保持原始行为）");
  } catch (error) {
    statusMessage.value = operationErrorText(
      "restore default button mappings failed",
      error,
      "没能恢复默认设置，请重试。",
    );
  } finally {
    busy.value = false;
  }
}

async function saveConfiguration(): Promise<void> {
  await persist(t("配置已保存并生效"));
}

async function exportConfiguration(): Promise<void> {
  busy.value = true;
  statusMessage.value = null;
  try {
    const exported = await exportButtonMappingConfiguration();
    if (exported) statusMessage.value = t("按键映射配置已导出");
  } catch (error) {
    statusMessage.value = operationErrorText(
      "export button mappings failed",
      error,
      "没能导出配置，请重试。",
    );
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
    statusMessage.value = t("按键映射配置已导入并生效");
  } catch (error) {
    statusMessage.value = operationErrorText(
      "import button mappings failed",
      error,
      "没能导入配置，请重试。",
    );
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
    Semicolon: "oem1",
    Equal: "oem_plus",
    Comma: "oem_comma",
    Minus: "oem_minus",
    Period: "oem_period",
    Slash: "oem2",
    Backquote: "oem3",
    BracketLeft: "oem4",
    Backslash: "oem5",
    BracketRight: "oem6",
    Quote: "oem7",
    IntlBackslash: "oem102",
    Numpad0: "numpad0",
    Numpad1: "numpad1",
    Numpad2: "numpad2",
    Numpad3: "numpad3",
    Numpad4: "numpad4",
    Numpad5: "numpad5",
    Numpad6: "numpad6",
    Numpad7: "numpad7",
    Numpad8: "numpad8",
    Numpad9: "numpad9",
    NumpadMultiply: "numpad_multiply",
    NumpadAdd: "numpad_add",
    NumpadSubtract: "numpad_subtract",
    NumpadDecimal: "numpad_decimal",
    NumpadDivide: "numpad_divide",
    NumLock: "numlock",
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
const CAPTURE_MODIFIER_SOURCES: Array<{ key: KeyCode; label: string }> = [
  { key: "left_control", label: "左 Ctrl" },
  { key: "left_shift", label: "左 Shift" },
  { key: "left_alt", label: "左 Alt" },
  { key: "left_windows", label: "左 Win" },
  { key: "right_control", label: "右 Ctrl" },
  { key: "right_shift", label: "右 Shift" },
  { key: "right_alt", label: "右 Alt" },
  { key: "right_windows", label: "右 Win" },
];
const captureModifierOptions = computed(() =>
  CAPTURE_MODIFIER_SOURCES.map((option) => ({
    key: option.key,
    label: t(option.label),
  })),
);

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
      void finishShortcutCapture(t("录入已超时，请重新录入"));
    }, 15_000);
  } catch (error) {
    statusMessage.value = operationErrorText(
      "start shortcut capture failed",
      error,
      "没能开始录入快捷键，请重试。",
    );
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
    void finishShortcutCapture(t("窗口失去焦点，已取消录入"));
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
        void finishShortcutCapture(t("快捷键已录入：{label}", { label }));
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
      statusMessage.value = t("安全录入中：请松开键盘修饰键，并在界面中点击选择");
    } else {
      captureDisplay.value = [...pressedCaptureModifiers];
    }
    return;
  }
  if (safeCaptureMode.value && pressedCaptureModifiers.size > 0) {
    statusMessage.value = t("未录入：请不要按住键盘修饰键；先在界面选择修饰键，再单独按主键");
    return;
  }
  const modifiers = safeCaptureMode.value
    ? [...selectedCaptureModifiers]
    : [...pressedCaptureModifiers];
  if (code === "escape" && modifiers.length === 0) {
    void finishShortcutCapture(t("已取消录入"));
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
  statusMessage.value = t("已录入 {label}，松开全部按键后完成", { label: chordLabel({ keys }) });
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
      return t("按键监听已就绪");
    case "starting":
      return t("正在启动监听");
    case "failed":
      return t("监听启动失败（自动重试中）");
    case "stopped":
      return t("监听已停止");
    case "awaiting":
      return t("等待遥控器连接");
    case "unsupported":
      return t("当前环境暂不支持");
    default:
      return t("正在读取状态");
  }
}

/** 手动启停监听：自动启动之外保留显式控制（停止后自动重试不生效）。 */
async function toggleListener(): Promise<void> {
  busy.value = true;
  statusMessage.value = null;
  try {
    if (rawInput.value?.phase === "ready") {
      await stopRawInput();
      statusMessage.value = t("监听已停止；映射与高亮暂停（按住说话不受影响）");
    } else {
      await startRawInput();
      statusMessage.value = t("监听已启动");
    }
  } catch (error) {
    statusMessage.value = operationErrorText(
      "toggle raw input listener failed",
      error,
      "没能切换按键监听状态，请重试。",
    );
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
  reconnectBusy.value
    ? t("连接中…")
    : connectionReady.value
      ? t("重新连接")
      : t("立即连接"),
);
const reconnectDisabled = computed(
  () => reconnectBusy.value || props.runtime?.platform.bleScanAvailable === false,
);
const reconnectTitle = computed(() => {
  if (reconnectBusy.value) return t("正在连接遥控器…");
  if (props.runtime?.platform.bleScanAvailable === false) {
    return t("蓝牙不可用：先在连接页确认蓝牙已打开，再回来重试");
  }
  return connectionReady.value
    ? t("断开后连回当前这支遥控器")
    : t("连接已配对的遥控器");
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
      statusMessage.value = t("没有找到已配对的遥控器；到连接页扫描后再试。");
      return;
    }
    await connectRemote(target.id);
  } catch (error) {
    statusMessage.value = operationErrorText(
      "reconnect remote failed",
      error,
      "没能连接遥控器，请重试。",
    );
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
/** 桥接相位（从快照中提取）：首帧可由缓存给出终值，后续随对账刷新。失败时进开关文案。 */
const rc003BridgePhase = ref<Rc003BridgePhase | null>(rc003UiCache.phase);
/** 桥接段失败（开关已开、助手没起来）：作为「开启失败」进开关下方文案。 */
const rc003BridgeFailed = ref(false);
/**
 * 「全按键支持」实际生效状态（2026-10-13）：由 Rust 的常驻对账发布，本页只读。
 * 开关开着 ≠ 生效——`inactive` 才显示提示（见 captureInactiveNote）。
 */
const rc003CaptureHealth = ref<Rc003CaptureHealth>("off");
/** 开着没生效的原因（2026-10-27，issue #291）：决定提示说"关掉再打开"还是"别的程序占着"。 */
const rc003CaptureUnavailable = ref<Rc003CaptureUnavailable | "unknown">("none");
/** 启动对账把开关关掉的原因（2026-10-15，issue #264 附加观察甲）：`none` = 没发生。 */
const rc003CaptureRevoked = ref<Rc003CaptureRevoked | "unknown">("none");
/** 上一次开关操作失败的**原文**：只进日志与映射，界面显示映射后的用户语言。 */
const rc003CaptureFailureRaw = ref<string | null>(null);

/**
 * 「全按键支持」在本机是否可用（2026-10-07 issue #206，Windows 11 ARM64）。
 *
 * `null` = 尚未读到（首帧、或对账失败）：按可用处理——宁可暂时给出可点的开关，
 * 也不因一次 IPC 失败把功能锁死；真不可用时后端启动即把开启意图回落为关闭，
 * 开关也不会显示成开启。
 */
const captureSupport = ref<CaptureSupport | null>(null);
const captureUnsupported = computed(() => captureSupport.value?.available === false);

/**
 * 开关三态（2026-10-10 Andy 定稿）：关闭 / 开启 / 正在操作；本机不可用时置灰。
 * 状态提示不再单列——进行中由开关自身的置灰 + 转圈表达，不配文字。
 */
const captureSwitchState = computed(() =>
  captureSwitchStateFn({
    enabled: rc003CaptureEnabled.value,
    busy: rc003CaptureBusy.value,
    unsupported: captureUnsupported.value,
  }),
);
/**
 * 失败文案（唯一允许出现在开关旁的文字）：操作失败显示映射后的原因；桥接段
 * 失败（开启后助手没起来）显示固定一句。其余情况为 null —— 一个字都不显示。
 */
const captureFailure = computed(() => {
  if (rc003BridgeFailed.value) return captureBridgeFailedText.value;
  const raw = rc003CaptureFailureRaw.value;
  if (!raw) return null;
  if (effectiveLanguage.value === "zh-Hans") {
    return captureFailureText(raw, CAPTURE_FAILURE_NAME);
  }
  // 英文界面只给固定一句，原始错误留在诊断日志里。
  return t("全按键支持没有切换成功。请关闭后重新开启；仍不行就先断开遥控器再重新配对。");
});

// 开关操作的原始错误只进日志/诊断，不直接显示（中文界面由 captureFailureText 映射）。
watch(rc003CaptureFailureRaw, (raw) => {
  if (raw) console.warn("rc003 capture toggle failed", raw);
});
/**
 * 「开着但没生效」的提示（2026-10-13 定稿）：后端说 `inactive` 才出现，且**只在
 * 遥控器链路正常时**出现——链路本身断掉时由头部连接状态与连接页既有文案说明，
 * 这里不重复、也不把用户往遥控器侧引（这条链断在 PC 内部，现场已证伪按遥控器有用）。
 */
const captureInactiveNote = computed(() =>
  shouldShowCaptureInactive({
    switchState: captureSwitchState.value,
    health: rc003CaptureHealth.value,
    linkReady: connectionReady.value,
  })
    ? captureInactiveText(CAPTURE_FAILURE_NAME, rc003CaptureUnavailable.value)
    : null,
);
/**
 * 「开关被应用自己关掉了」的解释（2026-10-15，issue #264 附加观察甲）：升级后对账
 * 会因系统授权需要重新确认而回落开关，界面必须说明原因，否则用户只看到"功能变差了"。
 * 与失败句、失效句同一位，优先级：操作失败 > 被关掉 > 开着没生效。
 */
const captureRevokedNote = computed(() =>
  shouldShowCaptureRevoked({
    switchState: captureSwitchState.value,
    revoked: rc003CaptureRevoked.value,
  })
    ? effectiveLanguage.value === "zh-Hans"
      ? captureRevokedText(CAPTURE_FAILURE_NAME)
      : t("全按键支持因为系统授权需要重新确认，已经被关掉了；重新打开一次就能继续使用。")
    : null,
);
/** 三键卡片/格子的置灰提示：本机开不了时不说「开启后恢复正常」。 */

/**
 * 返回/音量± 在「全按键支持」关闭时不可用：卡片置灰禁用并给悬停提示
 * （2026-10-03 Andy 定稿：置灰禁用 +「…需要开启全按键支持才能使用，开启后恢复正常」）。
 * 状态未就绪（null）按关闭态处理——与开关悬停提示、能力说明同一口径；开启后
 * 卡片与命中路径立即恢复（响应式，无需重进页面）。
 * 注意：不能只靠模板的 disabled 属性——合成事件会绕过原生 disabled，动作函数
 * （selectButton/openEditor）必须同样自守。
 *
 * 2026-10-07 增补：本机不可用（ARM64）时无条件置灰——开关这时也点不动，
 * 「开启后恢复正常」不成立（issue #206）。
 */
function captureGated(button: RemoteButton): boolean {
  const isThreeKey =
    button === "back" || button === "volume_up" || button === "volume_down";
  if (!isThreeKey) return false;
  if (captureUnsupported.value) return true;
  return rc003CaptureEnabled.value !== true;
}

// 开启成功但桥接段异步失败（助手起不来等）：开关已是开启态、不会再走
// applyCaptureToggle 的失败分支，必须在这里把失败变成开关下方的文案，
// 否则用户只看到一个不再变化的开关（2026-10-10：状态圆点已并入开关）。
watch(rc003BridgePhase, (phase, previous) => {
  if (phase === "connected") {
    rc003BridgeFailed.value = false;
    return;
  }
  if (phase === "failed" && previous !== "failed") {
    rc003BridgeFailed.value = true;
    reportFrontendEvent({
      event: "rc003_capture_bridge_failed",
      phase: "completed",
      result: "failed",
      reason: reasonToken("bridge_phase", phase),
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
  // 本机支持情况单独取、单独兜（2026-10-07 issue #206）：读不到就保持上一次的
  // 值，不因为一次 IPC 失败把开关锁成不可用。开销是一次内存读，随既有轮询刷新。
  const support = await getCaptureSupport().catch(() => null);
  if (support) captureSupport.value = support;
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
      rc003CaptureHealth.value = bridge.captureHealth;
      rc003CaptureUnavailable.value = normalizeCaptureUnavailable(
        bridge.captureUnavailable,
      );
      rc003CaptureRevoked.value = normalizeCaptureRevoked(bridge.captureRevoked);
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
  // 本机不可用（ARM64，issue #206）：开关已置灰，这里再守一道——合成事件与
  // 键盘操作都可能绕过原生 disabled。不弹确认、不动 IPC，把 DOM 写回绑定值。
  if (captureUnsupported.value) {
    syncCaptureSwitchDom();
    return;
  }
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
  // 新的操作开始：清掉上一次的失败文案（含桥接段失败）——正在操作不配文字，
  // 旧失败原因也不允许残留（2026-10-10 定稿）。
  rc003CaptureFailureRaw.value = null;
  rc003BridgeFailed.value = false;
  const wasEnabled = rc003CaptureEnabled.value;
  try {
    const next = wasEnabled ? await disableRc003Capture() : await enableRc003Capture();
    rc003Task.value = next;
    if (next.lastError) {
      // 操作失败（UAC 取消 / 停止被拒）：**保持开关原状态**。
      // 实际系统状态没变，开关却翻过去，就是又一次"证据说谎"。
      rc003CaptureFailureRaw.value = next.lastError;
      return;
    }
    // 成功后的反馈由开关自身承担（2026-10-10）：不再重复一条一次性的话；
    // 失败才写文案（见上）。
    rc003CaptureEnabled.value = next.enabled;
    reportFrontendEvent({
      event: "rc003_capture_toggle",
      phase: "completed",
      result: "passed",
      reason: reasonToken("enabled", next.enabled),
    });
  } catch (error) {
    rc003CaptureFailureRaw.value = error instanceof Error ? error.message : String(error);
    // **同步回退到点击前的状态**——不依赖"我没动过它"，也不等下一次异步
    // 对账：原生复选框在 click 时 DOM 已经先翻了，只有显式写回才确定。
    rc003CaptureEnabled.value = wasEnabled === true;
    reportFrontendEvent({
      event: "rc003_capture_toggle",
      phase: "completed",
      result: "failed",
      reason: reasonToken("was_enabled", wasEnabled === true, "switch_reverted"),
    });
    // 再用权威状态（设置里的意图）校正一次，防止本地值与系统状态漂移。
    void getRc003TaskStatus()
      .then((status) => {
        rc003CaptureEnabled.value = status.enabled;
        reportFrontendEvent({
          event: "rc003_capture_toggle",
          phase: "reconciled",
          result: "unknown",
          reason: reasonToken("authoritative_enabled", status.enabled),
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
  // 首帧只等三项**必需**数据：映射、快照、锁定偏好——它们决定那两个开关的初值与
  // 配置内容的首次渲染。其余三项（应用候选、常用语入口、组合动作芯片）不影响首帧
  // 布局，之前却挤在同一条链上：`listPresetApps` 实测 379 ms（要探测已安装应用），
  // 而 `getCombinedActions` 还额外 `await` 在映射赋值之前——于是"开关和配置内容都要
  // 等上一两秒"（2026-10-28 用户反馈）。现在它们就绪后各自补齐。
  const [loaded, snapshot, persistedLockSelection] = await Promise.all([
    getButtonMappings(),
    getButtonMappingSnapshot(),
    // 「锁定当前按键」偏好：读取失败回落默认（开启）并留一条前端事件，不阻断页面。
    getLockMappingSelection().catch(() => {
      reportFrontendEvent({
        event: "mapping_selection_lock",
        phase: "completed",
        result: "failed",
        reason: "load_failed",
      });
      return true;
    }),
  ]);
  if (unmounted) {
    return;
  }
  mappings.value = loaded;
  savedSnapshot.value = JSON.parse(JSON.stringify(loaded)) as ButtonMappings;
  mappingSnapshot.value = snapshot;
  // 两个开关的「就绪」与真值在同一渲染批次生效：本批次才创建开关本体，创建即带
  // 正确 checked，不产生属性变更，因此没有"状态回来后开→关"的滑动过渡。
  mappingsLoaded.value = true;
  lockSelectionPersisted = persistedLockSelection;
  lockSelection.value = persistedLockSelection;
  lockSelectionReady.value = true;
  // 权威值回写缓存：下次首帧直接用它渲染，不必再等 IPC。
  writeSwitchCache(loaded.enabled, persistedLockSelection);

  // 以下三项都不阻塞首帧，就绪后各自补齐（页面已卸载则丢弃）。
  void listPresetApps()
    .then((apps) => {
      if (unmounted) return;
      presetApps.value = apps.filter((app) => app.installed);
      registerPresetAppNames(presetApps.value);
    })
    .catch(() => undefined);
  // 常用语组件可用性：隐藏入口与动作摘要（「当前不可用」标注）共用同一判据。
  void getCommonPhrasePanelSnapshot()
    .then((commonPhrases) => {
      if (unmounted) return;
      commonPhrasesAvailable.value = commonPhrases.available;
      commonPhraseAccess.value = commonPhrases.access;
      registerCommonPhraseAvailability(commonPhrases.available);
    })
    .catch(() => undefined);
  // 组合动作候选：失败不影响按键页其余功能（入口显示"还没有组合动作"）。
  void getCombinedActions()
    .then((library) => {
      if (unmounted) return;
      // 同 id 多版本只取最新（编辑一次 = 版本追加，直接铺开会多出芯片）。
      macroDefinitions.value = latestMacroDefinitions(library.definitions);
      registerMacroNames(macroDefinitions.value);
    })
    .catch(() => undefined);

  if (rawInput.value?.activeButtons) {
    activeButtons.value = new Set(rawInput.value.activeButtons);
  }

  const stopEdges = await subscribeButtonEdges((edge: ButtonEdge) => {
    const arrival = performance.now();
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
    if (!edge.isPressed) endFiredFlash(edge.button);
    // 高亮时序取证（2026-10-13 现场反馈"有时快、有时有延迟、卡片和线条不一起亮"）：
    // 每次按下/松开各落一行，elapsed_ms = "收到边沿 → 下一帧"。与同一时刻的引擎
    // 日志 `map_edges` 对齐，可把延迟拆成 设备→引擎 / 引擎→前端 / 前端出画面 三段，
    // 不必再靠反复试错定位；字段全部是稳定 token（detail = 语义键名）。
    const reportEdge = () =>
      reportFrontendEvent({
        event: "button_edge",
        phase: "completed",
        result: "passed",
        reason: edge.isPressed ? "pressed" : "released",
        detail: edge.button,
        elapsedMs: Math.max(0, Math.round(performance.now() - arrival)),
      });
    if (typeof requestAnimationFrame === "function") {
      requestAnimationFrame(reportEdge);
    } else {
      window.setTimeout(reportEdge, 0);
    }
  });
  if (unmounted) {
    stopEdges();
    return;
  }
  unlistenEdges = stopEdges;

  // 动作触发反馈（闪烁）：起点 = 手势触发，终点 = **松开沿**（或 600ms 兜底）。
  // 现场（2026-10-13）：配了映射的键"不能同灭"——固定 600ms 的闪烁比按下高亮多亮
  // 约 400ms，看起来像卡片没跟着连线熄灭。闪烁现在跟随这一次按压，只有极短按
  // （<150ms）才补足到最短可见时间；按住连发每拍重新起算。
  const stopGestures = await subscribeButtonGestures((gesture: FiredGesture) => {
    lastFired.value = gesture;
    firedFlash.value = { button: gesture.button, trigger: gesture.trigger };
    flashStartedAt = performance.now();
    if (flashTimer !== null) window.clearTimeout(flashTimer);
    flashTimer = window.setTimeout(() => {
      firedFlash.value = null;
      flashTimer = null;
    }, FLASH_FALLBACK_MS);
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
  <!-- 「调整常用语」二级视图（对齐 macOS：从按键页进入，返回按键页）。 -->
  <section v-if="showCommonPhraseEditor && commonPhraseEditor" class="buttons-page">
    <header class="page-header">
      <div class="mapping-title-row">
        <button type="button" class="chip" @click="showCommonPhraseEditor = false">{{ t("返回") }}</button>
        <h1>{{ t("调整常用语") }}</h1>
      </div>
    </header>
    <component :is="commonPhraseEditor" />
  </section>
  <section v-else class="buttons-page">
    <!-- 头部对齐 Mac mappingPage：标题 + 启用开关相邻居左，遥控器状态最右
         （保存按钮移入编辑面板，与"测试一次/关闭"同排）。 -->
    <header class="page-header mapping-header">
      <div>
        <div class="mapping-title-row">
          <h1>{{ tPage("按键映射") }}</h1>
          <label class="toggle-row" :title="t('开启后，遥控器按键按本页配置执行动作；关闭时，遥控器保持原始按键行为。')">
            <span>{{ t("启用自定义按键功能") }}</span>
            <!-- 始终渲染：值先取上次已知值（localStorage），IPC 回来后以权威值覆盖。
                 数据未到之前禁用，避免用户用默认值触发一次保存、把真实设置覆盖掉。 -->
            <input
              v-model="enabled"
              type="checkbox"
              class="toggle-input"
              :disabled="busy || !mappingsLoaded"
            />
          </label>
          <!-- 全按键支持开关常驻页面头部（2026-09-27 Andy 要求）：不依赖
               选中某个按键的编辑面板，任何时刻都能开启/关闭。
               样式与「启用自定义按键功能」一致（toggle-input Switch，2026-09-28）；
               桥接状态是开关右侧的行内圆点——不能再用独立状态行：v-if 插行会把
               下方画布整体顶下去（页面抖动），胶囊底色+状态光晕也把标题区染了色
               （2026-09-28 Andy 报告）。 -->
          <!-- 悬停提示随开关状态切换（2026-09-28 Andy 定稿）：关闭态指向
               「开启后支持使用…」，开启态指向「关闭后…将不可映射」；本机不可用
               （ARM64，issue #206）时固定为不可用口径。 -->
          <label
            class="toggle-row"
            :title="captureSwitchTitle"
          >
            <span>{{ t("全按键支持") }}</span>
            <!-- 开关 + 状态提示合并成一个开关（2026-10-10 Andy 定稿，见
                 docs/product-copy.md §4）：只有关闭 / 开启 / 正在操作三态，
                 进行中=置灰 + 转圈且不配文字；只有失败才在标题下方显示一句原因。
                 终值就绪前用同尺寸占位符顶位、就绪后才创建开关本体——与
                 「启动行为」页登录自启动开关同法（2026-09-28 Andy 要求）：
                 开关创建即带正确 checked，不产生"状态回来后关→开"的滑动动画。 -->
            <span class="toggle-switch-slot">
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
                :disabled="rc003CaptureBusy || captureUnsupported"
                @change="toggleRc003Capture"
              />
              <span
                v-if="captureSwitchState === 'busy'"
                class="toggle-spinner"
                aria-hidden="true"
              ></span>
            </span>
          </label>
          <!-- 本机不可用（ARM64，issue #206）：开关旁一句短说明，说结果不说机制。 -->
          <span v-if="captureUnsupported" class="capture-unavailable-note">
            {{ captureUnsupportedNote }}
          </span>
        </div>
        <!-- 标题下方一行：平时是页面副标题；只有开关操作失败/启动失败、或
             「开着却没生效」时改说原因（2026-10-10 定稿 + 2026-10-13 定稿：
             其余情况不显示任何状态文案）。位置不动、不插行，沿用 2026-09-28 的防抖动约定。 -->
        <p
          class="page-subtitle"
          :class="{
            'is-failure': captureFailure || captureRevokedNote || captureInactiveNote,
          }"
        >
          {{
            captureFailure ??
            captureRevokedNote ??
            captureInactiveNote ??
            t("点击按键进行自定义配置")
          }}
        </p>
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

      <figure class="remote-photo" :style="{ left: `${remoteLeft}px`, top: `${REMOTE_TOP}px` }">
        <img src="/RC003-remote-photo@2x.webp" :alt="t('小米蓝牙语音遥控器 2 Pro 示意图')" draggable="false" />
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
        :title="captureGated(placement.button) ? triKeyGatedTitle : undefined"
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
                ? triKeyGatedTitle
                : actionCellTitle(placement.button, trigger)
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
          <strong>{{ t("语音键") }}</strong>
          <span class="badge pending voice-badge" :class="{ active: voiceActive }">{{ t("按住说话") }}</span>
        </div>
        <p class="voice-note">{{ t("按下开始、松开结束；不参与自定义映射，不加双击/长按延迟。") }}</p>
      </article>
    </div>

    <article v-if="editingTarget" ref="editorPanel" class="card mapping-editor">
      <div class="card-title-row">
        <div>
          <h2>{{ buttonLabel(editingTarget.button) }} · {{ buttonTriggerLabel(editingTarget.trigger) }}</h2>
          <p class="muted">{{ t("当前：{summary}", { summary: actionSummary(actionOf(editingTarget.button, editingTarget.trigger)) }) }}</p>
        </div>
        <div class="button-row">
          <!-- 全按键支持开关已移至页面头部（与「启用自定义按键功能」并排，
               2026-09-27），编辑面板只保留按键级操作。 -->
          <button
            class="secondary-button editor-disable-btn"
            :class="{ 'is-active': actionOf(editingTarget.button, editingTarget.trigger).type === 'disabled' }"
            type="button"
            :disabled="busy"
            :title="t('只禁用当前格子的映射，此按键恢复原始行为')"
            @click="applyAction({ type: 'disabled' })"
          >
            {{ t("禁用按键") }}
          </button>
          <button class="secondary-button" type="button" @click="editingTarget = null">{{ t("关闭") }}</button>
        </div>
      </div>
      <div class="action-sections">
        <p v-if="capabilityNote" class="muted editor-note capability-note">{{ capabilityNote }}</p>
        <section v-for="group in SHORTCUT_PRESET_GROUPS" :key="group.label" class="action-section">
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
              {{ preset.display ?? chordLabel({ keys: preset.keys }) }}
            </button>
          </div>
        </section>

        <!-- 自定义动作放在系统与媒体下方（2026-10-22 用户指定顺序）；
             原「设备操作 / 聚焦输入框」并入本组的录制按钮右侧。 -->
        <section class="action-section">
          <h4 class="action-section-title">{{ t("自定义动作") }}</h4>
          <div class="custom-shortcut-row">
            <button
              class="chip"
              :class="{ selected: capturingShortcut }"
              type="button"
              :disabled="captureStarting"
              @click="capturingShortcut ? finishShortcutCapture(t('已取消录入')) : beginShortcutCapture()"
            >
              {{ capturingShortcut ? t("录入中…（按 Esc 取消）") : t("录入自定义快捷键") }}
            </button>
            <button
              class="chip"
              :class="{ selected: selectedAction?.type === 'focus_input' }"
              type="button"
              :title="t('把键盘焦点放到当前前台应用的输入框（不切换应用、不输入文字）')"
              @click="applyAction({ type: 'focus_input' })"
            >
              {{ t("聚焦输入框") }}
            </button>
            <span v-if="capturingShortcut" class="capture-display">
              {{ captureDisplay.length ? chordLabel({ keys: captureDisplay }) : (safeCaptureMode ? t("先选择修饰键") : t("请按下快捷键组合")) }}
            </span>
          </div>
          <label
            class="toggle-row safe-capture-toggle"
            :title="t('开启后，通过界面选择修饰键，键盘只需按主键。')"
          >
            <span>{{ t("安全录入模式") }}</span>
            <input
              v-model="safeCaptureMode"
              type="checkbox"
              class="toggle-input"
              :disabled="capturingShortcut || captureStarting"
            />
            <small class="muted safe-capture-hint">
              {{ t("直接录入无法完成或会触发系统动作时再开启。") }}
            </small>
          </label>
          <template v-if="capturingShortcut && safeCaptureMode">
            <p class="muted editor-note capture-guide">
              {{ t("请用鼠标选择修饰键，再只按一次主键。不要在键盘上按完整组合，系统快捷键不会被执行。") }}
            </p>
            <div class="preset-grid capture-modifiers">
              <button
                v-for="modifier in captureModifierOptions"
                :key="modifier.key"
                class="chip"
                :class="{ selected: selectedCaptureModifiers.has(modifier.key) }"
                type="button"
                @click="toggleCaptureModifier(modifier.key)"
              >
                {{ modifier.label }}
              </button>
            </div>
            <p class="capture-display">{{ t("然后单独按主键（例如选择“左 Win”后，只按 L）") }}</p>
          </template>
        </section>

        <section class="action-section">
          <h4 class="action-section-title">{{ t("打开应用") }}</h4>
          <input v-if="customApps.length > 12" v-model="appFilter" class="app-library-search" type="search" :aria-label="t('筛选已添加应用')" :placeholder="t('筛选已添加应用')" />
          <!-- 预设应用与已添加应用共用一个换行网格（2026-10-03 用户要求）：
               两组名字连续排布，不再在中间强制换行；长名字的换行规则见 .chip.custom-app。 -->
          <div class="preset-grid app-library">
            <button
              v-for="app in presetApps"
              :key="app.id"
              class="chip"
              :class="{ selected: openAppTargetOf(editingTarget.button, editingTarget.trigger) === app.id }"
              type="button"
              :title="t('已运行则切到该应用窗口，未运行则启动')"
              @click="applyAction({ type: 'open_app', target: app.id })"
            >
              {{ openAppLabel(app.name) }}
            </button>
            <button v-for="app in filteredCustomApps" :key="app.path" class="chip custom-app" type="button"
              :class="{ selected: openAppTargetOf(editingTarget.button, editingTarget.trigger) === app.path }"
              :title="app.name" @click="applyAction({ type: 'open_app', target: app.path })">{{ openAppLabel(app.name) }}</button>
            <!-- 扫描 / 添加入口紧跟在所有应用之后、同一网格内换行（2026-10-20 用户要求：不要单独占一行）。 -->
            <button class="chip" type="button" @click="appPickerError = null; appPickerOpen = true">{{ t("扫描本机应用") }}</button>
            <button
              class="chip add-app"
              type="button"
              :title="t('从本机选择任意程序或快捷方式')"
              @click="addCustomApp"
            >
              {{ t("＋ 添加应用") }}
            </button>
          </div>
          <div v-if="showAppFocusStrategy && focusTarget" class="focus-profile">
            <p class="action-section-title focus-profile-title">{{ t("聚焦方式") }}</p>
            <div class="preset-grid">
              <button
                v-for="option in focusStrategyOptions"
                :key="option.value"
                class="chip focus-strategy-option"
                :class="{
                  selected: (focusProfile?.strategy ?? 'open_only') === option.value,
                  'focus-point-chip': option.value === 'recorded_point',
                }"
                type="button"
                :aria-pressed="(focusProfile?.strategy ?? 'open_only') === option.value"
                :disabled="focusBusy"
                @click="applyFocusStrategy(option.value)"
              >
                {{ option.label }}
              </button>
              <button
                v-if="hasPresetFocusOverride"
                class="focus-reset-link"
                type="button"
                :disabled="focusBusy"
                @click="resetPresetFocus"
              >
                {{ t("恢复默认") }}
              </button>
            </div>
            <template v-if="focusProfile?.strategy === 'app_shortcut'">
              <p class="muted focus-learn-hint">
                {{ t("先点击“录入快捷键”，再按下该 APP 用于聚焦输入框的单个按键或组合键。执行时会先激活 APP，再发送此按键。") }}
              </p>
              <div class="focus-shortcut-row">
                <span class="focus-shortcut-value" :class="{ empty: !focusProfile?.shortcut?.keys?.length }">
                  <svg class="focus-shortcut-keyboard" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                    <rect x="2.5" y="6" width="19" height="12" rx="2.5" />
                    <path d="M6 10h.01M9 10h.01M12 10h.01M15 10h.01M18 10h.01M6 13h.01M9 13h.01M12 13h.01M15 13h.01M18 13h.01M7 16h10" />
                  </svg>
                  {{
                    focusProfile?.shortcut?.keys?.length
                      ? chordLabel(focusProfile.shortcut)
                      : t("尚未录入")
                  }}
                </span>
                <button
                  class="chip"
                  type="button"
                  :disabled="captureStarting"
                  @click="
                    capturingShortcut && capturePurpose === 'focus_shortcut'
                      ? finishShortcutCapture(t('已取消录入'))
                      : beginFocusShortcutCapture()
                  "
                >
                  {{
                    capturingShortcut && capturePurpose === "focus_shortcut"
                      ? t("录入中…（按 Esc 取消）")
                      : t("点击录入快捷键")
                  }}
                </button>
                <button
                  class="chip"
                  type="button"
                  :disabled="!focusProfile?.shortcut?.keys?.length || capturingShortcut"
                  @click="clearFocusShortcut"
                >
                  {{ t("清除") }}
                </button>
              </div>
            </template>
            <template v-if="focusProfile?.strategy === 'recorded_element'">
              <p class="muted focus-learn-hint">
                {{ t("点击后会立即打开目标 APP。请在 6 秒内点击需要聚焦的输入框；无线麦 SayAll 只记录辅助功能特征，不读取输入内容。") }}
              </p>
              <div class="focus-learn-row">
                <button
                  class="chip"
                  type="button"
                  :disabled="focusLearnBusy"
                  @click="startFocusLearning"
                >
                  {{
                    focusLearnState === "opening"
                      ? t("正在打开应用…")
                      : focusLearnState === "learning"
                        ? t("等待点击输入框…")
                        : t("开始学习输入框")
                  }}
                </button>
                <span
                  class="focus-record-status"
                  :class="focusProfile?.recorded ? 'is-done' : 'is-pending'"
                >
                  {{ focusProfile?.recorded ? t("已记录目标输入框") : t("尚未记录目标输入框") }}
                </span>
                <button
                  v-if="focusProfile?.recorded"
                  class="focus-link-button"
                  type="button"
                  :disabled="focusLearnBusy"
                  @click="clearRecordedFocus"
                >
                  {{ t("清除") }}
                </button>
              </div>
              <p v-if="focusLearnMessage" class="focus-learn-message" :class="`is-${focusLearnState}`">
                {{ focusLearnMessage }}
              </p>
            </template>
            <template v-if="focusProfile?.strategy === 'recorded_point'">
              <p class="muted focus-learn-hint">
                {{ t("「记录点击位置」会先打开目标应用；把鼠标停在要点击的位置上，6 秒后自动记录，之后按这个位置点击聚焦。") }}
              </p>
              <div class="focus-learn-row">
                <button
                  class="chip"
                  type="button"
                  :disabled="pointLearnState === 'opening' || pointLearnState === 'recording'"
                  @click="recordFocusPoint"
                >
                  {{
                    pointLearnState === "opening"
                      ? t("正在打开应用…")
                      : pointLearnState === "recording"
                        ? t("等待鼠标停留…")
                        : t("开始记录位置")
                  }}
                </button>
                <button
                  v-if="focusProfile?.point"
                  class="chip"
                  type="button"
                  :disabled="pointLearnState === 'opening' || pointLearnState === 'recording'"
                  @click="clearRecordedPoint"
                >
                  {{ t("清除记录") }}
                </button>
                <span
                  class="focus-record-status"
                  :class="focusProfile?.point ? 'is-done' : 'is-pending'"
                >
                  {{ focusProfile?.point ? t("已记录点击位置") : t("尚未记录点击位置") }}
                </span>
              </div>
              <p v-if="pointLearnMessage" class="focus-learn-message" :class="`is-${pointLearnState}`">
                {{ pointLearnMessage }}
              </p>
            </template>
            <p v-if="lastFocusNotice" class="muted focus-result-message">{{ lastFocusNotice }}</p>
          </div>
        </section>

        <!-- 组合动作（自定义动作；2026-10-15）：把已有动作绑定到本按键。
             入口显隐跟随 COMBINATION_ACTIONS_ENABLED，隐藏时已绑定动作照常执行。 -->
        <section v-if="COMBINATION_ACTIONS_ENABLED" class="action-section">
          <h4 class="action-section-title">{{ t("组合动作") }}</h4>
          <p v-if="macroDefinitions.length === 0" class="muted">
            {{ t("还没有组合动作。先到“组合动作”页创建。") }}
          </p>
          <div class="preset-grid">
            <button
              v-for="macro in macroDefinitions"
              :key="macro.macroId"
              class="chip custom-app"
              :class="{
                selected:
                  selectedAction?.type === 'run_macro' &&
                  selectedAction.macroId === macro.macroId,
              }"
              type="button"
              :title="t('{count} 个步骤', { count: macro.steps.length })"
              @click="
                applyAction({
                  type: 'run_macro',
                  macroId: macro.macroId,
                  macroVersion: macro.version,
                })
              "
            >
              {{ macro.name }}
            </button>
            <!-- 与「扫描本机应用」同款 chip（2026-10-22 用户要求）：跳到组合动作页管理。 -->
            <button
              class="chip"
              type="button"
              :title="t('打开组合动作页，创建、编辑或删除组合动作')"
              @click="emit('navigate', 'macros')"
            >
              {{ t("管理组合动作") }}
            </button>
          </div>
        </section>

        <section v-if="commonPhrasesAvailable" class="action-section">
          <h4 class="action-section-title">{{ t("常用语") }}</h4>
          <div class="preset-grid">
            <button
              class="chip"
              :class="{ selected: selectedAction?.type === 'open_common_phrases' }"
              type="button"
              :disabled="!commonPhraseAllowed"
              :title="
                commonPhraseAllowed
                  ? t('打开常用语面板，按方向键和 OK 键插入对应常用语')
                  : t('登录会员后可使用常用语')
              "
              @click="applyAction({ type: 'open_common_phrases' })"
            >
              <svg
                v-if="!commonPhraseAllowed"
                class="chip-icon common-phrase-circle"
                viewBox="0 0 24 24"
                fill="none"
                aria-hidden="true"
              >
                <circle cx="12" cy="12" r="8" stroke="currentColor" stroke-width="1.8" />
              </svg>
              {{ t("打开常用语") }}
              <span v-if="commonPhraseAllowed" class="member-badge">{{ t("会员") }}</span>
            </button>
            <MembershipRequiredButton
              v-if="!commonPhraseAllowed"
              class="common-phrases-cta"
              @open="emit('navigate', 'membership')"
            />
            <button
              class="chip"
              type="button"
              :title="t('编辑常用语内容与五键分配')"
              @click="showCommonPhraseEditor = true"
            >
              <svg class="chip-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <g stroke="currentColor" stroke-width="1.9" stroke-linecap="round">
                  <path d="M4.5 6.5h15" />
                  <path d="M4.5 12h15" />
                  <path d="M4.5 17.5h15" />
                </g>
                <g fill="var(--surface-chip, #fff)" stroke="currentColor" stroke-width="1.7">
                  <circle cx="9.5" cy="6.5" r="1.9" />
                  <circle cx="14.5" cy="12" r="1.9" />
                  <circle cx="10.5" cy="17.5" r="1.9" />
                </g>
              </svg>
              {{ t("调整常用语") }}
            </button>
            <button
              class="chip"
              type="button"
              :title="t('预览常用语面板（未开通会员时为只读示例）')"
              @click="openExample"
            >
              <svg class="chip-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <g fill="currentColor">
                  <path
                    d="M4.4 6.2h3.4l-1.5 4.3c2 .5 3.2 2 3.2 4 0 2.4-1.7 4-4.1 4s-4-1.6-4-4c0-.5.1-1 .3-1.6l2.7-6.7z"
                  />
                  <path
                    d="M14.4 6.2h3.4l-1.5 4.3c2 .5 3.2 2 3.2 4 0 2.4-1.7 4-4.1 4s-4-1.6-4-4c0-.5.1-1 .3-1.6l2.7-6.7z"
                  />
                </g>
              </svg>
              {{ t("常用语示例") }}
            </button>
          </div>
        </section>

      </div>

      <!-- 鼠标动作固定为编辑区最后一块（2026-10-22 用户指定顺序）：
           单击 → 四方向移动 → 滚轮上下 → 数量输入框。 -->
      <div class="action-sections mouse-action-sections">
        <section class="action-section">
          <h4 class="action-section-title">{{ t("鼠标动作") }}</h4>
          <!-- 两个可换行的组：空间够时并排成一行；不够时整组换行，
               正好是“点击一行、移动与滚轮合并为下一行”。 -->
          <div class="mouse-action-row">
            <div class="mouse-action-group">
              <button v-for="(label, kind) in mouseClickLabels" :key="kind" class="chip" type="button"
                :class="{ selected: selectedAction?.type === 'mouse_click' && selectedAction.kind === kind }"
                :title="t('点击鼠标当前位置')" @click="applyAction({ type: 'mouse_click', kind })">{{ label }}</button>
            </div>
            <div class="mouse-action-group">
              <button v-for="(label, direction) in mouseMoveLabels" :key="direction" class="chip" type="button"
                :aria-label="label" :title="label" :class="{ selected: selectedAction?.type === 'mouse_move' && selectedAction.direction === direction }"
                @click="applyAction({ type: 'mouse_move', direction, distance: moveDistance })">{{ label }}</button>
              <button v-for="direction in (['up', 'down'] as const)" :key="direction" class="chip"
                :class="{ selected: isActiveScroll(direction) }" type="button" :title="t('在鼠标当前位置滚动')"
                @click="applyAction({ type: 'scroll', direction, steps: scrollSteps })">{{ direction === "up" ? t("滚轮向上") : t("滚轮向下") }}</button>
            </div>
          </div>
          <label v-if="mouseAmountVisible" class="mouse-amount">
            <span>{{ mouseAmountKind === "mouse_move" ? t("每次移动") : t("每次滚动") }}</span>
            <input v-model="mouseAmountDraft"
              :aria-label="mouseAmountKind === 'mouse_move' ? t('每次移动像素') : t('每次滚动格数')"
              type="text" inputmode="numeric" autocomplete="off"
              @focus="clearMouseAmountError"
              @click="clearMouseAmountError"
              @input="clearMouseAmountError"
              @keydown.enter="updateMouseAmount()"
              @blur="updateMouseAmount()" />
            <span>{{ mouseAmountKind === "mouse_move" ? t("像素") : t("格") }}</span>
            <span v-if="mouseAmountError" class="mouse-amount-error">{{ mouseAmountError }}</span>
            <span v-else-if="mouseAmountKind === 'mouse_move'" class="mouse-amount-hint">{{ MOUSE_AMOUNT_HINT }}</span>
          </label>
        </section>
      </div>
      <div v-if="repeatGroupVisible" class="action-sections repeat-action-sections">
        <section class="action-section repeat-group">
          <h4 class="action-section-title">{{ t("连续执行") }}</h4>
          <div class="repeat-options">
            <button
              class="chip"
              :class="{ selected: repeatChoice === 'off' }"
              type="button"
              :title="t('按住不放只执行一次')"
              :disabled="busy"
              @click="selectHoldRepeat('off')"
            >
              {{ repeatOffLabel }}
            </button>
            <button
              class="chip"
              :class="{ selected: repeatChoice === 'single' }"
              type="button"
              :title="repeatSingleTitle"
              :disabled="busy || (!!repeatSingleReason && repeatChoice !== 'single')"
              @click="selectHoldRepeat('single')"
            >
              {{ t("重复单击动作") }}
            </button>
            <button
              class="chip"
              :class="{ selected: repeatChoice === 'long' }"
              type="button"
              :title="repeatLongTitle"
              :disabled="busy || (!!repeatLongReason && repeatChoice !== 'long')"
              @click="selectHoldRepeat('long')"
            >
              {{ t("重复长按动作") }}
            </button>
          </div>
          <p v-if="repeatPageReason" class="muted repeat-hint">{{ repeatPageReason }}</p>
        </section>
      </div>
      <label v-if="okClickVisible" class="toggle-row ok-click-toggle" :title="t('刚用遥控器移动过光标时，按 OK 会点击光标位置')">
        <span>{{ t("移动光标后按 OK 点击") }}</span>
        <input
          class="toggle-input"
          type="checkbox"
          :checked="okClickChecked"
          :disabled="busy"
          @change="toggleOkContextClick"
        />
      </label>
      <p v-if="okClickVisible" class="muted ok-click-hint">
        {{ t("刚用遥控器移动过光标（5 秒内），按 OK 会点击光标位置。这时 OK 的单击、双击、长按都不会执行。") }}
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
          {{ rawInput?.phase === "ready" ? t("停止监听") : t("启动监听") }}
        </button>
        <small v-if="mappingSnapshot && !mappings.enabled" class="muted">{{ t(" · 总开关关闭（按键保持原样）") }}</small>
      </div>
      <label class="toggle-row" :title="t('开启后，操作实体遥控器不会切换正在编辑的按键。')">
        <span>{{ t("锁定当前按键") }}</span>
        <!-- 同「启用自定义按键功能」：始终渲染、先用上次已知值，数据未到前禁用。 -->
        <input
          v-model="lockSelection"
          type="checkbox"
          class="toggle-input"
          :disabled="!lockSelectionReady"
        />
      </label>
      <span class="muted lock-hint">{{ t("按遥控器时保持当前编辑项") }}</span>
      <div class="button-row">
        <button class="secondary-button" type="button" :disabled="busy" @click="saveConfiguration">
          {{ t("保存配置") }}
        </button>
        <button class="secondary-button" type="button" :disabled="busy" @click="importConfiguration">
          {{ t("导入配置…") }}
        </button>
        <button class="secondary-button" type="button" :disabled="busy" @click="exportConfiguration">
          {{ t("导出配置…") }}
        </button>
        <button class="secondary-button" type="button" :disabled="busy" @click="restoreDefaults">
          {{ t("恢复默认") }}
        </button>
      </div>
    </footer>

    <p v-if="statusMessage" class="operation-message mapping-status">{{ statusMessage }}</p>
    <p v-if="mappingSnapshot?.lastError" class="error-text">{{ t("按键监听遇到问题，请到连接页重新连接遥控器后再试。") }}</p>
    <RegisteredAppsDialog v-if="appPickerOpen" :known-apps="mappings.applications ?? []" :saving="busy" :save-error="appPickerError" @close="appPickerOpen = false" @add="addScannedApps" />
    <EnhancedCaptureConfirmDialog v-if="showCaptureConfirm" @confirm="confirmCaptureDialog" @close="closeCaptureDialog" />
  </section>
</template>

<style scoped>
/* 全按键支持开关的终值就绪前占位符：与 toggle-input 同尺寸（34x20），
   避免就绪后开关创建时标题行宽度跳动（同「启动行为」页做法）。 */
.toggle-placeholder { width: 34px; height: 20px; flex: none; }
/* 「连续执行」：编辑面板内的按键级 radio 三选一（2026-10-05），
   2026-10-23 起独立成组、放在鼠标动作下面。 */
.repeat-options { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 6px; }
.repeat-hint { margin: 6px 0 0; font-size: 12px; }
/* OK 键「移动光标后按 OK 点击」：按键级开关 + 常驻说明句（2026-10-06）。 */
.ok-click-toggle { margin-top: 12px; }
.ok-click-hint { margin: 6px 0 0; font-size: 12px; }
.mouse-action-row { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; margin-top: 6px; }
.mouse-action-group { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; }
.mouse-amount { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; margin-top: 10px; font-size: 13px; }
.mouse-amount input { width: 88px; max-width: 100%; padding: 5px 8px; font: inherit; color: inherit; background: transparent; border: 1px solid currentColor; border-radius: 4px; }
.mouse-amount-hint { color: var(--muted); }
.mouse-amount-error { color: var(--error-text); }
/* 滚动容器顶部会裁掉向上溢出（transform 不产生可滚动区域）：
   chip:hover 上浮 1px + 阴影会在容器顶边被切，故用内边距留出
   上浮空间，负外边距补偿保持原网格位置不变。 */
/* 打开应用：预设 + 已添加应用共用一个换行网格（2026-10-03 用户要求）。
   芯片内换行只给自定义应用名（预设名保持单行，避免撑高整行）。 */
.app-library .chip.custom-app { max-width: 100%; white-space: normal; overflow-wrap: anywhere; }
.app-library-search { display: block; width: min(300px, 100%); box-sizing: border-box; margin-top: 10px; padding: 6px 8px; font: inherit; color: inherit; background: transparent; border: 1px solid #888; border-radius: 4px; }
/* 「聚焦方式」面板：沿用基础按键 / 打开应用的 chip 风格，不另起大卡片。 */
.focus-profile { display: flex; flex-direction: column; gap: 6px; margin-top: 10px; }
.focus-profile-title { margin: 0; }
.focus-learn-row,
.focus-shortcut-row { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; }
.focus-shortcut-value {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-height: 30px;
  padding: 4px 11px;
  border-radius: 999px;
  border: 1px solid var(--border-strong);
  background: var(--surface-chip);
  font-size: 13px;
  font-weight: 600;
}
.focus-shortcut-value.empty { color: var(--muted); font-weight: 400; }
.focus-shortcut-keyboard {
  width: 15px;
  height: 15px;
  flex: none;
  stroke: currentColor;
  stroke-width: 1.7;
  stroke-linecap: round;
  stroke-linejoin: round;
}
.focus-learn-hint,
.focus-learn-message,
.focus-result-message { margin: 0; font-size: 12px; }
.focus-learn-message.is-opening,
.focus-learn-message.is-learning,
.focus-learn-message.is-recording { color: var(--warning-text); font-weight: 600; }
.focus-learn-message.is-captured,
.focus-learn-message.is-recorded { color: var(--success-text); font-weight: 600; }
.focus-learn-message.is-error { color: var(--error-text); font-weight: 600; }
.focus-record-status { font-size: 12px; font-weight: 600; }
.focus-record-status.is-pending { color: var(--warning-text); }
.focus-record-status.is-done { color: var(--success-text); }
.focus-link-button {
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--muted);
  font: inherit;
  font-size: 12px;
  text-decoration: underline;
  cursor: pointer;
}
.focus-link-button:disabled { opacity: 0.5; cursor: not-allowed; }
.focus-reset-link {
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--muted);
  font: inherit;
  font-size: 12px;
  text-decoration: underline;
  cursor: pointer;
}
.focus-reset-link:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
