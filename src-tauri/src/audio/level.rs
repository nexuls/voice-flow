//! RMS level metering for the visualizer.

/// Level reported at or below this RMS (dBFS).
pub const FLOOR_DB: f32 = -55.0;
/// Level reported as full scale at or above this RMS (dBFS). Typical close-mic
/// speech peaks around -15 to -10 dBFS.
pub const CEIL_DB: f32 = -10.0;

/// Maps an RMS amplitude to a perceptual level in [0, 1].
pub fn rms_to_level(rms: f32) -> f32 {
    if rms <= 0.0 || !rms.is_finite() {
        return 0.0;
    }
    let db = 20.0 * rms.log10();
    ((db - FLOOR_DB) / (CEIL_DB - FLOOR_DB)).clamp(0.0, 1.0)
}

/// Accumulates mono samples and yields one level per fixed-size window.
#[derive(Debug)]
pub struct LevelMeter {
    window: usize,
    sum_sq: f64,
    count: usize,
}

impl LevelMeter {
    /// `window` is the number of samples per reported level, for example
    /// 48000 / 30 for 30 updates a second at 48 kHz.
    pub fn new(window: usize) -> Self {
        Self {
            window: window.max(1),
            sum_sq: 0.0,
            count: 0,
        }
    }

    pub fn for_rate(rate: u32, updates_per_sec: u32) -> Self {
        Self::new((rate / updates_per_sec.max(1)) as usize)
    }

    /// Feeds samples. Calls `emit` once for every completed window.
    pub fn push(&mut self, samples: &[f32], mut emit: impl FnMut(f32)) {
        for &s in samples {
            self.sum_sq += (s as f64) * (s as f64);
            self.count += 1;
            if self.count == self.window {
                let rms = (self.sum_sq / self.count as f64).sqrt() as f32;
                emit(rms_to_level(rms));
                self.sum_sq = 0.0;
                self.count = 0;
            }
        }
    }
}

/// Summary statistics of a finished recording.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SignalStats {
    pub peak: f32,
    pub rms: f32,
}

impl SignalStats {
    pub fn of(samples: &[f32]) -> Self {
        if samples.is_empty() {
            return Self::default();
        }
        let mut peak = 0f32;
        let mut sum_sq = 0f64;
        for &s in samples {
            peak = peak.max(s.abs());
            sum_sq += (s as f64) * (s as f64);
        }
        Self {
            peak,
            rms: (sum_sq / samples.len() as f64).sqrt() as f32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_mapping() {
        assert_eq!(rms_to_level(0.0), 0.0);
        assert_eq!(rms_to_level(f32::NAN), 0.0);
        assert_eq!(rms_to_level(1e-6), 0.0);
        assert_eq!(rms_to_level(1.0), 1.0);
        // Midpoint of the dB range.
        let mid = 10f32.powf((FLOOR_DB + CEIL_DB) / 2.0 / 20.0);
        assert!((rms_to_level(mid) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn meter_emits_once_per_window_across_buffers() {
        let mut m = LevelMeter::new(100);
        let mut levels = Vec::new();
        m.push(&[0.5; 150], |l| levels.push(l));
        assert_eq!(levels.len(), 1);
        m.push(&[0.5; 49], |l| levels.push(l));
        assert_eq!(levels.len(), 1, "window not yet complete");
        m.push(&[0.5; 1], |l| levels.push(l));
        assert_eq!(levels.len(), 2);
        assert!(levels.iter().all(|l| *l > 0.9));
    }

    #[test]
    fn silence_is_zero_level() {
        let mut m = LevelMeter::for_rate(48_000, 30);
        let mut levels = Vec::new();
        m.push(&vec![0.0; 48_000], |l| levels.push(l));
        assert_eq!(levels.len(), 30);
        assert!(levels.iter().all(|l| *l == 0.0));
    }

    #[test]
    fn stats() {
        assert_eq!(SignalStats::of(&[]), SignalStats::default());
        let s = SignalStats::of(&[0.5, -1.0, 0.5, 0.0]);
        assert_eq!(s.peak, 1.0);
        assert!((s.rms - (1.5f32 / 4.0).sqrt()).abs() < 1e-6);
    }
}
