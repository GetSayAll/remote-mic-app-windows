import { describe, expect, it } from "vitest";
import { ENGLISH_TRANSLATIONS } from "./i18n";
import {
  USB_PORT_HINT_ATTEMPT,
  USB_PORT_HINT_TEXT,
  USB_PORT_HINT_TITLE,
  shouldShowUsbPortHint,
} from "./usb-port-hint";

const base = {
  reconnecting: true,
  reconnectAttempt: USB_PORT_HINT_ATTEMPT,
  rawInputPhase: "awaiting" as const,
  matchedDeviceCount: 0,
};

describe("shouldShowUsbPortHint", () => {
  it("持续重连且按键接口缺席时显示", () => {
    expect(shouldShowUsbPortHint(base)).toBe(true);
  });

  it("重连次数不足时不显示", () => {
    expect(
      shouldShowUsbPortHint({
        ...base,
        reconnectAttempt: USB_PORT_HINT_ATTEMPT - 1,
      }),
    ).toBe(false);
  });

  it("按键接口已就绪或已有匹配设备时不显示", () => {
    expect(shouldShowUsbPortHint({ ...base, rawInputPhase: "ready" })).toBe(false);
    expect(shouldShowUsbPortHint({ ...base, matchedDeviceCount: 1 })).toBe(false);
  });

  it("不在重连相位时不显示", () => {
    expect(shouldShowUsbPortHint({ ...base, reconnecting: false })).toBe(false);
  });

  it("英文界面有对应翻译", () => {
    expect(ENGLISH_TRANSLATIONS[USB_PORT_HINT_TITLE]).toBeTruthy();
    expect(ENGLISH_TRANSLATIONS[USB_PORT_HINT_TEXT]).toContain("USB 2.0");
  });
});
