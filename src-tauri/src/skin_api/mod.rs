//! Skin-facing backend APIs: read-only system information plus the
//! permission-gated capabilities.  Six permissions, each gating its own
//! command set:
//!
//!   * `files`     — skin_read_file / skin_write_file / skin_list_dir /
//!                   skin_delete_file (sandboxed to the skin's own folder)
//!   * `registry`  — read_registry_value
//!   * `shell`     — run_command
//!   * `system`    — set_volume / set_mute / media_control / open_external /
//!                   show_notification
//!   * `clipboard` — read_clipboard_text / write_clipboard_text
//!   * `mic`       — get_mic_spectrum
//!
//! Skins call these through `window.__DESK_PP__.invoke` — the bridge is a
//! raw passthrough, so every command registered here is skin-callable.
//! Sensitive commands MUST go through `require_perm` first.
//!
//! Rate-type readings (disk/network bps, GPU usage) keep a persistent
//! sampler behind a Mutex: the first call primes the baseline and reports 0.

mod fs;
mod shell;
mod system;

#[cfg(target_os = "windows")]
mod audio;
#[cfg(target_os = "windows")]
mod gpu;
#[cfg(target_os = "windows")]
mod media;
#[cfg(target_os = "windows")]
mod notify;
#[cfg(target_os = "windows")]
mod pdh;
#[cfg(target_os = "windows")]
mod registry;
#[cfg(target_os = "windows")]
mod status;
#[cfg(target_os = "windows")]
mod volume;

use std::sync::Mutex;
use std::time::Instant;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use crate::i18n::{tr, trf, Key};
use crate::AppState;

// ─── Shared output types ───

