//! Application state shared with Tauri commands.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use crate::config::{Settings, SettingsStore};
use crate::insertion::InsertionWorker;
use crate::live::{LiveConfig, Services};
use crate::overlay::OverlayController;
use crate::shortcuts::ShortcutManager;
use crate::transcription::TranscriptionService;
use crate::transcription::model::ModelStore;
use crate::workflow::WorkflowHandle;

pub struct AppState {
    pub workflow: WorkflowHandle,
    pub shortcuts: Mutex<ShortcutManager>,
    pub insertion: Arc<InsertionWorker>,
    pub live_config: Arc<RwLock<LiveConfig>>,
    /// `None` in diagnostic mode.
    pub services: Option<Arc<Services>>,
    pub overlay: Arc<dyn OverlayController>,
    pub settings: RwLock<Settings>,
    pub settings_store: SettingsStore,
}

/// Speech model files, engine and the install in progress, if any.
pub struct ModelState {
    pub store: ModelStore,
    pub transcription: Arc<TranscriptionService>,
    pub engine_ready: Arc<AtomicBool>,
    /// Cancel flag of the running install. `Some` while an install is active.
    pub install: Mutex<Option<Arc<AtomicBool>>>,
}
