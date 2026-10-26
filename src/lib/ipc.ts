import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  type AppInfo,
  type AudioLevel,
  EVENTS,
  type InputDevice,
  type MicTestResult,
  type ModelInfo,
  type ModelProgress,
  type SessionTimings,
  type Settings,
  type ShortcutStatus,
  type StateView,
  type Transition,
  type UpdateResult,
} from "@/types/ipc";

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  dictationState: () => invoke<StateView>("dictation_state"),
  cancelDictation: () => invoke<void>("cancel_dictation"),
  shortcutStatus: () => invoke<ShortcutStatus>("shortcut_status"),
  modelInfo: () => invoke<ModelInfo>("model_info"),
  installModel: () => invoke<ModelInfo>("install_model"),
  cancelModelInstall: () => invoke<void>("cancel_model_install"),
  removeModel: () => invoke<ModelInfo>("remove_model"),
  copyTranscript: () => invoke<void>("copy_transcript"),
  overlayRendered: (session: number) => invoke<void>("overlay_rendered", { session }),
  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (settings: Settings) => invoke<UpdateResult>("update_settings", { settings }),
  listMicrophones: () => invoke<InputDevice[]>("list_microphones"),
  startOnLoginEnabled: () => invoke<boolean>("start_on_login_enabled"),
  testMicrophone: (seconds: number) => invoke<MicTestResult>("test_microphone", { seconds }),
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
  modelStatus: (cb: (m: ModelInfo) => void): Promise<UnlistenFn> =>
    listen<ModelInfo>(EVENTS.modelStatus, (e) => cb(e.payload)),
  settingsChanged: (cb: (s: Settings) => void): Promise<UnlistenFn> =>
    listen<Settings>(EVENTS.settingsChanged, (e) => cb(e.payload)),
  micTestLevel: (cb: (l: AudioLevel) => void): Promise<UnlistenFn> =>
    listen<AudioLevel>(EVENTS.micTestLevel, (e) => cb(e.payload)),
  timings: (cb: (t: SessionTimings) => void): Promise<UnlistenFn> =>
    listen<SessionTimings>(EVENTS.timings, (e) => cb(e.payload)),
};

/** True when running inside the Tauri webview rather than a plain browser. */
export const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
