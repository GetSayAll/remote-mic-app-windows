import { invoke } from "@tauri-apps/api/core";

export type ConnectionPhase =
  | "idle"
  | "connecting"
  | "discovering"
  | "awaiting_capabilities"
  | "ready"
  | "streaming"
  | "draining"
  | "disconnected"
  | "failed";

export type VoiceSessionState = "idle" | "streaming" | "draining";

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
  capabilities: AtvvCapabilities | null;
  voiceState: VoiceSessionState;
  decodedSamples: number;
  generation: number;
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
}

export interface RuntimeSnapshot {
  appVersion: string;
  platform: PlatformSnapshot;
}

export interface PairedRemote {
  id: string;
  name: string;
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
    verificationStatus: "浏览器预览仅展示界面，不代表 Windows 或 RC003 已通过",
    connection: {
      phase: "idle",
      remoteName: null,
      capabilities: null,
      voiceState: "idle",
      decodedSamples: 0,
      generation: 0,
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

export function connectionPhaseLabel(phase: ConnectionPhase): string {
  return {
    idle: "尚未连接",
    connecting: "正在打开 RC003",
    discovering: "正在发现 ATVV 服务与特征",
    awaiting_capabilities: "正在确认 ATVV 能力",
    ready: "BLE / ATVV 已就绪",
    streaming: "正在接收 RC003 语音",
    draining: "正在结束本次语音",
    disconnected: "RC003 已断开",
    failed: "连接失败",
  }[phase];
}
