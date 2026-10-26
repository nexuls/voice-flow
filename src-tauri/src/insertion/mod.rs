//! Text insertion into the application that was focused when recording began.

use std::sync::mpsc;
use std::thread::JoinHandle;

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
}

/// Places text on the clipboard without pasting it.
pub struct ClipboardInsertion {
    clipboard: Option<arboard::Clipboard>,
}

impl ClipboardInsertion {
    pub fn new() -> Self {
        Self { clipboard: None }
    }

    fn clipboard(&mut self) -> Result<&mut arboard::Clipboard, InsertError> {
        if self.clipboard.is_none() {
            self.clipboard =
                Some(arboard::Clipboard::new().map_err(|e| InsertError::Clipboard(e.to_string()))?);
        }
        Ok(self.clipboard.as_mut().expect("just set"))
    }
}

impl Default for ClipboardInsertion {
    fn default() -> Self {
        Self::new()
    }
}

impl TextInsertionService for ClipboardInsertion {
    fn capture_target(&mut self) -> InsertTarget {
        InsertTarget::default()
    }

    fn insert(&mut self, text: &str, _: &InsertTarget) -> Result<InsertOutcome, InsertError> {
        self.copy(text)?;
        Ok(InsertOutcome::CopiedToClipboard)
    }

    fn copy(&mut self, text: &str) -> Result<(), InsertError> {
        self.clipboard()?
            .set_text(text.to_owned())
            .map_err(|e| InsertError::Clipboard(e.to_string()))
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
