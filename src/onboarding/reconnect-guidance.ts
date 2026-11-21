/**
 * 长时间重连后的可操作排查卡（issue #104，2026-11-19）。
 *
 * 现场：Windows 里只有配对记录，遥控器 HID 接口没有出现（`raw_input` 停在
 * `awaiting`），`service_discovery` 持续 `gatt_status=1`。原向导只写「最多等约
 * 60 秒」，超时后没有任何可执行动作。判据只决定**是否显示排查卡**，不改变
 * 连接重试；卡里的动作仍是用户可见的普通按键、关闭其他主机和重新配对。
 */
import type { RawInputPhase } from "../lib/bridge";

/** 约 60 秒：2/4/8/16/30 秒退避累计到第 5 次尝试。 */
export const RECONNECT_GUIDANCE_DELAY_MS = 60_000;
/** 次数兜底：用户中途进入第②步时，不必再等一个完整的 60 秒。 */
export const RECONNECT_GUIDANCE_ATTEMPT = 5;

export interface ReconnectGuidanceInput {
  reconnecting: boolean;
  /** 当前重连尝试序号（`ConnectionSnapshot.reconnectAttempt`）。 */
  reconnectAttempt: number;
  /** 本次连续重连已经等待的时间；由组件在 60 秒时置为阈值。 */
  waitedMs: number;
  rawInputPhase: RawInputPhase;
  buttonObserved: boolean;
}

/**
 * 只在「仍在重连、已经等够、按键通道确实缺席」时显示排查卡。
 *
 * `rawInputPhase !== "awaiting"` 时不显示：监听器还在启动时按键可能只是还没到达；
 * 已就绪时由按键行自己的说明接管。已经收到过按键也不显示——下一步是继续操作，
 * 不是重新配对。
 */
export function shouldShowReconnectGuidance(input: ReconnectGuidanceInput): boolean {
  if (!input.reconnecting || input.buttonObserved) return false;
  if (input.rawInputPhase !== "awaiting") return false;
  return (
    input.waitedMs >= RECONNECT_GUIDANCE_DELAY_MS ||
    input.reconnectAttempt >= RECONNECT_GUIDANCE_ATTEMPT
  );
}
