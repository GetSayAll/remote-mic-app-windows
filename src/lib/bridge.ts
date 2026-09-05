import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

export const VB_CABLE_DOWNLOAD_URL = "https://vb-audio.com/Cable/";

export type ConnectionPhase =
  | "idle"
  | "connecting"
  | "discovering"
  | "awaiting_capabilities"
  | "ready"
  | "streaming"
  | "draining"
  | "reconnecting"
  | "suspended"
  | "disconnected"
  | "failed";

export type VoiceSessionState = "idle" | "streaming" | "draining";

export type RemoteModel = "rc001" | "rc003" | "unknown";

export type AudioPhase =
  | "unconfigured"
  | "ready"
  | "streaming"
  | "draining"
  | "failed"
  | "unsupported";

export interface AudioEndpoint {
  id: string;
  name: string;
  isVirtualCableCandidate: boolean;
}

export interface AudioSnapshot {
  phase: AudioPhase;
  selectedEndpointId: string | null;
  selectedEndpointName: string | null;
  queuedSamples: number;
  submittedSamples: number;
  generation: number;
  lastError: string | null;
}

export type RawInputPhase = "stopped" | "starting" | "ready" | "failed" | "unsupported";

export type RemoteButton =
  | "back"
  | "ok"
  | "tv"
  | "home"
  | "right"
  | "left"
  | "down"
  | "up"
  | "menu"
  | "power"
  | "volume_mute"
  | "volume_up"
  | "volume_down";

export type ButtonTrigger = "single" | "double" | "long";

export interface ButtonEdge {
  button: RemoteButton;
  isPressed: boolean;
}

export interface RawInputSnapshot {
  phase: RawInputPhase;
  matchedDeviceCount: number;
  rawEventCount: number;
  semanticEdgeCount: number;
  lastButton: RemoteButton | null;
  lastIsPressed: boolean | null;
  activeButtons: RemoteButton[];
  lastError: string | null;
}

export type KeyCode = string;

export interface KeyChord {
  keys: KeyCode[];
}

export type ButtonAction =
  | { type: "disabled" }
  | { type: "shortcut"; chord: KeyChord }
  | { type: "open_app"; target: string };

/** 预设应用条目（list_preset_apps 返回；对齐 Mac PresetApplication）。 */
export interface PresetAppInfo {
  id: string;
  name: string;
  installed: boolean;
}

/** 每键三列（单击/双击/长按），对齐 Mac 原版 ButtonTrigger。 */
export interface ButtonActions {
  single: ButtonAction;
  double: ButtonAction;
  long: ButtonAction;
}

export interface ButtonMappings {
  enabled: boolean;
  actions: Partial<Record<RemoteButton, ButtonActions>>;
}

export interface FiredGesture {
  button: RemoteButton;
  trigger: ButtonTrigger;
}

export interface ButtonMappingSnapshot {
  enabled: boolean;
  gateActive: boolean;
  listenerActive: boolean;
  swallowedEdges: number;
  leakedDowns: number;
  firedGestures: number;
  lastFired: FiredGesture | null;
  lastError: string | null;
}

export interface SendInputSnapshot {
  available: boolean;
  submittedBatches: number;
  submittedEvents: number;
  lastError: string | null;
}

export interface AtvvCapabilities {
  version: number;
  codecs: number;
  interaction: number;
  frameSize: number;
  selectedCodec: number;
  sampleRate: number;
}

export interface ConnectionSnapshot {
  phase: ConnectionPhase;
  remoteName: string | null;
  remoteModel: RemoteModel;
  capabilities: AtvvCapabilities | null;
  voiceState: VoiceSessionState;
  decodedSamples: number;
  generation: number;
  reconnectAttempt: number;
  powerNotificationsAvailable: boolean;
  lastError: string | null;
}

