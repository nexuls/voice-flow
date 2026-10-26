//! Benchmarks Whistle inference time by audio length.
//!
//!     cargo run --release --example bench_whistle -- <model.cact> <speech.wav> [repeats]
//!
//! The WAV must be 16 kHz mono 16-bit. It is tiled to reach each length.

use std::path::PathBuf;

use voiceflow_lib::transcription::needle::NeedleEngine;
use voiceflow_lib::transcription::{TranscribeOptions, TranscriptionEngine};

fn main() {
    let mut args = std::env::args().skip(1);
    let model = PathBuf::from(args.next().expect("model path"));
    let wav = args.next().expect("wav path");
    let repeats: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(3);
    let base: Vec<f32> = hound::WavReader::open(&wav)
        .unwrap()
        .samples::<i16>()
        .map(|s| s.unwrap() as f32 / i16::MAX as f32)
        .collect();
    let mut engine = NeedleEngine::new().unwrap();
    let t = std::time::Instant::now();
    engine.load(&model).unwrap();
    println!("load_ms {:.1}", t.elapsed().as_secs_f64() * 1e3);
    let tiled: Vec<f32> = base.iter().cycle().take(16_000 * 60).copied().collect();
    // Warm-up run.
    engine
        .transcribe(&tiled[..16_000 * 3], &TranscribeOptions::default())
        .unwrap();
    println!("secs,run,inference_ms,ttft_ms,decode_tps,rtf");
    for secs in [1, 2, 3, 5, 7, 10, 15, 20, 30, 45, 60] {
        for run in 0..repeats {
            let audio = &tiled[..secs * 16_000];
            let tr = engine
                .transcribe(audio, &TranscribeOptions::default())
                .unwrap();
            println!(
                "{secs},{run},{:.1},{:.1},{:.1},{:.3}",
                tr.timing.inference_ms,
                tr.timing.ttft_ms,
                tr.timing.decode_tps,
                tr.timing.inference_ms / (secs as f64 * 1e3)
            );
        }
    }
}
