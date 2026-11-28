<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import RegisteredAppsDialog from "../components/RegisteredAppsDialog.vue";
import EnhancedCaptureConfirmDialog from "../components/EnhancedCaptureConfirmDialog.vue";
import BatteryIndicator from "../components/BatteryIndicator.vue";
import MembershipRequiredButton from "../components/MembershipRequiredButton.vue";
import {
  SHORTCUT_PRESET_GROUPS,
  shortcutPresetHint,
  type ShortcutPresetItem,
} from "../lib/shortcut-presets";
import { reportFrontendEvent, reasonToken } from "../lib/frontend-diagnostics";
import { t, tPage } from "../lib/i18n";
import { effectiveLanguage } from "../lib/language-state";
import { COMBINATION_ACTIONS_ENABLED } from "../lib/feature-flags";
import {
  CAPTURE_MODIFIER_SOURCES,
  MODIFIER_KEYS,
  codeToKeyCode,
} from "../lib/key-codes";
import {
  effectiveProfileForTarget,
  explicitProfileForTarget,
  focusStrategyLabel,
  scrollRegionForTarget,
} from "../lib/app-adaptation";
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
  buttonFullLabels,
  isKnownButtonAction,
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
  listPresetApps,
  resolvePresetTarget,
  mouseClickLabels,
  mouseMoveLabels,
  openAppLabel,
  openCommonPhraseExample,
  registerCommonPhraseAvailability,
  registerPresetAppNames,
  registerMacroNames,
  latestMacroDefinitions,
  remoteModelLabel,
  resetButtonMappings,
  saveButtonMappings,
  saveLockMappingSelection,
  setMappingSuspension,
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
  type ButtonGestureTiming,
  type ButtonMappingSnapshot,
  type ButtonMappings,
  type ButtonTrigger,
  type CaptureSupport,
  type CommonPhraseAccess,
  type CustomAppPick,
  type AppFocusProfile,
  type FiredGesture,
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
import type { NavigationRequest } from "../navigation";
import { settingsComponent as commonPhraseSettingsComponent } from "../private/windows-common-phrases";
import {
  createButtonProfileScope,
  type ButtonProfileScope,
  type ProfileBindingState,
} from "../private/windows-button-profiles";

const props = withDefaults(
  defineProps<{
    runtime: RuntimeSnapshot | null;
    initialEditor?: { button: RemoteButton; trigger: ButtonTrigger } | null;
    initialProfileId?: string | null;
  }>(),
  { initialEditor: null, initialProfileId: null },
);
const emit = defineEmits<{ navigate: [page: string | NavigationRequest] }>();

/**
 * 「调整常用语」二级视图（对齐 macOS：按键页 → 调整常用语 → 返回按键页）。
 * 编辑器来自私有 UI（公开构建返回 null → 入口不渲染）。
 */
const commonPhraseEditor = commonPhraseSettingsComponent();
const showCommonPhraseEditor = ref(false);

/**
 * 私有键位方案作用域。公开构建返回 null；有私有组件时由它加载方案并
 * 把方案绑定转换成公开 ButtonActions。页面只消费稳定接口。
 */
const profileScope: ButtonProfileScope | null = createButtonProfileScope();
const profileScopeState = computed(() => profileScope?.state.value ?? null);
const profileScopeAvailable = computed(
  () =>
    Boolean(profileScope && profileScopeState.value?.available) &&
    !profileScopeState.value?.loading,
);
const profileScopeProfiles = computed(
  () => profileScopeState.value?.profiles ?? [],
);
const profileScopeSelected = computed(
  () => profileScopeState.value?.selectedProfileId ?? null,
);
const profileScopeAccess = computed(
  () => profileScopeState.value?.access ?? "unavailable",
);
const profileScopeCanEdit = computed(
  () =>
    profileScopeAccess.value === "allowed" ||
    profileScopeAccess.value === "temporarilyOffline",
);
const profileScopeNeedsMembership = computed(
  () => profileScopeAccess.value === "requiresPlus",
);
const profileScopeUnsupportedTitle = computed(() =>
  t("键位方案只支持 10 个普通按键。"),
);

const PROFILE_SCOPE_BUTTONS: RemoteButton[] = [
  "up",
  "down",
  "left",
  "right",
  "ok",
  "back",
  "home",
  "volume_up",
  "volume_down",
  "power",
];

const showProfileSaveDialog = ref(false);
const profileNameDraft = ref("");
const profileSaveBusy = ref(false);
const profileSaveMode = ref<"create" | "copy">("create");
const profileSaveSourceId = ref<string | null>(null);
const profileSaveSourceName = ref("");

const profileSaveDialogTitle = computed(() =>
  profileSaveMode.value === "copy"
    ? t("复制为新的键位方案")
    : t("创建新的键位方案"),
);
const profileSaveDialogNote = computed(() =>
  t("会把当前的动作与按住方式复制成一份独立方案；之后修改基础映射，这份方案不会跟着变。"),
);
const profileSaveConfirmLabel = computed(() => profileSaveDialogTitle.value);
const profileScopeMembershipHint = computed(() =>
  profileScopeSelected.value === null
    ? t("开通后可创建新方案")
    : t("开通后可复制当前方案"),
);

function openProfileSaveDialog(): void {
  if (!profileScope || !profileScopeCanEdit.value) {
    membershipRoute();
    return;
  }
  const selected = profileScopeSelected.value
    ? profileScopeProfiles.value.find(
        (profile) => profile.id === profileScopeSelected.value,
      )
    : null;
  if (selected) {
    profileSaveMode.value = "copy";
    profileSaveSourceId.value = selected.id;
    profileSaveSourceName.value = selected.name;
    profileNameDraft.value = t("{name} 副本", { name: selected.name }).slice(
      0,
      20,
    );
  } else {
    profileSaveMode.value = "create";
    profileSaveSourceId.value = null;
    profileSaveSourceName.value = t("基础映射");
    profileNameDraft.value = t("按键方案 {index}", {
      index: profileScopeProfiles.value.length + 1,
    }).slice(0, 20);
  }
  showProfileSaveDialog.value = true;
}

function closeProfileSaveDialog(): void {
  if (profileSaveBusy.value) return;
  showProfileSaveDialog.value = false;
}

async function submitProfileSave(): Promise<void> {
  if (!profileScope || profileSaveBusy.value) return;
  const name = profileNameDraft.value.trim();
  if (!name) {
    statusMessage.value = t("请填写方案名称。");
    return;
  }
  profileSaveBusy.value = true;
  try {
    if (profileSaveMode.value === "copy") {
      const sourceId = profileSaveSourceId.value;
      if (!sourceId) throw new Error("profile_copy_source_missing");
      await profileScope.copyProfile(sourceId, name);
      statusMessage.value = t("键位方案已创建，已切换到新方案");
    } else {
      const entries = PROFILE_SCOPE_BUTTONS.flatMap((button) =>
        TRIGGERS.map((trigger) => ({
          button,
          trigger,
          action:
            mappings.value.actions[button]?.[trigger] ?? { type: "disabled" },
        })),
      );
      const holds: Array<{
        button: RemoteButton;
        hold: "single" | "long" | "singleHold" | "longHold";
      }> = [];
      for (const button of PROFILE_SCOPE_BUTTONS) {
        const actions = mappings.value.actions[button];
        if (!actions) continue;
        if (actions.directionalHold === "single") {
          holds.push({ button, hold: "singleHold" });
        } else if (actions.directionalHold === "long") {
          holds.push({ button, hold: "longHold" });
        } else if (actions.holdRepeat) {
          holds.push({ button, hold: actions.holdRepeat });
        }
      }
      await profileScope.createFromBase(name, entries, holds);
      statusMessage.value = t("键位方案已创建");
    }
    showProfileSaveDialog.value = false;
  } catch (reason) {
    statusMessage.value = operationErrorText(
      "profile save as new failed",
      reason,
      profileSaveMode.value === "copy"
        ? "复制键位方案失败，请重试。"
        : "创建键位方案失败，请重试。",
    );
  } finally {
    profileSaveBusy.value = false;
  }
}

function profileScopeSupports(button: RemoteButton): boolean {
  return !profileScopeSelected.value || !profileScope || profileScope.supportsButton(button);
}

function selectProfileScope(profileId: string | null): void {
  profileScope?.select(profileId);
}

function membershipRoute(): void {
  emit("navigate", "membership");
}

void profileScope?.load();

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
const CARD_HEIGHT = 78;
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
 * 设备卡片只显示型号、状态和电量；不再显示蓝牙广播名（MI RC）。
 */

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

/** 按键图标：SVG path 组（24x24 视窗）。与键位方案页的 ButtonKeyIcon 保持同一套
 * 形状与 1.8 描边；menu/tv/语音键在键位方案页没有对应项，沿用原图标。 */
