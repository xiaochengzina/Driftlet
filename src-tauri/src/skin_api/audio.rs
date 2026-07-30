//! System audio loopback capture + spectrum (Windows only).
//!
//! A background thread owns the WASAPI loopback client of the default render
//! device and keeps the latest samples in a small mono ring buffer.  The
//! `get_audio_spectrum` command just reads the ring and runs an FFT — cheap
//! enough to poll at 10–30 fps.
//!
//! Lifecycle: the thread starts lazily on the first poll and releases the
//! audio device after 30 s without polls (skins come and go; we must not
//! hold the endpoint open forever).  It re-opens on the next poll.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use super::Spectrum;

const SAMPLE_RATE: usize = 48_000;
const CHANNELS: usize = 2;
const FFT_SIZE: usize = 2048;
const RING_CAPACITY: usize = 8192;
const IDLE_STOP_SECS: u64 = 30;

struct Shared {
    samples: Mutex<VecDeque<f32>>,
    last_poll: Mutex<Instant>,
    /// Last capture-side failure, shown to the caller until capture recovers.
    error: Mutex<Option<String>>,
}

/// What to capture: the system's output (loopback) or the microphone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Loopback,
    Mic,
}

static LOOPBACK: Mutex<Option<Arc<Shared>>> = Mutex::new(None);
static MIC: Mutex<Option<Arc<Shared>>> = Mutex::new(None);

/// Read the current spectrum for `source`.  Starts the capture thread on
/// first call.  `bands` is clamped to 1–64.
pub fn spectrum(bands: usize, source: Source) -> Result<Spectrum, String> {
    let slot = match source {
        Source::Loopback => &LOOPBACK,
        Source::Mic => &MIC,
    };
    let shared = {
        let mut guard = slot.lock().unwrap();
        guard
            .get_or_insert_with(|| {
                let shared = Arc::new(Shared {
                    samples: Mutex::new(VecDeque::with_capacity(RING_CAPACITY)),
                    last_poll: Mutex::new(Instant::now()),
                    error: Mutex::new(None),
                });
                let s2 = shared.clone();
                // spawn 失败不能吞掉：记入错误状态，让后续频谱调用返回
                // 明确错误，而不是静默地永远返回空数据。
                if let Err(e) = std::thread::Builder::new()
                    .name(format!("audio-capture-{:?}", source))
                    .spawn(move || capture_thread(s2, source))
                {
                    *shared.error.lock().unwrap() =
                        Some(format!("capture thread spawn failed: {}", e));
                }
                shared
            })
            .clone()
    };

    *shared.last_poll.lock().unwrap() = Instant::now();

    if let Some(err) = shared.error.lock().unwrap().clone() {
        return Err(err);
    }
    let samples: Vec<f32> = shared.samples.lock().unwrap().iter().copied().collect();
    Ok(compute_spectrum(&samples, bands.clamp(1, 64)))
}

// ─── Capture thread ───

fn capture_thread(shared: Arc<Shared>, source: Source) {
    // COM (MTA) once per thread, before any WASAPI call.  S_FALSE (already
    // initialized) is a success HRESULT, so .ok() accepts it.
    if let Err(e) = wasapi::initialize_mta().ok() {
        *shared.error.lock().unwrap() = Some(format!("COM init failed: {}", e));
        return;
    }
    loop {
        // Park until a skin starts polling again.
        while shared.last_poll.lock().unwrap().elapsed() > Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(500));
        }
        match run_capture(&shared, source) {
            Ok(()) => {
                // Went idle — device already released, wait for the next poll.
                *shared.error.lock().unwrap() = None;
            }
            Err(e) => {
                log::warn!("audio capture ({:?}) failed: {}", source, e);
                *shared.error.lock().unwrap() = Some(e);
                std::thread::sleep(Duration::from_secs(5));
            }
        }
    }
}

fn run_capture(shared: &Arc<Shared>, source: Source) -> Result<(), String> {
    use wasapi::{get_default_device, Direction, SampleType, ShareMode, WaveFormat};

    // Loopback recipe: open the default RENDER device and initialize the
    // client for Capture — wasapi then sets AUDCLNT_STREAMFLAGS_LOOPBACK.
    // Mic recipe: the default CAPTURE device, plain Capture init.
    let device_direction = match source {
        Source::Loopback => Direction::Render,
        Source::Mic => Direction::Capture,
    };
    let device = get_default_device(&device_direction).map_err(|e| e.to_string())?;
    let mut client = device.get_iaudioclient().map_err(|e| e.to_string())?;
    let format = WaveFormat::new(32, 32, &SampleType::Float, SAMPLE_RATE, CHANNELS, None);
    let (default_period, _min) = client.get_periods().map_err(|e| e.to_string())?;
    // convert=true guarantees the requested f32/48k/stereo mix regardless of
    // the device's native format.
    client
        .initialize_client(&format, default_period, &Direction::Capture, &ShareMode::Shared, true)
        .map_err(|e| e.to_string())?;
    let event = client.set_get_eventhandle().map_err(|e| e.to_string())?;
    let capture = client.get_audiocaptureclient().map_err(|e| e.to_string())?;
    client.start_stream().map_err(|e| e.to_string())?;

    const FRAME_BYTES: usize = CHANNELS * 4;
    loop {
        if shared.last_poll.lock().unwrap().elapsed() > Duration::from_secs(IDLE_STOP_SECS) {
            let _ = client.stop_stream();
            return Ok(());
        }
        // Timeout = wake to re-check idleness even when nothing is playing.
        if event.wait_for_event(500).is_err() {
            continue;
        }
        // Drain every queued packet.
        loop {
            let frames_avail = capture
                .get_next_nbr_frames()
                .map_err(|e| e.to_string())?
                .unwrap_or(0);
            if frames_avail == 0 {
                break;
            }
            let mut buf = vec![0u8; frames_avail as usize * FRAME_BYTES];
            let (frames, flags) = capture
                .read_from_device(&mut buf)
                .map_err(|e| e.to_string())?;
            push_frames(shared, &buf[..frames as usize * FRAME_BYTES], frames as usize, flags.silent);
        }
    }
}

