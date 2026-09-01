export type PageId = "buttons" | "statistics" | "connection" | "permissions" | "about";

export interface NavigationItem {
  id: PageId;
  label: string;
  icon: string;
}

export const navigationItems: NavigationItem[] = [
  { id: "buttons", label: "按键", icon: "⌨" },
  { id: "statistics", label: "统计", icon: "▥" },
  { id: "connection", label: "连接与语音", icon: "◉" },
  { id: "permissions", label: "权限", icon: "◆" },
  { id: "about", label: "关于", icon: "ⓘ" },
];
