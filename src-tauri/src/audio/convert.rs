//! Audio format conversion: channel downmix and resampling to 16 kHz.
//!
//! Runs on the capture worker and after recording, never in the device
//! callback.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

/// Sample rate required by Whistle.
pub const TARGET_RATE: u32 = 16_000;

/// Averages interleaved frames into mono and appends them to `out`.
/// Trailing samples that do not form a whole frame are ignored.
pub fn downmix_into(interleaved: &[f32], channels: usize, out: &mut Vec<f32>) {
    debug_assert!(channels > 0);
    if channels == 1 {
        out.extend_from_slice(interleaved);
        return;
    }
    let scale = 1.0 / channels as f32;
    out.extend(
        interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() * scale),
    );
}

#[derive(Debug, thiserror::Error)]
#[error("resampling failed: {0}")]
pub struct ResampleError(String);

/// Resamples mono audio from `rate` to 16 kHz with an FFT resampler, which
/// includes anti-aliasing. Output length is `ceil(len * 16000 / rate)`, and the
/// resampler's delay is trimmed, so sample `i` keeps its timestamp.
pub fn resample_to_16k(input: &[f32], rate: u32) -> Result<Vec<f32>, ResampleError> {
    if rate == TARGET_RATE || input.is_empty() {
        return Ok(input.to_vec());
    }
    let err = |e: &dyn std::fmt::Display| ResampleError(e.to_string());
    let mut resampler = Fft::<f32>::new(
        rate as usize,
        TARGET_RATE as usize,
        1024,
        1,
        FixedSync::Input,
    )
    .map_err(|e| err(&e))?;
    let needed = resampler.process_all_needed_output_len(input.len());
    let mut output = vec![0.0f32; needed];
    let in_adapter = InterleavedSlice::new(input, 1, input.len()).map_err(|e| err(&e))?;
    let mut out_adapter = InterleavedSlice::new_mut(&mut output, 1, needed).map_err(|e| err(&e))?;
    let (_, produced) = resampler
        .process_all_into_buffer(&in_adapter, &mut out_adapter, input.len(), None)
        .map_err(|e| err(&e))?;
    let expected = (input.len() as u64 * TARGET_RATE as u64).div_ceil(rate as u64) as usize;
    output.truncate(produced.min(expected));
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: u32, secs: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin() * 0.5)
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
    }

    /// Amplitude of `freq` in `x` via a single-bin DFT.
    fn tone_amplitude(x: &[f32], freq: f32, rate: u32) -> f32 {
        let (mut re, mut im) = (0f64, 0f64);
        for (i, s) in x.iter().enumerate() {
            let ph = 2.0 * std::f64::consts::PI * freq as f64 * i as f64 / rate as f64;
            re += *s as f64 * ph.cos();
            im += *s as f64 * ph.sin();
        }
        (2.0 * (re * re + im * im).sqrt() / x.len() as f64) as f32
    }

    #[test]
    fn downmix_averages_channels() {
        let mut out = Vec::new();
        downmix_into(&[1.0, 0.0, 0.5, 0.5, -1.0, 1.0], 2, &mut out);
        assert_eq!(out, vec![0.5, 0.5, 0.0]);
    }

    #[test]
    fn downmix_mono_is_copy_and_ignores_partial_frames() {
        let mut out = vec![9.0];
        downmix_into(&[0.1, 0.2], 1, &mut out);
        assert_eq!(out, vec![9.0, 0.1, 0.2]);
        let mut out = Vec::new();
        downmix_into(&[1.0, 1.0, 1.0, 1.0, 1.0], 4, &mut out);
        assert_eq!(out, vec![1.0]);
    }

    #[test]
    fn resample_preserves_duration_for_common_rates() {
        for rate in [8_000, 22_050, 44_100, 48_000, 96_000] {
            let input = sine(440.0, rate, 1.5);
            let out = resample_to_16k(&input, rate).unwrap();
            let expected = (input.len() as u64 * 16_000).div_ceil(rate as u64) as usize;
            assert_eq!(out.len(), expected, "rate {rate}");
        }
    }

    #[test]
    fn resample_preserves_in_band_tone() {
        let out = resample_to_16k(&sine(1000.0, 48_000, 1.0), 48_000).unwrap();
        // Skip edges, where the FFT blocks ramp in and out.
        let mid = &out[2000..14000];
        let amp = tone_amplitude(mid, 1000.0, 16_000);
        assert!((amp - 0.5).abs() < 0.02, "amplitude {amp}");
    }

    #[test]
    fn resample_removes_content_above_nyquist() {
        // 12 kHz is above the 8 kHz Nyquist limit of 16 kHz audio. Without
        // anti-aliasing it would fold to 4 kHz.
        let out = resample_to_16k(&sine(12_000.0, 48_000, 1.0), 48_000).unwrap();
        let mid = &out[2000..14000];
        assert!(rms(mid) < 0.01, "residual rms {}", rms(mid));
        assert!(tone_amplitude(mid, 4000.0, 16_000) < 0.01);
    }

    #[test]
    fn resample_keeps_time_alignment() {
        // An impulse at 0.5 s must stay at about 0.5 s after resampling.
        let mut input = vec![0.0f32; 48_000];
        input[24_000] = 1.0;
        let out = resample_to_16k(&input, 48_000).unwrap();
        let peak = out
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .unwrap()
            .0;
        assert!((peak as i64 - 8_000).abs() <= 2, "peak at {peak}");
    }

    #[test]
    fn resample_identity_and_empty() {
        let input = sine(300.0, 16_000, 0.1);
        assert_eq!(resample_to_16k(&input, 16_000).unwrap(), input);
        assert!(resample_to_16k(&[], 48_000).unwrap().is_empty());
    }

    #[test]
    fn resample_handles_tiny_inputs() {
        let out = resample_to_16k(&[0.1, 0.2, 0.3], 48_000).unwrap();
        assert_eq!(out.len(), 1);
    }
}
