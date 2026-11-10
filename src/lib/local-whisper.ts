import { invoke } from "@tauri-apps/api/core";

export type WhisperFileKind = "executable" | "model" | "audio";
export type WhisperLanguage = "zh" | "en" | "auto";

export interface WhisperEngineRequest {
  executable: string;
  model: string;
  language: WhisperLanguage;
}

export interface WhisperRequest extends WhisperEngineRequest {
  wav: string;
}

export interface WhisperDraft {
  text: string;
  durationMs: number;
  elapsedMs: number;
}

export type LiveWhisperPhase =
  | "armed" | "capturing" | "transcribing" | "ready" | "cancelled" | "expired" | "failed";

export interface LiveWhisperStatus {
  phase: LiveWhisperPhase;
  draft?: WhisperDraft;
  error?: string;
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

export function armLocalWhisper(request: WhisperEngineRequest): Promise<{ token: number }> {
  return invoke("arm_local_whisper", { request });
}

export function getLocalWhisperStatus(token: number): Promise<LiveWhisperStatus> {
  return invoke("get_local_whisper_status", { token });
}

export function cancelLiveLocalWhisper(token: number): Promise<boolean> {
  return invoke("cancel_live_local_whisper", { token });
}
