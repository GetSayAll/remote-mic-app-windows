// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getSystemAccentColor, subscribeAccentChanges } from "./bridge";
import {
  applyAccentPalette,
  contrastRatio,
  deriveAccentPalette,
  disposeAccentColor,
  initializeAccentColor,
  type AccentRgb,
} from "./accent";

vi.mock("./bridge", () => ({
  getSystemAccentColor: vi.fn(),
  subscribeAccentChanges: vi.fn(),
  isTauriRuntime: () => false,
}));

const getAccentMock = vi.mocked(getSystemAccentColor);
const subscribeMock = vi.mocked(subscribeAccentChanges);

const WINDOWS_BLUE: AccentRgb = { r: 0, g: 120, b: 212 };
const WINDOWS_GOLD: AccentRgb = { r: 255, g: 200, b: 61 };
const WHITE: AccentRgb = { r: 255, g: 255, b: 255 };
const BLACK: AccentRgb = { r: 0, g: 0, b: 0 };
const LIGHT_CANVAS: AccentRgb = { r: 244, g: 245, b: 248 };
const DARK_CANVAS: AccentRgb = { r: 21, g: 22, b: 26 };
const DARK_TEXT_ON_ACCENT: AccentRgb = { r: 23, g: 24, b: 34 };

function parseRgb(value: string): AccentRgb {
  const match = /^rgba?\((\d+), (\d+), (\d+)/.exec(value);
  expect(match, `不是可解析的 rgb/rgba 颜色：${value}`).toBeTruthy();
  return { r: Number(match![1]), g: Number(match![2]), b: Number(match![3]) };
}

/** 与浅底（近似白）对比。 */
function contrastAgainstWhite(color: AccentRgb): number {
  return contrastRatio(color, WHITE);
}

/** 与深底对比。 */
function contrastAgainstDarkCanvas(color: AccentRgb): number {
  return contrastRatio(color, DARK_CANVAS);
}

describe("contrastRatio", () => {
  it("黑/白对比为 21，同色为 1，且对称", () => {
    expect(contrastRatio(BLACK, WHITE)).toBeCloseTo(21, 1);
    expect(contrastRatio(WHITE, BLACK)).toBeCloseTo(21, 1);
    expect(contrastRatio(WINDOWS_BLUE, WINDOWS_BLUE)).toBe(1);
  });

  it("Windows 默认蓝对白底约 4.53（原色可用作白字按钮底）", () => {
    expect(contrastAgainstWhite(WINDOWS_BLUE)).toBeCloseTo(4.53, 1);
    expect(contrastAgainstWhite(WINDOWS_BLUE)).toBeGreaterThanOrEqual(4.5);
  });
});

describe("deriveAccentPalette", () => {
  it("输出完整变量集合（12 个，覆盖 styles.css 全部强调色族）", () => {
    const palette = deriveAccentPalette(WINDOWS_BLUE, "light");
    expect(Object.keys(palette).sort()).toEqual(
      [
        "--accent",
        "--accent-dark",
        "--accent-text",
        "--accent-muted-text",
        "--accent-surface",
        "--accent-surface-strong",
        "--accent-border",
        "--accent-hover",
        "--canvas-glow",
        "--dot-active",
        "--data-button",
        "--empty-icon",
      ].sort(),
    );
  });

  it("浅色主题 + Windows 默认蓝：主色保持原样，白字可读（≥4.5）", () => {
    const palette = deriveAccentPalette(WINDOWS_BLUE, "light");
    expect(palette["--accent"]).toBe("rgb(0, 120, 212)");
    expect(contrastAgainstWhite(parseRgb(palette["--accent"]))).toBeGreaterThanOrEqual(4.5);
    expect(palette["--accent-dark"]).toBe("rgb(0, 106, 187)");
    expect(palette["--accent-text"]).toBe("rgb(0, 120, 212)");
    // 次级文字在可读前提下取"边缘可读"版本：仍达标，但不比主文字更醒目。
    const muted = parseRgb(palette["--accent-muted-text"]);
    expect(contrastAgainstWhite(muted)).toBeGreaterThanOrEqual(4.5);
    expect(contrastAgainstWhite(muted)).toBeLessThanOrEqual(
      contrastAgainstWhite(parseRgb(palette["--accent-text"])),
    );
  });

  it("浅色主题 + 过浅强调色（金黄）：主色被调暗到白字可读，图形色跟随原色", () => {
    const palette = deriveAccentPalette(WINDOWS_GOLD, "light");
    expect(palette["--accent"]).not.toBe("rgb(255, 200, 61)");
    expect(contrastAgainstWhite(parseRgb(palette["--accent"]))).toBeGreaterThanOrEqual(4.5);
    expect(contrastAgainstWhite(parseRgb(palette["--accent-text"]))).toBeGreaterThanOrEqual(4.5);
    // dot/data-button 等图形色不做文字级 clamp，但必须与画布可区分（≥3）。
    expect(contrastRatio(parseRgb(palette["--dot-active"]), LIGHT_CANVAS)).toBeGreaterThanOrEqual(
      3,
    );
  });

  it("浅色主题 alpha 变体与现有紫色版透明度一致", () => {
    const palette = deriveAccentPalette(WINDOWS_BLUE, "light");
    expect(palette["--accent-surface"]).toBe("rgba(0, 120, 212, 0.1)");
    expect(palette["--accent-surface-strong"]).toBe("rgba(0, 120, 212, 0.14)");
    expect(palette["--accent-border"]).toBe("rgba(0, 120, 212, 0.45)");
    expect(palette["--accent-hover"]).toBe("rgba(0, 120, 212, 0.1)");
    expect(palette["--canvas-glow"]).toBe("rgba(0, 120, 212, 0.12)");
  });

  it("深色主题 + Windows 默认蓝：主色提亮到黑字可读且在深底可见", () => {
    const palette = deriveAccentPalette(WINDOWS_BLUE, "dark");
    const accent = parseRgb(palette["--accent"]);
    expect(accent).not.toEqual(WINDOWS_BLUE);
    // 主按钮底色上的文字（深色主题 text-on-accent ≈ #171822）。
    expect(contrastRatio(accent, DARK_TEXT_ON_ACCENT)).toBeGreaterThanOrEqual(4.5);
    // 焦点环 / 选中连线在深底上必须可见。
    expect(contrastAgainstDarkCanvas(accent)).toBeGreaterThanOrEqual(3);
    // hover 更亮（深色主题惯例）。
    const hover = parseRgb(palette["--accent-dark"]);
    expect(contrastAgainstDarkCanvas(hover)).toBeGreaterThanOrEqual(
      contrastAgainstDarkCanvas(accent),
    );
  });

  it("深色主题文字变量在深底上可读（≥4.5）", () => {
    const palette = deriveAccentPalette(WINDOWS_GOLD, "dark");
    for (const key of ["--accent-text", "--accent-muted-text"]) {
      expect(contrastAgainstDarkCanvas(parseRgb(palette[key]))).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("深色主题 alpha 变体：底色与主色同源，透明度沿用紫色深色版数值", () => {
    const palette = deriveAccentPalette(WINDOWS_BLUE, "dark");
    // surface 底色跟随提亮后的主色（与 --accent 同源，避免叠加处颜色打架）。
    const accent = parseRgb(palette["--accent"]);
    const alpha = (variable: string, a: string): void => {
      expect(palette[variable]).toBe(
        `rgba(${accent.r}, ${accent.g}, ${accent.b}, ${a})`,
      );
    };
    alpha("--accent-surface", "0.15");
    alpha("--accent-surface-strong", "0.21");
    alpha("--accent-border", "0.52");
    alpha("--accent-hover", "0.18");
    const graphic = parseRgb(palette["--dot-active"]);
    expect(palette["--canvas-glow"]).toBe(
      `rgba(${graphic.r}, ${graphic.g}, ${graphic.b}, 0.09)`,
    );
  });
});

describe("initializeAccentColor", () => {
  beforeEach(() => {
    getAccentMock.mockReset();
    subscribeMock.mockReset().mockResolvedValue(() => undefined);
    vi.spyOn(console, "info").mockImplementation(() => undefined);
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    document.documentElement.style.removeProperty("--accent");
    document.documentElement.style.removeProperty("--accent-text");
    document.documentElement.dataset.theme = "light";
  });

  afterEach(() => {
    disposeAccentColor();
    document.documentElement.style.removeProperty("--accent");
    document.documentElement.style.removeProperty("--accent-text");
    vi.restoreAllMocks();
  });

  it("读取成功：按当前文档主题注入变量", async () => {
    getAccentMock.mockResolvedValue(WINDOWS_BLUE);
    await initializeAccentColor();

    expect(document.documentElement.style.getPropertyValue("--accent")).toBe(
      "rgb(0, 120, 212)",
    );
    expect(document.documentElement.style.getPropertyValue("--accent-text")).toBe(
      "rgb(0, 120, 212)",
    );
  });

  it("读取失败：不注入任何变量（CSS 内置紫色默认保留）", async () => {
    getAccentMock.mockResolvedValue(null);
    await initializeAccentColor();

    expect(document.documentElement.style.getPropertyValue("--accent")).toBe("");
    expect(console.warn).toHaveBeenCalledWith(
      expect.stringContaining("feature=accent"),
    );
  });

  it("主题切换：applyAccentPalette 按新主题重算变量", async () => {
    getAccentMock.mockResolvedValue(WINDOWS_BLUE);
    await initializeAccentColor();

    const lightAccent = document.documentElement.style.getPropertyValue("--accent");
    document.documentElement.dataset.theme = "dark";
    applyAccentPalette("dark");
    const darkAccent = document.documentElement.style.getPropertyValue("--accent");

    expect(darkAccent).not.toBe(lightAccent);
    expect(contrastRatio(parseRgb(darkAccent), DARK_TEXT_ON_ACCENT)).toBeGreaterThanOrEqual(4.5);
  });

  it("系统强调色变化事件：更新缓存并立即重注入（无需重启）", async () => {
    getAccentMock.mockResolvedValue(WINDOWS_BLUE);
    let push: ((color: AccentRgb) => void) | null = null;
    subscribeMock.mockImplementation(async (handler) => {
      push = handler;
      return () => undefined;
    });
    await initializeAccentColor();

    const before = document.documentElement.style.getPropertyValue("--accent");
    expect(push).toBeTruthy();
    push!(WINDOWS_GOLD);

    const after = document.documentElement.style.getPropertyValue("--accent");
    expect(after).not.toBe(before);
    expect(after).not.toBe("rgb(255, 200, 61)"); // 浅色主题下被 clamp 到白字可读
    expect(contrastAgainstWhite(parseRgb(after))).toBeGreaterThanOrEqual(4.5);
  });
});
