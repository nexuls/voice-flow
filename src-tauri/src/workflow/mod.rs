//! Dictation workflow: the state machine and the controller that drives it.

pub mod controller;
pub mod machine;

pub use controller::{
    DiagnosticExecutor, Executor, LevelSink, StateObserver, Transition, WorkflowHandle,
};
pub use machine::{DictationError, Effect, ErrorKind, Input, InsertOutcome, SessionId, StateView};
