/**
 * 遥控器「重置并重新配对」引导（2026-11-21 用户要求）。
 *
 * 只在用户**真的卡住**时出现，而不是看到某个名字就打扰：
 * - 应用已经显示「已连接」，但向导一直收不到普通按键；
 * - Raw Input 明确处于 `awaiting`（按键通道不在位）。
 *
 * 通用名 `MI RC` 只用于解释原因和选择文案，**不是触发条件**——仓库本就把它
 * 列为合法支持名，正常工作的遥控器也可能被 Windows 这样上报。无法连接的情况
 * 由第②步的重连排查卡负责（并在卡内补上重置步骤）。
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
  /** 连接就绪后已等待的毫秒数（由组件计时）。 */
  connectedWithoutButtonMs: number;
  /** Raw Input 阶段：`awaiting` 时按键通道不在位，也属于「等不到按键」。 */
  rawInputPhase: RawInputPhase;
}

/**
 * 显示条件（必须同时满足「没收到按键」和「确实卡在按键链路上」）：
 * - 已收到按键 → 永不显示（链路已经通了，重置只会打断用户）；
 * - 未连接 → 不显示（那是连接问题，由重连排查卡负责，并在其中给重置步骤）；
 * - 已连接但 20 秒没有按键 / 按键通道缺席 → 显示。
 *
 * `MI RC` 不参与这里的判定：名称只影响卡片文案，不影响是否打扰用户。
 */
export function shouldShowResetGuidance(input: ResetGuidanceInput): boolean {
  if (input.buttonObserved || !input.connected) return false;
  if (input.rawInputPhase === "awaiting") return true;
  return input.connectedWithoutButtonMs >= RESET_GUIDANCE_DELAY_MS;
}
