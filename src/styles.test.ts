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

  it("窗口壳用固定视口高度，内容区不撑开根元素", () => {
    // 标题栏占顶部一行，余下高度由 .window-body 分配；用 [{;] 前缀避免把
    // min-height: 100vh 误判成 height: 100vh。
    expect(css).toMatch(/\.window-frame\s*\{[^}]*\n\s*height:\s*100vh/);
    expect(css).not.toMatch(/\.app-shell\s*\{[^}]*[{;]\s*height:\s*100vh/);
  });

  it("主内容为浮在侧栏底色上的圆角面板，底部/右侧留 8px", () => {
    expect(css).toMatch(/\.app-shell\s*\{[^}]*background:\s*var\(--sidebar\)/);
    expect(css).toMatch(
      /\.content\s*\{[^}]*height:\s*calc\(100%\s*-\s*8px\)[^}]*margin:\s*0 8px 8px 0/,
    );
    expect(css).toMatch(/\.content\s*\{[^}]*border-radius:\s*10px/);
    expect(css).toMatch(/\.content\s*\{[^}]*background:\s*var\(--surface-canvas\)/);
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
// 自然宽度为 130，且不再画右边框。CSS 与推荐页子 WebView 的偏移必须同源，
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

// 2026-11-06 Andy：标题栏颜色与侧栏始终一致，去掉标题栏 logo/文字；侧栏高度、
// logo、字号和间距按 Mac SettingsSidebarRow / PR 803 的常量锁住。
describe("自定义标题栏与 Mac 侧栏尺寸", () => {
  it("标题栏与侧栏共用 --sidebar，且高度变量为 32px", () => {
    const titleBar = readFileSync(
      join(srcDir, "components", "WindowTitleBar.vue"),
      "utf8",
    );
    expect(css).toMatch(/:root\s*\{[^}]*--titlebar-height:\s*32px/);
    expect(titleBar).toMatch(/\.window-titlebar\s*\{[^}]*height:\s*var\(--titlebar-height\)/);
    expect(titleBar).toMatch(/\.window-titlebar\s*\{[^}]*background:\s*var\(--sidebar\)/);
    expect(css).toMatch(/\.sidebar\s*\{[^}]*background:\s*var\(--sidebar\)/);
  });

  it("侧栏行与 Mac 一致：40 高、44 行距、18 图标、13 semibold、8 间距", () => {
    expect(css).toMatch(/\.nav-item\s*\{[^}]*min-height:\s*40px/);
    expect(css).toMatch(/\.nav-item\s*\{[^}]*margin:\s*2px 6px/);
    expect(css).toMatch(/\.nav-item\s*\{[^}]*gap:\s*8px/);
    expect(css).toMatch(/\.nav-item\s*\{[^}]*padding:\s*4px 5px 4px 9px/);
    expect(css).toMatch(/\.nav-item\s*\{[^}]*font-size:\s*13px/);
    expect(css).toMatch(/\.nav-icon\s*\{[^}]*width:\s*18px/);
    const sidebar = readFileSync(join(srcDir, "components", "Sidebar.vue"), "utf8");
    expect(sidebar).toMatch(/\.nav-icon-keyboard\s*\{[^}]*transform:\s*scale\(0\.9\)/);
    // 2026-11-06 Andy：主页面与侧栏之间不要竖线。
    expect(css).not.toMatch(/\.sidebar\s*\{[^}]*border-right/);
  });

  it("主标题和副标题与内容左缘对齐，不再额外右缩 14px", () => {
    expect(css).toMatch(/\.page-header\s*\{[^}]*margin-bottom:\s*12px;\s*\}/);
    expect(css).not.toMatch(/\.page-header\s*\{[^}]*padding-left/);
  });

  it("品牌区与 Mac 一致：75 高、44 logo、16 字标、12 版本、15 边距", () => {
    const sidebar = readFileSync(join(srcDir, "components", "Sidebar.vue"), "utf8");
    expect(sidebar).toMatch(/\.sidebar-brand\s*\{[^}]*height:\s*75px/);
    expect(sidebar).toMatch(/\.sidebar-brand\s*\{[^}]*padding:\s*11px 15px 10px/);
    expect(sidebar).toMatch(/\.brand-logo\s*\{[^}]*width:\s*44px/);
    expect(sidebar).toMatch(/\.brand-name\s*\{[^}]*font-size:\s*16px/);
    expect(sidebar).toMatch(/\.brand-version\s*\{[^}]*font-size:\s*12px/);
    expect(sidebar).toMatch(/\.brand-version\s*\{[^}]*font-variant-numeric:\s*tabular-nums/);
  });

  it("账号入口使用实心头肩的 person 图标，账号区至少 56 高并允许文字换行", () => {
    const sidebar = readFileSync(join(srcDir, "components", "Sidebar.vue"), "utf8");
    expect(sidebar).toMatch(/ICON_PATHS\.person\.strokes/);
    expect(sidebar).toMatch(/ICON_PATHS\.person\.fills/);
    expect(sidebar).toMatch(/\.sidebar-account\s*\{[^}]*padding:\s*6px 0/);
    expect(sidebar).not.toMatch(/\.sidebar-account\s*\{[^}]*border-top/);
    expect(sidebar).toMatch(
      /\.profile-nav-item \.nav-label\s*\{[^}]*white-space:\s*normal/,
    );
  });

  it("推荐页子 WebView 让出标题栏高度", () => {
    const rust = readFileSync(
      join(process.cwd(), "src-tauri", "src", "recommendations.rs"),
      "utf8",
    );
    expect(rust).toMatch(/const TITLEBAR_HEIGHT:\s*f64\s*=\s*32\.0;/);
    expect(rust).toMatch(/const CONTENT_GAP:\s*f64\s*=\s*8\.0;/);
    expect(rust).toMatch(/let content_top = TITLEBAR_HEIGHT \+ top;/);
    expect(rust).toMatch(/LogicalPosition::new\(SIDEBAR_WIDTH, content_top\)/);
    expect(rust).toMatch(/size\.width - SIDEBAR_WIDTH - CONTENT_GAP/);
    expect(rust).toMatch(/size\.height - content_top - CONTENT_GAP/);
  });
});

describe("回眸与组合动作卡片跟随设置页材质", () => {
  it("回眸记录卡和 Agent 面板深浅色都使用 --card", () => {
    const reflections = readFileSync(
      join(srcDir, "pages", "ReflectionsPage.vue"),
      "utf8",
    );
    const agent = readFileSync(
      join(srcDir, "pages", "ReflectionsAgentAccess.vue"),
      "utf8",
    );
    expect(reflections).toMatch(/\.card\s*\{[^}]*background:\s*var\(--card\)/);
    expect(reflections).not.toMatch(/rgba\(246, 247, 251, 0\.98\)/);
    expect(agent).toMatch(/\.agent-access\s*\{[^}]*background:\s*var\(--card\)/);
    expect(agent).not.toMatch(/rgba\(58, 61, 78/);
    expect(agent).toMatch(
      /\.agent-client\s*\{[^}]*background:\s*var\(--surface-subtle\)/,
    );
  });

  it("组合动作左栏拉到底，页面高度跟随内容区且不显示底部状态条", () => {
    const macros = readFileSync(join(srcDir, "pages", "MacrosPage.vue"), "utf8");
    expect(macros).toMatch(/\.library-body\s*\{[^}]*align-items:\s*stretch/);
    expect(macros).toMatch(/\.library-pane\s*\{[^}]*align-self:\s*stretch/);
    expect(macros).toMatch(/\.library-pane\s*\{[^}]*background:\s*var\(--card\)/);
    expect(macros).toMatch(/\.library-pane\s*\{[^}]*border-radius:\s*12px/);
    expect(macros).not.toMatch(/\.library-pane\s*\{[^}]*border-right/);
    expect(macros).toMatch(/\.macros-page\s*\{[^}]*height:\s*100%/);
    expect(macros).not.toMatch(/\.macros-page\s*\{[^}]*height:\s*calc\(100vh/);
    expect(macros).not.toMatch(/class="status-line"/);
    // 编辑页：标题/步骤条与分隔线左对齐，左侧步骤卡与上下横线留 14px 并带圆角。
    expect(macros).toMatch(/\.wizard-head\s*\{[^}]*padding:\s*14px 0 12px/);
    expect(macros).toMatch(/\.basic-body\s*\{[^}]*padding:\s*20px 0/);
    expect(macros).toMatch(/\.steps-pane\s*\{[^}]*margin:\s*14px 0/);
    expect(macros).toMatch(/\.steps-pane\s*\{[^}]*border-radius:\s*12px/);
    expect(macros).not.toMatch(/\.steps-pane\s*\{[^}]*border-right/);
  });
});
