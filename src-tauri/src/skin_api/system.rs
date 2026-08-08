//! OS/host information and process listing (cross-platform, sysinfo-backed).

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct OsInfo {
    /// 产品名（注册表 ProductName），如 "Windows 11 Pro"；取不到回退 "Windows"。
    pub os_name: String,
    /// sysinfo 0.32 Windows 格式 "{major} ({build})"，如 "11 (22631)"。
    pub os_version: String,
    pub build: Option<u32>,
    pub is_windows_11: bool,
    pub host_name: String,
    pub user_name: String,
    pub uptime_secs: u64,
}

/// 从 os_version 提取 Windows build 号。sysinfo 0.32 在 Windows 上返回
/// "{major} ({build})"（注册表 CurrentBuildNumber 拼装，无点号——按
/// "10.0.22631" 点号三段解析会恒 None）；旧版与其他平台是点号三段。
/// 两种格式都试，Windows 以外通常拿不到 build 返回 None。
pub(crate) fn parse_windows_build(os_version: &str) -> Option<u32> {
    os_version
        .rsplit_once('(')
        .and_then(|(_, s)| s.trim_end_matches(')').parse::<u32>().ok())
        .or_else(|| {
            os_version
                .split('.')
                .nth(2)
                .and_then(|b| b.parse::<u32>().ok())
        })
}

pub fn os_info() -> OsInfo {
    // long_os_version = 注册表 ProductName（"Windows 11 Pro"）；name() 在
    // Windows 上恒为 "Windows"，只作兜底
    let os_name = sysinfo::System::long_os_version()
        .or_else(sysinfo::System::name)
        .unwrap_or_default();
    let os_version = sysinfo::System::os_version().unwrap_or_default();
    let build = parse_windows_build(&os_version);
    OsInfo {
        os_name,
        os_version,
        build,
        // 注册表构建号判 Win11（非 manifest 截断的兼容版本号）
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
    /// 整机口径 0–100（任务管理器同口径；首调 = 0，基线）。
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

    // sysinfo 的进程 CPU 是「单核口径」（公式 100×Δ进程/Δ系统×核数，量程
    // 0–100×核数）——除以核数归一化到整机 0–100，任务管理器同口径；皮肤
    // 按 0–100 画条才不会溢出
    let nb_cpus = sys.cpus().len().max(1) as f32;
    let mut list: Vec<ProcessInfo> = sys
        .processes()
        .iter()
        .map(|(pid, p)| ProcessInfo {
            pid: pid.as_u32(),
            name: p.name().to_string_lossy().to_string(),
            cpu: (p.cpu_usage() / nb_cpus).min(100.0),
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
        #[cfg(target_os = "windows")]
        assert!(
            info.build.is_some(),
            "Windows build should parse from os_version: {}",
            info.os_version
        );
        assert!(info.uptime_secs > 0);
    }

    #[test]
    fn parses_windows_build_formats() {
        // sysinfo 0.32 Windows 格式
        assert_eq!(parse_windows_build("11 (22631)"), Some(22631));
        assert_eq!(parse_windows_build("10 (19045)"), Some(19045));
        // 旧版/其他平台的点号三段
        assert_eq!(parse_windows_build("10.0.22631"), Some(22631));
        assert_eq!(parse_windows_build("5.15.0-91-generic"), None);
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
