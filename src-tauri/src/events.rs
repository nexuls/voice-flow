//! Event names emitted to the frontend. Keep in sync with `src/types/ipc.ts`.

pub const TRANSITION: &str = "dictation://transition";
pub const SHORTCUT_STATUS: &str = "shortcut://status";
pub const AUDIO_LEVEL: &str = "audio://level";
pub const MODEL_PROGRESS: &str = "model://progress";
pub const MODEL_STATUS: &str = "model://status";
pub const TIMINGS: &str = "diagnostics://timings";
pub const SETTINGS_CHANGED: &str = "settings://changed";
pub const MIC_TEST_LEVEL: &str = "mic-test://level";
