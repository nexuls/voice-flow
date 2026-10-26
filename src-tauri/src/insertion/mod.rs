//! Text insertion into the application that was focused when recording began.
//!
//! Strategy ([`PasteInsertion`]):
//! 1. At recording start, remember the focused window ([`focus`]).
//! 2. After transcription, check that the same window is still focused. If
//!    not, leave the text on the clipboard and do not paste.
//! 3. Save the current clipboard text, put the transcript on the clipboard
//!    and send the paste chord ([`keys`]).
//! 4. After a short delay, restore the saved text, but only if the clipboard
//!    still holds our transcript (another app or the user may have changed it).
//!
//! If no paste mechanism works, the transcript stays on the clipboard and the
//! outcome is `CopiedToClipboard`, never `Pasted`.

pub mod focus;
pub mod keys;

use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::sync::oneshot;
use zeroize::Zeroize;

use crate::workflow::InsertOutcome;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InsertError {
    #[error("clipboard unavailable: {0}")]
    Clipboard(String),
    #[error("automatic paste is not available: {0}")]
    PasteUnavailable(String),
    #[error("permission required: {0}")]
    PermissionDenied(String),
    #[error("focus changed during transcription")]
    FocusChanged,
    #[error("insertion service stopped")]
    Stopped,
}

/// Where text should go, captured when recording starts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InsertTarget {
    /// Opaque platform window identifier, if the platform exposes one.
    pub window: Option<String>,
}

pub trait TextInsertionService: Send + 'static {
    /// Records the current focus target. Called when recording starts.
    fn capture_target(&mut self) -> InsertTarget;
    /// Inserts `text` into `target`. Reports what was actually done.
    fn insert(&mut self, text: &str, target: &InsertTarget) -> Result<InsertOutcome, InsertError>;
    /// Copies `text` to the clipboard (manual fallback).
    fn copy(&mut self, text: &str) -> Result<(), InsertError>;
    /// Follow-up work after the insert result was reported, such as restoring
    /// the clipboard. May block briefly.
    fn after_insert(&mut self) {}
    /// Prepares slow resources (input devices, connections) ahead of the first insert.
    fn warm_up(&mut self) {}
}

/// Abstraction over the system clipboard, so the restore logic can be tested.
pub trait ClipboardAccess: Send {
    fn get_text(&mut self) -> Option<String>;
    fn set_text(&mut self, text: &str) -> Result<(), InsertError>;
}

/// The system clipboard via arboard. On Linux, arboard keeps serving our
/// content in a background thread as long as this object lives.
#[derive(Default)]
pub struct SystemClipboard(Option<arboard::Clipboard>);

impl SystemClipboard {
    fn inner(&mut self) -> Result<&mut arboard::Clipboard, InsertError> {
        if self.0.is_none() {
            self.0 =
                Some(arboard::Clipboard::new().map_err(|e| InsertError::Clipboard(e.to_string()))?);
        }
        Ok(self.0.as_mut().expect("just set"))
    }
}

impl ClipboardAccess for SystemClipboard {
    fn get_text(&mut self) -> Option<String> {
        self.inner().ok()?.get_text().ok()
    }
    fn set_text(&mut self, text: &str) -> Result<(), InsertError> {
        self.inner()?
            .set_text(text.to_owned())
            .map_err(|e| InsertError::Clipboard(e.to_string()))
    }
}

#[derive(Debug, Clone)]
pub struct PasteOptions {
    /// Send the paste keystroke. When false, transcripts are only copied.
    pub auto_paste: bool,
    pub restore_clipboard: bool,
    /// Time the target application gets to read the clipboard before restore.
    pub restore_delay: Duration,
}

impl Default for PasteOptions {
    fn default() -> Self {
        Self {
            auto_paste: true,
            restore_clipboard: true,
            restore_delay: Duration::from_millis(600),
        }
    }
}

struct PendingRestore {
    previous: String,
    ours: String,
}

impl Drop for PendingRestore {
    fn drop(&mut self) {
        self.previous.zeroize();
        self.ours.zeroize();
    }
}

