//! Measures the effect of silence trimming and the no-speech gate on real
//! Whistle inference: latency and word error rate against the fixture text.
//!
//!     cargo run --release --example eval_trim -- <model.cact>

use std::path::PathBuf;
use std::time::Instant;

use voiceflow_lib::audio::speech;
use voiceflow_lib::transcription::needle::NeedleEngine;
use voiceflow_lib::transcription::{TranscribeOptions, TranscriptionEngine};

const REFERENCE: &str =
    "the quick brown fox jumps over the lazy dog please send the report by friday";

fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

fn wer(hyp: &str) -> f64 {
    let (r, h) = (words(REFERENCE), words(hyp));
    let mut d: Vec<Vec<usize>> = (0..=r.len()).map(|i| vec![i; h.len() + 1]).collect();
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=r.len() {
        for j in 1..=h.len() {
            let sub = d[i - 1][j - 1] + usize::from(r[i - 1] != h[j - 1]);
            d[i][j] = sub.min(d[i - 1][j] + 1).min(d[i][j - 1] + 1);
        }
    }
    d[r.len()][h.len()] as f64 / r.len() as f64
}

fn noise(n: usize, amp: f32, seed: u32) -> Vec<f32> {
    let mut x = seed.max(1);
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x as f32 / u32::MAX as f32 * 2.0 - 1.0) * amp
        })
        .collect()
}

fn main() {
    let model = PathBuf::from(std::env::args().nth(1).expect("model path"));
    let speech_clip: Vec<f32> = hound::WavReader::open(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/quick_brown_fox_en.wav"),
    )
    .unwrap()
    .samples::<i16>()
    .map(|s| s.unwrap() as f32 / i16::MAX as f32)
    .collect();
    let mut engine = NeedleEngine::new().unwrap();
    engine.load(&model).unwrap();
    let opts = TranscribeOptions::default();
    engine.transcribe(&speech_clip, &opts).unwrap(); // warm-up

    // (name, leading s, trailing s, noise amplitude)
    let cases = [
        ("clean, 2.5 s + 1.5 s silence", 2.5, 1.5, 0.0005),
        ("room noise 0.01, 2.5 s + 1.5 s", 2.5, 1.5, 0.01),
        ("room noise 0.03, 1.0 s + 1.0 s", 1.0, 1.0, 0.03),
    ];
    println!("case,variant,run,secs,inference_ms,wer");
    for (name, lead, trail, amp) in cases {
        let lead_n = (lead * 16_000.0) as usize;
        let trail_n = (trail * 16_000.0) as usize;
        let total = lead_n + speech_clip.len() + trail_n;
        let mut audio = noise(total, amp, 42);
        for (i, s) in speech_clip.iter().enumerate() {
            audio[lead_n + i] += s;
        }
        let region = speech::detect(&audio).expect("speech found");
        let trimmed = &audio[region.start..region.end];
        for run in 0..3 {
            for (variant, a) in [("full", &audio[..]), ("trimmed", trimmed)] {
                let t = Instant::now();
                let tr = engine.transcribe(a, &opts).unwrap();
                println!(
                    "{name},{variant},{run},{:.2},{:.0},{:.3}",
                    a.len() as f64 / 16_000.0,
                    t.elapsed().as_secs_f64() * 1e3,
                    wer(&tr.text)
                );
            }
        }
    }

    // No-speech gate: what does the engine return for noise alone?
    println!("\nnoise_amp,gate,engine_text_chars");
    for (i, amp) in [0.003f32, 0.01, 0.03, 0.06].into_iter().enumerate() {
        let n = noise(16_000 * 4, amp, 100 + i as u32);
        let gate = speech::detect(&n).is_none();
        let tr = engine.transcribe(&n, &opts).unwrap();
        println!(
            "{amp},{},{}",
            if gate { "blocked" } else { "passed" },
            tr.text.chars().count()
        );
    }
}
