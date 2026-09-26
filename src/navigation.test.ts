import { beforeEach, describe, expect, it } from "vitest";
import {
  detectReloadRecovery,
  isBrowserReloadAccelerator,
  isWindowCloseAccelerator,
  loadPersistedPage,
  navigationItems,
  persistActivePage,
  touchLiveness,
} from "./navigation";

describe("Windows navigation", () => {
  it("keeps the approved Mac-derived page order without empty entries", () => {
    expect(navigationItems.map((item) => item.id)).toEqual([
      "buttons",
      "connection",
      "permissions",
      "about",
    ]);
    expect(navigationItems.every((item) => item.label.length > 0)).toBe(true);
  });
});

describe("active page persistence (webview reload recovery)", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("round-trips the current page across a reload", () => {
    expect(loadPersistedPage()).toBeNull();
    persistActivePage("connection");
    expect(loadPersistedPage()).toBe("connection");
    persistActivePage("about");
    expect(loadPersistedPage()).toBe("about");
  });

  it("ignores stored values that are no longer valid page ids", () => {
    localStorage.setItem("sayall.activePage", "statistics");
    expect(loadPersistedPage()).toBeNull();
    localStorage.setItem("sayall.activePage", "");
    expect(loadPersistedPage()).toBeNull();
  });
});

describe("liveness heartbeat (renderer reload detection)", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("reports a reload recovery only when the previous heartbeat is fresh", () => {
    expect(detectReloadRecovery(1_000)).toBe(false);
    touchLiveness(2_000);
    expect(detectReloadRecovery(4_000)).toBe(true);
    touchLiveness(4_000);
    // 超过 5 秒窗口视为冷启动，不再上报。
    expect(detectReloadRecovery(10_000)).toBe(false);
  });

  it("treats empty or corrupt heartbeats as cold start", () => {
    expect(detectReloadRecovery(1_000)).toBe(false);
    localStorage.setItem("sayall.livenessHeartbeat", "not-a-number");
    expect(detectReloadRecovery(1_100)).toBe(false);
  });

  it("reports recovery for every reload, not only the first one", () => {
    touchLiveness(1_000);
    expect(detectReloadRecovery(2_000)).toBe(true);
    touchLiveness(3_000);
    expect(detectReloadRecovery(3_500)).toBe(true);
  });
});

describe("browser reload accelerator blocking", () => {
  it("matches F5 and Ctrl/Meta+R in any case", () => {
    expect(isBrowserReloadAccelerator({ key: "F5", ctrlKey: false, metaKey: false })).toBe(true);
    expect(isBrowserReloadAccelerator({ key: "r", ctrlKey: true, metaKey: false })).toBe(true);
    expect(isBrowserReloadAccelerator({ key: "R", ctrlKey: true, metaKey: false })).toBe(true);
    expect(isBrowserReloadAccelerator({ key: "r", ctrlKey: false, metaKey: true })).toBe(true);
  });

  it("leaves ordinary keys and unmodified R untouched", () => {
    expect(isBrowserReloadAccelerator({ key: "r", ctrlKey: false, metaKey: false })).toBe(false);
    expect(isBrowserReloadAccelerator({ key: "a", ctrlKey: true, metaKey: false })).toBe(false);
    expect(isBrowserReloadAccelerator({ key: "F6", ctrlKey: false, metaKey: false })).toBe(false);
    expect(isBrowserReloadAccelerator({ key: "F5", ctrlKey: true, metaKey: false })).toBe(true);
  });
});

describe("window close accelerator (Ctrl+W)", () => {
  function press(overrides: Partial<Parameters<typeof isWindowCloseAccelerator>[0]> = {}) {
    return isWindowCloseAccelerator({
      key: "w",
      ctrlKey: false,
      metaKey: false,
      altKey: false,
      shiftKey: false,
      ...overrides,
    });
  }

  it("matches Ctrl+W regardless of the letter case", () => {
    expect(press({ ctrlKey: true })).toBe(true);
    expect(press({ ctrlKey: true, key: "W" })).toBe(true);
  });

  it("requires Ctrl: Cmd+W alone stays a plain keystroke here", () => {
    expect(press()).toBe(false);
    expect(press({ metaKey: true })).toBe(false);
  });

  it("never claims combinations that mean something else on Windows", () => {
    expect(press({ ctrlKey: true, altKey: true })).toBe(false);
    // Ctrl+Shift+W 在浏览器语义里是"关闭所有窗口"——误判会让用户丢掉全部上下文。
    expect(press({ ctrlKey: true, shiftKey: true })).toBe(false);
    expect(press({ ctrlKey: true, key: "q" })).toBe(false);
    expect(press({ ctrlKey: true, key: "F4" })).toBe(false);
  });
});