/// Clipboard + paste keystroke insertion. See the module docs.
pub struct PasteInsertion {
    clipboard: Box<dyn ClipboardAccess>,
    focus: Box<dyn focus::FocusProvider>,
    keys: Option<Box<dyn keys::Keystroker>>,
    /// Lazily creates a keystroke backend (also used for retries).
    make_keys: Box<dyn FnMut() -> Result<Box<dyn keys::Keystroker>, InsertError> + Send>,
    options: PasteOptions,
    pending: Option<PendingRestore>,
    /// Focus at capture time, kept for the paste chord choice.
    target_focus: Option<focus::FocusInfo>,
}

impl PasteInsertion {
    pub fn new(
        clipboard: Box<dyn ClipboardAccess>,
        focus: Box<dyn focus::FocusProvider>,
        make_keys: Box<dyn FnMut() -> Result<Box<dyn keys::Keystroker>, InsertError> + Send>,
        options: PasteOptions,
    ) -> Self {
        Self {
            clipboard,
            focus,
            keys: None,
            make_keys,
            options,
            pending: None,
            target_focus: None,
        }
    }

    /// The platform default: system clipboard, detected focus provider and
    /// keystroke backend.
    pub fn system(options: PasteOptions) -> Self {
        let focus = focus::detect();
        tracing::info!(focus = focus.name(), "insertion focus provider");
        Self::new(
            Box::new(SystemClipboard::default()),
            focus,
            Box::new(keys::detect),
            options,
        )
    }

    fn keys(&mut self) -> Result<&mut Box<dyn keys::Keystroker>, InsertError> {
        if self.keys.is_none() {
            let k = (self.make_keys)()?;
            tracing::info!(backend = k.name(), "paste keystroke backend ready");
            self.keys = Some(k);
        }
        Ok(self.keys.as_mut().expect("just set"))
    }

    fn send_paste(&mut self, chord: keys::PasteChord) -> Result<(), InsertError> {
        match self.keys().and_then(|k| k.paste(chord)) {
            Ok(()) => Ok(()),
            Err(e) => {
                // The backend may have gone stale (compositor restart, device
                // removed). Recreate it once and retry.
                tracing::warn!(error = %e, "paste failed; retrying with a fresh backend");
                self.keys = None;
                self.keys()?.paste(chord)
            }
        }
    }
}

impl TextInsertionService for PasteInsertion {
    fn capture_target(&mut self) -> InsertTarget {
        self.target_focus = self.focus.focused();
        InsertTarget {
            window: self.target_focus.as_ref().map(|f| f.id.clone()),
        }
    }

    fn insert(&mut self, text: &str, target: &InsertTarget) -> Result<InsertOutcome, InsertError> {
        // A restore from the previous dictation must not clobber this one.
        self.pending = None;
        let now = self.focus.focused();
        let focus_changed = match (&target.window, &now) {
            (Some(before), Some(after)) => *before != after.id,
            // Focus moved to nothing we can identify (for example the desktop).
            (Some(_), None) => true,
            (None, _) => false,
        };

        if !self.options.auto_paste {
            self.clipboard.set_text(text)?;
            return Ok(InsertOutcome::CopiedToClipboard);
        }
        let previous = if self.options.restore_clipboard {
            self.clipboard.get_text()
        } else {
            None
        };
        self.clipboard.set_text(text)?;

        if focus_changed {
            tracing::info!("focus changed during transcription; not pasting");
            return Ok(InsertOutcome::CopiedToClipboard);
        }
        let app_id = now.as_ref().and_then(|f| f.app_id.as_deref());
        let chord = keys::paste_chord(app_id);
        match self.send_paste(chord) {
            Ok(()) => {
                if let Some(previous) = previous.filter(|p| p != text) {
                    self.pending = Some(PendingRestore {
                        previous,
                        ours: text.to_owned(),
                    });
                }
                Ok(InsertOutcome::Pasted)
            }
            Err(e) => {
                // The transcript stays on the clipboard for manual pasting.
                tracing::warn!(error = %e, "automatic paste unavailable");
                Ok(InsertOutcome::CopiedToClipboard)
            }
        }
    }

    fn copy(&mut self, text: &str) -> Result<(), InsertError> {
        self.pending = None;
        self.clipboard.set_text(text)
    }

