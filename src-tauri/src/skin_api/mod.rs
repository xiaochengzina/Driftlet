//! Skin-facing backend APIs: read-only system information plus the
//! permission-gated capabilities.  Seven permissions, each gating its own
//! command set:
//!
//!   * `registry`  — read_registry_value
//!   * `shell`     — run_command
//!   * `system`    — set_volume / set_mute / media_control / open_external /
//!                   show_notification
//!   * `clipboard` — read_clipboard_text / write_clipboard_text
//!   * `mic`       — get_mic_spectrum
//!   * `file_system` — skin_read_any_file / skin_write_any_file（任意绝对路径，高危）
//!   * `control`   — skin_list_skins / skin_get_window_config /
//!                   skin_set_window_config / skin_load / skin_unload /
//!                   skin_reload / skin_hide / skin_show（作用于自己免权限）
//!
//! （皮肤自身目录内的文件读写不需要权限——fs.rs 的沙箱即边界；`http_request`
//! 亦免权限——页面本有 fetch 通道，设闸挡不住有心者；曾有过的 `files`
//! 权限已整体取消，该名字永不复活。）
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
/// Arbitrary-path file read/write (read_any_file / write_any_file) — goes
/// past the fs.rs skin-directory sandbox to the whole disk, hence high risk.
pub const PERM_FILE_SYSTEM: &str = "file_system";
/// Skin window-config control (skin_get_window_config /
/// skin_set_window_config) — reads and modifies ANY skin's window config
/// (position/size/placement/lock/…, other skins included).
pub const PERM_CONTROL: &str = "control";

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
/// 同步重活挪 spawn_blocking（DXGI/D3D12 全同步阻塞 async worker，
/// 高频轮询会停满 worker 池）。
#[tauri::command]
pub async fn get_gpu_info(app: AppHandle) -> Result<Vec<GpuInfo>, String> {
    #[cfg(target_os = "windows")]
    {
        let _ = &app; // used only by the non-Windows arm
        tauri::async_runtime::spawn_blocking(gpu::collect)
            .await
            .map_err(|e| e.to_string())
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

/// 检测 Windows 系统级浅色/深色主题（HKCU\…\Themes\Personalize 的
/// AppsUseLightTheme：1 浅 0 深），返回 "light" / "dark"。只读系统信息，
/// 免权限（与 §5.2 其他只读命令同组，无身份门槛）。系统主题变化不做
/// 推送——皮肤在需要时调用，或配合定时轮询/窗口可见事件刷新。
#[tauri::command]
pub fn get_system_theme(app: AppHandle) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        let _ = &app;
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let light = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
            .and_then(|k| k.get_value::<u32, _>("AppsUseLightTheme"))
            .unwrap_or(1); // 读不到按浅色兜底（Windows 默认主题为浅）
        Ok(if light == 0 { "dark".to_string() } else { "light".to_string() })
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
    // 与管理器侧 set_skin_custom_setting 同款：只记 key，（by skin）标出来源
    log::info!("Skin setting changed: {} key={} (by skin)", skin.id, key);

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

// ─── Skin log messages (no permission needed) ───

/// 皮肤主动发一条消息到宿主日志（设置页可打开的日志窗口）。caller_skin 只做
/// 身份识别：消息只进本机内存环形缓冲，无权限声明。level 只认
/// "warn"/"error"，缺省 info；source 自动带皮肤 id，便于开发者定位归属。
#[tauri::command]
pub fn skin_log(
    app: AppHandle,
    window: tauri::WebviewWindow,
    level: Option<String>,
    message: String,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let skin = caller_skin(&state, &window)?;
    crate::app_log::push(
        crate::app_log::LogLevel::from_level_str(level.as_deref()),
        format!("skin:{}", skin.id),
        message,
    );
    Ok(())
}

// ─── Skin console forwarding (no permission needed) ───

#[derive(serde::Deserialize)]
pub struct ConsoleEntry {
    level: Option<String>,
    message: String,
}

/// 皮肤 webview 控制台输出的批量上报通道：注入桥自动捕获 console.*、未捕获
/// 异常/rejection、资源加载失败与 CSP 拦截，队列每 250ms 整批上报一次。
/// 身份取自窗口 label（同 show_skin_context_menu），不走 caller_skin 的全量
/// 扫盘——本命令是持续高频通道，每批扫一次盘不值。消息只进本机内存环形
/// 缓冲（与 skin_log 同一口径），无权限声明。
#[tauri::command]
pub fn skin_console_log(
    window: tauri::WebviewWindow,
    entries: Vec<ConsoleEntry>,
) -> Result<(), String> {
    let state = window.app_handle().state::<AppState>();
    let lang = state.lang();
    let Some(skin_id) = window.label().strip_prefix("skin-").map(str::to_string) else {
        return Err(tr(&lang, Key::NotASkinWindow).to_string());
    };
    // 每批条数兜底截断：桥接侧已有 flush 上限，这里防绕过桥直接调的失控皮肤
    for entry in entries.into_iter().take(60) {
        crate::app_log::push(
            crate::app_log::LogLevel::from_level_str(entry.level.as_deref()),
            format!("skin:{}", skin_id),
            entry.message,
        );
    }
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
    // 尾点/尾空格绕过：Windows 路径规范化剥掉分量尾部的点与空格
    //（"RUN.EXE." 落盘即 "RUN.EXE"），而 Path::extension() 在剥前看——
    // "RUN.EXE." 的扩展名是 Some("")、"RUN.EXE " 是 Some("exe ")，均绕过
    // 黑名单。分量尾点/尾空格一律拒绝（fs.rs 沙箱同款防护，此路径曾漏）。
    for c in path.components() {
        if let std::path::Component::Normal(s) = c {
            match s.to_str() {
                Some(s) if s.ends_with('.') || s.ends_with(' ') => return false,
                _ => {}
            }
        }
    }
    !is_blocked_executable(path)
}

/// 可直接执行/被系统当代码解析、或能间接触发远程连接（NTLM 外泄面）的
/// 扩展名黑名单（大小写不敏感）。
fn is_blocked_executable(path: &std::path::Path) -> bool {
    const BLOCKED: [&str; 35] = [
        "exe", "bat", "cmd", "ps1", "vbs", "vbe", "js", "jse", "wsf", "wsh",
        "msi", "msp", "scr", "com", "pif", "cpl", "lnk", "hta", "reg", "dll",
        "msc", "jar", "url",
        // 间接远程连接面：Explorer 搜索/库可指向远程共享（NTLM 哈希外泄）、
        // ClickOnce 激活、msdt 诊断包、Internet 快捷方式变体
        "search-ms", "library-ms", "application", "appref-ms", "diagcab", "website",
        // 补充：.chm（hh.exe ActiveX 执行）、.settingcontent-ms（CVE-2018-8414
        // 控制面板项加载）、.scf（命令执行 + NTLM 外泄）；.hlp 已随 Win10 移除、
        // .wsc/.sct 需脚本宿主注册表键在，但同为可执行内容一并拒
        "chm", "settingcontent-ms", "scf", "hlp", "wsc", "sct",
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
pub async fn show_notification(
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
        // COM + 写盘 IO 不堵 async worker（同步版曾在主线程 IPC 上下文
        // 跑，同样问题域）
        let outer_lang = lang.clone();
        tauri::async_runtime::spawn_blocking(move || {
            notify::show(&title, body.as_deref().unwrap_or(""))
        })
        .await
        .map_err(|e| trf(&outer_lang, Key::TaskFailed, &[&e.to_string()]))?
        .map_err(|e| trf(&outer_lang, Key::NotificationFailed, &[&e]))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (title, body);
        Err(windows_only(&app))
    }
}

// ─── 任意路径文件读写（permission: file_system —— 高危）───
//
// 与 fs.rs 的沙箱命令（skin_read_file 等，免权限、限皮肤自身目录）不同：
// 这两条接受任意**绝对路径**，整盘可读可写。错误一律透传系统错误文案
// （皮肤需要知道真实失败原因）；相对路径拒绝（没有可参照的工作目录）。

fn any_absolute_path(path: &str) -> Result<std::path::PathBuf, String> {
    let trimmed = path.trim();
    // UNC 一律拒绝（与 open_external 同口径）：访问会触发 SMB 连接与
    // NTLM 认证外发（哈希外泄面）
    if trimmed.starts_with("\\\\") || trimmed.starts_with("//") {
        return Err(format!("UNC paths are not allowed: {}", trimmed));
    }
    let p = std::path::PathBuf::from(trimmed);
    if !p.is_absolute() {
        return Err(format!("path must be absolute: {}", trimmed));
    }
    // 前缀分量再兜一道 UNC/VerbatimUNC（双前缀形态之外的写法）
    #[cfg(target_os = "windows")]
    if let Some(std::path::Component::Prefix(prefix)) = p.components().next() {
        use std::path::Prefix;
        if matches!(prefix.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..)) {
            return Err(format!("UNC paths are not allowed: {}", trimmed));
        }
    }
    Ok(p)
}

#[tauri::command]
pub async fn skin_read_any_file(
    app: AppHandle,
    window: tauri::WebviewWindow,
    path: String,
    binary: Option<bool>,
) -> Result<String, String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_FILE_SYSTEM)?;
    let p = any_absolute_path(&path)?;
    let meta = std::fs::metadata(&p).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err(format!("not a regular file: {}", path));
    }
    if meta.len() > fs::MAX_READ_BYTES {
        return Err(format!("file too large: {} bytes (max {})", meta.len(), fs::MAX_READ_BYTES));
    }
    // TOCTOU 防线：metadata 检查后文件可能被换大——按上限+1 流式读，
    // 超限即拒，不做无界整文件分配
    let mut buf = Vec::new();
    {
        use std::io::Read;
        std::fs::File::open(&p)
            .map_err(|e| e.to_string())?
            .take(fs::MAX_READ_BYTES + 1)
            .read_to_end(&mut buf)
            .map_err(|e| e.to_string())?;
    }
    if buf.len() as u64 > fs::MAX_READ_BYTES {
        return Err(format!("file too large: > {} bytes (max {})", fs::MAX_READ_BYTES, fs::MAX_READ_BYTES));
    }
    let bytes = buf;
    if binary.unwrap_or(false) {
        use base64::Engine;
        Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
    } else {
        String::from_utf8(bytes).map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub fn skin_write_any_file(
    app: AppHandle,
    window: tauri::WebviewWindow,
    path: String,
    data: String,
    binary: Option<bool>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_FILE_SYSTEM)?;
    let bytes = if binary.unwrap_or(false) {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| e.to_string())?
    } else {
        data.into_bytes()
    };
    if bytes.len() > fs::MAX_WRITE_BYTES {
        return Err(format!("data too large: {} bytes (max {})", bytes.len(), fs::MAX_WRITE_BYTES));
    }
    let p = any_absolute_path(&path)?;
    // 与沙箱版一致：缺失的父目录一并创建
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, bytes).map_err(|e| e.to_string())
}

