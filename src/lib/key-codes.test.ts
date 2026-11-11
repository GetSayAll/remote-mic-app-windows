import { describe, expect, it } from "vitest";
import { codeToKeyCode, MODIFIER_KEYS } from "./key-codes";

describe("keyboard event code mapping", () => {
  it("keeps modifier side information", () => {
    expect(codeToKeyCode("ControlLeft")).toBe("left_control");
    expect(codeToKeyCode("ControlRight")).toBe("right_control");
    expect(codeToKeyCode("MetaRight")).toBe("right_windows");
  });

  it("maps letters, digits, function keys, and named keys", () => {
    expect(codeToKeyCode("KeyQ")).toBe("q");
    expect(codeToKeyCode("Digit7")).toBe("digit7");
    expect(codeToKeyCode("F12")).toBe("f12");
    expect(codeToKeyCode("PageDown")).toBe("page_down");
    expect(codeToKeyCode("NumpadAdd")).toBe("numpad_add");
  });

  it("returns null for unknown codes", () => {
    expect(codeToKeyCode("Unidentified")).toBeNull();
    expect(codeToKeyCode("AudioVolumeUp")).toBeNull();
  });

  it("recognizes all eight modifier sides", () => {
    expect(MODIFIER_KEYS.size).toBe(8);
    expect(MODIFIER_KEYS.has("right_alt")).toBe(true);
  });
});
