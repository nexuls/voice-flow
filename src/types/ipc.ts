// Types mirrored from the Rust backend (src-tauri/src). Keep in sync.

export const EVENTS = {
  transition: "dictation://transition",
  shortcutStatus: "shortcut://status",
  audioLevel: "audio://level",
} as const;

export type ErrorKind =
  | "noSpeech"
  | "tooShort"
  | "microphone"
  | "modelUnavailable"
  | "transcription"
  | "insertion"
  | "internal";

export interface DictationError {
  kind: ErrorKind;
  message: string;
}

export type InsertOutcome = "pasted" | "copiedToClipboard";

export type StateView =
  | { phase: "idle" }
  | { phase: "recording"; session: number }
  | { phase: "processing"; session: number }
  | { phase: "inserting"; session: number }
  | { phase: "success"; session: number; outcome: InsertOutcome }
  | { phase: "error"; session: number; error: DictationError; hasTranscript: boolean };

export type Phase = StateView["phase"];

export interface Transition {
  seq: number;
  atMs: number;
  input: string;
  from: StateView;
  to: StateView;
}

export type BackendKind = "globalHotkey" | "portal" | "evdev";
export type BackendPreference = "auto" | BackendKind;
export type SessionKind = "wayland" | "x11" | "other";

export interface ShortcutStatus {
  accelerator: string | null;
  backend: BackendKind | null;
  session: SessionKind;
  error: string | null;
}

export interface AppInfo {
  name: string;
  version: string;
  os: string;
  arch: string;
}

/** Throttled microphone level for the visualizer. */
export interface AudioLevel {
  /** Normalised level in [0, 1]. */
  level: number;
}
