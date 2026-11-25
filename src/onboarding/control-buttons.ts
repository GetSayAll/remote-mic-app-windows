/**
 * 向导「按一下遥控器的普通按键」步骤的按键清单（2026-11-24 用户要求）。
 *
 * 静音键不在此步骤展示，也不参与「3 个不同按键」的计数与门禁；其余 12 键
 * 保持设计稿的固定顺序。展示与计数必须共用本清单，避免出现「按了但没亮、
 * 计数却增加」的不一致。
 */
import type { RemoteButton } from "../lib/bridge";

export const ONBOARDING_CONTROL_BUTTONS: readonly RemoteButton[] = [
  "power",
  "home",
  "back",
  "menu",
  "tv",
  "ok",
  "up",
  "down",
  "left",
  "right",
  "volume_up",
  "volume_down",
];

export function isOnboardingControlButton(button: RemoteButton): boolean {
  return ONBOARDING_CONTROL_BUTTONS.includes(button);
}
