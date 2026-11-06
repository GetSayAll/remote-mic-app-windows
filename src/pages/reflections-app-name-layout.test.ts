import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

// 2026-11-06 用户截图：回眸应用栏与记录行把「腾讯电脑管家截图工具」截成
// 「腾讯电脑管家...」。应用名是用户识别记录来源的唯一线索，不能省略；
// 这里把两个位置都钉成「可完整显示」，防止以后又加回 ellipsis / nowrap。
const source = readFileSync(
  join(process.cwd(), "src/pages/ReflectionsPage.vue"),
  "utf8",
);
const style = (source.match(/<style[^>]*>([\s\S]*?)<\/style>/) ?? [])[1] ?? "";

function ruleBody(selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = style.match(
    new RegExp(`(?:^|\\n)\\s*${escaped}\\s*\\{([^}]*)\\}`),
  );
  return match?.[1] ?? "";
}

describe("回眸应用名不被截断", () => {
  it("应用栏 chip 名称允许换行，不再用省略号", () => {
    const label = ruleBody(".app-chip-label strong");
    expect(label).not.toBe("");
    expect(label).not.toMatch(/text-overflow:\s*ellipsis/);
    expect(label).not.toMatch(/white-space:\s*nowrap/);
    expect(label).toMatch(/white-space:\s*normal/);
    expect(label).toMatch(/overflow-wrap:\s*anywhere/);
  });

  it("chip 高度可随名称换行增长，而不是固定 48px 裁掉第二行", () => {
    const chip = ruleBody(".app-chip");
    expect(chip).not.toBe("");
    expect(chip).not.toMatch(/(?:^|;|\s)height:\s*48px/);
    expect(chip).toMatch(/min-height:\s*48px/);
  });

  it("记录行应用名允许换行，不再用省略号", () => {
    const name = ruleBody(".row-app-name");
    expect(name).not.toBe("");
    expect(name).not.toMatch(/text-overflow:\s*ellipsis/);
    expect(name).not.toMatch(/white-space:\s*nowrap/);
    expect(name).toMatch(/white-space:\s*normal/);
    expect(name).toMatch(/overflow-wrap:\s*anywhere/);
  });
});
