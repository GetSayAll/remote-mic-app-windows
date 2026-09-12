export type PageId = "buttons" | "connection" | "permissions" | "about";

export type NavIcon = "keyboard" | "link" | "shield" | "info";

export interface NavigationItem {
  id: PageId;
  label: string;
  /** 侧栏图标（形状对齐 macOS SF Symbols：keyboard/link/shield/info.circle）。 */
  icon: NavIcon;
}

export const navigationItems: NavigationItem[] = [
  { id: "buttons", label: "按键", icon: "keyboard" },
  { id: "connection", label: "连接与语音", icon: "link" },
  { id: "permissions", label: "权限", icon: "shield" },
  { id: "about", label: "关于", icon: "info" },
];

const ACTIVE_PAGE_STORAGE_KEY = "sayall.activePage";
const LIVENESS_STORAGE_KEY = "sayall.livenessHeartbeat";
/** 心跳由 1 秒运行快照轮询续写；5 秒内仍新鲜即视为重载而非冷启动，
 * 余量覆盖 WebView 后台节流与轮询单次失败。 */
const LIVENESS_FRESH_MS = 5_000;

/** WebView 渲染进程崩溃重载会把整个应用重置回默认页（Bugs/2026-09-12），
 * 当前页持久化让恢复后停在用户离开时的页面。存储不可用时静默放弃：
 * 回到默认页不比重载前更差。 */
export function loadPersistedPage(): PageId | null {
  try {
    const stored = localStorage.getItem(ACTIVE_PAGE_STORAGE_KEY);
    return navigationItems.some((item) => item.id === stored)
      ? (stored as PageId)
      : null;
  } catch {
    return null;
  }
}

export function persistActivePage(page: PageId): void {
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
