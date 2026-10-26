//! Event loop that drives the state machine.
//!
//! Inputs from shortcuts, audio, transcription and insertion services arrive
//! on one channel and are applied in order. Effects go to an [`Executor`].
//! Dismiss timers are owned here. Every transition is reported to a
//! [`StateObserver`].

use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::{mpsc, oneshot};

use super::machine::{Effect, Input, Machine, SessionId, StateView};

/// Executes side effects requested by the machine. Implementations must not
/// block: long work belongs on its own thread or task, and reports back
/// through the [`WorkflowHandle`].
pub trait Executor: Send + 'static {
    fn execute(&mut self, effect: Effect, handle: &WorkflowHandle);
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transition {
    /// Monotonic sequence number, unique per controller.
    pub seq: u64,
    /// Milliseconds since the controller started.
    pub at_ms: u64,
    pub input: &'static str,
    pub from: StateView,
    pub to: StateView,
}

pub trait StateObserver: Send + 'static {
    fn on_transition(&mut self, transition: &Transition);
}

impl<F: FnMut(&Transition) + Send + 'static> StateObserver for F {
    fn on_transition(&mut self, transition: &Transition) {
        self(transition)
    }
}

enum Command {
    Input(Input),
    State(oneshot::Sender<StateView>),
    TakeTranscript(oneshot::Sender<Option<String>>),
}

/// Cheap, cloneable handle for sending inputs to the controller.
#[derive(Clone)]
pub struct WorkflowHandle {
    tx: mpsc::UnboundedSender<Command>,
}

impl WorkflowHandle {
    /// Sends an input. Returns false if the controller has stopped.
    pub fn send(&self, input: Input) -> bool {
        self.tx.send(Command::Input(input)).is_ok()
    }

    pub async fn state(&self) -> Option<StateView> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(Command::State(tx)).ok()?;
        rx.await.ok()
    }

    /// Returns the transcript held for manual copying, if any.
    pub async fn retained_transcript(&self) -> Option<String> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(Command::TakeTranscript(tx)).ok()?;
        rx.await.ok().flatten()
    }
}

impl Input {
    pub fn kind(&self) -> &'static str {
        match self {
            Input::ShortcutPressed => "shortcutPressed",
            Input::ShortcutReleased => "shortcutReleased",
            Input::CaptureFailed { .. } => "captureFailed",
            Input::CaptureEmpty { .. } => "captureEmpty",
            Input::TranscriptReady { .. } => "transcriptReady",
            Input::TranscriptionFailed { .. } => "transcriptionFailed",
            Input::InsertionDone { .. } => "insertionDone",
            Input::InsertionFailed { .. } => "insertionFailed",
            Input::Dismiss { .. } => "dismiss",
            Input::Cancel => "cancel",
            Input::Shutdown => "shutdown",
        }
    }
}

/// Starts the controller on the given tokio runtime.
pub fn spawn(
    rt: &tokio::runtime::Handle,
    mut executor: impl Executor,
    mut observer: impl StateObserver,
) -> (WorkflowHandle, tokio::task::JoinHandle<()>) {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let handle = WorkflowHandle { tx };
    let loop_handle = handle.clone();
    let task = rt.spawn(async move {
        let started = Instant::now();
        let mut machine = Machine::new();
        let mut seq = 0u64;
        while let Some(cmd) = rx.recv().await {
            match cmd {
                Command::Input(input) => {
                    let shutdown = matches!(input, Input::Shutdown);
                    let kind = input.kind();
                    let from = machine.state().view();
                    let effects = machine.handle(input);
                    let to = machine.state().view();
                    if from != to {
                        seq += 1;
                        let t = Transition {
                            seq,
                            at_ms: started.elapsed().as_millis() as u64,
                            input: kind,
                            from,
                            to,
                        };
                        tracing::debug!(input = kind, to = ?t.to, "transition");
                        observer.on_transition(&t);
                    }
                    for effect in effects {
                        match effect {
                            Effect::ScheduleDismiss { session, after } => {
                                schedule_dismiss(&loop_handle, session, after)
                            }
                            other => executor.execute(other, &loop_handle),
                        }
                    }
                    if shutdown {
                        break;
                    }
                }
                Command::State(reply) => {
                    let _ = reply.send(machine.state().view());
                }
                Command::TakeTranscript(reply) => {
                    let _ = reply.send(machine.retained_transcript().map(str::to_owned));
                }
            }
        }
        tracing::debug!("workflow controller stopped");
    });
    (handle, task)
}

fn schedule_dismiss(handle: &WorkflowHandle, session: SessionId, after: Duration) {
    let handle = handle.clone();
    tokio::spawn(async move {
        tokio::time::sleep(after).await;
        handle.send(Input::Dismiss { session });
    });
}

/// Receives visualizer levels in [0, 1].
pub type LevelSink = std::sync::Arc<dyn Fn(f32) + Send + Sync>;

/// Simulates every service so state transitions can be checked with only the
/// shortcut. No audio is recorded and nothing is inserted. If a level sink is
/// set, a synthetic level signal drives the overlay visualizer.
pub struct DiagnosticExecutor {
    pub processing_delay: Duration,
    pub insertion_delay: Duration,
    pub levels: Option<LevelSink>,
    /// Session whose synthetic levels are running, or 0 for none.
    active: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl Default for DiagnosticExecutor {
    fn default() -> Self {
        Self {
            processing_delay: Duration::from_millis(600),
            insertion_delay: Duration::from_millis(150),
            levels: None,
            active: Default::default(),
        }
    }
}

impl DiagnosticExecutor {
    pub fn with_levels(levels: LevelSink) -> Self {
        Self {
            levels: Some(levels),
            ..Self::default()
        }
    }

