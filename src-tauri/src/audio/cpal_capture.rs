//! cpal-based [`AudioCaptureService`].
//!
//! Each recording runs on its own thread, which owns the `cpal::Stream`
//! (not `Send` on every platform) and drains the ring buffer. The device is
//! opened on `start` and closed on `finish` or `abort`, so the microphone is
//! only in use while recording.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig};
use rtrb::{Consumer, Producer, RingBuffer};

use super::{
    Accumulator, AudioCaptureService, CaptureConfig, CaptureError, CaptureEvent, CaptureEvents,
    CaptureInfo, InputDevice, PushOutcome, Recording,
};

/// Ring capacity in seconds of audio. The worker drains every 10 ms, so this
/// only fills if the worker is starved for a long time.
const RING_SECONDS: usize = 2;
const DRAIN_INTERVAL: Duration = Duration::from_millis(10);
const OPEN_TIMEOUT: Duration = Duration::from_secs(5);

fn map_cpal(e: cpal::Error) -> CaptureError {
    use cpal::ErrorKind as K;
    match e.kind() {
        K::PermissionDenied => CaptureError::PermissionDenied,
        K::DeviceBusy => CaptureError::DeviceBusy,
        K::DeviceNotAvailable | K::DeviceChanged | K::StreamInvalidated => {
            CaptureError::Disconnected
        }
        K::UnsupportedConfig | K::UnsupportedOperation => CaptureError::Unsupported(e.to_string()),
        _ => CaptureError::Backend(e.to_string()),
    }
}

/// Lists input devices on the default host.
pub fn list_input_devices() -> Result<Vec<InputDevice>, CaptureError> {
    let host = cpal::default_host();
    let default_id = host.default_input_device().and_then(|d| d.id().ok());
    let devices = host.input_devices().map_err(map_cpal)?;
    Ok(devices
        .filter_map(|d| {
            let id = d.id().ok()?;
            Some(InputDevice {
                is_default: default_id.as_ref() == Some(&id),
                id: id.to_string(),
                name: d.to_string(),
            })
        })
        .collect())
}

fn find_device(host: &cpal::Host, id: Option<&str>) -> Result<(cpal::Device, bool), CaptureError> {
    if let Some(id) = id {
        let found = id
            .parse::<cpal::DeviceId>()
            .ok()
            .and_then(|parsed| host.device_by_id(&parsed));
        if let Some(d) = found {
            return Ok((d, false));
        }
        tracing::warn!("configured microphone not found; using the default device");
        return host
            .default_input_device()
            .map(|d| (d, true))
            .ok_or(CaptureError::NoDevice);
    }
    host.default_input_device()
        .map(|d| (d, false))
        .ok_or(CaptureError::NoDevice)
}

/// State shared between the device callback and the worker.
struct Shared {
    dropped_frames: AtomicU64,
    fatal: AtomicBool,
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut producer: Producer<f32>,
    shared: Arc<Shared>,
    events: CaptureEvents,
) -> Result<Stream, cpal::Error>
where
    T: SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    let channels = config.channels.max(1) as usize;
    let cb_shared = shared.clone();
    device.build_input_stream::<T, _, _>(
        *config,
        // Real-time callback: no allocation, no locks, no blocking.
        move |data: &[T], _| {
            let free = producer.slots();
            let n = data.len().min(free - free % channels);
            if n > 0
                && let Ok(chunk) = producer.write_chunk_uninit(n)
            {
                chunk.fill_from_iter(data[..n].iter().map(|s| f32::from_sample(*s)));
            }
            if n < data.len() {
                cb_shared
                    .dropped_frames
                    .fetch_add(((data.len() - n) / channels) as u64, Ordering::Relaxed);
            }
        },
        move |err| {
            if err.kind() == cpal::ErrorKind::Xrun {
                tracing::debug!("audio xrun");
                return;
            }
            tracing::warn!(error = %err, "audio stream error");
            if !shared.fatal.swap(true, Ordering::SeqCst) {
                events(CaptureEvent::Failed(map_cpal(err)));
            }
        },
        Some(OPEN_TIMEOUT),
    )
}

