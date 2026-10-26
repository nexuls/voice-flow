//! Installs the Whistle model into a directory with the app's verified
//! installer (pinned URL plus SHA-256 check).
//!
//!     cargo run --example install_model -- ~/.local/share/dev.voiceflow.app/models

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use voiceflow_lib::transcription::model::{ModelStore, WHISTLE};

fn main() {
    let dir = std::env::args().nth(1).expect("usage: install_model <dir>");
    let store = ModelStore::new(dir);
    eprintln!("status: {:?}", store.status(&WHISTLE));
    let cancel = Arc::new(AtomicBool::new(false));
    let mut last = 0;
    let path = store
        .install(&WHISTLE, &cancel, |done, total| {
            let pct = done * 100 / total;
            if pct >= last + 10 {
                last = pct;
                eprintln!("{pct}%");
            }
        })
        .expect("install");
    eprintln!("installed and verified: {}", path.display());
}
