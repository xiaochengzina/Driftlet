//! Skin-facing backend APIs: the permission-gated capabilities (twelve
//! permissions in three display tiers — high / medium / low — tier is a
//! wizard display concern only; enforcement is binary declared-or-not),
//! plus the permission-free baseline:
//!
//!   * `shell`（高危） — run_command
//!   * `file_system`（高危） — skin_read_any_file / skin_write_any_file /
//!                   skin_list_any_dir / skin_create_any_dir /
//!                   skin_delete_any_path（任意绝对路径；应用数据根禁变更）
//!   * `system`（高危） — open_external（URI 目标；http(s) 另有 open_link
//!                   低危通道；本地路径面已裁撤）/ lock_workstation /
//!                   monitor_off / sleep / power_control / empty_recycle_bin
//!   * `registry`（中危） — read_registry_value
//!   * `clipboard`（中危） — read_clipboard_text / write_clipboard_text
//!   * `mic`（中危） — get_mic_spectrum
//!   * `control`（中危） — skin_list_skins / skin_get_window_config /
//!                   skin_set_window_config / skin_load / skin_unload /
//!                   skin_reload / skin_hide / skin_show（作用于自己免权限）
//!   * `media`（低危） — 读取（get_volume / get_media_info /
//!                   get_audio_spectrum 环回）+ 控制（set_volume / set_mute /
//!                   media_control / media_seek）同一族播控面同权
//!   * `notify`（低危） — show_notification（从 system 拆出单列）
//!   * `sys_info`（低危） — 只读系统信息 13 条：get_cpu_info / get_gpu_info /
//!                   get_memory_info / get_disks_info / get_disk_space /
//!                   get_network_info / get_os_info / get_battery_info /
//!                   get_monitors / get_system_theme / get_processes /
//!                   get_idle_time / get_foreground_window_info
//!   * `network`（低危） — http_request（曾有此名未发布即取消；复活为低危
//!                   新语义——闸的是绕 CORS 读响应，页面 fetch 仍在闸外）
//!   * `open_link`（低危） — open_external 的 http(s) 目标（从 system 分层
//!                   的低危通道；mailto/ms-settings/本地路径仍需 system）
//!
//! 免权限基线：皮肤自身目录内的文件读写（fs.rs 沙箱即边界）、自己 schema
//! 的设置读写、skin_log / skin_console_log / skin_broadcast、control 族
//! 「作用于自己」臂。曾有过的 `files` 权限已整体取消，该名字永不复活。
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
mod power;
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
    /// 源播放器是否支持拖动进度条寻址（TryChangePlaybackPositionAsync）。
    /// 媒体中心式播控常关寻址——为 false 时皮肤应把进度条锁成只读。
    pub seekable: bool,
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
/// State-changing system controls: open_external（URI 目标——http(s) 另
/// 有 open_link 低危通道；本地路径面已裁撤）、lock_workstation /
/// monitor_off / sleep / power_control / empty_recycle_bin。
///（音量与媒体控制挪进了 media、系统通知挪进了 notify——前者是同一族
/// 「当前在放什么」的播控面，后者是低危的可见打扰，均与电源动作不同级。
/// 注意：system 刻意没有「启动 exe」的通道——open_external 只收 URI
/// 白名单目标，其余五条命令无路径/无目标参数；运行程序只属于 shell
/// 权限的 run_command。）
pub const PERM_SYSTEM: &str = "system";
/// Media + volume: reads (get_volume / get_media_info / get_audio_spectrum
/// 环回) + control (set_volume / set_mute / media_control / media_seek)——
/// 同一族「当前在放什么」的播控面，读取与控制同权（均低危）；
/// media_info 曾单列读取、已并入本权限（名字退役，旧声明按未知名忽略）。
/// 从 system 拆出单列：与开外链/电源动作不同级，纯打扰无数据面。
pub const PERM_MEDIA: &str = "media";
/// Toast notifications (show_notification)——从 system 拆出单列：可见打扰、
/// 无数据面，低危
pub const PERM_NOTIFY: &str = "notify";
/// Read-only system & hardware info (13 probes: cpu / gpu / memory / disks /
/// disk_space / network / os / battery / monitors / system_theme /
/// processes / idle_time / foreground_window)——只读但含活动监视面（前台
/// 窗口标题/进程/空闲），单列低危让向导可见。
pub const PERM_SYS_INFO: &str = "sys_info";
/// http_request——绕 CORS 读任意公开 URL 响应 + 任意方法/头。复活历史
/// `network` 名为低危新语义：页面 fetch 的受限通道仍在闸外（无法闸），
/// 向导可见性价值 = 用户知道皮肤会联网。
pub const PERM_NETWORK: &str = "network";
/// open_external 的 http(s) 目标（用默认浏览器打开网页链接）——从
/// system 分层出的低危通道：mailto/ms-settings/本地路径目标仍需 system
/// （文件关联面留在高危）；system 自身能力不受影响（全目标仍可用）。
pub const PERM_OPEN_LINK: &str = "open_link";
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

