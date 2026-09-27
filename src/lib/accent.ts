import {
  getSystemAccentColor,
  subscribeAccentChanges,
  type AccentRgb,
} from "./bridge";

export type { AccentRgb } from "./bridge";

/**
 * 界面强调色跟随 Windows 系统主题色（设置 > 个性化 > 颜色）。
 *
 * 数据链：Rust `get_system_accent_color`（WinRT UISettings）→ 本模块缓存原始
 * RGB → 按当前深浅色主题派生整套 `--accent*` 变量注入 `documentElement`。
 * styles.css 里的紫色只是**读取失败时的回退默认值**；注入成功后内联变量
 * 优先级更高，全部选中态自动换色。
 *
 * 实时性：Rust 侧监听系统 WM_SETTINGCHANGE("ImmersiveColorSet") 广播，
 * 强调色变化时经 "system-accent-changed" 事件推送，本模块收到后立即重算
 * 重注入——用户改系统色不需要重启应用。
 *
 * 派生规则（WCAG 2.x 对比度，参照 2026-09-08 深色模式校准基线）：
 * - `--accent`（主按钮/选中底色）：浅色主题保证白字 ≥4.5，深色主题保证
 *   近黑文字 ≥4.5，不满足时向黑/白方向取最小调整量；
 * - `--accent-dark`（hover）：浅色主题混黑 12%、深色主题混白 12%；
 * - `--accent-text` / `--accent-muted-text`：在画布底色上 ≥4.5；muted 取
 *   "边缘可读"版本（恰好 4.5），视觉上次于主文字；
 * - `--dot-active` 等图形色：与画布对比 ≥3 即可，不做文字级 clamp；
 * - 透明度变体沿用原紫色版数值（浅 0.10/0.14/0.45，深 0.15/0.21/0.52）。
 */

const WHITE: AccentRgb = { r: 255, g: 255, b: 255 };
const BLACK: AccentRgb = { r: 0, g: 0, b: 0 };
/** styles.css 的 --surface-canvas：浅 #f4f5f8 / 深 #15161a。 */
const LIGHT_CANVAS: AccentRgb = { r: 244, g: 245, b: 248 };
const DARK_CANVAS: AccentRgb = { r: 21, g: 22, b: 26 };
/** styles.css 的 --text-on-accent：浅 #ffffff / 深 #171822。 */
const DARK_TEXT_ON_ACCENT: AccentRgb = { r: 23, g: 24, b: 34 };

export function contrastRatio(a: AccentRgb, b: AccentRgb): number {
  const la = relativeLuminance(a);
  const lb = relativeLuminance(b);
  const [hi, lo] = la >= lb ? [la, lb] : [lb, la];
  return (hi + 0.05) / (lo + 0.05);
}

function relativeLuminance(color: AccentRgb): number {
  const channel = (value: number): number => {
    const srgb = value / 255;
    return srgb <= 0.03928 ? srgb / 12.92 : ((srgb + 0.055) / 1.055) ** 2.4;
  };
  return (
    0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
  );
}

function mix(a: AccentRgb, b: AccentRgb, t: number): AccentRgb {
  return {
    r: Math.round(a.r + (b.r - a.r) * t),
    g: Math.round(a.g + (b.g - a.g) * t),
    b: Math.round(a.b + (b.b - a.b) * t),
  };
}

/** 朝白色（更亮）方向取满足对比度的最小调整量。 */
function lightenUntilReadable(
  color: AccentRgb,
  background: AccentRgb,
  target: number,
): AccentRgb {
  if (contrastRatio(color, background) >= target) return color;
  for (let t = 0.02; t <= 1; t += 0.02) {
    const candidate = mix(color, WHITE, t);
    if (contrastRatio(candidate, background) >= target) return candidate;
  }
  return WHITE;
}

/** 朝黑色（更暗）方向取满足对比度的最小调整量。 */
function darkenUntilReadable(
  color: AccentRgb,
  background: AccentRgb,
  target: number,
): AccentRgb {
  if (contrastRatio(color, background) >= target) return color;
  for (let t = 0.02; t <= 1; t += 0.02) {
    const candidate = mix(color, BLACK, t);
    if (contrastRatio(candidate, background) >= target) return candidate;
  }
  return BLACK;
}

/** 在保持 ≥target 的前提下推到对比度边缘——"能读的最浅/最暗"版本。 */
function pushToContrastEdge(
  color: AccentRgb,
  background: AccentRgb,
  target: number,
  direction: "lighter" | "darker",
): AccentRgb {
  const end = direction === "lighter" ? WHITE : BLACK;
  let last = color;
  for (let t = 0.02; t <= 1; t += 0.02) {
    const candidate = mix(color, end, t);
    if (contrastRatio(candidate, background) < target) break;
    last = candidate;
  }
  return last;
}

