//! Pure dictation state machine.
//!
//! `Machine::handle` maps (state, input) to (new state, effects). It performs
//! no I/O, so every transition is unit-testable. Each recording gets a fresh
//! session id. Late results from an older session are ignored, so a stale
//! transcription can never be inserted.

use std::time::Duration;

use serde::Serialize;

pub type SessionId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// Recording held no detectable speech. Not a failure, just feedback.
    NoSpeech,
    /// The recording was shorter than the minimum useful length.
    TooShort,
    Microphone,
    ModelUnavailable,
    Transcription,
    Insertion,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationError {
    pub kind: ErrorKind,
    /// Short, user-facing explanation. Never contains transcript text.
    pub message: String,
}

impl DictationError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    fn dismiss_after(&self, has_transcript: bool) -> Duration {
        match self.kind {
            ErrorKind::NoSpeech | ErrorKind::TooShort => Duration::from_millis(1500),
            _ if has_transcript => Duration::from_secs(8),
            _ => Duration::from_secs(4),
        }
    }
}

/// How the transcript reached the target application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InsertOutcome {
    /// A paste keystroke was sent to the target application.
    Pasted,
    /// Automatic paste is unavailable. The transcript was left on the clipboard.
    CopiedToClipboard,
}

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Idle,
    Recording {
        session: SessionId,
    },
    Processing {
        session: SessionId,
    },
    Inserting {
        session: SessionId,
    },
    Success {
        session: SessionId,
        outcome: InsertOutcome,
    },
    Error {
        session: SessionId,
        error: DictationError,
        /// Kept in memory for the manual-copy fallback. Never serialised.
        transcript: Option<String>,
    },
}

/// Serialisable view of the state, without transcript text.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum StateView {
    Idle,
    #[serde(rename_all = "camelCase")]
    Recording {
        session: SessionId,
    },
    #[serde(rename_all = "camelCase")]
    Processing {
        session: SessionId,
    },
    #[serde(rename_all = "camelCase")]
    Inserting {
        session: SessionId,
    },
    #[serde(rename_all = "camelCase")]
    Success {
        session: SessionId,
        outcome: InsertOutcome,
    },
    #[serde(rename_all = "camelCase")]
    Error {
        session: SessionId,
        error: DictationError,
        has_transcript: bool,
    },
}

impl State {
    pub fn view(&self) -> StateView {
        match self {
            State::Idle => StateView::Idle,
            State::Recording { session } => StateView::Recording { session: *session },
            State::Processing { session } => StateView::Processing { session: *session },
            State::Inserting { session } => StateView::Inserting { session: *session },
            State::Success { session, outcome } => StateView::Success {
                session: *session,
                outcome: *outcome,
            },
            State::Error {
                session,
                error,
                transcript,
            } => StateView::Error {
                session: *session,
                error: error.clone(),
                has_transcript: transcript.is_some(),
            },
        }
    }