/// 声明了 perms 中任一权限即放行（分层闸门：open_external 的 http(s) 目标
/// 过 open_link 低危或 system 高危）。拒绝时报错按首个权限名给出——分层
/// 场景低危在前，提示用户该目标其实只需声明低危。
fn require_any_perm(
    state: &AppState,
    window: &tauri::WebviewWindow,
    perms: &[&str],
) -> Result<(String, std::path::PathBuf), String> {
    let lang = state.lang();
    let skin = caller_skin(state, window)?;
    if perms
        .iter()
        .any(|perm| skin.manifest.permissions.iter().any(|p| p == perm))
    {
        return Ok((skin.id, skin.directory));
    }
    Err(trf(&lang, Key::PermissionDenied, &[skin.id.as_str(), perms[0]]))
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
pub async fn get_cpu_info(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Vec<CpuInfo>, String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_SYS_INFO)?;
    let perf_pct = cpu_performance_pct();
    let mut guard = CPU_SYS.lock().unwrap_or_else(|e| e.into_inner());
    let sys = guard.get_or_insert_with(new_light_system);
    sys.refresh_cpu_all();
    let cpus = sys.cpus();
    let nominal_mhz = cpus.iter().map(|c| c.frequency()).max().unwrap_or(0);
    Ok(vec![CpuInfo {
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
    }])
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
pub fn get_memory_info(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<MemoryInfo, String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_SYS_INFO)?;
    let mut guard = CPU_SYS.lock().unwrap_or_else(|e| e.into_inner());
    let sys = guard.get_or_insert_with(new_light_system);
    sys.refresh_memory();
    Ok(MemoryInfo {
        ram: MemoryGroup::new(sys.total_memory(), sys.used_memory()),
        swap: MemoryGroup::new(sys.total_swap(), sys.used_swap()),
        commit: commit_group(),
    })
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
pub async fn get_disks_info(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Vec<DiskInfo>, String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_SYS_INFO)?;
    let rates = sample_disk_rates();

    let mut guard = DISK_SAMPLER.lock().unwrap_or_else(|e| e.into_inner());
    let sampler = guard.get_or_insert_with(|| DiskSampler {
        disks: sysinfo::Disks::new_with_refreshed_list(),
    });
    sampler.disks.refresh();

    Ok(sampler
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
        .collect())
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
pub fn get_disk_space(
    app: AppHandle,
    window: tauri::WebviewWindow,
    path: String,
) -> Result<DiskSpace, String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_SYS_INFO)?;
    let lang = state.lang();
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
pub fn get_network_info(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<NetworkInfo, String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_SYS_INFO)?;
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
    Ok(NetworkInfo { adapters, local_ips })
}

// ─── GPU (Windows) ───

/// async：DXGI 枚举 + PDH 采样 + 建 D3D12 设备（UMA 判定）都是重负载，
/// 不跑主线程——同 get_cpu_info / get_disks_info 的写法，逻辑不变。
/// 同步重活挪 spawn_blocking（DXGI/D3D12 全同步阻塞 async worker，
/// 高频轮询会停满 worker 池）。
#[tauri::command]
pub async fn get_gpu_info(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Vec<GpuInfo>, String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_SYS_INFO)?;
    #[cfg(target_os = "windows")]
    {
        tauri::async_runtime::spawn_blocking(gpu::collect)
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(windows_only(&app))
    }
}

// ─── Audio spectrum (Windows; 环回读取归 media 低危，麦克风归 mic 中危) ───

#[tauri::command]
pub fn get_audio_spectrum(
    app: AppHandle,
    window: tauri::WebviewWindow,
    bands: Option<usize>,
) -> Result<Spectrum, String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_MEDIA)?;
    #[cfg(target_os = "windows")]
    {
        let lang = state.lang();
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

// ─── Status probes: battery / idle / foreground / monitors（sys_info 低危）───

#[tauri::command]
pub fn get_battery_info(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<BatteryInfo, String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_SYS_INFO)?;
    #[cfg(target_os = "windows")]
    {
        status::battery()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(windows_only(&app))
    }
}

/// Milliseconds since the last keyboard/mouse input.
#[tauri::command]
pub fn get_idle_time(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<u64, String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_SYS_INFO)?;
    #[cfg(target_os = "windows")]
    {
        status::idle_ms()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(windows_only(&app))
    }
}

/// Currently focused window; null in the rare case there is none.
#[tauri::command]
pub fn get_foreground_window_info(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Option<ForegroundWindowInfo>, String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_SYS_INFO)?;
    #[cfg(target_os = "windows")]
    {
        Ok(status::foreground_window())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(windows_only(&app))
    }
}

#[tauri::command]
pub fn get_monitors(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Vec<MonitorInfo>, String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_SYS_INFO)?;
    #[cfg(target_os = "windows")]
    {
        Ok(status::monitors())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(windows_only(&app))
    }
}

/// 检测 Windows 系统级浅色/深色主题（HKCU\…\Themes\Personalize 的
/// AppsUseLightTheme：1 浅 0 深），返回 "light" / "dark"。只读系统信息，
/// 归 sys_info 低危（与同组只读探针同闸）。系统主题变化不做
/// 推送——皮肤在需要时调用，或配合定时轮询/窗口可见事件刷新。
#[tauri::command]
pub fn get_system_theme(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<String, String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_SYS_INFO)?;
    #[cfg(target_os = "windows")]
    {
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


// ─── OS / processes（sys_info 低危，只读）───

#[tauri::command]
pub fn get_os_info(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<system::OsInfo, String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_SYS_INFO)?;
    Ok(system::os_info())
}

#[tauri::command]
pub async fn get_processes(
    app: AppHandle,
    window: tauri::WebviewWindow,
    sort: Option<String>,
    limit: Option<usize>,
) -> Result<system::ProcessList, String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_SYS_INFO)?;
    let mut guard = CPU_SYS.lock().unwrap_or_else(|e| e.into_inner());
    let sys = guard.get_or_insert_with(new_light_system);
    Ok(system::processes(
        sys,
        sort.as_deref().unwrap_or("cpu"),
        limit.unwrap_or(10),
    ))
}

// ─── Volume (Windows; get 读取与 set/mute 控制同属 media 低危) ───

