//! Global push-to-talk shortcut.
//!
//! Backends:
//! - [`hotkey::HotkeyBackend`]: Windows, macOS and X11, through the Tauri
//!   global-shortcut plugin (global-hotkey). It reports press and release.
//! - [`portal::PortalBackend`] (Linux): the XDG GlobalShortcuts portal for Wayland.
//!   `Activated`/`Deactivated` map to press and release.
//! - [`evdev::EvdevBackend`] (Linux): reads keyboards from `/dev/input`. It works on
//!   any compositor but needs the `input` group, and does not consume the
//!   keystroke.

pub mod accelerator;
#[cfg(target_os = "linux")]
pub mod evdev;
pub mod hotkey;
#[cfg(target_os = "linux")]
pub mod portal;

use std::sync::{Arc, Mutex};
use std::time::Instant;

pub use accelerator::{Accelerator, AcceleratorError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Pressed,
    Released,
}

#[derive(Debug, Clone, Copy)]
pub struct ShortcutEvent {
    pub state: KeyState,
    /// When the backend observed the event, for latency measurement.
    pub at: Instant,
}

pub type ShortcutSink = Arc<dyn Fn(ShortcutEvent) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackendKind {
    GlobalHotkey,
    Portal,
    Evdev,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackendPreference {
    #[default]
    Auto,
    GlobalHotkey,
    Portal,
    Evdev,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShortcutError {
    #[error("{0} is already in use by another application")]
    Conflict(String),
    #[error("shortcut backend unavailable: {0}")]
    Unavailable(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("shortcut registration failed: {0}")]
    Failed(String),
}

/// A native mechanism that reports press and release of one accelerator.
pub trait ShortcutService: Send {
    fn kind(&self) -> BackendKind;
    /// Registers `accel`, replacing any previous registration of this backend.
    /// Blocks until the backend confirms. Do not call from an async context.
    fn register(&mut self, accel: Accelerator, sink: ShortcutSink) -> Result<(), ShortcutError>;
    /// Releases the registration. Must be idempotent.
    fn unregister(&mut self);
}

/// Turns raw backend notifications into strictly alternating press/release
/// pairs. Removes key-repeat presses and stray releases.
#[derive(Debug, Default)]
pub struct PressFilter {
    down: bool,
}

impl PressFilter {
    pub fn accept(&mut self, state: KeyState) -> bool {
        match (self.down, state) {
            (false, KeyState::Pressed) => {
                self.down = true;
                true
            }
            (true, KeyState::Released) => {
                self.down = false;
                true
            }
            _ => false,
        }
    }
}

/// Wraps a sink so that it only sees filtered, alternating events.
pub fn filtered(sink: ShortcutSink) -> ShortcutSink {
    let filter = Mutex::new(PressFilter::default());
    Arc::new(move |ev: ShortcutEvent| {
        let pass = filter
            .lock()
            .map(|mut f| f.accept(ev.state))
            .unwrap_or(false);
        if pass {
            sink(ev);
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionKind {
    Wayland,
    X11,
    Other,
}

pub fn detect_session() -> SessionKind {
    if cfg!(target_os = "linux") {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v == "wayland");
        if wayland {
            SessionKind::Wayland
        } else {
            SessionKind::X11
        }
    } else {
        SessionKind::Other
    }
}

/// Backends to try, most preferred first.
pub fn candidate_backends(pref: BackendPreference, session: SessionKind) -> Vec<BackendKind> {
    use BackendKind::*;
    match pref {
        BackendPreference::GlobalHotkey => vec![GlobalHotkey],
        BackendPreference::Portal => vec![Portal],
        BackendPreference::Evdev => vec![Evdev],
        // On Wayland, global-hotkey would only grab keys through XWayland and
        // stay silent for native windows, so it is not a candidate there.
        BackendPreference::Auto => match session {
            SessionKind::Wayland => vec![Portal, Evdev],
            SessionKind::X11 => vec![GlobalHotkey, Evdev],
            SessionKind::Other => vec![GlobalHotkey],
        },
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutStatus {
    pub accelerator: Option<String>,
    pub backend: Option<BackendKind>,
    pub session: SessionKind,
    pub error: Option<String>,
}

pub type BackendFactory = Box<dyn Fn(BackendKind) -> Option<Box<dyn ShortcutService>> + Send>;

/// Owns the active backend and the current accelerator.
pub struct ShortcutManager {
    factory: BackendFactory,
    active: Option<(Box<dyn ShortcutService>, Accelerator)>,
    sink: ShortcutSink,
    session: SessionKind,
    last_error: Option<String>,
}

impl ShortcutManager {
    pub fn new(factory: BackendFactory, sink: ShortcutSink, session: SessionKind) -> Self {
        Self {
            factory,
            active: None,
            sink,
            session,
            last_error: None,
        }
    }

    /// Registers `accel` with the first working backend. If every backend
    /// fails, the previous registration is restored and the error returned.
    pub fn register(
        &mut self,
        accel: Accelerator,
        pref: BackendPreference,
    ) -> Result<BackendKind, ShortcutError> {
        let previous = self.active.take();
        let previous_accel = previous.as_ref().map(|(_, a)| *a);
        if let Some((mut backend, _)) = previous {
            backend.unregister();
        }

        let mut last_err =
            ShortcutError::Unavailable("no shortcut backend for this platform".into());
        for kind in candidate_backends(pref, self.session) {
            let Some(mut backend) = (self.factory)(kind) else {
                continue;
            };
            match backend.register(accel, filtered(self.sink.clone())) {
                Ok(()) => {
                    tracing::info!(backend = ?kind, shortcut = %accel, "shortcut registered");
                    self.active = Some((backend, accel));
                    self.last_error = None;
                    return Ok(kind);
                }
                Err(e) => {
                    tracing::warn!(backend = ?kind, error = %e, "shortcut backend failed");
                    backend.unregister();
                    last_err = e;
                }
            }
        }

        self.last_error = Some(last_err.to_string());
        if let Some(prev) = previous_accel.filter(|p| *p != accel) {
            // Keep the user's previous shortcut working after a failed change.
            if let Ok(kind) = self.register_inner(prev, pref) {
                tracing::info!(backend = ?kind, "restored previous shortcut");
            }
        }
        Err(last_err)
    }

    fn register_inner(
        &mut self,
        accel: Accelerator,
        pref: BackendPreference,
    ) -> Result<BackendKind, ShortcutError> {
        let err = self.last_error.clone();
        let r = self.register(accel, pref);
        // Report the error of the requested change, not of the restore.
        self.last_error = err;
        r
    }

    pub fn unregister(&mut self) {
        if let Some((mut backend, _)) = self.active.take() {
            backend.unregister();
        }
    }

    pub fn status(&self) -> ShortcutStatus {
        ShortcutStatus {
            accelerator: self.active.as_ref().map(|(_, a)| a.to_string()),
            backend: self.active.as_ref().map(|(b, _)| b.kind()),
            session: self.session,
            error: self.last_error.clone(),
        }
    }
}

impl Drop for ShortcutManager {
    fn drop(&mut self) {
        self.unregister();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn press_filter_dedupes() {
        let mut f = PressFilter::default();
        assert!(!f.accept(KeyState::Released));
        assert!(f.accept(KeyState::Pressed));
        assert!(!f.accept(KeyState::Pressed));
        assert!(!f.accept(KeyState::Pressed));
        assert!(f.accept(KeyState::Released));
        assert!(!f.accept(KeyState::Released));
        assert!(f.accept(KeyState::Pressed));
    }

    #[test]
    fn auto_candidates_per_session() {
        use BackendKind::*;
        let auto = BackendPreference::Auto;
        assert_eq!(
            candidate_backends(auto, SessionKind::Wayland),
            vec![Portal, Evdev]
        );
        assert_eq!(
            candidate_backends(auto, SessionKind::X11),
            vec![GlobalHotkey, Evdev]
        );
        assert_eq!(
            candidate_backends(auto, SessionKind::Other),
            vec![GlobalHotkey]
        );
        assert_eq!(
            candidate_backends(BackendPreference::Evdev, SessionKind::Other),
            vec![Evdev]
        );
    }

    /// A fake backend. It fails for accelerators listed in `taken` and counts
    /// registrations.
    struct Fake {
        kind: BackendKind,
        taken: Vec<Accelerator>,
        live: Arc<AtomicUsize>,
        registered: bool,
    }

    impl ShortcutService for Fake {
        fn kind(&self) -> BackendKind {
            self.kind
        }
        fn register(&mut self, a: Accelerator, sink: ShortcutSink) -> Result<(), ShortcutError> {
            if self.taken.contains(&a) {
                return Err(ShortcutError::Conflict(a.to_string()));
            }
            // Emit a repeated press and a release to exercise the filter.
            for s in [KeyState::Pressed, KeyState::Pressed, KeyState::Released] {
                sink(ShortcutEvent {
                    state: s,
                    at: Instant::now(),
                });
            }
            self.registered = true;
            self.live.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn unregister(&mut self) {
            if std::mem::take(&mut self.registered) {
                self.live.fetch_sub(1, Ordering::SeqCst);
            }
        }
    }

    fn manager(
        taken_by: Vec<(BackendKind, Vec<Accelerator>)>,
    ) -> (ShortcutManager, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let live = Arc::new(AtomicUsize::new(0));
        let events = Arc::new(AtomicUsize::new(0));
        let live_f = live.clone();
        let factory: BackendFactory = Box::new(move |kind| {
            let taken = taken_by
                .iter()
                .find(|(k, _)| *k == kind)
                .map(|(_, t)| t.clone())
                .unwrap_or_default();
            Some(Box::new(Fake {
                kind,
                taken,
                live: live_f.clone(),
                registered: false,
            }))
        });
        let ev = events.clone();
        let sink: ShortcutSink = Arc::new(move |_| {
            ev.fetch_add(1, Ordering::SeqCst);
        });
        (
            ShortcutManager::new(factory, sink, SessionKind::X11),
            live,
            events,
        )
    }

    #[test]
    fn registers_first_working_backend_and_filters_events() {
        let (mut m, live, events) = manager(vec![]);
        let kind = m
            .register(Accelerator::default(), BackendPreference::Auto)
            .unwrap();
        assert_eq!(kind, BackendKind::GlobalHotkey);
        assert_eq!(live.load(Ordering::SeqCst), 1);
        // Pressed, Pressed (dropped), Released.
        assert_eq!(events.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn falls_back_on_conflict() {
        let ctrl_space = Accelerator::default();
        let (mut m, _, _) = manager(vec![(BackendKind::GlobalHotkey, vec![ctrl_space])]);
        assert_eq!(
            m.register(ctrl_space, BackendPreference::Auto).unwrap(),
            BackendKind::Evdev
        );
    }

    #[test]
    fn failed_change_restores_previous_shortcut() {
        let taken: Accelerator = "Ctrl+Alt+T".parse().unwrap();
        let (mut m, live, _) = manager(vec![
            (BackendKind::GlobalHotkey, vec![taken]),
            (BackendKind::Evdev, vec![taken]),
        ]);
        m.register(Accelerator::default(), BackendPreference::Auto)
            .unwrap();
        let err = m.register(taken, BackendPreference::Auto).unwrap_err();
        assert!(matches!(err, ShortcutError::Conflict(_)));
        let status = m.status();
        assert_eq!(status.accelerator.as_deref(), Some("Ctrl+Space"));
        assert!(status.error.unwrap().contains("Ctrl+Alt+T"));
        assert_eq!(
            live.load(Ordering::SeqCst),
            1,
            "exactly one live registration"
        );
    }

    #[test]
    fn re_registering_does_not_leak_registrations() {
        let (mut m, live, _) = manager(vec![]);
        for _ in 0..5 {
            m.register(Accelerator::default(), BackendPreference::Auto)
                .unwrap();
        }
        assert_eq!(live.load(Ordering::SeqCst), 1);
        drop(m);
        assert_eq!(live.load(Ordering::SeqCst), 0, "drop unregisters");
    }
}
