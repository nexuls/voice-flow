//! Integration tests against the real Cactus Whistle model.
//!
//! They need the model file, so they are ignored by default:
//!
//!     cargo test --test whistle -- --ignored --nocapture
//!
//! The model is looked up at `$VOICEFLOW_TEST_MODEL`, then at the app's default
//! install location (`~/.local/share/dev.voiceflow.app/models/whistle.cact` on Linux).
//!
//! Needle state is process-global, so everything runs in one test, in order.
#![cfg(feature = "needle")]

use std::path::{Path, PathBuf};
use std::time::Instant;

use voiceflow_lib::transcription::model::{ModelStore, WHISTLE};
use voiceflow_lib::transcription::needle::NeedleEngine;
use voiceflow_lib::transcription::{
    EngineError, Language, TranscribeOptions, TranscriptionEngine, TranscriptionService,
};

fn model_path() -> PathBuf {
    if let Some(p) = std::env::var_os("VOICEFLOW_TEST_MODEL") {
        return p.into();
    }
    let home = std::env::var_os("HOME").expect("HOME");
    PathBuf::from(home).join(".local/share/dev.voiceflow.app/models/whistle.cact")
}

fn read_wav(path: &Path) -> Vec<f32> {
    let mut r = hound::WavReader::open(path).unwrap();
    let spec = r.spec();
    assert_eq!(
        (spec.sample_rate, spec.channels),
        (16_000, 1),
        "fixture format"
    );
    r.samples::<i16>()
        .map(|s| s.unwrap() as f32 / i16::MAX as f32)
        .collect()
}

fn fixture(name: &str) -> Vec<f32> {
    read_wav(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
}

fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

#[test]
#[ignore = "needs the Whistle model; see module docs"]
fn whistle_end_to_end() {
    let path = model_path();
    assert!(
        path.exists(),
        "model not found at {}; install it or set VOICEFLOW_TEST_MODEL",
        path.display()
    );
    // The installed file must pass the same checksum check as the app.
    let store = ModelStore::new(path.parent().unwrap());
    if path.file_name().unwrap() == WHISTLE.file_name {
        store.verify(&WHISTLE).expect("model checksum");
    }

    let mut engine = NeedleEngine::new().unwrap();
    assert!(matches!(NeedleEngine::new(), Err(EngineError::EngineBusy)));
    let en = fixture("quick_brown_fox_en.wav");

    // Not loaded yet.
    assert_eq!(
        engine
            .transcribe(&en, &TranscribeOptions::default())
            .unwrap_err(),
        EngineError::NotLoaded
    );

    // Missing model.
    assert_eq!(
        engine
            .load(Path::new("/nonexistent/whistle.cact"))
            .unwrap_err(),
        EngineError::ModelNotInstalled
    );

    // Corrupt model: a truncated copy of the real file.
    let dir = tempfile::tempdir().unwrap();
    let corrupt = dir.path().join("corrupt.cact");
    std::fs::write(&corrupt, &std::fs::read(&path).unwrap()[..4096]).unwrap();
    let err = engine.load(&corrupt).unwrap_err();
    assert!(
        matches!(
            err,
            EngineError::ModelCorrupt(_) | EngineError::LoadFailed(_)
        ),
        "{err:?}"
    );
    assert!(!engine.is_loaded());

    // Real model.
    let t = Instant::now();
    engine.load(&path).unwrap();
    eprintln!("load: {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    assert!(engine.is_loaded());

    // Known recording.
    let tr = engine
        .transcribe(&en, &TranscribeOptions::default())
        .unwrap();
    eprintln!("en: {:?} -> {:?}", tr.timing, tr.text);
    assert_eq!(tr.language.as_deref(), Some("en"));
    let got = words(&tr.text);
    for w in [
        "quick", "brown", "fox", "jumps", "lazy", "dog", "report", "friday",
    ] {
        assert!(
            got.contains(&w.to_owned()),
            "missing {w:?} in {:?}",
            tr.text
        );
    }

    // Language detection metadata.
    let de = fixture("guten_morgen_de.wav");
    let tr = engine
        .transcribe(&de, &TranscribeOptions::default())
        .unwrap();
    assert_eq!(tr.language.as_deref(), Some("de"));
    assert!(
        words(&tr.text).contains(&"morgen".to_owned()),
        "{:?}",
        tr.text
    );

    // A forced language is honoured.
    let forced = TranscribeOptions {
        language: Some(Language::En),
        ..Default::default()
    };
    let tr = engine.transcribe(&en, &forced).unwrap();
    assert_eq!(tr.language.as_deref(), Some("en"));

    // Word timestamps are ordered and inside the clip.
    let timed = TranscribeOptions {
        word_timestamps: true,
        ..Default::default()
    };
    let tr = engine.transcribe(&en, &timed).unwrap();
    let ws = tr.words.expect("words");
    assert!(ws.len() >= 10);
    let clip_secs = en.len() as f32 / 16_000.0;
    for pair in ws.windows(2) {
        assert!(pair[0].start <= pair[1].start);
    }
    assert!(
        ws.iter()
            .all(|w| w.start >= 0.0 && w.end <= clip_secs + 0.5)
    );

    // Silence and empty audio.
    for (name, audio) in [("silence", vec![0.0f32; 32_000]), ("empty", vec![])] {
        let tr = engine
            .transcribe(&audio, &TranscribeOptions::default())
            .unwrap();
        assert!(tr.text.is_empty(), "{name}: {:?}", tr.text);
    }

    // Long audio (> 30 s) is windowed by the engine.
    let long: Vec<f32> = std::iter::repeat_n(en.as_slice(), 6)
        .flatten()
        .copied()
        .collect();
    let tr = engine
        .transcribe(&long, &TranscribeOptions::default())
        .unwrap();
    eprintln!(
        "long ({:.1} s): {:?}",
        long.len() as f32 / 16_000.0,
        tr.timing
    );
    let n_fox = words(&tr.text).iter().filter(|w| *w == "fox").count();
    assert!(
        n_fox >= 5,
        "expected ~6 repetitions, got {n_fox}: {:?}",
        tr.text
    );

    // Benchmark: inference time by recording length (first run already warm).
    for secs in [1usize, 3, 5, 10] {
        let n = (secs * 16_000).min(long.len());
        let tr = engine
            .transcribe(&long[..n], &TranscribeOptions::default())
            .unwrap();
        eprintln!(
            "bench {secs:>2} s audio: inference {:>7.1} ms, ttft {:>6.1} ms, {:>6.1} tok/s",
            tr.timing.inference_ms, tr.timing.ttft_ms, tr.timing.decode_tps
        );
    }

    // The engine works behind the service (dedicated thread).
    drop(engine);
    let svc = TranscriptionService::spawn(Box::new(NeedleEngine::new().unwrap()));
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        svc.load(path.clone()).await.unwrap();
        let (_c, rx) = svc.transcribe(en.clone(), TranscribeOptions::default());
        let tr = rx.await.unwrap().unwrap();
        assert!(words(&tr.text).contains(&"fox".to_owned()));
    });
}
