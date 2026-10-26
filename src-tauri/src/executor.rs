//! Application executor: routes workflow effects to the native services.

use std::sync::Arc;

use crate::overlay::OverlayController;
use crate::workflow::{Effect, Executor, WorkflowHandle};

pub struct AppExecutor<E: Executor> {
    overlay: Arc<dyn OverlayController>,
    /// Handles capture, transcription and insertion effects.
    services: E,
}

impl<E: Executor> AppExecutor<E> {
    pub fn new(overlay: Arc<dyn OverlayController>, services: E) -> Self {
        Self { overlay, services }
    }
}

impl<E: Executor> Executor for AppExecutor<E> {
    fn execute(&mut self, effect: Effect, handle: &WorkflowHandle) {
        match effect {
            Effect::ShowOverlay => self.overlay.show(),
            Effect::HideOverlay => self.overlay.hide(),
            other => self.services.execute(other, handle),
        }
    }
}
