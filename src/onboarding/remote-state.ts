/**
 * 向导连接 / 按键步骤的脱敏状态快照（2026-11-21 用户要求：用户卡住时能只靠日志定位）。
 *
 * 目的：现场日志里必须同时看到「连接阶段 + BLE 语音就绪 + Raw Input 阶段 +
 * 匹配接口数 + 重连次数 + 是否已收到按键 + 是否有错误」，否则无法区分
 * 「没连上」「连上了但没有按键通道」「通道在位但没有按键」。
 *
 * 输出是**单个稳定 token**（≤64 字符，ASCII），由 `detail` 字段携带；不含设备名、
 * 地址、HID 路径、窗口标题、错误原文或任何用户内容。计数按上限截断，避免 token 超长。
 */
import type { ConnectionPhase, RawInputPhase, RemoteModel } from "../lib/bridge";

const CONNECTION_PHASE_CODES: Record<ConnectionPhase, string> = {
  idle: "i",
  connecting: "c",
  discovering: "d",
  awaiting_capabilities: "ac",
  ready: "r",
  streaming: "s",
  draining: "dr",
  reconnecting: "rc",
  suspended: "su",
  disconnected: "dc",
  failed: "f",
};

const RAW_INPUT_PHASE_CODES: Record<RawInputPhase, string> = {
  stopped: "s",
  starting: "st",
  awaiting: "a",
  ready: "r",
  failed: "f",
  unsupported: "u",
};

export function connectionPhaseCode(phase: ConnectionPhase): string {
  return CONNECTION_PHASE_CODES[phase] ?? "unknown";
}

export function rawInputPhaseCode(phase: RawInputPhase): string {
  return RAW_INPUT_PHASE_CODES[phase] ?? "unknown";
}

export interface RemoteStateInput {
  connectionPhase: ConnectionPhase;
  bleVoiceReady: boolean;
  rawInputPhase: RawInputPhase;
  matchedDeviceCount: number;
  rawEventCount: number;
  semanticEdgeCount: number;
  staleRemoteEventCount: number;
  reconnectAttempt: number;
  deviceCount: number;
  buttonObserved: boolean;
  connectionErrorPresent: boolean;
  rawInputErrorPresent: boolean;
  /** 仅在需要区分机型时带入；模型是稳定产品 token，不是设备身份。 */
  remoteModel?: RemoteModel;
}

export interface RemoteStateOptions {
  /** 心跳带 `re_/se_/st_` 计数；状态跳变只带阶段，避免长 token。 */
  includeCounters?: boolean;
  includeDeviceCount?: boolean;
  includeButtonObserved?: boolean;
  /** Raw Input 绑定失败位；只在状态跳变日志里带，心跳 token 已接近上限。 */
  includeRawInputError?: boolean;
  includeModel?: boolean;
}

function capped(value: number, max: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(Math.trunc(value), max));
}

/**
 * 生成快照 token。两种实际用法都 ≤64 字符：
 * - 心跳：`includeCounters=true`、不含 model；
 * - 状态跳变：`includeCounters=false`、按需含 model。
 */
export function remoteStateToken(
  input: RemoteStateInput,
  options: RemoteStateOptions = {},
): string {
  const includeCounters = options.includeCounters ?? true;
  const includeDeviceCount = options.includeDeviceCount ?? true;
  const includeButtonObserved = options.includeButtonObserved ?? true;
  const includeRawInputError = options.includeRawInputError ?? false;
  const includeModel = options.includeModel ?? false;

  const parts = [
    `p_${connectionPhaseCode(input.connectionPhase)}`,
    `ble_${input.bleVoiceReady ? 1 : 0}`,
    `raw_${rawInputPhaseCode(input.rawInputPhase)}`,
    `m_${capped(input.matchedDeviceCount, 9)}`,
  ];
  if (includeCounters) {
    parts.push(`re_${capped(input.rawEventCount, 999)}`);
    parts.push(`se_${capped(input.semanticEdgeCount, 999)}`);
    parts.push(`st_${capped(input.staleRemoteEventCount, 999)}`);
  }
  parts.push(`rc_${capped(input.reconnectAttempt, 99)}`);
  if (includeDeviceCount) parts.push(`n_${capped(input.deviceCount, 9)}`);
  if (includeButtonObserved) parts.push(`obs_${input.buttonObserved ? 1 : 0}`);
  parts.push(`err_${input.connectionErrorPresent ? 1 : 0}`);
  if (includeRawInputError) parts.push(`ce_${input.rawInputErrorPresent ? 1 : 0}`);
  if (includeModel && input.remoteModel) parts.push(`md_${input.remoteModel}`);
  return parts.join("_");
}