#[derive(Debug, Clone, Serialize)]
pub struct GpuInfo {
    pub name: String,
    /// "discrete" | "integrated"（核显：D3D12 UMA 统一内存适配器）
    pub gpu_type: String,
    pub usage: f32,
    pub vram_total: u64,
    pub vram_used: u64,
    pub vram_usage_pct: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Spectrum {
    /// Band energies normalized to 0.0–1.0 (log-spaced 30 Hz – 16 kHz).
    pub bands: Vec<f32>,
    /// Peak sample magnitude 0.0–1.0.
    pub peak: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegistryValue {
    pub kind: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct VolumeInfo {
    /// 0.0–100.0
    pub volume_pct: f32,
    pub muted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// "playing" | "paused" | "stopped"
    pub status: String,
    pub position_secs: f64,
    pub duration_secs: f64,
    /// JPEG/PNG bytes as base64, when the source app provides artwork.
    pub cover_base64: Option<String>,
    /// cover_base64 的格式（"image/jpeg" | "image/png" | ...），按 magic
    /// bytes 嗅探；认不出来为 null（皮肤自行回退）。
    pub cover_mime: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum MediaAction {
    Play,
    Pause,
    PlayPause,
    Next,
    Previous,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatteryInfo {
    /// False on desktops without a battery — the rest is meaningless then.
    pub has_battery: bool,
    /// Charger plugged in.
    pub ac_online: bool,
    pub charging: bool,
    /// 0–100, None when Windows reports "unknown".
    pub percent: Option<u8>,
    /// Estimated seconds remaining, None when unknown (often while charging).
    pub secs_remaining: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForegroundWindowInfo {
    pub title: String,
    pub pid: u32,
    /// Executable file name, e.g. "chrome.exe" ("" when undeterminable).
    pub process_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct MonitorInfo {
    /// Device name, e.g. "\\\\.\\DISPLAY1".
    pub name: String,
    /// Full area in physical pixels.
    pub rect: Rect,
    /// Work area (excludes taskbar), physical pixels.
    pub work_area: Rect,
    pub is_primary: bool,
    /// Effective DPI / 96 (1.25 = 125% scaling).
    pub scale_factor: f64,
}

// ─── Permission gate ───

pub const PERM_REGISTRY: &str = "registry";
pub const PERM_SHELL: &str = "shell";
/// State-changing system controls: volume (set_volume / set_mute), media
/// transport (media_control), open_external, show_notification.
pub const PERM_SYSTEM: &str = "system";
/// Clipboard read+write (read can expose what the user just copied).
pub const PERM_CLIPBOARD: &str = "clipboard";
/// Microphone input — eavesdropping risk, unlike the loopback spectrum
/// (which only hears what the machine itself plays).
pub const PERM_MIC: &str = "mic";

/// Extract the calling skin from its window label ("skin-<id>"), resolved
/// against a fresh directory scan so an uninstalled skin fails fast.
fn caller_skin(
    state: &AppState,
    window: &tauri::WebviewWindow,
) -> Result<crate::skin::types::Skin, String> {
    let lang = state.lang();
    let skin_id = window
        .label()
        .strip_prefix("skin-")
        .ok_or_else(|| tr(&lang, Key::NotASkinWindow).to_string())?
        .to_string();
    crate::skin::loader::scan_skins_directory(&state.skins_dir)
        .into_iter()
        .find(|s| s.id == skin_id)
        .ok_or_else(|| trf(&lang, Key::SkinNotFound, &[skin_id.as_str()]))
}

/// Check that the calling skin's skin.json declared `perm`.
/// Returns (skin_id, skin_dir) on success.
fn require_perm(
    state: &AppState,
    window: &tauri::WebviewWindow,
    perm: &str,
) -> Result<(String, std::path::PathBuf), String> {
    let lang = state.lang();
    let skin = caller_skin(state, window)?;
    if !skin.manifest.permissions.iter().any(|p| p == perm) {
        return Err(trf(&lang, Key::PermissionDenied, &[skin.id.as_str(), perm]));
    }
    Ok((skin.id, skin.directory))
}

#[cfg_attr(target_os = "windows", allow(dead_code))] // only the non-Windows arms call it
fn windows_only(app: &AppHandle) -> String {
    tr(&app.state::<AppState>().lang(), Key::WindowsOnly).to_string()
}

fn pct(part: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (part as f32 / total as f32) * 100.0
    }
}

// ─── CPU ───

static CPU_SYS: Mutex<Option<sysinfo::System>> = Mutex::new(None);

/// CPU + memory only.  `System::new_all()` would also load the full process
/// table — every process's cmd/environ strings (each read remotely from the
/// process's PEB on Windows) — which then stays resident for the app's
/// lifetime even though these commands never touch processes.  get_processes
/// loads its slice lazily via refresh_processes_specifics instead.
fn new_light_system() -> sysinfo::System {
    sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::new()
            .with_cpu(sysinfo::CpuRefreshKind::everything())
            .with_memory(sysinfo::MemoryRefreshKind::everything()),
    )
}

/// 当前频率与任务管理器「速度」同算法：名义频率 × PDH `\Processor
/// Information(_Total)\% Processor Performance`（该计数器 = 实测频率占名义
/// 频率的百分比，全平台逐秒真实波动，turbo 时超 100%——TM 速度可超基准值
/// 即源于此）。两条弯路：sysinfo 的频率来自 `CallNtPowerInformation` 的
/// `CurrentMhz`，硬件自主 P-state（Speed Shift）的机器上恒为基准频率；
/// PDH 直读 MHz 的 `Processor Frequency` 计数器在部分平台（台式机实测）
/// 也恒报名义值。PDH 未就绪（首调基线/计数器缺失）时回退 sysinfo 静态值。
#[cfg(target_os = "windows")]
static CPU_PERF_PDH: Mutex<Option<pdh::PdhMultiCounter>> = Mutex::new(None);

#[cfg(target_os = "windows")]
fn cpu_performance_pct() -> Option<f64> {
    let mut guard = CPU_PERF_PDH.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = pdh::PdhMultiCounter::new(
            "\\Processor Information(_Total)\\% Processor Performance",
        );
    }
    guard
        .as_mut()?
        .sample()
        .into_iter()
        .map(|(_, v)| v)
        .find(|v| *v > 0.0)
}

#[cfg(not(target_os = "windows"))]
fn cpu_performance_pct() -> Option<f64> {
    None
}

#[derive(Debug, Clone, Serialize)]
pub struct CpuInfo {
    pub name: String,
    pub physical_cores: usize,
    pub logical_cores: usize,
    pub frequency_mhz: u64,
    pub usage: f32,
    pub usage_per_core: Vec<f32>,
}

/// Array shape anticipates multi-socket machines; sysinfo aggregates all
/// cores into one entry on typical PCs.  async：采样重负载不跑主线程。
#[tauri::command]
pub async fn get_cpu_info() -> Vec<CpuInfo> {
    let perf_pct = cpu_performance_pct();
    let mut guard = CPU_SYS.lock().unwrap_or_else(|e| e.into_inner());
    let sys = guard.get_or_insert_with(new_light_system);
    sys.refresh_cpu_all();
    let cpus = sys.cpus();
    let nominal_mhz = cpus.iter().map(|c| c.frequency()).max().unwrap_or(0);
    vec![CpuInfo {
        name: cpus.first().map(|c| c.brand().to_string()).unwrap_or_default(),
        physical_cores: sys.physical_core_count().unwrap_or(0),
        logical_cores: cpus.len(),
        // 任务管理器同款：名义频率 × 实测性能百分比；PDH 未就绪回退静态名义值
        frequency_mhz: perf_pct
            .map(|p| (nominal_mhz as f64 * p / 100.0).round() as u64)
            .filter(|v| *v > 0)
            .unwrap_or(nominal_mhz),
        usage: sys.global_cpu_usage(),
        usage_per_core: cpus.iter().map(|c| c.cpu_usage()).collect(),
    }]
}

// ─── Memory ───

#[derive(Debug, Clone, Serialize)]
pub struct MemoryGroup {
    pub total: u64,
    pub used: u64,
    pub free: u64,
    pub usage_pct: f32,
    pub free_pct: f32,
}

impl MemoryGroup {
    fn new(total: u64, used: u64) -> Self {
        let usage_pct = pct(used, total);
        MemoryGroup {
            total,
            used,
            free: total.saturating_sub(used),
            usage_pct,
            // total 为 0（swap 禁用等）时按 0/0 报：给 free_pct 100 等于
            // 宣称「不存在的空间全空着」
            free_pct: if total == 0 { 0.0 } else { 100.0 - usage_pct },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryInfo {
    pub ram: MemoryGroup,
    /// Virtual memory = page file (swap), matching Task Manager's "分页" pool.
    pub swap: MemoryGroup,
    /// 虚拟内存（提交）= 任务管理器「已提交 xx/yy GB」：total = 提交限制
    /// （物理内存 + 页面文件总量 − 系统保留），used = 已提交字节数。
    /// 页面文件用量在现代系统常恒 0，虚拟内存压力要看这组。仅 Windows 提供。
    pub commit: Option<MemoryGroup>,
}

#[tauri::command]
pub fn get_memory_info() -> MemoryInfo {
    let mut guard = CPU_SYS.lock().unwrap_or_else(|e| e.into_inner());
    let sys = guard.get_or_insert_with(new_light_system);
    sys.refresh_memory();
    MemoryInfo {
        ram: MemoryGroup::new(sys.total_memory(), sys.used_memory()),
        swap: MemoryGroup::new(sys.total_swap(), sys.used_swap()),
        commit: commit_group(),
    }
}

/// 已提交/提交限制走 psapi `GetPerformanceInfo`（与任务管理器同源）：
/// 直查内核计数器，无 PDH 两阶段采样，首次调用即有效。
#[cfg(target_os = "windows")]
fn commit_group() -> Option<MemoryGroup> {
    use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
    let mut info = PERFORMANCE_INFORMATION::default();
    info.cb = std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32;
    unsafe { GetPerformanceInfo(&mut info, info.cb) }.ok()?;
    let page = info.PageSize as u64;
    Some(MemoryGroup::new(
        info.CommitLimit as u64 * page,
        info.CommitTotal as u64 * page,
    ))
}

#[cfg(not(target_os = "windows"))]
fn commit_group() -> Option<MemoryGroup> {
    None
}

// ─── Disks ───

struct DiskSampler {
    disks: sysinfo::Disks,
}

static DISK_SAMPLER: Mutex<Option<DiskSampler>> = Mutex::new(None);

#[derive(Debug, Clone, Serialize)]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub fs: String,
    pub total: u64,
    pub used: u64,
    pub free: u64,
    pub usage_pct: f32,
    /// Read/write throughput in bytes/sec (PDH per-second counters;
    /// 0 on the first call — baseline).
    pub read_bps: u64,
    pub write_bps: u64,
}

#[tauri::command]
pub async fn get_disks_info() -> Vec<DiskInfo> {
    let rates = sample_disk_rates();

    let mut guard = DISK_SAMPLER.lock().unwrap_or_else(|e| e.into_inner());
    let sampler = guard.get_or_insert_with(|| DiskSampler {
        disks: sysinfo::Disks::new_with_refreshed_list(),
    });
    sampler.disks.refresh();

    sampler
        .disks
        .list()
        .iter()
        .map(|d| {
            let total = d.total_space();
            let free = d.available_space();
            let used = total.saturating_sub(free);
            let letter = drive_letter(d.mount_point());
            let (read_bps, write_bps) = letter
                .and_then(|l| rates.get(&l).copied())
                .unwrap_or((0, 0));
            DiskInfo {
                name: d.name().to_string_lossy().to_string(),
                mount_point: d.mount_point().to_string_lossy().to_string(),
                fs: d.file_system().to_string_lossy().to_string(),
                total,
                used,
                free,
                usage_pct: pct(used, total),
                read_bps,
                write_bps,
            }
        })
        .collect()
}

/// "C:\\" → Some("C:"); other mount styles → None (no PDH rates for those).
fn drive_letter(mount_point: &std::path::Path) -> Option<String> {
    let s = mount_point.to_string_lossy();
    let c = s.chars().next()?;
    (c.is_ascii_alphabetic() && s.len() >= 2 && s.as_bytes()[1] == b':')
        .then(|| format!("{}:", c.to_ascii_uppercase()))
}

#[cfg(target_os = "windows")]
static DISK_READ_PDH: Mutex<Option<pdh::PdhMultiCounter>> = Mutex::new(None);
#[cfg(target_os = "windows")]
static DISK_WRITE_PDH: Mutex<Option<pdh::PdhMultiCounter>> = Mutex::new(None);

/// sysinfo 0.32 dropped disk-I/O stats, so throughput comes from PDH
/// `\LogicalDisk(*)\Disk Read/Write Bytes/sec` — already per-second rates,
/// and per-LETTER instances ("\PhysicalDisk" is per spindle: partitions of
/// one disk would all report the shared rate, which is not what a skin
/// wants to display for "C:").
#[cfg(target_os = "windows")]
fn sample_disk_rates() -> std::collections::HashMap<String, (u64, u64)> {
    let mut out: std::collections::HashMap<String, (u64, u64)> = std::collections::HashMap::new();
    for (name, v) in sample_pdh(&DISK_READ_PDH, "\\LogicalDisk(*)\\Disk Read Bytes/sec") {
        for letter in drive_letters_from_instance(&name) {
            out.entry(letter).or_default().0 += v.max(0.0) as u64;
        }
    }
    for (name, v) in sample_pdh(&DISK_WRITE_PDH, "\\LogicalDisk(*)\\Disk Write Bytes/sec") {
        for letter in drive_letters_from_instance(&name) {
            out.entry(letter).or_default().1 += v.max(0.0) as u64;
        }
    }
    out
}

#[cfg(not(target_os = "windows"))]
fn sample_disk_rates() -> std::collections::HashMap<String, (u64, u64)> {
    std::collections::HashMap::new()
}

#[cfg(target_os = "windows")]
fn sample_pdh(
    slot: &Mutex<Option<pdh::PdhMultiCounter>>,
    path: &str,
) -> Vec<(String, f64)> {
    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = pdh::PdhMultiCounter::new(path);
    }
    guard.as_mut().map(|c| c.sample()).unwrap_or_default()
}

/// "C:" / "D:" → kept; "_Total", "HarddiskVolume1" → dropped.
#[cfg(target_os = "windows")]
fn drive_letters_from_instance(name: &str) -> Vec<String> {
    name.split_whitespace()
        .filter(|t| {
            t.len() == 2 && t.ends_with(':') && t.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        })
        .map(str::to_ascii_uppercase)
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskSpace {
    pub total: u64,
    pub used: u64,
    pub free: u64,
    pub usage_pct: f32,
    pub free_pct: f32,
}

/// Space of the volume holding `path` ("C:", "D:\\data", ...).  The disk
/// whose mount point is the longest prefix of the probe wins.
#[tauri::command]
pub fn get_disk_space(app: AppHandle, path: String) -> Result<DiskSpace, String> {
    let lang = app.state::<AppState>().lang();
    let probe = std::path::Path::new(&path)
        .components()
        .fold(std::path::PathBuf::new(), |mut acc, c| {
            acc.push(c.as_os_str());
            acc
        })
        .to_string_lossy()
        .to_lowercase();

    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut best: Option<&sysinfo::Disk> = None;
    for d in disks.list() {
        let mp = d.mount_point().to_string_lossy().to_lowercase();
        let mp = mp.trim_end_matches(['\\', '/']).to_string();
        let hit = probe == mp
            || probe.starts_with(&format!("{}\\", mp))
            || probe.starts_with(&format!("{}/", mp));
        if hit {
            if best.map_or(true, |b| {
                d.mount_point().as_os_str().len() > b.mount_point().as_os_str().len()
            }) {
                best = Some(d);
            }
        }
    }

    let d = best.ok_or_else(|| trf(&lang, Key::InvalidPath, &[path.as_str()]))?;
    let total = d.total_space();
    let free = d.available_space();
    let used = total.saturating_sub(free);
    let usage_pct = pct(used, total);
    Ok(DiskSpace {
        total,
        used,
        free,
        usage_pct,
        // 与 MemoryGroup 同约定：total 为 0（空光驱等）按 0/0 报
        free_pct: if total == 0 { 0.0 } else { 100.0 - usage_pct },
    })
}

// ─── Network ───

struct NetSampler {
    nets: sysinfo::Networks,
    last: Instant,
    primed: bool,
}

static NET_SAMPLER: Mutex<Option<NetSampler>> = Mutex::new(None);

#[derive(Debug, Clone, Serialize)]
pub struct NetworkAdapter {
    pub name: String,
    pub ips: Vec<String>,
    pub mac: String,
    /// Upload/download throughput in bytes/sec between calls (0 = baseline).
    pub upload_bps: u64,
    pub download_bps: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetworkInfo {
    pub adapters: Vec<NetworkAdapter>,
    /// Every non-loopback IP of the machine (flattened, deduped).
    pub local_ips: Vec<String>,
}

#[tauri::command]
pub fn get_network_info() -> NetworkInfo {
    let mut guard = NET_SAMPLER.lock().unwrap_or_else(|e| e.into_inner());
    let sampler = guard.get_or_insert_with(|| NetSampler {
        nets: sysinfo::Networks::new_with_refreshed_list(),
        last: Instant::now(),
        primed: false,
    });

    let elapsed = sampler.last.elapsed().as_secs_f64().max(1e-6);
    sampler.nets.refresh();

    let primed = sampler.primed;
    let mut local_ips: Vec<String> = Vec::new();
    let mut adapters = Vec::new();
    for (name, data) in sampler.nets.list() {
        let ips: Vec<String> = data
            .ip_networks()
            .iter()
            .map(|n| n.addr.to_string())
            // 回环（127. / ::1）与 IPv6 链路本地（fe80: 前缀——Windows
            // 实配的链路本地地址都在此前缀下）不进列表——皮肤展示的
            // 「本机 IP」要的是可路由地址
            .filter(|a| {
                !a.starts_with("127.") && a != "::1" && !a.to_ascii_lowercase().starts_with("fe80:")
            })
            .collect();
        for ip in &ips {
            if !local_ips.contains(ip) {
                local_ips.push(ip.clone());
            }
        }
        let (upload_bps, download_bps) = if primed {
            (
                (data.transmitted() as f64 / elapsed) as u64,
                (data.received() as f64 / elapsed) as u64,
            )
        } else {
            (0, 0)
        };
        adapters.push(NetworkAdapter {
            name: name.clone(),
            ips,
            mac: data.mac_address().to_string(),
            upload_bps,
            download_bps,
        });
    }

    sampler.primed = true;
    sampler.last = Instant::now();
    NetworkInfo { adapters, local_ips }
}

// ─── GPU (Windows) ───

/// async：DXGI 枚举 + PDH 采样 + 建 D3D12 设备（UMA 判定）都是重负载，
/// 不跑主线程——同 get_cpu_info / get_disks_info 的写法，逻辑不变。
#[tauri::command]
pub async fn get_gpu_info(app: AppHandle) -> Result<Vec<GpuInfo>, String> {
    #[cfg(target_os = "windows")]
    {
        let _ = &app; // used only by the non-Windows arm
        Ok(gpu::collect())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(windows_only(&app))
    }
}

// ─── Audio spectrum (Windows) ───

#[tauri::command]
pub fn get_audio_spectrum(app: AppHandle, bands: Option<usize>) -> Result<Spectrum, String> {
    #[cfg(target_os = "windows")]
    {
        let lang = app.state::<AppState>().lang();
        audio::spectrum(bands.unwrap_or(32), audio::Source::Loopback)
            .map_err(|e| trf(&lang, Key::AudioUnavailable, &[&e]))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = bands;
        Err(windows_only(&app))
    }
}

/// Microphone spectrum (permission: mic) — same shape as get_audio_spectrum.
#[tauri::command]
pub fn get_mic_spectrum(
    app: AppHandle,
    window: tauri::WebviewWindow,
    bands: Option<usize>,
) -> Result<Spectrum, String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    require_perm(&state, &window, PERM_MIC)?;
    #[cfg(target_os = "windows")]
    {
        audio::spectrum(bands.unwrap_or(32), audio::Source::Mic)
            .map_err(|e| trf(&lang, Key::AudioUnavailable, &[&e]))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = bands;
        Err(windows_only(&app))
    }
}

// ─── Status probes: battery / idle / foreground / monitors (read-only) ───

#[tauri::command]
pub fn get_battery_info(app: AppHandle) -> Result<BatteryInfo, String> {
    #[cfg(target_os = "windows")]
    {
        let _ = &app;
        status::battery()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(windows_only(&app))
    }
}

/// Milliseconds since the last keyboard/mouse input.
#[tauri::command]
pub fn get_idle_time(app: AppHandle) -> Result<u64, String> {
    #[cfg(target_os = "windows")]
    {
        let _ = &app;
        status::idle_ms()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(windows_only(&app))
    }
}

/// Currently focused window; null in the rare case there is none.
#[tauri::command]
pub fn get_foreground_window_info(app: AppHandle) -> Result<Option<ForegroundWindowInfo>, String> {
    #[cfg(target_os = "windows")]
    {
        let _ = &app;
        Ok(status::foreground_window())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(windows_only(&app))
    }
}

#[tauri::command]
pub fn get_monitors(app: AppHandle) -> Result<Vec<MonitorInfo>, String> {
    #[cfg(target_os = "windows")]
    {
        let _ = &app;
        Ok(status::monitors())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(windows_only(&app))
    }
}

// ─── Skin-local files (no permission needed — fs.rs sandboxes every
//     operation to the skin's own directory; caller_skin only establishes
//     WHICH skin is calling and fails fast for uninstalled skins) ───

#[tauri::command]
pub async fn skin_read_file(
    app: AppHandle,
    window: tauri::WebviewWindow,
    path: String,
    binary: Option<bool>,
) -> Result<String, String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    let dir = caller_skin(&state, &window)?.directory;
    fs::read_file(&dir, &path, binary.unwrap_or(false), &lang)
}

#[tauri::command]
pub fn skin_write_file(
    app: AppHandle,
    window: tauri::WebviewWindow,
    path: String,
    data: String,
    binary: Option<bool>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    let dir = caller_skin(&state, &window)?.directory;
    fs::write_file(&dir, &path, &data, binary.unwrap_or(false), &lang)
}

#[tauri::command]
pub fn skin_list_dir(
    app: AppHandle,
    window: tauri::WebviewWindow,
    path: Option<String>,
) -> Result<Vec<fs::DirEntry>, String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    let dir = caller_skin(&state, &window)?.directory;
    fs::list_dir(&dir, path.as_deref().unwrap_or("."), &lang)
}

#[tauri::command]
pub fn skin_delete_file(
    app: AppHandle,
    window: tauri::WebviewWindow,
    path: String,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    let dir = caller_skin(&state, &window)?.directory;
    fs::delete_file(&dir, &path, &lang)
}

// ─── Skin reads/writes its own custom settings (no permission needed) ───

/// Read one of the calling skin's OWN declared custom settings — 单条读取
/// 通道。只允许读自己 skin.json schema 里声明过的 key（含 password 类型：
/// 注入桥烘焙的 __DESK_PP__.settings 不含 password 值，本命令是其唯一
/// 读取通道）；返回有效值（用户覆盖值或 schema 默认值，与
/// effective_settings 同一套归并）。身份取自窗口 label，皮肤永远够不到
/// 其他皮肤的值。
#[tauri::command]
pub async fn skin_get_setting(
    window: tauri::WebviewWindow,
    app: AppHandle,
    key: String,
) -> Result<serde_json::Value, String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    let skin = caller_skin(&state, &window)?;
    if !skin.manifest.settings.iter().any(|d| d.key == key) {
        return Err(trf(&lang, Key::SkinHasNoSetting, &[skin.id.as_str(), key.as_str()]));
    }

    let overrides = crate::skin::settings::load_skin_settings(&skin.directory);
    let values = crate::skin::loader::effective_settings(&skin.manifest, Some(&overrides));
    Ok(values.get(&key).cloned().unwrap_or(serde_json::Value::Null))
}

/// Persist one of the calling skin's OWN declared custom settings — the same
/// `settings.json` the manager's 「皮肤设置」 page edits, so both sides share
/// one file.  Deliberately separate from the manager's
/// `set_skin_custom_setting`: here the caller's identity comes from its
/// window label, so a skin can never reach another skin's values, and only
/// keys declared in its own skin.json schema are writable (values are
/// validated/coerced exactly like the manager path).  No permission
/// declaration: reading these values is already free (baked into the
/// bridge), and the write cannot leave the skin's own folder.
#[tauri::command]
pub fn skin_set_setting(
    app: AppHandle,
    window: tauri::WebviewWindow,
    key: String,
    value: serde_json::Value,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    let skin = caller_skin(&state, &window)?;
    let def = skin.manifest.settings.iter()
        .find(|d| d.key == key)
        .ok_or_else(|| trf(&lang, Key::SkinHasNoSetting, &[skin.id.as_str(), key.as_str()]))?;

    let value = crate::commands::validate_custom_setting(def, &value, &lang)?;

    // 与 set_skin_custom_setting 同一把锁：settings.json 有两个写入方，
    // load→save 全程持锁防互相丢更新。
    let _guard = state.settings_lock.lock().unwrap_or_else(|e| e.into_inner());
    let mut overrides = crate::skin::settings::load_skin_settings(&skin.directory);
    overrides.insert(key.clone(), value.clone());
    crate::skin::settings::save_skin_settings(&skin.directory, &overrides)
        .map_err(|e| trf(&lang, Key::ConfigSaveFailed, &[&e.to_string()]))?;
    drop(_guard);

    // Notify the manager panel so an open config page refreshes in place.
    // 定向发给管理器窗口：广播会把设置值（可能含 password）泄露给所有皮肤窗口。
    let _ = app.emit_to("main", "skin-setting-changed", serde_json::json!({
        "skinId": skin.id,
        "key": key,
        "value": value,
    }));

    // Silently sync the caller's baked copy — WITHOUT dispatching
    // 'desk-setting-changed': the skin set this value itself, and an event
    // back would loop (its change handler may well write again).
    let key_json = serde_json::to_string(&key).map_err(|e| e.to_string())?;
    let val_json = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    let script = format!(
        "(function(){{var k={key},v={val};var b=window.__DESK_PP__;if(b){{b.settings=b.settings||{{}};b.settings[k]=v;}}}})();",
        key = key_json, val = val_json
    );
    let _ = window.eval(&script);

    Ok(())
}

// ─── Registry read (permission: registry, Windows) ───

#[tauri::command]
pub fn read_registry_value(
    app: AppHandle,
    window: tauri::WebviewWindow,
    root: String,
    path: String,
    name: String,
) -> Result<RegistryValue, String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    require_perm(&state, &window, PERM_REGISTRY)?;
    #[cfg(target_os = "windows")]
    {
        registry::read(&root, &path, &name, &lang)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (root, path, name);
        Err(windows_only(&app))
    }
}

// ─── Run command (permission: shell) ───

#[tauri::command]
pub async fn run_command(
    app: AppHandle,
    window: tauri::WebviewWindow,
    command: String,
    args: Option<Vec<String>>,
    timeout_ms: Option<f64>,
) -> Result<shell::CommandOutput, String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    require_perm(&state, &window, PERM_SHELL)?;
    let lang_inner = lang.clone();
    tauri::async_runtime::spawn_blocking(move || {
        shell::run(&command, &args.unwrap_or_default(), timeout_ms, &lang_inner)
    })
    .await
    .map_err(|e| trf(&lang, Key::TaskFailed, &[&e.to_string()]))?
}


// ─── OS / processes (read-only) ───

#[tauri::command]
pub fn get_os_info() -> system::OsInfo {
    system::os_info()
}

#[tauri::command]
pub async fn get_processes(sort: Option<String>, limit: Option<usize>) -> system::ProcessList {
    let mut guard = CPU_SYS.lock().unwrap_or_else(|e| e.into_inner());
    let sys = guard.get_or_insert_with(new_light_system);
    system::processes(sys, sort.as_deref().unwrap_or("cpu"), limit.unwrap_or(10))
}

// ─── Volume (Windows; get is read-only, set/mute need `system`) ───

#[tauri::command]
pub fn get_volume(app: AppHandle) -> Result<VolumeInfo, String> {
    #[cfg(target_os = "windows")]
    {
        let lang = app.state::<AppState>().lang();
        volume::get_volume().map_err(|e| trf(&lang, Key::VolumeFailed, &[&e]))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(windows_only(&app))
    }
}

#[tauri::command]
pub fn set_volume(app: AppHandle, window: tauri::WebviewWindow, volume_pct: f32) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    require_perm(&state, &window, PERM_SYSTEM)?;
    #[cfg(target_os = "windows")]
    {
        volume::set_volume(volume_pct).map_err(|e| trf(&lang, Key::VolumeFailed, &[&e]))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = volume_pct;
        Err(windows_only(&app))
    }
}