/// Convert one packet of stereo f32 frames into the mono ring.  Silent
/// buffers (endpoint idle) become zeros so the visualizer decays.
fn push_frames(shared: &Arc<Shared>, buf: &[u8], frames: usize, silent: bool) {
    const FRAME_BYTES: usize = CHANNELS * 4;
    let mut q = shared.samples.lock().unwrap();
    if silent {
        for _ in 0..frames {
            push_capped(&mut q, 0.0);
        }
        return;
    }
    for chunk in buf.chunks_exact(FRAME_BYTES) {
        let l = f32::from_le_bytes(chunk[0..4].try_into().unwrap());
        let r = f32::from_le_bytes(chunk[4..8].try_into().unwrap());
        push_capped(&mut q, (l + r) * 0.5);
    }
}

fn push_capped(q: &mut VecDeque<f32>, v: f32) {
    q.push_back(v);
    if q.len() > RING_CAPACITY {
        q.pop_front();
    }
}

// ─── FFT → bands (pure, unit-tested) ───

fn compute_spectrum(samples: &[f32], bands: usize) -> Spectrum {
    use rustfft::num_complex::Complex32;
    use rustfft::FftPlanner;

    let peak = samples
        .iter()
        .fold(0.0f32, |m, &s| m.max(s.abs()))
        .min(1.0);

    // Latest FFT_SIZE samples, Hann-windowed, zero-padded when short.
    let n = FFT_SIZE;
    let take = samples.len().min(n);
    let start = samples.len() - take;
    let mut buf = vec![Complex32::ZERO; n];
    for (i, &s) in samples[start..].iter().enumerate() {
        let w = hann(i, take);
        buf[i] = Complex32::new(s * w, 0.0);
    }
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n);
    fft.process(&mut buf);

    let sr = SAMPLE_RATE as f32;
    let f_min = 30.0f32;
    let f_max = 16_000.0f32.min(sr * 0.45);
    let ratio = (f_max / f_min).powf(1.0 / bands as f32);
    let mut out = Vec::with_capacity(bands);
    for b in 0..bands {
        let lo = f_min * ratio.powi(b as i32);
        let hi = lo * ratio;
        let bin_lo = ((lo * n as f32 / sr) as usize).max(1).min(n / 2 - 1);
        let bin_hi = ((hi * n as f32 / sr) as usize)
            .max(bin_lo + 1)
            .min(n / 2);
        let sum: f32 = (bin_lo..bin_hi).map(|i| buf[i].norm()).sum();
        let mean = sum / (bin_hi - bin_lo) as f32;
        // Full-scale sine ≈ N/4 per bin after Hann loss → *4/N ≈ 0 dBFS.
        let db = 20.0 * (mean * 4.0 / n as f32 + 1e-10).log10();
        out.push(((db + 60.0) / 60.0).clamp(0.0, 1.0));
    }

    Spectrum { bands: out, peak }
}

fn hann(i: usize, n: usize) -> f32 {
    if n < 2 {
        return 1.0;
    }
    0.5 - 0.5 * ((2.0 * std::f32::consts::PI * i as f32) / (n as f32 - 1.0)).cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_gives_zero_bands() {
        let s = compute_spectrum(&[0.0; FFT_SIZE], 16);
        assert_eq!(s.bands.len(), 16);
        assert!(s.bands.iter().all(|&b| b == 0.0));
        assert_eq!(s.peak, 0.0);
    }

    #[test]
    fn sine_lands_in_one_band() {
        let freq = 440.0f32;
        let samples: Vec<f32> = (0..FFT_SIZE)
            .map(|i| {
                0.8 * (2.0 * std::f32::consts::PI * freq * i as f32 / SAMPLE_RATE as f32).sin()
            })
            .collect();
        let s = compute_spectrum(&samples, 16);
        assert!((s.peak - 0.8).abs() < 0.01);
        let max_idx = s
            .bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, _)| i)
            .unwrap();
        // 440 Hz sits in a low band; the top band (≈11–16 kHz) must be far weaker.
        assert!(s.bands[max_idx] > 0.5, "dominant band too weak: {:?}", s.bands);
        assert!(s.bands[max_idx] > s.bands[15] + 0.3, "no spectral contrast: {:?}", s.bands);
    }

    #[test]
    fn tolerates_short_input() {
        let s = compute_spectrum(&[0.5; 100], 32);
        assert_eq!(s.bands.len(), 32);
    }

    #[test]
    #[ignore = "hardware probe — run manually with --nocapture"]
    fn probe_loopback_capture() {
        std::thread::sleep(std::time::Duration::from_millis(500));
        match spectrum(16, Source::Loopback) {
            Ok(s) => println!("loopback OK: peak={} bands={:?}", s.peak, s.bands),
            Err(e) => panic!("loopback capture failed on this machine: {}", e),
        }
    }

    #[test]
    #[ignore = "hardware probe — run manually with --nocapture"]
    fn probe_mic_capture() {
        std::thread::sleep(std::time::Duration::from_millis(500));
        match spectrum(16, Source::Mic) {
            Ok(s) => println!("mic OK: peak={} bands={:?}", s.peak, s.bands),
            Err(e) => panic!("mic capture failed on this machine: {}", e),
        }
    }
}
