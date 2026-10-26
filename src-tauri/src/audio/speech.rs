//! Energy-based speech detection on finished 16 kHz recordings.
//!
//! Used for two things:
//! 1. **Silence trimming.** Leading and trailing silence is cut (with padding)
//!    before inference. Whistle's latency grows sharply past about 5 s of audio.
//! 2. **No-speech gate.** Recordings with no frame clearly above the noise floor
//!    are not transcribed. Whistle can invent text for noise.
//!
//! The threshold adapts to the recording: a low percentile of frame energy
//! estimates the noise floor, and speech must be about 12 dB above it.

/// 20 ms frames at 16 kHz.
pub const FRAME: usize = 320;
/// Kept around the detected speech so word onsets and endings are not clipped.
pub const PAD_SAMPLES: usize = 16_000 * 300 / 1000;
/// Minimum total speech, in frames (200 ms), to count as speech.
pub const MIN_SPEECH_FRAMES: usize = 10;
/// Speech must exceed the noise floor by this factor (about 12 dB).
pub const FLOOR_FACTOR: f32 = 4.0;
/// Absolute minimum threshold (about -52 dBFS), for near-silent noise floors.
pub const MIN_THRESHOLD: f32 = 0.0025;
/// Upper bound for the floor estimate, so continuous speech without pauses
/// does not raise the threshold above the speech itself.
pub const MAX_FLOOR: f32 = 0.02;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeechRegion {
    /// Sample range to transcribe, padding included.
    pub start: usize,
    pub end: usize,
    pub speech_frames: usize,
    pub noise_floor: f32,
    pub threshold: f32,
}

fn frame_rms(samples: &[f32]) -> Vec<f32> {
    samples
        .chunks(FRAME)
        .map(|f| (f.iter().map(|s| s * s).sum::<f32>() / f.len() as f32).sqrt())
        .collect()
}

/// Finds the speech region, or `None` if the recording holds no speech.
pub fn detect(samples: &[f32]) -> Option<SpeechRegion> {
    let rms = frame_rms(samples);
    if rms.is_empty() {
        return None;
    }
    let mut sorted = rms.clone();
    sorted.sort_by(f32::total_cmp);
    let floor = sorted[sorted.len() / 10].min(MAX_FLOOR);
    let threshold = (floor * FLOOR_FACTOR).max(MIN_THRESHOLD);

    let loud: Vec<usize> = rms
        .iter()
        .enumerate()
        .filter(|(_, r)| **r > threshold)
        .map(|(i, _)| i)
        .collect();
    if loud.len() < MIN_SPEECH_FRAMES {
        return None;
    }
    let first = *loud.first().expect("non-empty");
    let last = *loud.last().expect("non-empty");
    Some(SpeechRegion {
        start: (first * FRAME).saturating_sub(PAD_SAMPLES),
        end: ((last + 1) * FRAME + PAD_SAMPLES).min(samples.len()),
        speech_frames: loud.len(),
        noise_floor: floor,
        threshold,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random noise in [-amp, amp].
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

    /// Syllable-like bursts: 150 ms of tone, 100 ms gap.
    fn speechy(n: usize, amp: f32) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let in_burst = (i % 4000) < 2400;
                if in_burst {
                    (i as f32 * 0.09).sin() * amp
                } else {
                    0.0
                }
            })
            .collect()
    }

    fn mix(a: &[f32], b: &[f32]) -> Vec<f32> {
        a.iter().zip(b).map(|(x, y)| x + y).collect()
    }

    #[test]
    fn trims_leading_and_trailing_silence_with_padding() {
        let mut audio = noise(48_000, 0.001, 1); // 3 s quiet room
        audio.extend(speechy(32_000, 0.2)); // 2 s speech
        audio.extend(noise(24_000, 0.001, 2)); // 1.5 s quiet
        let r = detect(&audio).unwrap();
        assert!(
            (48_000 - PAD_SAMPLES - FRAME..=48_000 - PAD_SAMPLES + FRAME).contains(&r.start),
            "{r:?}"
        );
        let speech_end = 80_000;
        assert!(
            r.end >= speech_end && r.end <= speech_end + PAD_SAMPLES + 2 * FRAME,
            "{r:?}"
        );
    }

    #[test]
    fn pure_noise_is_not_speech() {
        for amp in [0.0005, 0.01, 0.1] {
            assert_eq!(
                detect(&noise(64_000, amp, 7)),
                None,
                "noise amplitude {amp}"
            );
        }
        assert_eq!(detect(&vec![0.0; 32_000]), None);
        assert_eq!(detect(&[]), None);
    }

    #[test]
    fn quiet_speech_in_silence_is_found() {
        let mut audio = noise(16_000, 0.0003, 3);
        audio.extend(speechy(16_000, 0.01)); // about -43 dBFS peaks
        audio.extend(noise(16_000, 0.0003, 4));
        assert!(detect(&audio).is_some());
    }

    #[test]
    fn speech_over_steady_noise_is_found() {
        let n = 80_000;
        let mut speech = vec![0.0; 16_000];
        speech.extend(speechy(n - 32_000, 0.3));
        speech.extend(vec![0.0; 16_000]);
        let audio = mix(&noise(n, 0.03, 5), &speech);
        let r = detect(&audio).unwrap();
        assert!(r.start > 8_000 && r.end < n, "{r:?}");
    }

    #[test]
    fn continuous_speech_without_pauses_is_kept_whole() {
        let audio: Vec<f32> = (0..64_000).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
        let r = detect(&audio).unwrap();
        assert_eq!((r.start, r.end), (0, audio.len()));
    }

    #[test]
    fn a_single_click_is_not_speech() {
        let mut audio = noise(32_000, 0.001, 9);
        for s in &mut audio[16_000..16_320] {
            *s = 0.8;
        }
        assert_eq!(detect(&audio), None);
    }
}
