/// "On Desktop" pinning — keep skin windows glued to the desktop layer so
/// they survive "Show Desktop" (Win+D / the taskbar show-desktop button)
/// on both Windows 10 and Windows 11.
///
/// What Win+D actually does to a pinned skin (verified empirically on
/// Win10 21H2 against the real app, tools/win32-probes):
///
///   * The skin is NOT minimized.  Minimize-all only targets windows that
///     can be minimized (WS_MINIMIZEBOX); the frameless skin has none, so
///     IsIconic stays false through Win+D.
///   * What hides the skin is Z-ORDER: Show Desktop raises the
///     desktop-icons host (and the wallpaper WorkerW) to the top of the
///     non-topmost band, covering everything that lives in the desktop
///     layer.  Any one-shot "reposition on the transition" logic races
///     with that raise and loses — which was the old bug.
///
/// Fix: a 250 ms enforcement loop keeps every pinned skin glued DIRECTLY
/// ABOVE the desktop-icons host at all times (with several pinned skins
/// they form a stack right above the host — see enforce_desktop_layer):
///
///   * Normal state: the host sits at the bottom of the z-order, so the
///     skin rests on the desktop — above the icons, below normal windows.
///   * Show Desktop: the host is raised to the top of the non-topmost
///     band; the skin rises with it and stays visible above the desktop
///     (genuine always-on-top windows stay above the skin, as intended).
///   * Restore: the host drops back down and the skin follows.
///
/// There is deliberately NO show-desktop state machine and NO helper
/// window: every tick re-verifies the adjacency and repairs it, so the
/// mechanism cannot get stuck (the old one did — the helper stayed
/// topmost forever after a restore), cannot miss a transition, and also
/// heals by itself after explorer.exe restarts (the host is
/// re-discovered when the cached handle dies).
///
/// Two refinements:
///   * A skin that currently has the input focus is left alone — clicking
///     a skin raises it above normal windows while it is being used; it
///     sinks back to the desktop layer once it loses foreground.
///   * Pinned skins carry WS_EX_TOOLWINDOW (no WS_EX_APPWINDOW) so they
///     stay out of the taskbar / Alt+Tab and out of minimize-all's way;
///     unpinning clears BOTH bits — tao never sets WS_EX_APPWINDOW on a
///     skip_taskbar window, so "restoring" it would add a bit the window
///     never had and the skin would appear in the taskbar / Alt+Tab.

