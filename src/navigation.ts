import { COMBINATION_ACTIONS_ENABLED } from "./lib/feature-flags";
import { privateFeaturePages, type PrivateFeaturePage } from "./private/feature-pages";

export type PageId =
  | "buttons"
  | "recommendations"
  | "macros"
  | "connection"
  | "permissions"
  | "settings";

export type NavIcon =
  | "keyboard"
  | "sparkles"
  | "layers"
  | "link"
  | "shield"
  | "gear"
  | "member";

export interface NavigationItem {
  id: string;
  label: string;
  /** 侧栏图标（形状对齐 macOS SF Symbols：keyboard/link/shield.lefthalf.filled/gearshape）。 */
  icon: NavIcon;
}

/** 公开页面表。公开构建的侧栏就是它与私有页面注册表（通常为空）的合并结果。 */
export const publicNavigationItems: NavigationItem[] = [
  { id: "buttons", label: "按键", icon: "keyboard" },
  // 2026-10-15 组合动作（对齐 Mac 侧边栏「组合动作」条目与层叠方块图标）；
  // 入口显隐跟随 COMBINATION_ACTIONS_ENABLED，隐藏时已绑定动作照常执行。
  ...(COMBINATION_ACTIONS_ENABLED
    ? [{ id: "macros", label: "组合动作", icon: "layers" } as NavigationItem]
    : []),
  { id: "connection", label: "连接", icon: "link" },
  // 2026-10-20 用户指定顺序：按键、组合动作、连接、推荐、权限、设置。
  { id: "recommendations", label: "推荐", icon: "sparkles" },
  { id: "permissions", label: "权限", icon: "shield" },
  // 2026-10-02 用户指定：原「关于」页改名「设置」，图标对齐 Mac main
  // SettingsSection.about 的 "gearshape"。
  { id: "settings", label: "设置", icon: "gear" },
];

/**
 * 合并公开页面与私有注册的页面（2026-10-15）。
 *
 * 抽成纯函数是为了能直接测「接入点真的接通了」——不必给整个应用打桩。
 * 公开构建传入空数组，结果必须与 `publicNavigationItems` 逐项相同
 * （仓库规则：未实现的页面不显示空入口）。
 */
export function mergeNavigationItems(
  base: NavigationItem[],
  extra: readonly PrivateFeaturePage[],
): NavigationItem[] {
  return [
    ...base,
    ...extra.map((page) => ({ id: page.id, label: page.label, icon: page.icon })),
  ];
}

export const navigationItems: NavigationItem[] = mergeNavigationItems(
  publicNavigationItems,
  privateFeaturePages,
);

const ACTIVE_PAGE_STORAGE_KEY = "sayall.activePage";
const LIVENESS_STORAGE_KEY = "sayall.livenessHeartbeat";
/** 心跳由 1 秒运行快照轮询续写；5 秒内仍新鲜即视为重载而非冷启动，
 * 余量覆盖 WebView 后台节流与轮询单次失败。 */
const LIVENESS_FRESH_MS = 5_000;

/** WebView 渲染进程崩溃重载会把整个应用重置回默认页（Bugs/2026-09-12），
 * 当前页持久化让恢复后停在用户离开时的页面。存储不可用时静默放弃：
 * 回到默认页不比重载前更差。 */
export function loadPersistedPage(): string | null {
  try {
    const stored = localStorage.getItem(ACTIVE_PAGE_STORAGE_KEY);
    return navigationItems.some((item) => item.id === stored)
      ? stored
      : null;
  } catch {
    return null;
  }
}

export function persistActivePage(page: string): void {
  try {
    localStorage.setItem(ACTIVE_PAGE_STORAGE_KEY, page);
  } catch {
    // 同 loadPersistedPage：存储不可用不属于导航职责，不向调用方扩散。
  }
}

/** 写入活性心跳（每次运行快照轮询调用）。 */
export function touchLiveness(now = Date.now()): void {
  try {
    localStorage.setItem(LIVENESS_STORAGE_KEY, String(now));
  } catch {
    // 心跳丢失只会让下一次重载被误判为冷启动，不产生其他影响。
  }
}

/** 崩溃前的心跳仍新鲜则为重载恢复；调用时顺带写入当前心跳。 */
export function detectReloadRecovery(now = Date.now()): boolean {
  try {
    const previous = Number(localStorage.getItem(LIVENESS_STORAGE_KEY));
    localStorage.setItem(LIVENESS_STORAGE_KEY, String(now));
    return Number.isFinite(previous) && previous > 0 && now - previous < LIVENESS_FRESH_MS;
  } catch {
    return false;
  }
}

/**
 * Ctrl+W：关闭主窗口快捷键（2026-09-26）。命中即隐藏到托盘，与点标题栏“X”同义。
 *
 * 带 Alt/Shift/Meta 一律不算：Ctrl+Alt+W、Ctrl+Shift+W（浏览器里是“关闭所有
 * 标签页/窗口”）与 Cmd+W 都不该让本窗口消失。只收 Ctrl 是为了避免与 Windows
 * 其它组合键抢语义。key 用小写归一，覆盖 Ctrl 与字母键大小写/输入法切层差异。
 */
export function isWindowCloseAccelerator(event: {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}): boolean {
  if (event.altKey || event.shiftKey || event.metaKey) return false;
  if (!event.ctrlKey) return false;
  return event.key.toLowerCase() === "w";
}

/** WebView2 默认把 F5/Ctrl+R 当浏览器刷新键（渲染进程重载在用户侧表现为
 * 白屏后回到初始页）。返回 true 表示命中刷新键，调用方应 preventDefault
 * 并阻止传播；host 侧预处理的加速键可能先于本拦截，故这只是页面层兜底。 */
export function isBrowserReloadAccelerator(event: {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
}): boolean {
  if (event.key === "F5") return true;
  if ((event.ctrlKey || event.metaKey) && (event.key === "r" || event.key === "R")) {
    return true;
  }
  return false;
}
