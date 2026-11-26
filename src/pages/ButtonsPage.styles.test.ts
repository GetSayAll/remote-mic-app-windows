// 防止「页面结构已改、样式没跟着进 main」再次发生（2026-11-26 1.9.8 回归）：
// 按键页新增的固定头部、滚动容器和按住摘要 chip 必须有对应公开样式。
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const pageSource = readFileSync(join(process.cwd(), "src/pages/ButtonsPage.vue"), "utf8");
const styles = readFileSync(join(process.cwd(), "src/styles.css"), "utf8");

describe("按键页结构与样式同步", () => {
  it("固定头部、滚动容器和按住摘要都有公开样式", () => {
    expect(pageSource).toContain("buttons-page mapping-page");
    expect(styles).toContain(".buttons-page.mapping-page");
    for (const className of [
      "mapping-fixed-top",
      "mapping-scroll-area",
      "mapping-header-copy",
      "mapping-toggle-row",
      "mapping-hold-chip",
    ]) {
      expect(pageSource).toContain(className);
      expect(styles).toContain(`.${className}`);
    }
  });

  it("按键卡与动作格不再使用旧的固定高度/单行省略规则", () => {
    expect(styles).toContain("height: 78px");
    expect(styles).toContain("-webkit-line-clamp: 2");
  });
});