const buttonIcons: Record<RemoteButton, string[]> = {
  power: ["M12 3v9", "M6.4 6.4a8 8 0 1 0 11.2 0"],
  up: ["M12 19V5", "M6 11l6-6 6 6"],
  down: ["M12 5v14", "M6 13l6 6 6-6"],
  left: ["M19 12H5", "M11 6l-6 6 6 6"],
  right: ["M5 12h14", "M13 6l6 6-6 6"],
  ok: [
    "M12 3.2a8.8 8.8 0 1 0 0 17.6 8.8 8.8 0 0 0 0-17.6z",
    "M8.2 12.3l2.4 2.4 5.2-5.4",
  ],
  back: ["M9 7L4 12l5 5", "M4 12h9.5a5.5 5.5 0 0 1 5.5 5.5V19"],
  home: ["M3.4 11.4L12 4l8.6 7.4", "M5.5 10.5V20h13v-9.5", "M10 20v-5h4v5"],
  menu: ["M4 6h16", "M4 12h16", "M4 18h16"],
  tv: ["M3 7.5h18a1 1 0 0 1 1 1V18a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V8.5a1 1 0 0 1 1-1z", "M17 3l-5 4-5-4"],
  volume_up: [
    "M4 9.5h3.2L12 6v12l-4.8-3.5H4z",
    "M15 9a4.2 4.2 0 0 1 0 6",
    "M17.6 6.4a8 8 0 0 1 0 11.2",
  ],
  volume_down: [
    "M4 9.5h3.2L12 6v12l-4.8-3.5H4z",
    "M15 9a4.2 4.2 0 0 1 0 6",
  ],
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
const editorScrollHost = ref<HTMLElement | null>(null);
const restoringProfileBinding = ref(false);
const profileBindingState = computed<ProfileBindingState | null>(() => {
  if (
    !profileScope ||
    !profileScopeSelected.value ||
    !editingTarget.value ||
    !profileScope.supportsButton(editingTarget.value.button)
  ) {
    return null;
  }
  return profileScope.bindingState(
    editingTarget.value.button,
    editingTarget.value.trigger,
  );
});
const editorCanWrite = computed(() => {
  if (profileBindingState.value === "unavailable") return false;
  return !profileScopeSelected.value || profileScopeCanEdit.value;
});
const disableActionActive = computed(() => {
  if (!editingTarget.value) return false;
  if (profileScopeSelected.value) return profileBindingState.value === "disabled";
  return actionOf(editingTarget.value.button, editingTarget.value.trigger).type === "disabled";
});
type EditorSaveOutcome = "idle" | "saving" | "saved" | "failed" | "unknown";
const editorSaveOutcome = ref<EditorSaveOutcome>("idle");
const editorSaveRetry = ref<(() => void) | null>(null);
const EDITOR_SAVE_FEEDBACK_MS = 2400;
let editorSaveTimer: number | null = null;
const editorSaveLabel = computed(() => {
  if (profileBindingState.value === "unavailable") return t("不可用");
  if (!editorCanWrite.value) return t("只读");
  if (editorSaveOutcome.value === "saved") return t("自动保存成功");
  if (editorSaveOutcome.value === "failed") return t("保存失败");
  if (editorSaveOutcome.value === "unknown") return t("保存状态未知，请重试。");
  return t("自动保存");
});
const editorSaveTone = computed(() => {
  if (editorSaveOutcome.value === "saved") return "saved";
  if (editorSaveOutcome.value === "failed" || editorSaveOutcome.value === "unknown") {
    return "error";
  }
  return "idle";
});
function setEditorSaveOutcome(
  outcome: EditorSaveOutcome,
  retry: (() => void) | null = null,
): void {
  if (editorSaveTimer !== null) {
    window.clearTimeout(editorSaveTimer);
    editorSaveTimer = null;
  }
  editorSaveOutcome.value = outcome;
  editorSaveRetry.value = retry;
  if (outcome === "saved") {
    editorSaveTimer = window.setTimeout(() => {
      if (editorSaveOutcome.value === "saved") {
        editorSaveOutcome.value = "idle";
        editorSaveRetry.value = null;
      }
      editorSaveTimer = null;
    }, EDITOR_SAVE_FEEDBACK_MS);
  }
}
function resetEditorSaveStatus(): void {
  if (editorSaveTimer !== null) {
    window.clearTimeout(editorSaveTimer);
    editorSaveTimer = null;
  }
  editorSaveOutcome.value = "idle";
  editorSaveRetry.value = null;
}
function retryEditorSave(): void {
  editorSaveRetry.value?.();
}
const lockSelection = ref(switchCache?.lockSelection ?? true);
const activeButtons = ref<Set<RemoteButton>>(new Set());
const lastFired = ref<FiredGesture | null>(null);
const firedFlash = ref<{ button: RemoteButton; trigger: ButtonTrigger } | null>(null);
const mappingSnapshot = ref<ButtonMappingSnapshot | null>(null);
const busy = ref(false);
/**
 * 页面底部状态提示：所有写入都经过 setter，覆盖旧提示时重置计时，
 * 8 秒后自动清空（与组合动作页一致；2026-11-22 Andy 要求）。
 * 错误也走同一条路径，避免提示永久占住页面底部。
 */
const statusMessageText = ref<string | null>(null);
const STATUS_VISIBLE_MS = 8_000;
let statusTimer: number | null = null;

function clearStatusTimer(): void {
  if (statusTimer !== null) {
    window.clearTimeout(statusTimer);
    statusTimer = null;
  }
}

const statusMessage = computed<string | null>({
  get: () => statusMessageText.value,
  set: (message) => {
    clearStatusTimer();
    statusMessageText.value = message;
    if (!message) return;
    statusTimer = window.setTimeout(() => {
      statusMessageText.value = null;
      statusTimer = null;
    }, STATUS_VISIBLE_MS);
  },
});
async function restoreProfileBinding(): Promise<void> {
  const scope = profileScope;
  const target = editingTarget.value;
  if (!scope || !target || profileScopeSelected.value === null) return;
  restoringProfileBinding.value = true;
  setEditorSaveOutcome("saving");
  try {
    await scope.clearAction(target.button, target.trigger);
    setEditorSaveOutcome("saved");
  } catch (reason) {
    console.warn("profile binding restore failed", reason);
    setEditorSaveOutcome("failed", () => {
      void restoreProfileBinding();
    });
  } finally {
    restoringProfileBinding.value = false;
  }
}

function saveProfileAction(
  target: { button: RemoteButton; trigger: ButtonTrigger },
  action: ButtonAction,
): void {
  const scope = profileScope;
  if (!scope) return;
  const retry = () => saveProfileAction(target, action);
  setEditorSaveOutcome("saving");
  void scope
    .writeAction(target.button, target.trigger, action)
    .then(() => setEditorSaveOutcome("saved"))
    .catch((reason) => {
      console.warn("profile action save failed", reason);
      setEditorSaveOutcome("failed", retry);
    });
}

function toggleDisableAction(event: Event): void {
  const target = editingTarget.value;
  if (!target) return;
  const disabled = (event.target as HTMLInputElement).checked;
  if (disabled) {
    applyAction({ type: "disabled" });
    return;
  }
  if (profileScope && profileScopeSelected.value !== null) {
    void restoreProfileBinding();
    return;
  }
  applyAction({ type: "shortcut", chord: { keys: ["escape"] } });
}

const capturingShortcut = ref(false);
const captureStarting = ref(false);
/** start 返回的 preheld 键尚未全部松开：后端吞键但不投递边沿，必须提示用户。 */
const waitingPreheldRelease = ref(false);
const captureDisplay = ref<string[]>([]);
const safeCaptureMode = ref(false);
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
  void stopShortcutCapture();
  if (snapshotTimer !== null) window.clearInterval(snapshotTimer);
  snapshotTimer = null;
  if (flashTimer !== null) window.clearTimeout(flashTimer);
  flashTimer = null;
  clearStatusTimer();
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
  const base =
    mappings.value.actions[button] ?? {
      single: { type: "disabled" },
      double: { type: "disabled" },
      long: { type: "disabled" },
    };
  if (!profileScope || !profileScopeSelected.value) return base;
  return profileScope.displayActions(button, base);
}

function actionOf(button: RemoteButton, trigger: ButtonTrigger): ButtonAction {
  return actionsOf(button)[trigger];
}

/** 按键卡里的空动作按 Mac 文案显示为「不执行」。 */
function mappingActionSummary(
  button: RemoteButton,
  trigger: ButtonTrigger,
): string {
  const action = actionOf(button, trigger);
  return action.type === "disabled" ? t("不执行") : actionSummary(action);
}

function holdTriggerFor(button: RemoteButton): ButtonTrigger {
  const actions = actionsOf(button);
  if (actions.directionalHold === "long" || actions.holdRepeat === "long") {
    return "long";
  }
  return "single";
}

function holdSummaryFor(button: RemoteButton): string {
  const actions = actionsOf(button);
  const actionFor = (trigger: ButtonTrigger) => {
    const action = actionOf(button, trigger);
    return action.type === "shortcut" &&
      action.chord.keys.length === 1 &&
      (action.chord.keys[0] === "left" || action.chord.keys[0] === "right")
      ? actionSummary(action)
      : mappingActionSummary(button, trigger);
  };
  if (actions.directionalHold === "single") {
    return `${t("按住时 按住 ")}${actionFor("single")}`;
  }
  if (actions.directionalHold === "long") {
    return `${t("按住时 长按后按住 ")}${actionFor("long")}`;
  }
  if (actions.holdRepeat === "single") {
    return `${t("按住时 重复单击动作 ")}${actionFor("single")}`;
  }
  if (actions.holdRepeat === "long") {
    return `${t("按住时 重复长按动作 ")}${actionFor("long")}`;
  }
  return t("按住时");
}

function openHoldEditor(button: RemoteButton): void {
  if (captureGated(button)) return;
  if (profileScopeSelected.value && !profileScopeSupports(button)) {
    statusMessage.value = profileScopeUnsupportedTitle.value;
    return;
  }
  if (profileScope && profileScopeSelected.value && !profileScopeCanEdit.value) {
    membershipRoute();
    return;
  }
  openEditor(button, holdTriggerFor(button), "hold");
}

/** 卡片格子的悬停标题：随界面语言实时生成（含全角冒号的原文作为字典 key）。 */
function actionCellTitle(button: RemoteButton, trigger: ButtonTrigger): string {
  return t("{button} · {trigger}：{summary}", {
    button: buttonFullLabels[button],
    trigger: buttonTriggerLabel(trigger),
    summary: mappingActionSummary(button, trigger),
  });
}

/**
 * 方向键在系统字体里只有细线字形，和同一行的加粗文字放一起明显偏细。
 * 格子内容命中这几个字形时加一层极细描边（见 styles.css）。
 */
const DIRECTION_SUMMARIES = new Set(["↑", "↓", "←", "→"]);

function isDirectionSummary(summary: string): boolean {
  return DIRECTION_SUMMARIES.has(summary);
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

/** AppsFolder 目标无法在前端按路径匹配预设，由后端解析真实 exe 后补齐。 */
const resolvedFocusPresetId = ref<string | null>(null);
let focusPresetGeneration = 0;
watch(
  focusTarget,
  async (target) => {
    const generation = ++focusPresetGeneration;
    resolvedFocusPresetId.value = null;
    if (!target) return;
    try {
      const presetId = await resolvePresetTarget(target);
      if (generation === focusPresetGeneration) resolvedFocusPresetId.value = presetId;
    } catch {
      // 解析失败只影响预设标记；显式档案和自定义目标仍照常显示。
    }
  },
  { immediate: true },
);

/** 目标匹配到的预设 id：内置预设 id 或用户手动添加的预设路径。 */
const focusTargetPresetId = computed<string | null>(() => {
  const target = focusTarget.value?.trim().toLocaleLowerCase();
  if (!target) return null;
  return (
    presetApps.value.find(
      (app) =>
        app.id.toLocaleLowerCase() === target ||
        Boolean(app.path && app.path.toLocaleLowerCase() === target),
    )?.id ?? null
  );
});
const effectiveFocusPresetId = computed(
  () => focusTargetPresetId.value ?? resolvedFocusPresetId.value,
);
const focusTargetIsBuiltInPreset = computed(() =>
  Boolean(
    focusTarget.value &&
      (presetAppIds.value.has(focusTarget.value) ||
        (!focusTargetPresetId.value && resolvedFocusPresetId.value)),
  ),
);
const focusTargetIsLibraryPreset = computed(
  () => Boolean(effectiveFocusPresetId.value && !focusTargetIsBuiltInPreset.value),
);
/** 预置应用只有存在用户显式档案时才在按键页显示摘要，避免把预置默认误报成用户设置。 */
const focusTargetHasExplicitProfile = computed(() =>
  Boolean(
    focusTarget.value &&
      explicitProfileForTarget(
        mappings.value.focusProfiles,
        focusTarget.value,
        effectiveFocusPresetId.value,
      ),
  ),
);

/** 该目标实际会执行的聚焦档案：显式档案优先，预设默认兜底。 */
const focusProfile = computed<AppFocusProfile | null>(() => {
  const target = focusTarget.value;
  if (!target) return null;
  return effectiveProfileForTarget(
    mappings.value.focusProfiles,
    target,
    effectiveFocusPresetId.value,
  );
});

/** 按键页只读摘要：可写控件统一在「APP 适配」页。 */
const focusSummary = computed(() =>
  focusProfile.value
    ? t(focusStrategyLabel(focusProfile.value.strategy))
    : t("只打开 APP"),
);

function configureFocusProfile(): void {
  const target = focusTarget.value;
  if (!target) return;
  emit("navigate", { page: "apps", params: { appTarget: target } });
}

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
  // 聚焦服务只服务「打开后自动聚焦」；旧版「记录点击位置」不走这个服务，
  // 继续显示旧的失败会让用户以为新配置仍然失败（2026-10-19 现场）。
  if (focusProfile.value?.strategy !== "recorded_element") return null;
  const report = mappingSnapshot.value?.lastFocus;
  if (!report || report.focused) return null;
  return t("上次聚焦失败：{reason}", {
    reason: focusFailureLabel(report.reason ?? ""),
  });
});

// 2026-10-23 Andy：预设应用也显示聚焦方式；2026-11-09 起可写控件迁到「APP 适配」页，
// 这里只保留只读摘要和跳转入口。
const showAppFocusStrategy = computed(() => Boolean(focusTarget.value));

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
const customAppsOpen = ref<boolean | null>(null);
const customAppGroupEl = ref<HTMLElement | null>(null);
const customAppsExpanded = computed(
  () =>
    customAppsOpen.value ??
    Boolean(focusTarget.value && !focusTargetIsBuiltInPreset.value),
);
/** 展开自定义 APP 列表后滚到完整可见：首次打开时列表在编辑卡底部，不滚动会看不全。 */
function toggleCustomApps(): void {
  customAppsOpen.value = !customAppsExpanded.value;
  if (!customAppsExpanded.value) return;
  void nextTick(() => {
    customAppGroupEl.value?.scrollIntoView?.({ block: "start", behavior: "auto" });
  });
}
const focusScrollStatus = computed(() => {
  const target = focusTarget.value;
  if (!target) return null;
  if (
    scrollRegionForTarget(
      mappings.value.scrollRegions,
      target,
      effectiveFocusPresetId.value,
    )
  ) {
    return t("已学习");
  }
  if (focusTargetIsBuiltInPreset.value || focusTargetIsLibraryPreset.value) {
    return t("预置可用");
  }
  return t("需学习");
});
watch([presetApps, () => mappings.value.applications], () => {
  registerPresetAppNames([...presetApps.value, ...(mappings.value.applications ?? []).map(app => ({ id: app.path, name: app.name }))]);
}, { deep: true });