/// 列任意目录（与沙箱版 skin_list_dir 同款 DirEntry 结构；目录项排前、
/// 名称小写排序）。
#[tauri::command]
pub fn skin_list_any_dir(
    app: AppHandle,
    window: tauri::WebviewWindow,
    path: String,
) -> Result<Vec<fs::DirEntry>, String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_FILE_SYSTEM)?;
    let p = any_absolute_path(&path)?;
    let meta = std::fs::metadata(&p).map_err(|e| e.to_string())?;
    if !meta.is_dir() {
        return Err(format!("not a directory: {}", path));
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&p).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let md = entry.metadata().map_err(|e| e.to_string())?;
        out.push(fs::DirEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            is_dir: md.is_dir(),
            size: if md.is_dir() { 0 } else { md.len() },
        });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

/// 建任意目录（含多级，已存在视为成功）。
#[tauri::command]
pub fn skin_create_any_dir(
    app: AppHandle,
    window: tauri::WebviewWindow,
    path: String,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_FILE_SYSTEM)?;
    let p = any_absolute_path(&path)?;
    std::fs::create_dir_all(&p).map_err(|e| e.to_string())
}

/// 删任意路径：文件直接删；目录默认只删空目录，整棵目录树须显式
/// recursive: true（防一条命令误删一片——写权限同层但删除不可逆，
/// 多要一个开关）。
#[tauri::command]
pub fn skin_delete_any_path(
    app: AppHandle,
    window: tauri::WebviewWindow,
    path: String,
    recursive: Option<bool>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_FILE_SYSTEM)?;
    let p = any_absolute_path(&path)?;
    let meta = std::fs::metadata(&p).map_err(|e| e.to_string())?;
    if meta.is_dir() {
        if recursive.unwrap_or(false) {
            std::fs::remove_dir_all(&p).map_err(|e| e.to_string())
        } else {
            std::fs::remove_dir(&p).map_err(|e| e.to_string())
        }
    } else {
        std::fs::remove_file(&p).map_err(|e| e.to_string())
    }
}