struct Opened {
    stream: Stream,
    consumer: Consumer<f32>,
    shared: Arc<Shared>,
    info: CaptureInfo,
    rate: u32,
    channels: u16,
}

fn open(config: &CaptureConfig, events: &CaptureEvents) -> Result<Opened, CaptureError> {
    let started = Instant::now();
    let host = cpal::default_host();
    let (device, fell_back) = find_device(&host, config.device_id.as_deref())?;
    let supported = device.default_input_config().map_err(map_cpal)?;
    let stream_config = supported.config();
    let rate = stream_config.sample_rate;
    let channels = stream_config.channels;
    let (producer, consumer) =
        RingBuffer::<f32>::new(rate as usize * channels.max(1) as usize * RING_SECONDS);
    let shared = Arc::new(Shared {
        dropped_frames: AtomicU64::new(0),
        fatal: AtomicBool::new(false),
    });
    let ev = events.clone();
    let sh = shared.clone();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build_stream::<f32>(&device, &stream_config, producer, sh, ev),
        SampleFormat::I16 => build_stream::<i16>(&device, &stream_config, producer, sh, ev),
        SampleFormat::U16 => build_stream::<u16>(&device, &stream_config, producer, sh, ev),
        SampleFormat::I32 => build_stream::<i32>(&device, &stream_config, producer, sh, ev),
        SampleFormat::I8 => build_stream::<i8>(&device, &stream_config, producer, sh, ev),
        SampleFormat::U8 => build_stream::<u8>(&device, &stream_config, producer, sh, ev),
        SampleFormat::F64 => build_stream::<f64>(&device, &stream_config, producer, sh, ev),
        other => return Err(CaptureError::Unsupported(format!("sample format {other}"))),
    }
    .map_err(map_cpal)?;
    stream.play().map_err(map_cpal)?;
    let info = CaptureInfo {
        device_name: device.to_string(),
        sample_rate: rate,
        channels,
        sample_format: supported.sample_format().to_string(),
        fell_back_to_default: fell_back,
        open_ms: started.elapsed().as_secs_f64() * 1e3,
    };
    Ok(Opened {
        stream,
        consumer,
        shared,
        info,
        rate,
        channels,
    })
}

/// Moves whole frames from the ring into the accumulator.
fn drain(
    consumer: &mut Consumer<f32>,
    channels: usize,
    shared: &Shared,
    acc: &mut Accumulator,
    events: &CaptureEvents,
    seen_dropped: &mut u64,
) {
    let available = consumer.slots();
    let n = available - available % channels;
    let mut outcome = PushOutcome::Ok;
    if n > 0
        && let Ok(chunk) = consumer.read_chunk(n)
    {
        let (a, b) = chunk.as_slices();
        // The ring holds whole frames, so a frame never straddles the wrap.
        // `a` may still end mid-frame if channels does not divide the
        // capacity, so carry its remainder over into `b`.
        let split = a.len() - a.len() % channels;
        let emit = |l| events(CaptureEvent::Level(l));
        if acc.push_interleaved(&a[..split], emit) == PushOutcome::LimitReached {
            outcome = PushOutcome::LimitReached;
        }
        if split < a.len() {
            let mut frame = [0f32; 16];
            let rest = &a[split..];
            let need = channels - rest.len();
            if channels <= frame.len() && b.len() >= need {
                frame[..rest.len()].copy_from_slice(rest);
                frame[rest.len()..channels].copy_from_slice(&b[..need]);
                if acc.push_interleaved(&frame[..channels], emit) == PushOutcome::LimitReached {
                    outcome = PushOutcome::LimitReached;
                }
                if acc.push_interleaved(&b[need..], emit) == PushOutcome::LimitReached {
                    outcome = PushOutcome::LimitReached;
                }
            }
        } else if acc.push_interleaved(b, emit) == PushOutcome::LimitReached {
            outcome = PushOutcome::LimitReached;
        }
        chunk.commit_all();
    }
    let dropped = shared.dropped_frames.load(Ordering::Relaxed);
    if dropped > *seen_dropped {
        tracing::warn!(frames = dropped - *seen_dropped, "audio frames dropped");
        if acc.push_gap(dropped - *seen_dropped) == PushOutcome::LimitReached {
            outcome = PushOutcome::LimitReached;
        }
        *seen_dropped = dropped;
    }
    if outcome == PushOutcome::LimitReached {
        events(CaptureEvent::LimitReached);
    }
}

