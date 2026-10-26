//! Live executor: connects the workflow to the real capture, transcription
//! and insertion services.
//!
//! The executor is called on the controller task and must not block, so each
//! effect forwards work to a service thread or spawns a task. Results come
//! back as workflow inputs tagged with their session id. The state machine
//! drops results from stale sessions.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::oneshot;

use crate::audio::{
    AudioCaptureService, CaptureConfig, CaptureError, CaptureEvent, CaptureEvents, CaptureInfo,
    Recording,
};
use crate::insertion::{InsertTarget, InsertionWorker};
use crate::transcription::{CancelToken, EngineError, TranscribeOptions, TranscriptionService};
use crate::workflow::{
    DictationError, Effect, ErrorKind, Executor, Input, LevelSink, SessionId, WorkflowHandle,
};

/// Recordings shorter than this are not transcribed.
pub const MIN_RECORDING: Duration = Duration::from_millis(300);
/// Recordings whose peak stays below this are treated as a silent microphone.
pub const SILENT_PEAK: f32 = 1e-3;

#[derive(Debug, Clone, Default)]
pub struct LiveConfig {
    pub capture: CaptureConfig,
    pub transcribe: TranscribeOptions,
}

/// Per-dictation latency measurements. Contains no audio or text.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTimings {
    pub session: SessionId,
    /// Press to a running microphone stream.
    pub capture_open_ms: Option<f64>,
    /// Press to the overlay showing the recording state (reported by the overlay).
    pub overlay_visible_ms: Option<f64>,
    pub recorded_ms: Option<f64>,
    /// Release to finalised 16 kHz audio (stop, drain and resample).
    pub finalize_ms: Option<f64>,
    /// Audio ready to transcript ready (engine call plus queueing).
    pub inference_ms: Option<f64>,
    pub engine_ttft_ms: Option<f64>,
    /// Transcript ready to completed insertion attempt.
    pub insert_ms: Option<f64>,
    /// Release to completed insertion attempt.
    pub release_to_insert_ms: Option<f64>,
}

pub type TimingSink = Arc<dyn Fn(&SessionTimings) + Send + Sync>;

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

// ---------------------------------------------------------------------------
// Audio control thread
// ---------------------------------------------------------------------------

enum AudioCmd {
    Start {
        session: SessionId,
        config: CaptureConfig,
        events: CaptureEvents,
        reply: oneshot::Sender<Result<CaptureInfo, CaptureError>>,
    },
    Finish {
        session: SessionId,
        reply: oneshot::Sender<Result<Recording, CaptureError>>,
    },
    Abort {
        session: SessionId,
    },
}

/// Serialises capture commands on one thread. A release that arrives while
/// the device is still opening is handled after the open completes, so start
/// and finish cannot race.
pub struct AudioControl {
    tx: Option<mpsc::Sender<AudioCmd>>,
    thread: Option<JoinHandle<()>>,
}

impl AudioControl {
    pub fn spawn(mut capture: Box<dyn AudioCaptureService>) -> Self {
        let (tx, rx) = mpsc::channel::<AudioCmd>();
        let thread = std::thread::Builder::new()
            .name("audio-control".into())
            .spawn(move || {
                let mut current: Option<SessionId> = None;
                for cmd in rx {
                    match cmd {
                        AudioCmd::Start {
                            session,
                            config,
                            events,
                            reply,
                        } => {
                            if current.take().is_some() {
                                capture.abort();
                            }
                            let r = capture.start(&config, events);
                            if r.is_ok() {
                                current = Some(session);
                            }
                            let _ = reply.send(r);
                        }
                        AudioCmd::Finish { session, reply } => {
                            let r = if current == Some(session) {
                                current = None;
                                capture.finish()
                            } else {
                                Err(CaptureError::NotRecording)
                            };
                            let _ = reply.send(r);
                        }
                        AudioCmd::Abort { session } => {
                            if current == Some(session) {
                                current = None;
                                capture.abort();
                            }
                        }
                    }
                }
                capture.abort();
            })
            .expect("spawn audio control thread");
        Self {
            tx: Some(tx),
            thread: Some(thread),
        }
    }