    fn warm_up(&mut self) {
        if !self.options.auto_paste {
            return;
        }
        if let Err(e) = self.keys() {
            tracing::warn!(error = %e, "no paste keystroke backend; transcripts will be copied only");
        }
    }

    fn after_insert(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        std::thread::sleep(self.options.restore_delay);
        // Restore only if nobody replaced the clipboard in the meantime.
        match self.clipboard.get_text() {
            Some(current) if current == pending.ours => {
                if let Err(e) = self.clipboard.set_text(&pending.previous) {
                    tracing::warn!(error = %e, "clipboard restore failed");
                }
            }
            _ => tracing::debug!("clipboard changed after paste; not restoring"),
        }
    }
}

enum Job {
    Capture(oneshot::Sender<InsertTarget>),
    Insert {
        text: String,
        target: InsertTarget,
        reply: oneshot::Sender<Result<InsertOutcome, InsertError>>,
    },
    Copy {
        text: String,
        reply: oneshot::Sender<Result<(), InsertError>>,
    },
}

/// Runs a [`TextInsertionService`] on its own thread. Clipboard and input
/// APIs may block, and on Linux the clipboard owner must stay alive to
/// serve the content.
pub struct InsertionWorker {
    tx: Option<mpsc::Sender<Job>>,
    thread: Option<JoinHandle<()>>,
}

impl InsertionWorker {
    pub fn spawn(mut service: Box<dyn TextInsertionService>) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let thread = std::thread::Builder::new()
            .name("insertion".into())
            .spawn(move || {
                service.warm_up();
                for job in rx {
                    match job {
                        Job::Capture(reply) => {
                            let _ = reply.send(service.capture_target());
                        }
                        Job::Insert {
                            mut text,
                            target,
                            reply,
                        } => {
                            let _ = reply.send(service.insert(&text, &target));
                            text.zeroize();
                            service.after_insert();
                        }
                        Job::Copy { mut text, reply } => {
                            let _ = reply.send(service.copy(&text));
                            text.zeroize();
                        }
                    }
                }
            })
            .expect("spawn insertion thread");
        Self {
            tx: Some(tx),
            thread: Some(thread),
        }
    }

    pub async fn capture_target(&self) -> InsertTarget {
        let (reply, rx) = oneshot::channel();
        match &self.tx {
            Some(tx) if tx.send(Job::Capture(reply)).is_ok() => rx.await.unwrap_or_default(),
            _ => InsertTarget::default(),
        }
    }

    pub async fn insert(
        &self,
        text: String,
        target: InsertTarget,
    ) -> Result<InsertOutcome, InsertError> {
        let (reply, rx) = oneshot::channel();
        let tx = self.tx.as_ref().ok_or(InsertError::Stopped)?;
        tx.send(Job::Insert {
            text,
            target,
            reply,
        })
        .map_err(|_| InsertError::Stopped)?;
        rx.await.unwrap_or(Err(InsertError::Stopped))
    }

    pub async fn copy(&self, text: String) -> Result<(), InsertError> {
        let (reply, rx) = oneshot::channel();
        let tx = self.tx.as_ref().ok_or(InsertError::Stopped)?;
        tx.send(Job::Copy { text, reply })
            .map_err(|_| InsertError::Stopped)?;
        rx.await.unwrap_or(Err(InsertError::Stopped))
    }
}