#[cfg(target_os = "windows")]
mod imp {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::ptr::null_mut;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tauri::AppHandle;
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, GetClassNameW, GetForegroundWindow, GetShellWindow, GetSystemMetrics,
        GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindow,
        IsWindowVisible, SetWindowLongPtrW, SetWindowPos, ShowWindow, GW_CHILD, GW_HWNDNEXT,
        GW_HWNDPREV, GWL_EXSTYLE, HWND_BOTTOM, HWND_NOTOPMOST, HWND_TOP, SET_WINDOW_POS_FLAGS,
        SW_HIDE, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER,
        SWP_NOSENDCHANGING, SWP_NOSIZE, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    };

    /// How often the enforcement loop re-verifies that every pinned skin
    /// sits directly above the desktop-icons host.  Same cadence Rainmeter
    /// uses for its show-desktop resync; cheap (a few Win32 calls).
    const ENFORCE_INTERVAL_MS: u64 = 250;

    const ZPOS_FLAGS: SET_WINDOW_POS_FLAGS = SET_WINDOW_POS_FLAGS(
        SWP_NOMOVE.0 | SWP_NOSIZE.0 | SWP_NOOWNERZORDER.0 | SWP_NOACTIVATE.0 | SWP_NOSENDCHANGING.0,
    );

    // ── tracked state ────────────────────────────────────────────────────

    struct Inner {
        skins: Vec<(String, isize)>,
    }

    // ── public Pinner ────────────────────────────────────────────────────

    pub struct Pinner {
        inner: Arc<Mutex<Inner>>,
    }

    unsafe impl Send for Pinner {}
    unsafe impl Sync for Pinner {}

    impl Pinner {
        pub fn new(_app: AppHandle, main_thread_id: std::thread::ThreadId) -> Self {
            assert_eq!(
                std::thread::current().id(),
                main_thread_id,
                "Pinner must be created on the main thread"
            );

            let inner = Arc::new(Mutex::new(Inner { skins: Vec::new() }));

            let i = inner.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_millis(ENFORCE_INTERVAL_MS));
                enforce_desktop_layer(&i);
            });

            Self { inner }
        }

        pub fn pin(&self, skin_id: &str, skin_hwnd: isize) {
            // A pinned skin is a desktop tool window: no taskbar button,
            // no Alt+Tab entry, no minimize-all attention.
            set_tool_window_bits(skin_hwnd, true);
            {
                let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                inner.skins.retain(|(id, _)| id != skin_id);
                inner.skins.push((skin_id.to_string(), skin_hwnd));
            }
            // Anchor immediately so the skin drops to the desktop layer now.
            enforce_desktop_layer(&self.inner);
        }

        pub fn unpin(&self, skin_id: &str, skin_hwnd: isize) {
            {
                let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                inner.skins.retain(|(id, _)| id != skin_id);
            }
            if skin_hwnd != 0 {
                set_tool_window_bits(skin_hwnd, false);
                unsafe {
                    let hwnd = HWND(skin_hwnd as *mut _);
                    if IsWindow(Some(hwnd)).as_bool() {
                        let _ = SetWindowPos(hwnd, Some(HWND_BOTTOM), 0, 0, 0, 0, ZPOS_FLAGS);
                    }
                }
            }
        }
    }

    // ── enforcement ──────────────────────────────────────────────────────

    /// Set (pinned=true) or clear (pinned=false) the WS_EX_TOOLWINDOW /
    /// WS_EX_APPWINDOW bits on a skin window.  Clearing returns to the tao
    /// baseline: tao never sets WS_EX_APPWINDOW on a skip_taskbar window,
    /// so both bits come off.
    fn set_tool_window_bits(skin_hwnd: isize, pinned: bool) {
        if skin_hwnd == 0 {
            return;
        }
        unsafe {
            let hwnd = HWND(skin_hwnd as *mut _);
            if !IsWindow(Some(hwnd)).as_bool() {
                return;
            }
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let want = if pinned {
                (ex | WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize)
            } else {
                ex & !(WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize)
            };
            if want != ex {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, want);
            }
        }
    }

    /// The core loop: keep every pinned skin glued directly above the
    /// desktop-icons host.  Self-healing — safe to call at any time.
    fn enforce_desktop_layer(inner: &Arc<Mutex<Inner>>) {
        let skins = inner.lock().unwrap_or_else(|e| e.into_inner()).skins.clone();
        if skins.is_empty() {
            return;
        }

        let host = match get_desktop_icons_host_window() {
            Some(h) if unsafe { IsWindow(Some(h)).as_bool() } => h,
            _ => return, // shell not up (or restarting) — retry next tick
        };

        let mut dead: Vec<(String, isize)> = Vec::new();
        for (id, hwnd_val) in &skins {
            let hwnd = HWND(*hwnd_val as *mut _);
            unsafe {
                if !IsWindow(Some(hwnd)).as_bool() {
                    dead.push((id.clone(), *hwnd_val));
                    continue;
                }

                // HWNDs are recycled by the system: a stale entry may now
                // name another process's window.  Never touch it — reap it.
                let mut pid: u32 = 0;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                if pid != std::process::id() {
                    dead.push((id.clone(), *hwnd_val));
                    continue;
                }

                // Insurance: if some OS build / shell action did manage to
                // minimize the skin, bring it back.
                if IsIconic(hwnd).as_bool() {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                }

                // While the user is interacting with the skin it may stay
                // raised; it sinks back once it loses foreground.
                if GetForegroundWindow() == hwnd {
                    continue;
                }

                // Pinned skins are never topmost.
                let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                if (ex & WS_EX_TOPMOST.0 as isize) != 0 {
                    let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, ZPOS_FLAGS);
                }

                // tao style rewrites can restore WS_EX_APPWINDOW — put the
                // tool-window bits back if they drifted.
                set_tool_window_bits(*hwnd_val, true);

                // Glue: a pinned skin is in place when the window directly
                // BELOW it is the icons host or another pinned skin — with
                // ≥2 pinned skins the stricter "directly above the host"
                // invariant cannot hold for all of them and made the skins
                // flip positions every tick.  Repairs insert the skin
                // directly above the host: whatever is immediately above
                // the host right now becomes the insert-after reference, so
                // the skin lands between the two.  Works in both directions
                // (raised by a click -> pull down; covered by a show-desktop
                // raise -> lift up).  When the host itself tops the z-order
                // (no window above it), fall back to HWND_TOP — otherwise a
                // covered skin would never be lifted.
                let next = GetWindow(hwnd, GW_HWNDNEXT).unwrap_or(HWND(null_mut()));
                let in_place = !next.0.is_null()
                    && (next.0 == host.0
                        || skins.iter().any(|(_, h)| *h == next.0 as isize));
                if !in_place {
                    let prev = GetWindow(host, GW_HWNDPREV).unwrap_or(HWND(null_mut()));
                    let after = if prev.0.is_null() { HWND_TOP } else { prev };
                    if after.0 != hwnd.0 {
                        let _ = SetWindowPos(hwnd, Some(after), 0, 0, 0, 0, ZPOS_FLAGS);
                    }
                }
            }
        }

        if !dead.is_empty() {
            // Match id AND hwnd: a skin reloaded mid-tick (unpin -> new
            // hwnd re-pinned) must not have its fresh entry reaped.
            inner
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .skins
                .retain(|(id, hwnd)| !dead.iter().any(|(did, dhwnd)| did == id && dhwnd == hwnd));
        }
    }

    // ── desktop shell discovery ──────────────────────────────────────────

    fn get_default_shell_window() -> Option<HWND> {
        let shell = unsafe { GetShellWindow() };
        if shell.0.is_null() {
            return None;
        }

        unsafe {
            let mut class = [0u16; 16];
            let len = GetClassNameW(shell, &mut class);
            let class_str = OsString::from_wide(&class[..len as usize]);
            if class_str != "Progman" {
                return None;
            }
        }

        Some(shell)
    }

    /// Find the window that hosts the desktop icons (SHELLDLL_DefView).
    ///
    /// The hierarchy is NOT stable across OS versions — or even across an
    /// explorer restart on the same machine:
    ///
    ///   * Win10 / Win11 <= 23H2 (steady state): a top-level WorkerW holds
    ///     SHELLDLL_DefView.
    ///   * Win11 24H2+: Progman holds SHELLDLL_DefView directly.
    ///   * Win10 right after explorer.exe restarts: Progman holds
    ///     SHELLDLL_DefView directly (observed on 21H2), until explorer
    ///     re-creates the WorkerW arrangement.
    ///
    /// So instead of version heuristics, collect every visible candidate
    /// host and pick the one HIGHEST in the z-order — that is the desktop
    /// surface actually being rendered; anything lower would be covered by
    /// another desktop window.  No caching: the enumeration is a handful of
    /// calls and fresh results are immune to explorer rebuilding windows.
    fn get_desktop_icons_host_window() -> Option<HWND> {
        let shell = get_default_shell_window()?;

        unsafe {
            let mut candidates: Vec<HWND> = Vec::new();

            // Candidate 1: the shell window itself hosts the icons.
            let shell_dv = FindWindowExW(Some(shell), Some(HWND(null_mut())), w!("SHELLDLL_DefView"), None)
                .unwrap_or(HWND(null_mut()));
            if !shell_dv.0.is_null() && IsWindowVisible(shell_dv).as_bool() {
                candidates.push(shell);
            }

            // Candidate 2: a top-level WorkerW hosts the icons.
            let mut workerw: HWND = HWND(null_mut());
            loop {
                workerw = FindWindowExW(Some(HWND(null_mut())), Some(workerw), w!("WorkerW"), None)
                    .unwrap_or(HWND(null_mut()));
                if workerw.0.is_null() {
                    break;
                }
                if !IsWindowVisible(workerw).as_bool() {
                    continue;
                }
                if !belong_to_same_process(shell, workerw) {
                    continue;
                }
                let dv = FindWindowExW(Some(workerw), Some(HWND(null_mut())), w!("SHELLDLL_DefView"), None)
                    .unwrap_or(HWND(null_mut()));
                if !dv.0.is_null() && IsWindowVisible(dv).as_bool() {
                    candidates.push(workerw);
                }
            }

            candidates.into_iter().min_by_key(|h| windows_above(*h))
        }
    }

    /// Number of top-level windows above `hwnd` in the z-order.
    fn windows_above(hwnd: HWND) -> usize {
        let mut n = 0;
        let mut cur = hwnd;
        unsafe {
            loop {
                match GetWindow(cur, GW_HWNDPREV) {
                    Ok(prev) if !prev.0.is_null() => {
                        n += 1;
                        cur = prev;
                    }
                    _ => break,
                }
            }
        }
        n
    }

    fn belong_to_same_process(a: HWND, b: HWND) -> bool {
        unsafe {
            let mut pid_a: u32 = 0;
            let mut pid_b: u32 = 0;
            GetWindowThreadProcessId(a, Some(&mut pid_a));
            GetWindowThreadProcessId(b, Some(&mut pid_b));
            pid_a == pid_b
        }
    }

    // ── 壁纸层移除的一次性迁移重绘 ───────────────────────────────────────

    /// 壁纸层移除的升级路径兜底（不是壁纸层功能的残留）：旧版本把皮肤
    /// 钉进壁纸 WorkerW 的用户，其壁纸表面可能留有黑色窗形破洞（旧版无
    /// 任何自愈，且洞不自愈）；升级到无壁纸层版本后这些洞无人清理，会
    /// 长期留在用户壁纸上。`normalize_mode_flags` 迁移到 wallpaper_layer
    /// 条目时调用一次：对所有候选壁纸表面做 SW_HIDE→SW_SHOW 强制全表面
    /// 重绘（实测唯一可愈手段）。后台线程 1 秒延迟执行（等启动稳定），
    /// 无迁移配置时零成本。
    pub fn repaint_wallpaper_surfaces_once() {
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(1000));
            unsafe {
                let Some(shell) = get_default_shell_window() else {
                    return;
                };
                let host = get_desktop_icons_host_window().unwrap_or(HWND(null_mut()));
                let mut surfaces: Vec<HWND> = Vec::new();
                // 经典形态：全屏可见的顶层 WorkerW（≠图标宿主）
                let mut w = HWND(null_mut());
                loop {
                    w = FindWindowExW(Some(HWND(null_mut())), Some(w), w!("WorkerW"), None)
                        .unwrap_or(HWND(null_mut()));
                    if w.0.is_null() {
                        break;
                    }
                    if w != host && is_fullscreen_visible(w) {
                        surfaces.push(w);
                    }
                }
                // 24H2+ 形态：Progman 的全屏 WorkerW 子窗（≠图标宿主）
                let mut child = GetWindow(shell, GW_CHILD).unwrap_or(HWND(null_mut()));
                while !child.0.is_null() {
                    if child != host && is_fullscreen_visible(child) && class_is(child, "WorkerW") {
                        surfaces.push(child);
                    }
                    child = GetWindow(child, GW_HWNDNEXT).unwrap_or(HWND(null_mut()));
                }
                for s in surfaces {
                    if IsWindow(Some(s)).as_bool() {
                        log::info!("migration: repaint wallpaper surface {:p} (hide->show)", s.0);
                        let _ = ShowWindow(s, SW_HIDE);
                        std::thread::sleep(Duration::from_millis(60));
                        let _ = ShowWindow(s, SW_SHOW);
                    }
                }
            }
        });
    }

    /// 全屏可见判定：系统常驻一批 136x39 的隐藏 WorkerW，按「可见 +
    /// 不小于主屏」过滤。
    fn is_fullscreen_visible(h: HWND) -> bool {
        unsafe {
            if !IsWindowVisible(h).as_bool() {
                return false;
            }
            let mut rc = RECT::default();
            if GetWindowRect(h, &mut rc).is_err() {
                return false;
            }
            (rc.right - rc.left) >= GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN)
                && (rc.bottom - rc.top) >= GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN)
        }
    }

    fn class_is(h: HWND, name: &str) -> bool {
        unsafe {
            let mut buf = [0u16; 64];
            let n = GetClassNameW(h, &mut buf);
            n > 0 && String::from_utf16_lossy(&buf[..n as usize]) == name
        }
    }

}

#[cfg(target_os = "windows")]
pub use imp::{repaint_wallpaper_surfaces_once, Pinner};

#[cfg(not(target_os = "windows"))]
pub struct Pinner;
#[cfg(not(target_os = "windows"))]
impl Pinner {
    pub fn new(_app: tauri::AppHandle, _main_thread_id: std::thread::ThreadId) -> Self {
        Self
    }
    pub fn pin(&self, _: &str, _: isize) {}
    pub fn unpin(&self, _: &str, _: isize) {}
}

#[cfg(not(target_os = "windows"))]
pub fn repaint_wallpaper_surfaces_once() {}
