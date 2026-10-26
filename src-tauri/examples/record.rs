//! Development aid: records from the microphone through the app's capture
//! pipeline and writes a 16 kHz mono WAV.
//!
//!     cargo run --example record -- out.wav 3
//!
//! The output is exactly what the speech engine receives. Never commit it.

use std::sync::Arc;
use std::time::Duration;

use voiceflow_lib::audio::cpal_capture::CpalCapture;
use voiceflow_lib::audio::{AudioCaptureService, CaptureConfig, CaptureEvent, CaptureEvents};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: record <out.wav> [seconds]");
    let secs: f64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(3.0);

    let events: CaptureEvents = Arc::new(|e| match e {
        CaptureEvent::Level(l) => eprint!("\r{:<40}", "#".repeat((l * 40.0) as usize)),
        other => eprintln!("\n{other:?}"),
    });
    let mut cap = CpalCapture::new();
    let info = cap
        .start(&CaptureConfig::default(), events)
        .expect("start capture");
    eprintln!("recording {secs}s from {info:?}");
    std::thread::sleep(Duration::from_secs_f64(secs));
    let rec = cap.finish().expect("finish capture");
    eprintln!("\n{rec:?} convert_ms={:.2}", rec.convert_ms);

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(&path, spec).expect("create wav");
    for s in &rec.samples {
        w.write_sample(*s).expect("write sample");
    }
    w.finalize().expect("finalize wav");
    eprintln!("wrote {path}");
}