/**
 * 应用选择器确认：把选中的 APP 写入应用库，并立即绑定到当前编辑格。
 *
 * 选择器一次只返回一个 APP，不再区分扫描/文件来源；两者都走同一个
 * `open_app` 动作。应用库与按键动作放在一次保存里，避免半写入。
 */
async function selectApp(app: CustomAppPick): Promise<void> {
  if (busy.value) return;
  // 只在确实要替换当前编辑格时要求二次确认；单纯添加应用到库不应被拦。
  if (editingTarget.value && blockUnsupportedReplacement()) return;
  unsupportedReplaceArmed.value = false;
  busy.value = true;
  appPickerError.value = null;
  try {
    const target = editingTarget.value;
    const applications = [...(mappings.value.applications ?? [])];
    const key = app.path.toLowerCase();
    if (!applications.some((candidate) => candidate.path.toLowerCase() === key)) {
      applications.push({ name: app.name, path: app.path });
    }
    const next: ButtonMappings = {
      ...mappings.value,
      applications,
      actions: { ...mappings.value.actions },
    };
    let notice: string | null = null;
    if (target) {
      const actions = { ...actionsOf(target.button) };
      actions[target.trigger] = { type: "open_app", target: app.path };
      const reconciled = reconcileHoldRepeat(actions);
      next.actions[target.button] = reconciled.actions;
      notice = reconciled.notice;
    }
    const saved = await saveButtonMappings(next);
    mappings.value = saved;
    savedSnapshot.value = JSON.parse(JSON.stringify(saved)) as ButtonMappings;
    writeSwitchCache(saved.enabled, lockSelection.value);
    customAppsOpen.value = true;
    appPickerOpen.value = false;
    statusMessage.value =
      notice ??
      (target
        ? t("已选择：{name}", { name: app.name })
        : t("已添加应用：{name}", { name: app.name }));
  } catch (cause) {
    appPickerError.value = operationErrorText(
      "select app failed",
      cause,
      "没能添加应用，请重试。",
    );
  } finally {
    busy.value = false;
  }
}

function selectButton(button: RemoteButton): void {
  // 三键在「全按键支持」关闭时置灰禁用：合成事件绕过原生 disabled，这里再守一道。
  if (captureGated(button)) return;
  if (profileScopeSelected.value && !profileScopeSupports(button)) {
    statusMessage.value = profileScopeUnsupportedTitle.value;
    return;
  }
  selectedButton.value = button;
}