    fn start_levels(&self, session: SessionId) {
        use std::sync::atomic::Ordering;
        let Some(sink) = self.levels.clone() else {
            return;
        };
        self.active.store(session, Ordering::SeqCst);
        let active = self.active.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(33));
            let mut t = 0f32;
            while active.load(Ordering::SeqCst) == session {
                tick.tick().await;
                t += 0.033;
                // Speech-like envelope: syllable rhythm on a slow phrase swell.
                let level =
                    (0.5 + 0.5 * (t * 9.0).sin()).powi(2) * (0.4 + 0.6 * (t * 1.3).sin().abs());
                sink(level);
            }
        });
    }

    fn stop_levels(&self) {
        self.active.store(0, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Executor for DiagnosticExecutor {
    fn execute(&mut self, effect: Effect, handle: &WorkflowHandle) {
        let handle = handle.clone();
        match effect {
            Effect::BeginCapture { session } => self.start_levels(session),
            Effect::AbortCapture { .. } => self.stop_levels(),
            Effect::FinishCapture { session } => {
                self.stop_levels();
                let delay = self.processing_delay;
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    handle.send(Input::TranscriptReady {
                        session,
                        text: "diagnostic transcript".into(),
                    });
                });
            }
            Effect::Insert { session, .. } => {
                let delay = self.insertion_delay;
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    handle.send(Input::InsertionDone {
                        session,
                        outcome: super::machine::InsertOutcome::CopiedToClipboard,
                    });
                });
            }
            other => tracing::debug!(?other, "diagnostic: effect skipped"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Clone, Default)]
    struct Recorder {
        effects: Arc<Mutex<Vec<Effect>>>,
        transitions: Arc<Mutex<Vec<String>>>,
    }

    impl Executor for Recorder {
        fn execute(&mut self, effect: Effect, _: &WorkflowHandle) {
            self.effects.lock().unwrap().push(effect);
        }
    }

    fn rt() -> tokio::runtime::Handle {
        tokio::runtime::Handle::current()
    }

    fn phase(v: &StateView) -> String {
        serde_json::to_value(v).unwrap()["phase"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn settle(h: &WorkflowHandle) -> StateView {
        h.state().await.unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn diagnostic_mode_runs_full_cycle_without_services() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let sink = log.clone();
        let (h, task) = spawn(
            &rt(),
            DiagnosticExecutor::default(),
            move |t: &Transition| {
                sink.lock().unwrap().push(phase(&t.to));
            },
        );
        h.send(Input::ShortcutPressed);
        h.send(Input::ShortcutReleased);
        // Advance virtual time past processing, insertion and dismissal.
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert_eq!(phase(&settle(&h).await), "idle");
        assert_eq!(
            *log.lock().unwrap(),
            ["recording", "processing", "inserting", "success", "idle"]
        );
        h.send(Input::Shutdown);
        task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn executor_receives_effects_and_dismiss_is_internal() {
        let rec = Recorder::default();
        let transitions = rec.transitions.clone();
        let (h, _task) = spawn(&rt(), rec.clone(), move |t: &Transition| {
            transitions.lock().unwrap().push(t.input.to_owned());
        });
        h.send(Input::ShortcutPressed);
        h.send(Input::ShortcutReleased);
        h.send(Input::TranscriptionFailed {
            session: 1,
            error: super::super::machine::DictationError::new(
                super::super::machine::ErrorKind::Transcription,
                "boom",
            ),
        });
        assert_eq!(phase(&settle(&h).await), "error");
        tokio::time::sleep(Duration::from_secs(10)).await;
        assert_eq!(phase(&settle(&h).await), "idle");
        let effects = rec.effects.lock().unwrap().clone();
        assert_eq!(
            effects,
            vec![
                Effect::ShowOverlay,
                Effect::BeginCapture { session: 1 },
                Effect::FinishCapture { session: 1 },
                Effect::HideOverlay,
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn new_press_during_success_is_not_dismissed_by_old_timer() {
        let (h, _task) = spawn(&rt(), DiagnosticExecutor::default(), |_: &Transition| {});
        h.send(Input::ShortcutPressed);
        h.send(Input::ShortcutReleased);
        // Reach Success, then press again before the dismiss timer fires.
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert_eq!(phase(&settle(&h).await), "success");
        h.send(Input::ShortcutPressed);
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(phase(&settle(&h).await), "recording");
    }

    #[tokio::test(start_paused = true)]
    async fn diagnostic_levels_run_only_while_recording() {
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c = count.clone();
        let exec = DiagnosticExecutor::with_levels(Arc::new(move |l: f32| {
            assert!((0.0..=1.0).contains(&l));
            c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));
        let (h, _task) = spawn(&rt(), exec, |_: &Transition| {});
        h.send(Input::ShortcutPressed);
        tokio::time::sleep(Duration::from_millis(500)).await;
        h.send(Input::ShortcutReleased);
        settle(&h).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let during = count.load(std::sync::atomic::Ordering::SeqCst);
        assert!(during >= 10, "expected ~15 levels, got {during}");
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), during);
    }

    #[tokio::test]
    async fn shutdown_stops_the_loop() {
        let (h, task) = spawn(&rt(), Recorder::default(), |_: &Transition| {});
        h.send(Input::Shutdown);
        task.await.unwrap();
        assert!(!h.send(Input::ShortcutPressed));
    }
}