#[tauri::command]
pub fn set_mute(app: AppHandle, window: tauri::WebviewWindow, muted: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    require_perm(&state, &window, PERM_SYSTEM)?;
    #[cfg(target_os = "windows")]
    {
        volume::set_mute(muted).map_err(|e| trf(&lang, Key::VolumeFailed, &[&e]))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = muted;
        Err(windows_only(&app))
    }
}

// ─── Media: SMTC now-playing + transport (Windows) ───
// WinRT async is awaited with blocking .get() — must stay off the main
// thread, hence async + spawn_blocking (same rule as run_command).

#[tauri::command]
pub async fn get_media_info(app: AppHandle) -> Result<Option<MediaInfo>, String> {
    #[cfg(target_os = "windows")]
    {
        let lang = app.state::<AppState>().lang();
        let lang_inner = lang.clone();
        tauri::async_runtime::spawn_blocking(move || media::info())
            .await
            .map_err(|e| trf(&lang, Key::TaskFailed, &[&e.to_string()]))?
            .map_err(|e| trf(&lang_inner, Key::MediaControlFailed, &[&e]))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(windows_only(&app))
    }
}

#[tauri::command]
pub async fn media_control(app: AppHandle, window: tauri::WebviewWindow, action: String) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    require_perm(&state, &window, PERM_SYSTEM)?;

    #[cfg(target_os = "windows")]
    {
        let act = match action.as_str() {
            "play" => MediaAction::Play,
            "pause" => MediaAction::Pause,
            "play_pause" => MediaAction::PlayPause,
            "next" => MediaAction::Next,
            "previous" => MediaAction::Previous,
            other => return Err(trf(&lang, Key::InvalidMediaAction, &[other])),
        };
        let lang_inner = lang.clone();
        tauri::async_runtime::spawn_blocking(move || media::control(act))
            .await
            .map_err(|e| trf(&lang, Key::TaskFailed, &[&e.to_string()]))?
            .map_err(|e| trf(&lang_inner, Key::MediaControlFailed, &[&e]))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = action;
        Err(windows_only(&app))
    }
}

