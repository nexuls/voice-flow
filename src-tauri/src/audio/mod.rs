//! Microphone capture.
//!
//! The pipeline has three stages:
//! 1. **Device callback** (`cpal_capture`): converts samples to f32 and pushes
//!    whole frames into a lock-free SPSC ring buffer. No locks and no
//!    allocation. If the ring is full, the frames are counted as dropped.
//! 2. **Capture worker**: drains the ring every 10 ms into an [`Accumulator`],
//!    which downmixes to mono, meters levels for the visualizer, fills dropped
//!    gaps with silence (to keep timing consistent) and enforces the limits.
//! 3. **Finish**: resamples the native-rate mono buffer to 16 kHz with
//!    anti-aliasing ([`convert::resample_to_16k`]).
//!
//! Audio buffers are zeroed on drop.

pub mod convert;
pub mod cpal_capture;
pub mod level;

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use zeroize::Zeroize;

use level::{LevelMeter, SignalStats};

/// Visualizer updates per second.
pub const LEVEL_UPDATES_PER_SEC: u32 = 30;
/// Hard upper bound for any recording, whatever the configuration.
pub const ABSOLUTE_MAX_DURATION: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, PartialEq)]
pub struct CaptureConfig {
    /// cpal device id (`DeviceId` display form), or `None` for the default device.
    pub device_id: Option<String>,
    pub max_duration: Duration,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            device_id: None,
            max_duration: Duration::from_secs(120),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CaptureError {
    #[error("no microphone found")]
    NoDevice,
    #[error("microphone access denied")]
    PermissionDenied,
    #[error("microphone is busy")]
    DeviceBusy,
    #[error("microphone disconnected")]
    Disconnected,
    #[error("microphone format not supported: {0}")]
    Unsupported(String),
    #[error("a recording is already in progress")]
    AlreadyRecording,
    #[error("no recording in progress")]
    NotRecording,
    #[error("microphone did not start in time")]
    Timeout,
    #[error("audio error: {0}")]
    Backend(String),
}

#[derive(Debug, Clone)]
pub enum CaptureEvent {
    /// Visualizer level in [0, 1], about [`LEVEL_UPDATES_PER_SEC`] times a second.
    Level(f32),
    /// The maximum duration was reached. Capture continues to run, but further
    /// audio is discarded until `finish` is called.
    LimitReached,
    /// Fatal stream error, for example a disconnected device.
    Failed(CaptureError),
}

pub type CaptureEvents = Arc<dyn Fn(CaptureEvent) + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureInfo {
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: String,
    /// True if the configured device was missing and the default was used.
    pub fell_back_to_default: bool,
    /// Time from `start` to a running stream.
    pub open_ms: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// A finished recording: 16 kHz mono f32 in [-1, 1].
pub struct Recording {
    pub samples: Vec<f32>,
    pub source_rate: u32,
    pub source_channels: u16,
    /// Frames lost to ring-buffer overflow, replaced by silence.
    pub dropped_frames: u64,
    pub limit_reached: bool,
    pub stats: SignalStats,
    /// Time spent converting to 16 kHz in `finish`.
    pub convert_ms: f64,
}

impl Recording {
    pub fn duration(&self) -> Duration {
        Duration::from_secs_f64(self.samples.len() as f64 / convert::TARGET_RATE as f64)
    }
}

impl std::fmt::Debug for Recording {
    // Never print samples.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recording")
            .field("duration", &self.duration())
            .field("source_rate", &self.source_rate)
            .field("dropped_frames", &self.dropped_frames)
            .field("stats", &self.stats)
            .finish()
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.samples.zeroize();
    }
}

/// Captures one recording at a time.
pub trait AudioCaptureService: Send {
    /// Opens the device and starts capturing. Fails with
    /// [`CaptureError::AlreadyRecording`] if a capture is active.
    fn start(
        &mut self,
        config: &CaptureConfig,
        events: CaptureEvents,
    ) -> Result<CaptureInfo, CaptureError>;
    /// Stops capture, releases the device and returns 16 kHz mono audio.
    fn finish(&mut self) -> Result<Recording, CaptureError>;
    /// Stops capture, releases the device and discards the audio.
    fn abort(&mut self);
    fn is_active(&self) -> bool;
}

/// Native-rate mono buffer with limits, gap filling and metering.
pub struct Accumulator {
    channels: usize,
    rate: u32,
    max_frames: usize,
    mono: Vec<f32>,
    meter: LevelMeter,
    limit_reached: bool,
    dropped_frames: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    Ok,
    /// The limit was hit during this push. Reported once.
    LimitReached,
}

impl Accumulator {
    pub fn new(rate: u32, channels: u16, max_duration: Duration) -> Self {
        let max = max_duration.min(ABSOLUTE_MAX_DURATION);
        let max_frames = (max.as_secs_f64() * rate as f64) as usize;
        Self {
            channels: channels.max(1) as usize,
            rate,
            max_frames,
            // Reserve about 10 s up front. Longer recordings grow the vector.
            mono: Vec::with_capacity(max_frames.min(rate as usize * 10)),
            meter: LevelMeter::for_rate(rate, LEVEL_UPDATES_PER_SEC),
            limit_reached: false,
            dropped_frames: 0,
        }
    }

    fn room(&self) -> usize {
        self.max_frames.saturating_sub(self.mono.len())
    }

    fn check_limit(&mut self) -> PushOutcome {
        if !self.limit_reached && self.room() == 0 {
            self.limit_reached = true;
            PushOutcome::LimitReached
        } else {
            PushOutcome::Ok
        }
    }

