<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import BatteryIndicator from "../components/BatteryIndicator.vue";
import EnhancedCaptureConfirmDialog from "../components/EnhancedCaptureConfirmDialog.vue";
import { VOICE_HOTKEY_CUSTOM_CAPTURE_ENABLED } from "../lib/feature-flags";
import { reportFrontendEvent } from "../lib/frontend-diagnostics";
import type {
  AudioEndpoint,
  AudioSnapshot,
  ConnectionSnapshot,
  KeyChord,
  PairedRemote,
  Rc003TaskStatus,
  RuntimeSnapshot,
  VoiceInputTool,
} from "../lib/bridge";
import {
  audioPhaseLabel,
  chordLabel,
  connectRemote,
  connectionPhaseLabel,
  disableRc003Capture,
  disconnectRemote,
  enableRc003Capture,
  getAudioSnapshot,
  getConnectionSnapshot,
  getOtherVoiceHotkey,
  getRc003TaskStatus,
  getVoiceHoldHotkey,
  getVoiceInputTool,
  getVokieInstallation,
  isRecommendedVoiceEndpoint,
  listAudioEndpoints,
  openVbCableDownloadPage,
  launchVokie,
  openVokieHomepage,
  remoteModelLabel,
  scanPairedRemotes,
  selectAudioEndpoint,
  setOtherVoiceHotkey,
  setVoiceHoldHotkey,
  setVoiceInputTool,
  startShortcutCapture,
  stopShortcutCapture,
  subscribeShortcutCaptureEdges,
  voiceHoldHotkeyLabel,
  type KeyCode,
  type ShortcutCaptureEdge,
} from "../lib/bridge";

const props = defineProps<{ runtime: RuntimeSnapshot | null }>();

const emptyConnection = (): ConnectionSnapshot => ({
  phase: "idle",
  remoteName: null,
  remoteModel: "unknown",
  capabilities: null,
  voiceState: "idle",
  decodedSamples: 0,
  generation: 0,
  reconnectAttempt: 0,
  powerNotificationsAvailable: false,
  lastError: null,
});

const emptyAudio = (): AudioSnapshot => ({
  phase: "unsupported",
  selectedEndpointId: null,
  selectedEndpointName: null,
  queuedSamples: 0,
  submittedSamples: 0,
  generation: 0,
  lastError: null,
});

const connection = ref<ConnectionSnapshot>(emptyConnection());
const audio = ref<AudioSnapshot>(emptyAudio());
const scanning = ref(false);
const connectingDeviceId = ref("");
const disconnecting = ref(false);
const devices = ref<PairedRemote[]>([]);
const scanMessage = ref("");
const operationMessage = ref("");
const audioEndpoints = ref<AudioEndpoint[]>([]);
const showEndpointList = ref(false);
const scanningAudio = ref(false);
const audioScanComplete = ref(false);
const selectingEndpointId = ref("");
const openingVbCablePage = ref(false);
const audioMessage = ref("");
const voiceHotkey = ref<KeyChord | null>(null);
const savingVoiceHotkey = ref(false);
const voiceHotkeyMessage = ref("");
const capturingVoiceHotkey = ref(false);
const captureStartingVoiceHotkey = ref(false);
/** 录入开始时仍有 preheld 键按住：后端吞键但不投递边沿，直到全部松开。 */
const waitingPreheldRelease = ref(false);
const voiceCaptureDisplay = ref<KeyCode[]>([]);
/** 当前选择的输入工具；null = 尚未读取（或从未选择过，正在推断）。 */
const voiceInputTool = ref<VoiceInputTool | null>(null);
const savingVoiceInputTool = ref(false);
/**
 * 正在为哪个工具落"按住说话快捷键"（含两段 IPC 全程）。见 toolChordPill：
 * 这期间状态胶囊按乐观态显示，避免切换工具的瞬间闪一帧"未同步"黄标。
 */
const applyingTool = ref<VoiceInputTool | null>(null);
/**
 * Vokie 检测状态（2026-10-01 Andy 需求 + 快捷键冲突处理）：
 * - `installed === false`：Vokie 面板显示官网入口；
 * - 已安装但没运行：提示“没有运行”——没运行就不会响应右 Alt；
 * - `running === true`：豆包面板给出冲突提示（两者都用右 Alt）。
 * 检测在 Rust 侧只读完成（卸载表 / 开始菜单 / App Paths + 进程名），只回布尔值、不回路径。
 */
const vokieInstalled = ref<boolean | null>(null);
const vokieRunning = ref<boolean | null>(null);
const checkingVokie = ref(false);
const openingVokiePage = ref(false);
const launchingVokie = ref(false);
const vokieCheckMessage = ref("");
let pollTimer: ReturnType<typeof setInterval> | undefined;
let unlistenVoiceCapture: (() => void) | null = null;
let voiceCaptureTimeout: number | null = null;
/**
 * 落盘稳定窗口：外部钩子（微信输入法等）会吞掉完成键的物理边沿、随后把整个
 * 组合以注入副本重放（见 Bugs/2026-09-27-ime-chord-hook-eats-active-hotkey-capture.md）。
 * 副本可能晚于用户物理松开到达，因此"全部松开"后不立即落盘，先等一个短窗口；
 * 窗口内又出现按下沿则取消并重新等待。
 */
let voiceCaptureSettleTimeout: number | null = null;
let voiceCaptureRequestId = 0;
let unmounted = false;
const voiceCapturePressed = new Set<KeyCode>();
/** 本次录入会话按过的全部按键（按首次按下顺序，去重）。 */
const voiceCaptureEverPressed: KeyCode[] = [];
let voiceCapturedKeys: KeyCode[] | null = null;

/** 按住说话快捷键默认值（v1 固定，适配微信输入法的默认语音热键）。 */
const DEFAULT_VOICE_HOTKEY_KEYS: KeyCode[] = ["left_control", "left_windows"];

/**
 * 输入工具 → "按住说话快捷键"（2026-09-30 连接页改版的设计契约）：
 * 选工具即自动落这个组合，用户不需要理解快捷键本身。
 * `other` 不预设——由用户在"其他工具"面板里自己选（右 Alt / 左 Alt / 不按键）。
 */
const VOICE_TOOL_CHORDS: Record<VoiceInputTool, KeyCode[] | null> = {
  doubao: ["right_alt"],
  wechat: [...DEFAULT_VOICE_HOTKEY_KEYS],
  vokie: ["right_alt"],
  other: null,
};

/** 工具卡片顺序：豆包第一（2026-09-30 Andy 要求），Vokie 在“其他工具”之前（2026-10-01 新增）。 */
const TOOL_CARDS: Array<{ id: VoiceInputTool; name: string; note: string }> = [
  { id: "doubao", name: "豆包输入法", note: "要开启“支持更多输入工具”" },
  { id: "wechat", name: "微信输入法", note: "用默认语音键，最省事" },
  { id: "vokie", name: "Vokie", note: "流式显示，智能整理，不占用输入法" },
  { id: "other", name: "其他工具", note: "自己指定按键" },
];

/** "其他工具"可选的按键（报告层合成白名单 = 左右 Alt；不按键 = 只收语音）。 */
const OTHER_CHORD_OPTIONS: Array<{ id: string; keys: KeyCode[]; label: string }> = [
  { id: "right_alt", keys: ["right_alt"], label: "右 Alt" },
  { id: "left_alt", keys: ["left_alt"], label: "左 Alt" },
  { id: "none", keys: [], label: "不按键" },
];

