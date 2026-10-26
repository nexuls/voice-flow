//! VoiceFlow native backend.

pub mod audio;
pub mod commands;
pub mod config;
pub mod events;
pub mod executor;
pub mod insertion;
pub mod live;
pub mod logging;
pub mod overlay;
pub mod shortcuts;
pub mod state;
pub mod transcription;
pub mod workflow;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};

use overlay::OverlayController;
use shortcuts::{
    Accelerator, BackendFactory, BackendKind, KeyState, ShortcutManager, ShortcutService,
    ShortcutSink,
};
use state::{AppState, ModelState};
use workflow::{DiagnosticExecutor, Executor, Input, LevelSink, StateView, Transition};

/// Command-line flag used by start-on-login: start without opening Settings.
pub const BACKGROUND_FLAG: &str = "--background";
const MAIN_WINDOW: &str = "main";

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

/// Shows and focuses the settings window.
pub fn show_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN_WINDOW) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// The Linux tray needs an AppIndicator library at runtime, and the tray
/// crate panics instead of returning an error when it is missing. Probe first.
#[cfg(target_os = "linux")]
fn tray_supported() -> bool {
    const LIBS: [&str; 4] = [
        "libayatana-appindicator3.so.1",
        "libappindicator3.so.1",
        "libayatana-appindicator3.so",
        "libappindicator3.so",
    ];
    LIBS.iter().any(|name| {
        let c = std::ffi::CString::new(*name).expect("static name");
        // SAFETY: dlopen with a valid C string. The handle is closed at once.
        unsafe {
            let h = libc::dlopen(c.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
            if h.is_null() {
                false
            } else {
                libc::dlclose(h);
                true
            }
        }
    })
}

#[cfg(not(target_os = "linux"))]
fn tray_supported() -> bool {
    true
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit VoiceFlow", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&settings, &PredefinedMenuItem::separator(app)?, &quit],
    )?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("VoiceFlow")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "settings" => show_settings(app),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    logging::init();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting VoiceFlow");
    let background = std::env::args().any(|a| a == BACKGROUND_FLAG);

    let app = tauri::Builder::default()
        // Must be first: a second launch only opens Settings in this instance.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_settings(app);
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![BACKGROUND_FLAG]),
        ))
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::dictation_state,
            commands::cancel_dictation,
            commands::shortcut_status,
            commands::copy_transcript,
            commands::overlay_rendered,
            commands::model::model_info,
            commands::model::install_model,
            commands::model::cancel_model_install,
            commands::model::remove_model,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::settings::list_microphones,
            commands::settings::start_on_login_enabled,
            commands::settings::test_microphone,
        ])
        .on_window_event(|window, event| {
            // Closing Settings keeps VoiceFlow running in the background.
            if window.label() == MAIN_WINDOW
                && let WindowEvent::CloseRequested { api, .. } = event
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            let store = config::SettingsStore::new(app.path().app_config_dir()?);
            let first_run = !store.path().exists();
            let settings = store.load();
            if first_run && let Err(e) = store.save(&settings) {
                tracing::warn!(error = %e, "could not write default settings");
            }

            let overlay: Arc<dyn OverlayController> =
                Arc::new(overlay::TauriOverlay::create(&handle)?);

            let emitter = handle.clone();
            let observer_overlay = overlay.clone();
            let observer = move |t: &Transition| {
                // Clicks are only needed for the manual-copy action.
                let interactive = matches!(
                    t.to,
                    StateView::Error {
                        has_transcript: true,
                        ..
                    }
                );
                observer_overlay.set_interactive(interactive);
                let _ = emitter.emit(events::TRANSITION, t);
            };
            let level_emitter = handle.clone();
            let levels: LevelSink = Arc::new(move |level| {
                let _ =
                    level_emitter.emit(events::AUDIO_LEVEL, serde_json::json!({ "level": level }));
            });

            let transcription = Arc::new(transcription::TranscriptionService::spawn(
                transcription::default_engine(),
            ));
            let engine_ready = Arc::new(AtomicBool::new(false));
            let insertion = Arc::new(insertion::InsertionWorker::spawn(Box::new(
                insertion::PasteInsertion::system(insertion::PasteOptions {
                    auto_paste: settings.auto_paste,
                    restore_clipboard: settings.restore_clipboard,
                    ..Default::default()
                }),
            )));
            let live_config = Arc::new(RwLock::new(live::LiveConfig::default()));

            let diagnostic = std::env::var_os("VOICEFLOW_DIAGNOSTIC").is_some();
            let (services, inner): (Option<Arc<live::Services>>, Box<dyn Executor>) = if diagnostic
            {
                tracing::warn!("diagnostic mode: no audio is recorded");
                (None, Box::new(DiagnosticExecutor::with_levels(levels)))
            } else {
                let timing_emitter = handle.clone();
                let services = live::Services::new(
                    live::AudioControl::spawn(Box::new(audio::cpal_capture::CpalCapture::new())),
                    transcription.clone(),
                    insertion.clone(),
                    engine_ready.clone(),
                    live_config.clone(),
                    levels,
                    Arc::new(move |t: &live::SessionTimings| {
                        let _ = timing_emitter.emit(events::TIMINGS, t);
                    }),
                );
                (
                    Some(services.clone()),
                    Box::new(live::LiveExecutor::new(services)),
                )
            };
            let executor = executor::AppExecutor::new(overlay.clone(), inner);
            let (workflow, _task) = tauri::async_runtime::block_on(async {
                let rt = tokio::runtime::Handle::current();
                workflow::controller::spawn(&rt, executor, observer)
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
                insertion,
                live_config,
                services,
                overlay,
                settings: RwLock::new(settings.clone()),
                settings_store: store,
            });
            let models_dir = app.path().app_data_dir()?.join("models");
            let model_store = transcription::model::ModelStore::new(models_dir);
            let model_missing = !matches!(
                model_store.status(&transcription::model::WHISTLE),
                transcription::model::ModelStatus::Installed { .. }
            );
            app.manage(ModelState {
                store: model_store,
                transcription,
                engine_ready,
                install: Mutex::new(None),
            });

            for w in commands::settings::apply_runtime(&handle, None, &settings) {
                tracing::warn!(warning = %w, "setting not applied");
            }
            if !tray_supported() {
                tracing::warn!(
                    "no AppIndicator library (libayatana-appindicator); tray icon disabled. \
                     Launch VoiceFlow again to open Settings."
                );
            } else if let Err(e) = build_tray(&handle) {
                // Some desktops (GNOME without an extension) have no tray.
                tracing::warn!(error = %e, "tray icon unavailable");
            }
            // First run, or nothing works yet: open Settings so the user can
            // install the model.
            if !background && (first_run || model_missing) {
                show_settings(&handle);
            }

            // Load the model in the background so the app starts at once.
            let model_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                commands::model::load_installed_model(&model_handle).await;
            });

            // Registration may wait for a portal dialog, so keep it off the
            // main thread.
            std::thread::spawn(move || {
                let mut s = settings;
                // Development override.
                if let Some(a) = std::env::var("VOICEFLOW_SHORTCUT")
                    .ok()
                    .and_then(|v| v.parse::<Accelerator>().ok())
                {
                    s.shortcut = a;
                }
                if let Err(e) = commands::settings::apply_shortcut(&handle, &s) {
                    tracing::error!(error = %e, "no shortcut backend could be registered");
                }
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
