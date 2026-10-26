//! Cactus Whistle on the Needle engine, through its official C API (`needle.h`).
//!
//! Needle keeps one process-global, non-thread-safe model per kind. This
//! module therefore allows only one [`NeedleEngine`] per process, and the
//! [`super::TranscriptionService`] calls it from a single thread. Needle has
//! no unload call: model bytes stay alive until the process exits.

use std::ffi::{CStr, CString, c_char, c_int, c_uchar, c_ulonglong};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use serde::Deserialize;

use super::{
    EngineError, TranscribeOptions, Transcript, TranscriptTiming, TranscriptionEngine, Word,
};

mod ffi {
    use super::*;

    pub const NEEDLE_SPEECH: c_int = 2;

    // Signatures exactly as declared in needle.h (Cactus-Compute/needle3).
    unsafe extern "C" {
        pub fn needle_load(cact: *const c_uchar, n: c_ulonglong) -> c_int;
        pub fn needle_models() -> c_int;
        pub fn needle_last_error() -> *const c_char;
        pub fn needle_transcribe(
            pcm: *const f32,
            samples: c_int,
            language: *const c_char,
            keywords: *const c_char,
            word_timestamps: c_int,
            out: *mut c_char,
            out_capacity: c_int,
        ) -> c_int;
    }
}

/// Only one engine may exist, because Needle state is process-global.
static CLAIMED: AtomicBool = AtomicBool::new(false);
/// Model buffers passed to `needle_load`. They are kept alive for the
/// process lifetime because the API does not say whether it keeps the pointer.
static MODEL_BUFFERS: Mutex<Vec<Box<[u8]>>> = Mutex::new(Vec::new());

/// Whistle processes up to 30 s per window and caps each at 320 tokens.
/// Size the JSON buffer generously and retry once if the engine asks for more.
fn initial_capacity(samples: usize, word_timestamps: bool) -> usize {
    let windows = samples / (30 * 16_000) + 1;
    let per_window = if word_timestamps { 48 * 1024 } else { 8 * 1024 };
    (16 * 1024 + windows * per_window).min(i32::MAX as usize)
}

