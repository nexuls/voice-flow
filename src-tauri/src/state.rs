//! Application state shared with Tauri commands.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use crate::shortcuts::ShortcutManager;
use crate::transcription::model::ModelStore;
use crate::workflow::WorkflowHandle;

pub struct AppState {
    pub workflow: WorkflowHandle,
    pub shortcuts: Mutex<ShortcutManager>,
}

/// Model files and the install in progress, if any.
pub struct ModelState {
    pub store: ModelStore,
    /// Cancel flag of the running install. `Some` while an install is active.
    pub install: Mutex<Option<Arc<AtomicBool>>>,
}
