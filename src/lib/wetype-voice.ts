import type { KeyChord, KeyCode } from "./bridge";

export const WETYPE_VOICE_KEYS: readonly KeyCode[] = ["left_control", "left_windows"];

export function isWeTypeVoiceChord(chord: KeyChord | null): boolean {
  return chord !== null && chord.keys.length === WETYPE_VOICE_KEYS.length
    && WETYPE_VOICE_KEYS.every((key) => chord.keys.includes(key));
}