const CAPTURE_MODIFIER_KEYS: ReadonlySet<KeyCode> = new Set<KeyCode>([
  "left_control",
  "right_control",
  "left_shift",
  "right_shift",
  "left_alt",
  "right_alt",
  "left_windows",
  "right_windows",
]);

const activeVoiceHotkeyKeys = computed(() =>
  voiceHotkey.value ? [...voiceHotkey.value.keys].sort().join("+") : "",
);

function chordKeysActive(keys: string[]): boolean {
  return [...keys].sort().join("+") === activeVoiceHotkeyKeys.value;
}

function otherChordActive(option: { keys: KeyCode[] }): boolean {
  return option.keys.length ? chordKeysActive(option.keys) : activeVoiceHotkeyKeys.value === "";
}

/** 工具卡片/豆包面板显示的"已自动设置"判据：当前快捷键 == 该工具要求的组合。 */
function toolChordActive(tool: VoiceInputTool): boolean {
  const keys = VOICE_TOOL_CHORDS[tool];
  return keys !== null && chordKeysActive(keys);
}

/**
 * 状态胶囊（"已自动设置 / 未同步，点左侧卡片重设"）的判据。
 *
 * 切换工具是两段异步 IPC（落工具选择 → 写按住说话快捷键），期间 `voiceHotkey`
 * 还是上一代的组合——纯按 `toolChordActive` 判定会闪一帧"未同步"黄标
 * （2026-10-01 Andy 实测：快速在微信/豆包之间切换时黄标闪现）。正在为这个工具
 * 落快捷键时按乐观态显示"已自动设置"；失败时 applyingTool 清空，回到真实判定
 * （写失败的真实状态由 voiceHotkeyMessage + 重新读取的 voiceHotkey 呈现）。
 */
function toolChordPill(tool: VoiceInputTool): { ok: boolean; label: string } {
  if (applyingTool.value === tool) {
    return { ok: true, label: "已自动设置" };
  }
  return toolChordActive(tool)
    ? { ok: true, label: "已自动设置" }
    : { ok: false, label: "未同步，点左侧卡片重设" };
}

/**
 * 老配置（从未选过工具）按当前快捷键推断一次：左 Ctrl + 左 Win = 微信、
 * 右 Alt = 豆包（Vokie 与豆包用同一个组合，只能靠已保存的工具区分）、
 * 其余（左 Alt / 不按键 / 自定义组合）= 其他工具。
 */
function deriveToolFromChord(chord: KeyChord | null): VoiceInputTool {
  const normalized = chord ? [...chord.keys].sort().join("+") : "";
  if (normalized === [...DEFAULT_VOICE_HOTKEY_KEYS].sort().join("+")) return "wechat";
  if (normalized === "right_alt") return "doubao";
  return "other";
}

/**
 * 选择输入工具：立即把该工具要求的"按住说话快捷键"落盘（用户不需要理解快捷键），
 * 并把工具选择持久化（决定下次进页展示哪套引导与开关）。
 */
async function selectVoiceInputTool(tool: VoiceInputTool): Promise<void> {
  if (savingVoiceInputTool.value) return;
  if (capturingVoiceHotkey.value) {
    await finishVoiceHotkeyCapture("已切换输入工具，已取消录入");
  }
  const previous = voiceInputTool.value;
  savingVoiceInputTool.value = true;
  // 乐观态窗口：工具已切、快捷键还在写盘路上，期间状态胶囊不许判成"未同步"。
  applyingTool.value = tool;
  voiceHotkeyMessage.value = "";
  voiceInputTool.value = tool;
  try {
    await setVoiceInputTool(tool);
  } catch (error) {
    voiceInputTool.value = previous;
    voiceHotkeyMessage.value = error instanceof Error ? error.message : String(error);
    savingVoiceInputTool.value = false;
    applyingTool.value = null;
    return;
  }
  savingVoiceInputTool.value = false;
  const keys = VOICE_TOOL_CHORDS[tool];
  try {
    if (keys) {
      await applyVoiceHotkey([...keys]);
    } else if (tool === "other") {
      // 「其他工具」有自己的记忆：切去豆包/微信会改写"按住说话快捷键"，
      // 切回来时按用户上次选的恢复（never = 从未选过 → 保持现状）。
      const remembered = await getOtherVoiceHotkey();
      if (remembered) {
        await applyVoiceHotkey([...remembered]);
      }
    }
  } finally {
    applyingTool.value = null;
  }
  // Vokie：每次选中都重查一次安装状态（用户可能刚装好就回来选）。
  if (tool === "vokie") {
    void refreshVokieInstallation();
  }
}

/** 「其他工具」记住用户选的按键：写失败不影响本次生效（只是下次不恢复）。 */
async function rememberOtherVoiceHotkey(keys: string[]): Promise<void> {
  try {
    await setOtherVoiceHotkey([...keys]);
  } catch (error) {
    voiceHotkeyMessage.value = error instanceof Error ? error.message : String(error);
  }
}

/** 「其他工具」面板里选按键：先生效，再记住（切去豆包/微信再切回来时恢复）。 */
async function applyOtherChord(keys: string[]): Promise<void> {
  await applyVoiceHotkey([...keys]);
  await rememberOtherVoiceHotkey(keys);
}

/** Vokie 检测；失败保持 null，不把"检测失败"当成"没装/没运行"。 */
async function refreshVokieInstallation(): Promise<void> {
  if (checkingVokie.value) return;
  checkingVokie.value = true;
  try {
    const status = await getVokieInstallation();
    vokieInstalled.value = status.installed;
    vokieRunning.value = status.running;
    vokieCheckMessage.value = "";
  } catch (error) {
    vokieInstalled.value = null;
    vokieRunning.value = null;
    vokieCheckMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    checkingVokie.value = false;
  }
}

async function openVokiePage(): Promise<void> {
  openingVokiePage.value = true;
  try {
    await openVokieHomepage();
    vokieCheckMessage.value = "已打开 Vokie 官网；安装后点“重新检测”。";
  } catch (error) {
    vokieCheckMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    openingVokiePage.value = false;
  }
}

/** 打开 Vokie 之后的自动复查：间隔 1.5s、最多 8 次（约 12 秒）。 */
const VOKIE_LAUNCH_POLL_INTERVAL_MS = 1500;
const VOKIE_LAUNCH_POLL_ATTEMPTS = 8;

/**
 * 点「打开 Vokie」后替用户等它起来并自动复查。
 * 2026-10-02 Andy 反馈：点了按钮、Vokie 明明已经打开，但「Vokie 没有运行」的
 * 提示还挂着，得手动再点一次「重新检测」——因为启动是异步的，点完立刻检测
 * 多半还是 false。这里轮询到 running=true 就清掉提示（提示区变成「Vokie 已在
 * 运行。」）；超时仍没起来就明确告诉他可以再点「重新检测」。
 */
async function waitForVokieRunning(): Promise<void> {
  for (let attempt = 0; attempt < VOKIE_LAUNCH_POLL_ATTEMPTS; attempt += 1) {
    await new Promise<void>((resolve) =>
      window.setTimeout(resolve, VOKIE_LAUNCH_POLL_INTERVAL_MS),
    );
    if (unmounted) return;
    await refreshVokieInstallation();
    if (unmounted) return;
    if (vokieRunning.value === true) {
      vokieCheckMessage.value = "Vokie 已在运行。";
      return;
    }
  }
  if (!unmounted) {
    vokieCheckMessage.value = "还没检测到 Vokie 在运行；如果它已经打开，点“重新检测”。";
  }
}