fn last_error() -> String {
    // SAFETY: needle_last_error returns a runtime-owned C string valid until
    // the next API call, or null. We copy it before any further call.
    unsafe {
        let p = ffi::needle_last_error();
        if p.is_null() {
            "unknown engine error".into()
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

/// Parses "output buffer is too small: N bytes needed".
fn required_capacity(message: &str) -> Option<usize> {
    let rest = message.split("too small:").nth(1)?;
    rest.split_whitespace().next()?.parse().ok()
}

#[derive(Deserialize)]
struct RawTranscript {
    text: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    ttft_ms: f64,
    #[serde(default)]
    decode_tps: f64,
    #[serde(default)]
    words: Option<Vec<Word>>,
}

pub struct NeedleEngine {
    loaded: bool,
}

impl NeedleEngine {
    pub fn new() -> Result<Self, EngineError> {
        if CLAIMED.swap(true, Ordering::SeqCst) {
            return Err(EngineError::EngineBusy);
        }
        Ok(Self { loaded: false })
    }
}

impl Drop for NeedleEngine {
    fn drop(&mut self) {
        // The loaded model stays in Needle's global state. A new engine
        // instance may reuse it, so release the claim.
        CLAIMED.store(false, Ordering::SeqCst);
    }
}

impl TranscriptionEngine for NeedleEngine {
    fn name(&self) -> &'static str {
        "cactus-whistle"
    }

    fn is_loaded(&self) -> bool {
        // SAFETY: needle_models has no preconditions.
        self.loaded && unsafe { ffi::needle_models() } & ffi::NEEDLE_SPEECH != 0
    }

    fn load(&mut self, path: &Path) -> Result<(), EngineError> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b.into_boxed_slice(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(EngineError::ModelNotInstalled);
            }
            Err(e) => return Err(EngineError::LoadFailed(e.to_string())),
        };
        // SAFETY: `bytes` is a valid buffer of the given length and is kept
        // alive in MODEL_BUFFERS for the rest of the process.
        let rc = unsafe { ffi::needle_load(bytes.as_ptr(), bytes.len() as c_ulonglong) };
        if rc < 0 {
            let msg = last_error();
            return Err(if msg.contains("invalid") {
                EngineError::ModelCorrupt(msg)
            } else {
                EngineError::LoadFailed(msg)
            });
        }
        // SAFETY: no preconditions.
        if unsafe { ffi::needle_models() } & ffi::NEEDLE_SPEECH == 0 {
            return Err(EngineError::LoadFailed(
                "file is not a speech (Whistle) model".into(),
            ));
        }
        MODEL_BUFFERS.lock().expect("model buffers").push(bytes);
        self.loaded = true;
        Ok(())
    }

    fn transcribe(
        &mut self,
        audio: &[f32],
        options: &TranscribeOptions,
    ) -> Result<Transcript, EngineError> {
        if !self.is_loaded() {
            return Err(EngineError::NotLoaded);
        }
        let samples = c_int::try_from(audio.len())
            .map_err(|_| EngineError::InvalidAudio("recording too long".into()))?;
        let language = options
            .language
            .map(|l| CString::new(l.code()).expect("static code"));
        let keywords = if options.keywords.is_empty() {
            None
        } else {
            let joined = options
                .keywords
                .iter()
                .map(|k| k.replace(['\n', '\r', '\0'], " "))
                .collect::<Vec<_>>()
                .join("\n");
            Some(CString::new(joined).expect("nul bytes removed"))
        };

        let mut capacity = initial_capacity(audio.len(), options.word_timestamps);
        let started = Instant::now();
        let json = loop {
            let mut out = vec![0u8; capacity];
            // SAFETY: pointers are valid for the given lengths for the whole
            // call. Strings are NUL-terminated or null. `out` is writable for
            // `capacity` bytes. Calls are serialised by the service.
            let rc = unsafe {
                ffi::needle_transcribe(
                    audio.as_ptr(),
                    samples,
                    language.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
                    keywords.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
                    options.word_timestamps as c_int,
                    out.as_mut_ptr().cast(),
                    capacity as c_int,
                )
            };
            if rc < 0 {
                let msg = last_error();
                match required_capacity(&msg) {
                    Some(need) if need > capacity => {
                        capacity = need + 1024;
                        continue;
                    }
                    _ => return Err(EngineError::Inference(msg)),
                }
            }
            let len = out.iter().position(|b| *b == 0).unwrap_or(out.len());
            out.truncate(len);
            break out;
        };
        let inference_ms = started.elapsed().as_secs_f64() * 1e3;

        let raw: RawTranscript = serde_json::from_slice(&json)
            .map_err(|e| EngineError::Inference(format!("unexpected engine output: {e}")))?;
        Ok(Transcript {
            text: raw.text.trim().to_owned(),
            language: (!raw.language.is_empty()).then_some(raw.language),
            words: raw.words,
            timing: TranscriptTiming {
                audio_ms: audio.len() as f64 / 16.0,
                inference_ms,
                ttft_ms: raw.ttft_ms,
                decode_tps: raw.decode_tps,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_required_capacity() {
        assert_eq!(
            required_capacity("output buffer is too small: 139 bytes needed"),
            Some(139)
        );
        assert_eq!(required_capacity("invalid .cact model"), None);
    }

    #[test]
    fn capacity_grows_with_duration_and_word_times() {
        let short = initial_capacity(16_000, false);
        let long = initial_capacity(16_000 * 300, false);
        assert!(long > short);
        assert!(initial_capacity(16_000, true) > short);
    }

    #[test]
    fn parses_engine_json() {
        let raw: RawTranscript = serde_json::from_str(
            r#"{"text":" hi ","language":"en","ttft_ms":1.5,"decode_tps":300.0,
               "words":[{"word":"hi","start":0.1,"end":0.3,"probability":0.9}]}"#,
        )
        .unwrap();
        assert_eq!(raw.text, " hi ");
        assert_eq!(raw.words.unwrap()[0].word, "hi");
        let silent: RawTranscript =
            serde_json::from_str(r#"{"text":"","language":"","ttft_ms":0.0,"decode_tps":0.0}"#)
                .unwrap();
        assert!(silent.text.is_empty() && silent.language.is_empty());
    }
}