// ─── 皮肤窗口配置项控制（permission: control —— 中危）───
//
// 读取/修改任意皮肤（含自己）的窗口配置项。修改逐项分发到管理器命令的
// 进程内实现（commands::set_skin_*_impl），语义与面板操作完全一致——
// 只读读取与运行态应用、持久化、钳制全走同一条路径，不另抄一份。

/// 窗口配置项读取结果（与 get_skin_detail 同款有效值口径：
/// resizable/zoom 取 None 时回退 skin.json 默认；宽高 = 基础尺寸 × 有效 zoom）
#[derive(Debug, Clone, Serialize)]
pub struct SkinWindowConfigInfo {
    pub loaded: bool,
    pub opacity: f64,
    pub always_on_top: bool,
    pub on_desktop: bool,
    pub click_through: bool,
    pub position_locked: bool,
    pub resizable: bool,
    pub zoom: f64,
    pub edge_snap: bool,
    pub snap_gap: u32,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: u32,
    pub height: u32,
}

#[tauri::command]
pub fn skin_get_window_config(
    app: AppHandle,
    window: tauri::WebviewWindow,
    skin_id: Option<String>,
) -> Result<SkinWindowConfigInfo, String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    let skin_id = resolve_control_target(&state, &window, skin_id)?;
    let skins = crate::skin::loader::scan_skins_directory(&state.skins_dir);
    let skin = skins
        .iter()
        .find(|s| s.id == skin_id)
        .ok_or_else(|| trf(&lang, Key::SkinNotFound, &[skin_id.as_str()]))?;
    let loaded = state.registry.is_loaded(&skin_id);
    let mut cfg = {
        let app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
        app_config
            .skin_settings
            .get(&skin_id)
            .cloned()
            .unwrap_or_else(|| crate::skin::types::SkinRuntimeConfig::from_manifest(&skin.manifest))
    };
    let resizable = cfg.resizable.unwrap_or(skin.manifest.window.resizable);
    let zoom = crate::commands::clamp_zoom(cfg.zoom.unwrap_or(skin.manifest.window.zoom));
    cfg.width = ((cfg.width as f64) * zoom).round() as u32;
    cfg.height = ((cfg.height as f64) * zoom).round() as u32;
    Ok(SkinWindowConfigInfo {
        loaded,
        opacity: cfg.opacity,
        always_on_top: cfg.always_on_top,
        on_desktop: cfg.on_desktop,
        click_through: cfg.click_through,
        position_locked: cfg.position_locked,
        resizable,
        zoom,
        edge_snap: cfg.edge_snap,
        snap_gap: cfg.snap_gap,
        x: cfg.x,
        y: cfg.y,
        width: cfg.width,
        height: cfg.height,
    })
}