export interface PlatformSnapshot {
  platform: string;
  windowsApiAvailable: boolean;
  bleScanAvailable: boolean;
  bleVoiceReady: boolean;
  wasapiReady: boolean;
  rawInputReady: boolean;
  sendInputReady: boolean;
  verificationStatus: string;
  connection: ConnectionSnapshot;
  audio: AudioSnapshot;
  rawInput: RawInputSnapshot;
  buttonMapping: ButtonMappingSnapshot;
}

export interface RuntimeSnapshot {
  appVersion: string;
  platform: PlatformSnapshot;
}

export interface DiagnosticReport {
  schemaVersion: number;
  appVersion: string;
  platform: string;
  verificationStatus: string;
  capabilities: {
    windowsApiAvailable: boolean;
    bleScanAvailable: boolean;
    bleVoiceReady: boolean;
    wasapiReady: boolean;
    rawInputReady: boolean;
    sendInputReady: boolean;
  };
  connection: {
    phase: ConnectionPhase;
    capabilitiesConfirmed: boolean;
    sampleRate: number | null;
    frameSize: number | null;
    decodedSamples: number;
    generation: number;
    reconnectAttempt: number;
    powerNotificationsAvailable: boolean;
    errorPresent: boolean;
  };
  audio: {
    phase: AudioPhase;
    endpointConfigured: boolean;
    queuedSamples: number;
    submittedSamples: number;
    generation: number;
    errorPresent: boolean;
  };
  rawInput: {
    phase: RawInputPhase;
    matchedDeviceCount: number;
    rawEventCount: number;
    semanticEdgeCount: number;
    lastButton: RemoteButton | null;
    lastIsPressed: boolean | null;
    errorPresent: boolean;
  };
  sendInput: {
    available: boolean;
    submittedBatches: number;
    submittedEvents: number;
    errorPresent: boolean;
  };
  buttonMapping: {
    enabled: boolean;
    gateActive: boolean;
    listenerActive: boolean;
    swallowedEdges: number;
    leakedDowns: number;
    firedGestures: number;
    errorPresent: boolean;
  };
}

export interface PairedRemote {
  id: string;
  name: string;
  model: RemoteModel;
  isSupportedCandidate: boolean;
}

const browserSnapshot: RuntimeSnapshot = {
  appVersion: "0.1.0",
  platform: {
    platform: "browser-preview",
    windowsApiAvailable: false,
    bleScanAvailable: false,
    bleVoiceReady: false,
    wasapiReady: false,
    rawInputReady: false,
    sendInputReady: false,
    verificationStatus: "浏览器预览仅展示界面，不代表真机已通过",
    connection: {
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
    },
    audio: {
      phase: "unsupported",
      selectedEndpointId: null,
      selectedEndpointName: null,
      queuedSamples: 0,
      submittedSamples: 0,
      generation: 0,
      lastError: null,
    },
    rawInput: {
      phase: "unsupported",
      matchedDeviceCount: 0,
      rawEventCount: 0,
      semanticEdgeCount: 0,
      lastButton: null,
      lastIsPressed: null,
      activeButtons: [],
      lastError: null,
    },
    buttonMapping: {
      enabled: true,
      gateActive: false,
      listenerActive: false,
      swallowedEdges: 0,
      leakedDowns: 0,
      firedGestures: 0,
      lastFired: null,
      lastError: null,
    },
  },
};

function isTauriRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export async function getRuntimeSnapshot(): Promise<RuntimeSnapshot> {
  if (!isTauriRuntime()) {
    return browserSnapshot;
  }
  return invoke<RuntimeSnapshot>("get_runtime_snapshot");
}