    fn send(&self, cmd: AudioCmd) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(cmd);
        }
    }

    pub fn start(
        &self,
        session: SessionId,
        config: CaptureConfig,
        events: CaptureEvents,
    ) -> oneshot::Receiver<Result<CaptureInfo, CaptureError>> {
        let (reply, rx) = oneshot::channel();
        self.send(AudioCmd::Start {
            session,
            config,
            events,
            reply,
        });
        rx
    }

    pub fn finish(&self, session: SessionId) -> oneshot::Receiver<Result<Recording, CaptureError>> {
        let (reply, rx) = oneshot::channel();
        self.send(AudioCmd::Finish { session, reply });
        rx
    }

    pub fn abort(&self, session: SessionId) {
        self.send(AudioCmd::Abort { session });
    }
}

impl Drop for AudioControl {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// ---------------------------------------------------------------------------
// Executor
// ---------------------------------------------------------------------------

pub fn capture_error(e: &CaptureError) -> DictationError {
    let message = match e {
        CaptureError::NoDevice => "No microphone found",
        CaptureError::PermissionDenied => "Microphone access denied",
        CaptureError::DeviceBusy => "Microphone is busy",
        CaptureError::Disconnected => "Microphone disconnected",
        CaptureError::Timeout => "Microphone did not start",
        _ => "Microphone error",
    };
    DictationError::new(ErrorKind::Microphone, message)
}

pub fn engine_error(e: &EngineError) -> DictationError {
    match e {
        EngineError::ModelNotInstalled | EngineError::NotLoaded | EngineError::Unavailable => {
            DictationError::new(ErrorKind::ModelUnavailable, "Speech model not installed")
        }
        EngineError::ModelCorrupt(_) | EngineError::LoadFailed(_) => DictationError::new(
            ErrorKind::ModelUnavailable,
            "Speech model could not be loaded",
        ),
        _ => DictationError::new(ErrorKind::Transcription, "Transcription failed"),
    }
}

#[derive(Default)]
struct SessionCtx {
    pressed_at: Option<Instant>,
    released_at: Option<Instant>,
    target: Option<InsertTarget>,
    cancel: Option<CancelToken>,
    timings: SessionTimings,
}

/// Shared between the executor and the tasks it spawns.
pub struct Services {
    pub audio: AudioControl,
    pub transcription: Arc<TranscriptionService>,
    pub insertion: Arc<InsertionWorker>,
    /// True once a model is loaded into the engine.
    pub engine_ready: Arc<AtomicBool>,
    pub config: Arc<RwLock<LiveConfig>>,
    pub levels: LevelSink,
    pub on_timings: TimingSink,
    sessions: Mutex<HashMap<SessionId, SessionCtx>>,
}

impl Services {
    pub fn new(
        audio: AudioControl,
        transcription: Arc<TranscriptionService>,
        insertion: Arc<InsertionWorker>,
        engine_ready: Arc<AtomicBool>,
        config: Arc<RwLock<LiveConfig>>,
        levels: LevelSink,
        on_timings: TimingSink,
    ) -> Arc<Self> {
        Arc::new(Self {
            audio,
            transcription,
            insertion,
            engine_ready,
            config,
            levels,
            on_timings,
            sessions: Mutex::new(HashMap::new()),
        })
    }

    fn with<R>(&self, s: SessionId, f: impl FnOnce(&mut SessionCtx) -> R) -> R {
        let mut map = self.sessions.lock().expect("sessions lock");
        f(map.entry(s).or_default())
    }

    /// Ends a session: reports its timings and forgets its context.
    fn close(&self, s: SessionId) {
        let ctx = self.sessions.lock().expect("sessions lock").remove(&s);
        if let Some(ctx) = ctx {
            let mut t = ctx.timings;
            t.session = s;
            tracing::info!(timings = ?t, "dictation finished");
            (self.on_timings)(&t);
        }
    }

    /// Called by the overlay once it has rendered the recording state.
    pub fn overlay_rendered(&self, s: SessionId) {
        self.with(s, |ctx| {
            if let Some(p) = ctx.pressed_at
                && ctx.timings.overlay_visible_ms.is_none()
            {
                ctx.timings.overlay_visible_ms = Some(ms(p.elapsed()));
            }
        });
    }

    /// Current capture and transcription options. Transcripts are never stored
    /// here. Only the state machine keeps one, for the manual-copy fallback.
    fn options(&self) -> (CaptureConfig, TranscribeOptions) {
        let c = self.config.read().expect("config lock");
        (c.capture.clone(), c.transcribe.clone())
    }
}

pub struct LiveExecutor {
    services: Arc<Services>,
}

impl LiveExecutor {
    pub fn new(services: Arc<Services>) -> Self {
        Self { services }
    }

