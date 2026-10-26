//! XDG Desktop Portal GlobalShortcuts backend (Wayland).
//!
//! The portal sends `Activated` when the shortcut is pressed and `Deactivated`
//! when it is released, which gives push-to-talk semantics. The compositor
//! owns the binding: the user may be asked to confirm or change the trigger,
//! and the key is not delivered to the focused application.
//!
//! Support depends on the portal backend. KDE Plasma, GNOME 48+ and Hyprland
//! implement it. Other compositors may expose the interface without a working
//! implementation, in which case `register` fails and the manager falls back.

use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ashpd::desktop::CreateSessionOptions;
use ashpd::desktop::global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut};
use futures_util::StreamExt;
use tokio::sync::oneshot;

use super::{
    Accelerator, BackendKind, KeyState, ShortcutError, ShortcutEvent, ShortcutService, ShortcutSink,
};

const SHORTCUT_ID: &str = "push-to-talk";
/// Must match the bundle identifier and the installed `.desktop` file name.
const APP_ID: &str = "dev.voiceflow.app";
/// The portal may show a confirmation dialog, so allow time for the user.
const BIND_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct PortalBackend {
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl PortalBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

fn map_err(e: ashpd::Error) -> ShortcutError {
    match e {
        ashpd::Error::PortalNotFound(_) => {
            ShortcutError::Unavailable("GlobalShortcuts portal not available".into())
        }
        ashpd::Error::Response(r) => ShortcutError::PermissionDenied(format!("portal request {r}")),
        other => ShortcutError::Unavailable(other.to_string()),
    }
}

async fn run(
    accel: Accelerator,
    sink: ShortcutSink,
    ready: std::sync::mpsc::Sender<Result<String, ShortcutError>>,
    mut stop: oneshot::Receiver<()>,
) {
    let setup = async {
        // Unsandboxed apps must register an app id on the connection before
        // using session portals. Otherwise they get "An app id is required".
        let conn = ashpd::zbus::Connection::session()
            .await
            .map_err(|e| ShortcutError::Unavailable(e.to_string()))?;
        let app_id: ashpd::AppID = APP_ID
            .parse()
            .map_err(|_| ShortcutError::Failed("invalid app id".into()))?;
        if let Err(e) = ashpd::register_host_app_with_connection(conn.clone(), app_id).await {
            // Older portals lack the Registry interface. Carry on and let
            // the GlobalShortcuts call report the real problem.
            tracing::debug!(error = %e, "host app registration failed");
        }
        let portal = GlobalShortcuts::with_connection(conn).await.map_err(map_err)?;
        let session = portal
            .create_session(CreateSessionOptions::default())
            .await
            .map_err(map_err)?;
        let trigger = accel.to_portal_trigger();
        let shortcut =
            NewShortcut::new(SHORTCUT_ID, "Push to talk").preferred_trigger(trigger.as_str());
        let request = portal
            .bind_shortcuts(&session, &[shortcut], None, BindShortcutsOptions::default())
            .await
            .map_err(map_err)?;
        let bound = request.response().map_err(map_err)?;
        let assigned = bound
            .shortcuts()
            .iter()
            .find(|s| s.id() == SHORTCUT_ID)
            .map(|s| s.trigger_description().to_owned())
            .ok_or_else(|| ShortcutError::PermissionDenied("shortcut was not bound".into()))?;
        let activated = portal.receive_activated().await.map_err(map_err)?;
        let deactivated = portal.receive_deactivated().await.map_err(map_err)?;
        Ok::<_, ShortcutError>((portal, session, assigned, activated, deactivated))
    };

    // Stop also aborts a setup that is still waiting on the portal.
    let result = tokio::select! {
        r = setup => r,
        _ = &mut stop => return,
    };
    let (_portal, session, assigned, activated, deactivated) = match result {
        Ok(v) => v,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let _ = ready.send(Ok(assigned));

    let mut activated = Box::pin(activated);
    let mut deactivated = Box::pin(deactivated);
    loop {
        tokio::select! {
            _ = &mut stop => break,
            Some(ev) = activated.next() => if ev.shortcut_id() == SHORTCUT_ID {
                sink(ShortcutEvent { state: KeyState::Pressed, at: Instant::now() });
            },
            Some(ev) = deactivated.next() => if ev.shortcut_id() == SHORTCUT_ID {
                sink(ShortcutEvent { state: KeyState::Released, at: Instant::now() });
            },
            else => break,
        }
    }
    if let Err(e) = session.close().await {
        tracing::debug!(error = %e, "closing portal session failed");
    }
}

impl ShortcutService for PortalBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Portal
    }

    fn register(&mut self, accel: Accelerator, sink: ShortcutSink) -> Result<(), ShortcutError> {
        self.unregister();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (stop_tx, stop_rx) = oneshot::channel();
        let thread = std::thread::Builder::new()
            .name("shortcut-portal".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = ready_tx.send(Err(ShortcutError::Failed(e.to_string())));
                        return;
                    }
                };
                rt.block_on(run(accel, sink, ready_tx, stop_rx));
            })
            .map_err(|e| ShortcutError::Failed(e.to_string()))?;
        self.stop = Some(stop_tx);
        self.thread = Some(thread);

        match ready_rx.recv_timeout(BIND_TIMEOUT) {
            Ok(Ok(assigned)) => {
                tracing::info!(trigger = %assigned, "portal shortcut bound");
                Ok(())
            }
            Ok(Err(e)) => {
                self.unregister();
                Err(e)
            }
            Err(_) => {
                self.unregister();
                Err(ShortcutError::Unavailable(
                    "GlobalShortcuts portal did not respond".into(),
                ))
            }
        }
    }

    fn unregister(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            // Stop makes the thread exit promptly. Closing the session is a
            // D-Bus call, so wait at most a second rather than hang shutdown.
            let deadline = Instant::now() + Duration::from_secs(1);
            while !thread.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if thread.is_finished() {
                let _ = thread.join();
            } else {
                tracing::warn!("portal shortcut thread did not stop in time");
            }
        }
    }
}

impl Drop for PortalBackend {
    fn drop(&mut self) {
        self.unregister();
    }
}
