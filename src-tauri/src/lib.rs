//! VoiceFlow native backend.

pub mod commands;
pub mod events;
pub mod logging;
pub mod shortcuts;
pub mod state;
pub mod workflow;

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager, RunEvent};

use shortcuts::{
    Accelerator, BackendFactory, BackendKind, BackendPreference, KeyState, ShortcutManager,
    ShortcutService, ShortcutSink,
};
use state::AppState;
use workflow::{DiagnosticExecutor, Input, Transition};

fn backend_factory(app: AppHandle) -> BackendFactory {
    Box::new(move |kind| -> Option<Box<dyn ShortcutService>> {
        match kind {
            BackendKind::GlobalHotkey => {
                Some(Box::new(shortcuts::hotkey::HotkeyBackend::new(app.clone())))
            }
            #[cfg(target_os = "linux")]
            BackendKind::Portal => Some(Box::new(shortcuts::portal::PortalBackend::new())),
            #[cfg(target_os = "linux")]
            BackendKind::Evdev => Some(Box::new(shortcuts::evdev::EvdevBackend::new())),
            #[cfg(not(target_os = "linux"))]
            BackendKind::Portal | BackendKind::Evdev => None,
        }
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    logging::init();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting VoiceFlow");

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::dictation_state,
            commands::cancel_dictation,
            commands::shortcut_status,
            commands::set_shortcut,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let emitter = handle.clone();
            let observer = move |t: &Transition| {
                let _ = emitter.emit(events::TRANSITION, t);
            };
            let (workflow, _task) = tauri::async_runtime::block_on(async {
                let rt = tokio::runtime::Handle::current();
                workflow::controller::spawn(&rt, DiagnosticExecutor::default(), observer)
            });

            let wf = workflow.clone();
            let sink: ShortcutSink = Arc::new(move |ev| {
                wf.send(match ev.state {
                    KeyState::Pressed => Input::ShortcutPressed,
                    KeyState::Released => Input::ShortcutReleased,
                });
            });
            let manager = ShortcutManager::new(
                backend_factory(handle.clone()),
                sink,
                shortcuts::detect_session(),
            );
            app.manage(AppState {
                workflow,
                shortcuts: Mutex::new(manager),
            });

            // Registration may wait for a portal dialog, so keep it off the
            // main thread.
            std::thread::spawn(move || {
                let state = handle.state::<AppState>();
                let mut mgr = state.shortcuts.lock().expect("shortcut lock");
                // Development override until settings exist (milestone 8).
                let accel = std::env::var("VOICEFLOW_SHORTCUT")
                    .ok()
                    .and_then(|s| s.parse::<Accelerator>().ok())
                    .unwrap_or_default();
                if let Err(e) = mgr.register(accel, BackendPreference::Auto) {
                    tracing::error!(error = %e, "no shortcut backend could be registered");
                }
                let _ = handle.emit(events::SHORTCUT_STATUS, mgr.status());
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            let state = app.state::<AppState>();
            state.workflow.send(Input::Shutdown);
            if let Ok(mut mgr) = state.shortcuts.lock() {
                mgr.unregister();
            }
            tracing::info!("VoiceFlow stopped");
        }
    });
}
