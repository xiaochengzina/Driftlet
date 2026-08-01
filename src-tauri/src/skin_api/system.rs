//! OS/host information and process listing (cross-platform, sysinfo-backed).

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct OsInfo {
    /// e.g. "Windows 11 Pro" / distribution name.
    pub os_name: String,
    /// e.g. "10.0.22631".
    pub os_version: String,
    pub build: Option<u32>,
    pub is_windows_11: bool,
    pub host_name: String,
    pub user_name: String,
    pub uptime_secs: u64,
}

pub fn os_info() -> OsInfo {
    let os_name = sysinfo::System::name().unwrap_or_default();
    let os_version = sysinfo::System::os_version().unwrap_or_default();
    let build = os_version
        .split('.')
        .nth(2)
        .and_then(|b| b.parse::<u32>().ok());
    OsInfo {
        os_name,
        os_version,
        build,
        // Same truth source as commands::is_windows_11_or_newer
        // (RtlGetVersion-based, not manifest-capped).
        is_windows_11: build.map(|b| b >= 22000).unwrap_or(true),
        host_name: sysinfo::System::host_name().unwrap_or_default(),
        user_name: std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_default(),
        uptime_secs: sysinfo::System::uptime(),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    /// % of one logical core, like Task Manager (first call = 0, baseline).
    pub cpu: f32,
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessList {
    pub total: usize,
    pub processes: Vec<ProcessInfo>,
}

pub fn processes(sys: &mut sysinfo::System, sort: &str, limit: usize) -> ProcessList {
    // cpu + memory only: the default (ProcessRefreshKind::everything()) also
    // pulls every process's exe path / cmd line / environment — on Windows
    // each of those is a remote read of the process's PEB, and the strings
    // then sit resident in the process table.  We only ever return name,
    // cpu and memory (name always comes from the toolhelp snapshot).
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::All,
        true,
        sysinfo::ProcessRefreshKind::new().with_cpu().with_memory(),
    );

    let mut list: Vec<ProcessInfo> = sys
        .processes()
        .iter()
        .map(|(pid, p)| ProcessInfo {
            pid: pid.as_u32(),
            name: p.name().to_string_lossy().to_string(),
            cpu: p.cpu_usage(),
            memory_bytes: p.memory(),
        })
        .collect();

    match sort {
        "memory" => list.sort_by(|a, b| b.memory_bytes.cmp(&a.memory_bytes)),
        _ => list.sort_by(|a, b| {
            b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal)
        }),
    }
    list.truncate(limit.clamp(1, 100));

    ProcessList {
        total: sys.processes().len(),
        processes: list,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_info_is_populated() {
        let info = os_info();
        assert!(!info.os_name.is_empty());
        assert!(!info.os_version.is_empty());
        assert!(info.uptime_secs > 0);
    }

    #[test]
    fn process_listing_respects_limit_and_sort() {
        let mut sys = sysinfo::System::new_all();
        let list = processes(&mut sys, "memory", 5);
        assert!(list.total > 0);
        assert!(list.processes.len() <= 5);
        // Memory sort: descending
        assert!(list
            .processes
            .windows(2)
            .all(|w| w[0].memory_bytes >= w[1].memory_bytes));
    }
}