function openEditor(
  button: RemoteButton,
  trigger: ButtonTrigger,
  filterOverride: ActionFilter | null = null,
): void {
  if (captureGated(button)) return;
  if (profileScopeSelected.value && !profileScopeSupports(button)) {
    statusMessage.value = t("键位方案只支持 10 个普通按键。");
    return;
  }
  selectedButton.value = button;
  pendingActionFilterOverride = filterOverride;
  editingTarget.value = { button, trigger };
  const action = actionOf(button, trigger);
  customAppsOpen.value =
    action.type === "open_app" && !presetAppIds.value.has(action.target);
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

/**
 * 未知动作来自更新版本，不能被一次误点直接覆盖。第一次选择只解除保护，
 * 第二次选择才真正替换；用户仍可显式改掉它。
 */
const unsupportedReplaceArmed = ref(false);

function blockUnsupportedReplacement(): boolean {
  const current = selectedAction.value;
  if (!current || isKnownButtonAction(current) || unsupportedReplaceArmed.value) {
    return false;
  }
  unsupportedReplaceArmed.value = true;
  statusMessage.value = t(
    "这个动作来自更新版本，当前版本只保留、不执行。再次选择任意动作将替换它；如需保留原动作，请升级无线麦。",
  );
  return true;
}

function applyAction(action: ButtonAction): void {
  const target = editingTarget.value;
  if (!target) return;
  if (profileScope && profileScopeSelected.value && !profileScopeCanEdit.value) {
    void membershipRoute();
    return;
  }
  if (blockUnsupportedReplacement()) return;
  unsupportedReplaceArmed.value = false;
  if (profileScope && profileScopeSelected.value) {
    saveProfileAction(target, action);
    return;
  }
  const next: ButtonMappings = {
    ...mappings.value,
    actions: { ...mappings.value.actions },
  };
  const actions = { ...actionsOf(target.button) };
  actions[target.trigger] = action;
  const reconciled = reconcileHoldRepeat(actions);
  next.actions[target.button] = reconciled.actions;
  mappings.value = next;
  if (action.type === "open_app") {
    customAppsOpen.value = !presetAppIds.value.has(action.target);
  }
  // 对齐 Mac：点击动作即自动保存生效（静默；失败信息显示在编辑面板右上方）。
  const retry = () => {
    void persist(reconciled.notice ?? undefined, false).then((ok) => {
      if (ok) setEditorSaveOutcome("saved");
      else setEditorSaveOutcome("failed", retry);
    });
  };
  setEditorSaveOutcome("saving");
  void persist(reconciled.notice ?? undefined, false).then((ok) => {
    if (ok) setEditorSaveOutcome("saved");
    else setEditorSaveOutcome("failed", retry);
  });
}

const selectedAction = computed(() => editingTarget.value ? actionOf(editingTarget.value.button, editingTarget.value.trigger) : null);

type ActionFilter =
  | "all"
  | "basic"
  | "customActions"
  | "applications"
  | "mouse"
  | "combinations"
  | "commonPhrases"
  | "hold"
  | "buttonSpeed";
const activeActionFilter = ref<ActionFilter>("all");
const actionFilterItems = computed<Array<{ id: ActionFilter; label: string }>>(() => {
  // 2026-11-24 用户定稿：基础按键合并「系统与媒体」，自定义动作合并
  // 「自定义按键 + 输入框」；鼠标动作独立成类。顺序固定如下。
  const items: Array<{ id: ActionFilter; label: string }> = [
    { id: "all", label: t("全部") },
    { id: "basic", label: t("基础按键") },
    { id: "customActions", label: t("自定义动作") },
    { id: "applications", label: t("打开 App") },
    { id: "mouse", label: t("鼠标动作") },
  ];
  if (COMBINATION_ACTIONS_ENABLED) {
    items.push({ id: "combinations", label: t("组合动作") });
  }
  if (commonPhrasesAvailable.value) {
    items.push({ id: "commonPhrases", label: t("常用语") });
  }
  items.push(
    { id: "hold", label: t("按住") },
    { id: "buttonSpeed", label: t("按键速度") },
  );
  return items;
});
const showActionEditor = computed(
  () =>
    !!editingTarget.value &&
    activeActionFilter.value !== "hold" &&
    activeActionFilter.value !== "buttonSpeed",
);
const showStandaloneHoldPanel = computed(
  () => !!editingTarget.value && activeActionFilter.value === "hold",
);
const showSpeedPanel = computed(
  () =>
    !!editingTarget.value &&
    (activeActionFilter.value === "all" || activeActionFilter.value === "buttonSpeed"),
);
const visibleShortcutGroups = computed(() => {
  if (activeActionFilter.value === "all" || activeActionFilter.value === "basic") {
    return SHORTCUT_PRESET_GROUPS;
  }
  return [];
});
const showCustomShortcutSection = computed(
  () => activeActionFilter.value === "all" || activeActionFilter.value === "customActions",
);
const showInputSection = computed(
  () => activeActionFilter.value === "all" || activeActionFilter.value === "customActions",
);
const showAppSection = computed(
  () => activeActionFilter.value === "all" || activeActionFilter.value === "applications",
);
const showMacroSection = computed(
  () => activeActionFilter.value === "all" || activeActionFilter.value === "combinations",
);
const showCommonPhrasesSection = computed(
  () => activeActionFilter.value === "all" || activeActionFilter.value === "commonPhrases",
);
const showMouseSection = computed(
  () => activeActionFilter.value === "all" || activeActionFilter.value === "mouse",
);
const showInlineHoldSection = computed(
  () => repeatGroupVisible.value && activeActionFilter.value === "all",
);
let pendingActionFilterOverride: ActionFilter | null = null;

const DEFAULT_GESTURE_TIMING: ButtonGestureTiming = {
  doubleClickMs: 300,
  longPressMs: 550,
  repeatStartMs: 350,
};
function normalizeGestureTiming(timing?: ButtonGestureTiming | null): ButtonGestureTiming {
  const roundStep = (value: number, min: number, max: number) =>
    Math.min(max, Math.max(min, Math.round(value / 50) * 50));
  const source = timing ?? DEFAULT_GESTURE_TIMING;
  return {
    doubleClickMs: roundStep(
      Number.isFinite(source.doubleClickMs) ? source.doubleClickMs : 300,
      50,
      500,
    ),
    longPressMs: roundStep(
      Number.isFinite(source.longPressMs) ? source.longPressMs : 550,
      300,
      1000,
    ),
    repeatStartMs: roundStep(
      Number.isFinite(source.repeatStartMs) ? source.repeatStartMs : 350,
      50,
      2000,
    ),
  };
}
const gestureTimingDraft = reactive<ButtonGestureTiming>({ ...DEFAULT_GESTURE_TIMING });
const gestureTimingSaved = ref<ButtonGestureTiming>({ ...DEFAULT_GESTURE_TIMING });
const gestureTimingSaveState = ref<"idle" | "saving" | "saved" | "failed">("idle");
const gestureTimingModelKey = computed(
  () => props.runtime?.platform.connection?.remoteModel ?? "unknown",
);
const gestureTimingModelKnown = computed(() => gestureTimingModelKey.value !== "unknown");
function markGestureTimingEdited(): void {
  if (gestureTimingSaveState.value !== "saving") {
    gestureTimingSaveState.value = "idle";
  }
}
function storedGestureTiming(): ButtonGestureTiming {
  const key = gestureTimingModelKey.value;
  const buckets = mappings.value.gestureTimingsByModel ?? {};
  if (Object.keys(buckets).length > 0) {
    return normalizeGestureTiming(buckets[key]);
  }
  return normalizeGestureTiming(mappings.value.gestureTiming);
}
const gestureTimingDirty = computed(
  () =>
    gestureTimingDraft.doubleClickMs !== gestureTimingSaved.value.doubleClickMs ||
    gestureTimingDraft.longPressMs !== gestureTimingSaved.value.longPressMs ||
    gestureTimingDraft.repeatStartMs !== gestureTimingSaved.value.repeatStartMs,
);
const gestureTimingCanSave = computed(
  () =>
    gestureTimingModelKnown.value &&
    !busy.value &&
    gestureTimingSaveState.value !== "saving" &&
    gestureTimingDirty.value,
);
const gestureTimingHint = computed(() => {
  if (!gestureTimingModelKnown.value) {
    return t("连接并识别遥控器后才能保存按键速度。");
  }
  if (gestureTimingSaveState.value === "failed") {
    return t("保存失败，请重试。");
  }
  if (gestureTimingSaveState.value === "saved") {
    return t("已保存；重新打开也会保留。");
  }
  if (gestureLearning.step === "complete") {
    return t("学习结果会填入识别时间并保留，重新打开也会显示学习完成，点保存后才会用到按键上。");
  }
  if (gestureTimingDirty.value) {
    return t("点保存后才会用到按键上。");
  }
  return t("学习结果会填入识别时间并保留，重新打开也会显示学习完成，点保存后才会用到按键上。");
});
type GestureLearningStep = "idle" | "single" | "double" | "long" | "complete";
const gestureLearning = reactive<{
  step: GestureLearningStep;
  button: RemoteButton | null;
  single: number | null;
  double: number | null;
  long: number | null;
  pressedAt: number | null;
  firstDoubleReleaseAt: number | null;
  doublePresses: number;
  ignoreNextDoubleRelease: boolean;
}>({
  step: "idle",
  button: null,
  single: null,
  double: null,
  long: null,
  pressedAt: null,
  firstDoubleReleaseAt: null,
  doublePresses: 0,
  ignoreNextDoubleRelease: false,
});
const GESTURE_LEARNING_CACHE = "sayall.buttonGestureLearning.v1";
function gestureLearningCacheKey(): string {
  return `${GESTURE_LEARNING_CACHE}.${gestureTimingModelKey.value}`;
}
function restoreGestureLearningCache(): void {
  gestureLearning.single = null;
  gestureLearning.double = null;
  gestureLearning.long = null;
  gestureLearning.step = "idle";
  try {
    const raw = window.localStorage.getItem(gestureLearningCacheKey());
    if (!raw) return;
    const saved = JSON.parse(raw) as Partial<typeof gestureLearning>;
    if (
      typeof saved.single === "number" &&
      typeof saved.double === "number" &&
      typeof saved.long === "number"
    ) {
      gestureLearning.single = saved.single;
      gestureLearning.double = saved.double;
      gestureLearning.long = saved.long;
      gestureLearning.step = "complete";
    }
  } catch {
    // 学习缓存只是展示用的便利数据；损坏时静默丢弃。
  }
}
function persistGestureLearningCache(): void {
  try {
    window.localStorage.setItem(
      gestureLearningCacheKey(),
      JSON.stringify({
        single: gestureLearning.single,
        double: gestureLearning.double,
        long: gestureLearning.long,
      }),
    );
  } catch {
    // localStorage 不可用时不阻断学习流程。
  }
}
restoreGestureLearningCache();
watch(gestureTimingModelKey, () => {
  cancelGestureLearning();
  restoreGestureLearningCache();
});
watch(
  () => {
    const phase = props.runtime?.platform.connection?.phase;
    return phase === "ready" || phase === "streaming";
  },
  (ready) => {
    if (!ready) cancelGestureLearning();
  },
);
watch(showSpeedPanel, (visible) => {
  if (!visible) cancelGestureLearning();
});
const gestureLearningActive = computed(
  () => gestureLearning.step !== "idle" && gestureLearning.step !== "complete",
);
const gestureLearningCanStart = computed(
  () => connectionReady.value && gestureTimingModelKnown.value,
);
const gestureLearningSuspensionHeld = ref(false);
const gestureLearningStarting = ref(false);
let gestureLearningStartRequest = 0;
async function setGestureLearningSuspension(suspended: boolean): Promise<boolean> {
  try {
    await setMappingSuspension(suspended);
    gestureLearningSuspensionHeld.value = suspended;
    return true;
  } catch (error) {
    console.warn("button gesture learning suspension failed", error);
    statusMessage.value = operationErrorText(
      "button gesture learning suspension failed",
      error,
      t("无法暂停按键动作，学习未开始。"),
    );
    return false;
  }
}
function releaseGestureLearningSuspension(): void {
  if (!gestureLearningSuspensionHeld.value) return;
  gestureLearningSuspensionHeld.value = false;
  void setMappingSuspension(false).catch(() => {
    // 页面已卸载时桥接可能不可用；学习暂停必须在可用的下一次进入页面时由
    // 开始流程重新建立，这里只保证不因收尾异常打断卸载。
  });
}
function cancelGestureLearning(): void {
  gestureLearningStartRequest += 1;
  if (!gestureLearningActive.value) {
    releaseGestureLearningSuspension();
    return;
  }
  gestureLearning.step = "idle";
  gestureLearning.button = null;
  gestureLearning.single = null;
  gestureLearning.double = null;
  gestureLearning.long = null;
  gestureLearning.pressedAt = null;
  gestureLearning.firstDoubleReleaseAt = null;
  gestureLearning.doublePresses = 0;
  gestureLearning.ignoreNextDoubleRelease = false;
  releaseGestureLearningSuspension();
}
const gestureLearningStatusText = computed(() => {
  if (gestureLearningStarting.value) return t("正在暂停按键动作…");
  if (gestureLearning.step === "complete") return t("学习完成");
  if (gestureLearning.step === "single") return t("正在学习 1/3");
  if (gestureLearning.step === "double") return t("正在学习 2/3");
  if (gestureLearning.step === "long") return t("正在学习 3/3");
  return t("未开始学习");
});

watch(gestureTimingModelKey, () => {
  gestureTimingSaveState.value = "idle";
});
watch(
  () =>
    [
      gestureTimingModelKey.value,
      mappings.value.gestureTimingsByModel,
      mappings.value.gestureTiming,
    ] as const,
  () => {
    const next = storedGestureTiming();
    Object.assign(gestureTimingDraft, next);
    gestureTimingSaved.value = { ...next };
    if (gestureTimingSaveState.value !== "saving") {
      gestureTimingSaveState.value = "idle";
    }
  },
  { immediate: true, deep: true },
);

async function startGestureLearning(): Promise<void> {
  if (!gestureLearningCanStart.value || gestureLearningActive.value) return;
  if (gestureLearningStarting.value) return;
  const request = ++gestureLearningStartRequest;
  gestureLearningStarting.value = true;
  let suspended = false;
  try {
    suspended = await setGestureLearningSuspension(true);
  } finally {
    gestureLearningStarting.value = false;
  }
  if (request !== gestureLearningStartRequest || unmounted) {
    releaseGestureLearningSuspension();
    return;
  }
  if (!suspended) return;
  gestureLearning.step = "single";
  gestureLearning.button = null;
  gestureLearning.single = null;
  gestureLearning.double = null;
  gestureLearning.long = null;
  gestureLearning.pressedAt = null;
  gestureLearning.firstDoubleReleaseAt = null;
  gestureLearning.doublePresses = 0;
  gestureLearning.ignoreNextDoubleRelease = false;
  gestureTimingSaveState.value = "idle";
}

function handleGestureLearningEdge(edge: ButtonEdge): void {
  if (!gestureLearningActive.value) return;
  if (gestureLearning.button === null) {
    if (!edge.isPressed) return;
    gestureLearning.button = edge.button;
  } else if (gestureLearning.button !== edge.button) {
    return;
  }
  const now = performance.now();
  if (edge.isPressed) {
    gestureLearning.pressedAt = now;
    if (gestureLearning.step === "double") {
      if (gestureLearning.doublePresses === 1 && gestureLearning.firstDoubleReleaseAt !== null) {
        const interval = Math.max(
          0,
          Math.round(now - gestureLearning.firstDoubleReleaseAt),
        );
        // 报告层可能把一次按压拆成相邻边沿：松手到再按下 < 20ms 不可能是
        // 人工双击，整对忽略，保留第一次松手时刻等待用户真正补第二下。
        if (interval < 20) {
          gestureLearning.ignoreNextDoubleRelease = true;
        } else {
          gestureLearning.double = interval;
        }
      }
    }
    return;
  }
  const pressedAt = gestureLearning.pressedAt;
  if (pressedAt === null) return;
  gestureLearning.pressedAt = null;
  const duration = Math.max(0, Math.round(now - pressedAt));
  if (gestureLearning.step === "single") {
    gestureLearning.single = duration;
    gestureLearning.step = "double";
    gestureLearning.doublePresses = 0;
    gestureLearning.ignoreNextDoubleRelease = false;
    gestureLearning.firstDoubleReleaseAt = null;
    return;
  }
  if (gestureLearning.step === "double") {
    if (gestureLearning.ignoreNextDoubleRelease) {
      gestureLearning.ignoreNextDoubleRelease = false;
      return;
    }
    gestureLearning.doublePresses += 1;
    if (gestureLearning.doublePresses === 1) {
      gestureLearning.firstDoubleReleaseAt = now;
    } else {
      gestureLearning.step = "long";
    }
    return;
  }
  if (gestureLearning.step === "long") {
    if (duration < 150) {
      return;
    }
    gestureLearning.long = duration;
    gestureLearning.step = "complete";
    persistGestureLearningCache();
    releaseGestureLearningSuspension();
    const double = gestureLearning.double ?? 0;
    gestureTimingDraft.doubleClickMs = normalizeGestureTiming({
      doubleClickMs: Math.max(150, Math.ceil(double / 50) * 50),
      longPressMs: Math.floor(duration / 50) * 50,
      repeatStartMs: gestureTimingDraft.repeatStartMs,
    }).doubleClickMs;
    gestureTimingDraft.longPressMs = normalizeGestureTiming({
      doubleClickMs: gestureTimingDraft.doubleClickMs,
      longPressMs: Math.floor(duration / 50) * 50,
      repeatStartMs: gestureTimingDraft.repeatStartMs,
    }).longPressMs;
  }
}

async function saveGestureTiming(): Promise<void> {
  if (!gestureTimingCanSave.value || !gestureTimingModelKnown.value) return;
  const timing = normalizeGestureTiming(gestureTimingDraft);
  const modelKey = gestureTimingModelKey.value;
  gestureTimingSaveState.value = "saving";
  mappings.value = {
    ...mappings.value,
    gestureTiming: timing,
    gestureTimingsByModel: {
      ...(mappings.value.gestureTimingsByModel ?? {}),
      [modelKey]: timing,
    },
  };
  const ok = await persist(t("按键速度已保存"));
  if (ok) {
    gestureTimingSaved.value = { ...timing };
    gestureTimingSaveState.value = "saved";
  } else {
    gestureTimingSaveState.value = "failed";
  }
}

function shortcutCategory(keys: readonly KeyCode[]): ActionFilter {
  for (const group of SHORTCUT_PRESET_GROUPS) {
    if (
      group.items.some(
        (item) =>
          item.keys.length === keys.length &&
          item.keys.every((key, keyIndex) => key === keys[keyIndex]),
      )
    ) {
      // 预设快捷键（基础按键 + 系统与媒体）都归入「基础按键」；
      // 不在预设表里的快捷键归入「自定义动作」。
      return "basic";
    }
  }
  return "customActions";
}

function filterForAction(action: ButtonAction | null): ActionFilter {
  if (!action || action.type === "disabled") return "all";
  switch (action.type) {
    case "shortcut":
      return shortcutCategory(action.chord.keys);
    case "app_switcher":
      return "basic";
    case "mouse_click":
    case "mouse_move":
    case "scroll":
    case "page_up":
    case "page_down":
    case "scroll_to_top":
    case "scroll_to_bottom":
      return "mouse";
    case "focus_input":
      return "customActions";
    case "run_macro":
      return COMBINATION_ACTIONS_ENABLED ? "combinations" : "all";
    case "open_common_phrases":
      return commonPhrasesAvailable.value ? "commonPhrases" : "customActions";
    case "open_app":
      return "applications";
    default:
      return "all";
  }
}

function selectActionFilter(filter: ActionFilter): void {
  if (activeActionFilter.value === filter) return;
  activeActionFilter.value = filter;
  void nextTick(() => {
    const host = editorScrollHost.value;
    if (host && typeof host.scrollIntoView === "function") {
      host.scrollIntoView({ block: "start" });
    }
  });
}

watch(editingTarget, (target) => {
  unsupportedReplaceArmed.value = false;
  resetEditorSaveStatus();
  if (target) {
    activeActionFilter.value =
      pendingActionFilterOverride ??
      filterForAction(actionOf(target.button, target.trigger));
    pendingActionFilterOverride = null;
  }
});
watch(selectedAction, () => {
  unsupportedReplaceArmed.value = false;
});
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

/** 预设 chip 是否是当前格子的动作（快捷键按组合键，切换应用按动作类型）。 */
function isActivePresetItem(preset: ShortcutPresetItem): boolean {
  if (preset.action === "app_switcher") {
    return selectedAction.value?.type === "app_switcher";
  }
  return isActivePreset(preset.keys);
}

/** 点击预设 chip：切换应用写入会话动作，其余写入普通快捷键组合。 */
function applyPresetItem(preset: ShortcutPresetItem): void {
  if (preset.action === "app_switcher") {
    applyAction({ type: "app_switcher" });
    return;
  }
  applyAction({ type: "shortcut", chord: { keys: [...preset.keys] } });
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
  // 来自更新版本的动作当前版本不执行，自然也不能连续执行。
  if (!isKnownButtonAction(action)) return false;
  // 切换应用会话内每次触发只前进一项；按住连发等价于系统 Alt+Tab 连续 Tab。
  if (action.type === "app_switcher") return true;
  if (action.type === "page_up" || action.type === "page_down") return true;
  if (action.type === "scroll" || action.type === "mouse_move") return true;
  if (action.type !== "shortcut") return false;
  return action.chord.keys.length === 1 && !REPEAT_MODIFIER_KEYS.has(action.chord.keys[0]);
}

const repeatGroupVisible = computed(() => {
  const target = editingTarget.value;
  if (!target) return false;
  return target.trigger === "single" || target.trigger === "long";
});

type HoldRepeatChoice =
  | "off"
  | "single"
  | "long"
  | "singleHold"
  | "longHold";

/** 选项不可选的原因（null = 可选）；radio 展示与一致性收口共用同一判据。 */
function holdRepeatBlockedReason(
  actions: ButtonActions,
  slot: "single" | "long",
): string | null {
  if (slot === "long") {
    if (!isKnownButtonAction(actions.long)) {
      return t("这个动作来自更新版本，当前版本只保留、不执行，不能设为连续执行");
    }
    if (actions.long.type === "disabled") return t("先给这个按键配置长按动作");
    if (!actionAllowsRepeat(actions.long)) {
      return t("“长按”的动作只执行一次（组合快捷键、打开应用等），不能连续执行；把它改成单个按键后可选");
    }
    return null;
  }
  if (!isKnownButtonAction(actions.single)) {
    return t("这个动作来自更新版本，当前版本只保留、不执行，不能设为连续执行");
  }
  if (actions.single.type === "disabled") return t("先给这个按键配置单击动作");
  if (!actionAllowsRepeat(actions.single)) {
    return t("“单击”的动作只执行一次（组合快捷键、打开应用等），不能连续执行；改成单个按键后可选");
  }
  if (!isKnownButtonAction(actions.long)) {
    return t("这个按键的长按动作来自更新版本，当前版本无法判断能否连续执行；请先升级无线麦");
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

/** 方向键按住只支持左/右；目标动作必须是单个左或右方向键。 */
function directionalHoldBlockedReason(
  actions: ButtonActions,
  slot: "single" | "long",
): string | null {
  const target = slot === "single" ? actions.single : actions.long;
  if (!isKnownButtonAction(target)) {
    return t("这个动作来自更新版本，当前版本只保留、不执行，不能设为按住方向键");
  }
  if (target.type === "disabled") {
    return slot === "single"
      ? t("先给这个按键配置单击方向键")
      : t("先给这个按键配置长按方向键");
  }
  if (
    target.type !== "shortcut" ||
    target.chord.keys.length !== 1 ||
    (target.chord.keys[0] !== "left" && target.chord.keys[0] !== "right")
  ) {
    return slot === "single"
      ? t("“单击”的动作必须是单个左或右方向键")
      : t("“长按”的动作必须是单个左或右方向键");
  }
  return null;
}

const directionalHoldVisible = computed(() => {
  const target = editingTarget.value;
  return (
    !!target &&
    (target.button === "left" || target.button === "right") &&
    (target.trigger === "single" || target.trigger === "long")
  );
});

const singleHoldReason = computed<string | null>(() => {
  const target = editingTarget.value;
  return target
    ? directionalHoldBlockedReason(actionsOf(target.button), "single")
    : null;
});

const longHoldReason = computed<string | null>(() => {
  const target = editingTarget.value;
  return target
    ? directionalHoldBlockedReason(actionsOf(target.button), "long")
    : null;
});

const singleHoldTitle = computed(
  () =>
    singleHoldReason.value ??
    t("按住不放会保持方向键按下，松开才释放"),
);

const longHoldTitle = computed(
  () =>
    longHoldReason.value ??
    t("按住约 0.55 秒后会保持方向键按下，松开才释放"),
);

/** 当前按键的按住状态（按键级，两页同源）。 */
const repeatChoice = computed<HoldRepeatChoice>(() => {
  const target = editingTarget.value;
  if (!target) return "off";
  const actions = actionsOf(target.button);
  if (actions.directionalHold === "single") return "singleHold";
  if (actions.directionalHold === "long") return "longHold";
  const hold = actions.holdRepeat;
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
  if (repeatChoice.value === "singleHold") return singleHoldReason.value;
  if (repeatChoice.value === "longHold") return longHoldReason.value;
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

type HoldPanelOption = {
  id: HoldRepeatChoice;
  title: string;
  description: string;
  reason: string | null;
};
const holdPanelTarget = computed(() => editingTarget.value?.button ?? null);
const holdPanelSupportsHoldDown = computed(
  () => holdPanelTarget.value === "left" || holdPanelTarget.value === "right",
);
const holdPanelOptions = computed<HoldPanelOption[]>(() => {
  const button = holdPanelTarget.value;
  if (!button) return [];
  const single = mappingActionSummary(button, "single");
  const long = mappingActionSummary(button, "long");
  const options: HoldPanelOption[] = [
    {
      id: "single",
      title: t("重复执行单击动作 {action}", { action: single }),
      description: t("按住时重复执行单击动作，松手后停止。"),
      reason: repeatSingleReason.value,
    },
    {
      id: "long",
      title: t("重复执行长按动作 {action}", { action: long }),
      description: t("达到当前长按时间后重复执行长按动作，松手后停止。"),
      reason: repeatLongReason.value,
    },
  ];
  if (holdPanelSupportsHoldDown.value) {
    options.push(
      {
        id: "singleHold",
        title: t("按住 {action}", { action: single }),
        description: t("相当于按住键盘上的 {action}，直到松手。", { action: single }),
        reason: singleHoldReason.value,
      },
      {
        id: "longHold",
        title: t("长按后按住 {action}", { action: long }),
        description: t("达到当前长按时间后按住 {action}，松手时松开。", { action: long }),
        reason: longHoldReason.value,
      },
    );
  }
  return options;
});

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
  if (profileScope && profileScopeSelected.value) {
    statusMessage.value = t("键位方案不支持这个开关。");
    return;
  }
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
  if (actions.directionalHold) {
    const targetAction =
      actions.directionalHold === "single" ? actions.single : actions.long;
    // 更新版本的动作当前不执行；保留用户的按住设置，等信息完整的版本处理。
    if (!isKnownButtonAction(targetAction)) return { actions, notice: null };
    if (directionalHoldBlockedReason(actions, actions.directionalHold) === null) {
      return { actions, notice: null };
    }
    const next = { ...actions };
    delete next.directionalHold;
    return {
      actions: next,
      notice: t("目标动作已变更，方向键按住已关闭"),
    };
  }
  const hold = actions.holdRepeat;
  if (!hold) return { actions, notice: null };
  const slot = hold === "long" ? "long" : "single";
  // holdRepeat 指向的动作来自更新版本：当前版本不知道它能否连续执行，
  // 不能替用户清掉。若指向 single 而 long 未知，只豁免“长按冲突”，
  // single 自身为空/不可重复时仍按正常规则收口。
  if (!isKnownButtonAction(actions.single) && slot === "single") {
    return { actions, notice: null };
  }
  if (!isKnownButtonAction(actions.long) && slot === "long") {
    return { actions, notice: null };
  }
  if (slot === "single" && !isKnownButtonAction(actions.long)) {
    if (actions.single.type !== "disabled" && actionAllowsRepeat(actions.single)) {
      return { actions, notice: null };
    }
    // 落到下面由 holdRepeatBlockedReason 给出原因并清掉无效值。
  }
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
  if (profileScope && profileScopeSelected.value && !profileScopeCanEdit.value) {
    void membershipRoute();
    return;
  }
  const reason =
    choice === "single"
      ? repeatSingleReason.value
      : choice === "long"
        ? repeatLongReason.value
        : choice === "singleHold"
          ? singleHoldReason.value
          : choice === "longHold"
            ? longHoldReason.value
            : null;
  if (reason) {
    statusMessage.value = reason;
    return;
  }
  if (repeatChoice.value === choice) return;
  if (profileScope && profileScopeSelected.value) {
    const save = (): void => {
      setEditorSaveOutcome("saving");
      void profileScope
        .writeHold(target.button, choice === "off" ? null : choice)
        .then(() => setEditorSaveOutcome("saved"))
        .catch((reason) => {
          console.warn("profile hold save failed", reason);
          setEditorSaveOutcome("failed", save);
        });
    };
    save();
    return;
  }
  const actions = { ...actionsOf(target.button) };
  if (choice === "off") {
    delete actions.holdRepeat;
    delete actions.directionalHold;
  } else if (choice === "single" || choice === "long") {
    actions.holdRepeat = choice;
    delete actions.directionalHold;
  } else {
    actions.directionalHold = choice === "singleHold" ? "single" : "long";
    delete actions.holdRepeat;
  }
  mappings.value = {
    ...mappings.value,
    actions: { ...mappings.value.actions, [target.button]: actions },
  };
  const message =
    choice === "off"
      ? t("已关闭“按住时连续执行”")
      : choice === "long"
        ? t("已开启：长按后不松手会继续连续执行")
        : choice === "singleHold"
          ? t("已开启：按住不放会保持方向键按下")
          : choice === "longHold"
            ? t("已开启：长按后不松手会保持方向键按下")
            : t("已开启：按住不放会连续执行");
  const save = (): void => {
    setEditorSaveOutcome("saving");
    void persist(message).then((ok) => {
      if (ok) setEditorSaveOutcome("saved");
      else setEditorSaveOutcome("failed", save);
    });
  };
  save();
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
    ? captureSupport.value?.reason === "admin_required"
      ? t("返回 / 音量+ / 音量−需要全按键支持；全按键支持需要管理员账户登录后开启。")
      : t("返回 / 音量+ / 音量−需要全按键支持；这台电脑暂不支持全按键支持，新版本支持后可配置。")
    : t("{hint}，开启后恢复正常", { hint: triKeyCaptureHint.value }),
);

/**
 * 本机不可用（ARM64，2026-10-07 issue #206）时说不得「开启后恢复正常」——
 * 这台电脑根本开不了。三句都只讲用户能懂的事：哪个开关、为什么、怎么办。
 * 用户可见文案的规范来源是 docs/product-copy.md（§2 名称表 / §7 决定索引）。
 */
const captureUnsupportedTitle = computed(() =>
  captureSupport.value?.reason === "admin_required"
    ? t("全按键支持需要管理员账户；请用管理员账户登录这台电脑后再开启。")
    : t("全按键支持在这台电脑上暂不可用；新版本支持后会恢复正常。"),
);
const captureUnsupportedNote = computed(() =>
  captureSupport.value?.reason === "admin_required"
    ? t("当前登录的账号不是管理员；全按键支持需要管理员账户登录后开启，基础语音与其他按键不受影响。")
    : t("这台电脑暂不支持全按键支持，其他按键不受影响。"),
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
async function persist(
  message?: string,
  reportFailure = true,
): Promise<boolean> {
  const request = ++saveRequest;
  const payload = JSON.parse(JSON.stringify(mappings.value)) as ButtonMappings;
  busy.value = true;
  statusMessage.value = null;
  let ok = false;
  const task = saveQueue.then(async () => {
    try {
      const saved = await saveButtonMappings(payload);
      savedSnapshot.value = JSON.parse(JSON.stringify(saved)) as ButtonMappings;
      writeSwitchCache(saved.enabled, lockSelection.value);
      if (request === saveRequest) {
        mappings.value = saved;
        if (message) statusMessage.value = message;
      }
      ok = true;
    } catch (error) {
      if (request === saveRequest) {
        if (reportFailure) {
          statusMessage.value = operationErrorText(
            "save button mappings failed",
            error,
            "没能保存按键映射，请重试。",
          );
        }
      }
    }
  });
  saveQueue = task;
  await task;
  if (request === saveRequest) busy.value = false;
  return ok;
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
const selectedCaptureModifiers = reactive(new Set<KeyCode>());
const pressedCaptureModifiers = new Set<KeyCode>();
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
    const preheld = await startShortcutCapture();
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
    waitingPreheldRelease.value = preheld.length > 0;
    if (waitingPreheldRelease.value) {
      statusMessage.value = t(
        "检测到仍有按住的按键，请先松开所有按键；松开后即可按新组合，录入将自动开始",
      );
    }
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
  waitingPreheldRelease.value = false;
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
  if (waitingPreheldRelease.value) {
    waitingPreheldRelease.value = false;
    statusMessage.value = null;
  }
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
  applyAction({ type: "shortcut", chord: { keys } });
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
  editorScrollHost.value?.scrollIntoView?.({ behavior: "smooth", block: "start" });
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
  !capturingShortcut.value &&
  !captureStarting.value &&
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
  if (props.initialProfileId && profileScope) {
    await profileScope.load();
    profileScope.select(props.initialProfileId);
  }
  // 两个开关的「就绪」与真值在同一渲染批次生效：本批次才创建开关本体，创建即带
  // 正确 checked，不产生属性变更，因此没有"状态回来后开→关"的滑动过渡。
  mappingsLoaded.value = true;
  if (props.initialEditor) {
    selectedButton.value = props.initialEditor.button;
    editingTarget.value = { ...props.initialEditor };
  }
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
    handleGestureLearningEdge(edge);
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
  releaseGestureLearningSuspension();
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
  <section v-else class="buttons-page mapping-page">
    <div class="mapping-fixed-top">
    <!-- 头部对齐 Mac mappingPage：标题、副标题、自定义按键开关纵向排列，
         遥控器状态最右；顶部整体不参与内容滚动。
         保存按钮移入编辑面板，与"测试一次/关闭"同排。 -->
    <header class="page-header mapping-header">
      <div class="mapping-header-copy">
        <h1>{{ tPage("按键映射") }}</h1>
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
            t("点击按键进行设置")
          }}
        </p>
        <div class="mapping-toggle-row">
          <label class="toggle-row" :title="t('开启后，遥控器按键按本页配置执行动作；关闭时，遥控器保持原始按键行为。')">
            <span>{{ t("自定义按键") }}</span>
            <input
              v-model="enabled"
              type="checkbox"
              class="toggle-input"
              :disabled="busy || !mappingsLoaded"
            />
          </label>
          <label
            class="toggle-row"
            :title="captureSwitchTitle"
          >
            <span>{{ t("全按键支持") }}</span>
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
          <span v-if="captureUnsupported" class="capture-unavailable-note">
            {{ captureUnsupportedNote }}
          </span>
        </div>
      </div>
      <div class="mapping-header-controls">
        <!-- 遥控器信息卡片：型号 + 状态/电量 + 重新连接。 -->
        <div class="remote-info-card" :class="{ connected: connectionReady }">
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

    <div
      v-if="profileScopeAvailable"
      class="profile-scope-bar"
    >
      <div class="scope-chips">
        <button
          type="button"
          class="scope-chip"
          :class="{ selected: profileScopeSelected === null }"
          @click="selectProfileScope(null)"
        >
          {{ t("默认") }}
        </button>
        <button
          v-for="profile in profileScopeProfiles"
          :key="profile.id"
          type="button"
          class="scope-chip"
          :class="{ selected: profileScopeSelected === profile.id }"
          :title="profile.name"
          @click="selectProfileScope(profile.id)"
        >
          {{ profile.name }}
        </button>
      </div>
      <span
        v-if="profileScopeNeedsMembership"
        class="scope-membership-hint"
      >
        {{ profileScopeMembershipHint }}
      </span>
      <MembershipRequiredButton
        v-if="profileScopeNeedsMembership"
        class="scope-membership-cta"
        label="开通会员"
        @open="membershipRoute"
      />
      <button
        v-else
        type="button"
        class="secondary-button"
        :disabled="!profileScopeCanEdit"
        :title="profileSaveDialogNote"
        @click="openProfileSaveDialog"
      >
        {{
          profileScopeSelected === null
            ? t("创建新的键位方案")
            : t("复制为新的键位方案")
        }}
      </button>
    </div>
    <p
      v-if="profileScopeAvailable && profileScopeSelected !== null"
      class="profile-scope-hint"
    >
      {{ t("键位方案只覆盖这些按键：电源键、方向键、确定键、返回键、主页键、音量 +、音量 −；其他按键仍按基础映射执行。") }}
    </p>
    </div>

    <div class="mapping-scroll-area">

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
          'is-profile-disabled':
            profileScopeSelected !== null && !profileScopeSupports(placement.button),
        }"
        :style="{ top: `${cardTop(placement)}px`, width: `${cardWidth}px` }"
        :title="
          captureGated(placement.button)
            ? triKeyGatedTitle
            : profileScopeSelected !== null && !profileScopeSupports(placement.button)
              ? profileScopeUnsupportedTitle
              : undefined
        "
        :aria-disabled="
          captureGated(placement.button) ||
          (profileScopeSelected !== null && !profileScopeSupports(placement.button)) ||
          undefined
        "
        @click="selectButton(placement.button)"
      >
        <div class="mapping-card-title">
          <svg class="mapping-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path
              v-for="(path, index) in buttonIcons[placement.button]"
              :key="index"
              :d="path"
              stroke="currentColor"
              stroke-width="1.8"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          </svg>
          <strong>{{ buttonFullLabels[placement.button] }}</strong>
          <button
            type="button"
            class="mapping-hold-chip"
            :disabled="
              captureGated(placement.button) ||
              (profileScopeSelected !== null && !profileScopeSupports(placement.button))
            "
            :title="`${buttonFullLabels[placement.button]} · ${holdSummaryFor(placement.button)}`"
            @click.stop="openHoldEditor(placement.button)"
          >
            {{ holdSummaryFor(placement.button) }}
          </button>
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
            :disabled="
              captureGated(placement.button) ||
              (profileScopeSelected !== null && !profileScopeSupports(placement.button))
            "
            :title="
              captureGated(placement.button)
                ? triKeyGatedTitle
                : profileScopeSelected !== null && !profileScopeSupports(placement.button)
                  ? profileScopeUnsupportedTitle
                  : actionCellTitle(placement.button, trigger)
            "
            @click.stop="openEditor(placement.button, trigger)"
          >
            <small>{{ buttonTriggerLabel(trigger) }}</small>
            <span
              :class="{
                'direction-arrow': isDirectionSummary(
                  mappingActionSummary(placement.button, trigger),
                ),
              }"
            >
              {{ mappingActionSummary(placement.button, trigger) }}
            </span>
          </button>
        </div>
      </article>

      <article
        class="mapping-card voice-card right"
        :class="{
          active: voiceActive,
          'is-profile-disabled': profileScopeSelected !== null,
        }"
        :style="{ top: `${cardTop(VOICE_PLACEMENT)}px`, width: `${cardWidth}px` }"
        :title="profileScopeSelected !== null ? profileScopeUnsupportedTitle : undefined"
      >
        <div class="mapping-card-title">
          <svg class="mapping-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path :d="VOICE_ICON_FILLED" fill="currentColor" />
            <path
              v-for="(path, index) in VOICE_ICON_STROKES"
              :key="index"
              :d="path"
              stroke="currentColor"
              stroke-width="1.8"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          </svg>
          <strong>{{ t("语音键") }}</strong>
          <span class="badge pending voice-badge" :class="{ active: voiceActive }">{{ t("语音触发键") }}</span>
        </div>
        <p class="voice-note">{{ t("按下开始、松开结束；不参与自定义映射，不加双击/长按延迟。") }}</p>
      </article>
    </div>

    <div v-if="editingTarget" ref="editorScrollHost" class="mapping-editor-host">
    <div class="mapping-action-filter" role="tablist" :aria-label="t('动作分类')">
      <button
        v-for="item in actionFilterItems"
        :key="item.id"
        class="action-filter-chip"
        :class="{ selected: activeActionFilter === item.id }"
        type="button"
        role="tab"
        :aria-selected="activeActionFilter === item.id"
        @click="selectActionFilter(item.id)"
      >
        {{ item.label }}
      </button>
    </div>

    <article v-if="showActionEditor" ref="editorPanel" class="card mapping-editor">
      <div class="card-title-row">
        <div>
          <div class="editor-title-row">
            <svg class="mapping-icon editor-title-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <path
                v-for="(path, index) in buttonIcons[editingTarget.button]"
                :key="index"
                :d="path"
                stroke="currentColor"
                stroke-width="1.8"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
            <h2>{{ buttonFullLabels[editingTarget.button] }}</h2>
            <span class="editor-title-separator"> · </span>
            <span class="editor-trigger-pill">{{ buttonTriggerLabel(editingTarget.trigger) }}</span>
          </div>
          <p v-if="!profileBindingState" class="muted">{{ t("当前：{summary}", { summary: mappingActionSummary(editingTarget.button, editingTarget.trigger) }) }}</p>
          <div v-if="profileBindingState" class="profile-binding-source">
            <template v-if="profileBindingState === 'inherit'">
              <span class="profile-binding-source-label">
                <svg class="profile-binding-link-icon" viewBox="0 0 16 16" aria-hidden="true">
                  <path d="M6.2 9.8 9.8 6.2M6.8 4.6l.8-.8a2.4 2.4 0 0 1 3.4 3.4l-.8.8M9.2 11.4l-.8.8a2.4 2.4 0 0 1-3.4-3.4l.8-.8" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
                </svg>
                {{ t("继承默认方案：{summary}", { summary: mappingActionSummary(editingTarget.button, editingTarget.trigger) }) }}
              </span>
            </template>
            <template v-else>
              <span class="profile-binding-source-label">
                {{
                  profileBindingState === "disabled"
                    ? t("本方案禁用")
                    : profileBindingState === "unavailable"
                      ? t("不可用")
                      : t("本方案 · {summary}", {
                          summary: mappingActionSummary(
                            editingTarget.button,
                            editingTarget.trigger,
                          ),
                        })
                }}
              </span>
              <button
                class="link-button"
                type="button"
                :disabled="restoringProfileBinding || !profileScopeCanEdit"
                @click="restoreProfileBinding"
              >
                {{ restoringProfileBinding ? t("恢复中…") : t("恢复继承默认") }}
              </button>
            </template>
          </div>
        </div>
        <div class="button-row">
          <!-- 全按键支持开关已移至页面头部（与「启用自定义按键功能」并排，
               2026-09-27），编辑面板只保留按键级操作。 -->
          <span class="editor-save-status" :class="editorSaveTone">{{ editorSaveLabel }}</span>
          <button
            v-if="editorSaveOutcome === 'failed' || editorSaveOutcome === 'unknown'"
            class="link-button editor-save-retry"
            type="button"
            @click="retryEditorSave"
          >
            {{ t("重试") }}
          </button>
          <label class="editor-disable-toggle" :class="{ active: disableActionActive }">
            <span>{{ t("停用此动作") }}</span>
            <input
              class="toggle-input"
              type="checkbox"
              :checked="disableActionActive"
              :disabled="busy || !editorCanWrite"
              @change="toggleDisableAction"
            />
          </label>
        </div>
      </div>
      <div class="action-sections">
        <p v-if="capabilityNote" class="muted editor-note capability-note">{{ capabilityNote }}</p>
        <p
          v-if="selectedAction && !isKnownButtonAction(selectedAction)"
          class="muted editor-note unsupported-action-note"
        >
          {{ t("这个动作来自更新版本：当前版本保留它但不执行。选择下面的任意动作后，再选一次才会替换它；如需使用原动作，请升级无线麦。") }}
        </p>
        <section v-for="group in visibleShortcutGroups" :key="group.label" class="action-section">
          <h4 class="action-section-title">{{ group.label }}</h4>
          <div class="preset-grid">
            <!-- 芯片显示实际按键组合（组合在不同 App 里语义不同，功能描述
                 只作悬停提示，避免把 Ctrl+C 一类写成"复制"造成误判）。 -->
            <button
              v-for="preset in group.items"
              :key="preset.label"
              class="chip"
              :class="{ selected: isActivePresetItem(preset) }"
              type="button"
              :title="shortcutPresetHint(preset)"
              @click="applyPresetItem(preset)"
            >
              {{ preset.display ?? chordLabel({ keys: preset.keys }) }}
            </button>
          </div>
        </section>

        <!-- 自定义按键、输入框是两类独立候选（2026-11-24 对齐 Mac 2.0.10）。 -->
        <section v-if="showCustomShortcutSection" class="action-section">
          <h4 class="action-section-title">{{ t("自定义按键") }}</h4>
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

        <section v-if="showInputSection" class="action-section">
          <h4 class="action-section-title">{{ t("输入框") }}</h4>
          <div class="custom-shortcut-row">
            <button
              class="chip"
              :class="{ selected: selectedAction?.type === 'focus_input' }"
              type="button"
              :title="t('把键盘焦点放到当前前台应用的输入框（不切换应用、不输入文字）')"
              @click="applyAction({ type: 'focus_input' })"
            >
              {{ t("聚焦输入框") }}
            </button>
            <span class="muted input-section-hint">
              {{ t("学习指定 App 的输入框请去「聚焦与翻页」页面") }}
            </span>
          </div>
        </section>

        <section v-if="showAppSection" class="action-section">
          <h4 class="action-section-title">{{ t("打开 APP") }}</h4>
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
            <button
              class="chip open-custom-app"
              :class="{ selected: Boolean(focusTarget && !focusTargetIsBuiltInPreset) }"
              type="button"
              :aria-expanded="customAppsExpanded"
              aria-controls="custom-app-group"
              @click="toggleCustomApps"
            >
              {{ t("打开自定义 APP") }}
            </button>
          </div>
          <div v-if="customAppsExpanded" id="custom-app-group" ref="customAppGroupEl" class="custom-app-group">
            <p class="action-section-title target-app-title">{{ t("目标 APP") }}</p>
            <div class="preset-grid app-library">
              <button
                v-for="app in customApps"
                :key="app.path"
                class="chip custom-app"
                type="button"
                :class="{ selected: openAppTargetOf(editingTarget.button, editingTarget.trigger) === app.path }"
                :title="app.name"
                @click="applyAction({ type: 'open_app', target: app.path })"
              >
                {{ openAppLabel(app.name) }}
              </button>
              <!-- 添加自定义 APP 的唯一入口，跟在目标 APP 之后。 -->
              <button
                class="chip add-app"
                type="button"
                :title="t('扫描本机应用，或选择任意程序或快捷方式')"
                @click="appPickerError = null; appPickerOpen = true"
              >
                {{ t("添加应用") }}
              </button>
            </div>
          </div>
          <div
            v-if="showAppFocusStrategy && focusTarget && (!focusTargetIsBuiltInPreset || focusTargetHasExplicitProfile)"
            class="focus-profile-summary"
          >
            <span class="muted">
              {{ t("聚焦：{summary}", { summary: focusSummary }) }} ·
              {{ t("翻页：{status}", { status: focusScrollStatus ?? t("需学习") }) }}
            </span>
            <button class="link-button" type="button" @click="configureFocusProfile">
              {{ t("去配置") }}
            </button>
          </div>
          <p v-if="lastFocusNotice" class="muted focus-result-message">{{ lastFocusNotice }}</p>
        </section>

        <!-- 组合动作（自定义动作；2026-10-15）：把已有动作绑定到本按键。
             入口显隐跟随 COMBINATION_ACTIONS_ENABLED，隐藏时已绑定动作照常执行。 -->
        <section v-if="COMBINATION_ACTIONS_ENABLED && showMacroSection" class="action-section">
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

        <section v-if="commonPhrasesAvailable && showCommonPhrasesSection" class="action-section">
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
      <div v-if="showMouseSection" class="action-sections mouse-action-sections">
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
          <div class="page-action-row">
            <button
              v-for="item in [
                { type: 'page_up' as const, label: '向上翻页' },
                { type: 'page_down' as const, label: '向下翻页' },
                { type: 'scroll_to_top' as const, label: '滚到顶部' },
                { type: 'scroll_to_bottom' as const, label: '滚到底部' },
              ]"
              :key="item.type"
              class="chip"
              :class="{ selected: selectedAction?.type === item.type }"
              type="button"
              :title="t('使用聚焦与翻页里学习的滚动区域；没有记录时自动寻找可滚动容器')"
              @click="applyAction({ type: item.type })"
            >
              {{ t(item.label) }}
            </button>
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
        <div v-if="okClickVisible" class="ok-click-setting">
          <label class="toggle-row ok-click-toggle" :title="t('刚用遥控器移动过光标时，按 OK 会点击光标位置')">
            <span>{{ t("移动光标后按 OK 点击") }}</span>
            <input
              class="toggle-input"
              type="checkbox"
              :checked="okClickChecked"
              :disabled="busy"
              @change="toggleOkContextClick"
            />
          </label>
          <p class="muted ok-click-hint">
            {{ t("刚用遥控器移动过光标（5 秒内），按 OK 会点击光标位置。这时 OK 的单击、双击、长按都不会执行。") }}
          </p>
        </div>
      </div>
      <div v-if="showInlineHoldSection" class="action-sections repeat-action-sections">
        <section class="action-section repeat-group">
          <h4 class="action-section-title">{{ t("按住行为") }}</h4>
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
            <button
              v-if="directionalHoldVisible"
              class="chip"
              :class="{ selected: repeatChoice === 'singleHold' }"
              type="button"
              :title="singleHoldTitle"
              :disabled="busy || (!!singleHoldReason && repeatChoice !== 'singleHold')"
              @click="selectHoldRepeat('singleHold')"
            >
              {{ t("按住单击方向") }}
            </button>
            <button
              v-if="directionalHoldVisible"
              class="chip"
              :class="{ selected: repeatChoice === 'longHold' }"
              type="button"
              :title="longHoldTitle"
              :disabled="busy || (!!longHoldReason && repeatChoice !== 'longHold')"
              @click="selectHoldRepeat('longHold')"
            >
              {{ t("长按后按住方向") }}
            </button>
          </div>
          <p v-if="repeatPageReason" class="muted repeat-hint">{{ repeatPageReason }}</p>
        </section>
      </div>
    </article>

    <article
      v-if="showStandaloneHoldPanel && holdPanelTarget"
      class="card mapping-editor mapping-hold-panel"
    >
      <div class="card-title-row">
        <div>
          <div class="editor-title-row">
            <svg class="mapping-icon editor-title-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <path
                v-for="(path, index) in buttonIcons[holdPanelTarget]"
                :key="index"
                :d="path"
                stroke="currentColor"
                stroke-width="1.8"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
            <h2>{{ buttonFullLabels[holdPanelTarget] }}</h2>
            <span class="editor-title-separator"> · </span>
            <span class="editor-trigger-pill">{{ t("按住时") }}</span>
          </div>
          <p class="hold-panel-subtitle">{{ t("选择一种行为") }}</p>
        </div>
        <div class="button-row">
          <span class="editor-save-status" :class="editorSaveTone">{{ editorSaveLabel }}</span>
          <button
            class="secondary-button"
            type="button"
            :disabled="busy || repeatChoice === 'off'"
            @click="selectHoldRepeat('off')"
          >
            {{ t("清除设置") }}
          </button>
        </div>
      </div>

      <div class="hold-mode-grid">
        <button
          v-for="option in holdPanelOptions"
          :key="option.id"
          class="hold-mode-card"
          :class="{ selected: repeatChoice === option.id }"
          type="button"
          :title="option.reason ?? option.description"
          :disabled="busy || (!!option.reason && repeatChoice !== option.id)"
          @click="selectHoldRepeat(option.id)"
        >
          <span class="hold-mode-radio" aria-hidden="true"></span>
          <span class="hold-mode-title">{{ option.title }}</span>
          <span class="hold-mode-description">{{ option.description }}</span>
          <span v-if="option.id === 'long' || option.id === 'longHold'" class="hold-mode-description">
            {{ t("长按时间采用当前已保存的按键速度设置。") }}
          </span>
          <span v-if="option.reason" class="hold-mode-reason">{{ option.reason }}</span>
        </button>
      </div>
    </article>

    <article v-if="showSpeedPanel" class="card mapping-editor mapping-speed-panel">
      <h2 class="speed-panel-title">{{ t("按键速度") }}</h2>
      <p class="speed-panel-subtitle">{{ t("本型号的全部普通按键共用以下识别时间。") }}</p>

      <h3 class="speed-section-title">{{ t("识别时间") }}</h3>
      <div class="speed-slider-grid">
        <label class="speed-slider">
          <span class="speed-slider-head">
            <span>{{ t("双击等待时间") }}</span>
            <output>{{ gestureTimingDraft.doubleClickMs }} ms</output>
          </span>
          <span class="speed-slider-track">
            <span>50</span>
            <input
              v-model.number="gestureTimingDraft.doubleClickMs"
              type="range"
              @input="markGestureTimingEdited"
              min="50"
              max="500"
              step="50"
              :aria-label="t('双击等待时间')"
            />
            <span>500</span>
          </span>
        </label>
        <label class="speed-slider">
          <span class="speed-slider-head">
            <span>{{ t("长按判定时间") }}</span>
            <output>{{ gestureTimingDraft.longPressMs }} ms</output>
          </span>
          <span class="speed-slider-track">
            <span>300</span>
            <input
              v-model.number="gestureTimingDraft.longPressMs"
              type="range"
              @input="markGestureTimingEdited"
              min="300"
              max="1000"
              step="50"
              :aria-label="t('长按判定时间')"
            />
            <span>1,000</span>
          </span>
        </label>
      </div>
      <div class="speed-save-row">
        <span class="speed-save-hint">{{ gestureTimingHint }}</span>
        <button
          class="primary-button"
          type="button"
          :disabled="!gestureTimingCanSave"
          @click="saveGestureTiming"
        >
          {{ gestureTimingSaveState === "saving" ? t("保存中…") : t("保存") }}
        </button>
      </div>

      <section class="speed-learning-card">
        <div class="speed-learning-head">
          <h3 class="speed-section-title">{{ t("学习你的单击、双击和长按") }}</h3>
          <span class="speed-learning-status">{{ gestureLearningStatusText }}</span>
        </div>
        <p class="speed-panel-subtitle">
          {{ t("需要依次完成 3 次操作：单击、双击和长按。学习结果会填入下面的识别时间。") }}
        </p>
        <div class="speed-learning-grid">
          <div class="speed-learning-step" :class="{ current: gestureLearning.step === 'single' }">
            <strong>{{ t("1/3 · 单击一次") }}</strong>
            <span>{{ t("按下后立即松开") }}</span>
            <output>{{ gestureLearning.single === null ? t("等待中") : `${gestureLearning.single} ms` }}</output>
          </div>
          <div class="speed-learning-step" :class="{ current: gestureLearning.step === 'double' }">
            <strong>{{ t("2/3 · 双击一次") }}</strong>
            <span>{{ t("连续按下两次") }}</span>
            <output>{{ gestureLearning.double === null ? t("等待中") : `${gestureLearning.double} ms` }}</output>
          </div>
          <div class="speed-learning-step" :class="{ current: gestureLearning.step === 'long' }">
            <strong>{{ t("3/3 · 长按一次") }}</strong>
            <span>{{ t("按住后自然松开") }}</span>
            <output>{{ gestureLearning.long === null ? t("等待中") : `${gestureLearning.long} ms` }}</output>
          </div>
        </div>
        <p v-if="!gestureLearningCanStart" class="speed-learning-unavailable">
          {{ t("连接遥控器后才能学习按键速度。") }}
        </p>
        <div class="speed-learning-actions">
          <button
            class="secondary-button"
            type="button"
            :disabled="!gestureLearningCanStart || gestureLearningActive || gestureLearningStarting || gestureLearning.step === 'idle'"
            @click="startGestureLearning"
          >
            {{ t("重新学习") }}
          </button>
          <button
            class="primary-button"
            type="button"
            :disabled="!gestureLearningCanStart || gestureLearningActive || gestureLearningStarting || gestureLearning.step === 'complete'"
            @click="startGestureLearning"
          >
            {{ t("开始学习") }}
          </button>
        </div>
      </section>
    </article>
    </div>

    <footer class="mapping-footer">
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
    </div>

    <p v-if="statusMessage" class="operation-message mapping-status">{{ statusMessage }}</p>
    <p v-if="mappingSnapshot?.lastError" class="error-text">{{ t("按键监听遇到问题，请到连接页重新连接遥控器后再试。") }}</p>
    <p v-if="(mappingSnapshot?.uipiBlocked ?? 0) > 0" class="operation-message mapping-status">
      {{ t("有按键没能送达：当前窗口的应用以管理员身份运行时，Windows 会拦截无线麦发送的按键。请取消它的“以管理员身份运行”后再试。") }}
    </p>
    <RegisteredAppsDialog v-if="appPickerOpen" :known-apps="mappings.applications ?? []" :saving="busy" :save-error="appPickerError" @close="appPickerOpen = false; unsupportedReplaceArmed = false" @select="selectApp" />
    <EnhancedCaptureConfirmDialog v-if="showCaptureConfirm" @confirm="confirmCaptureDialog" @close="closeCaptureDialog" />
    <div v-if="showProfileSaveDialog" class="profile-save-overlay" @click.self="closeProfileSaveDialog">
      <div class="profile-save-dialog" role="dialog" aria-modal="true">
        <h2>{{ profileSaveDialogTitle }}</h2>
        <label class="profile-save-field">
          <span>{{ t("方案名称") }}</span>
          <input
            v-model="profileNameDraft"
            maxlength="20"
            :placeholder="t('按键方案')"
            @keydown.enter.prevent="submitProfileSave"
          />
        </label>
        <p class="muted profile-save-note">{{ profileSaveDialogNote }}</p>
        <p class="muted">{{ t("覆盖范围") }}</p>
        <div class="profile-save-scope">
          <span
            v-for="button in PROFILE_SCOPE_BUTTONS"
            :key="button"
            class="profile-save-scope-chip"
          >
            {{ buttonFullLabels[button] }}
          </span>
        </div>
        <p class="muted">
          {{ t("来源 {name}", { name: profileSaveSourceName }) }}
        </p>
        <div class="profile-save-actions">
          <button type="button" class="secondary-button" :disabled="profileSaveBusy" @click="closeProfileSaveDialog">
            {{ t("取消") }}
          </button>
          <button type="button" class="primary-button" :disabled="profileSaveBusy || !profileNameDraft.trim()" @click="submitProfileSave">
            {{ profileSaveConfirmLabel }}
          </button>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
