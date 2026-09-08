// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getThemePreference, reportThemeResult, saveThemePreference } from "./bridge";
import { disposeTheme, initializeTheme, setThemePreference, useTheme } from "./theme";

vi.mock("./bridge", () => ({
  getThemePreference: vi.fn(),
  reportThemeResult: vi.fn(),
  saveThemePreference: vi.fn(),
  isTauriRuntime: () => false,
}));

const getPreferenceMock = vi.mocked(getThemePreference);
const savePreferenceMock = vi.mocked(saveThemePreference);
const reportThemeResultMock = vi.mocked(reportThemeResult);

function installMatchMedia(initiallyDark: boolean) {
  let dark = initiallyDark;
  const listeners = new Set<(event: MediaQueryListEvent) => void>();
  const query = {
    get matches() {
      return dark;
    },
    media: "(prefers-color-scheme: dark)",
    onchange: null,
    addEventListener: (_type: string, listener: (event: MediaQueryListEvent) => void) =>
      listeners.add(listener),
    removeEventListener: (_type: string, listener: (event: MediaQueryListEvent) => void) =>
      listeners.delete(listener),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  } as unknown as MediaQueryList;
  vi.stubGlobal("matchMedia", vi.fn(() => query));
  return {
    change(nextDark: boolean) {
      dark = nextDark;
      const event = { matches: dark, media: query.media } as MediaQueryListEvent;
      listeners.forEach((listener) => listener(event));
    },
  };
}

describe("theme controller", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
    getPreferenceMock.mockReset().mockResolvedValue("system");
    savePreferenceMock.mockReset().mockImplementation(async (value) => value);
    reportThemeResultMock.mockReset().mockResolvedValue(undefined);
    vi.spyOn(console, "info").mockImplementation(() => undefined);
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
  });

  afterEach(() => {
    disposeTheme();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("系统模式按媒体查询初始化并实时切换", async () => {
    const media = installMatchMedia(true);
    await initializeTheme();

    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(useTheme().preference.value).toBe("system");

    media.change(false);
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("固定深色立即生效并忽略后续系统变化", async () => {
    const media = installMatchMedia(false);
    await initializeTheme();
    await setThemePreference("dark");

    expect(savePreferenceMock).toHaveBeenCalledWith("dark", expect.stringMatching(/^theme-/));
    expect(useTheme().preference.value).toBe("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(localStorage.getItem("sayall-theme-preference")).toBe("dark");
    expect(reportThemeResultMock).toHaveBeenLastCalledWith(
      expect.objectContaining({
        action: "change",
        preference: "dark",
        resolvedTheme: "dark",
        terminalResult: "passed",
        reason: "applied",
      }),
    );

    media.change(true);
    media.change(false);
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("保存失败时回滚选择和有效主题", async () => {
    installMatchMedia(false);
    await initializeTheme();
    savePreferenceMock.mockRejectedValueOnce(new Error("disk full"));

    await setThemePreference("dark");

    expect(useTheme().preference.value).toBe("system");
    expect(useTheme().errorMessage.value).toContain("保存失败");
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(reportThemeResultMock).toHaveBeenLastCalledWith(
      expect.objectContaining({
        action: "change",
        preference: "system",
        resolvedTheme: "light",
        terminalResult: "failed",
        reason: "apply_or_save_failed",
      }),
    );
  });
});