export async function getDiagnosticReport(): Promise<DiagnosticReport> {
  if (!isTauriRuntime()) {
    return {
      schemaVersion: 1,
      appVersion: browserSnapshot.appVersion,
      platform: browserSnapshot.platform.platform,
      verificationStatus: browserSnapshot.platform.verificationStatus,
      capabilities: {
        windowsApiAvailable: false,
        bleScanAvailable: false,
        bleVoiceReady: false,
        wasapiReady: false,
        rawInputReady: false,
        sendInputReady: false,
      },
      connection: {
        phase: browserSnapshot.platform.connection.phase,
        capabilitiesConfirmed: false,
        sampleRate: null,
        frameSize: null,
        decodedSamples: 0,
        generation: 0,
        reconnectAttempt: 0,
        powerNotificationsAvailable: false,
        errorPresent: false,
      },
      audio: {
        phase: browserSnapshot.platform.audio.phase,
        endpointConfigured: false,
        queuedSamples: 0,
        submittedSamples: 0,
        generation: 0,
        errorPresent: false,
      },
      rawInput: {
        phase: browserSnapshot.platform.rawInput.phase,
        matchedDeviceCount: 0,
        rawEventCount: 0,
        semanticEdgeCount: 0,
        lastButton: null,
        lastIsPressed: null,
        errorPresent: false,
      },
      sendInput: {
        available: false,
        submittedBatches: 0,
        submittedEvents: 0,
        errorPresent: false,
      },
      buttonMapping: {
        enabled: true,
        gateActive: false,
        listenerActive: false,
        swallowedEdges: 0,
        leakedDowns: 0,
        firedGestures: 0,
        errorPresent: false,
      },
    };
  }
  return invoke<DiagnosticReport>("get_diagnostic_report");
}

export function formatDiagnosticReport(
  report: DiagnosticReport,
  generatedAt = new Date().toISOString(),
): string {
  return JSON.stringify({ generatedAt, ...report }, null, 2);
}

export async function scanPairedRemotes(): Promise<PairedRemote[]> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法读取已配对设备");
  }
  return invoke<PairedRemote[]>("scan_paired_remotes");
}

export async function getConnectionSnapshot(): Promise<ConnectionSnapshot> {
  if (!isTauriRuntime()) {
    return browserSnapshot.platform.connection;
  }
  return invoke<ConnectionSnapshot>("get_connection_snapshot");
}

export async function connectRemote(deviceId: string): Promise<ConnectionSnapshot> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法连接遥控器");
  }
  return invoke<ConnectionSnapshot>("connect_remote", { deviceId });
}

export async function disconnectRemote(): Promise<ConnectionSnapshot> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法断开遥控器");
  }
  return invoke<ConnectionSnapshot>("disconnect_remote");
}

export async function listAudioEndpoints(): Promise<AudioEndpoint[]> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法读取音频设备");
  }
  return invoke<AudioEndpoint[]>("list_audio_endpoints");
}

export async function getAudioSnapshot(): Promise<AudioSnapshot> {
  if (!isTauriRuntime()) {
    return browserSnapshot.platform.audio;
  }
  return invoke<AudioSnapshot>("get_audio_snapshot");
}

export async function selectAudioEndpoint(endpointId: string): Promise<AudioSnapshot> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法选择音频设备");
  }
  return invoke<AudioSnapshot>("select_audio_endpoint", { endpointId });
}

export async function openVbCableDownloadPage(): Promise<void> {
  if (!isTauriRuntime()) {
    window.open(VB_CABLE_DOWNLOAD_URL, "_blank", "noopener,noreferrer");
    return;
  }
  await openUrl(VB_CABLE_DOWNLOAD_URL);
}

export async function getRawInputSnapshot(): Promise<RawInputSnapshot> {
  if (!isTauriRuntime()) {
    return browserSnapshot.platform.rawInput;
  }
  return invoke<RawInputSnapshot>("get_raw_input_snapshot");
}

export async function startRawInput(): Promise<RawInputSnapshot> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法启动按键监听");
  }
  return invoke<RawInputSnapshot>("start_raw_input");
}

export async function stopRawInput(): Promise<RawInputSnapshot> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法停止按键监听");
  }
  return invoke<RawInputSnapshot>("stop_raw_input");
}