/** 打开 Vokie（装了但没运行时）：启动后自动等待并复查，用户不用再点一次。 */
async function launchVokieApp(): Promise<void> {
  launchingVokie.value = true;
  try {
    await launchVokie();
    vokieCheckMessage.value = "已打开 Vokie；正在等待它启动…";
    void waitForVokieRunning();
  } catch (error) {
    vokieCheckMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    launchingVokie.value = false;
  }
}

async function applyVoiceHotkey(keys: string[]) {
  savingVoiceHotkey.value = true;
  voiceHotkeyMessage.value = "";
  try {
    voiceHotkey.value = await setVoiceHoldHotkey(
      keys.length ? { keys: [...keys] } : null,
    );
    voiceHotkeyMessage.value = voiceHotkey.value
      ? `按住说话快捷键已设为 ${voiceHoldHotkeyLabel(voiceHotkey.value)}`
      : "按住说话快捷键已关闭，语音键仅输出语音";
  } catch (error) {
    voiceHotkeyMessage.value = error instanceof Error ? error.message : String(error);
    await refreshVoiceHotkey();
  } finally {
    savingVoiceHotkey.value = false;
  }
}

// ---------------------------------------------------------------------------
// 「支持更多输入工具」开关（2026-09-29 Andy 需求：连接页的第二入口）。
//
// **与按键页「全按键支持」是同一个设置项**（AppSettings.rc003_capture_enabled），
// 不是新状态：两处开关操作同一对 IPC（enable/disable_rc003_capture），状态
// 由每页挂载时的 getRc003TaskStatus 对账对齐——页面经 <component :is> 切换
// 时组件重建，每次进页都对账一次，两个入口不会漂移。
//
// 判据与文案与 ButtonsPage.toggleRc003Capture 同源：2026-10-03 起**每次
// 开启都先弹确认**（每次开启都会重新授权、都会弹 Windows 授权窗口），
// 不再依赖授权判据；关闭方向永远直接执行。开关只在需要它的工具下面板里
// 出现（豆包必须开、其他工具建议开、微信不需要——见 2026-09-30 设计稿 v3）。
// ---------------------------------------------------------------------------

const rc003CaptureEnabled = ref<boolean | null>(null);
const rc003CaptureBusy = ref(false);
const showCaptureConfirm = ref(false);
const captureSwitchEl = ref<HTMLInputElement | null>(null);
const rc003CaptureHint = ref("");

/**
 * 把开关的 DOM 状态写回绑定值。原生复选框被点击的瞬间浏览器先翻了 checked，
 * 而操作失败（状态没变）时 Vue 判定 props 无变化、不会生成 DOM 补丁——
 * 会出现「状态是关、界面是开」（ButtonsPage 2026-09-25 真机实证，同法防御）。
 */
function syncCaptureSwitchDom(): void {
  const el = captureSwitchEl.value;
  if (el) {
    el.checked = rc003CaptureEnabled.value === true;
  }
}

async function reconcileRc003Capture(): Promise<void> {
  try {
    const status = await getRc003TaskStatus();
    // 首次对账（null → 权威值）直接采纳；此后以本页用户操作为准，
    // 不被每秒变化无关的 installed 抖动带偏（与 ButtonsPage 同策略）。
    if (rc003CaptureEnabled.value === null) {
      rc003CaptureEnabled.value = status.enabled;
    }
  } catch {
    // 对账失败保持 null：开关显示占位符（不可点），不猜状态。
  }
}

async function toggleRc003Capture() {
  if (rc003CaptureBusy.value) return;
  // 与 ButtonsPage.toggleRc003Capture 同源：每次开启都先弹确认
  // （2026-10-03 定稿：每次开启都重新授权，都会弹 Windows 授权窗口）。
  // 关闭方向永远直接执行。
  if (rc003CaptureEnabled.value !== true) {
    showCaptureConfirm.value = true;
    // 开关 DOM 在点击瞬间已被浏览器翻转，先写回关闭，等确认后再真正执行。
    syncCaptureSwitchDom();
    return;
  }
  await applyCaptureToggle();
}

async function applyCaptureToggle() {
  if (rc003CaptureBusy.value) return;
  rc003CaptureBusy.value = true;
  const wasEnabled = rc003CaptureEnabled.value;
  try {
    const next: Rc003TaskStatus =
      wasEnabled === true ? await disableRc003Capture() : await enableRc003Capture();
    if (next.lastError) {
      rc003CaptureHint.value = next.lastError;
      reportFrontendEvent({
        event: "rc003_capture_toggle",
        phase: "completed",
        result: "failed",
        reason: `page=connection was_enabled=${String(wasEnabled === true)} error=${next.lastError}`,
      });
      return;
    }
    rc003CaptureEnabled.value = next.enabled;
    rc003CaptureHint.value = "";
    reportFrontendEvent({
      event: "rc003_capture_toggle",
      phase: "completed",
      result: "passed",
      reason: `page=connection enabled=${String(next.enabled)}`,
    });
  } catch (error) {
    rc003CaptureHint.value = error instanceof Error ? error.message : String(error);
    reportFrontendEvent({
      event: "rc003_capture_toggle",
      phase: "completed",
      result: "failed",
      reason: `page=connection was_enabled=${String(wasEnabled === true)} switch_reverted`,
    });
    // 同步回退到点击前的状态，并用权威状态校正一次（与 ButtonsPage 同源）。
    rc003CaptureEnabled.value = wasEnabled === true;
    void getRc003TaskStatus()
      .then((status) => {
        rc003CaptureEnabled.value = status.enabled;
      })
      .catch(() => {});
  } finally {
    rc003CaptureBusy.value = false;
    syncCaptureSwitchDom();
  }
}

function confirmCaptureDialog(): void {
  showCaptureConfirm.value = false;
  void applyCaptureToggle();
}

function closeCaptureDialog(): void {
  showCaptureConfirm.value = false;
  syncCaptureSwitchDom();
}

/**
 * 按住说话快捷键录入：录入门走 OS 级低级钩子（与按键映射页的自定义录入
 * 同一条链路），Win+L 之类的系统组合在到达 Shell 前就被成对吞下，不会
 * 真的锁屏。保存点放在"全部按键松开"之后，避免录入完成但物理键尚未松开
 * 时被系统补执行。Esc（未按修饰键）取消；15 秒未完成自动结束，此时已录到
 * 的组合不再丢弃。
 *
 * 入口只在"其他工具"面板（自定义组合键，feature flag 控制）；微信 / 豆包
 * 两个工具的快捷键是固定组合，选择即自动设置。
 */