// ─── Clipboard (permission: clipboard) ───

#[tauri::command]
pub fn read_clipboard_text(app: AppHandle, window: tauri::WebviewWindow) -> Result<String, String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    require_perm(&state, &window, PERM_CLIPBOARD)?;
    use tauri_plugin_clipboard_manager::ClipboardExt;
    app.clipboard()
        .read_text()
        .map_err(|e| trf(&lang, Key::ClipboardFailed, &[&e.to_string()]))
}

#[tauri::command]
pub fn write_clipboard_text(app: AppHandle, window: tauri::WebviewWindow, text: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    require_perm(&state, &window, PERM_CLIPBOARD)?;
    use tauri_plugin_clipboard_manager::ClipboardExt;
    app.clipboard()
        .write_text(text)
        .map_err(|e| trf(&lang, Key::ClipboardFailed, &[&e.to_string()]))
}

// ─── Open external link/file (permission: system) ───

/// http(s) / mailto 链接，或本地绝对路径（目录、文档等）。拒绝：其他
/// scheme（file:/javascript: 等）、相对路径、UNC 路径（\\、// 前缀——
/// 可能触发 NTLM 认证外发）、可直接执行/被系统当代码解析的扩展名
/// （ShellExecute 打开等同于运行）。目标是否存在不做提前探测：不存在
/// 与打开失败统一报 OpenFailed，消除路径存在性探针。
fn is_open_target_allowed(target: &str) -> bool {
    let t = target.trim();
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:") {
        return true;
    }
    if t.starts_with("\\\\") || t.starts_with("//") {
        return false;
    }
    let path = std::path::Path::new(t);
    if !path.is_absolute() {
        return false;
    }
    !is_blocked_executable(path)
}

