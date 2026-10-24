import type { KeyCode } from "./bridge";
import { t } from "./i18n";

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

/** label 用 getter：常量表也要在语言切换后按当前语言重新取值。 */
function localize(label: string): string {
  return /[\u4e00-\u9fff]/.test(label) ? t(label) : label;
}

function preset(label: string, keys: KeyCode[], display?: string) {
  return {
    get label() {
      return localize(label);
    },
    keys,
    display,
  };
}

function group(label: string, items: ShortcutPresetGroup["items"]) {
  return {
    get label() {
      return localize(label);
    },
    items,
  };
}

export const SHORTCUT_PRESET_GROUPS: ShortcutPresetGroup[] = [
  group("基础按键", [
    preset("Enter", ["enter"]),
    preset("Esc", ["escape"]),
    preset("空格", ["space"]),
    preset("Tab", ["tab"]),
    preset("退格", ["backspace"]),
    preset("删除", ["delete"]),
    preset("↑", ["up"]),
    preset("↓", ["down"]),
    preset("←", ["left"]),
    preset("→", ["right"]),
    // Home 为"主页键同键映射"的必需预设（能力矩阵 identity 档的唯一
    // 合法目标；Mac 端基础键列表无此键，Windows 端因泄漏对冲需要保留）。
    preset("Home", ["home"]),
    preset("复制", ["control", "c"]),
    preset("粘贴", ["control", "v"]),
    preset("剪切", ["control", "x"]),
    preset("全选", ["control", "a"]),
    preset("撤销", ["control", "z"]),
    preset("重做", ["control", "y"]),
    preset("查找", ["control", "f"]),
    preset("保存", ["control", "s"]),
    preset("发送", ["control", "enter"]),
    preset("换行", ["shift", "enter"]),
    preset("右键菜单", ["apps"]),
    preset("刷新", ["f5"]),
  ]),
  group("系统与媒体", [
    preset("切换窗口", ["alt", "tab"]),
    preset("显示桌面", ["left_windows", "d"], "Win + D"),
    preset("关闭窗口", ["control", "w"]),
    preset("锁定", ["left_windows", "l"], "Win + L"),
    preset("搜索", ["left_windows", "s"], "Win + S"),
    preset("截图", ["left_windows", "shift", "s"], "Win + Shift + S"),
    preset("静音", ["volume_mute"]),
    preset("音量+", ["volume_up"]),
    preset("音量−", ["volume_down"]),
    preset("播放/暂停", ["media_play_pause"]),
    preset("上一首", ["media_prev"]),
    preset("下一首", ["media_next"]),
  ]),
];