export function deriveAccentPalette(
  accent: AccentRgb,
  theme: "light" | "dark",
): Record<string, string> {
  const canvas = theme === "light" ? LIGHT_CANVAS : DARK_CANVAS;
  const textOnAccent = theme === "light" ? WHITE : DARK_TEXT_ON_ACCENT;

  // 主色：按钮/选中底色，先保证按钮文字可读。
  let accentMain =
    theme === "light"
      ? darkenUntilReadable(accent, textOnAccent, 4.5)
      : lightenUntilReadable(accent, textOnAccent, 4.5);
  // 再保证深/浅底上可见（焦点环、选中连线、强调底色）。
  if (contrastRatio(accentMain, canvas) < 3) {
    accentMain =
      theme === "light"
        ? darkenUntilReadable(accentMain, canvas, 3)
        : lightenUntilReadable(accentMain, canvas, 3);
  }

  const accentDark =
    theme === "light" ? mix(accentMain, BLACK, 0.12) : mix(accentMain, WHITE, 0.12);

  // 文字色：画布底色上 ≥4.5（用户选极浅/极深强调色时需调整）。
  const accentText =
    theme === "light"
      ? darkenUntilReadable(accent, WHITE, 4.5)
      : lightenUntilReadable(accent, canvas, 4.5);
  const accentMutedText =
    contrastRatio(accentText, canvas) >= 4.5
      ? pushToContrastEdge(accentText, canvas, 4.5, theme === "light" ? "lighter" : "darker")
      : accentText;

  // 图形色（状态点、图表条、空态图标）：与画布可区分即可。
  const graphic =
    theme === "light"
      ? darkenUntilReadable(accent, LIGHT_CANVAS, 3)
      : lightenUntilReadable(accent, DARK_CANVAS, 3);

  const rgb = (color: AccentRgb): string => `rgb(${color.r}, ${color.g}, ${color.b})`;
  const rgba = (color: AccentRgb, alpha: number): string =>
    `rgba(${color.r}, ${color.g}, ${color.b}, ${alpha})`;
  // 透明度沿用原紫色版数值，视觉层次不变。
  const alphas =
    theme === "light"
      ? { surface: 0.1, strong: 0.14, border: 0.45, hover: 0.1, glow: 0.12 }
      : { surface: 0.15, strong: 0.21, border: 0.52, hover: 0.18, glow: 0.09 };

  return {
    "--accent": rgb(accentMain),
    "--accent-dark": rgb(accentDark),
    "--accent-text": rgb(accentText),
    "--accent-muted-text": rgb(accentMutedText),
    "--accent-surface": rgba(accentMain, alphas.surface),
    "--accent-surface-strong": rgba(accentMain, alphas.strong),
    "--accent-border": rgba(accentMain, alphas.border),
    "--accent-hover": rgba(accentMain, alphas.hover),
    "--canvas-glow": rgba(graphic, alphas.glow),
    "--dot-active": rgb(graphic),
    "--data-button": rgb(graphic),
    "--empty-icon": rgb(graphic),
  };
}

let rawAccent: AccentRgb | null = null;
let removeChangeListener: (() => void) | null = null;

/** 当前文档主题（styles.css 的 data-theme；不反向依赖 theme 模块避免循环）。 */
function currentDocumentTheme(): "light" | "dark" {
  return document.documentElement.dataset.theme === "dark" ? "dark" : "light";
}

export function applyAccentPalette(theme: "light" | "dark"): void {
  if (!rawAccent) return;
  const palette = deriveAccentPalette(rawAccent, theme);
  for (const [name, value] of Object.entries(palette)) {
    document.documentElement.style.setProperty(name, value);
  }
}

export async function initializeAccentColor(): Promise<void> {
  try {
    rawAccent = await getSystemAccentColor();
  } catch {
    rawAccent = null;
  }
  if (!rawAccent) {
    console.warn(
      "feature=accent event=initialized result=fallback reason=system_accent_unavailable",
    );
    return;
  }
  applyAccentPalette(currentDocumentTheme());
  console.info(
    `feature=accent event=initialized result=passed r=${rawAccent.r} g=${rawAccent.g} b=${rawAccent.b}`,
  );

  try {
    removeChangeListener = await subscribeAccentChanges((color) => {
      rawAccent = color;
      applyAccentPalette(currentDocumentTheme());
      console.info(
        `feature=accent event=changed result=passed r=${color.r} g=${color.g} b=${color.b}`,
      );
    });
  } catch {
    console.warn("feature=accent event=fallback reason=change_listener_failed");
  }
}

export function disposeAccentColor(): void {
  removeChangeListener?.();
  removeChangeListener = null;
  rawAccent = null;
}
