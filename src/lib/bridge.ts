import { invoke } from "@tauri-apps/api/core";

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

export interface RawInputSnapshot {
  phase: RawInputPhase;
  matchedDeviceCount: number;
  rawEventCount: number;
  semanticEdgeCount: number;
  lastButton: RemoteButton | null;
  lastIsPressed: boolean | null;
  lastError: string | null;
}

export type KeyCode = string;

export interface KeyChord {
  keys: KeyCode[];
}

export type ButtonAction =
  | { type: "disabled" }
  | { type: "shortcut"; chord: KeyChord };

export interface ButtonMappings {
  actions: Partial<Record<RemoteButton, ButtonAction>>;
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
}

export interface RuntimeSnapshot {
  appVersion: string;
  platform: PlatformSnapshot;
}

export interface UsageTotals {
  buttonPresses: number;
  voiceSessions: number;
  voiceSeconds: number;
}

export interface DatedUsage {
  localDate: string;
  usage: UsageTotals;
}

export interface UsageStatisticsSummary {
  today: UsageTotals;
  thisWeek: UsageTotals;
  total: UsageTotals;
  recentDays: DatedUsage[];
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
    verificationStatus: "浏览器预览仅展示界面，不代表 Windows 或 RC001/RC003 已通过",
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
      lastError: null,
    },
  },
};

function isTauriRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

function emptyUsageStatistics(): UsageStatisticsSummary {
  const recentDays = Array.from({ length: 7 }, (_, index) => {
    const date = new Date();
    date.setHours(12, 0, 0, 0);
    date.setDate(date.getDate() - (6 - index));
    return {
      localDate: [
        date.getFullYear(),
        String(date.getMonth() + 1).padStart(2, "0"),
        String(date.getDate()).padStart(2, "0"),
      ].join("-"),
      usage: { buttonPresses: 0, voiceSessions: 0, voiceSeconds: 0 },
    };
  });
  return {
    today: { buttonPresses: 0, voiceSessions: 0, voiceSeconds: 0 },
    thisWeek: { buttonPresses: 0, voiceSessions: 0, voiceSeconds: 0 },
    total: { buttonPresses: 0, voiceSessions: 0, voiceSeconds: 0 },
    recentDays,
  };
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
    };
  }
  return invoke<DiagnosticReport>("get_diagnostic_report");
}

export async function getUsageStatistics(): Promise<UsageStatisticsSummary> {
  if (!isTauriRuntime()) {
    return emptyUsageStatistics();
  }
  return invoke<UsageStatisticsSummary>("get_usage_statistics");
}

export function formatUsageDuration(durationSeconds: number): string {
  const totalSeconds = Number.isFinite(durationSeconds)
    ? Math.max(0, Math.round(durationSeconds))
    : 0;
  const hours = Math.floor(totalSeconds / 3_600);
  const minutes = Math.floor((totalSeconds % 3_600) / 60);
  const seconds = totalSeconds % 60;
  if (hours > 0) return `${hours}小时${minutes}分钟`;
  if (minutes > 0) return `${minutes}分${seconds}秒`;
  return `${seconds}秒`;
}

export function formatDiagnosticReport(
  report: DiagnosticReport,
  generatedAt = new Date().toISOString(),
): string {
  return JSON.stringify({ generatedAt, ...report }, null, 2);
}

export async function scanPairedRemotes(): Promise<PairedRemote[]> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法调用 Windows 蓝牙 API");
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
    throw new Error("当前是浏览器预览，无法连接 Windows 蓝牙设备");
  }
  return invoke<ConnectionSnapshot>("connect_remote", { deviceId });
}

export async function disconnectRemote(): Promise<ConnectionSnapshot> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法断开 Windows 蓝牙设备");
  }
  return invoke<ConnectionSnapshot>("disconnect_remote");
}

export async function listAudioEndpoints(): Promise<AudioEndpoint[]> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法枚举 Windows 音频端点");
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
    throw new Error("当前是浏览器预览，无法选择 Windows 音频端点");
  }
  return invoke<AudioSnapshot>("select_audio_endpoint", { endpointId });
}

export async function getRawInputSnapshot(): Promise<RawInputSnapshot> {
  if (!isTauriRuntime()) {
    return browserSnapshot.platform.rawInput;
  }
  return invoke<RawInputSnapshot>("get_raw_input_snapshot");
}

export async function startRawInput(): Promise<RawInputSnapshot> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法启动 Windows Raw Input");
  }
  return invoke<RawInputSnapshot>("start_raw_input");
}

export async function stopRawInput(): Promise<RawInputSnapshot> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法停止 Windows Raw Input");
  }
  return invoke<RawInputSnapshot>("stop_raw_input");
}

export async function getButtonMappings(): Promise<ButtonMappings> {
  if (!isTauriRuntime()) {
    return { actions: {} };
  }
  return invoke<ButtonMappings>("get_button_mappings");
}

export async function saveButtonMappings(mappings: ButtonMappings): Promise<ButtonMappings> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法保存 Windows 按键映射");
  }
  return invoke<ButtonMappings>("save_button_mappings", { mappings });
}

export async function testButtonMapping(button: RemoteButton): Promise<SendInputSnapshot> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法执行 Windows SendInput");
  }
  return invoke<SendInputSnapshot>("test_button_mapping", { button });
}

export async function getSendInputSnapshot(): Promise<SendInputSnapshot> {
  if (!isTauriRuntime()) {
    return { available: false, submittedBatches: 0, submittedEvents: 0, lastError: null };
  }
  return invoke<SendInputSnapshot>("get_send_input_snapshot");
}

export function connectionPhaseLabel(phase: ConnectionPhase): string {
  return {
    idle: "尚未连接",
    connecting: "正在打开遥控器",
    discovering: "正在发现 ATVV 服务与特征",
    awaiting_capabilities: "正在确认 ATVV 能力",
    ready: "BLE / ATVV 已就绪",
    streaming: "正在接收遥控器语音",
    draining: "正在结束本次语音",
    reconnecting: "正在等待重新连接遥控器",
    suspended: "Windows 已进入睡眠",
    disconnected: "遥控器已断开",
    failed: "连接失败",
  }[phase];
}

export function remoteModelLabel(model: RemoteModel): string {
  return {
    rc001: "小米蓝牙遥控器 2（RC001）",
    rc003: "小米蓝牙遥控器 2 Pro（RC003）",
    unknown: "型号待设备确认",
  }[model];
}

export function audioPhaseLabel(phase: AudioPhase): string {
  return {
    unconfigured: "尚未选择输出端点",
    ready: "WASAPI 已就绪",
    streaming: "正在写入 Windows 音频端点",
    draining: "正在排空 Windows 音频缓冲",
    failed: "WASAPI 输出失败",
    unsupported: "当前环境不支持 WASAPI",
  }[phase];
}