#[tauri::command]
pub fn get_volume(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<VolumeInfo, String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_MEDIA)?;
    #[cfg(target_os = "windows")]
    {
        let lang = state.lang();
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
    require_perm(&state, &window, PERM_MEDIA)?;
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
    require_perm(&state, &window, PERM_MEDIA)?;
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
pub async fn get_media_info(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Option<MediaInfo>, String> {
    let state = app.state::<AppState>();
    require_perm(&state, &window, PERM_MEDIA)?;
    #[cfg(target_os = "windows")]
    {
        let lang = state.lang();
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
    require_perm(&state, &window, PERM_MEDIA)?;

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

/// 拖动进度条寻址（绝对秒数）。源不支持寻址时返回 false（不是错误）。
#[tauri::command]
pub async fn media_seek(app: AppHandle, window: tauri::WebviewWindow, position_secs: f64) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    require_perm(&state, &window, PERM_MEDIA)?;

    #[cfg(target_os = "windows")]
    {
        let lang_inner = lang.clone();
        tauri::async_runtime::spawn_blocking(move || media::seek(position_secs))
            .await
            .map_err(|e| trf(&lang, Key::TaskFailed, &[&e.to_string()]))?
            .map_err(|e| trf(&lang_inner, Key::MediaControlFailed, &[&e]))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = position_secs;
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

// ─── Open external link (permission: open_link 低危 / system 高危分层) ───

/// URI 白名单：http(s) / mailto / ms-settings（系统设置页 URI，由设置应用
/// 处理，无代码执行面）。**本地路径面已整体裁撤**——ShellExecute 本地文件
/// 靠「可执行扩展名黑名单」设防是负枚举（35 项清单追不上新执行面，且曾
/// 被尾点/尾空格绕过），负枚举换不来安全；确有打开本地文件需要的皮肤走
/// shell 权限的 run_command（高危、安装页明示）。目标是否存在不做提前
/// 探测：不存在与打开失败统一报 OpenFailed，消除路径存在性探针。
fn is_open_target_allowed(target: &str) -> bool {
    let lower = target.trim().to_ascii_lowercase();
    lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("mailto:")
        || lower.starts_with("ms-settings:")
}

/// open_external 的分层闸门（纯函数，测试钉住）：http(s) 目标过
/// open_link（低危）或 system（高危）任一；其余目标（mailto/ms-settings
/// 等——本地路径连 is_open_target_allowed 的白名单都进不了）只过 system。
fn open_external_required_perms(target: &str) -> &'static [&'static str] {
    let lower = target.trim().to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        &[PERM_OPEN_LINK, PERM_SYSTEM]
    } else {
        &[PERM_SYSTEM]
    }
}

#[tauri::command]
pub fn open_external(app: AppHandle, window: tauri::WebviewWindow, target: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    let target = target.trim();
    require_any_perm(&state, &window, open_external_required_perms(target))?;
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

// ─── Toast notification (permission: notify —— 低危，Windows) ───

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
    require_perm(&state, &window, PERM_NOTIFY)?;
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

// ─── 电源与回收站（permission: system，Windows）───
//
// 五条命令全部无路径/无目标参数——system 权限刻意没有「启动 exe」的
// 通道（与 open_external 的可执行黑名单同一条防线）；运行程序只属于
// shell 权限的 run_command。Win32 调用统一走 spawn_blocking（与
// media_control 同规则：不占主线程/async worker）。

/// 锁定当前会话（等同 Win+L）。
#[tauri::command]
pub async fn lock_workstation(app: AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    require_perm(&state, &window, PERM_SYSTEM)?;
    #[cfg(target_os = "windows")]
    {
        let lang_inner = lang.clone();
        tauri::async_runtime::spawn_blocking(power::lock)
            .await
            .map_err(|e| trf(&lang, Key::TaskFailed, &[&e.to_string()]))?
            .map_err(|e| trf(&lang_inner, Key::PowerControlFailed, &[&e]))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(windows_only(&app))
    }
}

/// 熄灭显示器（任意输入即唤醒，不是睡眠）。
#[tauri::command]
pub async fn monitor_off(app: AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    require_perm(&state, &window, PERM_SYSTEM)?;
    #[cfg(target_os = "windows")]
    {
        let lang_inner = lang.clone();
        tauri::async_runtime::spawn_blocking(power::monitor_off)
            .await
            .map_err(|e| trf(&lang, Key::TaskFailed, &[&e.to_string()]))?
            .map_err(|e| trf(&lang_inner, Key::PowerControlFailed, &[&e]))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(windows_only(&app))
    }
}

/// 进入睡眠（不强制、不休眠；系统策略禁用睡眠时报错）。
#[tauri::command]
pub async fn sleep(app: AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    require_perm(&state, &window, PERM_SYSTEM)?;
    #[cfg(target_os = "windows")]
    {
        let lang_inner = lang.clone();
        tauri::async_runtime::spawn_blocking(power::sleep)
            .await
            .map_err(|e| trf(&lang, Key::TaskFailed, &[&e.to_string()]))?
            .map_err(|e| trf(&lang_inner, Key::PowerControlFailed, &[&e]))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(windows_only(&app))
    }
}

/// 关机 / 重启 / 注销（不带 force——有未保存数据的应用可以阻止，用户会
/// 看到系统级阻止界面，皮肤不能绕过它静默丢数据）。
#[tauri::command]
pub async fn power_control(
    app: AppHandle,
    window: tauri::WebviewWindow,
    action: String,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    require_perm(&state, &window, PERM_SYSTEM)?;
    #[cfg(target_os = "windows")]
    {
        let act = match action.as_str() {
            "shutdown" => power::PowerAction::Shutdown,
            "restart" => power::PowerAction::Restart,
            "logoff" => power::PowerAction::Logoff,
            other => return Err(trf(&lang, Key::InvalidPowerAction, &[other])),
        };
        let lang_inner = lang.clone();
        tauri::async_runtime::spawn_blocking(move || power::power(act))
            .await
            .map_err(|e| trf(&lang, Key::TaskFailed, &[&e.to_string()]))?
            .map_err(|e| trf(&lang_inner, Key::PowerControlFailed, &[&e]))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = action;
        Err(windows_only(&app))
    }
}

/// 清空回收站（带系统确认框与音效——破坏性操作的最终确认权留给用户；
/// 回收站已空时直接成功、不弹框）。
#[tauri::command]
pub async fn empty_recycle_bin(app: AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    require_perm(&state, &window, PERM_SYSTEM)?;
    #[cfg(target_os = "windows")]
    {
        let lang_inner = lang.clone();
        tauri::async_runtime::spawn_blocking(power::empty_recycle_bin)
            .await
            .map_err(|e| trf(&lang, Key::TaskFailed, &[&e.to_string()]))?
            .map_err(|e| trf(&lang_inner, Key::RecycleBinFailed, &[&e]))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(windows_only(&app))
    }
}

// ─── 任意路径文件读写（permission: file_system —— 高危）───
//
// 与 fs.rs 的沙箱命令（skin_read_file 等，免权限、限皮肤自身目录）不同：
// 这五条接受任意**绝对路径**，整盘可达。错误一律透传系统错误文案
// （皮肤需要知道真实失败原因）；相对路径拒绝（没有可参照的工作目录）。
// 写/建/删三条额外过 ensure_mutable_any_path：应用自身数据根
//（skins_dir/config_dir）禁止变更——防改写 skin.json 自我提权。

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

/// 应用数据根变更保护（file_system 权限）：写 / 建 / 删三条命令的目标
/// 不得落在禁写根内。没有这道防线时皮肤可改写自己的 skin.json 往
/// permissions 里加项——require_perm 每次调用实时重扫 manifest，新权限
/// 立即生效，等于绕过安装页承诺静默自我提权（config.json 被改、其他皮肤
/// 被删同理）。读 / 列保留：高危权限「整盘可读」是安装页声明过的语义。
///
/// 禁写根四个（审查 H2 后从两个扩到四个）：
///   - skins_dir / config_dir：防自我提权（上述原始动机）；
///   - update_dir（<数据根>/update）：宿主会**执行**其中的安装包
///    （install_update），皮肤可写 = 借「立即安装」这个可信动作把写能力
///     升级为代码执行；
///   - exe 所在目录（便携布局下是前三个根的父目录，冗余但显式）：覆盖
///     Driftlet.exe 本体 / WebView2Loader.dll / 卸载器——改写任一 = 下次
///     启动或卸载时代码执行。非便携回退布局（%APPDATA% 数据 + Program
///     Files 程序）下四个根不相交，各自独立生效。
fn ensure_mutable_any_path(
    skins_dir: &std::path::Path,
    config_dir: &std::path::Path,
    p: &std::path::Path,
) -> Result<(), String> {
    // `..` 分量会让 resolve_location 的「最深现存祖先 + 词法重拼尾段」
    // 失真（Windows 沿符号链接逐分量解析 `..`，非纯词法）——变更类目标
    // 一律拒绝，调用方传规范形式即可（与 fs.rs 沙箱拒 `..` 同口径）。
    if p
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(format!("path must not contain '..': {}", p.display()));
    }
    let resolved = resolve_location(p)
        .ok_or_else(|| format!("cannot resolve path location: {}", p.display()))?;
    let update_dir = crate::update::update_dir(config_dir);
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.to_path_buf()));
    let mut roots = vec![skins_dir, config_dir, update_dir.as_path()];
    if let Some(exe) = exe_dir.as_deref() {
        roots.push(exe);
    }
    for root in roots {
        let Some(canon_root) = resolve_location(root) else {
            continue;
        };
        if path_starts_with_ci(&resolved, &canon_root) {
            return Err(format!(
                "path is inside Driftlet's own data directory and cannot be modified: {}",
                p.display()
            ));
        }
    }
    Ok(())
}

/// 解析路径的真实落点：全路径存在则 canonicalize（解析符号链接 / 8.3
/// 短名 / 真实大小写）；目标尚不存在时 canonicalize 最深现存祖先后词法
/// 重拼不存在的尾段——尾段分量均不存在、不可能是符号链接，重拼即真实
/// 落点。调用方须已拒绝 `..` 分量。数据根本身缺失时同样适用（现存祖先
/// 一路退到盘符根），缺失根也能解析出用于比较的形式。
fn resolve_location(p: &std::path::Path) -> Option<std::path::PathBuf> {
    if let Ok(c) = p.canonicalize() {
        return Some(c);
    }
    let mut ancestor = p.parent();
    loop {
        let a = ancestor?;
        match a.canonicalize() {
            Ok(c) => return Some(c.join(p.strip_prefix(a).ok()?)),
            Err(_) => ancestor = a.parent(),
        }
    }
}

/// 分量级前缀比较，Windows 下 ASCII 忽略大小写（NTFS 大小写不敏感，与
/// fs.rs 受保护名单 eq_ignore_ascii_case 同口径）——canonicalize 已覆盖
/// 现存分量的真实大小写，这里兜底不存在的尾段（含数据根缺失时的重拼
/// 形式）。与 `starts_with` 同语义，相等也算包含。
fn path_starts_with_ci(p: &std::path::Path, prefix: &std::path::Path) -> bool {
    let mut got = p.components();
    prefix.components().all(|want| {
        got.next().is_some_and(|g| {
            if cfg!(target_os = "windows") {
                g.as_os_str()
                    .to_str()
                    .zip(want.as_os_str().to_str())
                    .is_some_and(|(g, w)| g.eq_ignore_ascii_case(w))
            } else {
                g == want
            }
        })
    })
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
    // TOCTOU 防线：metadata 检查后文件可能被换大——read_capped 按上限+1
    // 流式读、按实际字节再审，不做无界分配（与沙箱版同一函数，审查 M2）
    let bytes = match fs::read_capped(&p, fs::MAX_READ_BYTES) {
        Ok(b) => b,
        Err(fs::CapReadError::TooLarge) => {
            return Err(format!("file too large: > {} bytes (max {})", fs::MAX_READ_BYTES, fs::MAX_READ_BYTES));
        }
        Err(fs::CapReadError::Io(e)) => return Err(e),
    };
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
    ensure_mutable_any_path(&state.skins_dir, &state.config_dir, &p)?;
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
    ensure_mutable_any_path(&state.skins_dir, &state.config_dir, &p)?;
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
    ensure_mutable_any_path(&state.skins_dir, &state.config_dir, &p)?;
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

// ─── 通用 HTTP 请求（permission: network —— 低危）───
//
// 突破页面 fetch 的 CORS 限制：任意 http(s) URL、自定义头、文本/二进制负载。
// `network` 权限名属复活（曾有此名、未发布即取消——当时理由「页面本有 fetch
// 通道，设闸挡不住有心者只做展示噪音」对**外发数据**依然成立：no-cors POST
// 谁也闸不住）；复活后的新语义闸的是**能力增量**——绕 CORS 读取任意公开
// URL 的响应体 + 任意方法/请求头，这是 fetch 够不到的。向导可见性价值 =
// 用户在安装页能看到皮肤会联网。页面 fetch 的受限通道仍在闸外（无法闸）。
// 阻塞式 ureq/rustls（与更新检测同栈）放 spawn_blocking。
// HTTP 错误状态（4xx/5xx）不 reject——状态码与响应体照返（皮肤常需要
// 错误页内容）；网络层失败才 reject。响应体截断 4MB。binary: true 时请求体
// 按 base64 解码发送、响应体按 base64 返回（图片/字体等二进制内容不被
// UTF-8 替换字符毁损），响应头一并回传（Content-Type 等）。

/// SSRF 防线：localhost 主机名与环回/链路本地/私有/未指定/广播 IP 一律
/// 拒绝（network 是低危权限，但这块内容页面 fetch 因 CORS 够不到，构成
/// 实质能力增量）。覆盖形态：标准点分/IPv6 字面量 + inet_aton 数字字面量
///（2130706433 / 0x7f000001 / 127.1 / 0177.0.0.1——IpAddr::parse 不认、
/// OS 解析器会当成 IP 的形态，不拦即可借它们绕过全部段检查）。
/// 调用方：http_request 发起前 + 每一次重定向跳转后逐跳复检。
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
    let v4_blocked = |v4: std::net::Ipv4Addr| {
        v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified() || v4.is_broadcast()
            || v4.octets()[0] == 0 // 0.0.0.0/8「本网络」段（is_unspecified 只盖 0.0.0.0）
    };
    match bare.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(v4)) => v4_blocked(v4),
        Ok(std::net::IpAddr::V6(v6)) => {
            // 先查 v6 自身的 loopback/unspecified：to_ipv4 会把 v4 兼容形态
            // ::1 折成 0.0.0.1（不在 v4 各私网段内）而漏拦（测试钉住）
            if v6.is_loopback() || v6.is_unspecified() {
                return true;
            }
            // IPv4-mapped/compatible（::ffff:127.0.0.1 等）经双栈 socket 实际
            // 打到 v4 目标——折回 IPv4 走同一套段检查（复审 D-B：原臂只查
            // loopback/unspecified，http://[::ffff:127.0.0.1]/ 穿透防线）
            if let Some(v4) = v6.to_ipv4() {
                return v4_blocked(v4);
            }
            false
        }
        Err(_) => match parse_inet_aton(bare) {
            Some(v4) => v4_blocked(v4),
            None => false, // 域名放行（DNS 解析到内网的残余面不在本层）
        },
    }
}