/* 2026-11-22 Andy：内容不足一屏时把底部操作栏贴到面板底部，避免状态提示
   自动关闭后底部留下大块空白。 */
.buttons-page-main {
  min-height: 100%;
  display: flex;
  flex-direction: column;
  padding-bottom: 8px;
}
.mapping-bottom {
  margin-top: auto;
  flex: 0 0 auto;
}
/* 全按键支持开关的终值就绪前占位符：与 toggle-input 同尺寸（34x20），
   避免就绪后开关创建时标题行宽度跳动（同「启动行为」页做法）。 */
.toggle-placeholder { width: 34px; height: 20px; flex: none; }
/* 「连续执行」：编辑面板内的按键级 radio 三选一（2026-10-05），
   2026-10-23 起独立成组、放在鼠标动作下面。 */
.repeat-options { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 6px; }
.repeat-hint { margin: 6px 0 0; font-size: 12px; }
/* OK 键「移动光标后按 OK 点击」：按键级开关 + 常驻说明句（2026-10-06）。 */
.ok-click-setting { display: flex; flex-direction: column; gap: 6px; }
.ok-click-toggle { margin-top: 0; }
.ok-click-hint { margin: 0; font-size: 12px; }
.mouse-action-row { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; margin-top: 6px; }
.mouse-action-group { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; }
.page-action-row { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; margin-top: 6px; }
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

