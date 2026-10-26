// Types mirrored from the Rust backend (src-tauri/src). Keep in sync.

export const EVENTS = {
  transition: "dictation://transition",
  shortcutStatus: "shortcut://status",
  audioLevel: "audio://level",
  modelProgress: "model://progress",
  modelStatus: "model://status",
  settingsChanged: "settings://changed",
  micTestLevel: "mic-test://level",
  timings: "diagnostics://timings",
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

export type ModelStatus =
  | { state: "notInstalled" }
  | { state: "installed"; path: string; size: number }
  | { state: "invalid"; path: string; reason: string }
  | { state: "incomplete"; path: string; bytes: number };

export interface ModelInfo {
  /** True once the model is loaded and ready. */
  loaded: boolean;
  id: string;
  name: string;
  size: number;
  license: string;
  source: string;
  status: ModelStatus;
  installing: boolean;
}

export interface ModelProgress {
  downloaded: number;
  total: number;
}

export type LanguageCode = "en" | "de" | "fr" | "es" | "it" | "nl" | "pl";
export type OverlayPosition = "bottom" | "top";

export interface Settings {
  version: number;
  shortcut: string;
  shortcutBackend: BackendPreference;
  microphone: string | null;
  language: LanguageCode | null;
  maxRecordingSecs: number;
  startOnLogin: boolean;
  overlayPosition: OverlayPosition;
  showOverlay: boolean;
  soundFeedback: boolean;
  autoPaste: boolean;
  restoreClipboard: boolean;
  diagnostics: boolean;
}

export interface UpdateResult {
  settings: Settings;
  warnings: string[];
}

export interface InputDevice {
  id: string;
  name: string;
  isDefault: boolean;
}

export interface MicTestResult {
  deviceName: string;
  sampleRate: number;
  channels: number;
  durationMs: number;
  peak: number;
  rms: number;
  signalDetected: boolean;
}

export interface SessionTimings {
  session: number;
  captureOpenMs: number | null;
  overlayVisibleMs: number | null;
  recordedMs: number | null;
  transcribedMs: number | null;
  finalizeMs: number | null;
  inferenceMs: number | null;
  engineTtftMs: number | null;
  insertMs: number | null;
  releaseToInsertMs: number | null;
}

export interface SessionRecord {
  session: number;
  outcome: string | null;
  timings: SessionTimings | null;
}

export interface DiagnosticsReport {
  app: AppInfo;
  sessionType: SessionKind;
  shortcut: ShortcutStatus;
  engine: {
    engine: string;
    loaded: boolean;
    modelPath: string | null;
    loadMs: number | null;
  } | null;
  model: ModelStatus;
  settings: Record<string, unknown>;
  sessions: SessionRecord[];
}
