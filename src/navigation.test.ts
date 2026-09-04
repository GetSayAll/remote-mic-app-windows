import { describe, expect, it } from "vitest";
import { navigationItems } from "./navigation";

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