    fn begin(&self, s: SessionId, handle: &WorkflowHandle) {
        let svc = self.services.clone();
        let now = Instant::now();
        svc.with(s, |ctx| ctx.pressed_at = Some(now));

        if !svc.engine_ready.load(Ordering::SeqCst) {
            handle.send(Input::CaptureFailed {
                session: s,
                error: DictationError::new(
                    ErrorKind::ModelUnavailable,
                    "Install the speech model in VoiceFlow settings",
                ),
            });
            svc.close(s);
            return;
        }

        let (capture, _) = svc.options();
        let h = handle.clone();
        let levels = svc.levels.clone();
        let events: CaptureEvents = Arc::new(move |e| match e {
            CaptureEvent::Level(l) => levels(l),
            CaptureEvent::LimitReached => {
                h.send(Input::CaptureLimitReached { session: s });
            }
            CaptureEvent::Failed(err) => {
                h.send(Input::CaptureFailed {
                    session: s,
                    error: capture_error(&err),
                });
            }
        });
        let started = svc.audio.start(s, capture, events);
        let h = handle.clone();
        let svc2 = svc.clone();
        tokio::spawn(async move {
            match started.await {
                Ok(Ok(_)) => svc2.with(s, |ctx| {
                    ctx.timings.capture_open_ms = Some(ms(now.elapsed()))
                }),
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "capture failed to start");
                    h.send(Input::CaptureFailed {
                        session: s,
                        error: capture_error(&e),
                    });
                }
                Err(_) => {}
            }
        });
        // Remember the focus target while the user's application is still
        // focused. The overlay never takes focus.
        let svc3 = svc.clone();
        tokio::spawn(async move {
            let target = svc3.insertion.capture_target().await;
            svc3.with(s, |ctx| ctx.target = Some(target));
        });
    }

    fn finish(&self, s: SessionId, handle: &WorkflowHandle) {
        let svc = self.services.clone();
        let released = Instant::now();
        svc.with(s, |ctx| ctx.released_at = Some(released));
        let recording = svc.audio.finish(s);
        let h = handle.clone();
        tokio::spawn(async move {
            let rec = match recording.await {
                Ok(Ok(rec)) => rec,
                Ok(Err(e)) => {
                    // NotRecording means the start failed and was already reported.
                    if e != CaptureError::NotRecording {
                        h.send(Input::CaptureFailed {
                            session: s,
                            error: capture_error(&e),
                        });
                    }
                    svc.close(s);
                    return;
                }
                Err(_) => return,
            };
            let finalize = released.elapsed();
            svc.with(s, |ctx| {
                ctx.timings.finalize_ms = Some(ms(finalize));
                ctx.timings.recorded_ms = Some(ms(rec.duration()));
            });
            tracing::debug!(?rec, convert_ms = format!("{:.1}", rec.convert_ms), "recording finished");

            if rec.duration() < MIN_RECORDING {
                h.send(Input::CaptureEmpty {
                    session: s,
                    error: DictationError::new(
                        ErrorKind::TooShort,
                        "Hold the shortcut while speaking",
                    ),
                });
                svc.close(s);
                return;
            }
            if rec.stats.peak < SILENT_PEAK {
                h.send(Input::CaptureEmpty {
                    session: s,
                    error: DictationError::new(
                        ErrorKind::NoSpeech,
                        "No sound from the microphone. Is it muted?",
                    ),
                });
                svc.close(s);
                return;
            }

            let mut rec = rec;
            let samples = std::mem::take(&mut rec.samples);
            drop(rec);
            let (_, options) = svc.options();
            let infer_start = Instant::now();
            let (cancel, result) = svc.transcription.transcribe(samples, options);
            svc.with(s, |ctx| ctx.cancel = Some(cancel));
            match result.await {
                Ok(Ok(t)) => {
                    svc.with(s, |ctx| {
                        ctx.timings.inference_ms = Some(ms(infer_start.elapsed()));
                        ctx.timings.engine_ttft_ms = Some(t.timing.ttft_ms);
                    });
                    tracing::debug!(transcript = ?t, "transcript ready");
                    h.send(Input::TranscriptReady {
                        session: s,
                        text: t.text,
                    });
                }
                Ok(Err(EngineError::Cancelled)) | Err(_) => svc.close(s),
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "transcription failed");
                    h.send(Input::TranscriptionFailed {
                        session: s,
                        error: engine_error(&e),
                    });
                    svc.close(s);
                }
            }
        });
    }

    fn insert(&self, s: SessionId, text: String, handle: &WorkflowHandle) {
        let svc = self.services.clone();
        let h = handle.clone();
        tokio::spawn(async move {
            let target = svc.with(s, |ctx| ctx.target.clone()).unwrap_or_default();
            let start = Instant::now();
            let result = svc.insertion.insert(text.clone(), target).await;
            svc.with(s, |ctx| {
                ctx.timings.insert_ms = Some(ms(start.elapsed()));
                ctx.timings.release_to_insert_ms = ctx.released_at.map(|r| ms(r.elapsed()));
            });
            match result {
                Ok(outcome) => {
                    h.send(Input::InsertionDone {
                        session: s,
                        outcome,
                    });
                }
                Err(e) => {
                    tracing::warn!(error = %e, "insertion failed");
                    h.send(Input::InsertionFailed {
                        session: s,
                        error: DictationError::new(ErrorKind::Insertion, e.to_string()),
                        text,
                    });
                }
            }
            svc.close(s);
        });
    }
}

