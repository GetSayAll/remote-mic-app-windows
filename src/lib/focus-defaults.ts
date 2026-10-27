import type { AppFocusProfile, FocusStrategy } from "./bridge";

/** 按键页与组合动作页共用的聚焦方式选项（顺序即界面顺序）。 */
export const FOCUS_STRATEGY_OPTIONS: ReadonlyArray<{
  value: FocusStrategy;
  label: string;
}> = [
  { value: "open_only", label: "只打开 APP" },
  { value: "auto_composer", label: "自动选择输入框" },
  { value: "app_shortcut", label: "使用 APP 快捷键聚焦" },
  { value: "recorded_element", label: "聚焦已记录的输入框" },
  { value: "recorded_point", label: "记录点击位置" },
];

/**
 * 预设应用的默认聚焦方式（与 Rust `preset_focus_profile` 同源）。
 *
 * 按键页与组合动作页都从这里读取，避免两处对“Chrome 地址栏 / 网易云搜索框”
 * 的默认值出现分叉。用户显式保存的档案优先于这里的默认值。
 */
export const PRESET_FOCUS_DEFAULTS: Record<string, AppFocusProfile> = {
  chrome: {
    strategy: "app_shortcut",
    shortcut: { keys: ["left_control", "l"] },
  },
  netease_music: {
    strategy: "app_shortcut",
    shortcut: { keys: ["left_control", "f"] },
  },
};

/** 预设应用的默认档案；未列出的预设使用通用输入框自动选择。 */
export function presetFocusDefault(
  target: string,
  isPreset: boolean,
): AppFocusProfile | null {
  if (!isPreset) return null;
  return PRESET_FOCUS_DEFAULTS[target] ?? { strategy: "auto_composer" };
}

/** 按键页实际会执行的聚焦档案：显式档案优先，否则预设默认。 */
export function effectiveFocusProfile(
  target: string,
  profiles: Record<string, AppFocusProfile> | undefined,
  isPreset: (target: string) => boolean,
): AppFocusProfile | null {
  return profiles?.[target] ?? presetFocusDefault(target, isPreset(target));
}
