import type { RawInputPhase } from "./bridge";
import { reportFrontendEvent, reasonToken } from "./frontend-diagnostics";

/**
 * USB 2.0 排查提示（2026-11-26，issue #104 现场）。
 *
 * 现场日志只能证明 BLE 链路没有建立，并不能证明 USB 3.0 是根因；提示文案因此写成
 * “如果使用外接 USB 蓝牙适配器……”的条件句，不声称已检测到 USB 3.0。
 *
 * 只在「持续重连 + 按键接口仍未出现」时出现：HID 已就绪时下一步是按键排查，
 * 不是换 USB 接口；次数阈值与向导的重连排查卡一致（约 60 秒）。
 */
export const USB_PORT_HINT_ATTEMPT = 5;

export const USB_PORT_HINT_TITLE = "可以换一个 USB 接口再试";
export const USB_PORT_HINT_TEXT =
  "如果遥控器是通过外接 USB 蓝牙适配器连接的，换一个 USB 2.0 接口再试（通常是黑色接口）；部分 USB 3.0 接口会干扰蓝牙。换好后重新插拔适配器，应用会继续自动重试。";

export interface UsbPortHintInput {
  reconnecting: boolean;
  reconnectAttempt: number;
  rawInputPhase: RawInputPhase;
  matchedDeviceCount: number;
}

export function shouldShowUsbPortHint(input: UsbPortHintInput): boolean {
  return (
    input.reconnecting &&
    input.reconnectAttempt >= USB_PORT_HINT_ATTEMPT &&
    input.rawInputPhase === "awaiting" &&
    input.matchedDeviceCount === 0
  );
}

/** 提示曝光事件；只记稳定 token，不含设备身份或用户内容。 */
export function reportUsbPortHintShown(
  scope: "connection" | "onboarding",
  attempt: number,
  rawInputPhase: RawInputPhase,
): void {
  reportFrontendEvent({
    event: "usb_hint",
    phase: "shown",
    result: "unknown",
    reason: reasonToken("scope", scope, "attempt", attempt, "hid", rawInputPhase),
  });
}

export function reportUsbPortHintDismissed(
  scope: "connection" | "onboarding",
  attempt: number,
): void {
  reportFrontendEvent({
    event: "usb_hint",
    phase: "dismissed",
    result: "unknown",
    reason: reasonToken("scope", scope, "attempt", attempt, "user_dismissed"),
  });
}
