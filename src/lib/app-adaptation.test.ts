import { describe, expect, it } from "vitest";
import type {
  ButtonMappings,
  MacroDefinition,
  PresetAppInfo,
} from "./bridge";
import {
  appFileName,
  buildAppRows,
  compactFocusLabel,
  effectiveAppProfile,
  findAppBindings,
  findMacroReferences,
  mergeApplications,
  nextSelectedTarget,
  removeAppAdaptation,
} from "./app-adaptation";

const PRESETS: PresetAppInfo[] = [
  { id: "wechat", name: "微信", installed: true },
  { id: "netease_music", name: "网易云音乐", installed: true },
  { id: "chrome", name: "Chrome 浏览器", installed: true },
];

function mappings(overrides: Partial<ButtonMappings> = {}): ButtonMappings {
  return { enabled: true, actions: {}, ...overrides };
}

describe("app adaptation rows", () => {
  it("lists added apps first, then only presets with an explicit profile", () => {
    const rows = buildAppRows(
      [
        { name: "Cursor", path: "C:\\Apps\\Cursor.exe" },
        { name: "微信", path: "shell:AppsFolder\\Tencent.WeChat" },
      ],
      {
        netease_music: { strategy: "open_only" },
        chrome: { strategy: "auto_composer" },
      },
      PRESETS,
    );

    expect(rows.map((row) => row.target)).toEqual([
      "C:\\Apps\\Cursor.exe",
      "shell:AppsFolder\\Tencent.WeChat",
      "netease_music",
      "chrome",
    ]);
    // 没有显式档案的预设不进入列表。
    expect(rows.some((row) => row.target === "wechat")).toBe(false);
    expect(rows[2]).toMatchObject({ preset: true, name: "网易云音乐" });
  });

  it("can expose one preset that the caller navigated to", () => {
    const rows = buildAppRows([], {}, PRESETS, ["chrome"]);
    expect(rows).toEqual([
      { target: "chrome", name: "Chrome 浏览器", preset: true, installed: true },
    ]);
  });

  it("deduplicates added apps by case-insensitive path and keeps first name", () => {
    const rows = buildAppRows(
      [
        { name: "Cursor", path: "C:\\Apps\\Cursor.exe" },
        { name: "cursor duplicate", path: "c:\\apps\\cursor.exe" },
      ],
      {},
      PRESETS,
    );

    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("Cursor");
  });

  it("uses the file name when a scanned app has no display name", () => {
    expect(appFileName("C:\\Apps\\Cursor.exe")).toBe("Cursor");
    expect(appFileName("shell:AppsFolder\\Tencent.WeChat")).toBe(
      "Tencent.WeChat",
    );
  });

  it("merges scanned apps without duplicating existing paths", () => {
    const merged = mergeApplications(
      [{ name: "Cursor", path: "C:\\Apps\\Cursor.exe" }],
      [
        { name: "Cursor again", path: "c:\\apps\\cursor.exe" },
        { name: "微信", path: "C:\\WeChat\\Weixin.exe" },
      ],
    );

    expect(merged).toEqual([
      { name: "Cursor", path: "C:\\Apps\\Cursor.exe" },
      { name: "微信", path: "C:\\WeChat\\Weixin.exe" },
    ]);
  });
});