struct Active {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<Result<Recording, CaptureError>>,
}

#[derive(Default)]
pub struct CpalCapture {
    active: Option<Active>,
}

impl CpalCapture {
    pub fn new() -> Self {
        Self::default()
    }

    fn stop(&mut self) -> Option<Result<Recording, CaptureError>> {
        let active = self.active.take()?;
        active.stop.store(true, Ordering::SeqCst);
        Some(
            active
                .thread
                .join()
                .unwrap_or_else(|_| Err(CaptureError::Backend("capture thread panicked".into()))),
        )
    }
}

impl AudioCaptureService for CpalCapture {
    fn start(
        &mut self,
        config: &CaptureConfig,
        events: CaptureEvents,
    ) -> Result<CaptureInfo, CaptureError> {
        if self.is_active() {
            return Err(CaptureError::AlreadyRecording);
        }
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<CaptureInfo, CaptureError>>(1);
        let config = config.clone();
        let thread = std::thread::Builder::new()
            .name("audio-capture".into())
            .spawn(move || {
                let opened = match open(&config, &events) {
                    Ok(o) => o,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e.clone()));
                        return Err(e);
                    }
                };
                let Opened {
                    stream,
                    mut consumer,
                    shared,
                    info,
                    rate,
                    channels,
                } = opened;
                let _ = ready_tx.send(Ok(info));
                let mut acc = Accumulator::new(rate, channels, config.max_duration);
                let mut seen_dropped = 0;
                let ch = channels.max(1) as usize;
                while !thread_stop.load(Ordering::SeqCst) && !shared.fatal.load(Ordering::SeqCst) {
                    std::thread::sleep(DRAIN_INTERVAL);
                    drain(
                        &mut consumer,
                        ch,
                        &shared,
                        &mut acc,
                        &events,
                        &mut seen_dropped,
                    );
                }
                // Release the microphone before the (fast) conversion.
                drop(stream);
                drain(
                    &mut consumer,
                    ch,
                    &shared,
                    &mut acc,
                    &events,
                    &mut seen_dropped,
                );
                if shared.fatal.load(Ordering::SeqCst) {
                    return Err(CaptureError::Disconnected);
                }
                acc.finish(channels)
            })
            .map_err(|e| CaptureError::Backend(e.to_string()))?;
        self.active = Some(Active { stop, thread });

        match ready_rx.recv_timeout(OPEN_TIMEOUT + Duration::from_secs(1)) {
            Ok(Ok(info)) => {
                tracing::info!(
                    rate = info.sample_rate,
                    channels = info.channels,
                    format = %info.sample_format,
                    open_ms = format!("{:.1}", info.open_ms),
                    "capture started"
                );
                Ok(info)
            }
            Ok(Err(e)) => {
                let _ = self.stop();
                Err(e)
            }
            Err(_) => {
                // Do not join: the backend may still be blocked opening the
                // device. The stop flag ends the thread once it returns.
                if let Some(a) = self.active.take() {
                    a.stop.store(true, Ordering::SeqCst);
                }
                Err(CaptureError::Timeout)
            }
        }
    }

    fn finish(&mut self) -> Result<Recording, CaptureError> {
        self.stop().unwrap_or(Err(CaptureError::NotRecording))
    }

    fn abort(&mut self) {
        // Dropping the result zeroes the audio.
        let _ = self.stop();
    }

    fn is_active(&self) -> bool {
        self.active.is_some()
    }
}

