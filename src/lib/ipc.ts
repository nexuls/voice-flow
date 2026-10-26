import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  type AppInfo,
  type AudioLevel,
  type BackendPreference,
  EVENTS,
  type ModelInfo,
  type ModelProgress,
  type ShortcutStatus,
  type StateView,
  type Transition,
} from "@/types/ipc";

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  dictationState: () => invoke<StateView>("dictation_state"),
  cancelDictation: () => invoke<void>("cancel_dictation"),
  shortcutStatus: () => invoke<ShortcutStatus>("shortcut_status"),
  setShortcut: (accelerator: string, backend?: BackendPreference) =>
    invoke<ShortcutStatus>("set_shortcut", { accelerator, backend }),
  modelInfo: () => invoke<ModelInfo>("model_info"),
  installModel: () => invoke<ModelInfo>("install_model"),
  cancelModelInstall: () => invoke<void>("cancel_model_install"),
  removeModel: () => invoke<ModelInfo>("remove_model"),
  copyTranscript: () => invoke<void>("copy_transcript"),
  overlayRendered: (session: number) => invoke<void>("overlay_rendered", { session }),
};

export const on = {
  transition: (cb: (t: Transition) => void): Promise<UnlistenFn> =>
    listen<Transition>(EVENTS.transition, (e) => cb(e.payload)),
  shortcutStatus: (cb: (s: ShortcutStatus) => void): Promise<UnlistenFn> =>
    listen<ShortcutStatus>(EVENTS.shortcutStatus, (e) => cb(e.payload)),
  audioLevel: (cb: (l: AudioLevel) => void): Promise<UnlistenFn> =>
    listen<AudioLevel>(EVENTS.audioLevel, (e) => cb(e.payload)),
  modelProgress: (cb: (p: ModelProgress) => void): Promise<UnlistenFn> =>
    listen<ModelProgress>(EVENTS.modelProgress, (e) => cb(e.payload)),
};

/** True when running inside the Tauri webview rather than a plain browser. */
export const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