async function beginVoiceHotkeyCapture(): Promise<void> {
  if (capturingVoiceHotkey.value || captureStartingVoiceHotkey.value) return;
  const requestId = ++voiceCaptureRequestId;
  captureStartingVoiceHotkey.value = true;
  voiceHotkeyMessage.value = "";
  cancelVoiceCaptureSettle();
  try {
    const preheld = await startShortcutCapture();
    if (unmounted || requestId !== voiceCaptureRequestId) {
      await stopShortcutCapture().catch(() => undefined);
      return;
    }
    voiceCapturePressed.clear();
    voiceCaptureEverPressed.length = 0;
    voiceCapturedKeys = null;
    voiceCaptureDisplay.value = [];
    capturingVoiceHotkey.value = true;
    // preheld 键的边沿对录入不可见（其 DOWN 已进 OS，UP 必须放行），
    // 后端等它们全部松开后才开始投递边沿；提示用户先松手，避免把
    // "按住中打开录入"截断成半截组合。
    waitingPreheldRelease.value = preheld.length > 0;
    if (preheld.length > 0) {
      voiceHotkeyMessage.value =
        "检测到仍有按住的按键，请先松开所有按键；松开后即可按新组合，录入将自动开始";
    }
    if (voiceCaptureTimeout !== null) window.clearTimeout(voiceCaptureTimeout);
    voiceCaptureTimeout = window.setTimeout(() => {
      void finishVoiceHotkeyCapture("录入已超时，请重新录入");
    }, 15_000);
  } catch (error) {
    voiceHotkeyMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    if (requestId === voiceCaptureRequestId) captureStartingVoiceHotkey.value = false;
  }
}

/**
 * 结束录入：已录到组合则落盘生效；零/半截边沿时结合微信输入法语音观测推断
 * （见 applyVoiceHotkey 上方的推断说明）；否则只显示传入的取消原因。
 */
async function finishVoiceHotkeyCapture(cancelMessage?: string): Promise<void> {
  voiceCaptureRequestId += 1;
  captureStartingVoiceHotkey.value = false;
  capturingVoiceHotkey.value = false;
  waitingPreheldRelease.value = false;
  cancelVoiceCaptureSettle();
  if (voiceCaptureTimeout !== null) window.clearTimeout(voiceCaptureTimeout);
  voiceCaptureTimeout = null;
  voiceCapturePressed.clear();
  // 推断判定在清空前取样：会话内见到的边沿数（0 = 全吞，1 = 半截）。
  const seenKeyCount = voiceCaptureEverPressed.length;
  voiceCaptureEverPressed.length = 0;
  const keys = voiceCapturedKeys;
  voiceCapturedKeys = null;
  voiceCaptureDisplay.value = [];
  const stop = await stopShortcutCapture().catch(() => null);
  const wetypeVoice = stop?.wetypeVoice ?? "unknown";
  // 半截会话（scheduleVoiceCaptureFinish 在稳定窗口到期时已把唯一修饰键写进
  // keys）与零边沿会话同样走推断：微信输入法吞键发生在 RIT 层，唯一旁证是其
  // 语音被触发。单主键（如 D）不推断——按主键不会触发微信输入法语音。
  const shouldInferWetypeChord =
    wetypeVoice === "observed" &&
    (keys === null
      ? seenKeyCount <= 1
      : keys.length === 1 && CAPTURE_MODIFIER_KEYS.has(keys[0]));
  if (shouldInferWetypeChord) {
    await applyVoiceHotkey([...DEFAULT_VOICE_HOTKEY_KEYS]);
    // applyVoiceHotkey 成功会覆写消息，推断说明必须在其后写入；失败时保留错误信息。
    if (voiceHotkey.value) {
      voiceHotkeyMessage.value =
        "你按下的组合触发了微信输入法的语音（按键被其拦截，内容无法读取），已按微信输入法语音键默认值 左 Ctrl + 左 Win 生效。此快捷键需与微信输入法语音键一致；若你修改过微信输入法的语音键，请在微信输入法设置中查看后重新录入对应组合";
    }
    return;
  }
  if (keys && keys.length > 0) {
    await applyVoiceHotkey([...keys]);
    if (voiceInputTool.value === "other") {
      await rememberOtherVoiceHotkey([...keys]);
    }
    appendWetypeChordNotice([...keys]);
    return;
  }
  if (cancelMessage) {
    voiceHotkeyMessage.value = cancelMessage;
    return;
  }
  voiceHotkeyMessage.value =
    "本次未捕获到任何按键。微信输入法会拦截它自己的语音键（默认 左 Ctrl + 左 Win），本次也未观测到语音被触发；请重试，或直接选择上方工具卡片使用固定组合";
}

/**
 * 落盘组合不是微信输入法语音键（默认 左 Ctrl + 左 Win）时提醒：按住说话会把
 * 该组合注入系统来唤起微信输入法语音，组合不一致则按住说话无法生效——这正是
 * "能录上 Win+右 Ctrl 反而没用"的原因。默认组合与推断落盘不需要这句提醒。
 */
function appendWetypeChordNotice(keys: KeyCode[]): void {
  if (!voiceHotkeyMessage.value.startsWith("按住说话快捷键已设为")) return;
  const normalized = [...keys].sort().join("+");
  if ([...DEFAULT_VOICE_HOTKEY_KEYS].sort().join("+") === normalized) return;
  voiceHotkeyMessage.value +=
    "。注意：按住说话会把该组合注入系统来唤起微信输入法语音，若与微信输入法语音键不一致将无法生效";
}

/** 落盘稳定窗口时长：外部钩子重放的注入副本通常在物理边沿的同一输入批次内到达。 */
const VOICE_CAPTURE_SETTLE_MS = 200;

/** 取消待执行的落盘稳定窗口（新按下沿到来时需要重新计时）。 */
function cancelVoiceCaptureSettle(): void {
  if (voiceCaptureSettleTimeout !== null) window.clearTimeout(voiceCaptureSettleTimeout);
  voiceCaptureSettleTimeout = null;
}

/**
 * 全部按键松开后延迟定稿：外部钩子吞掉完成键的物理边沿后会以注入副本重放
 * 整个组合，副本可能晚于物理松开到达；立即落盘会把组合截断成"只剩第一个键"
 * （Bugs/2026-09-27）。窗口内若再出现按下沿，cancelVoiceCaptureSettle 会取消
 * 本次计时并重新等待。
 */
function scheduleVoiceCaptureFinish(): void {
  cancelVoiceCaptureSettle();
  voiceCaptureSettleTimeout = window.setTimeout(() => {
    voiceCaptureSettleTimeout = null;
    if (!capturingVoiceHotkey.value || voiceCapturePressed.size > 0) return;
    if (!voiceCapturedKeys && voiceCaptureEverPressed.length > 0) {
      voiceCapturedKeys = [...voiceCaptureEverPressed];
    }
    void finishVoiceHotkeyCapture();
  }, VOICE_CAPTURE_SETTLE_MS);
}