/// 可直接执行/被系统当代码解析的扩展名黑名单（大小写不敏感）。
fn is_blocked_executable(path: &std::path::Path) -> bool {
    const BLOCKED: [&str; 23] = [
        "exe", "bat", "cmd", "ps1", "vbs", "vbe", "js", "jse", "wsf", "wsh",
        "msi", "msp", "scr", "com", "pif", "cpl", "lnk", "hta", "reg", "dll",
        "msc", "jar", "url",
    ];
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| BLOCKED.contains(&e.to_ascii_lowercase().as_str()))
}

#[tauri::command]
pub fn open_external(app: AppHandle, window: tauri::WebviewWindow, target: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    require_perm(&state, &window, PERM_SYSTEM)?;
    let target = target.trim();
    if !is_open_target_allowed(target) {
        return Err(trf(&lang, Key::InvalidTarget, &[target]));
    }
    open_target_impl(target, &lang)
}

/// Open with the OS default handler (browser / associated app).  Windows
/// uses ShellExecuteW directly — the shell plugin's `open` is deprecated.
/// pub(crate)：管理器的「前往下载」（commands::open_release_page）也走这里。
#[cfg(target_os = "windows")]
pub(crate) fn open_target_impl(target: &str, lang: &str) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    // Values ≤ 32 are failure codes (file not found, no association, ...).
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR::null(),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as usize > 32 {
        Ok(())
    } else {
        Err(trf(lang, Key::OpenFailed, &[&format!("ShellExecute code {}", result.0 as usize)]))
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn open_target_impl(target: &str, lang: &str) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|e| trf(lang, Key::OpenFailed, &[&e.to_string()]))
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
pub(crate) fn open_target_impl(target: &str, lang: &str) -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|e| trf(lang, Key::OpenFailed, &[&e.to_string()]))
}