/// 修改窗口配置项（patch 按键逐项应用）。**先全量校验再动手**——未知键或
/// 值类型不对时整个 patch 拒绝，不留改了一半的配置。
/// 支持的键与类型：opacity(0.1–1.0) / placement("top"|"desktop") /
/// click_through(bool) / position_locked(bool) / resizable(bool) /
/// zoom(0.5–2.0) / edge_snap(bool) / snap_gap(uint) / x,y(int，逻辑像素) /
/// width,height(uint，所见实际尺寸)。
/// 权限：省略 skinId（或空串/传自己 id）= 改自己，**免权限**——全键放开
///（自己的窗口自己调，与显隐自己免权限同例）；指定其他皮肤一律 control。
/// 注意：opacity / x,y / width,height / position_locked / resizable 五项
/// 要求目标皮肤已加载（运行态 eval/几何操作需要窗口），未加载时报
/// SkinNotLoaded；其余键未加载时仅持久化，下次建窗生效。
/// 应用顺序固定为 zoom → size → position → 其余（size 持久化要按新 zoom 折算）。
#[tauri::command]
pub fn skin_set_window_config(
    app: AppHandle,
    window: tauri::WebviewWindow,
    skin_id: Option<String>,
    patch: serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let skin_id = resolve_control_target(&state, &window, skin_id)?;

    // ── 第一遍：解析 + 校验（任何一项不合法，整个 patch 不落地）──
    enum Op {
        Zoom(f64),
        Size(u32, u32),
        Position(i32, i32),
        Opacity(f64),
        Placement(String),
        ClickThrough(bool),
        PositionLocked(bool),
        Resizable(bool),
        EdgeSnap(bool),
        SnapGap(u32),
    }
    const KEYS: &str = "opacity/placement/click_through/position_locked/resizable/zoom/edge_snap/snap_gap/x/y/width/height";
    let mut ops: Vec<Op> = Vec::new();
    let mut pos: (Option<i32>, Option<i32>) = (None, None);
    let mut size: (Option<u32>, Option<u32>) = (None, None);
    for (key, value) in &patch {
        let bad = || format!("invalid value for '{}': {} (支持 {})", key, value, KEYS);
        match key.as_str() {
            "opacity" => ops.push(Op::Opacity(value.as_f64().ok_or_else(bad)?)),
            "placement" => {
                let v = value.as_str().ok_or_else(bad)?;
                if v != "top" && v != "desktop" {
                    return Err(bad());
                }
                ops.push(Op::Placement(v.to_string()));
            }
            "click_through" => ops.push(Op::ClickThrough(value.as_bool().ok_or_else(bad)?)),
            "position_locked" => ops.push(Op::PositionLocked(value.as_bool().ok_or_else(bad)?)),
            "resizable" => ops.push(Op::Resizable(value.as_bool().ok_or_else(bad)?)),
            "zoom" => ops.push(Op::Zoom(value.as_f64().ok_or_else(bad)?)),
            "edge_snap" => ops.push(Op::EdgeSnap(value.as_bool().ok_or_else(bad)?)),
            // 截断回绕防护：超大 JSON 数字在 impl 的 clamp 之前先 as 截断
            // 会静默回绕（4294967296_u64 as u32 = 0）——try_from 越界报错
            "snap_gap" => ops.push(Op::SnapGap(u32::try_from(value.as_u64().ok_or_else(bad)?).map_err(|_| bad())?)),
            "x" => pos.0 = Some(i32::try_from(value.as_i64().ok_or_else(bad)?).map_err(|_| bad())?),
            "y" => pos.1 = Some(i32::try_from(value.as_i64().ok_or_else(bad)?).map_err(|_| bad())?),
            "width" => size.0 = Some(u32::try_from(value.as_u64().ok_or_else(bad)?).map_err(|_| bad())?),
            "height" => size.1 = Some(u32::try_from(value.as_u64().ok_or_else(bad)?).map_err(|_| bad())?),
            other => return Err(format!("unknown config key: '{}' (支持 {})", other, KEYS)),
        }
    }
    // x/y、width/height 合并成单次调用；缺的一边取**当前实际几何**（已加载
    // 时读窗口现场——回退持久化基础尺寸会在 zoom≠1 时被 impl 再除一次
    // zoom 双重缩小；位置回退 (0,0) 会把窗口跳去左上角）。未加载时回退
    // 持久化值（size/position 的 impl 本就要求已加载，此分支是兜底）。
    if pos.0.is_some() || pos.1.is_some() {
        let (cx, cy) = current_geometry(&state, &skin_id).0;
        ops.push(Op::Position(pos.0.unwrap_or(cx), pos.1.unwrap_or(cy)));
    }
    if size.0.is_some() || size.1.is_some() {
        let (cw, ch) = current_geometry(&state, &skin_id).1;
        ops.push(Op::Size(size.0.unwrap_or(cw), size.1.unwrap_or(ch)));
    }

    // ── 第二遍：按固定顺序应用（zoom 先于 size，其余按声明顺序）──
    ops.sort_by_key(|op| match op {
        Op::Zoom(_) => 0,
        Op::Size(_, _) => 1,
        Op::Position(_, _) => 2,
        _ => 3,
    });
    for op in ops {
        match op {
            Op::Zoom(v) => crate::commands::set_skin_zoom_impl(&app, &skin_id, v)?,
            Op::Size(w, h) => crate::commands::set_skin_size_impl(&app, &skin_id, w, h)?,
            Op::Position(x, y) => crate::commands::set_skin_position_impl(&app, &skin_id, x, y)?,
            Op::Opacity(v) => crate::commands::set_skin_opacity_impl(&app, &skin_id, v)?,
            Op::Placement(v) => crate::commands::set_skin_placement_impl(&app, &skin_id, &v)?,
            Op::ClickThrough(v) => crate::commands::set_skin_click_through_impl(&app, &skin_id, v)?,
            Op::PositionLocked(v) => crate::commands::set_skin_position_locked_impl(&app, &skin_id, v)?,
            Op::Resizable(v) => crate::commands::set_skin_resizable_impl(&app, &skin_id, v)?,
            Op::EdgeSnap(v) => crate::commands::set_skin_edge_snap_impl(&app, &skin_id, v)?,
            Op::SnapGap(v) => crate::commands::set_skin_snap_gap_impl(&app, &skin_id, v)?,
        }
    }
    log::info!("Skin window config patched: {} keys={:?} (by skin {})", skin_id, patch.keys().collect::<Vec<_>>(), window.label());
    Ok(())
}

