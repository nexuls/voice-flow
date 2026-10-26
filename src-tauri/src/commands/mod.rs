//! Typed Tauri commands exposed to the frontend.

pub mod model;
pub mod settings;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

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

/// Never blocks: registration may hold the manager while a portal dialog is open.
#[tauri::command]
pub fn shortcut_status(state: State<'_, AppState>) -> ShortcutStatus {
    if let Some(s) = state.shortcut_status.read().expect("status lock").clone() {
        return s;
    }
    match state.shortcuts.try_lock() {
        Ok(mgr) => mgr.status(),
        Err(_) => ShortcutStatus {
            accelerator: None,
            backend: None,
            session: crate::shortcuts::detect_session(),
            error: None,
        },
    }
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsReport {
    pub app: AppInfo,
    pub session_type: crate::shortcuts::SessionKind,
    pub shortcut: ShortcutStatus,
    pub engine: Option<crate::transcription::EngineStatus>,
    pub model: crate::transcription::model::ModelStatus,
    /// Settings without device identifiers.
    pub settings: serde_json::Value,
    pub sessions: Vec<crate::diagnostics::SessionRecord>,
}

/// Builds a report for bug reports: versions, backends, settings and recent
/// timings and outcomes. Contains no audio, transcripts, window titles or
/// device names. Model paths are reduced to their file name.
#[tauri::command]
pub async fn diagnostics_report(app: AppHandle) -> Result<DiagnosticsReport, String> {
    use crate::transcription::model::{ModelStatus, WHISTLE};
    let state = app.state::<AppState>();
    let models = app.state::<crate::state::ModelState>();
    let mut settings = serde_json::to_value(&*state.settings.read().expect("settings lock"))
        .map_err(|e| e.to_string())?;
    if let Some(obj) = settings.as_object_mut() {
        let custom = obj.get("microphone").is_some_and(|m| !m.is_null());
        obj.insert(
            "microphone".into(),
            serde_json::Value::from(if custom { "custom" } else { "default" }),
        );
    }
    let file_only =
        |p: &std::path::Path| std::path::PathBuf::from(p.file_name().unwrap_or_default());
    let model = match models.store.status(&WHISTLE) {
        ModelStatus::Installed { path, size } => ModelStatus::Installed {
            path: file_only(&path),
            size,
        },
        ModelStatus::Invalid { path, reason } => ModelStatus::Invalid {
            path: file_only(&path),
            reason,
        },
        ModelStatus::Incomplete { path, bytes } => ModelStatus::Incomplete {
            path: file_only(&path),
            bytes,
        },
        other => other,
    };
    let mut engine = models.transcription.status().await;
    if let Some(e) = engine.as_mut() {
        e.model_path = e.model_path.as_deref().map(file_only);
    }
    Ok(DiagnosticsReport {
        app: app_info_value(),
        session_type: crate::shortcuts::detect_session(),
        shortcut: shortcut_status(state.clone()),
        engine,
        model,
        settings,
        sessions: state.diagnostics.records(),
    })
}

/// Copies the diagnostics report to the clipboard as JSON.
#[tauri::command]
pub async fn copy_diagnostics_report(app: AppHandle) -> Result<(), String> {
    let report = diagnostics_report(app.clone()).await?;
    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    app.state::<AppState>()
        .insertion
        .copy(json)
        .await
        .map_err(|e| e.to_string())
}
