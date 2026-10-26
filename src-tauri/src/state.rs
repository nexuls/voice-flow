//! Application state shared with Tauri commands.

use std::sync::Mutex;

use crate::shortcuts::ShortcutManager;
use crate::workflow::WorkflowHandle;

pub struct AppState {
    pub workflow: WorkflowHandle,
    pub shortcuts: Mutex<ShortcutManager>,
}
