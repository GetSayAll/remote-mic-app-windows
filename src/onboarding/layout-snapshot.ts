/**
 * 向导布局快照（2026-11-24）：把「主按钮是否真的在视口里、可点」变成可落盘的
 * 事实，用于区分 ①按钮没渲染、②按钮在视口外/横向被裁、③被内容遮挡、④禁用、
 * ⑤用户没点。尺寸/布尔属于 LOGGING.md 允许的脱敏字段；不含路径、窗口标题或
 * 用户输入。
 *
 * 日志侧只写紧凑 token（Rust `diagnostic_token` 只放行 `[A-Za-z0-9_.-]` 且
 * ≤64 字符）；复制诊断侧写可读的 `Layout:` 行，允许更长。
 */
import { reasonToken } from "../lib/frontend-diagnostics";

export type OnboardingLayoutMode = "single_column" | "two_column";

export interface OnboardingRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface OnboardingLayoutSnapshot {
  mode: OnboardingLayoutMode;
  viewportWidth: number;
  viewportHeight: number;
  devicePixelRatio: number;
  /** window.visualViewport.scale（捏合缩放）；读不到时为 1。 */
  visualScale: number;
  /** documentElement 的 scrollWidth / scrollHeight。 */
  documentWidth: number;
  documentHeight: number;
  /** 文档比可视宽度更宽（横向裁切）。 */
  hOverflow: boolean;
  actionBar: OnboardingRect | null;
  primary: OnboardingRect | null;
  /** 主按钮完整落在视口内且有面积。 */
  primaryVisible: boolean;
  /** 主按钮中心点的 elementFromPoint 命中按钮本身。 */
  primaryHittable: boolean;
  primaryEnabled: boolean;
}

function rectOf(element: Element | null): OnboardingRect | null {
  if (!element) return null;
  const rect = element.getBoundingClientRect();
  return {
    x: Math.round(rect.left),
    y: Math.round(rect.top),
    width: Math.round(rect.width),
    height: Math.round(rect.height),
  };
}

export function measureOnboardingLayout(
  win: Window = window,
  doc: Document = document,
): OnboardingLayoutSnapshot {
  const viewportWidth = Math.round(win.innerWidth);
  const viewportHeight = Math.round(win.innerHeight);
  const devicePixelRatio = win.devicePixelRatio || 1;
  const visualScale = win.visualViewport?.scale ?? 1;
  const root = doc.documentElement;
  const documentWidth = root.scrollWidth;
  const documentHeight = root.scrollHeight;
  const hOverflow = root.scrollWidth > root.clientWidth;

  const actionBarElement = doc.querySelector(".onboarding-actions");
  const primaryElement = doc.querySelector(".onboarding-actions .primary-button");
  const actionBar = rectOf(actionBarElement);
  const primary = rectOf(primaryElement);

  const media = win.matchMedia?.("(max-width: 1024px), (max-height: 660px)");
  const mode: OnboardingLayoutMode = media?.matches ? "single_column" : "two_column";

  const primaryVisible =
    primary !== null &&
    primary.width > 0 &&
    primary.height > 0 &&
    primary.x >= 0 &&
    primary.y >= 0 &&
    primary.x + primary.width <= viewportWidth &&
    primary.y + primary.height <= viewportHeight;

  let primaryHittable = false;
  if (primaryVisible && primary) {
    const centerX = Math.min(Math.max(primary.x + primary.width / 2, 0), viewportWidth - 1);
    const centerY = Math.min(Math.max(primary.y + primary.height / 2, 0), viewportHeight - 1);
    const hit = doc.elementFromPoint?.(centerX, centerY) ?? null;
    primaryHittable = Boolean(
      hit && primaryElement && (hit === primaryElement || primaryElement.contains(hit)),
    );
  }

  return {
    mode,
    viewportWidth,
    viewportHeight,
    devicePixelRatio,
    visualScale,
    documentWidth,
    documentHeight,
    hOverflow,
    actionBar,
    primary,
    primaryVisible,
    primaryHittable,
    primaryEnabled: primaryElement instanceof HTMLButtonElement && !primaryElement.disabled,
  };
}

function yesNo(value: boolean): string {
  return value ? "yes" : "no";
}

function rectText(rect: OnboardingRect | null): string {
  return rect ? `${rect.x},${rect.y},${rect.width},${rect.height}` : "none";
}

/** 持久日志用的紧凑 token，触发器 + 视口/模式/溢出/可见/命中/可用。 */
export function layoutLogDetail(
  snapshot: OnboardingLayoutSnapshot,
  trigger: "step" | "resize" | "click",
): string {
  return reasonToken(
    trigger,
    `vp${snapshot.viewportWidth}x${snapshot.viewportHeight}`,
    `dpr${snapshot.devicePixelRatio.toFixed(2)}`,
    `mode_${snapshot.mode === "single_column" ? "single" : "two"}`,
    `ovf${snapshot.hOverflow ? 1 : 0}`,
    `btn${snapshot.primaryVisible ? 1 : 0}`,
    `hit${snapshot.primaryHittable ? 1 : 0}`,
    `en${snapshot.primaryEnabled ? 1 : 0}`,
  );
}

/** 复制诊断用的可读行（不受 64 字符 token 限制）。 */
export function formatLayoutCopyLine(snapshot: OnboardingLayoutSnapshot): string {
  return (
    `Layout: mode=${snapshot.mode} viewport=${snapshot.viewportWidth}x${snapshot.viewportHeight} ` +
    `dpr=${snapshot.devicePixelRatio.toFixed(2)} scale=${snapshot.visualScale.toFixed(2)} ` +
    `doc=${snapshot.documentWidth}x${snapshot.documentHeight} h_overflow=${yesNo(snapshot.hOverflow)} ` +
    `action_bar=${rectText(snapshot.actionBar)} primary=${rectText(snapshot.primary)} ` +
    `visible=${yesNo(snapshot.primaryVisible)} hittable=${yesNo(snapshot.primaryHittable)} ` +
    `enabled=${yesNo(snapshot.primaryEnabled)}`
  );
}
