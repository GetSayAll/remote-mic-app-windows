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

// 2026-11-05 Andy：触摸板双指上下拖动会把整页拖出视口，顶部/底部露出空白。
// 修法是把滚动收敛到内容区：根元素不可滚，内容区内部滚，并关掉 overscroll。
// 这里把三件事钉住——任何一件被改回去，双指拖动都会重新拖出空白：
// ① html/body 满高 + overflow:hidden（根滚动器没有可滚距离）；
// ② html/body 上 overscroll-behavior:none（内容不足一屏时也拖不动）；
// ③ .content 是唯一滚动容器，且用 contain 拦住滚动溢出往根传。
describe("根元素不参与滚动（触摸板拖出空白）", () => {
  it("html/body 固定满高、禁止溢出并关闭 overscroll", () => {
    expect(css).toMatch(/html,\s*body\s*\{[^}]*[;\s{]height:\s*100%/);
    expect(css).toMatch(/html,\s*body\s*\{[^}]*overflow:\s*hidden/);
    expect(css).toMatch(/html,\s*body\s*\{[^}]*overscroll-behavior:\s*none/);
  });

  it("内容区是唯一滚动容器，滚动溢出不传给根", () => {
    expect(css).toMatch(/\.content\s*\{[^}]*overflow:\s*auto/);
    expect(css).toMatch(/\.content\s*\{[^}]*overscroll-behavior:\s*contain/);
  });

  it("外壳用固定视口高度，不随内容增高撑开根元素", () => {
    // 用 [{;] 前缀避免把 min-height: 100vh 误判成 height: 100vh。
    expect(css).toMatch(/\.app-shell\s*\{[^}]*[{;]\s*height:\s*100vh/);
    expect(css).not.toMatch(/\.app-shell\s*\{[^}]*min-height:\s*100vh/);
  });
});
