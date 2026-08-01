//! GPU information (Windows only).
//!
//! - Name / LUID / dedicated VRAM total: DXGI adapter enumeration
//! - VRAM used: PDH `\GPU Adapter Memory(*)\Dedicated Usage` per adapter LUID —
//!   AMD 驱动下 `IDXGIAdapter3::QueryVideoMemoryInfo` 恒返 0，DXGI 值仅作
//!   PDH 未就绪时的回退
//! - Utilization %: PDH `\GPU Engine(*)\Utilization Percentage`, instances
//!   carry the adapter LUID in their name and are summed per adapter
//! - IddCx 虚拟显示器（向日葵 OrayIddDriver 等）会把渲染 GPU 的名字、
//!   vid/did/vram 整体克隆后混入 DXGI 枚举——它没有 WDDM 性能计数器实例，
//!   按「引擎/显存计数器实例名中的 LUID 并集」过滤；PDH 不可用时不过滤
//!
//! First call after process start reports usage/vram-used 0 (PDH baseline),
//! same convention as the CPU sampler.

use std::collections::{HashMap, HashSet};
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
    // 先采样（同时初始化两个 PDH 计数器），再取实例名集合
    let usage = usage_by_luid();
    let vram = vram_used_by_luid();
    let present = pdh_adapter_luids();

    adapters
        .into_iter()
        // 克隆适配器（IddCx 虚拟显示器）在性能计数器里没有实例，剔除；
        // present 为空说明 PDH 枚举失败，退化为不过滤（保持旧行为）
        .filter(|a| present.is_empty() || present.contains(&a.luid))
        .map(|a| {
            let usage = usage.get(&a.luid).copied().unwrap_or(0.0).min(100.0);
            // 显存用量以 PDH 为准（AMD 的 DXGI 值恒为 0）；PDH 未采到
            // （首调基线/计数器缺失）时回退 DXGI 值
            let vram_used = vram.get(&a.luid).copied().unwrap_or(a.vram_used);
            let vram_usage_pct = if a.vram_total > 0 {
                ((vram_used as f64 / a.vram_total as f64) * 100.0).min(100.0) as f32
            } else {
                0.0
            };
            GpuInfo {
                name: a.name,
                usage,
                vram_total: a.vram_total,
                vram_used,
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

// ─── PDH: 显存用量（Dedicated Usage）───

static GPU_VRAM_PDH: Mutex<Option<super::pdh::PdhMultiCounter>> = Mutex::new(None);

/// 每适配器已用显存（字节）；实例名同样携带 LUID，按适配器汇总
fn vram_used_by_luid() -> HashMap<(u32, u32), u64> {
    let mut map: HashMap<(u32, u32), u64> = HashMap::new();
    let mut guard = GPU_VRAM_PDH.lock().unwrap();
    if guard.is_none() {
        *guard = super::pdh::PdhMultiCounter::new("\\GPU Adapter Memory(*)\\Dedicated Usage");
    }
    let Some(counter) = guard.as_mut() else {
        return map;
    };
    for (name, value) in counter.sample() {
        if let Some(luid) = luid_from_instance(&name) {
            *map.entry(luid).or_insert(0) += value.max(0.0) as u64;
        }
    }
    map
}

/// 性能计数器里真实存在的适配器 LUID 集合——「GPU 引擎占用」与「GPU 显存」
/// 两个计数器实例名（均携带 LUID）的并集。实例名在计数器基线采集后即可
/// 读取，不受速率计数器两阶段采样限制，首次调用即有效；且走
/// `PdhAddEnglishCounterW` 路径，中文系统上不受对象名本地化影响。
/// 返回空集合表示 PDH 不可用（调用方退化为不过滤）。
fn pdh_adapter_luids() -> HashSet<(u32, u32)> {
    let mut set = HashSet::new();
    // 两个静态计数器已在上面的 usage_by_luid / vram_used_by_luid 中初始化
    let mut engine = GPU_PDH.lock().unwrap();
    if let Some(counter) = engine.as_mut() {
        for name in counter.instance_names() {
            if let Some(luid) = luid_from_instance(&name) {
                set.insert(luid);
            }
        }
    }
    drop(engine);
    let mut vram = GPU_VRAM_PDH.lock().unwrap();
    if let Some(counter) = vram.as_mut() {
        for name in counter.instance_names() {
            if let Some(luid) = luid_from_instance(&name) {
                set.insert(luid);
            }
        }
    }
    set
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

    #[test]
    #[ignore = "hardware probe — run manually with --nocapture"]
    fn probe_adapter_luids() {
        println!("{:?}", pdh_adapter_luids());
    }

    /// Dump the full DXGI desc of every enumerated adapter — used to find a
    /// field that distinguishes real hardware from IddCx (virtual display)
    /// clones, which surface under the render GPU's name.
    #[test]
    #[ignore = "hardware probe — run manually with --nocapture"]
    fn probe_dxgi_descs() {
        use windows::core::Interface;
        use windows::Win32::Graphics::Dxgi::*;

        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1().unwrap();
            for i in 0.. {
                let adapter: IDXGIAdapter1 = match factory.EnumAdapters1(i) {
                    Ok(a) => a,
                    Err(_) => break,
                };
                let d = adapter.GetDesc1().unwrap();
                let end = d
                    .Description
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(d.Description.len());
                let name = String::from_utf16_lossy(&d.Description[..end]);
                let has_vmem = adapter.cast::<IDXGIAdapter3>().is_ok();
                println!(
                    "#{} {:?}\n  vid={:#06x} did={:#06x} subsys={:#010x} rev={:#04x} flags={}\n  \
                     dedicated_vram={} dedicated_sysmem={} shared_sysmem={}\n  \
                     luid={:#010x}_{:#010x} idxgiadapter3={}",
                    i,
                    name,
                    d.VendorId,
                    d.DeviceId,
                    d.SubSysId,
                    d.Revision,
                    d.Flags,
                    d.DedicatedVideoMemory,
                    d.DedicatedSystemMemory,
                    d.SharedSystemMemory,
                    d.AdapterLuid.HighPart,
                    d.AdapterLuid.LowPart,
                    has_vmem,
                );
            }
        }
    }
}
