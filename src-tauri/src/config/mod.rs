//! Persistent user preferences.
//!
//! Stored as JSON in the platform config directory. Loading is lenient: each
//! field is parsed on its own, and an invalid or obsolete value falls back to
//! its default instead of discarding the whole file. An unreadable file is set
//! aside as `settings.json.corrupt` and defaults are used. Writes are atomic
//! (temporary file, then rename).
//!
//! Settings never contain audio or transcripts.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::shortcuts::{Accelerator, BackendPreference};
use crate::transcription::Language;

pub const CURRENT_VERSION: u32 = 1;
pub const MIN_RECORDING_SECS: u32 = 5;
pub const MAX_RECORDING_SECS: u32 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum OverlayPosition {
    #[default]
    Bottom,
    Top,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub version: u32,
    pub shortcut: Accelerator,
    pub shortcut_backend: BackendPreference,
    /// Input device id, or `None` for the system default.
    pub microphone: Option<String>,
    /// `None` = automatic language detection.
    pub language: Option<Language>,
    pub max_recording_secs: u32,
    pub start_on_login: bool,
    pub overlay_position: OverlayPosition,
    /// Show the overlay while dictating. When off, only sounds give feedback.
    pub show_overlay: bool,
    /// Short tones when recording starts and stops.
    pub sound_feedback: bool,
    pub auto_paste: bool,
    pub restore_clipboard: bool,
    /// Privacy-preserving diagnostics: timings and errors only.
    pub diagnostics: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            shortcut: Accelerator::default(),
            shortcut_backend: BackendPreference::Auto,
            microphone: None,
            language: None,
            max_recording_secs: 120,
            start_on_login: false,
            overlay_position: OverlayPosition::Bottom,
            show_overlay: true,
            sound_feedback: false,
            auto_paste: true,
            restore_clipboard: true,
            diagnostics: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    #[error("invalid setting {field}: {reason}")]
    Invalid { field: &'static str, reason: String },
    #[error("could not save settings: {0}")]
    Io(String),
}

impl Settings {
    pub fn max_recording(&self) -> Duration {
        Duration::from_secs(self.max_recording_secs as u64)
    }

    /// Checks a settings value received from the UI. Unlike loading, invalid
    /// input is rejected rather than silently corrected.
    pub fn validate(&self) -> Result<(), SettingsError> {
        if !(MIN_RECORDING_SECS..=MAX_RECORDING_SECS).contains(&self.max_recording_secs) {
            return Err(SettingsError::Invalid {
                field: "maxRecordingSecs",
                reason: format!("must be between {MIN_RECORDING_SECS} and {MAX_RECORDING_SECS}"),
            });
        }
        if let Some(m) = &self.microphone
            && (m.is_empty() || m.len() > 1024)
        {
            return Err(SettingsError::Invalid {
                field: "microphone",
                reason: "invalid device id".into(),
            });
        }
        Ok(())
    }

    /// Lenient parse: valid fields are kept, invalid ones use defaults.
    /// Returns the settings and the names of the fields that were reset.
    pub fn from_value(value: &Value) -> (Self, Vec<&'static str>) {
        let mut s = Settings::default();
        let mut reset = Vec::new();
        let Some(obj) = value.as_object() else {
            return (s, vec!["<root>"]);
        };

        macro_rules! field {
            ($key:literal, $target:expr) => {
                if let Some(v) = obj.get($key) {
                    match serde_json::from_value(v.clone()) {
                        Ok(parsed) => $target = parsed,
                        Err(_) => reset.push($key),
                    }
                }
            };
        }
        field!("shortcut", s.shortcut);
        field!("shortcutBackend", s.shortcut_backend);
        field!("microphone", s.microphone);
        field!("language", s.language);
        field!("maxRecordingSecs", s.max_recording_secs);
        field!("startOnLogin", s.start_on_login);
        field!("overlayPosition", s.overlay_position);
        field!("showOverlay", s.show_overlay);
        field!("soundFeedback", s.sound_feedback);
        field!("autoPaste", s.auto_paste);
        field!("restoreClipboard", s.restore_clipboard);
        field!("diagnostics", s.diagnostics);

        if !(MIN_RECORDING_SECS..=MAX_RECORDING_SECS).contains(&s.max_recording_secs) {
            s.max_recording_secs = s
                .max_recording_secs
                .clamp(MIN_RECORDING_SECS, MAX_RECORDING_SECS);
            reset.push("maxRecordingSecs");
        }
        if s.microphone.as_deref().is_some_and(str::is_empty) {
            s.microphone = None;
            reset.push("microphone");
        }
        // Files written by older versions are migrated here. Version 1 is the
        // first format, so only the version number is updated.
        s.version = CURRENT_VERSION;
        (s, reset)
    }
}

#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            path: dir.as_ref().join("settings.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads settings, recovering from missing, corrupt or partly invalid files.
    pub fn load(&self) -> Settings {
        let raw = match std::fs::read_to_string(&self.path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Settings::default(),
            Err(e) => {
                tracing::warn!(error = %e, "settings unreadable; using defaults");
                return Settings::default();
            }
        };
        match serde_json::from_str::<Value>(&raw) {
            Ok(value) => {
                let (settings, reset) = Settings::from_value(&value);
                if !reset.is_empty() {
                    tracing::warn!(?reset, "invalid settings were reset to defaults");
                }
                settings
            }
            Err(e) => {
                let backup = self.path.with_extension("json.corrupt");
                let _ = std::fs::rename(&self.path, &backup);
                tracing::warn!(error = %e, backup = %backup.display(), "settings file corrupt; using defaults");
                Settings::default()
            }
        }
    }

    pub fn save(&self, settings: &Settings) -> Result<(), SettingsError> {
        let io = |e: std::io::Error| SettingsError::Io(e.to_string());
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let json =
            serde_json::to_vec_pretty(settings).map_err(|e| SettingsError::Io(e.to_string()))?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(io)?;
        std::fs::rename(&tmp, &self.path).map_err(io)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn defaults_round_trip() {
        let s = Settings::default();
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["shortcut"], "Ctrl+Space");
        assert_eq!(v["language"], Value::Null);
        let (back, reset) = Settings::from_value(&v);
        assert_eq!(back, s);
        assert!(reset.is_empty());
    }

    #[test]
    fn invalid_fields_fall_back_individually() {
        let v = json!({
            "version": 1,
            "shortcut": "Space",          // no modifier: invalid
            "language": "xx",             // unsupported
            "maxRecordingSecs": 100000,   // out of range: clamped
            "autoPaste": false,           // valid, must be kept
            "microphone": "",
            "removedInV2": true           // unknown keys are ignored
        });
        let (s, reset) = Settings::from_value(&v);
        assert_eq!(s.shortcut, Accelerator::default());
        assert_eq!(s.language, None);
        assert_eq!(s.max_recording_secs, MAX_RECORDING_SECS);
        assert!(!s.auto_paste);
        assert_eq!(s.microphone, None);
        for f in ["shortcut", "language", "maxRecordingSecs", "microphone"] {
            assert!(reset.contains(&f), "{f} should be reported");
        }
    }

    #[test]
    fn non_object_root_gives_defaults() {
        let (s, reset) = Settings::from_value(&json!([1, 2]));
        assert_eq!(s, Settings::default());
        assert_eq!(reset, ["<root>"]);
    }

    #[test]
    fn older_version_is_migrated() {
        let (s, _) = Settings::from_value(&json!({"version": 0, "language": "de"}));
        assert_eq!(s.version, CURRENT_VERSION);
        assert_eq!(s.language, Some(Language::De));
    }

    #[test]
    fn validate_rejects_bad_ui_input() {
        let mut s = Settings::default();
        assert!(s.validate().is_ok());
        s.max_recording_secs = 1;
        assert!(s.validate().is_err());
        s.max_recording_secs = 60;
        s.microphone = Some(String::new());
        assert!(s.validate().is_err());
    }

    #[test]
    fn store_saves_atomically_and_loads() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(dir.path().join("nested"));
        assert_eq!(store.load(), Settings::default(), "missing file");
        let s = Settings {
            language: Some(Language::Fr),
            sound_feedback: true,
            ..Default::default()
        };
        store.save(&s).unwrap();
        assert_eq!(store.load(), s);
        assert!(!store.path().with_extension("json.tmp").exists());
    }

    #[test]
    fn corrupt_file_is_set_aside() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(dir.path());
        std::fs::write(store.path(), "{ not json").unwrap();
        assert_eq!(store.load(), Settings::default());
        assert!(store.path().with_extension("json.corrupt").exists());
        assert!(!store.path().exists());
    }
}