impl Executor for LiveExecutor {
    fn execute(&mut self, effect: Effect, handle: &WorkflowHandle) {
        match effect {
            Effect::BeginCapture { session } => self.begin(session, handle),
            Effect::FinishCapture { session } => self.finish(session, handle),
            Effect::AbortCapture { session } => {
                self.services.audio.abort(session);
                self.services.close(session);
            }
            Effect::CancelTranscription { session } => {
                if let Some(c) = self.services.with(session, |ctx| ctx.cancel.clone()) {
                    c.cancel();
                }
            }
            Effect::Insert { session, text } => self.insert(session, text, handle),
            // Overlay effects and timers are handled elsewhere.
            Effect::ShowOverlay | Effect::HideOverlay | Effect::ScheduleDismiss { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;
    use crate::audio::level::SignalStats;
    use crate::insertion::{InsertError, TextInsertionService};
    use crate::transcription::MockEngine;
    use crate::workflow::{InsertOutcome, StateView, Transition, controller};

    /// Capture fake that yields `audio` (16 kHz) on finish.
    struct FakeCapture {
        audio: Vec<f32>,
        open_delay: Duration,
        fail_start: Option<CaptureError>,
        active: bool,
        starts: Arc<AtomicUsize>,
    }

    impl AudioCaptureService for FakeCapture {
        fn start(
            &mut self,
            _: &CaptureConfig,
            events: CaptureEvents,
        ) -> Result<CaptureInfo, CaptureError> {
            std::thread::sleep(self.open_delay);
            if let Some(e) = &self.fail_start {
                return Err(e.clone());
            }
            self.active = true;
            self.starts.fetch_add(1, Ordering::SeqCst);
            events(CaptureEvent::Level(0.5));
            Ok(CaptureInfo {
                device_name: "fake".into(),
                sample_rate: 16_000,
                channels: 1,
                sample_format: "f32".into(),
                fell_back_to_default: false,
                open_ms: 0.0,
            })
        }
        fn finish(&mut self) -> Result<Recording, CaptureError> {
            if !std::mem::take(&mut self.active) {
                return Err(CaptureError::NotRecording);
            }
            Ok(Recording {
                stats: SignalStats::of(&self.audio),
                samples: self.audio.clone(),
                source_rate: 16_000,
                source_channels: 1,
                dropped_frames: 0,
                limit_reached: false,
                convert_ms: 0.0,
            })
        }
        fn abort(&mut self) {
            self.active = false;
        }
        fn is_active(&self) -> bool {
            self.active
        }
    }

    #[derive(Clone)]
    struct FakeInsertion {
        fail: bool,
        inserted: Arc<Mutex<Vec<String>>>,
    }

    impl TextInsertionService for FakeInsertion {
        fn capture_target(&mut self) -> InsertTarget {
            InsertTarget {
                window: Some("editor".into()),
            }
        }
        fn insert(&mut self, text: &str, t: &InsertTarget) -> Result<InsertOutcome, InsertError> {
            if self.fail {
                return Err(InsertError::PasteUnavailable("test".into()));
            }
            assert_eq!(t.window.as_deref(), Some("editor"));
            self.inserted.lock().unwrap().push(text.to_owned());
            Ok(InsertOutcome::Pasted)
        }
        fn copy(&mut self, _: &str) -> Result<(), InsertError> {
            Ok(())
        }
    }

    struct Harness {
        handle: WorkflowHandle,
        phases: Arc<Mutex<Vec<String>>>,
        inserted: Arc<Mutex<Vec<String>>>,
        timings: Arc<Mutex<Vec<SessionTimings>>>,
        starts: Arc<AtomicUsize>,
    }

    struct Opts {
        audio: Vec<f32>,
        engine: MockEngine,
        ready: bool,
        open_delay: Duration,
        fail_start: Option<CaptureError>,
        fail_insert: bool,
    }

    impl Default for Opts {
        fn default() -> Self {
            Self {
                audio: (0..16_000).map(|i| (i as f32 * 0.07).sin() * 0.3).collect(),
                engine: MockEngine::default(),
                ready: true,
                open_delay: Duration::ZERO,
                fail_start: None,
                fail_insert: false,
            }
        }
    }

    fn harness(o: Opts) -> Harness {
        let starts = Arc::new(AtomicUsize::new(0));
        let capture = FakeCapture {
            audio: o.audio,
            open_delay: o.open_delay,
            fail_start: o.fail_start,
            active: false,
            starts: starts.clone(),
        };
        let inserted = Arc::new(Mutex::new(Vec::new()));
        let insertion = FakeInsertion {
            fail: o.fail_insert,
            inserted: inserted.clone(),
        };
        let timings = Arc::new(Mutex::new(Vec::new()));
        let t2 = timings.clone();
        let services = Services::new(
            AudioControl::spawn(Box::new(capture)),
            Arc::new(TranscriptionService::spawn(Box::new(o.engine))),
            Arc::new(InsertionWorker::spawn(Box::new(insertion))),
            Arc::new(AtomicBool::new(o.ready)),
            Arc::new(RwLock::new(LiveConfig::default())),
            Arc::new(|_| {}),
            Arc::new(move |t: &SessionTimings| t2.lock().unwrap().push(t.clone())),
        );
        let phases = Arc::new(Mutex::new(Vec::new()));
        let p2 = phases.clone();
        let (handle, _task) = controller::spawn(
            &tokio::runtime::Handle::current(),
            LiveExecutor::new(services),
            move |t: &Transition| {
                let v = serde_json::to_value(&t.to).unwrap();
                p2.lock()
                    .unwrap()
                    .push(v["phase"].as_str().unwrap().to_owned());
            },
        );
        Harness {
            handle,
            phases,
            inserted,
            timings,
            starts,
        }
    }

    async fn wait_for(h: &Harness, phase: &str) {
        for _ in 0..400 {
            if h.phases.lock().unwrap().last().map(String::as_str) == Some(phase) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("never reached {phase}: {:?}", h.phases.lock().unwrap());
    }

    async fn state(h: &Harness) -> StateView {
        h.handle.state().await.unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn full_dictation_inserts_transcript() {
        let h = harness(Opts::default());
        h.handle.send(Input::ShortcutPressed);
        tokio::time::sleep(Duration::from_millis(30)).await;
        h.handle.send(Input::ShortcutReleased);
        wait_for(&h, "success").await;
        assert_eq!(*h.inserted.lock().unwrap(), ["mock transcript"]);
        assert_eq!(
            *h.phases.lock().unwrap(),
            ["recording", "processing", "inserting", "success"]
        );
        let t = h.timings.lock().unwrap()[0].clone();
        assert!(t.capture_open_ms.is_some() && t.inference_ms.is_some() && t.insert_ms.is_some());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn release_during_device_startup_is_handled_in_order() {
        let h = harness(Opts {
            open_delay: Duration::from_millis(150),
            ..Default::default()
        });
        h.handle.send(Input::ShortcutPressed);
        h.handle.send(Input::ShortcutReleased);
        // Finish is queued behind the slow start, then transcribes normally.
        wait_for(&h, "success").await;
        assert_eq!(h.starts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn missing_model_fails_fast_without_recording() {
        let h = harness(Opts {
            ready: false,
            ..Default::default()
        });
        h.handle.send(Input::ShortcutPressed);
        wait_for(&h, "error").await;
        assert_eq!(h.starts.load(Ordering::SeqCst), 0, "microphone not opened");
        match state(&h).await {
            StateView::Error { error, .. } => assert_eq!(error.kind, ErrorKind::ModelUnavailable),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn microphone_failure_is_recoverable() {
        let h = harness(Opts {
            fail_start: Some(CaptureError::PermissionDenied),
            ..Default::default()
        });
        h.handle.send(Input::ShortcutPressed);
        wait_for(&h, "error").await;
        h.handle.send(Input::ShortcutReleased);
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(matches!(state(&h).await, StateView::Error { .. }));
        // A new press starts a new attempt.
        h.handle.send(Input::ShortcutPressed);
        wait_for(&h, "error").await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn inference_failure_and_empty_transcript() {
        let h = harness(Opts {
            engine: MockEngine {
                reply: Err(EngineError::Inference("boom".into())),
                ..Default::default()
            },
            ..Default::default()
        });
        h.handle.send(Input::ShortcutPressed);
        h.handle.send(Input::ShortcutReleased);
        wait_for(&h, "error").await;
        match state(&h).await {
            StateView::Error { error, .. } => assert_eq!(error.kind, ErrorKind::Transcription),
            other => panic!("{other:?}"),
        }

        let h = harness(Opts {
            engine: MockEngine {
                reply: Ok("   ".into()),
                ..Default::default()
            },
            ..Default::default()
        });
        h.handle.send(Input::ShortcutPressed);
        h.handle.send(Input::ShortcutReleased);
        wait_for(&h, "error").await;
        match state(&h).await {
            StateView::Error { error, .. } => assert_eq!(error.kind, ErrorKind::NoSpeech),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn silent_and_short_recordings_skip_inference() {
        let h = harness(Opts {
            audio: vec![0.0; 16_000],
            ..Default::default()
        });
        h.handle.send(Input::ShortcutPressed);
        h.handle.send(Input::ShortcutReleased);
        wait_for(&h, "error").await;
        assert!(matches!(
            state(&h).await,
            StateView::Error {
                error: DictationError {
                    kind: ErrorKind::NoSpeech,
                    ..
                },
                ..
            }
        ));

        let h = harness(Opts {
            audio: vec![0.3; 1_000],
            ..Default::default()
        });
        h.handle.send(Input::ShortcutPressed);
        h.handle.send(Input::ShortcutReleased);
        wait_for(&h, "error").await;
        assert!(matches!(
            state(&h).await,
            StateView::Error {
                error: DictationError {
                    kind: ErrorKind::TooShort,
                    ..
                },
                ..
            }
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancel_during_processing_discards_result() {
        let h = harness(Opts {
            engine: MockEngine {
                delay: Duration::from_millis(200),
                ..Default::default()
            },
            ..Default::default()
        });
        h.handle.send(Input::ShortcutPressed);
        h.handle.send(Input::ShortcutReleased);
        wait_for(&h, "processing").await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        h.handle.send(Input::Cancel);
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(state(&h).await, StateView::Idle);
        assert!(h.inserted.lock().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn repeated_shortcut_events_do_not_corrupt_the_session() {
        let h = harness(Opts {
            engine: MockEngine {
                delay: Duration::from_millis(100),
                ..Default::default()
            },
            ..Default::default()
        });
        for _ in 0..5 {
            h.handle.send(Input::ShortcutPressed);
        }
        h.handle.send(Input::ShortcutReleased);
        // Mash the shortcut while processing.
        for _ in 0..5 {
            h.handle.send(Input::ShortcutPressed);
            h.handle.send(Input::ShortcutReleased);
        }
        wait_for(&h, "success").await;
        assert_eq!(*h.inserted.lock().unwrap(), ["mock transcript"]);
        assert_eq!(h.starts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn insertion_failure_keeps_transcript_for_copy() {
        let h = harness(Opts {
            fail_insert: true,
            ..Default::default()
        });
        h.handle.send(Input::ShortcutPressed);
        h.handle.send(Input::ShortcutReleased);
        wait_for(&h, "error").await;
        assert!(matches!(
            state(&h).await,
            StateView::Error {
                has_transcript: true,
                ..
            }
        ));
        assert_eq!(
            h.handle.retained_transcript().await.as_deref(),
            Some("mock transcript")
        );
    }
}