    pub fn session(&self) -> Option<SessionId> {
        match self {
            State::Idle => None,
            State::Recording { session }
            | State::Processing { session }
            | State::Inserting { session }
            | State::Success { session, .. }
            | State::Error { session, .. } => Some(*session),
        }
    }

    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            State::Recording { .. } | State::Processing { .. } | State::Inserting { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    ShortcutPressed,
    ShortcutReleased,
    CaptureFailed {
        session: SessionId,
        error: DictationError,
    },
    /// Capture finished but held no usable audio (too short or silent).
    CaptureEmpty {
        session: SessionId,
        error: DictationError,
    },
    TranscriptReady {
        session: SessionId,
        text: String,
    },
    TranscriptionFailed {
        session: SessionId,
        error: DictationError,
    },
    InsertionDone {
        session: SessionId,
        outcome: InsertOutcome,
    },
    InsertionFailed {
        session: SessionId,
        error: DictationError,
        text: String,
    },
    Dismiss {
        session: SessionId,
    },
    /// User-requested abort (for example Escape or a tray action).
    Cancel,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    ShowOverlay,
    HideOverlay,
    BeginCapture {
        session: SessionId,
    },
    /// Stop capture, finalise the buffer and start transcription.
    FinishCapture {
        session: SessionId,
    },
    AbortCapture {
        session: SessionId,
    },
    CancelTranscription {
        session: SessionId,
    },
    Insert {
        session: SessionId,
        text: String,
    },
    ScheduleDismiss {
        session: SessionId,
        after: Duration,
    },
}

pub const SUCCESS_DISMISS: Duration = Duration::from_millis(900);

#[derive(Debug)]
pub struct Machine {
    state: State,
    next_session: SessionId,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    pub fn new() -> Self {
        Self {
            state: State::Idle,
            next_session: 1,
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// Transcript held for the manual-copy fallback, if any.
    pub fn retained_transcript(&self) -> Option<&str> {
        match &self.state {
            State::Error {
                transcript: Some(t),
                ..
            } => Some(t),
            _ => None,
        }
    }

    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        let mut fx = Vec::new();

        let next = match (&self.state, input) {
            // Start a new recording. Pressing again while a result is on screen
            // replaces that result.
            (State::Idle | State::Success { .. } | State::Error { .. }, Input::ShortcutPressed) => {
                let session = self.next_session;
                self.next_session += 1;
                fx.push(Effect::ShowOverlay);
                fx.push(Effect::BeginCapture { session });
                Some(State::Recording { session })
            }
            (State::Recording { session }, Input::ShortcutReleased) => {
                fx.push(Effect::FinishCapture { session: *session });
                Some(State::Processing { session: *session })
            }
            (State::Recording { session }, Input::CaptureFailed { session: s, error })
                if *session == s =>
            {
                Some(self.fail(s, error, None, &mut fx))
            }
            (State::Processing { session }, Input::CaptureEmpty { session: s, error })
                if *session == s =>
            {
                Some(self.fail(s, error, None, &mut fx))
            }
            // Capture can also fail while finalising the buffer.
            (State::Processing { session }, Input::CaptureFailed { session: s, error })
                if *session == s =>
            {
                Some(self.fail(s, error, None, &mut fx))
            }
            (State::Processing { session }, Input::TranscriptReady { session: s, text })
                if *session == s =>
            {
                let text = text.trim().to_owned();
                if text.is_empty() {
                    let error = DictationError::new(ErrorKind::NoSpeech, "No speech detected");
                    Some(self.fail(s, error, None, &mut fx))
                } else {
                    fx.push(Effect::Insert { session: s, text });
                    Some(State::Inserting { session: s })
                }
            }
            (State::Processing { session }, Input::TranscriptionFailed { session: s, error })
                if *session == s =>
            {
                Some(self.fail(s, error, None, &mut fx))
            }
            (
                State::Inserting { session },
                Input::InsertionDone {
                    session: s,
                    outcome,
                },
            ) if *session == s => {
                fx.push(Effect::ScheduleDismiss {
                    session: s,
                    after: SUCCESS_DISMISS,
                });
                Some(State::Success {
                    session: s,
                    outcome,
                })
            }
            (
                State::Inserting { session },
                Input::InsertionFailed {
                    session: s,
                    error,
                    text,
                },
            ) if *session == s => Some(self.fail(s, error, Some(text), &mut fx)),
            (
                State::Success { session, .. } | State::Error { session, .. },
                Input::Dismiss { session: s },
            ) if *session == s => {
                fx.push(Effect::HideOverlay);
                Some(State::Idle)
            }
            (State::Recording { session }, Input::Cancel | Input::Shutdown) => {
                fx.push(Effect::AbortCapture { session: *session });
                fx.push(Effect::HideOverlay);
                Some(State::Idle)
            }
            (State::Processing { session }, Input::Cancel | Input::Shutdown) => {
                fx.push(Effect::CancelTranscription { session: *session });
                fx.push(Effect::HideOverlay);
                Some(State::Idle)
            }
            // A paste in flight cannot be undone safely. Cancel waits for it,
            // but shutdown hides the overlay at once.
            (State::Inserting { .. }, Input::Shutdown)
            | (State::Success { .. } | State::Error { .. }, Input::Cancel | Input::Shutdown) => {
                fx.push(Effect::HideOverlay);
                Some(State::Idle)
            }
            // Anything else is a duplicate, a stale result or a no-op. Pressing
            // during processing is ignored so the current transcript survives.
            (_, input) => {
                tracing::debug!(?input, "ignored input");
                None
            }
        };

        if let Some(state) = next {
            self.state = state;
        }
        fx
    }

    fn fail(
        &self,
        session: SessionId,
        error: DictationError,
        transcript: Option<String>,
        fx: &mut Vec<Effect>,
    ) -> State {
        fx.push(Effect::ScheduleDismiss {
            session,
            after: error.dismiss_after(transcript.is_some()),
        });
        State::Error {
            session,
            error,
            transcript,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(kind: ErrorKind) -> DictationError {
        DictationError::new(kind, "x")
    }

    /// Drives a machine to `Processing` and returns the session id.
    fn to_processing(m: &mut Machine) -> SessionId {
        m.handle(Input::ShortcutPressed);
        let s = m.state().session().unwrap();
        m.handle(Input::ShortcutReleased);
        s
    }

    #[test]
    fn happy_path() {
        let mut m = Machine::new();
        assert_eq!(
            m.handle(Input::ShortcutPressed),
            vec![Effect::ShowOverlay, Effect::BeginCapture { session: 1 }]
        );
        assert_eq!(m.state(), &State::Recording { session: 1 });
        assert_eq!(
            m.handle(Input::ShortcutReleased),
            vec![Effect::FinishCapture { session: 1 }]
        );
        assert_eq!(
            m.handle(Input::TranscriptReady {
                session: 1,
                text: "  hello  ".into()
            }),
            vec![Effect::Insert {
                session: 1,
                text: "hello".into()
            }]
        );
        let fx = m.handle(Input::InsertionDone {
            session: 1,
            outcome: InsertOutcome::Pasted,
        });
        assert_eq!(
            fx,
            vec![Effect::ScheduleDismiss {
                session: 1,
                after: SUCCESS_DISMISS
            }]
        );
        assert_eq!(
            m.handle(Input::Dismiss { session: 1 }),
            vec![Effect::HideOverlay]
        );
        assert_eq!(m.state(), &State::Idle);
    }

    #[test]
    fn duplicate_presses_and_releases_are_ignored() {
        let mut m = Machine::new();
        m.handle(Input::ShortcutPressed);
        assert!(m.handle(Input::ShortcutPressed).is_empty());
        assert_eq!(m.state(), &State::Recording { session: 1 });
        m.handle(Input::ShortcutReleased);
        assert!(m.handle(Input::ShortcutReleased).is_empty());
        assert_eq!(m.state(), &State::Processing { session: 1 });
    }

    #[test]
    fn release_without_press_is_ignored() {
        let mut m = Machine::new();
        assert!(m.handle(Input::ShortcutReleased).is_empty());
        assert_eq!(m.state(), &State::Idle);
    }

    #[test]
    fn pressing_while_processing_does_not_start_a_second_recording() {
        let mut m = Machine::new();
        let s = to_processing(&mut m);
        assert!(m.handle(Input::ShortcutPressed).is_empty());
        assert_eq!(m.state(), &State::Processing { session: s });
        m.handle(Input::TranscriptReady {
            session: s,
            text: "ok".into(),
        });
        assert!(m.handle(Input::ShortcutPressed).is_empty());
        assert_eq!(m.state(), &State::Inserting { session: s });
    }

    #[test]
    fn stale_results_are_ignored() {
        let mut m = Machine::new();
        let first = to_processing(&mut m);
        m.handle(Input::Cancel);
        let second = to_processing(&mut m);
        assert_ne!(first, second);
        // The cancelled session's transcript arrives late.
        assert!(
            m.handle(Input::TranscriptReady {
                session: first,
                text: "stale".into()
            })
            .is_empty()
        );
        assert_eq!(m.state(), &State::Processing { session: second });
        // A stale dismiss timer must not hide a newer session.
        assert!(m.handle(Input::Dismiss { session: first }).is_empty());
    }

    #[test]
    fn empty_transcript_becomes_no_speech_error() {
        let mut m = Machine::new();
        let s = to_processing(&mut m);
        let fx = m.handle(Input::TranscriptReady {
            session: s,
            text: "   ".into(),
        });
        assert!(matches!(fx.as_slice(), [Effect::ScheduleDismiss { .. }]));
        assert!(matches!(
            m.state(),
            State::Error {
                error: DictationError {
                    kind: ErrorKind::NoSpeech,
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn capture_failure_during_recording() {
        let mut m = Machine::new();
        m.handle(Input::ShortcutPressed);
        m.handle(Input::CaptureFailed {
            session: 1,
            error: err(ErrorKind::Microphone),
        });
        assert!(matches!(m.state(), State::Error { session: 1, .. }));
        // The user can retry immediately.
        m.handle(Input::ShortcutPressed);
        assert_eq!(m.state(), &State::Recording { session: 2 });
    }

    #[test]
    fn empty_capture_after_release() {
        let mut m = Machine::new();
        let s = to_processing(&mut m);
        m.handle(Input::CaptureEmpty {
            session: s,
            error: err(ErrorKind::TooShort),
        });
        assert!(matches!(m.state(), State::Error { .. }));
    }

    #[test]
    fn transcription_failure_is_recoverable() {
        let mut m = Machine::new();
        let s = to_processing(&mut m);
        m.handle(Input::TranscriptionFailed {
            session: s,
            error: err(ErrorKind::ModelUnavailable),
        });
        assert!(matches!(m.state(), State::Error { .. }));
        assert_eq!(
            m.handle(Input::Dismiss { session: s }),
            vec![Effect::HideOverlay]
        );
        assert_eq!(m.state(), &State::Idle);
    }

    #[test]
    fn insertion_failure_retains_transcript_for_manual_copy() {
        let mut m = Machine::new();
        let s = to_processing(&mut m);
        m.handle(Input::TranscriptReady {
            session: s,
            text: "keep me".into(),
        });
        m.handle(Input::InsertionFailed {
            session: s,
            error: err(ErrorKind::Insertion),
            text: "keep me".into(),
        });
        assert_eq!(m.retained_transcript(), Some("keep me"));
        let view = serde_json::to_value(m.state().view()).unwrap();
        assert_eq!(view["phase"], "error");
        assert_eq!(view["hasTranscript"], true);
        assert!(
            !view.to_string().contains("keep me"),
            "view must not leak text"
        );
    }

    #[test]
    fn cancel_during_recording_aborts_capture() {
        let mut m = Machine::new();
        m.handle(Input::ShortcutPressed);
        assert_eq!(
            m.handle(Input::Cancel),
            vec![Effect::AbortCapture { session: 1 }, Effect::HideOverlay]
        );
        assert_eq!(m.state(), &State::Idle);
    }

    #[test]
    fn cancel_during_processing_cancels_transcription() {
        let mut m = Machine::new();
        let s = to_processing(&mut m);
        assert_eq!(
            m.handle(Input::Cancel),
            vec![
                Effect::CancelTranscription { session: s },
                Effect::HideOverlay
            ]
        );
    }

    #[test]
    fn cancel_during_insertion_is_deferred() {
        let mut m = Machine::new();
        let s = to_processing(&mut m);
        m.handle(Input::TranscriptReady {
            session: s,
            text: "x".into(),
        });
        assert!(m.handle(Input::Cancel).is_empty());
        assert_eq!(m.state(), &State::Inserting { session: s });
    }

    #[test]
    fn shutdown_from_every_state_returns_to_idle() {
        for steps in 0..4 {
            let mut m = Machine::new();
            if steps >= 1 {
                m.handle(Input::ShortcutPressed);
            }
            if steps >= 2 {
                m.handle(Input::ShortcutReleased);
            }
            if steps >= 3 {
                m.handle(Input::TranscriptReady {
                    session: 1,
                    text: "x".into(),
                });
            }
            m.handle(Input::Shutdown);
            assert_eq!(m.state(), &State::Idle, "after {steps} steps");
        }
    }

    #[test]
    fn view_serialises_with_phase_tag() {
        let mut m = Machine::new();
        m.handle(Input::ShortcutPressed);
        let v = serde_json::to_value(m.state().view()).unwrap();
        assert_eq!(v, serde_json::json!({"phase": "recording", "session": 1}));
    }
}
