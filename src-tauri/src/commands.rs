//! Typed Tauri commands exposed to the frontend.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::events;
use crate::shortcuts::{Accelerator, BackendPreference, ShortcutStatus};
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

/// Registers a new shortcut. Runs on a blocking thread because a portal
/// backend may wait for the user to confirm.
#[tauri::command]
pub async fn set_shortcut(
    app: AppHandle,
    accelerator: String,
    backend: Option<BackendPreference>,
) -> Result<ShortcutStatus, String> {
    let accel: Accelerator = accelerator.parse().map_err(|e| format!("{e}"))?;
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = handle.state::<AppState>();
        let mut mgr = state.shortcuts.lock().expect("shortcut lock");
        let result = mgr.register(accel, backend.unwrap_or_default());
        let status = mgr.status();
        let _ = handle.emit(events::SHORTCUT_STATUS, &status);
        result.map(|_| status).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
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
