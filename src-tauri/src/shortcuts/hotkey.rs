//! Global shortcut backend built on the Tauri global-shortcut plugin.
//!
//! Release semantics, verified against global-hotkey 0.8 sources:
//! - X11: XGrabKey with XKB detectable auto-repeat. Release comes from KeyRelease.
//! - Windows: RegisterHotKey for press. Release is polled with
//!   `GetAsyncKeyState` every 50 ms, which adds up to 50 ms of release latency.
//! - macOS: Carbon `kEventHotKeyReleased`.
//!
//! On all three, release fires when the trigger key (not the modifier) is released.

use std::time::Instant;

use tauri::{AppHandle, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use super::{
    Accelerator, BackendKind, KeyState, ShortcutError, ShortcutEvent, ShortcutService, ShortcutSink,
};

pub struct HotkeyBackend<R: Runtime> {
    app: AppHandle<R>,
    registered: Option<Shortcut>,
}

impl<R: Runtime> HotkeyBackend<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self {
            app,
            registered: None,
        }
    }
}

fn map_error(accel: Accelerator, e: tauri_plugin_global_shortcut::Error) -> ShortcutError {
    let msg = e.to_string();
    let lower = msg.to_ascii_lowercase();
    if lower.contains("already") || lower.contains("in use") || lower.contains("badaccess") {
        ShortcutError::Conflict(accel.to_string())
    } else {
        ShortcutError::Failed(msg)
    }
}

impl<R: Runtime> ShortcutService for HotkeyBackend<R> {
    fn kind(&self) -> BackendKind {
        BackendKind::GlobalHotkey
    }

    fn register(&mut self, accel: Accelerator, sink: ShortcutSink) -> Result<(), ShortcutError> {
        self.unregister();
        let shortcut = accel.to_hotkey();
        let gs = self.app.global_shortcut();
        // Another part of this app (or a previous crashed registration) may
        // hold the same shortcut. Treat that as a conflict, not as success.
        if gs.is_registered(shortcut) {
            return Err(ShortcutError::Conflict(accel.to_string()));
        }
        gs.on_shortcut(shortcut, move |_app, _shortcut, event| {
            let state = match event.state {
                ShortcutState::Pressed => KeyState::Pressed,
                ShortcutState::Released => KeyState::Released,
            };
            sink(ShortcutEvent {
                state,
                at: Instant::now(),
            });
        })
        .map_err(|e| map_error(accel, e))?;
        self.registered = Some(shortcut);
        Ok(())
    }

    fn unregister(&mut self) {
        if let Some(shortcut) = self.registered.take()
            && let Err(e) = self.app.global_shortcut().unregister(shortcut)
        {
            tracing::warn!(error = %e, "failed to unregister global shortcut");
        }
    }
}

impl<R: Runtime> Drop for HotkeyBackend<R> {
    fn drop(&mut self) {
        self.unregister();
    }
}