async function acceptVoiceCaptureEdge(edge: ShortcutCaptureEdge): Promise<void> {
  if (!capturingVoiceHotkey.value) return;
  // 后端只在 preheld 键全部松开后才开始投递边沿：第一条边沿即已武装。
  if (waitingPreheldRelease.value) {
    waitingPreheldRelease.value = false;
    voiceHotkeyMessage.value = "";
  }
  const { key, isPressed } = edge;
  if (!isPressed) {
    voiceCapturePressed.delete(key);
    if (voiceCapturedKeys) {
      voiceCaptureDisplay.value = voiceCapturedKeys;
      if (voiceCapturePressed.size === 0) scheduleVoiceCaptureFinish();
      return;
    }
    // 组合里没有主键时（默认的 左 Ctrl + 左 Win、豆包的"长按右 Alt"都是这种），
    // 在最后一个按键松开时按本次会话按过的全部修饰键落盘。不能只看最后松开
    // 的那个键：Ctrl+Win 先松 Win 会把组合截断成只剩 Ctrl（Bugs/2026-09-27）。
    // 能走到这里说明会话里没有主键（主键按下时即成组），故全部是修饰键。
    // 定稿延后到稳定窗口（见 scheduleVoiceCaptureFinish）：被吞掉的完成键只
    // 会以输入法重放的注入副本形式稍后到达，立即落盘会把它丢掉。
    if (voiceCapturePressed.size === 0 && voiceCaptureEverPressed.length > 0) {
      voiceCaptureDisplay.value = [...voiceCaptureEverPressed];
      scheduleVoiceCaptureFinish();
    }
    return;
  }
  // 已经拿到终止键后继续保持原生拦截，直到本次组合的所有 DOWN 都收到配对 UP。
  if (voiceCapturedKeys) return;
  cancelVoiceCaptureSettle();
  if (key === "escape" && voiceCapturePressed.size === 0) {
    await finishVoiceHotkeyCapture("已取消录入");
    return;
  }
  voiceCapturePressed.add(key);
  if (!voiceCaptureEverPressed.includes(key)) voiceCaptureEverPressed.push(key);
  if (CAPTURE_MODIFIER_KEYS.has(key)) {
    voiceCaptureDisplay.value = [...voiceCapturePressed];
    return;
  }
  voiceCapturedKeys = [...voiceCapturePressed];
  voiceCaptureDisplay.value = voiceCapturedKeys;
}

function handleVoiceCaptureBlur(): void {
  if (capturingVoiceHotkey.value || captureStartingVoiceHotkey.value) {
    void finishVoiceHotkeyCapture("窗口失去焦点，已取消录入");
  }
}

async function refreshVoiceHotkey() {
  try {
    voiceHotkey.value = await getVoiceHoldHotkey();
  } catch (error) {
    voiceHotkeyMessage.value = error instanceof Error ? error.message : String(error);
  }
}

/**
 * 初始化快捷键设置与输入工具：两者一起读，工具缺失（老配置）时按当前
 * 快捷键推断并落存一次——此后以用户的选择为准（其他工具 + 右 Alt 与
 * 豆包 + 右 Alt 的组合相同，只有落存的工具能把它们区分开）。
 */
async function initializeShortcutSettings(): Promise<void> {
  const [chord, storedTool] = await Promise.all([
    getVoiceHoldHotkey().catch((error) => {
      voiceHotkeyMessage.value = error instanceof Error ? error.message : String(error);
      return null;
    }),
    getVoiceInputTool().catch(() => null),
  ]);
  voiceHotkey.value = chord;
  if (storedTool) {
    voiceInputTool.value = storedTool;
    return;
  }
  const derived = deriveToolFromChord(chord);
  voiceInputTool.value = derived;
  void setVoiceInputTool(derived).catch(() => undefined);
}

watch(
  () => props.runtime?.platform.connection,
  (snapshot) => {
    if (snapshot) connection.value = snapshot;
  },
  { immediate: true },
);

watch(
  () => props.runtime?.platform.audio,
  (snapshot) => {
    if (snapshot) audio.value = snapshot;
  },
  { immediate: true },
);

const connectionActive = computed(() =>
  [
    "connecting",
    "discovering",
    "awaiting_capabilities",
    "ready",
    "streaming",
    "draining",
    "reconnecting",
    "suspended",
  ].includes(connection.value.phase),
);

const atvvReady = computed(() =>
  ["ready", "streaming", "draining"].includes(connection.value.phase),
);

const audioBusy = computed(() => ["streaming", "draining"].includes(audio.value.phase));

const wasapiReady = computed(() =>
  ["ready", "streaming", "draining"].includes(audio.value.phase),
);

const virtualCableEndpoints = computed(() =>
  audioEndpoints.value.filter((endpoint) => endpoint.isVirtualCableCandidate),
);

const virtualCableInstalled = computed(() => virtualCableEndpoints.value.length > 0);

const phaseTone = computed(() => {
  if (connection.value.phase === "failed") return "error";
  if (connection.value.phase === "streaming") return "active";
  if (connection.value.phase === "ready") return "success";
  if (connectionActive.value) return "warning";
  return "pending";
});

const phaseDetail = computed(() => {
  if (connection.value.lastError) return connection.value.lastError;
  if (connection.value.capabilities) return "语音功能已就绪，按住遥控器语音键就能说话";
  return "连接后即可使用遥控器语音键";
});

/**
 * 状态条标题：已识别型号时显示型号（2026-10-01 Andy 要求——RC001 = 小米蓝牙语音遥控器 2、
 * RC003 = 小米蓝牙语音遥控器 2 Pro）。型号未识别（GATT 2A24 还没读回）或未连接时退回
 * 蓝牙广播名 / 阶段文案，避免把"连接后显示"这类占位当标题。
 */
const connectionTitle = computed(() => {
  const model = connection.value.remoteModel;
  if (connectionActive.value && model !== "unknown") return remoteModelLabel(model);
  return connection.value.remoteName ?? connectionPhaseLabel(connection.value.phase);
});

const audioTone = computed(() => {
  if (audio.value.phase === "failed") return "error";
  if (audio.value.phase === "streaming") return "active";
  if (audio.value.phase === "ready") return "success";
  if (audio.value.phase === "draining") return "warning";
  return "pending";
});

const audioDetail = computed(() => {
  if (audio.value.lastError) return audio.value.lastError;
  if (audio.value.selectedEndpointName) return "语音会写入选中的设备";
  return "不会自动改动系统默认设备，需要在这里明确选择";
});

async function refreshConnection() {
  try {
    connection.value = await getConnectionSnapshot();
  } catch (error) {
    operationMessage.value = error instanceof Error ? error.message : String(error);
  }
}

async function refreshAudio() {
  try {
    audio.value = await getAudioSnapshot();
    return true;
  } catch (error) {
    audioMessage.value = error instanceof Error ? error.message : String(error);
    return false;
  }
}

async function scan() {
  scanning.value = true;
  operationMessage.value = "";
  scanMessage.value = "正在寻找小米遥控器…";
  try {
    devices.value = await scanPairedRemotes();
    scanMessage.value = devices.value.length
      ? `找到 ${devices.value.length} 个已配对的小米遥控器`
      : "没有找到已配对的小米遥控器";
  } catch (error) {
    devices.value = [];
    scanMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    scanning.value = false;
  }
}

async function connect(device: PairedRemote) {
  connectingDeviceId.value = device.id;
  operationMessage.value = "";
  try {
    connection.value = await connectRemote(device.id);
    operationMessage.value = "已连接，正在确认语音功能";
  } catch (error) {
    operationMessage.value = error instanceof Error ? error.message : String(error);
    await refreshConnection();
  } finally {
    connectingDeviceId.value = "";
  }
}

async function disconnect() {
  disconnecting.value = true;
  operationMessage.value = "";
  try {
    connection.value = await disconnectRemote();
    operationMessage.value = "遥控器连接已释放，本次运行已停止自动重连";
  } catch (error) {
    operationMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    disconnecting.value = false;
  }
}