// ─── 皮肤生命周期控制（自己免权限 / 他人 control 中危）───
//
// 加载/卸载/重载任意皮肤。skinId 省略/空串/传自己 id = 作用于自己，免权限
//（resolve_control_target 统一收敛——与窗口配置/显隐同一约定）；指定他人
// 过 control 门。目标是自己时走 fire-and-forget——动作会销毁发起调用的
// webview 自身，await 会让 wry 把 invoke 响应投递到死窗口（与皮肤右键
// 菜单的刷新/卸载同一教训），此时返回值不可依赖。

#[tauri::command]
pub async fn skin_load(app: AppHandle, window: tauri::WebviewWindow, skin_id: Option<String>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let target = resolve_control_target(&state, &window, skin_id)?;
    // 生命周期互斥（与 load_skin 命令同一把锁；锁序 lifecycle → install）。
    // guard 借用 state（借用自 app），impl 调用用 app.clone() 防 move 冲突
    let _a = state.lifecycle_lock.lock().await;
    let _b = state.install_lock.lock().await;
    crate::commands::load_skin_impl(app.clone(), target).await
}

#[tauri::command]
pub async fn skin_unload(app: AppHandle, window: tauri::WebviewWindow, skin_id: Option<String>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let target = resolve_control_target(&state, &window, skin_id)?;
    if is_caller(&window, &target) {
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            let _a = state.lifecycle_lock.lock().await;
            let _b = state.install_lock.lock().await;
            let _ = crate::commands::unload_skin_impl(app.clone(), target).await;
        });
        return Ok(());
    }
    let _a = state.lifecycle_lock.lock().await;
    let _b = state.install_lock.lock().await;
    crate::commands::unload_skin_impl(app.clone(), target).await
}

