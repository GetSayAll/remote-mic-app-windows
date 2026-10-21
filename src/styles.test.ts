import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

// 2026-10-20 Andy：所有滚动条都要隐藏，且以后新增的页面同样适用。
// 该诉求靠全局通配规则实现（styles.css 顶部），这里用测试把规则钉住：
// 删掉全局规则会红；新增样式把滚动条重新显示出来也会红。
// jsdom 环境下 import.meta.url 不是 file: 协议，用 vitest 的 root（进程 cwd）定位源码。
const srcDir = join(process.cwd(), "src");
const css = readFileSync(join(srcDir, "styles.css"), "utf8");

function collectStyleText(directory: string): { path: string; text: string }[] {
  const files: { path: string; text: string }[] = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...collectStyleText(path));
    } else if (entry.name.endsWith(".vue") || entry.name.endsWith(".css")) {
      // 先去掉注释再匹配，避免注释里提到 ::-webkit-scrollbar 被当成规则。
      const text = readFileSync(path, "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
      files.push({ path, text });
    }
  }
  return files;
}

describe("全局滚动条隐藏", () => {
  it("用通配选择器关闭滚动条，新增页面自动继承", () => {
    expect(css).toMatch(/\*\s*\{[^}]*scrollbar-width:\s*none/);
    expect(css).toMatch(/\*\s*\{[^}]*-ms-overflow-style:\s*none/);
    expect(css).toMatch(/\*::-webkit-scrollbar\s*\{[^}]*display:\s*none/);
  });

  it("没有其他样式把滚动条重新显示出来", () => {
    const offenders = collectStyleText(srcDir).flatMap(({ path, text }) => {
      const blocks = text.matchAll(/([^{}]*::-webkit-scrollbar[^{}]*)\{([^{}]*)\}/g);
      return [...blocks]
        .filter(([, , body]) => !/display:\s*none/.test(body))
        .map(([, selector]) => `${path}:${selector.trim()}`);
    });
    expect(offenders).toEqual([]);
  });
});