impl Drop for InsertionWorker {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Clone, Default)]
    struct FakeClipboard {
        text: Arc<Mutex<Option<String>>>,
        fail: bool,
    }

    impl ClipboardAccess for FakeClipboard {
        fn get_text(&mut self) -> Option<String> {
            self.text.lock().unwrap().clone()
        }
        fn set_text(&mut self, text: &str) -> Result<(), InsertError> {
            if self.fail {
                return Err(InsertError::Clipboard("denied".into()));
            }
            *self.text.lock().unwrap() = Some(text.to_owned());
            Ok(())
        }
    }

    #[derive(Clone, Default)]
    struct FakeFocus(Arc<Mutex<Option<focus::FocusInfo>>>);

    impl FakeFocus {
        fn set(&self, id: &str, app: &str) {
            *self.0.lock().unwrap() = Some(focus::FocusInfo {
                id: id.into(),
                app_id: Some(app.into()),
            });
        }
    }

    impl focus::FocusProvider for FakeFocus {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn focused(&mut self) -> Option<focus::FocusInfo> {
            self.0.lock().unwrap().clone()
        }
    }

    #[derive(Clone, Default)]
    struct FakeKeys {
        sent: Arc<Mutex<Vec<keys::PasteChord>>>,
        fail: Arc<Mutex<u32>>,
        /// Clipboard content at the moment of the paste.
        seen: Arc<Mutex<Vec<Option<String>>>>,
        clipboard: Option<FakeClipboard>,
    }

    impl keys::Keystroker for FakeKeys {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn paste(&mut self, chord: keys::PasteChord) -> Result<(), InsertError> {
            let mut fail = self.fail.lock().unwrap();
            if *fail > 0 {
                *fail -= 1;
                return Err(InsertError::PasteUnavailable("no keyboard".into()));
            }
            if let Some(c) = &self.clipboard {
                self.seen
                    .lock()
                    .unwrap()
                    .push(c.text.lock().unwrap().clone());
            }
            self.sent.lock().unwrap().push(chord);
            Ok(())
        }
    }

    struct Rig {
        ins: PasteInsertion,
        clip: FakeClipboard,
        focus: FakeFocus,
        keys: FakeKeys,
    }

    fn rig(previous: Option<&str>, fail_keys: u32, make_fails: bool) -> Rig {
        let clip = FakeClipboard::default();
        *clip.text.lock().unwrap() = previous.map(str::to_owned);
        let focus = FakeFocus::default();
        focus.set("w1", "gedit");
        let keys = FakeKeys {
            clipboard: Some(clip.clone()),
            ..Default::default()
        };
        *keys.fail.lock().unwrap() = fail_keys;
        let k = keys.clone();
        let ins = PasteInsertion::new(
            Box::new(clip.clone()),
            Box::new(focus.clone()),
            Box::new(move || {
                if make_fails {
                    Err(InsertError::PasteUnavailable("none".into()))
                } else {
                    Ok(Box::new(k.clone()) as Box<dyn keys::Keystroker>)
                }
            }),
            PasteOptions {
                restore_clipboard: true,
                restore_delay: Duration::from_millis(1),
                ..Default::default()
            },
        );
        Rig {
            ins,
            clip,
            focus,
            keys,
        }
    }

    fn clip_text(r: &Rig) -> Option<String> {
        r.clip.text.lock().unwrap().clone()
    }

    #[test]
    fn pastes_and_restores_previous_clipboard() {
        let mut r = rig(Some("user data"), 0, false);
        let t = r.ins.capture_target();
        assert_eq!(r.ins.insert("hello\nworld", &t), Ok(InsertOutcome::Pasted));
        assert_eq!(*r.keys.seen.lock().unwrap(), [Some("hello\nworld".into())]);
        assert_eq!(
            *r.keys.sent.lock().unwrap(),
            [keys::PasteChord::CtrlV.for_platform()]
        );
        r.ins.after_insert();
        assert_eq!(clip_text(&r).as_deref(), Some("user data"));
    }

    #[test]
    fn does_not_restore_if_clipboard_changed_meanwhile() {
        let mut r = rig(Some("user data"), 0, false);
        let t = r.ins.capture_target();
        r.ins.insert("hello", &t).unwrap();
        *r.clip.text.lock().unwrap() = Some("copied by user".into());
        r.ins.after_insert();
        assert_eq!(clip_text(&r).as_deref(), Some("copied by user"));
    }

    #[test]
    fn empty_or_non_text_clipboard_keeps_transcript() {
        let mut r = rig(None, 0, false);
        let t = r.ins.capture_target();
        assert_eq!(r.ins.insert("hello", &t), Ok(InsertOutcome::Pasted));
        r.ins.after_insert();
        assert_eq!(clip_text(&r).as_deref(), Some("hello"));
    }

    #[test]
    fn focus_change_copies_without_pasting() {
        let mut r = rig(Some("user data"), 0, false);
        let t = r.ins.capture_target();
        r.focus.set("w2", "firefox");
        assert_eq!(
            r.ins.insert("hello", &t),
            Ok(InsertOutcome::CopiedToClipboard)
        );
        assert!(r.keys.sent.lock().unwrap().is_empty());
        r.ins.after_insert();
        assert_eq!(
            clip_text(&r).as_deref(),
            Some("hello"),
            "kept for manual paste"
        );
    }

    #[test]
    fn unavailable_paste_falls_back_to_clipboard() {
        let mut r = rig(Some("user data"), 0, true);
        let t = r.ins.capture_target();
        assert_eq!(
            r.ins.insert("hello", &t),
            Ok(InsertOutcome::CopiedToClipboard)
        );
        r.ins.after_insert();
        assert_eq!(clip_text(&r).as_deref(), Some("hello"));
    }

    #[test]
    fn transient_paste_failure_is_retried_once() {
        let mut r = rig(None, 1, false);
        let t = r.ins.capture_target();
        assert_eq!(r.ins.insert("hello", &t), Ok(InsertOutcome::Pasted));
        let mut r = rig(None, 2, false);
        let t = r.ins.capture_target();
        assert_eq!(
            r.ins.insert("hello", &t),
            Ok(InsertOutcome::CopiedToClipboard)
        );
    }

    #[test]
    fn clipboard_failure_is_an_error() {
        let clip = FakeClipboard {
            fail: true,
            ..Default::default()
        };
        let mut ins = PasteInsertion::new(
            Box::new(clip),
            Box::new(FakeFocus::default()),
            Box::new(|| Err(InsertError::PasteUnavailable("x".into()))),
            PasteOptions::default(),
        );
        let t = ins.capture_target();
        assert!(matches!(
            ins.insert("hello", &t),
            Err(InsertError::Clipboard(_))
        ));
    }

    #[test]
    fn terminal_gets_ctrl_shift_v_on_linux() {
        let mut r = rig(None, 0, false);
        r.focus.set("w1", "kitty");
        let t = r.ins.capture_target();
        r.ins.insert("ls", &t).unwrap();
        let expected = if cfg!(target_os = "linux") {
            keys::PasteChord::CtrlShiftV
        } else {
            keys::PasteChord::CtrlV.for_platform()
        };
        assert_eq!(*r.keys.sent.lock().unwrap(), [expected]);
    }

    #[test]
    fn copy_only_mode_never_pastes() {
        let mut r = rig(Some("user data"), 0, false);
        r.ins.options.auto_paste = false;
        let t = r.ins.capture_target();
        assert_eq!(
            r.ins.insert("hello", &t),
            Ok(InsertOutcome::CopiedToClipboard)
        );
        assert!(r.keys.sent.lock().unwrap().is_empty());
        r.ins.after_insert();
        assert_eq!(clip_text(&r).as_deref(), Some("hello"));
    }

    #[test]
    fn unknown_focus_still_pastes() {
        let mut r = rig(None, 0, false);
        *r.focus.0.lock().unwrap() = None;
        let t = r.ins.capture_target();
        assert_eq!(t.window, None);
        assert_eq!(r.ins.insert("hello", &t), Ok(InsertOutcome::Pasted));
    }

    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<String>>>);

    impl TextInsertionService for Recorder {
        fn capture_target(&mut self) -> InsertTarget {
            InsertTarget {
                window: Some("w1".into()),
            }
        }
        fn insert(&mut self, text: &str, t: &InsertTarget) -> Result<InsertOutcome, InsertError> {
            self.0
                .lock()
                .unwrap()
                .push(format!("{}:{text}", t.window.as_deref().unwrap_or("")));
            Ok(InsertOutcome::Pasted)
        }
        fn copy(&mut self, text: &str) -> Result<(), InsertError> {
            self.0.lock().unwrap().push(format!("copy:{text}"));
            Ok(())
        }
    }

    #[tokio::test]
    async fn worker_runs_jobs_in_order() {
        let rec = Recorder::default();
        let worker = InsertionWorker::spawn(Box::new(rec.clone()));
        let target = worker.capture_target().await;
        assert_eq!(
            worker.insert("a\nb".into(), target).await,
            Ok(InsertOutcome::Pasted)
        );
        worker.copy("c".into()).await.unwrap();
        assert_eq!(*rec.0.lock().unwrap(), ["w1:a\nb", "copy:c"]);
    }
}