// ─── Toast notification (permission: system, Windows) ───

/// Startup identity setup for toasts: make sure the AUMID shortcut exists
/// and points at the current exe (icon included).  Called once from setup so
/// the taskbar's AUMID→shortcut icon resolution always lands on the real
/// binary — a stale shortcut (dev/test/relocated install) shows the default
/// program icon instead.
#[cfg(target_os = "windows")]
pub fn ensure_notification_identity() {
    if let Err(e) = notify::ensure_aumid_shortcut() {
        log::warn!("notification identity setup failed: {}", e);
    }
}

#[tauri::command]
pub fn show_notification(
    app: AppHandle,
    window: tauri::WebviewWindow,
    title: String,
    body: Option<String>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    require_perm(&state, &window, PERM_SYSTEM)?;
    #[cfg(target_os = "windows")]
    {
        notify::show(&title, body.as_deref().unwrap_or(""))
            .map_err(|e| trf(&lang, Key::NotificationFailed, &[&e]))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (title, body);
        Err(windows_only(&app))
    }
}

// ─── Hardware probe (manual) ───

#[cfg(test)]
mod tests {
    #[test]
    fn open_target_validation() {
        assert!(super::is_open_target_allowed("https://example.com"));
        assert!(super::is_open_target_allowed("http://example.com"));
        assert!(super::is_open_target_allowed("mailto:a@b.c"));
        assert!(!super::is_open_target_allowed("file:///c:/windows"));
        assert!(!super::is_open_target_allowed("javascript:alert(1)"));
        assert!(!super::is_open_target_allowed("relative/path.txt"));
        // 存在性不再提前探测：不存在的普通路径也放行，由打开失败统一报错
        assert!(super::is_open_target_allowed("C:\\no-such-file-driftlet.xyz"));
        // 可执行扩展名（含大小写变体）一律拒绝；exe 本体也不例外
        let abs = std::env::current_exe().unwrap();
        assert!(!super::is_open_target_allowed(abs.to_str().unwrap()));
        assert!(!super::is_open_target_allowed("C:\\tools\\RUN.EXE"));
        assert!(!super::is_open_target_allowed("C:\\tools\\script.Ps1"));
        assert!(!super::is_open_target_allowed("C:\\tools\\app.lnk"));
        assert!(!super::is_open_target_allowed("C:\\tools\\doc.hta"));
        // UNC 路径（两种前缀）一律拒绝
        assert!(!super::is_open_target_allowed("\\\\server\\share\\doc.pdf"));
        assert!(!super::is_open_target_allowed("//server/share/doc.pdf"));
    }

