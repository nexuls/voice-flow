//! Typed Tauri commands exposed to the frontend.

pub mod model;
pub mod settings;

use serde::Serialize;
use tauri::State;

use crate::shortcuts::ShortcutStatus;
use crate::state::AppState;
use crate::workflow::{Input, StateView};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
}

pub fn app_info_value() -> AppInfo {
    AppInfo {
        name: "VoiceFlow",
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    app_info_value()
}

#[tauri::command]
pub async fn dictation_state(state: State<'_, AppState>) -> Result<StateView, String> {
    state
        .workflow
        .state()
        .await
        .ok_or_else(|| "workflow stopped".to_owned())
}

#[tauri::command]
pub fn cancel_dictation(state: State<'_, AppState>) {
    state.workflow.send(Input::Cancel);
}

#[tauri::command]
pub fn shortcut_status(state: State<'_, AppState>) -> ShortcutStatus {
    state.shortcuts.lock().expect("shortcut lock").status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_reports_crate_version() {
        let info = app_info_value();
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert!(!info.os.is_empty());
    }
}

/// Copies the transcript kept after a failed insertion to the clipboard.
#[tauri::command]
pub async fn copy_transcript(state: State<'_, AppState>) -> Result<(), String> {
    let text = state
        .workflow
        .retained_transcript()
        .await
        .ok_or_else(|| "no transcript to copy".to_owned())?;
    state
        .insertion
        .copy(text)
        .await
        .map_err(|e| e.to_string())?;
    // The user has their text, so dismiss the error.
    state.workflow.send(Input::Cancel);
    Ok(())
}

/// Reported by the overlay after it has painted the recording state.
#[tauri::command]
pub fn overlay_rendered(state: State<'_, AppState>, session: u64) {
    if let Some(s) = &state.services {
        s.overlay_rendered(session);
    }
}
