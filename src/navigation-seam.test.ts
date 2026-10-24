import { describe, expect, it } from "vitest";
import type { Component } from "vue";
import { mergeNavigationItems, navigationItems, publicNavigationItems } from "./navigation";
import type { PrivateFeaturePage } from "./private/feature-pages";

/**
 * 私有组件接入点的契约测试（2026-10-15）。
 *
 * 公开构建的行为必须**逐项不变**：私有注册表为空时，侧栏与改动前完全一致，
 * 不出现任何空入口。注入私有页面时只做追加，不改动公开页面。
 */
describe("私有功能页接入点", () => {
  const injected: PrivateFeaturePage[] = [
    {
      id: "membership",
      label: "会员",
      icon: "gear",
      component: { render: () => null } as unknown as Component,
    },
  ];

  it("公开构建不增加任何页面", () => {
    expect(navigationItems).toEqual(publicNavigationItems);
    expect(navigationItems.map((item) => item.id)).toEqual([
      "buttons",
      "macros",
      "connection",
      "recommendations",
      "permissions",
      "settings",
      "profile",
    ]);
  });

  it("空注册表是恒等变换", () => {
    expect(mergeNavigationItems(publicNavigationItems, [])).toEqual(publicNavigationItems);
  });

  it("私有页面追加在公开页面之后且不改动公开表", () => {
    const merged = mergeNavigationItems(publicNavigationItems, injected);

    expect(merged.map((item) => item.id)).toEqual([
      "buttons",
      "macros",
      "connection",
      "recommendations",
      "permissions",
      "settings",
      "profile",
      "membership",
    ]);
    expect(merged.at(-1)?.label).toBe("会员");
    // 合并函数必须是纯的：公开表不能被就地改写。
    expect(publicNavigationItems).toHaveLength(7);
    expect(publicNavigationItems.map((item) => item.id)).not.toContain("membership");
  });

  it("校验码：私有页面 id 不得与公开页面冲突", () => {
    // 这条是给私有构建的护栏：重名会让 activePage 指向错的组件。
    const publicIds = new Set(publicNavigationItems.map((item) => item.id));
    for (const page of injected) {
      expect(publicIds.has(page.id)).toBe(false);
    }
  });
});
