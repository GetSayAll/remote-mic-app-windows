import { invoke } from "@tauri-apps/api/core";

export type WhisperFileKind = "executable" | "model" | "audio";
export type WhisperLanguage = "zh" | "en" | "auto";

export interface WhisperRequest {
  executable: string;
  model: string;
  wav: string;
  language: WhisperLanguage;
}

export interface WhisperDraft {
  text: string;
  durationMs: number;
  elapsedMs: number;
}

export function canUseLocalWhisper(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export function pickLocalWhisperFile(kind: WhisperFileKind): Promise<string | null> {
  return invoke("pick_local_whisper_file", { kind });
}

export function transcribeLocalWhisper(request: WhisperRequest): Promise<WhisperDraft> {
  return invoke("transcribe_local_whisper", { request });
}

export function cancelLocalWhisper(): Promise<boolean> {
  return invoke("cancel_local_whisper");
}