async function detectAudioEndpoints(autoSelectVirtualCable: boolean) {
  scanningAudio.value = true;
  audioMessage.value = "正在读取语音设备…";
  try {
    audioEndpoints.value = await listAudioEndpoints();
    audioScanComplete.value = true;
    const virtualCables = audioEndpoints.value.filter(
      (endpoint) => endpoint.isVirtualCableCandidate,
    );
    if (virtualCables.length === 1 && autoSelectVirtualCable && !audio.value.selectedEndpointId) {
      await chooseAudioEndpoint(virtualCables[0], true);
      return;
    }
    audioMessage.value = virtualCables.length
      ? `已检测到 ${virtualCables.length} 个 VB-CABLE 语音设备`
      : "未检测到 VB-CABLE；安装完成后需要重启电脑，再重新检测";
  } catch (error) {
    audioEndpoints.value = [];
    audioScanComplete.value = true;
    audioMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    scanningAudio.value = false;
  }
}

async function scanAudio() {
  await detectAudioEndpoints(false);
  // 用户主动读取端点 = 想看列表；选好即收起（每次只用一个端点）。
  showEndpointList.value = audioEndpoints.value.length > 0;
}

async function chooseAudioEndpoint(endpoint: AudioEndpoint, automatic = false) {
  selectingEndpointId.value = endpoint.id;
  audioMessage.value = "正在打开语音设备…";
  try {
    const snapshot = await selectAudioEndpoint(endpoint.id);
    audio.value = snapshot;
    // 后端可能在打不开所选端点时自动改用同一台虚拟声卡的另一个 CABLE 端点
    // （2026-10-02 现场：新版驱动的 CABLE In 16 Ch 会瞬态被占用），按实际使用的
    // 设备报文案，避免界面与事实不符。
    const actualName = snapshot.selectedEndpointName ?? endpoint.name;
    audioMessage.value =
      actualName === endpoint.name
        ? automatic
          ? `已自动选择 ${actualName}`
          : `已选择 ${actualName}`
        : `已自动改用 ${actualName}（${endpoint.name} 暂时打不开）`;
    showEndpointList.value = false;
  } catch (error) {
    audioMessage.value = error instanceof Error ? error.message : String(error);
    await refreshAudio();
  } finally {
    selectingEndpointId.value = "";
  }
}

async function openVbCablePage() {
  openingVbCablePage.value = true;
  try {
    await openVbCableDownloadPage();
    audioMessage.value = "已打开 VB-CABLE 官方下载页面；安装时需要管理员权限，完成后请重启电脑";
  } catch (error) {
    audioMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    openingVbCablePage.value = false;
  }
}

async function initializeAudio() {
  const restoredAudio = await refreshAudio();
  await detectAudioEndpoints(restoredAudio);
}

onMounted(async () => {
  window.addEventListener("blur", handleVoiceCaptureBlur);
  void refreshConnection();
  void initializeAudio();
  void initializeShortcutSettings();
  void reconcileRc003Capture();
  void refreshVokieInstallation();
  pollTimer = setInterval(() => {
    void refreshConnection();
    void refreshAudio();
  }, 1_000);
  const stopCaptureEdges = await subscribeShortcutCaptureEdges((edge) => {
    void acceptVoiceCaptureEdge(edge);
  });
  if (unmounted) {
    stopCaptureEdges();
    return;
  }
  unlistenVoiceCapture = stopCaptureEdges;
});

onUnmounted(() => {
  unmounted = true;
  window.removeEventListener("blur", handleVoiceCaptureBlur);
  if (pollTimer) clearInterval(pollTimer);
  cancelVoiceCaptureSettle();
  if (voiceCaptureTimeout !== null) window.clearTimeout(voiceCaptureTimeout);
  voiceCaptureTimeout = null;
  unlistenVoiceCapture?.();
  unlistenVoiceCapture = null;
  void stopShortcutCapture().catch(() => undefined);
});
</script>

