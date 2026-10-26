//! Speech model commands: status, explicit installation, removal and loading.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::events;
use crate::state::ModelState;
use crate::transcription::model::{ModelStatus, WHISTLE};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub size: u64,
    pub license: &'static str,
    pub source: &'static str,
    pub status: ModelStatus,
    pub installing: bool,
    /// True once the model is loaded and ready to transcribe.
    pub loaded: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    downloaded: u64,
    total: u64,
}

fn info(state: &ModelState) -> ModelInfo {
    ModelInfo {
        id: WHISTLE.id,
        name: WHISTLE.display_name,
        size: WHISTLE.size,
        license: WHISTLE.license,
        source: WHISTLE.url,
        status: state.store.status(&WHISTLE),
        installing: state.install.lock().expect("install lock").is_some(),
        loaded: state.engine_ready.load(Ordering::SeqCst),
    }
}

#[tauri::command]
pub fn model_info(state: State<'_, ModelState>) -> ModelInfo {
    info(&state)
}

/// Downloads and verifies the model, then loads it. This is the only code
/// path that makes a network request, and it runs only when the user asks.
#[tauri::command]
pub async fn install_model(app: AppHandle) -> Result<ModelInfo, String> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let state = app.state::<ModelState>();
        let mut slot = state.install.lock().expect("install lock");
        if slot.is_some() {
            return Err("an installation is already running".into());
        }
        *slot = Some(cancel.clone());
    }
    let _ = app.emit(events::MODEL_STATUS, info(&app.state::<ModelState>()));
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = handle.state::<ModelState>();
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        state.store.install(&WHISTLE, &cancel, |downloaded, total| {
            // Throttle progress events to about 10 per second.
            if last_emit.elapsed() >= Duration::from_millis(100) || downloaded == total {
                last_emit = Instant::now();
                let _ = handle.emit(events::MODEL_PROGRESS, Progress { downloaded, total });
            }
        })
    })
    .await
    .map_err(|e| e.to_string())?;
    let state = app.state::<ModelState>();
    state.install.lock().expect("install lock").take();
    match result {
        Ok(path) => {
            tracing::info!(path = %path.display(), "model installed");
            load_installed_model(&app).await;
            Ok(info(&state))
        }
        Err(e) => {
            tracing::warn!(error = %e, "model install failed");
            let _ = app.emit(events::MODEL_STATUS, info(&state));
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub fn cancel_model_install(state: State<'_, ModelState>) {
    if let Some(flag) = state.install.lock().expect("install lock").as_ref() {
        flag.store(true, Ordering::SeqCst);
    }
}

/// Deletes the model file. The engine keeps an already loaded model until
/// the app restarts (Needle has no unload call), but it is no longer used.
#[tauri::command]
pub fn remove_model(app: AppHandle, state: State<'_, ModelState>) -> Result<ModelInfo, String> {
    if state.install.lock().expect("install lock").is_some() {
        return Err("an installation is running".into());
    }
    state.store.remove(&WHISTLE).map_err(|e| e.to_string())?;
    state.engine_ready.store(false, Ordering::SeqCst);
    let i = info(&state);
    let _ = app.emit(events::MODEL_STATUS, &i);
    Ok(i)
}

/// Verifies and loads the installed model, if any. Emits the new status.
pub async fn load_installed_model(app: &AppHandle) {
    let state = app.state::<ModelState>();
    if !matches!(state.store.status(&WHISTLE), ModelStatus::Installed { .. }) {
        tracing::info!("speech model not installed");
        let _ = app.emit(events::MODEL_STATUS, info(&state));
        return;
    }
    let store = state.store.clone();
    let verified = tauri::async_runtime::spawn_blocking(move || store.verify(&WHISTLE)).await;
    match verified {
        Ok(Ok(path)) => match state.transcription.load(path).await {
            Ok(status) => {
                state.engine_ready.store(true, Ordering::SeqCst);
                tracing::info!(?status, "speech engine ready");
            }
            Err(e) => tracing::error!(error = %e, "loading speech model failed"),
        },
        Ok(Err(e)) => tracing::error!(error = %e, "installed speech model failed verification"),
        Err(e) => tracing::error!(error = %e, "model verification task failed"),
    }
    let _ = app.emit(events::MODEL_STATUS, info(&state));
}