export async function getButtonMappings(): Promise<ButtonMappings> {
  if (!isTauriRuntime()) {
    return { enabled: true, actions: {} };
  }
  return invoke<ButtonMappings>("get_button_mappings");
}

export async function saveButtonMappings(mappings: ButtonMappings): Promise<ButtonMappings> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法保存按键映射");
  }
  return invoke<ButtonMappings>("save_button_mappings", { mappings });
}

export async function resetButtonMappings(): Promise<ButtonMappings> {
  if (!isTauriRuntime()) {
    return { enabled: true, actions: {} };
  }
  return invoke<ButtonMappings>("reset_button_mappings");
}

export async function testButtonMapping(
  button: RemoteButton,
  trigger: ButtonTrigger,
): Promise<SendInputSnapshot> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法执行按键测试");
  }
  return invoke<SendInputSnapshot>("test_button_mapping", { button, trigger });
}

export async function listPresetApps(): Promise<PresetAppInfo[]> {
  if (!isTauriRuntime()) {
    // 浏览器预览：展示完整预设表（仅渲染验证）。
    return [
      { id: "wechat", name: "微信", installed: true },
      { id: "edge", name: "Edge 浏览器", installed: true },
      { id: "chrome", name: "Chrome 浏览器", installed: true },
      { id: "notepad", name: "记事本", installed: true },
      { id: "calc", name: "计算器", installed: true },
      { id: "explorer", name: "文件资源管理器", installed: true },
      { id: "netease_music", name: "网易云音乐", installed: true },
    ];
  }
  return invoke<PresetAppInfo[]>("list_preset_apps");
}

export async function getButtonMappingSnapshot(): Promise<ButtonMappingSnapshot> {
  if (!isTauriRuntime()) {
    return {
      enabled: true,
      gateActive: false,
      listenerActive: false,
      swallowedEdges: 0,
      leakedDowns: 0,
      firedGestures: 0,
      lastFired: null,
      lastError: null,
    };
  }
  return invoke<ButtonMappingSnapshot>("get_button_mapping_snapshot");
}

/** 订阅语义按键边沿（画布高亮数据源）；浏览器预览下为空订阅。 */
export async function subscribeButtonEdges(
  handler: (edge: ButtonEdge) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) {
    return () => {};
  }
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<ButtonEdge>("button-edge", (event) => handler(event.payload));
  return () => {
    void unlisten();
  };
}

/** 订阅已触发手势（单击/双击/长按反馈）；浏览器预览下为空订阅。 */
export async function subscribeButtonGestures(
  handler: (gesture: FiredGesture) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) {
    return () => {};
  }
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<FiredGesture>("button-gesture", (event) => handler(event.payload));
  return () => {
    void unlisten();
  };
}

export async function getSendInputSnapshot(): Promise<SendInputSnapshot> {
  if (!isTauriRuntime()) {
    return { available: false, submittedBatches: 0, submittedEvents: 0, lastError: null };
  }
  return invoke<SendInputSnapshot>("get_send_input_snapshot");
}

export async function getVoiceHoldHotkey(): Promise<KeyChord | null> {
  if (!isTauriRuntime()) {
    return null;
  }
  return invoke<KeyChord | null>("get_voice_hold_hotkey");
}

export async function setVoiceHoldHotkey(hotkey: KeyChord | null): Promise<KeyChord | null> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法保存按住说话快捷键");
  }
  return invoke<KeyChord | null>("set_voice_hold_hotkey", { hotkey });
}

const voiceHotkeyKeyLabels: Record<string, string> = {
  control: "Ctrl",
  left_control: "左 Ctrl",
  right_control: "右 Ctrl",
  shift: "Shift",
  left_shift: "左 Shift",
  right_shift: "右 Shift",
  alt: "Alt",
  left_alt: "左 Alt",
  right_alt: "右 Alt",
  left_windows: "左 Win",
  right_windows: "右 Win",
  enter: "Enter",
  escape: "Esc",
  space: "空格",
  tab: "Tab",
  apps: "菜单键",
};