/// inet_aton 语义还原数字 IP 字面量：纯十进制（2130706433）、十六进制
///（0x7f000001）、短点分（127.1——前 n-1 段各占 8 位、尾段占剩余位）、
/// 八进制（0177.0.0.1，前导 0）都是 OS 解析器（getaddrinfo/inet_aton）
/// 接受而 IpAddr::parse 拒绝的形态。返回还原出的 IPv4 地址；非数字字面量
/// 形态（普通域名）返回 None。段值溢出（如 999.1.1.1）返回 None——OS
/// 会把这种串当域名解析并失败，无 SSRF 面。
fn parse_inet_aton(host: &str) -> Option<std::net::Ipv4Addr> {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    let mut nums: Vec<u64> = Vec::with_capacity(parts.len());
    for p in parts {
        let (digits, radix) = if let Some(hex) = p.strip_prefix("0x").or_else(|| p.strip_prefix("0X")) {
            (hex, 16)
        } else if p.len() > 1 && p.starts_with('0') {
            (&p[1..], 8)
        } else {
            (p, 10)
        };
        if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
            return None;
        }
        nums.push(u64::from_str_radix(digits, radix).ok()?);
    }
    // 位宽规则：n 段时前 n-1 段各占 8 位，尾段占剩余位（1 段 = 32 位整数值）
    let last_bits = (5 - nums.len() as u32) * 8;
    let mut value: u64 = 0;
    for (i, &n) in nums.iter().enumerate() {
        if i + 1 == nums.len() {
            if n >= (1u64 << last_bits) {
                return None;
            }
            value = (value << last_bits) | n;
        } else {
            if n > 0xFF {
                return None;
            }
            value = (value << 8) | n;
        }
    }
    Some(std::net::Ipv4Addr::from(value as u32))
}

