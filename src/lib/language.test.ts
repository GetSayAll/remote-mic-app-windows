// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getApplicationLanguage, saveApplicationLanguage } from "./bridge";
import {
  disposeLanguage,
  initializeLanguage,
  resolveLanguage,
  setLanguagePreference,
  useLanguage,
} from "./language";

vi.mock("./bridge", () => ({
  getApplicationLanguage: vi.fn(),
  saveApplicationLanguage: vi.fn(),
  isTauriRuntime: () => false,
}));
vi.mock("./frontend-diagnostics", () => ({
  reportFrontendEvent: vi.fn(),
  reasonToken: (...parts: Array<string | boolean | number | null | undefined>) =>
    parts.filter((part) => part !== null && part !== undefined).join("_"),
}));

const getPreferenceMock = vi.mocked(getApplicationLanguage);
const savePreferenceMock = vi.mocked(saveApplicationLanguage);

describe("language controller", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.lang = "";
    getPreferenceMock.mockReset().mockResolvedValue("system");
    savePreferenceMock.mockReset().mockImplementation(async (value) => value);
  });

  afterEach(() => {
    disposeLanguage();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("system 跟随系统中文/英文，显式选择优先", () => {
    expect(resolveLanguage("system", "zh-CN")).toBe("zh-Hans");
    expect(resolveLanguage("system", "zh-Hant-TW")).toBe("zh-Hans");
    expect(resolveLanguage("system", "en-US")).toBe("en");
    expect(resolveLanguage("system", "fr-FR")).toBe("en");
    expect(resolveLanguage("zh-Hans", "en-US")).toBe("zh-Hans");
    expect(resolveLanguage("en", "zh-CN")).toBe("en");
  });

  it("初始化读取原生语言，应用到文档并缓存", async () => {
    getPreferenceMock.mockResolvedValue("en");
    await initializeLanguage();

    expect(getPreferenceMock).toHaveBeenCalledWith(expect.stringMatching(/^language-/));
    expect(useLanguage().preference.value).toBe("en");
    expect(useLanguage().effectiveLanguage.value).toBe("en");
    expect(document.documentElement.lang).toBe("en");
    expect(localStorage.getItem("sayall-language-preference")).toBe("en");
  });

  it("切换语言写入并生效；保存失败回滚", async () => {
    await initializeLanguage();
    savePreferenceMock.mockResolvedValueOnce("en");
    await setLanguagePreference("en");

    expect(savePreferenceMock).toHaveBeenCalledWith("en", expect.stringMatching(/^language-/));
    expect(useLanguage().preference.value).toBe("en");
    expect(useLanguage().effectiveLanguage.value).toBe("en");
    expect(document.documentElement.lang).toBe("en");

    savePreferenceMock.mockRejectedValueOnce(new Error("disk full"));
    await setLanguagePreference("zh-Hans");
    expect(useLanguage().preference.value).toBe("en");
    expect(useLanguage().effectiveLanguage.value).toBe("en");
    expect(useLanguage().errorMessage.value).toContain("保存失败");
  });
});