impl Drop for CpalCapture {
    fn drop(&mut self) {
        self.abort();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    fn ring_with(data: &[f32], capacity: usize, pre_advance: usize) -> Consumer<f32> {
        let (mut p, mut c) = RingBuffer::<f32>::new(capacity);
        // Advance the read and write positions so the data wraps around.
        for _ in 0..pre_advance {
            p.push(0.0).unwrap();
            c.pop().unwrap();
        }
        for s in data {
            p.push(*s).unwrap();
        }
        c
    }

    fn shared() -> Shared {
        Shared {
            dropped_frames: AtomicU64::new(0),
            fatal: AtomicBool::new(false),
        }
    }

    #[test]
    fn drain_handles_frames_split_across_ring_wrap() {
        // Capacity 7 with stereo: a frame can straddle the wrap point.
        let data = [0.2, 0.4, 0.6, 0.8, 1.0, 0.0];
        let mut c = ring_with(&data, 7, 5);
        let mut acc = Accumulator::new(16_000, 2, Duration::from_secs(1));
        let events: CaptureEvents = Arc::new(|_| {});
        let mut seen = 0;
        drain(&mut c, 2, &shared(), &mut acc, &events, &mut seen);
        assert_eq!(acc.frames(), 3);
        assert_eq!(c.slots(), 0);
    }

    #[test]
    fn drain_converts_dropped_frames_into_silence() {
        let mut c = ring_with(&[0.5; 4], 16, 0);
        let sh = shared();
        sh.dropped_frames.store(10, Ordering::Relaxed);
        let mut acc = Accumulator::new(16_000, 1, Duration::from_secs(1));
        let events: CaptureEvents = Arc::new(|_| {});
        let mut seen = 0;
        drain(&mut c, 1, &sh, &mut acc, &events, &mut seen);
        assert_eq!(acc.frames(), 14);
        assert_eq!(seen, 10);
        drain(&mut c, 1, &sh, &mut acc, &events, &mut seen);
        assert_eq!(acc.frames(), 14, "gap counted once");
    }

    #[test]
    fn drain_reports_limit() {
        let mut c = ring_with(&[0.5; 20], 32, 0);
        let mut acc = Accumulator::new(10, 1, Duration::from_secs(1));
        let got = Arc::new(Mutex::new(Vec::new()));
        let g = got.clone();
        let events: CaptureEvents = Arc::new(move |e| {
            if matches!(e, CaptureEvent::LimitReached) {
                g.lock().unwrap().push(());
            }
        });
        let mut seen = 0;
        drain(&mut c, 1, &shared(), &mut acc, &events, &mut seen);
        assert_eq!(got.lock().unwrap().len(), 1);
        assert_eq!(acc.frames(), 10);
    }

    #[test]
    fn finish_and_abort_without_start() {
        let mut cap = CpalCapture::new();
        assert!(!cap.is_active());
        assert_eq!(cap.finish().unwrap_err(), CaptureError::NotRecording);
        cap.abort();
    }

    /// Records from the real default microphone. Run with
    /// `cargo test -- --ignored microphone`.
    #[test]
    #[ignore = "needs a microphone"]
    fn microphone_records_and_releases() {
        let levels = Arc::new(AtomicU64::new(0));
        let l = levels.clone();
        let events: CaptureEvents = Arc::new(move |e| {
            if let CaptureEvent::Level(_) = e {
                l.fetch_add(1, Ordering::Relaxed);
            }
        });
        let mut cap = CpalCapture::new();
        let info = cap
            .start(&CaptureConfig::default(), events.clone())
            .unwrap();
        eprintln!("{info:?}");
        assert_eq!(
            cap.start(&CaptureConfig::default(), events.clone())
                .unwrap_err(),
            CaptureError::AlreadyRecording
        );
        std::thread::sleep(Duration::from_millis(1000));
        let rec = cap.finish().unwrap();
        eprintln!("{rec:?} levels={}", levels.load(Ordering::Relaxed));
        let secs = rec.duration().as_secs_f64();
        assert!((0.8..1.3).contains(&secs), "duration {secs}");
        assert!(levels.load(Ordering::Relaxed) >= 20);
        assert!(!cap.is_active());
        // The device must be reusable straight away.
        cap.start(&CaptureConfig::default(), events).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        cap.abort();
        assert!(!cap.is_active());
    }
}
