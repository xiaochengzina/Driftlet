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

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
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
    /// 采集线程存活标志：线程入口置位，任何退出路径（spawn 失败后根本
    /// 没起、COM 初始化失败返回、panic 展开）复位。错误驻留且线程已死时
    /// spectrum 侧清错重建——否则一次 spawn 失败就是永驻错误。
    alive: AtomicBool,
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
        let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .get_or_insert_with(|| {
                let shared = Arc::new(Shared {
                    samples: Mutex::new(VecDeque::with_capacity(RING_CAPACITY)),
                    last_poll: Mutex::new(Instant::now()),
                    error: Mutex::new(None),
                    alive: AtomicBool::new(false),
                });
                spawn_capture(&shared, source);
                shared
            })
            .clone()
    };

    *shared.last_poll.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();

    if let Some(err) = shared.error.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        // 线程还活着（采集暂时失败、5s 后自动重试中）：如实上报错误。
        // 线程已死（spawn 失败 / COM 初始化失败 / panic）：错误不会自愈，
        // 清错并重建线程；本次调用先按无数据返回，重建若再失败会把错误
        // 重新记入，下次调用继续走本分支重试。
        if shared.alive.load(Ordering::Relaxed) {
            return Err(err);
        }
        *shared.error.lock().unwrap_or_else(|e| e.into_inner()) = None;
        // 残留的可能是线程死亡前的旧样本，一并清掉再重建
        shared.samples.lock().unwrap_or_else(|e| e.into_inner()).clear();
        spawn_capture(&shared, source);
    }
    let samples: Vec<f32> = {
        // Only the latest FFT_SIZE samples feed the FFT — copying the whole
        // 8192-sample ring here also extends the lock hold the capture
        // thread contends with on every packet.
        let q = shared.samples.lock().unwrap_or_else(|e| e.into_inner());
        let take = q.len().min(FFT_SIZE);
        q.iter().skip(q.len() - take).copied().collect()
    };
    Ok(compute_spectrum(&samples, bands.clamp(1, 64)))
}

// ─── Capture thread ───

/// 启动采集线程。spawn 失败不能吞掉：记入错误状态，让后续频谱调用返回
/// 明确错误，而不是静默地永远返回空数据（alive 保持 false，下次频谱
/// 调用会走重建分支再试）。
fn spawn_capture(shared: &Arc<Shared>, source: Source) {
    let s2 = shared.clone();
    if let Err(e) = std::thread::Builder::new()
        .name(format!("audio-capture-{:?}", source))
        .spawn(move || capture_thread(s2, source))
    {
        *shared.error.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(format!("capture thread spawn failed: {}", e));
    }
}

/// 采集线程退出（含 panic 展开）时复位存活标志，允许 spectrum 侧重建；
/// 统一清空环形缓冲（loop 底部的清空只管 run_capture 正常返回的路径，
/// panic 展开会跳过它）。
struct AliveReset(Arc<Shared>);

impl Drop for AliveReset {
    fn drop(&mut self) {
        self.0.alive.store(false, Ordering::Relaxed);
        self.0.samples.lock().unwrap_or_else(|e| e.into_inner()).clear();
        if std::thread::panicking() {
            // panic 展开跳过了错误记录：alive 已假、error 为空时 spectrum
            // 侧认不到死亡、永不重建——补一条错误让下次调用走重建分支
            let mut err = self.0.error.lock().unwrap_or_else(|e| e.into_inner());
            if err.is_none() {
                *err = Some("capture thread panicked".to_string());
            }
        }
    }
}

fn capture_thread(shared: Arc<Shared>, source: Source) {
    shared.alive.store(true, Ordering::Relaxed);
    let _alive = AliveReset(shared.clone());
    // COM (MTA) once per thread, before any WASAPI call.  S_FALSE (already
    // initialized) is a success HRESULT, so .ok() accepts it.
    if let Err(e) = wasapi::initialize_mta().ok() {
        *shared.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(format!("COM init failed: {}", e));
        return;
    }
    loop {
        // Park until a skin starts polling again.
        while shared.last_poll.lock().unwrap_or_else(|e| e.into_inner()).elapsed() > Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(500));
        }
        match run_capture(&shared, source) {
            Ok(()) => {
                // Went idle — device already released, wait for the next poll.
                *shared.error.lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
            Err(e) => {
                log::warn!("audio capture ({:?}) failed: {}", source, e);
                *shared.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(e);
                std::thread::sleep(Duration::from_secs(5));
            }
        }
        // 退出采集（闲置释放设备/出错重试）后清空环形缓冲：残留旧样本会
        // 在恢复轮询时被回放成一帧中断前的旧画面
        shared.samples.lock().unwrap_or_else(|e| e.into_inner()).clear();
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
    // 采集已实际恢复（如设备热插拔后重开成功）：清除此前驻留的错误——
    // 否则持续轮询的皮肤要等 30s 闲置退出（run_capture 返回 Ok）后才看到
    // 错误消失，期间频谱一直报旧错。
    *shared.error.lock().unwrap_or_else(|e| e.into_inner()) = None;

    const FRAME_BYTES: usize = CHANNELS * 4;
    loop {
        if shared.last_poll.lock().unwrap_or_else(|e| e.into_inner()).elapsed() > Duration::from_secs(IDLE_STOP_SECS) {
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
    let mut q = shared.samples.lock().unwrap_or_else(|e| e.into_inner());
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

/// FFT plan (twiddle tables etc.).  Rebuilding it per call was tens of KB
/// of setup work at the documented 10–30 fps polling rate; the transform
/// size here is always FFT_SIZE.
static FFT_PLAN: LazyLock<Arc<dyn rustfft::Fft<f32>>> =
    LazyLock::new(|| rustfft::FftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE));

thread_local! {
    /// Reused 16 KB scratch buffer, zeroed before each transform.
    static FFT_BUF: RefCell<Vec<rustfft::num_complex::Complex32>> =
        RefCell::new(vec![rustfft::num_complex::Complex32::ZERO; FFT_SIZE]);
}

fn compute_spectrum(samples: &[f32], bands: usize) -> Spectrum {
    use rustfft::num_complex::Complex32;

    let peak = samples
        .iter()
        .fold(0.0f32, |m, &s| m.max(s.abs()))
        .min(1.0);

    // Latest FFT_SIZE samples, Hann-windowed, zero-padded when short.
    let n = FFT_SIZE;
    let take = samples.len().min(n);
    let start = samples.len() - take;

    let sr = SAMPLE_RATE as f32;
    let f_min = 30.0f32;
    let f_max = 16_000.0f32.min(sr * 0.45);
    let ratio = (f_max / f_min).powf(1.0 / bands as f32);
    let mut out = Vec::with_capacity(bands);

    FFT_BUF.with(|cell| {
        let mut buf = cell.borrow_mut();
        buf.fill(Complex32::ZERO);
        for (i, &s) in samples[start..].iter().enumerate() {
            let w = hann(i, take);
            buf[i] = Complex32::new(s * w, 0.0);
        }
        FFT_PLAN.process(&mut buf);

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
    });

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