describe("focus labels", () => {
  it("uses the preset default when no explicit profile exists", () => {
    const profile = effectiveAppProfile(
      { target: "chrome", preset: true },
      {},
    );
    expect(profile?.strategy).toBe("app_shortcut");
    expect(compactFocusLabel(profile)).toBe("快捷键");
  });

  it("keeps an explicit preset profile ahead of the built-in default", () => {
    const profile = effectiveAppProfile(
      { target: "netease_music", preset: true },
      { netease_music: { strategy: "open_only" } },
    );
    expect(profile?.strategy).toBe("open_only");
    expect(compactFocusLabel(profile)).toBe("只打开");
  });

  it("reads a stored profile whose path case differs from the app row", () => {
    const profile = effectiveAppProfile(
      { target: "C:\\Apps\\Cursor.exe", preset: false },
      { "c:\\apps\\cursor.exe": { strategy: "recorded_point" } },
    );
    expect(profile?.strategy).toBe("recorded_point");
  });

  it("returns no profile for a custom app without an explicit profile", () => {
    expect(
      effectiveAppProfile(
        { target: "C:\\Apps\\Cursor.exe", preset: false },
        {},
      ),
    ).toBeNull();
    expect(compactFocusLabel(null)).toBe("只打开");
  });
});

describe("bindings", () => {
  it("finds every trigger that opens the target", () => {
    const bindings = findAppBindings(
      {
        ok: {
          single: { type: "open_app", target: "C:\\Apps\\Cursor.exe" },
          double: { type: "disabled" },
          long: { type: "open_app", target: "c:\\apps\\cursor.exe" },
        },
        home: {
          single: { type: "open_app", target: "chrome" },
          double: { type: "disabled" },
          long: { type: "disabled" },
        },
      },
      "C:\\Apps\\Cursor.exe",
    );

    expect(bindings).toEqual([
      { button: "ok", trigger: "single" },
      { button: "ok", trigger: "long" },
    ]);
  });

  it("finds macro steps that open, wait for, or focus the target", () => {
    const macro = (macroId: string, target: string): MacroDefinition => ({
      schemaVersion: "1",
      macroId,
      version: "1",
      name: macroId,
      steps: [
        {
          stepId: "step.01",
          action: "focus_input",
          parameters: { target },
        },
      ],
    });

    expect(
      findMacroReferences(
        [macro("routine-a", "C:\\Apps\\Cursor.exe"), macro("routine-b", "chrome")],
        "c:\\apps\\cursor.exe",
      ),
    ).toEqual([{ macroId: "routine-a", name: "routine-a" }]);
  });
});

describe("deleting an adaptation", () => {
  it("removes a custom app from both the library and profile map", () => {
    const result = removeAppAdaptation(
      mappings({
        applications: [
          { name: "Cursor", path: "C:\\Apps\\Cursor.exe" },
          { name: "微信", path: "C:\\WeChat\\Weixin.exe" },
        ],
        focusProfiles: {
          "C:\\Apps\\Cursor.exe": { strategy: "recorded_element" },
          "C:\\WeChat\\Weixin.exe": { strategy: "open_only" },
        },
      }),
      "c:\\apps\\cursor.exe",
      false,
    );

    expect(result.applications).toEqual([
      { name: "微信", path: "C:\\WeChat\\Weixin.exe" },
    ]);
    expect(result.focusProfiles).toEqual({
      "C:\\WeChat\\Weixin.exe": { strategy: "open_only" },
    });
  });

  it("only clears the explicit profile for a preset", () => {
    const result = removeAppAdaptation(
      mappings({
        focusProfiles: {
          chrome: { strategy: "open_only" },
          wechat: { strategy: "recorded_element" },
        },
      }),
      "chrome",
      true,
    );

    expect(result.focusProfiles).toEqual({
      wechat: { strategy: "recorded_element" },
    });
    expect(result.applications).toBeUndefined();
  });
});

describe("selection", () => {
  const rows = buildAppRows(
    [{ name: "Cursor", path: "C:\\Apps\\Cursor.exe" }],
    { chrome: { strategy: "open_only" } },
    PRESETS,
  );

  it("keeps an existing selection", () => {
    expect(nextSelectedTarget(rows, "chrome")).toBe("chrome");
  });

  it("falls back to the first row when the selection disappeared", () => {
    expect(nextSelectedTarget(rows, "missing")).toBe("C:\\Apps\\Cursor.exe");
    expect(nextSelectedTarget([], "missing")).toBeNull();
  });
});