    /// Appends interleaved samples. The slice must hold whole frames.
    pub fn push_interleaved(&mut self, data: &[f32], emit_level: impl FnMut(f32)) -> PushOutcome {
        if self.limit_reached {
            return PushOutcome::Ok;
        }
        let frames = (data.len() / self.channels).min(self.room());
        let start = self.mono.len();
        convert::downmix_into(&data[..frames * self.channels], self.channels, &mut self.mono);
        self.meter.push(&self.mono[start..], emit_level);
        self.check_limit()
    }

    /// Inserts silence for frames lost to buffer overflow, so later audio keeps
    /// its correct position in time.
    pub fn push_gap(&mut self, frames: u64) -> PushOutcome {
        self.dropped_frames += frames;
        if self.limit_reached {
            return PushOutcome::Ok;
        }
        let n = (frames as usize).min(self.room());
        self.mono.resize(self.mono.len() + n, 0.0);
        self.check_limit()
    }

    pub fn frames(&self) -> usize {
        self.mono.len()
    }

    /// Converts to a 16 kHz [`Recording`].
    pub fn finish(mut self, source_channels: u16) -> Result<Recording, CaptureError> {
        let t = Instant::now();
        let samples = convert::resample_to_16k(&self.mono, self.rate)
            .map_err(|e| CaptureError::Backend(e.to_string()))?;
        let convert_ms = t.elapsed().as_secs_f64() * 1e3;
        self.mono.zeroize();
        Ok(Recording {
            stats: SignalStats::of(&samples),
            samples,
            source_rate: self.rate,
            source_channels,
            dropped_frames: self.dropped_frames,
            limit_reached: self.limit_reached,
            convert_ms,
        })
    }
}

impl Drop for Accumulator {
    fn drop(&mut self) {
        self.mono.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_accumulates_as_mono_with_levels() {
        let mut acc = Accumulator::new(48_000, 2, Duration::from_secs(10));
        let mut levels = 0;
        // 0.5 s of stereo in uneven chunks, crossing level-window boundaries.
        let data = vec![0.25f32; 48_000];
        for chunk in data.chunks(960 * 2 + 2) {
            assert_eq!(acc.push_interleaved(chunk, |_| levels += 1), PushOutcome::Ok);
        }
        assert_eq!(acc.frames(), 24_000);
        assert_eq!(levels, 15, "30 per second for 0.5 s");
        let rec = acc.finish(2).unwrap();
        assert_eq!(rec.samples.len(), 8_000);
        assert!((rec.duration().as_secs_f64() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn limit_is_enforced_and_reported_once() {
        let mut acc = Accumulator::new(16_000, 1, Duration::from_secs(1));
        assert_eq!(acc.push_interleaved(&[0.1; 10_000], |_| {}), PushOutcome::Ok);
        assert_eq!(
            acc.push_interleaved(&[0.1; 10_000], |_| {}),
            PushOutcome::LimitReached
        );
        assert_eq!(acc.push_interleaved(&[0.1; 10_000], |_| {}), PushOutcome::Ok);
        assert_eq!(acc.frames(), 16_000);
        assert!(acc.finish(1).unwrap().limit_reached);
    }

    #[test]
    fn absolute_cap_applies_to_config() {
        let acc = Accumulator::new(16_000, 1, Duration::from_secs(100_000));
        assert_eq!(acc.max_frames, 16_000 * 600);
    }

    #[test]
    fn gaps_preserve_timing() {
        let mut acc = Accumulator::new(16_000, 1, Duration::from_secs(5));
        acc.push_interleaved(&[0.5; 1000], |_| {});
        acc.push_gap(500);
        acc.push_interleaved(&[0.5; 1000], |_| {});
        let rec = acc.finish(1).unwrap();
        assert_eq!(rec.samples.len(), 2500);
        assert_eq!(rec.dropped_frames, 500);
        assert!(rec.samples[1000..1500].iter().all(|s| *s == 0.0));
        assert_eq!(rec.samples[1500], 0.5);
    }

    #[test]
    fn gap_respects_limit() {
        let mut acc = Accumulator::new(16_000, 1, Duration::from_millis(100));
        assert_eq!(acc.push_gap(10_000), PushOutcome::LimitReached);
        assert_eq!(acc.frames(), 1600);
    }

    #[test]
    fn empty_recording() {
        let acc = Accumulator::new(48_000, 2, Duration::from_secs(5));
        let rec = acc.finish(2).unwrap();
        assert!(rec.samples.is_empty());
        assert_eq!(rec.duration(), Duration::ZERO);
        assert_eq!(rec.stats, SignalStats::default());
    }

    #[test]
    fn silence_has_zero_stats() {
        let mut acc = Accumulator::new(44_100, 1, Duration::from_secs(5));
        acc.push_interleaved(&vec![0.0; 44_100], |l| assert_eq!(l, 0.0));
        let rec = acc.finish(1).unwrap();
        assert_eq!(rec.stats.peak, 0.0);
        assert_eq!(rec.samples.len(), 16_000);
    }

    #[test]
    fn debug_never_prints_samples() {
        let mut acc = Accumulator::new(16_000, 1, Duration::from_secs(1));
        acc.push_interleaved(&[0.25; 1000], |_| {});
        let rec = acc.finish(1).unwrap();
        let dbg = format!("{rec:?}");
        assert!(!dbg.contains("samples") && !dbg.contains('['), "{dbg}");
    }
}
