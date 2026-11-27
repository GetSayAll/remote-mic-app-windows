import { afterEach, describe, expect, it, vi } from "vitest";
import { effectiveLanguage } from "./language-state";
import {
  approvePhoneRemote,
  clearPhoneRemoteTrustedDevices,
  denyPhoneRemote,
  openIOSAppStore,
  startPhoneRemoteIos,
  startPhoneRemoteWeb,
  stopPhoneRemoteIos,
  stopPhoneRemoteWeb,
} from "./bridge";

describe("phone remote browser preview state", () => {
  afterEach(() => {
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it("moves the iOS row through waiting, connected and idle", async () => {
    effectiveLanguage.value = "zh-Hans";
    const waiting = await startPhoneRemoteIos();
    expect(waiting.ios.state).toBe("waiting");

    const connected = await approvePhoneRemote("approval-1");
    expect(connected.ios.state).toBe("connected");
    expect(connected.approval).toBeNull();

    const idle = await stopPhoneRemoteIos();
    expect(idle.ios.state).toBe("idle");
  });

  it("keeps the web row independent from iOS", async () => {
    await startPhoneRemoteIos();
    const webWaiting = await startPhoneRemoteWeb();
    expect(webWaiting.web.state).toBe("waiting_for_scan");
    expect(webWaiting.ios.state).toBe("waiting");

    const webIdle = await stopPhoneRemoteWeb();
    expect(webIdle.web.state).toBe("idle");
    expect(webIdle.ios.state).toBe("waiting");

    const denied = await denyPhoneRemote("approval-1");
    expect(denied.ios.state).toBe("waiting");
    await stopPhoneRemoteIos();
  });

  it("clears trusted devices and opens the localized App Store URL", async () => {
    const snapshot = await clearPhoneRemoteTrustedDevices();
    expect(snapshot.trustedDeviceCount).toBe(0);

    const open = vi.spyOn(window, "open").mockImplementation(() => null);
    await openIOSAppStore();
    expect(open).toHaveBeenCalledWith(
      "https://apps.apple.com/app/%E6%97%A0%E7%BA%BF%E9%BA%A6/id6797208067",
      "_blank",
      "noopener,noreferrer",
    );
    open.mockRestore();
  });
});