    #[test]
    #[ignore = "hardware probe — run manually with --nocapture"]
    fn probe_hardware_info() {
        let _ = tauri::async_runtime::block_on(super::get_cpu_info());
        let _ = tauri::async_runtime::block_on(super::get_disks_info());
        let _ = super::get_network_info();
        std::thread::sleep(std::time::Duration::from_secs(1));
        println!("CPU   : {:#?}", tauri::async_runtime::block_on(super::get_cpu_info()));
        println!("MEM   : {:#?}", super::get_memory_info());
        println!("DISKS : {:#?}", tauri::async_runtime::block_on(super::get_disks_info()));
        println!("NET   : {:#?}", super::get_network_info());
    }

    #[test]
    #[ignore = "hardware probe — run manually with --nocapture"]
    fn probe_cpu_frequency() {
        // 第一次 = PDH 基线（None → 回退 sysinfo 静态值）；第二次 = 实测值
        println!("first : {:?}", super::cpu_performance_pct());
        std::thread::sleep(std::time::Duration::from_secs(1));
        println!("second: {:?}", super::cpu_performance_pct());
    }

    /// 对比「任务管理器速度」的候选口径（实测结论：直读 MHz 的 Processor
    /// Frequency 计数器在台式机平台恒报名义值 2808 不跳动——i5-8400 实测；
    /// % Processor Performance 全平台逐秒真实波动、turbo 超 100%，TM 速度
    /// = 名义频率 × 该百分比）。前半段空闲、后半段部分核加负载。
    #[test]
    #[ignore = "hardware probe — run manually with --nocapture"]
    fn probe_cpu_frequency_variants() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::Duration;

