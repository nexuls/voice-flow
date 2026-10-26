//! Structured diagnostic logging.
//!
//! Log lines carry timings, states and error kinds only. Audio samples and
//! transcript text must never be logged.

use tracing_subscriber::{EnvFilter, fmt};

/// Installs the global tracing subscriber. Safe to call more than once.
pub fn init() {
    let filter = EnvFilter::try_from_env("VOICEFLOW_LOG")
        .unwrap_or_else(|_| EnvFilter::new("voiceflow_lib=info,warn"));
    let _ = fmt().with_env_filter(filter).with_target(true).try_init();
}