#[tauri::command]
pub async fn skin_reload(app: AppHandle, window: tauri::WebviewWindow, skin_id: Option<String>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let target = resolve_control_target(&state, &window, skin_id)?;
    if is_caller(&window, &target) {
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            let _a = state.lifecycle_lock.lock().await;
            let _b = state.install_lock.lock().await;
            let _ = crate::commands::reload_skin_impl(app.clone(), target).await;
        });
        return Ok(());
    }
    let _a = state.lifecycle_lock.lock().await;
    let _b = state.install_lock.lock().await;
    crate::commands::reload_skin_impl(app.clone(), target).await
}

/// 目标是否是发起调用的皮肤自己（label 前缀反查；caller_skin 已验过身份）
fn is_caller(window: &tauri::WebviewWindow, target: &str) -> bool {
    window.label().strip_prefix("skin-") == Some(target)
}

// ─── 皮肤清单（permission: control —— 中危）───
//
// control 权限的入口命令：没有它，跨皮肤操作只能靠猜 id。返回全部已安装
// 皮肤的 id/名称/版本/作者与加载态（loaded + hidden）——control 皮肤
// 属于受信管理员角色，名称与加载态不构成额外隐私面。

/// skin_list_skins 的条目结构
#[derive(Debug, Clone, Serialize)]
pub struct SkinListEntry {
    pub id: String,
    pub name: String,
    pub name_en: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub loaded: bool,
    pub hidden: bool,
}

#[tauri::command]
pub fn skin_list_skins(app: AppHandle, window: tauri::WebviewWindow) -> Result<Vec<SkinListEntry>, String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_CONTROL)?;
    let skins = crate::skin::loader::scan_skins_directory(&state.skins_dir);
    Ok(skins
        .into_iter()
        .map(|s| SkinListEntry {
            loaded: state.registry.is_loaded(&s.id),
            hidden: state
                .registry
                .get(&s.id)
                .map(|w| !w.is_visible().unwrap_or(true))
                .unwrap_or(false),
            id: s.id,
            name: s.manifest.name,
            name_en: s.manifest.name_en,
            version: s.manifest.version,
            author: s.manifest.author,
        })
        .collect())
}

// ─── 通用 HTTP 请求（免权限）───
//
// 突破页面 fetch 的 CORS 限制：任意 http(s) URL、自定义头、文本/二进制负载。
// 免权限的理由：皮肤页面本就有 fetch 通道（no-cors POST 已能外发数据），
// 单独设闸挡不住有心者，只做展示噪音；身份仍经 caller_skin 校验（未安装的
// 皮肤快速失败）。阻塞式 ureq/rustls（与更新检测同栈）放 spawn_blocking。
// HTTP 错误状态（4xx/5xx）不 reject——状态码与响应体照返（皮肤常需要
// 错误页内容）；网络层失败才 reject。响应体截断 4MB。binary: true 时请求体
// 按 base64 解码发送、响应体按 base64 返回（图片/字体等二进制内容不被
// UTF-8 替换字符毁损），响应头一并回传（Content-Type 等）。

/// SSRF 防线：localhost 主机名与环回/链路本地/私有/未指定/广播 IP 字面量
/// 一律拒绝（http_request 免权限，但这块内容页面 fetch 因 CORS 够不到，
/// 构成实质能力增量）。
fn is_private_host(url: &str) -> bool {
    let Ok(u) = url.parse::<tauri::Url>() else {
        return true; // 解析失败按拒绝处理
    };
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    if host.is_empty() || host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return true;
    }
    // IPv6 字面量在 URL 里带方括号（http://[::1]/x → host_str 可能带 []）——剥掉再解析
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    match bare.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(v4)) => {
            v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified() || v4.is_broadcast()
        }
        Ok(std::net::IpAddr::V6(v6)) => v6.is_loopback() || v6.is_unspecified(),
        Err(_) => false, // 域名放行（DNS 解析到内网的残余面不在本层）
    }
}

/// http_request 的返回结构
#[derive(Debug, Clone, Serialize)]
pub struct HttpResponseInfo {
    pub status: u16,
    /// 文本响应体（binary: true 时为 base64）
    pub body: String,
    /// 响应头（同名多头只保留首个值）
    pub headers: serde_json::Map<String, serde_json::Value>,
    /// 响应体是否因超 4MB 被截断
    pub truncated: bool,
}