        const NOMINAL_MHZ: f64 = 2808.0; // 本机 i5-8400，按实际机器调整

        let mut freq = super::pdh::PdhMultiCounter::new(
            "\\Processor Information(*)\\Processor Frequency",
        )
        .expect("freq counter");
        let mut perf = super::pdh::PdhMultiCounter::new(
            "\\Processor Information(*)\\% Processor Performance",
        )
        .expect("perf counter");
        let _ = freq.sample(); // 基线
        let _ = perf.sample();

        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let mut workers = Vec::new();
        println!("phase  | freq_Total | core min~max | core avg | %Perf_Total | nominal×%Perf");
        for round in 0..8 {
            if round == 4 {
                // 半载：6 核机器起 3 个自旋线程
                for _ in 0..3 {
                    let s = stop.clone();
                    workers.push(std::thread::spawn(move || {
                        let mut x = 0u64;
                        while !s.load(Ordering::Relaxed) {
                            x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                            std::hint::black_box(x);
                        }
                    }));
                }
            }
            std::thread::sleep(Duration::from_secs(1));
            let f = freq.sample();
            let p = perf.sample();
            let total_f = f.iter().find(|(n, _)| n == "_Total").map(|(_, v)| *v);
            let cores: Vec<f64> = f
                .iter()
                .filter(|(n, _)| n != "_Total")
                .map(|(_, v)| *v)
                .filter(|v| *v > 0.0)
                .collect();
            let total_p = p.iter().find(|(n, _)| n == "_Total").map(|(_, v)| *v);
            let min = cores.iter().cloned().reduce(f64::min).unwrap_or(0.0);
            let max = cores.iter().cloned().reduce(f64::max).unwrap_or(0.0);
            let avg = cores.iter().sum::<f64>() / cores.len().max(1) as f64;
            println!(
                "round{} | {:9.0} | {:5.0}~{:5.0} | {:8.0} | {:11.1} | {:12.0}",
                round,
                total_f.unwrap_or(f64::NAN),
                min,
                max,
                avg,
                total_p.unwrap_or(f64::NAN),
                total_p.unwrap_or(0.0) * NOMINAL_MHZ / 100.0,
            );
        }
        stop.store(true, Ordering::Relaxed);
        for w in workers {
            let _ = w.join();
        }
    }
}