/// 逐跳解析重定向目标并复检（纯函数，测试钉住）：相对 Location 按当前
/// URL join；scheme 限 http/https（跳到 file: 等一律拒绝）；目标主机过
/// is_private_host（含 inet_aton 字面量形态）——SSRF 防线对全链每一跳
/// 生效，不让 302 成为进内网的跳板（审查 H1）。
fn resolve_redirect_url(current: &str, location: &str) -> Result<String, String> {
    let base = current
        .parse::<tauri::Url>()
        .map_err(|e| format!("cannot parse current url: {}", e))?;
    let next = base
        .join(location.trim())
        .map_err(|e| format!("bad redirect location {:?}: {}", location, e))?;
    let next_str = next.to_string();
    let lower = next_str.to_ascii_lowercase();
    if !lower.starts_with("https://") && !lower.starts_with("http://") {
        return Err(format!("redirect to non-http(s) URL is not allowed: {}", next_str));
    }
    if is_private_host(&next_str) {
        return Err(format!("redirect to local/private address is not allowed: {}", next_str));
    }
    Ok(next_str)
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

/// 重定向跳转时的请求头处理（纯函数，测试钉住）：
/// - 跨源跳（scheme/host/port 任一变化）：剥 authorization / cookie /
///   proxy-authorization——皮肤给 api.a.com 配的令牌不得因 302 泄漏给
///   第三方主机（ureq 自动跟随时代的内建行为——其 RedirectAuthHeaders
///   默认 Never + 恒剥 cookie/content-length；手动跟跳后须自行维持，
///   否则相对改动前是安全回退，复审 D-A）；
/// - 转 GET 丢体（301/302/303）：另剥 content-length / content-type——
///   体没了头还在会让服务器等 body 挂起。
fn headers_for_hop(
    headers: &[(String, String)],
    cross_origin: bool,
    body_dropped: bool,
) -> Vec<(String, String)> {
    headers
        .iter()
        .filter(|(k, _)| {
            let k = k.to_ascii_lowercase();
            if body_dropped && (k == "content-length" || k == "content-type") {
                return false;
            }
            if cross_origin
                && matches!(k.as_str(), "authorization" | "cookie" | "proxy-authorization")
            {
                return false;
            }
            true
        })
        .cloned()
        .collect()
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
    require_perm(&state, &window, PERM_NETWORK)?;

    let url = url.trim().to_string();
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err(format!("only http(s) URLs are allowed: {}", url));
    }
    // SSRF 防线：低危不等于能读内网——localhost/环回/链路本地/私有网段的
    // 响应体一律不给出（页面 fetch 受 CORS 本就够不到这些内容，「低危」
    // 不覆盖这一增量）。
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
        // SSRF 防线覆盖重定向全链：ureq 自动跟随的跳数不经过 is_private_host
        //（公网站点 302 → 169.254.169.254 云元数据 / 127.0.0.1 本机服务即可
        // 整体绕过——审查 H1），故 redirects(0) 关掉自动跟随，手动逐跳解析
        // Location 并复检 scheme + 私网判定，上限 3 跳。方法传递按浏览器
        // 语义：303（及 301/302 的非 GET/HEAD）转 GET 丢体；307/308 原样重发。
        // 请求头逐跳经 headers_for_hop 过滤：跨源剥授权类头、丢体剥体头
        //（复审 D-A——自动跟随时代 ureq 内建此行为，手动跟跳须自行维持）。
        let agent = ureq::AgentBuilder::new().redirects(0).build();
        let mut current_url = url;
        let mut current_method = method;
        let mut current_body = body_bytes;
        let mut hop_headers = headers;
        let mut hops = 0u32;
        let resp = loop {
            let mut req = agent.request(&current_method, &current_url)
                .set("User-Agent", concat!("Driftlet/", env!("CARGO_PKG_VERSION")))
                .timeout(std::time::Duration::from_millis(timeout));
            for (k, v) in &hop_headers {
                req = req.set(k, v);
            }
            let result = match &current_body {
                Some(b) => req.send_bytes(b),
                None => req.call(),
            };
            // HTTP 错误状态照返（ureq 把 4xx/5xx 归入 Err::Status，拆出来当
            // 正常响应）；redirects(0) 下 3xx 同样是普通响应，走重定向分支
            let resp = match result {
                Ok(r) => r,
                Err(ureq::Error::Status(_, r)) => r,
                Err(e) => return Err(e.to_string()),
            };
            let location = match resp.status() {
                301 | 302 | 303 | 307 | 308 => resp.header("Location").map(String::from),
                _ => None,
            };
            let Some(location) = location else { break resp };
            if hops >= 3 {
                return Err(format!("too many redirects (max 3): {}", current_url));
            }
            hops += 1;
            let next = resolve_redirect_url(&current_url, &location)?;
            let cross_origin = tauri::Url::parse(&current_url).map(|u| u.origin())
                != tauri::Url::parse(&next).map(|u| u.origin());
            let body_dropped = resp.status() == 303
                || ((resp.status() == 301 || resp.status() == 302)
                    && current_method != "GET"
                    && current_method != "HEAD");
            if body_dropped {
                current_method = "GET".to_string();
                current_body = None;
            }
            hop_headers = headers_for_hop(&hop_headers, cross_origin, body_dropped);
            current_url = next;
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
//
// （曾有 probe_hardware_info 直调 get_cpu_info 等信息命令——这些命令已纳入
// sys_info/media 权限门、需要活的皮肤窗口，探针随之移除；硬件信息
// 目检改用 sys-monitor 示例皮肤。以下两个 PDH 探针直调内部函数不受影响。）

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
        // IPv4-mapped IPv6（复审 D-B）：双栈下实际打到 v4 目标，必须同规则
        assert!(super::is_private_host("http://[::ffff:127.0.0.1]/x"));
        assert!(super::is_private_host("http://[::ffff:169.254.169.254]/x"));
        assert!(!super::is_private_host("http://[::ffff:8.8.8.8]/x"));
        // inet_aton 数字字面量形态：IpAddr::parse 不认但 OS 会当成 IP——
        // 还原后按同一套段检查（审查 H1 配套加固）
        assert!(super::is_private_host("http://2130706433/x")); // 127.0.0.1 纯十进制
        assert!(super::is_private_host("http://0x7f000001/x")); // 127.0.0.1 十六进制
        assert!(super::is_private_host("http://127.1/x")); // 127.0.0.1 短点分
        assert!(super::is_private_host("http://0177.0.0.1/x")); // 八进制段
        assert!(super::is_private_host("http://2852039166/x")); // 169.254.169.254
        assert!(!super::is_private_host("http://134744072/x")); // 8.8.8.8 公网放行
        // 溢出段字面量（999.1.1.1）连 WHATWG URL 解析都过不了（特殊 scheme
        // 对类 IPv4 主机名严格）→ 走「解析失败按拒绝」；域名形态不误伤
        assert!(super::is_private_host("http://999.1.1.1/x"));
        assert!(!super::is_private_host("http://example.123.com/x"));
    }

    /// 重定向请求头过滤（复审 D-A）：跨源剥授权类头、丢体剥体头
    #[test]
    fn redirect_headers_stripped() {
        let h = || -> Vec<(String, String)> {
            vec![
                ("Authorization".into(), "Bearer t".into()),
                ("Cookie".into(), "s=1".into()),
                ("Content-Length".into(), "5".into()),
                ("Content-Type".into(), "text/plain".into()),
                ("X-Custom".into(), "keep".into()),
            ]
        };
        // 同源同跳：全保留
        assert_eq!(super::headers_for_hop(&h(), false, false).len(), 5);
        // 跨源：授权类剥掉，自定义保留
        let out = super::headers_for_hop(&h(), true, false);
        assert!(!out.iter().any(|(k, _)| k.eq_ignore_ascii_case("authorization")));
        assert!(!out.iter().any(|(k, _)| k.eq_ignore_ascii_case("cookie")));
        assert!(out.iter().any(|(k, _)| k == "X-Custom"));
        // 丢体（含同源）：体头剥掉
        let out = super::headers_for_hop(&h(), false, true);
        assert!(!out.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-length")));
        assert!(!out.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")));
        assert!(out.iter().any(|(k, _)| k.eq_ignore_ascii_case("authorization")));
        // 跨源 + 丢体：两类都剥
        let out = super::headers_for_hop(&h(), true, true);
        assert_eq!(out.len(), 1);
    }

    #[test]
    /// 重定向目标逐跳复检（审查 H1：302 跳板曾是 SSRF 防线的整体绕过）
    fn redirect_targets_rechecked() {
        // 相对路径 join 到当前 URL
        assert_eq!(
            super::resolve_redirect_url("https://example.com/a/b", "c").unwrap(),
            "https://example.com/a/c"
        );
        assert!(super::resolve_redirect_url("https://example.com/", "//cdn.example.com/x").is_ok());
        assert!(super::resolve_redirect_url("https://example.com/", "https://cdn.example.com/x").is_ok());
        // 跳本机/内网一律拒绝（含数字字面量跳板）
        assert!(super::resolve_redirect_url("https://example.com/", "http://127.0.0.1/x").is_err());
        assert!(super::resolve_redirect_url(
            "https://example.com/",
            "http://169.254.169.254/latest/meta-data"
        )
        .is_err());
        assert!(super::resolve_redirect_url("https://example.com/", "http://2130706433/").is_err());
        assert!(super::resolve_redirect_url("https://example.com/", "http://localhost/admin").is_err());
        // 非 http(s) scheme 拒绝
        assert!(super::resolve_redirect_url("https://example.com/", "file:///c:/windows").is_err());
        assert!(super::resolve_redirect_url("https://example.com/", "javascript:alert(1)").is_err());
    }

    #[test]
    fn open_target_validation() {
        assert!(super::is_open_target_allowed("https://example.com"));
        assert!(super::is_open_target_allowed("http://example.com"));
        assert!(super::is_open_target_allowed("  HTTPS://example.com  "));
        assert!(super::is_open_target_allowed("mailto:a@b.c"));
        // Windows 设置页 URI（由设置应用处理，无代码执行面）
        assert!(super::is_open_target_allowed("ms-settings:display"));
        assert!(super::is_open_target_allowed("MS-Settings:WindowsUpdate"));
        assert!(!super::is_open_target_allowed("file:///c:/windows"));
        assert!(!super::is_open_target_allowed("javascript:alert(1)"));
        assert!(!super::is_open_target_allowed("relative/path.txt"));
        // 本地路径面整体裁撤（负枚举的黑名单追不上执行面）：文档、目录、
        // 可执行文件、UNC 一律拒绝——需要开本地文件的皮肤走 shell 权限
        assert!(!super::is_open_target_allowed("C:\\docs\\a.pdf"));
        assert!(!super::is_open_target_allowed("C:\\no-such-file-driftlet.xyz"));
        assert!(!super::is_open_target_allowed("C:\\tools"));
        assert!(!super::is_open_target_allowed("\\\\server\\share\\doc.pdf"));
        assert!(!super::is_open_target_allowed("//server/share/doc.pdf"));
        let abs = std::env::current_exe().unwrap();
        assert!(!super::is_open_target_allowed(abs.to_str().unwrap()));
    }

    #[test]
    fn open_external_gate_tiers() {
        // http(s) 目标：open_link 低危或 system 高危任一；其余目标只过 system
        assert_eq!(
            super::open_external_required_perms("https://example.com"),
            &["open_link", "system"][..]
        );
        assert_eq!(
            super::open_external_required_perms("  HTTP://example.com/a?b=c "),
            &["open_link", "system"][..]
        );
        for target in [
            "mailto:a@b.c",
            "ms-settings:display",
            "C:\\docs\\a.pdf",
            "ftp://example.com",
            "file:///c:/windows",
        ] {
            assert_eq!(
                super::open_external_required_perms(target),
                &["system"][..],
                "{target} must require system"
            );
        }
    }

    fn guard_temp_base(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("driftlet-guard-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn make_data_roots(base: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
        let skins = base.join("skins");
        let config = base.join("config");
        std::fs::create_dir_all(skins.join("clock")).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(skins.join("clock").join("skin.json"), "{}").unwrap();
        std::fs::write(config.join("config.json"), "{}").unwrap();
        (skins, config)
    }

    #[test]
    fn mutable_guard_blocks_data_roots() {
        let base = guard_temp_base("roots");
        let (skins, config) = make_data_roots(&base);
        // 已存在文件（改写自己 skin.json = 自我提权路径）、尚未存在的
        // 文件/目录、根目录自身、深层新路径一律拒绝
        for target in [
            skins.join("clock").join("skin.json"),
            skins.join("clock").join("evil.js"),
            skins.join("new-skin").join("skin.json"),
            skins.clone(),
            config.join("config.json"),
            config.join("deep").join("new.txt"),
            config.clone(),
        ] {
            assert!(
                super::ensure_mutable_any_path(&skins, &config, &target).is_err(),
                "{:?} must be blocked",
                target
            );
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn mutable_guard_allows_outside_paths() {
        let base = guard_temp_base("outside");
        let (skins, config) = make_data_roots(&base);
        let outside = base.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("ok.txt"), "x").unwrap();

        assert!(super::ensure_mutable_any_path(&skins, &config, &outside.join("ok.txt")).is_ok());
        // 尚不存在的深层目标（最深现存祖先重拼尾段）
        assert!(super::ensure_mutable_any_path(&skins, &config, &outside.join("new").join("deep.txt")).is_ok());
        // 前缀陷阱：分量级比较不会把 skins-evil / config.json 当成数据根
        assert!(super::ensure_mutable_any_path(&skins, &config, &base.join("skins-evil")).is_ok());
        assert!(super::ensure_mutable_any_path(&skins, &config, &base.join("config.json")).is_ok());
        // `..` 分量一律拒绝（哪怕词法上指向数据根之外）。
        // 必须从字符串构造测试路径：PathBuf::join("..") 在 verbatim 基底
        //（canonicalize 的产物）上会直接把 `..` 词法消掉（pop 尾部分量），
        // 生产路径无此问题——皮肤路径来自 JSON 字符串解析，`..` 原样进入
        let dotdot = std::path::PathBuf::from(format!(
            r"{}\outside\..\outside\ok.txt",
            base.display()
        ));
        assert!(super::ensure_mutable_any_path(&skins, &config, &dotdot).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn mutable_guard_blocks_case_variants_and_missing_roots() {
        let base = guard_temp_base("case");
        let (skins, config) = make_data_roots(&base);
        // NTFS 大小写不敏感：现存分量经 canonicalize 拿真实大小写
        assert!(super::ensure_mutable_any_path(
            &skins,
            &config,
            &base.join("SKINS").join("clock").join("skin.json")
        )
        .is_err());

        // 数据根缺失（尚未创建）：现存祖先重拼后，尾段大小写变体也要拦
        //（否则皮肤可预植 skins/<id>/skin.json 绕过安装页权限确认）
        let gone = base.join("gone");
        let missing_skins = gone.join("skins");
        let missing_config = gone.join("config");
        assert!(super::ensure_mutable_any_path(
            &missing_skins,
            &missing_config,
            &gone.join("SKINS").join("planted").join("skin.json")
        )
        .is_err());
        assert!(super::ensure_mutable_any_path(
            &missing_skins,
            &missing_config,
            &gone.join("Config").join("config.json")
        )
        .is_err());
        // 缺失根的相邻路径不误伤
        assert!(super::ensure_mutable_any_path(
            &missing_skins,
            &missing_config,
            &gone.join("other").join("ok.txt")
        )
        .is_ok());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 更新下载目录入禁写根（审查 H2）：皮肤改写 update/ 下的安装包 +
    /// 版本标记，用户点「立即安装」即执行任意代码。目录尚不存在也要拦
    ///（现存祖先重拼尾段的形态）。
    #[test]
    fn mutable_guard_blocks_update_dir() {
        let base = guard_temp_base("update");
        let (skins, config) = make_data_roots(&base);
        let update = base.join("update"); // update_dir(config) = config 的父目录 / update
        for target in [
            update.join("Driftlet-update-setup.exe"),
            update.join("downloaded-version.json"),
            update.join("new").join("deep.exe"),
            update.clone(),
        ] {
            assert!(
                super::ensure_mutable_any_path(&skins, &config, &target).is_err(),
                "{:?} must be blocked",
                target
            );
        }
        // 相邻同前缀目录不误伤（update-evil 不是 update）
        assert!(super::ensure_mutable_any_path(&skins, &config, &base.join("update-evil")).is_ok());
        let _ = std::fs::remove_dir_all(&base);
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
