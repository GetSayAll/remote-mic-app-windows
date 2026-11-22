import { describe, expect, it } from "vitest";
import { SHORTCUT_PRESET_GROUPS, shortcutPresetHint } from "./shortcut-presets";

/**
 * 2026-11-22 Andy：按键页与组合动作页共用的「系统与媒体」清单中，
 * Alt + F4 必须紧跟在 Win + D 右侧，用于关闭窗口。
 */
describe("系统与媒体预设", () => {
  const systemGroup = SHORTCUT_PRESET_GROUPS.find((group) =>
    group.items.some((item) => item.display === "Win + D"),
  );
  const displays = systemGroup?.items.map(
    (item) => item.display ?? item.keys.join(" + "),
  );

  it("Alt + F4 紧跟在 Win + D 右侧", () => {
    expect(systemGroup).toBeDefined();
    const index = displays!.indexOf("Win + D");
    expect(index).toBeGreaterThanOrEqual(0);
    expect(displays![index + 1]).toBe("Alt + F4");
  });

  it("Alt + F4 注入 Alt+F4 并提示关闭当前窗口", () => {
    const item = systemGroup!.items.find(
      (entry) => (entry.display ?? entry.keys.join(" + ")) === "Alt + F4",
    );
    expect(item).toBeDefined();
    expect(item!.keys).toEqual(["alt", "f4"]);
    expect(shortcutPresetHint(item!)).toBe("关闭当前窗口");
  });
});
