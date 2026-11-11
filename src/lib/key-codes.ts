import type { KeyCode } from "./bridge";

/** 浏览器 `KeyboardEvent.code` → 产品内部 KeyCode。 */
export function codeToKeyCode(code: string): KeyCode | null {
  const modifierMap: Record<string, KeyCode> = {
    ControlLeft: "left_control",
    ControlRight: "right_control",
    ShiftLeft: "left_shift",
    ShiftRight: "right_shift",
    AltLeft: "left_alt",
    AltRight: "right_alt",
    MetaLeft: "left_windows",
    MetaRight: "right_windows",
  };
  if (modifierMap[code]) return modifierMap[code];
  const named: Record<string, KeyCode> = {
    Enter: "enter",
    Space: "space",
    Tab: "tab",
    Backspace: "backspace",
    Escape: "escape",
    ArrowLeft: "left",
    ArrowUp: "up",
    ArrowRight: "right",
    ArrowDown: "down",
    Home: "home",
    End: "end",
    PageUp: "page_up",
    PageDown: "page_down",
    Insert: "insert",
    Delete: "delete",
    ContextMenu: "apps",
    Semicolon: "oem1",
    Equal: "oem_plus",
    Comma: "oem_comma",
    Minus: "oem_minus",
    Period: "oem_period",
    Slash: "oem2",
    Backquote: "oem3",
    BracketLeft: "oem4",
    Backslash: "oem5",
    BracketRight: "oem6",
    Quote: "oem7",
    IntlBackslash: "oem102",
    Numpad0: "numpad0",
    Numpad1: "numpad1",
    Numpad2: "numpad2",
    Numpad3: "numpad3",
    Numpad4: "numpad4",
    Numpad5: "numpad5",
    Numpad6: "numpad6",
    Numpad7: "numpad7",
    Numpad8: "numpad8",
    Numpad9: "numpad9",
    NumpadMultiply: "numpad_multiply",
    NumpadAdd: "numpad_add",
    NumpadSubtract: "numpad_subtract",
    NumpadDecimal: "numpad_decimal",
    NumpadDivide: "numpad_divide",
    NumLock: "numlock",
    VolumeMute: "volume_mute",
    VolumeUp: "volume_up",
    VolumeDown: "volume_down",
  };
  if (named[code]) return named[code];
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return letter[1].toLowerCase();
  const digit = /^Digit([0-9])$/.exec(code);
  if (digit) return `digit${digit[1]}`;
  const functionKey = /^F([1-9]|1[0-2])$/.exec(code);
  if (functionKey) return `f${functionKey[1]}`;
  return null;
}

export const MODIFIER_KEYS = new Set<KeyCode>([
  "left_control",
  "right_control",
  "left_shift",
  "right_shift",
  "left_alt",
  "right_alt",
  "left_windows",
  "right_windows",
]);

export const CAPTURE_MODIFIER_SOURCES: Array<{ key: KeyCode; label: string }> = [
  { key: "left_control", label: "左 Ctrl" },
  { key: "left_shift", label: "左 Shift" },
  { key: "left_alt", label: "左 Alt" },
  { key: "left_windows", label: "左 Win" },
  { key: "right_control", label: "右 Ctrl" },
  { key: "right_shift", label: "右 Shift" },
  { key: "right_alt", label: "右 Alt" },
  { key: "right_windows", label: "右 Win" },
];
