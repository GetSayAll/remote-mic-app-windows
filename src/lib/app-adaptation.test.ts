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
  scrollRegionForTarget,
} from "./app-adaptation";

const PRESETS: PresetAppInfo[] = [
  { id: "wechat", name: "微信", installed: true, path: "C:\\Apps\\Weixin.exe" },
  {
    id: "netease_music",
    name: "网易云音乐",
    installed: true,
    path: "C:\\Program Files\\Netease\\CloudMusic\\cloudmusic.exe",
  },
  {
    id: "chrome",
    name: "Chrome 浏览器",
    installed: true,
    path: "C:\\Program Files\\Chrome\\chrome.exe",
  },
];

function mappings(overrides: Partial<ButtonMappings> = {}): ButtonMappings {
  return { enabled: true, actions: {}, ...overrides };
}

describe("app adaptation rows", () => {
  it("lists only manually added apps and marks a library entry that matches a preset", () => {
    const rows = buildAppRows(
      [
        { name: "Cursor", path: "C:\\Apps\\Cursor.exe" },
        { name: "Chrome", path: "C:\\Program Files\\Chrome\\chrome.exe" },
        { name: "微信", path: "shell:AppsFolder\\Tencent.WeChat" },
      ],
      {},
      PRESETS,
    );

    expect(rows.map((row) => row.target)).toEqual([
      "C:\\Apps\\Cursor.exe",
      "C:\\Program Files\\Chrome\\chrome.exe",
      "shell:AppsFolder\\Tencent.WeChat",
    ]);
    expect(rows[0]).toMatchObject({ preset: false });
    expect(rows[1]).toMatchObject({
      preset: true,
      presetId: "chrome",
      name: "Chrome",
      path: "C:\\Program Files\\Chrome\\chrome.exe",
    });
    expect(rows[2]).toMatchObject({ preset: false });
  });

  it("shows a navigated preset without adding it to the library", () => {
    const rows = buildAppRows(
      [{ name: "Cursor", path: "C:\\Apps\\Cursor.exe" }],
      {},
      PRESETS,
      ["chrome"],
    );
    expect(rows.map((row) => row.target)).toEqual([
      "C:\\Apps\\Cursor.exe",
      "chrome",
    ]);
    expect(rows[1]).toMatchObject({
      preset: true,
      presetId: "chrome",
      name: "Chrome 浏览器",
      path: "C:\\Program Files\\Chrome\\chrome.exe",
      installed: true,
    });
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
  it("uses the preset id when a manually added app matches a preset", () => {
    const profile = effectiveAppProfile(
      {
        target: "C:\\Program Files\\Chrome\\chrome.exe",
        preset: true,
        presetId: "chrome",
      },
      {},
    );
    expect(profile?.strategy).toBe("app_shortcut");
    expect(compactFocusLabel(profile)).toBe("快捷键");
  });

  it("preset rows use explicit profiles before preset defaults", () => {
    const profile = effectiveAppProfile(
      {
        target: "C:\\Program Files\\Netease\\CloudMusic\\cloudmusic.exe",
        preset: true,
        presetId: "netease_music",
      },
      {
        "C:\\Program Files\\Netease\\CloudMusic\\cloudmusic.exe": {
          strategy: "recorded_point",
          point: {
            xRatio: 0.5,
            yRatio: 0.8,
            windowWidth: 1200,
            windowHeight: 800,
          },
        },
      },
    );
    expect(profile?.strategy).toBe("recorded_point");
    expect(compactFocusLabel(profile)).toBe("点击位置");
  });

  it("reads a preset profile stored under the preset id when the row uses a real path", () => {
    const profile = effectiveAppProfile(
      {
        target: "C:\\Apps\\Weixin.exe",
        preset: true,
        presetId: "wechat",
      },
      {
        wechat: {
          strategy: "recorded_point",
          point: {
            xRatio: 0.5,
            yRatio: 0.8,
            windowWidth: 1200,
            windowHeight: 800,
          },
        },
      },
    );
    expect(profile?.strategy).toBe("recorded_point");
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
        scrollRegions: {
          "C:\\Apps\\Cursor.exe": { controlType: "Document", automationId: "RootWebArea" },
          "C:\\WeChat\\Weixin.exe": { controlType: "Window" },
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
    expect(result.scrollRegions).toEqual({
      "C:\\WeChat\\Weixin.exe": { controlType: "Window" },
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

  it("reads scroll regions with case-insensitive keys", () => {
    expect(
      scrollRegionForTarget(
        { "C:\\Apps\\Cursor.exe": { controlType: "Document" } },
        "c:\\apps\\cursor.exe",
      ),
    ).toEqual({ controlType: "Document" });
    expect(scrollRegionForTarget(undefined, "chrome")).toBeNull();
  });
});

describe("selection", () => {
  const rows = buildAppRows(
    [{ name: "Cursor", path: "C:\\Apps\\Cursor.exe" }],
    { chrome: { strategy: "open_only" } },
    PRESETS,
  );

  it("keeps an existing selection", () => {
    expect(nextSelectedTarget(rows, "C:\\Apps\\Cursor.exe")).toBe(
      "C:\\Apps\\Cursor.exe",
    );
  });

  it("falls back to the first row when the selection disappeared", () => {
    expect(nextSelectedTarget(rows, "missing")).toBe("C:\\Apps\\Cursor.exe");
    expect(nextSelectedTarget([], "missing")).toBeNull();
  });
});
