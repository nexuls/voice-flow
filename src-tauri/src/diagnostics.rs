//! Privacy-preserving diagnostics: per-dictation timings and outcomes, kept
//! in memory only. Records never contain audio, transcripts, window titles or
//! device names.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;

use crate::live::SessionTimings;
use crate::workflow::{SessionId, StateView};

pub const MAX_RECORDS: usize = 50;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub session: SessionId,
    /// `pasted`, `copiedToClipboard`, or an error kind such as `noSpeech`.
    pub outcome: Option<String>,
    pub timings: Option<SessionTimings>,
}

#[derive(Default)]
pub struct DiagnosticsLog {
    records: Mutex<VecDeque<SessionRecord>>,
}

impl DiagnosticsLog {
    fn upsert(&self, session: SessionId, f: impl FnOnce(&mut SessionRecord)) {
        let mut r = self.records.lock().expect("diagnostics lock");
        if let Some(rec) = r.iter_mut().find(|x| x.session == session) {
            f(rec);
            return;
        }
        let mut rec = SessionRecord {
            session,
            ..Default::default()
        };
        f(&mut rec);
        r.push_back(rec);
        while r.len() > MAX_RECORDS {
            r.pop_front();
        }
    }

    pub fn record_timings(&self, t: &SessionTimings) {
        self.upsert(t.session, |r| r.timings = Some(t.clone()));
    }

    /// Records the final state of a session from a transition target.
    pub fn record_state(&self, state: &StateView) {
        let (session, outcome) = match state {
            StateView::Success { session, outcome } => (
                *session,
                serde_json::to_value(outcome)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned)),
            ),
            StateView::Error { session, error, .. } => (
                *session,
                serde_json::to_value(error.kind)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned)),
            ),
            _ => return,
        };
        self.upsert(session, |r| r.outcome = outcome);
    }

    /// Most recent first.
    pub fn records(&self) -> Vec<SessionRecord> {
        let r = self.records.lock().expect("diagnostics lock");
        r.iter().rev().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::{DictationError, ErrorKind, InsertOutcome};

    #[test]
    fn merges_timings_and_outcomes_and_caps_size() {
        let log = DiagnosticsLog::default();
        log.record_state(&StateView::Success {
            session: 1,
            outcome: InsertOutcome::Pasted,
        });
        log.record_timings(&SessionTimings {
            session: 1,
            inference_ms: Some(10.0),
            ..Default::default()
        });
        let r = log.records();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].outcome.as_deref(), Some("pasted"));
        assert!(r[0].timings.is_some());

        for s in 2..200 {
            log.record_state(&StateView::Error {
                session: s,
                error: DictationError::new(ErrorKind::NoSpeech, "x"),
                has_transcript: false,
            });
        }
        let r = log.records();
        assert_eq!(r.len(), MAX_RECORDS);
        assert_eq!(r[0].session, 199);
        assert_eq!(r[0].outcome.as_deref(), Some("noSpeech"));
    }

    #[test]
    fn ignores_non_final_states() {
        let log = DiagnosticsLog::default();
        log.record_state(&StateView::Recording { session: 1 });
        assert!(log.records().is_empty());
    }
}
