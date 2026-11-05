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

// 2026-11-06 Andy：侧栏左上角 SayAll 与设置页左上角「无线麦 SayAll」要和 Mac
// 用同一套品牌字（Oxanium SemiBold 16 / 18）。字体文件缺失或选择器回退时这里会红。
describe("品牌字体（Oxanium SemiBold）", () => {
  it("@font-face 指向随包字体，并暴露品牌字体变量", () => {
    expect(css).toMatch(/@font-face\s*\{[^}]*font-family:\s*"Oxanium"/);
    expect(css).toMatch(/@font-face\s*\{[^}]*url\("\/fonts\/oxanium-semibold\.ttf"\)/);
    expect(css).toMatch(/--font-brand:\s*"Oxanium"/);
  });

  it("侧栏 SayAll 与设置页应用名使用品牌字体", () => {
    const sidebar = readFileSync(join(srcDir, "components", "Sidebar.vue"), "utf8");
    const settings = readFileSync(join(srcDir, "pages", "SettingsPage.vue"), "utf8");
    expect(sidebar).toMatch(/\.brand-name\s*\{[^}]*font-family:\s*var\(--font-brand\)/);
    expect(settings).toMatch(
      /\.overview-identity h2\s*\{[^}]*font-family:\s*var\(--font-brand\)[^}]*font-size:\s*18px/,
    );
  });

  it("字体文件与 OFL 许可随包存在", () => {
    const font = readFileSync(join(process.cwd(), "public", "fonts", "oxanium-semibold.ttf"));
    // TrueType 文件头；只校验类型和最低体量，避免把损坏/占位文件当字体。
    expect([...font.subarray(0, 4)]).toEqual([0, 1, 0, 0]);
    expect(font.length).toBeGreaterThan(20_000);
    const license = readFileSync(join(process.cwd(), "public", "fonts", "OFL.txt"), "utf8");
    expect(license).toContain("SIL OPEN FONT LICENSE Version 1.1");
  });
});

// 2026-11-06：Mac origin/main 的侧栏宽度按内容测量（104...136），当前品牌区
// 自然宽度为 129 内容 + 1px 边框 = 130。CSS 与推荐页子 WebView 的偏移必须同源，
// 否则推荐页会盖住侧栏或留下空隙。
describe("侧栏宽度与推荐页偏移同步", () => {
  it("CSS 侧栏列宽为 130px", () => {
    expect(css).toMatch(/\.app-shell\s*\{[^}]*grid-template-columns:\s*130px/);
  });

  it("推荐页子 WebView 使用同一个 130px 偏移", () => {
    const rust = readFileSync(
      join(process.cwd(), "src-tauri", "src", "recommendations.rs"),
      "utf8",
    );
    expect(rust).toMatch(/const SIDEBAR_WIDTH:\s*f64\s*=\s*130\.0;/);
  });
});
