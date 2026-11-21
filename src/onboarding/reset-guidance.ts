/**
 * 遥控器「重置并重新配对」引导（2026-11-21 用户要求）。
 *
 * 两个现场条件都说明遥控器侧状态需要清掉，而不是继续等连接：
 * - 应用已经显示「已连接」，但向导一直收不到普通按键；
 * - Windows 报出的设备名是通用名 `MI RC`（没有完成初始化 / 仍停在旧的配对状态）。
 *
 * 判据只决定**是否显示引导卡**，不改变连接与重试逻辑；动作仍是用户可见的
 * 「主页 + 菜单重置 → 删除旧配对 → TV 重新配对」。
 *
 * 纯逻辑：组件负责计时，本模块只给出可测试的布尔判定。
 */
import type { RawInputPhase } from "../lib/bridge";

/** 连接就绪后等待按键的时间；到点仍无按键才提示重置，避免正常节奏下误报。 */
export const RESET_GUIDANCE_DELAY_MS = 20_000;

/** 小米遥控器未初始化 / 未完成配对时 Windows 常显示的通用名。 */
const GENERIC_REMOTE_NAMES = new Set(["mi rc"]);

/** 设备名是否为通用名（大小写与首尾空白不敏感）。 */
export function isGenericRemoteName(name: string | null | undefined): boolean {
  if (typeof name !== "string") return false;
  return GENERIC_REMOTE_NAMES.has(name.trim().toLowerCase());
}

export interface ResetGuidanceInput {
  /** 当前连接阶段是否属于「已连接」。 */
  connected: boolean;
  /** 本向导会话里是否已收到过普通按键。 */
  buttonObserved: boolean;
  /** 当前连接的设备或扫描列表里出现了通用名 `MI RC`。 */
  genericRemoteName: boolean;
  /** 连接就绪后已等待的毫秒数（由组件计时）。 */
  connectedWithoutButtonMs: number;
  /** Raw Input 阶段：`awaiting` 时按键通道不在位，也属于「等不到按键」。 */
  rawInputPhase: RawInputPhase;
}

/**
 * 显示条件：
 * - 已收到按键 → 永不显示（链路已经通了，重置只会打断用户）；
 * - 通用名 `MI RC` → 立即显示（名字本身就说明遥控器需要重新初始化）；
 * - 已连接且 20 秒没有按键 / 按键通道缺席 → 显示；
 * - 未连接时不显示（那是连接问题，由重连排查卡负责）。
 */
export function shouldShowResetGuidance(input: ResetGuidanceInput): boolean {
  if (input.buttonObserved) return false;
  if (input.genericRemoteName) return true;
  if (!input.connected) return false;
  if (input.rawInputPhase === "awaiting") return true;
  return input.connectedWithoutButtonMs >= RESET_GUIDANCE_DELAY_MS;
}