/* 「打开自定义 APP」展开后的目标 APP 组：与预设应用分开，只有这里的目标才有
   聚焦与翻页配置。 */
.open-custom-app { border-style: dashed; }
.custom-app-group {
  margin-top: 10px;
  padding: 10px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface-subtle);
}
.target-app-title { margin: 0 0 6px; font-size: 12.5px; color: var(--muted); }

/* 「聚焦方式」只读摘要；可写控件统一在 APP 适配页。 */
.focus-profile-summary {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 9px;
  padding: 7px 9px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface-subtle);
  font-size: 12px;
}
.focus-profile-summary .link-button {
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--accent-text);
  font: inherit;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
}
.focus-result-message { margin: 8px 0 0; font-size: 12px; }
/* 按键页方案作用域：显示当前动作来自默认方案还是本方案。 */
.profile-binding-source {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 7px;
  font-size: 12px;
  color: var(--muted);
}
.profile-binding-source-label {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
}
.profile-binding-link-icon { width: 14px; height: 14px; flex: none; }
.profile-binding-source .link-button {
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--accent-text);
  font: inherit;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
}
.profile-binding-source .link-button:disabled { cursor: default; opacity: 0.5; }
/* 编辑面板右上角：自动保存状态 + Mac 同款「停用此动作」开关。 */
.editor-title-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.editor-title-row h2 { margin: 0; }
.editor-title-separator {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0 0 0 0);
  white-space: nowrap;
}
.editor-title-icon { width: 18px; height: 18px; color: var(--accent); }
.editor-trigger-pill {
  padding: 3px 9px;
  border-radius: 999px;
  color: var(--accent-text);
  background: var(--accent-surface);
  font-size: 12px;
  font-weight: 600;
}
.card-title-row .button-row { align-items: center; }
/* 动作分类胶囊：位置和顺序对齐 Mac 按键页。 */
.mapping-editor-host { display: flex; flex-direction: column; gap: 8px; }
/* 全局 .mapping-editor 自带 10px 上边距；分类行与编辑卡之间的实际间距
   只保留 host 的 8px gap，避免 8 + 10 变成 18px。 */