function voiceHotkeyKeyLabel(code: string): string {
  const known = voiceHotkeyKeyLabels[code];
  if (known) return known;
  const digit = /^digit([0-9])$/.exec(code);
  if (digit) return digit[1];
  return code.toUpperCase();
}

export function voiceHoldHotkeyLabel(hotkey: KeyChord | null): string {
  if (!hotkey || hotkey.keys.length === 0) return "关闭";
  return hotkey.keys.map(voiceHotkeyKeyLabel).join(" + ");
}

export function connectionPhaseLabel(phase: ConnectionPhase): string {
  return {
    idle: "尚未连接",
    connecting: "正在连接遥控器",
    discovering: "正在连接遥控器",
    awaiting_capabilities: "正在确认语音功能",
    ready: "已连接",
    streaming: "正在接收语音",
    draining: "正在结束本次语音",
    reconnecting: "正在等待遥控器重连",
    suspended: "电脑已进入睡眠",
    disconnected: "遥控器已断开",
    failed: "连接失败",
  }[phase];
}

export function remoteModelLabel(model: RemoteModel): string {
  return {
    rc001: "小米蓝牙遥控器 2",
    rc003: "小米蓝牙遥控器 2 Pro",
    unknown: "连接后显示",
  }[model];
}

export function audioPhaseLabel(phase: AudioPhase): string {
  return {
    unconfigured: "尚未选择设备",
    ready: "已就绪",
    streaming: "正在写入语音",
    draining: "正在结束",
    failed: "语音设备出错",
    unsupported: "当前环境不支持语音设备",
  }[phase];
}

export const buttonLabels: Record<RemoteButton, string> = {
  back: "返回",
  ok: "确定",
  tv: "TV",
  home: "主页",
  right: "右",
  left: "左",
  down: "下",
  up: "上",
  menu: "菜单",
  power: "电源",
  volume_mute: "静音",
  volume_up: "音量+",
  volume_down: "音量−",
};

export function buttonLabel(button: RemoteButton): string {
  return buttonLabels[button];
}

export function buttonTriggerLabel(trigger: ButtonTrigger): string {
  return {
    single: "单击",
    double: "双击",
    long: "长按",
  }[trigger];
}

const keyLabels: Record<string, string> = {
  ...voiceHotkeyKeyLabels,
  backspace: "退格",
  page_up: "Page Up",
  page_down: "Page Down",
  end: "End",
  insert: "Insert",
  delete: "Delete",
  left: "←",
  up: "↑",
  right: "→",
  down: "↓",
  volume_mute: "静音",
  volume_down: "音量−",
  volume_up: "音量+",
  f1: "F1",
  f2: "F2",
  f3: "F3",
  f4: "F4",
  f5: "F5",
  f6: "F6",
  f7: "F7",
  f8: "F8",
  f9: "F9",
  f10: "F10",
  f11: "F11",
  f12: "F12",
};

export function keyLabel(code: KeyCode): string {
  const known = keyLabels[code];
  if (known) return known;
  const digit = /^digit([0-9])$/.exec(code);
  if (digit) return digit[1];
  return code.toUpperCase();
}

export function chordLabel(chord: KeyChord): string {
  return chord.keys.map(keyLabel).join(" + ");
}

/** 预设应用显示名（页面加载 listPresetApps 后更新；测试可注入）。 */
const presetAppNames: Map<string, string> = new Map();

export function registerPresetAppNames(apps: Array<{ id: string; name: string }>): void {
  presetAppNames.clear();
  for (const app of apps) {
    presetAppNames.set(app.id, app.name);
  }
}

export function actionSummary(action: ButtonAction | undefined): string {
  if (!action || action.type === "disabled") return "未设置";
  if (action.type === "open_app") {
    return `打开${presetAppNames.get(action.target) ?? action.target}`;
  }
  return chordLabel(action.chord);
}
