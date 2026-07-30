//! GPU information (Windows only).
//!
//! - Name / LUID / dedicated VRAM: DXGI adapter enumeration
//!   (`IDXGIAdapter3::QueryVideoMemoryInfo` for the live usage)
//! - Utilization %: PDH `\GPU Engine(*)\Utilization Percentage`, instances
//!   carry the adapter LUID in their name and are summed per adapter
//!
//! First call after process start reports usage 0 (PDH baseline), same
//! convention as the CPU sampler.

use std::collections::HashMap;
use std::sync::Mutex;
use super::GpuInfo;

struct AdapterInfo {
    name: String,
    luid: (u32, u32), // (HighPart as u32, LowPart)
    vram_total: u64,
    vram_used: u64,
}

pub fn collect() -> Vec<GpuInfo> {
    let adapters = enum_adapters();
    let usage = usage_by_luid();

    adapters
        .into_iter()
        .map(|a| {
            let usage = usage.get(&a.luid).copied().unwrap_or(0.0).min(100.0);
            let vram_usage_pct = if a.vram_total > 0 {
                ((a.vram_used as f64 / a.vram_total as f64) * 100.0).min(100.0) as f32
            } else {
                0.0
            };
            GpuInfo {
                name: a.name,
                usage,
                vram_total: a.vram_total,
                vram_used: a.vram_used,
                vram_usage_pct,
            }
        })
        .collect()
}

// ─── DXGI: adapters + VRAM ───

fn enum_adapters() -> Vec<AdapterInfo> {
    use windows::core::Interface;
    use windows::Win32::Graphics::Dxgi::*;

    let mut out = Vec::new();
    unsafe {
        let factory: IDXGIFactory1 = match CreateDXGIFactory1() {
            Ok(f) => f,
            Err(e) => {
                log::warn!("CreateDXGIFactory1 failed: {:?}", e);
                return out;
            }
        };
        for i in 0.. {
            let adapter: IDXGIAdapter1 = match factory.EnumAdapters1(i) {
                Ok(a) => a,
                Err(_) => break,
            };
            let desc = match adapter.GetDesc1() {
                Ok(d) => d,
                Err(_) => continue,
            };
            // DXGI_ADAPTER_FLAG_SOFTWARE = 2 — skip "Microsoft Basic Render Driver"
            if desc.Flags & 2 != 0 {
                continue;
            }
            let end = desc
                .Description
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(desc.Description.len());
            let name = String::from_utf16_lossy(&desc.Description[..end]);

            let mut vram_used = 0u64;
            if let Ok(a3) = adapter.cast::<IDXGIAdapter3>() {
                let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
                if a3
                    .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info)
                    .is_ok()
                {
                    vram_used = info.CurrentUsage;
                }
            }

            out.push(AdapterInfo {
                name,
                luid: (desc.AdapterLuid.HighPart as u32, desc.AdapterLuid.LowPart),
                vram_total: desc.DedicatedVideoMemory as u64,
                vram_used,
            });
        }
    }
    out
}

// ─── PDH: per-engine utilization, grouped by adapter LUID ───

static GPU_PDH: Mutex<Option<super::pdh::PdhMultiCounter>> = Mutex::new(None);

fn usage_by_luid() -> HashMap<(u32, u32), f32> {
    let mut map: HashMap<(u32, u32), f32> = HashMap::new();
    let mut guard = GPU_PDH.lock().unwrap();
    if guard.is_none() {
        // Stays None on PDH failure → retried on the next call.
        *guard = super::pdh::PdhMultiCounter::new("\\GPU Engine(*)\\Utilization Percentage");
    }
    let Some(counter) = guard.as_mut() else {
        return map;
    };
    for (name, value) in counter.sample() {
        if let Some(luid) = luid_from_instance(&name) {
            *map.entry(luid).or_insert(0.0) += value.max(0.0) as f32;
        }
    }
    map
}

/// Instance names look like
/// `pid_1234_luid_0x00000000_0x0003F7D5_phys_0_eng_0_engtype_3D`.
fn luid_from_instance(name: &str) -> Option<(u32, u32)> {
    let idx = name.find("luid_")? + 5;
    let mut parts = name[idx..].split('_');
    let hi = u32::from_str_radix(parts.next()?.trim_start_matches("0x"), 16).ok()?;
    let lo = u32::from_str_radix(parts.next()?.trim_start_matches("0x"), 16).ok()?;
    Some((hi, lo))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_luid_from_instance_name() {
        assert_eq!(
            luid_from_instance("pid_1234_luid_0x00000000_0x0003F7D5_phys_0_eng_0_engtype_3D"),
            Some((0, 0x3F7D5))
        );
        assert_eq!(luid_from_instance("pid_1_engtype_3D"), None);
    }

    #[test]
    #[ignore = "hardware probe — run manually with --nocapture"]
    fn probe_gpu_info() {
        let first = collect();
        std::thread::sleep(std::time::Duration::from_secs(1));
        let second = collect();
        println!("first : {:#?}", first);
        println!("second: {:#?}", second);
    }
}
