//! Settings commands and applying settings to the running services.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;

use crate::audio::cpal_capture::{CpalCapture, list_input_devices};
use crate::audio::{AudioCaptureService, CaptureConfig, CaptureEvent, CaptureEvents, InputDevice};
use crate::config::Settings;
use crate::events;
use crate::insertion::PasteOptions;
use crate::shortcuts::ShortcutStatus;
use crate::state::AppState;
use crate::workflow::StateView;

/// Applies settings that take effect without restarting. The shortcut is
/// handled separately because registering it may block.
pub fn apply_runtime(app: &AppHandle, old: Option<&Settings>, new: &Settings) -> Vec<String> {
    let state = app.state::<AppState>();
    let mut warnings = Vec::new();
    {
        let mut live = state.live_config.write().expect("live config lock");
        live.capture.device_id = new.microphone.clone();
        live.capture.max_duration = new.max_recording();
        live.transcribe.language = new.language;
    }
    state.insertion.set_options(PasteOptions {
        auto_paste: new.auto_paste,
        restore_clipboard: new.restore_clipboard,
        ..PasteOptions::default()
    });
    state
        .overlay
        .configure(new.overlay_position, new.show_overlay);

    if old.map(|o| o.start_on_login) != Some(new.start_on_login) {
        let autostart = app.autolaunch();
        let actual = autostart.is_enabled().unwrap_or(false);
        if actual != new.start_on_login {
            let r = if new.start_on_login {
                autostart.enable()
            } else {
                autostart.disable()
            };
            if let Err(e) = r {
                tracing::warn!(error = %e, "start-on-login change failed");
                warnings.push(format!("Start on login could not be changed: {e}"));
            }
        }
    }
    warnings
}

/// Registers the shortcut from `settings`. Blocking: call it from a
/// blocking thread.
pub fn apply_shortcut(app: &AppHandle, settings: &Settings) -> Result<ShortcutStatus, String> {
    let state = app.state::<AppState>();
    let mut mgr = state.shortcuts.lock().expect("shortcut lock");
    let result = mgr.register(settings.shortcut, settings.shortcut_backend);
    let status = mgr.status();
    let _ = app.emit(events::SHORTCUT_STATUS, &status);
    result.map(|_| status).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.read().expect("settings lock").clone()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateResult {
    pub settings: Settings,
    pub warnings: Vec<String>,
}

#[tauri::command]
pub async fn update_settings(
    app: AppHandle,
    mut settings: Settings,
) -> Result<UpdateResult, String> {
    settings.validate().map_err(|e| e.to_string())?;
    settings.version = crate::config::CURRENT_VERSION;
    let old = app
        .state::<AppState>()
        .settings
        .read()
        .expect("settings lock")
        .clone();
    let mut warnings = Vec::new();

    let shortcut_changed =
        old.shortcut != settings.shortcut || old.shortcut_backend != settings.shortcut_backend;
    if shortcut_changed {
        let handle = app.clone();
        let wanted = settings.clone();
        let result = tauri::async_runtime::spawn_blocking(move || apply_shortcut(&handle, &wanted))
            .await
            .map_err(|e| e.to_string())?;
        if let Err(e) = result {
            // The manager restored the previous shortcut. Keep the persisted
            // setting in sync with what is actually registered.
            settings.shortcut = old.shortcut;
            settings.shortcut_backend = old.shortcut_backend;
            warnings.push(e);
        }
    }

    warnings.extend(apply_runtime(&app, Some(&old), &settings));
    let state = app.state::<AppState>();
    state
        .settings_store
        .save(&settings)
        .map_err(|e| e.to_string())?;
    *state.settings.write().expect("settings lock") = settings.clone();
    let _ = app.emit(events::SETTINGS_CHANGED, &settings);
    Ok(UpdateResult { settings, warnings })
}

#[tauri::command]
pub fn list_microphones() -> Result<Vec<InputDevice>, String> {
    list_input_devices().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn start_on_login_enabled(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicTestResult {
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_ms: f64,
    pub peak: f32,
    pub rms: f32,
    /// Heuristic: the signal clearly rose above the noise floor.
    pub signal_detected: bool,
}

/// Records for a few seconds with the configured microphone and reports
/// levels. The audio is discarded.
#[tauri::command]
pub async fn test_microphone(app: AppHandle, seconds: f64) -> Result<MicTestResult, String> {
    let state = app.state::<AppState>();
    if !matches!(state.workflow.state().await, Some(StateView::Idle)) {
        return Err("Dictation is in progress".into());
    }
    let config = CaptureConfig {
        device_id: state
            .settings
            .read()
            .expect("settings lock")
            .microphone
            .clone(),
        max_duration: Duration::from_secs(10),
    };
    let secs = seconds.clamp(1.0, 10.0);
    let emitter = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let events: CaptureEvents = Arc::new(move |e| {
            if let CaptureEvent::Level(l) = e {
                let _ = emitter.emit(events::MIC_TEST_LEVEL, serde_json::json!({ "level": l }));
            }
        });
        let mut capture = CpalCapture::new();
        let info = capture.start(&config, events).map_err(|e| e.to_string())?;
        std::thread::sleep(Duration::from_secs_f64(secs));
        let rec = capture.finish().map_err(|e| e.to_string())?;
        Ok(MicTestResult {
            device_name: info.device_name,
            sample_rate: info.sample_rate,
            channels: info.channels,
            duration_ms: rec.duration().as_secs_f64() * 1e3,
            peak: rec.stats.peak,
            rms: rec.stats.rms,
            signal_detected: rec.stats.peak > 0.05,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
