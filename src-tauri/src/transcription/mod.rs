//! Speech-to-text.
//!
//! The rest of the app depends only on [`TranscriptionEngine`] and
//! [`TranscriptionService`]. Needle-specific code lives in [`needle`]
//! (feature `needle`), and model files are handled by [`model`].

pub mod model;
#[cfg(feature = "needle")]
pub mod needle;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use zeroize::Zeroize;

/// Languages supported by Whistle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    En,
    De,
    Fr,
    Es,
    It,
    Nl,
    Pl,
}

impl Language {
    pub const ALL: [Language; 7] = [
        Language::En,
        Language::De,
        Language::Fr,
        Language::Es,
        Language::It,
        Language::Nl,
        Language::Pl,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Language::En => "en",
            Language::De => "de",
            Language::Fr => "fr",
            Language::Es => "es",
            Language::It => "it",
            Language::Nl => "nl",
            Language::Pl => "pl",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TranscribeOptions {
    /// `None` means detect the language automatically.
    pub language: Option<Language>,
    /// Words and phrases to bias decoding towards (custom vocabulary).
    pub keywords: Vec<String>,
    pub word_timestamps: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Word {
    pub word: String,
    pub start: f32,
    pub end: f32,
    pub probability: f32,
}

#[derive(Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub text: String,
    /// Detected or forced language. Empty when no speech was found.
    pub language: Option<String>,
    pub words: Option<Vec<Word>>,
    pub timing: TranscriptTiming,
}

impl std::fmt::Debug for Transcript {
    // Transcript text is sensitive. Never print it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transcript")
            .field("chars", &self.text.chars().count())
            .field("language", &self.language)
            .field("timing", &self.timing)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptTiming {
    pub audio_ms: f64,
    /// Wall time of the engine call.
    pub inference_ms: f64,
    /// Engine-reported time to first token.
    pub ttft_ms: f64,
    /// Engine-reported decoder tokens per second.
    pub decode_tps: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    #[error("speech model is not installed")]
    ModelNotInstalled,
    #[error("speech model is damaged: {0}")]
    ModelCorrupt(String),
    #[error("could not load speech model: {0}")]
    LoadFailed(String),
    #[error("speech model is not loaded")]
    NotLoaded,
    #[error("speech engine is already in use by this process")]
    EngineBusy,
    #[error("speech recognition is not available in this build")]
    Unavailable,
    #[error("invalid audio: {0}")]
    InvalidAudio(String),
    #[error("transcription failed: {0}")]
    Inference(String),
    #[error("transcription cancelled")]
    Cancelled,
    #[error("transcription service stopped")]
    Stopped,
}

/// A speech-to-text backend. Implementations run on one dedicated thread
/// and may block.
pub trait TranscriptionEngine: Send + 'static {
    fn name(&self) -> &'static str;
    fn is_loaded(&self) -> bool;
    /// Loads a model file. Loading again replaces the active model.
    fn load(&mut self, path: &Path) -> Result<(), EngineError>;
    /// Transcribes 16 kHz mono f32 audio in [-1, 1].
    fn transcribe(
        &mut self,
        audio: &[f32],
        options: &TranscribeOptions,
    ) -> Result<Transcript, EngineError>;
}

/// Removes NaN/inf and clamps to [-1, 1], in place.
pub fn sanitize_audio(audio: &mut [f32]) {
    for s in audio {
        *s = if s.is_finite() {
            s.clamp(-1.0, 1.0)
        } else {
            0.0
        };
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub engine: &'static str,
    pub loaded: bool,
    pub model_path: Option<PathBuf>,
    pub load_ms: Option<f64>,
}

/// Cancels a queued or running job. A running engine call cannot be
/// interrupted, so its result is discarded instead.
#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

enum Job {
    Load {
        path: PathBuf,
        reply: oneshot::Sender<Result<EngineStatus, EngineError>>,
    },
    Transcribe {
        audio: Vec<f32>,
        options: TranscribeOptions,
        cancel: CancelToken,
        reply: oneshot::Sender<Result<Transcript, EngineError>>,
    },
    Status(oneshot::Sender<EngineStatus>),
}

/// Owns the engine on a dedicated inference thread. Engine calls never run
/// on the UI thread, a tokio worker or the audio callback, and are strictly
/// serialised (Needle is process-global and not thread-safe).
pub struct TranscriptionService {
    tx: Option<std::sync::mpsc::Sender<Job>>,
    thread: Option<JoinHandle<()>>,
}

impl TranscriptionService {
    pub fn spawn(mut engine: Box<dyn TranscriptionEngine>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        let thread = std::thread::Builder::new()
            .name("inference".into())
            .spawn(move || {
                let mut status = EngineStatus {
                    engine: engine.name(),
                    loaded: engine.is_loaded(),
                    model_path: None,
                    load_ms: None,
                };
                for job in rx {
                    match job {
                        Job::Load { path, reply } => {
                            let t = Instant::now();
                            let r = engine.load(&path);
                            let ms = t.elapsed().as_secs_f64() * 1e3;
                            status.loaded = engine.is_loaded();
                            let r = r.map(|()| {
                                tracing::info!(
                                    engine = engine.name(),
                                    load_ms = format!("{ms:.1}"),
                                    "model loaded"
                                );
                                status.model_path = Some(path);
                                status.load_ms = Some(ms);
                                status.clone()
                            });
                            let _ = reply.send(r);
                        }
                        Job::Transcribe {
                            mut audio,
                            options,
                            cancel,
                            reply,
                        } => {
                            let result = if cancel.is_cancelled() {
                                Err(EngineError::Cancelled)
                            } else {
                                sanitize_audio(&mut audio);
                                let r = engine.transcribe(&audio, &options);
                                if cancel.is_cancelled() {
                                    Err(EngineError::Cancelled)
                                } else {
                                    r
                                }
                            };
                            audio.zeroize();
                            let _ = reply.send(result);
                        }
                        Job::Status(reply) => {
                            let _ = reply.send(status.clone());
                        }
                    }
                }
            })
            .expect("spawn inference thread");
        Self {
            tx: Some(tx),
            thread: Some(thread),
        }
    }

    fn send(&self, job: Job) -> bool {
        self.tx.as_ref().is_some_and(|tx| tx.send(job).is_ok())
    }

    pub async fn load(&self, path: PathBuf) -> Result<EngineStatus, EngineError> {
        let (reply, rx) = oneshot::channel();
        if !self.send(Job::Load { path, reply }) {
            return Err(EngineError::Stopped);
        }
        rx.await.unwrap_or(Err(EngineError::Stopped))
    }

    /// Queues a transcription. Returns immediately. The audio is moved into
    /// the service and zeroed after use.
    pub fn transcribe(
        &self,
        audio: Vec<f32>,
        options: TranscribeOptions,
    ) -> (
        CancelToken,
        oneshot::Receiver<Result<Transcript, EngineError>>,
    ) {
        let cancel = CancelToken::default();
        let (reply, rx) = oneshot::channel();
        let job = Job::Transcribe {
            audio,
            options,
            cancel: cancel.clone(),
            reply,
        };
        let rejected = match &self.tx {
            Some(tx) => tx.send(job).err().map(|e| e.0),
            None => Some(job),
        };
        if let Some(Job::Transcribe { reply, .. }) = rejected {
            let _ = reply.send(Err(EngineError::Stopped));
        }
        (cancel, rx)
    }

    pub async fn status(&self) -> Option<EngineStatus> {
        let (reply, rx) = oneshot::channel();
        self.send(Job::Status(reply)).then_some(())?;
        rx.await.ok()
    }

    /// Stops accepting jobs and waits up to `timeout` for the current job to
    /// end. An engine call in progress cannot be interrupted.
    pub fn shutdown(&mut self, timeout: Duration) {
        self.tx.take();
        if let Some(t) = self.thread.take() {
            let deadline = Instant::now() + timeout;
            while !t.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            if t.is_finished() {
                let _ = t.join();
            } else {
                tracing::warn!("inference thread still busy at shutdown");
            }
        }
    }
}

impl Drop for TranscriptionService {
    fn drop(&mut self) {
        self.shutdown(Duration::from_secs(2));
    }
}

/// Deterministic engine for tests and diagnostics.
pub struct MockEngine {
    pub loaded: bool,
    pub delay: Duration,
    pub reply: Result<String, EngineError>,
}

impl Default for MockEngine {
    fn default() -> Self {
        Self {
            loaded: true,
            delay: Duration::ZERO,
            reply: Ok("mock transcript".into()),
        }
    }
}

impl TranscriptionEngine for MockEngine {
    fn name(&self) -> &'static str {
        "mock"
    }
    fn is_loaded(&self) -> bool {
        self.loaded
    }
    fn load(&mut self, path: &Path) -> Result<(), EngineError> {
        if !path.exists() {
            return Err(EngineError::ModelNotInstalled);
        }
        self.loaded = true;
        Ok(())
    }
    fn transcribe(
        &mut self,
        audio: &[f32],
        _: &TranscribeOptions,
    ) -> Result<Transcript, EngineError> {
        if !self.loaded {
            return Err(EngineError::NotLoaded);
        }
        std::thread::sleep(self.delay);
        let silent = audio.iter().all(|s| s.abs() < 1e-4);
        let text = if silent {
            String::new()
        } else {
            self.reply.clone()?
        };
        Ok(Transcript {
            language: (!text.is_empty()).then(|| "en".into()),
            text,
            words: None,
            timing: TranscriptTiming {
                audio_ms: audio.len() as f64 / 16.0,
                inference_ms: self.delay.as_secs_f64() * 1e3,
                ..Default::default()
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speech() -> Vec<f32> {
        (0..16_000).map(|i| (i as f32 * 0.05).sin() * 0.3).collect()
    }

    #[tokio::test]
    async fn service_transcribes_with_mock() {
        let svc = TranscriptionService::spawn(Box::new(MockEngine::default()));
        let (_c, rx) = svc.transcribe(speech(), TranscribeOptions::default());
        let t = rx.await.unwrap().unwrap();
        assert_eq!(t.text, "mock transcript");
        assert_eq!(t.timing.audio_ms, 1000.0);
    }

    #[tokio::test]
    async fn silence_and_empty_give_empty_text() {
        let svc = TranscriptionService::spawn(Box::new(MockEngine::default()));
        for audio in [vec![0.0; 16_000], vec![]] {
            let (_c, rx) = svc.transcribe(audio, TranscribeOptions::default());
            let t = rx.await.unwrap().unwrap();
            assert!(t.text.is_empty());
            assert_eq!(t.language, None);
        }
    }

    #[tokio::test]
    async fn errors_are_reported() {
        let engine = MockEngine {
            reply: Err(EngineError::Inference("boom".into())),
            ..Default::default()
        };
        let svc = TranscriptionService::spawn(Box::new(engine));
        let (_c, rx) = svc.transcribe(speech(), TranscribeOptions::default());
        assert_eq!(
            rx.await.unwrap(),
            Err(EngineError::Inference("boom".into()))
        );
    }

    #[tokio::test]
    async fn unloaded_engine_reports_not_loaded_and_missing_model() {
        let engine = MockEngine {
            loaded: false,
            ..Default::default()
        };
        let svc = TranscriptionService::spawn(Box::new(engine));
        let (_c, rx) = svc.transcribe(speech(), TranscribeOptions::default());
        assert_eq!(rx.await.unwrap(), Err(EngineError::NotLoaded));
        let err = svc.load(PathBuf::from("/nonexistent/whistle.cact")).await;
        assert_eq!(err.unwrap_err(), EngineError::ModelNotInstalled);
        assert!(!svc.status().await.unwrap().loaded);
    }

    #[tokio::test]
    async fn queued_job_can_be_cancelled_and_running_job_is_discarded() {
        let engine = MockEngine {
            delay: Duration::from_millis(200),
            ..Default::default()
        };
        let svc = TranscriptionService::spawn(Box::new(engine));
        let (running, rx1) = svc.transcribe(speech(), TranscribeOptions::default());
        let (queued, rx2) = svc.transcribe(speech(), TranscribeOptions::default());
        queued.cancel();
        running.cancel();
        assert_eq!(rx1.await.unwrap(), Err(EngineError::Cancelled));
        assert_eq!(rx2.await.unwrap(), Err(EngineError::Cancelled));
    }

    #[tokio::test]
    async fn inference_does_not_block_the_async_runtime() {
        let engine = MockEngine {
            delay: Duration::from_millis(400),
            ..Default::default()
        };
        let svc = TranscriptionService::spawn(Box::new(engine));
        let t0 = Instant::now();
        let (_c, rx) = svc.transcribe(speech(), TranscribeOptions::default());
        assert!(
            t0.elapsed() < Duration::from_millis(20),
            "submit must not block"
        );
        // The runtime keeps running other work, like recording controls,
        // while inference runs.
        let mut ticks = 0;
        let mut rx = rx;
        let result = loop {
            tokio::select! {
                r = &mut rx => break r,
                _ = tokio::time::sleep(Duration::from_millis(10)) => ticks += 1,
            }
        };
        assert!(result.unwrap().is_ok());
        assert!(
            ticks >= 20,
            "runtime made progress during inference ({ticks} ticks)"
        );
    }

    #[tokio::test]
    async fn stopped_service_fails_fast() {
        let mut svc = TranscriptionService::spawn(Box::new(MockEngine::default()));
        svc.shutdown(Duration::from_secs(1));
        let (_c, rx) = svc.transcribe(speech(), TranscribeOptions::default());
        assert_eq!(rx.await.unwrap(), Err(EngineError::Stopped));
        assert_eq!(
            svc.load(PathBuf::from("x")).await.unwrap_err(),
            EngineError::Stopped
        );
    }

    #[test]
    fn sanitize_clamps_and_removes_non_finite() {
        let mut a = [f32::NAN, 2.0, -3.0, 0.5, f32::INFINITY];
        sanitize_audio(&mut a);
        assert_eq!(a, [0.0, 1.0, -1.0, 0.5, 0.0]);
    }

    #[test]
    fn transcript_debug_hides_text() {
        let t = Transcript {
            text: "secret words".into(),
            language: Some("en".into()),
            words: None,
            timing: Default::default(),
        };
        assert!(!format!("{t:?}").contains("secret"));
    }
}
