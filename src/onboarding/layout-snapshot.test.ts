import { describe, expect, it } from "vitest";
import {
  formatLayoutCopyLine,
  layoutLogDetail,
  type OnboardingLayoutSnapshot,
} from "./layout-snapshot";

const visibleSnapshot: OnboardingLayoutSnapshot = {
  mode: "single_column",
  viewportWidth: 800,
  viewportHeight: 600,
  devicePixelRatio: 1,
  visualScale: 1,
  documentWidth: 800,
  documentHeight: 807,
  hOverflow: false,
  actionBar: { x: 0, y: 515, width: 800, height: 85 },
  primary: { x: 703, y: 559, width: 53, height: 27 },
  primaryVisible: true,
  primaryHittable: true,
  primaryEnabled: true,
};

describe("onboarding layout snapshot", () => {
  it("encodes the layout as a bounded, safe log token", () => {
    const detail = layoutLogDetail(visibleSnapshot, "step");

    expect(detail.length).toBeLessThanOrEqual(64);
    expect(detail).toContain("vp800x600");
    expect(detail).toContain("mode_single");
    expect(detail).toContain("ovf0");
    expect(detail).toContain("btn1");
    expect(detail).toContain("hit1");
    expect(detail).toContain("en1");
    expect(detail).toMatch(/^[A-Za-z0-9_.-]+$/);
  });

  it("formats the copyable diagnostics line with the button geometry", () => {
    expect(formatLayoutCopyLine(visibleSnapshot)).toBe(
      "Layout: mode=single_column viewport=800x600 dpr=1.00 scale=1.00 doc=800x807 " +
        "h_overflow=no action_bar=0,515,800,85 primary=703,559,53,27 " +
        "visible=yes hittable=yes enabled=yes",
    );
  });

  it("marks a horizontally cropped button as not visible and not hittable", () => {
    const cropped: OnboardingLayoutSnapshot = {
      ...visibleSnapshot,
      documentWidth: 940,
      hOverflow: true,
      primary: { x: 843, y: 559, width: 53, height: 27 },
      primaryVisible: false,
      primaryHittable: false,
    };

    expect(layoutLogDetail(cropped, "click")).toContain("ovf1_btn0_hit0");
    expect(formatLayoutCopyLine(cropped)).toContain("h_overflow=yes");
    expect(formatLayoutCopyLine(cropped)).toContain("visible=no hittable=no");
  });
});