#[tauri::command]
pub async fn http_request(
    app: AppHandle,
    window: tauri::WebviewWindow,
    url: String,
    method: Option<String>,
    headers: Option<serde_json::Map<String, serde_json::Value>>,
    body: Option<String>,
    timeout_ms: Option<u64>,
    binary: Option<bool>,
) -> Result<HttpResponseInfo, String> {
    let state = app.state::<AppState>();
    caller_skin(&state, &window)?;

    let url = url.trim().to_string();
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err(format!("only http(s) URLs are allowed: {}", url));
    }
    // SSRF 防线：免权限不等于能读内网——localhost/环回/链路本地/私有网段的
    // 响应体一律不给出（页面 fetch 受 CORS 本就够不到这些内容，「免权限
    // 因为 fetch 已有通道」的论证不覆盖这一增量）。
    if is_private_host(&url) {
        return Err(format!("requests to local/private addresses are not allowed: {}", url));
    }
    let method = method.unwrap_or_else(|| "GET".into()).trim().to_ascii_uppercase();
    if !["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"].contains(&method.as_str()) {
        return Err(format!("unsupported method: {}", method));
    }
    let timeout = timeout_ms.unwrap_or(15000).clamp(1000, 60000);
    let headers: Vec<(String, String)> = headers
        .unwrap_or_default()
        .into_iter()
        .map(|(k, v)| (k, v.as_str().map(String::from).unwrap_or_else(|| v.to_string())))
        .collect();
    let binary = binary.unwrap_or(false);
    // binary: 请求体是 base64；文本：原样 UTF-8 发送
    let body_bytes = match (body, binary) {
        (Some(b), true) => {
            use base64::Engine;
            Some(
                base64::engine::general_purpose::STANDARD
                    .decode(b)
                    .map_err(|e| format!("request body is not valid base64: {}", e))?,
            )
        }
        (Some(b), false) => Some(b.into_bytes()),
        (None, _) => None,
    };

    tauri::async_runtime::spawn_blocking(move || {
        // 重定向上限 3（默认 10——跟穿到内网的跳板面收窄；SSRF 主机段在
        // 发起前已拦，重定向目标段由 ureq 逐跳同规则校验不到的残面接受此
        // 代价上限）
        let agent = ureq::AgentBuilder::new().redirects(3).build();
        let mut req = agent.request(&method, &url)
            .set("User-Agent", concat!("Driftlet/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_millis(timeout));
        for (k, v) in &headers {
            req = req.set(k, v);
        }
        let result = match &body_bytes {
            Some(b) => req.send_bytes(b),
            None => req.call(),
        };
        // HTTP 错误状态照返（ureq 把 4xx/5xx 归入 Err::Status，拆出来当正常响应）
        let resp = match result {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => return Err(e.to_string()),
        };
        let status = resp.status();
        // 响应头收集（同名多头只取首个值——Set-Cookie 这类场景皮肤自行斟酌）
        let mut resp_headers = serde_json::Map::new();
        for name in resp.headers_names() {
            if let Some(v) = resp.header(&name) {
                resp_headers.insert(name, serde_json::Value::String(v.to_string()));
            }
        }
        const CAP: u64 = 4 * 1024 * 1024;
        let mut buf = Vec::new();
        use std::io::Read;
        resp.into_reader()
            .take(CAP + 1)
            .read_to_end(&mut buf)
            .map_err(|e| e.to_string())?;
        let truncated = buf.len() as u64 > CAP;
        if truncated {
            buf.truncate(CAP as usize);
        }
        let body = if binary {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(&buf)
        } else {
            // 文本通道：非法 UTF-8 以替换字符兜底（二进制请用 binary: true）
            String::from_utf8_lossy(&buf).into_owned()
        };
        Ok(HttpResponseInfo {
            status,
            body,
            headers: resp_headers,
            truncated,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

// ─── 皮肤间事件总线（免权限）───
//
// 向所有已加载皮肤窗口（含自己）广播一条自定义 DOM 事件
// `desk-skin-message`：detail = { channel, from, payload }。只投递事件、
// 皮肤自行选择监听，不读写任何宿主状态，故免权限（与 skin_log 同属本机
// 内存面）。channel 1–64 字符；payload 序列化后上限 16KB 防滥用。

#[tauri::command]
pub fn skin_broadcast(
    app: AppHandle,
    window: tauri::WebviewWindow,
    channel: String,
    payload: serde_json::Value,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let caller_id = caller_skin(&state, &window)?.id;
    let channel = channel.trim();
    if channel.is_empty() || channel.len() > 64 {
        return Err("channel must be 1–64 characters".into());
    }
    let payload_json = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
    if payload_json.len() > 16 * 1024 {
        return Err("payload too large (max 16KB)".into());
    }
    let detail = serde_json::json!({
        "channel": channel,
        "from": caller_id,
        "payload": payload,
    });
    let script = format!(
        r#"document.dispatchEvent(new CustomEvent('desk-skin-message',{{detail:{}}}));"#,
        serde_json::to_string(&detail).map_err(|e| e.to_string())?
    );
    for id in state.registry.loaded_ids() {
        if let Some(win) = state.registry.get(&id) {
            let _ = win.eval(&script);
        }
    }
    Ok(())
}

// ─── 皮肤窗口显隐（自己免权限 / 他人 control 中危）───
//
// 隐藏/显示任意已加载皮肤的窗口。省略 skinId（或传自己 id）= 作用于自己
// ——免权限（仅自身可见性，无害路径，通知式皮肤的「看完即消失」）；
// 指定其他皮肤 = `control` 中危门（显隐他者窗口与窗口配置/生命周期同层）。
// 显隐变化同步走 hotkey::sync_tray_toggle_item 漏斗（托盘勾选与管理器
// 「已隐藏」徽标随之刷新）；用户侧唤回通道（全局热键/托盘勾选）始终兜底。

#[tauri::command]
pub fn skin_hide(
    app: AppHandle,
    window: tauri::WebviewWindow,
    skin_id: Option<String>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let target = resolve_control_target(&state, &window, skin_id)?;
    let win = state
        .registry
        .get(&target)
        .ok_or_else(|| tr(&state.lang(), Key::SkinNotLoaded).to_string())?;
    win.hide().map_err(|e| e.to_string())?;
    crate::hotkey::sync_tray_toggle_item(&app);
    Ok(())
}

/// skin_show：skin_hide 的配对。只显示**不抢焦点**（再现不应打断用户
/// 当前操作）。
#[tauri::command]
pub fn skin_show(
    app: AppHandle,
    window: tauri::WebviewWindow,
    skin_id: Option<String>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let target = resolve_control_target(&state, &window, skin_id)?;
    let win = state
        .registry
        .get(&target)
        .ok_or_else(|| tr(&state.lang(), Key::SkinNotLoaded).to_string())?;
    win.show().map_err(|e| e.to_string())?;
    crate::hotkey::sync_tray_toggle_item(&app);
    Ok(())
}

/// control 组命令的统一目标解析：省略/空串/传自己 id = 自己（免权限）；
/// 指定他人 = control 中危门。皮肤经桥拿不到自己的 id，「省略即自己」
/// 让自操作免于硬编码 id。返回目标皮肤 id。
fn resolve_control_target(
    state: &AppState,
    window: &tauri::WebviewWindow,
    skin_id: Option<String>,
) -> Result<String, String> {
    let caller = caller_skin(state, window)?;
    let target = skin_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| caller.id.clone());
    if target != caller.id {
        require_perm(state, window, PERM_CONTROL)?;
    }
    Ok(target)
}

/// 皮肤窗口的当前实际几何（逻辑像素）：已加载时读窗口现场；未加载回退
/// 持久化配置（size/position 的 impl 本就要求已加载，回退分支是兜底）。
/// 供单边 patch 合并用——缺边补另一边，不得偏离现场。
fn current_geometry(state: &AppState, skin_id: &str) -> ((i32, i32), (u32, u32)) {
    if let Some(w) = state.registry.get(skin_id) {
        let sf = w.scale_factor().unwrap_or(1.0);
        let pos = w
            .outer_position()
            .map(|p| (((p.x as f64) / sf).round() as i32, ((p.y as f64) / sf).round() as i32))
            .unwrap_or((0, 0));
        let size = w
            .outer_size()
            .map(|s| (((s.width as f64) / sf).round() as u32, ((s.height as f64) / sf).round() as u32))
            .unwrap_or((300, 200));
        return (pos, size);
    }
    let app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
    let entry = app_config.skin_settings.get(skin_id);
    (
        (
            entry.and_then(|e| e.x).unwrap_or(0),
            entry.and_then(|e| e.y).unwrap_or(0),
        ),
        (
            entry.map(|e| e.width).unwrap_or(300),
            entry.map(|e| e.height).unwrap_or(200),
        ),
    )
}

// ─── Hardware probe (manual) ───

#[cfg(test)]
mod tests {
    #[test]
    fn private_hosts_blocked() {
        assert!(super::is_private_host("http://127.0.0.1/x"));
        assert!(super::is_private_host("http://localhost/x"));
        assert!(super::is_private_host("http://[::1]/x"));
        assert!(super::is_private_host("http://10.1.2.3/x"));
        assert!(super::is_private_host("http://172.16.0.1/x"));
        assert!(super::is_private_host("http://192.168.1.1/x"));
        assert!(super::is_private_host("http://169.254.169.254/latest/meta-data"));
        assert!(super::is_private_host("http://0.0.0.0/x"));
        assert!(!super::is_private_host("https://example.com/x"));
        assert!(!super::is_private_host("https://8.8.8.8/x"));
    }

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
        // 尾点/尾空格绕过（Windows 规范化剥尾部后真执行）
        assert!(!super::is_open_target_allowed("C:\\tools\\RUN.EXE."));
        assert!(!super::is_open_target_allowed("C:\\tools\\RUN.EXE "));
        // 补入的可解析为代码的类型
        assert!(!super::is_open_target_allowed("C:\\tools\\help.chm"));
        assert!(!super::is_open_target_allowed("C:\\tools\\x.SettingContent-ms"));
        assert!(!super::is_open_target_allowed("C:\\tools\\x.scf"));
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