<template>
  <section>
    <header class="page-header">
      <div>
        <h1>连接</h1>
      </div>
      <span class="badge" :class="phaseTone">{{ connectionPhaseLabel(connection.phase) }}</span>
    </header>

    <!-- ============ 设备：遥控器 + 语音设备 ============ -->
    <div class="device-row">
      <article class="card">
        <div class="card-title-row">
          <div>
            <h2>遥控器连接</h2>
          </div>
          <button
            class="primary-button"
            type="button"
            :disabled="scanning || connectionActive || !runtime?.platform.bleScanAvailable"
            @click="scan"
          >
            {{ scanning ? "扫描中…" : "扫描已配对设备" }}
          </button>
        </div>

        <div class="status-panel" aria-live="polite">
          <div class="status-copy">
            <div class="status-heading">
              <span class="status-dot" :class="phaseTone"></span>
              <strong class="connection-title">{{ connectionTitle }}</strong>
            </div>
            <small>{{ phaseDetail }}</small>
          </div>
          <button
            v-if="connectionActive"
            class="secondary-button status-action"
            type="button"
            :disabled="disconnecting"
            @click="disconnect"
          >
            {{ disconnecting ? "断开中…" : "断开" }}
          </button>
        </div>

        <div v-if="connectionActive" class="chip-row">
          <span class="device-chip">
            <BatteryIndicator :connection="connection" />
          </span>
          <span class="device-chip">
            <span class="status-dot" :class="atvvReady ? 'success' : 'pending'"></span>
            {{ atvvReady ? "语音按键已就绪" : "正在确认语音功能" }}
          </span>
          <span v-if="connection.powerNotificationsAvailable" class="device-chip">
            <span class="status-dot success"></span>
            睡眠唤醒后自动重连
          </span>
        </div>

        <p v-if="scanMessage" class="muted scan-summary">{{ scanMessage }}</p>
        <p v-if="operationMessage" class="operation-message">{{ operationMessage }}</p>

        <ul v-if="devices.length" class="device-list">
          <li v-for="device in devices" :key="device.id">
            <div><strong>{{ device.name }}</strong><small>{{ remoteModelLabel(device.model) }}</small></div>
            <button
              type="button"
              :disabled="connectionActive || Boolean(connectingDeviceId)"
              @click="connect(device)"
            >
              {{ connectingDeviceId === device.id ? "连接中…" : "连接" }}
            </button>
          </li>
        </ul>
      </article>

      <article class="card">
        <div class="card-title-row">
          <div>
            <h2>语音设备</h2>
          </div>
          <button
            class="secondary-button"
            type="button"
            :disabled="scanningAudio || audioBusy || !runtime?.platform.windowsApiAvailable"
            @click="scanAudio()"
          >
            {{ scanningAudio ? "读取中…" : "刷新设备列表" }}
          </button>
        </div>

        <div class="status-panel" aria-live="polite">
          <div class="status-copy">
            <div class="status-heading">
              <span class="status-dot" :class="audioTone"></span>
              <strong>{{ audio.selectedEndpointName ?? audioPhaseLabel(audio.phase) }}</strong>
            </div>
            <small>{{ audioDetail }}</small>
          </div>
        </div>

        <p v-if="audioMessage" class="muted scan-summary">{{ audioMessage }}</p>
        <div v-if="audioEndpoints.length" class="endpoint-select-row">
          <button
            class="secondary-button"
            type="button"
            @click="showEndpointList = !showEndpointList"
          >
            {{ showEndpointList ? "收起列表" : audio.selectedEndpointId ? "更换设备" : "选择设备" }}
          </button>
          <span v-if="!showEndpointList" class="muted endpoint-count">
            共 {{ audioEndpoints.length }} 个设备可选
          </span>
        </div>
        <ul v-if="showEndpointList && audioEndpoints.length" class="device-list endpoint-list">
          <li v-for="endpoint in audioEndpoints" :key="endpoint.id">
            <div>
              <strong>{{ endpoint.name }}</strong>
              <strong
                v-if="isRecommendedVoiceEndpoint(endpoint)"
                class="endpoint-recommend"
              >
                推荐
              </strong>
              <small v-else>其他音频设备</small>
            </div>
            <button
              type="button"
              :disabled="audioBusy || Boolean(selectingEndpointId) || audio.selectedEndpointId === endpoint.id"
              @click="chooseAudioEndpoint(endpoint)"
            >
              {{
                selectingEndpointId === endpoint.id
                  ? "正在启用…"
                  : audio.selectedEndpointId === endpoint.id
                    ? "当前设备"
                    : "选择"
              }}
            </button>
          </li>
        </ul>

        <div v-if="audioScanComplete && !virtualCableInstalled" class="info-callout warning vb-cable-callout">
          <div>
            <strong>需要安装 VB-CABLE</strong>
            <p>由 VB-Audio 提供的免费虚拟声卡。安装需要管理员权限，完成后需重启电脑。</p>
          </div>
          <div class="button-row">
            <button class="primary-button" type="button" :disabled="openingVbCablePage" @click="openVbCablePage">
              {{ openingVbCablePage ? "正在打开…" : "打开官方下载页" }}
            </button>
            <button class="secondary-button" type="button" :disabled="scanningAudio" @click="scanAudio()">
              重新检测
            </button>
          </div>
        </div>
        <div v-else class="info-callout" :class="{ warning: !wasapiReady }">
          {{
            wasapiReady
              ? "语音设备已就绪。"
              : virtualCableInstalled
                ? "已检测到 VB-CABLE。在下面列表里选择带「推荐」标记的 CABLE 设备；输入法的麦克风请选择 CABLE Output。"
                : "正在检测 VB-CABLE…"
          }}
        </div>
      </article>
    </div>

    <!-- ============ 语音输入设置（三步） ============ -->
    <article class="card setup-card">
      <div class="card-title-row">
        <div>
          <h2>语音输入设置</h2>
          <p class="muted">设置一次，之后按住遥控器语音键说话，松开就出字。</p>
        </div>
      </div>

      <div class="setup-columns">
        <!-- ① 输入工具 -->
        <section class="setup-col">
          <div class="col-head">
            <span class="step-index">1</span>
            <strong>选择你在用的输入工具</strong>
          </div>
          <div class="tool-list">
            <button
              v-for="card in TOOL_CARDS"
              :key="card.id"
              class="tool-card"
              :class="{ selected: voiceInputTool === card.id }"
              type="button"
              :disabled="savingVoiceInputTool"
              @click="selectVoiceInputTool(card.id)"
            >
              <span class="radio" aria-hidden="true"></span>
              <span>
                <strong>{{ card.name }}</strong>
                <small>{{ card.note }}</small>
              </span>
            </button>
          </div>
        </section>

        <!-- ② 遥控器语音键替你按哪个键 -->
        <section class="setup-col">
          <div class="col-head">
            <span class="step-index">2</span>
            <strong>遥控器语音键替你按哪个键</strong>
          </div>

          <div v-if="voiceInputTool === 'doubao'" class="tool-panel">
            <div class="chord-line">
              按住遥控器语音键 <span class="muted">=</span>
              <span class="key">右 Alt</span>
              <span class="pill" :class="toolChordPill('doubao').ok ? 'ok' : 'warn'">
                {{ toolChordPill("doubao").label }}
              </span>
            </div>
            <div class="switch-line">
              <label class="toggle-row" for="capture-switch-doubao">
                <span>支持更多输入工具</span>
                <span
                  v-if="rc003CaptureEnabled === null"
                  class="toggle-placeholder"
                  aria-hidden="true"
                ></span>
                <input
                  v-else
                  id="capture-switch-doubao"
                  ref="captureSwitchEl"
                  type="checkbox"
                  class="toggle-input capture-switch"
                  :checked="rc003CaptureEnabled === true"
                  :disabled="rc003CaptureBusy"
                  @change="toggleRc003Capture"
                />
              </label>
              <span
                class="switch-state"
                :class="rc003CaptureEnabled === true ? 'ok' : 'warn'"
              >
                {{
                  rc003CaptureEnabled === true
                    ? "已开启"
                    : rc003CaptureEnabled === false
                      ? "需要开启"
                      : "正在读取…"
                }}
              </span>
            </div>
            <div v-if="rc003CaptureEnabled === false" class="info-callout warning callout-small">
              还差一步：开启后豆包才能收到遥控器语音键。每次开启都会弹出系统授权，请点“是”。
              已开启：现在按住遥控器语音键，豆包的语音条就会出现。
            </div>
            <div v-if="vokieRunning === true" class="info-callout warning callout-small">
              检测到 Vokie 正在运行：它和豆包用的是同一个快捷键（右 Alt），按住遥控器语音键可能唤起 Vokie。要用豆包，请先退出 Vokie。
            </div>
            <p v-if="rc003CaptureHint" class="tiny muted">{{ rc003CaptureHint }}</p>
          </div>

          <div v-else-if="voiceInputTool === 'wechat'" class="tool-panel">
            <div class="chord-line">
              按住遥控器语音键 <span class="muted">=</span>
              <span class="key">左 Ctrl</span>
              <span class="muted">+</span>
              <span class="key">左 Win</span>
              <span class="pill" :class="toolChordPill('wechat').ok ? 'ok' : 'warn'">
                {{ toolChordPill("wechat").label }}
              </span>
            </div>
            <p class="tiny muted">微信输入法不需要“支持更多输入工具”开关。</p>
          </div>

          <div v-else-if="voiceInputTool === 'vokie'" class="tool-panel">
            <div v-if="vokieInstalled === false" class="info-callout warning callout-small">
              没有检测到 Vokie。先安装，再回来设置。
              <div class="button-row">
                <button
                  class="primary-button"
                  type="button"
                  :disabled="openingVokiePage"
                  @click="openVokiePage"
                >
                  {{ openingVokiePage ? "正在打开…" : "打开官网 vokie.com" }}
                </button>
                <button
                  class="secondary-button"
                  type="button"
                  :disabled="checkingVokie"
                  @click="refreshVokieInstallation"
                >
                  {{ checkingVokie ? "检测中…" : "重新检测" }}
                </button>
              </div>
            </div>
            <div
              v-else-if="vokieRunning === false"
              class="info-callout warning callout-small"
            >
              Vokie 没有运行：按住遥控器语音键不会唤起它（如果豆包输入法正在使用，出现的是豆包语音条）。打开 Vokie 后再试。
              <div class="button-row">
                <button
                  class="primary-button"
                  type="button"
                  :disabled="launchingVokie"
                  @click="launchVokieApp"
                >
                  {{ launchingVokie ? "正在打开…" : "打开 Vokie" }}
                </button>
                <button
                  class="secondary-button"
                  type="button"
                  :disabled="checkingVokie"
                  @click="refreshVokieInstallation"
                >
                  {{ checkingVokie ? "检测中…" : "重新检测" }}
                </button>
              </div>
            </div>
            <div class="chord-line">
              按住遥控器语音键 <span class="muted">=</span>
              <span class="key">右 Alt</span>
              <span class="pill" :class="toolChordPill('vokie').ok ? 'ok' : 'warn'">
                {{ toolChordPill("vokie").label }}
              </span>
            </div>
            <p v-if="vokieCheckMessage" class="tiny muted">{{ vokieCheckMessage }}</p>
          </div>

          <div v-else-if="voiceInputTool === 'other'" class="tool-panel">
            <p class="tiny muted" style="margin-top: 0">选与你输入工具里一致的语音键：</p>
            <div class="chip-select">
              <button
                v-for="option in OTHER_CHORD_OPTIONS"
                :key="option.id"
                class="chip"
                type="button"
                :aria-pressed="otherChordActive(option)"
                :disabled="savingVoiceHotkey || capturingVoiceHotkey"
                @click="applyOtherChord(option.keys)"
              >
                {{ option.label }}
              </button>
              <button
                v-if="VOICE_HOTKEY_CUSTOM_CAPTURE_ENABLED"
                class="chip"
                type="button"
                :aria-pressed="!OTHER_CHORD_OPTIONS.some(otherChordActive)"
                :disabled="savingVoiceHotkey || captureStartingVoiceHotkey"
                @click="
                  capturingVoiceHotkey
                    ? finishVoiceHotkeyCapture('已取消录入')
                    : beginVoiceHotkeyCapture()
                "
              >
                {{ capturingVoiceHotkey ? "录入中…（按 Esc 取消）" : "自定义组合键" }}
              </button>
              <button v-else class="chip" type="button" disabled>自定义组合键（暂未开放）</button>
            </div>
            <div class="switch-line">
              <label class="toggle-row" for="capture-switch-other">
                <span>支持更多输入工具</span>
                <span
                  v-if="rc003CaptureEnabled === null"
                  class="toggle-placeholder"
                  aria-hidden="true"
                ></span>
                <input
                  v-else
                  id="capture-switch-other"
                  ref="captureSwitchEl"
                  type="checkbox"
                  class="toggle-input capture-switch"
                  :checked="rc003CaptureEnabled === true"
                  :disabled="rc003CaptureBusy"
                  @change="toggleRc003Capture"
                />
              </label>
              <span
                class="switch-state"
                :class="rc003CaptureEnabled === true ? 'ok' : 'warn'"
              >
                {{ rc003CaptureEnabled === true ? "已开启" : "未开启" }}
              </span>
            </div>
            <p class="tiny muted">建议开启：部分输入工具需要它才能收到遥控器按键。</p>
            <p v-if="rc003CaptureHint" class="tiny muted">{{ rc003CaptureHint }}</p>
            <p v-if="capturingVoiceHotkey" class="capture-display voice-hotkey-capture">
              {{
                voiceCaptureDisplay.length
                  ? chordLabel({ keys: voiceCaptureDisplay })
                  : waitingPreheldRelease
                    ? "检测到仍有按住的按键，请先松开所有按键；松开后即可按新组合，录入将自动开始"
                    : "请按下你输入工具当前设置的语音键（也可单独按一个修饰键）；按 Esc 取消"
              }}
            </p>
            <p v-if="capturingVoiceHotkey && waitingPreheldRelease" class="tiny muted">
              按“自定义组合键”时仍按着键的组合不会完整录入，先松手即可。
            </p>
          </div>

          <p v-if="voiceHotkeyMessage" class="tiny muted voice-hotkey-message">
            {{ voiceHotkeyMessage }}
          </p>
          <div class="info-callout callout-small">
            避免其他 App 使用同一个快捷键（Vokie、Chatterfly 这类语音工具会抢在输入法前面）。按住语音键时，如果当前输入法还不是你选的工具，可能需要再按住一次才能正常使用。
          </div>
        </section>

        <!-- ③ 照着做 -->
        <section class="setup-col">
          <div class="col-head">
            <span class="step-index">3</span>
            <strong>照着做（只做一次）</strong>
          </div>

          <ol v-if="voiceInputTool === 'doubao'" class="checklist">
            <li><span class="mark">1</span><span>豆包麦克风选 CABLE Output</span></li>
            <li><span class="mark">2</span><span>豆包长按语音键选 右 Alt</span></li>
            <li><span class="mark">3</span><span>切到豆包后，按住遥控器语音键说话</span></li>
          </ol>
          <ol v-else-if="voiceInputTool === 'wechat'" class="checklist">
            <li><span class="mark">1</span><span>微信输入法麦克风选 CABLE Output</span></li>
            <li><span class="mark">2</span><span>切到微信输入法后，按住遥控器语音键说话</span></li>
          </ol>
          <ol v-else-if="voiceInputTool === 'vokie'" class="checklist">
            <li><span class="mark">1</span><span>Vokie 麦克风选 CABLE Output</span></li>
            <li><span class="mark">2</span><span>Vokie 快捷键保持默认的 右 Alt</span></li>
            <li><span class="mark">3</span><span>在要写字的地方按住遥控器语音键说话</span></li>
          </ol>
          <ol v-else-if="voiceInputTool === 'other'" class="checklist">
            <li><span class="mark">1</span><span>输入工具麦克风选 CABLE Output</span></li>
            <li><span class="mark">2</span><span>语音键与第 2 步选的键一致</span></li>
            <li><span class="mark">3</span><span>切到该输入法后，按住遥控器语音键说话</span></li>
          </ol>
        </section>
      </div>
    </article>

    <details class="usage-hint-details faq">
      <summary>常见问题（点开查看）</summary>
      <ul>
        <li>为什么语音键要“替你按一个键”？遥控器语音键不是键盘按键，输入法只认键盘按键，所以应用替你按住它。</li>
        <li>“支持更多输入工具”和“按键”页的“全按键支持”是同一个开关，两处随时同步；每次开启都会弹出一次系统授权。</li>
        <li>“替你按下的键”目前提供 左 Ctrl + 左 Win、右 Alt、左 Alt 和不按键四种；自由录入正在重做，暂未开放。</li>
        <li>微信输入法要求按住约半秒以上（需要联网），快速点按不出字是它自己的要求，不是故障。</li>
        <li>豆包要是当前输入法，否则按住遥控器语音键只会弹出 Windows 的 Alt 菜单（记事本里会出现“文件(F)、编辑(E)”这类字母）。应用会在你**按住语音键时**把输入法切到所选工具；刚切换过输入工具后的第一次按住如果没反应，松开再按一次即可（第一次那下用于切换输入法）。</li>
        <li>Vokie、微信输入法或豆包如果没有单独的麦克风选项，把系统默认录音设备设为 CABLE Output。</li>
        <li>Vokie 和豆包用的是同一个快捷键（右 Alt），同一时间只会有一个在响应：Vokie 要在运行中，豆包要是当前输入法；两个同时开着时 Vokie 会抢先。</li>
      </ul>
    </details>
  </section>
  <!-- 授权确认弹窗：与 ButtonsPage 共用同一组件（同一设置项、同一授权流程）；
       每次开启都先出现（toggleRc003Capture 决定，2026-10-03 起无授权判据）。 -->
  <EnhancedCaptureConfirmDialog
    v-if="showCaptureConfirm"
    @confirm="confirmCaptureDialog"
    @close="closeCaptureDialog"
  />
</template>
