//! Small read-only status probes (Windows): battery, idle time, foreground
//! window, monitor list.  All free-tier (no permission) — they only observe.

use super::{BatteryInfo, ForegroundWindowInfo, MonitorInfo, Rect};

pub fn battery() -> Result<BatteryInfo, String> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

    let mut sps = SYSTEM_POWER_STATUS::default();
    unsafe {
        if GetSystemPowerStatus(&mut sps).is_err() {
            return Err("GetSystemPowerStatus failed".to_string());
        }
    }
    // BatteryFlag: 128 = no system battery, 255 = unknown, bit 3 (8) = charging
    let has_battery = sps.BatteryFlag != 128 && sps.BatteryFlag != 255;
    Ok(BatteryInfo {
        has_battery,
        ac_online: sps.ACLineStatus == 1,
        charging: sps.BatteryFlag & 8 != 0,
        percent: (sps.BatteryLifePercent <= 100).then_some(sps.BatteryLifePercent),
        secs_remaining: (sps.BatteryLifeTime != u32::MAX).then_some(sps.BatteryLifeTime),
    })
}

// ─── Idle time ───

/// Milliseconds since the last keyboard/mouse input.
pub fn idle_ms() -> Result<u64, String> {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    let mut lii = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    unsafe {
        if !GetLastInputInfo(&mut lii).as_bool() {
            return Err("GetLastInputInfo failed".to_string());
        }
        // Both are 32-bit tick counters; wrapping_sub handles the 49.7-day wrap.
        Ok(GetTickCount().wrapping_sub(lii.dwTime) as u64)
    }
}

// ─── Foreground window ───

pub fn foreground_window() -> Option<ForegroundWindowInfo> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut buf);
        let title = String::from_utf16_lossy(&buf[..len as usize]);

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));

        Some(ForegroundWindowInfo {
            title,
            pid,
            process_name: process_name_of(pid).unwrap_or_default(),
        })
    }
}

#[cfg(target_os = "windows")]
fn process_name_of(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        // 先取查询结果再无条件关句柄——查询失败经 `.ok()?` 提前返回时
        // 句柄也不能泄漏。
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        result.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        Some(
            std::path::Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or(path),
        )
    }
}

// ─── Monitors ───

pub fn monitors() -> Vec<MonitorInfo> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
    };
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

    unsafe extern "system" fn collect(
        hmon: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        // panic 跨 FFI 边界 unwind 是 UB——拦住，保留已收集的结果并让
        // 枚举继续。
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let out = &mut *(lparam.0 as *mut Vec<MonitorInfo>);
            let mut mi = MONITORINFOEXW {
                monitorInfo: MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32,
                    ..Default::default()
                },
                ..Default::default()
            };
            if GetMonitorInfoW(hmon, &mut mi as *mut _ as *mut _).as_bool() {
                let to_rect = |r: RECT| Rect {
                    x: r.left,
                    y: r.top,
                    width: r.right - r.left,
                    height: r.bottom - r.top,
                };
                let mut dpi_x = 96u32;
                let mut dpi_y = 96u32;
                let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
                let end = mi
                    .szDevice
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(mi.szDevice.len());
                out.push(MonitorInfo {
                    name: String::from_utf16_lossy(&mi.szDevice[..end]),
                    rect: to_rect(mi.monitorInfo.rcMonitor),
                    work_area: to_rect(mi.monitorInfo.rcWork),
                    is_primary: mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
                    scale_factor: dpi_x as f64 / 96.0,
                });
            }
            BOOL(1)
        }))
        .unwrap_or(BOOL(1))
    }

    let mut out = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut out as *mut _ as isize),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_query_succeeds() {
        // Every machine answers this; desktops just report has_battery=false.
        let b = battery().unwrap();
        if b.has_battery {
            assert!(b.percent.is_some());
        }
    }

    #[test]
    fn idle_time_query_succeeds() {
        // No upper bound can be asserted — the user may genuinely be away
        // from the keyboard while the suite runs.  Smoke-test the API only:
        // it must answer, and the value must fit the 32-bit tick window.
        let ms = idle_ms().unwrap();
        assert!(ms <= u32::MAX as u64);
    }

    #[test]
    fn at_least_one_primary_monitor() {
        let ms = monitors();
        assert!(!ms.is_empty());
        assert!(ms.iter().any(|m| m.is_primary));
        assert!(ms.iter().all(|m| m.rect.width > 0 && m.scale_factor > 0.0));
    }
}
