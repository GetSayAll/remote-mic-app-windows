import type { KeyCode } from "./bridge";

/**
 * 预设快捷键分组（对齐 Mac `ButtonActionCategory` 的 basicKeys/systemAndMedia，
 * 并按 Windows 语义适配：Home/End/PageUp/PageDown 属低频导航键、Mac 端基础
 * 按键列表亦无此四键，故移除；复制族从系统组移入基础组，对齐 Mac basicKeys）。
 *
 * 按键页「按键选择」与组合动作「单键或快捷键 / 系统与媒体动作」共用同一份清单，
 * 避免两个入口出现不同的“常用快捷键”（2026-10-19 用户要求）。
 */
export interface ShortcutPresetGroup {
  label: string;
  items: Array<{ label: string; keys: KeyCode[]; display?: string }>;
}

export const SHORTCUT_PRESET_GROUPS: ShortcutPresetGroup[] = [
  {
    label: "基础按键",
    items: [
      { label: "Enter", keys: ["enter"] },
      { label: "Esc", keys: ["escape"] },
      { label: "空格", keys: ["space"] },
      { label: "Tab", keys: ["tab"] },
      { label: "退格", keys: ["backspace"] },
      { label: "删除", keys: ["delete"] },
      { label: "↑", keys: ["up"] },
      { label: "↓", keys: ["down"] },
      { label: "←", keys: ["left"] },
      { label: "→", keys: ["right"] },
      // Home 为"主页键同键映射"的必需预设（能力矩阵 identity 档的唯一
      // 合法目标；Mac 端基础键列表无此键，Windows 端因泄漏对冲需要保留）。
      { label: "Home", keys: ["home"] },
      { label: "复制", keys: ["control", "c"] },
      { label: "粘贴", keys: ["control", "v"] },
      { label: "剪切", keys: ["control", "x"] },
      { label: "全选", keys: ["control", "a"] },
      { label: "撤销", keys: ["control", "z"] },
      { label: "重做", keys: ["control", "y"] },
      { label: "查找", keys: ["control", "f"] },
      { label: "保存", keys: ["control", "s"] },
      { label: "发送", keys: ["control", "enter"] },
      { label: "换行", keys: ["shift", "enter"] },
      { label: "右键菜单", keys: ["apps"] },
      { label: "刷新", keys: ["f5"] },
    ],
  },
  {
    label: "系统与媒体",
    items: [
      { label: "切换窗口", keys: ["alt", "tab"] },
      { label: "显示桌面", keys: ["left_windows", "d"], display: "Win + D" },
      { label: "关闭窗口", keys: ["control", "w"] },
      { label: "锁定", keys: ["left_windows", "l"], display: "Win + L" },
      { label: "搜索", keys: ["left_windows", "s"], display: "Win + S" },
      { label: "截图", keys: ["left_windows", "shift", "s"], display: "Win + Shift + S" },
      { label: "静音", keys: ["volume_mute"] },
      { label: "音量+", keys: ["volume_up"] },
      { label: "音量−", keys: ["volume_down"] },
      { label: "播放/暂停", keys: ["media_play_pause"] },
      { label: "上一首", keys: ["media_prev"] },
      { label: "下一首", keys: ["media_next"] },
    ],
  },
];