.mapping-editor-host > .mapping-editor { margin-top: 0; }
:deep(.scope-membership-cta.membership-required-button),
:deep(.common-phrases-cta.membership-required-button) {
  height: 26px;
  min-height: 26px;
  padding: 0 12px;
  border-radius: 6px;
  background: var(--accent);
  font-size: 13px;
  line-height: 1;
}

:deep(.scope-membership-cta .membership-required-icon),
:deep(.common-phrases-cta .membership-required-icon) {
  width: 16px;
  height: 16px;
}

.mapping-action-filter {

  display: flex;
  gap: 6px;
  /* 2026-11-24 用户定稿：分类行不再使用灰色底板；上方留白 8px，
     与下方编辑卡的间距由 host 的 8px gap 提供。 */
  padding: 8px 0 0;
  background: transparent;
  overflow-x: auto;
  scrollbar-width: none;
}
.mapping-action-filter::-webkit-scrollbar { display: none; }
.action-filter-chip {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex: none;
  min-height: 32px;
  padding: 0 12px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: transparent;
  color: var(--text-secondary);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
  white-space: nowrap;
}
.action-filter-chip:hover { background: var(--surface-hover); }
.action-filter-chip.selected {
  color: var(--text-on-accent);
  border-color: var(--accent);
  background: var(--accent);
  font-weight: 600;
}
.editor-save-status {
  display: inline-flex;
  align-items: center;
  min-height: 20px;
  font-size: 12px;
  line-height: 1;
  color: var(--muted);
  white-space: nowrap;
}
.editor-save-status.saved {
  padding: 4px 8px;
  border-radius: 999px;
  color: #fff;
  background: #12704a;
}
html[data-theme="dark"] .editor-save-status.saved { background: #1a7a52; }
.editor-save-status.error { color: var(--error-text); }
.editor-save-retry {
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--accent-text);
  font: inherit;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
}
.editor-disable-toggle {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 5px 10px;
  border-radius: 10px;
  background: rgba(255, 59, 48, 0.08);
  color: var(--text-primary);
  font-size: 13px;
  font-weight: 600;
  white-space: nowrap;
}
html[data-theme="dark"] .editor-disable-toggle { background: rgba(255, 69, 58, 0.14); }
.editor-disable-toggle .toggle-input:checked { background: var(--delete-text); }
.hold-panel-subtitle { margin: 4px 0 0; color: var(--muted); font-size: 13px; }
.input-section-hint { font-size: 12px; line-height: 1.45; }
.hold-mode-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}
.hold-mode-card {
  display: grid;
  grid-template-columns: 16px minmax(0, 1fr);
  column-gap: 8px;
  align-content: start;
  align-items: start;
  min-height: 116px;
  padding: 12px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface-subtle);
  color: var(--text-primary);
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.hold-mode-card:hover:not(:disabled) { border-color: var(--accent-border); background: var(--accent-surface); }
.hold-mode-card.selected {
  border-color: var(--accent-border);
  background: var(--accent-surface);
  box-shadow: inset 0 0 0 1px var(--accent-border);
}
.hold-mode-card:disabled { cursor: default; opacity: 0.62; }
.hold-mode-radio {
  width: 15px;
  height: 15px;
  margin-top: 2px;
  border: 1.5px solid var(--muted);
  border-radius: 50%;
  box-sizing: border-box;
}
.hold-mode-card.selected .hold-mode-radio {
  border: 4px solid var(--accent);
  background: var(--surface-chip);
}
.hold-mode-title { font-size: 13px; font-weight: 600; line-height: 1.35; }
.hold-mode-description {
  grid-column: 2;
  margin-top: 4px;
  color: var(--muted);
  font-size: 12px;
  line-height: 1.45;
}
.hold-mode-reason {
  grid-column: 2;
  margin-top: 5px;
  color: var(--warning-text);
  font-size: 12px;
  line-height: 1.45;
}
@media (max-width: 720px) {
  .hold-mode-grid { grid-template-columns: 1fr; }
}
.speed-panel-title { margin: 0 0 4px; font-size: 20px; }
.speed-panel-subtitle { margin: 0; color: var(--muted); font-size: 13px; line-height: 1.5; }
.speed-section-title { margin: 16px 0 0; font-size: 15px; }
.speed-slider-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 24px;
  margin-top: 10px;
}
.speed-slider { display: flex; flex-direction: column; gap: 8px; }
.speed-slider-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
  font-size: 13px;
  font-weight: 600;
}
.speed-slider-head output { font-variant-numeric: tabular-nums; font-weight: 600; }
.speed-slider-track {
  display: flex;
  align-items: center;
  gap: 10px;
  color: var(--muted);
  font-size: 12px;
}
.speed-slider-track input { flex: 1; accent-color: var(--accent); }
.speed-save-row {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-top: 14px;
  padding: 10px;
  border-radius: 9px;
  background: rgba(0, 0, 0, 0.035);
}
html[data-theme="dark"] .speed-save-row { background: rgba(255, 255, 255, 0.05); }
.speed-save-hint { flex: 1; color: var(--muted); font-size: 12px; line-height: 1.45; }
.speed-learning-card {
  margin-top: 16px;
  padding: 12px;
  border: 1px solid var(--border);
  border-radius: 12px;
  background: var(--surface-subtle);
}
.speed-learning-head { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; }
.speed-learning-head .speed-section-title { margin: 0; }
.speed-learning-status { color: var(--accent-text); font-size: 12px; font-weight: 600; }
.speed-learning-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 10px;
  margin-top: 10px;
}
.speed-learning-step {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-height: 76px;
  padding: 10px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface-chip);
  color: var(--muted);
  font-size: 12px;
}
.speed-learning-step strong { color: var(--text-primary); font-size: 13px; }
.speed-learning-step output { margin-top: auto; font-variant-numeric: tabular-nums; color: var(--text-primary); }
.speed-learning-step.current { border-color: var(--accent-border); box-shadow: inset 0 0 0 1px var(--accent-border); }
.speed-learning-unavailable { margin: 10px 0 0; color: var(--warning-text); font-size: 12px; }
.speed-learning-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 10px; }
@media (max-width: 720px) {
  .speed-slider-grid, .speed-learning-grid { grid-template-columns: 1fr; }
}
</style>
