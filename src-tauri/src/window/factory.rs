use crate::skin::types::{Skin, SkinRuntimeConfig};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

// ── Windows: frameless skin-window helpers ──────────────────────────

/// Frameless style mask for GWL_STYLE — strips everything that gives
/// the window a title bar, border, or resize frame.
#[cfg(target_os = "windows")]
const FRAMELESS_STYLE: isize = {
    use windows::Win32::UI::WindowsAndMessaging::{
        WS_BORDER, WS_CAPTION, WS_DLGFRAME, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU,
        WS_THICKFRAME,
    };
    !(WS_CAPTION.0 as isize
        | WS_THICKFRAME.0 as isize
        | WS_SYSMENU.0 as isize
        | WS_MINIMIZEBOX.0 as isize
        | WS_MAXIMIZEBOX.0 as isize
        | WS_BORDER.0 as isize
        | WS_DLGFRAME.0 as isize)
};

#[cfg(target_os = "windows")]
const FRAMELESS_EXSTYLE: isize = {
    use windows::Win32::UI::WindowsAndMessaging::{
        WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_EX_STATICEDGE, WS_EX_TRANSPARENT,
        WS_EX_WINDOWEDGE,
    };
    !(WS_EX_WINDOWEDGE.0 as isize
        | WS_EX_CLIENTEDGE.0 as isize
        | WS_EX_DLGMODALFRAME.0 as isize
        | WS_EX_STATICEDGE.0 as isize
        | WS_EX_TRANSPARENT.0 as isize)
};

/// FRAMELESS_EXSTYLE 的穿透变体：保留 WS_EX_TRANSPARENT|WS_EX_LAYERED。
/// tao 的 set_ignore_cursor_events 给顶层窗口置这两位（window_state.rs:
/// IGNORE_CURSOR_EVENT => style_ex |= WS_EX_TRANSPARENT|WS_EX_LAYERED），
/// OS 命中测试随即整体跳过该窗口（WebView2 子孙窗口根本不会被命中）。
#[cfg(target_os = "windows")]
const FRAMELESS_EXSTYLE_PASSTHROUGH: isize = {
    use windows::Win32::UI::WindowsAndMessaging::{
        WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_EX_STATICEDGE, WS_EX_WINDOWEDGE,
    };
    !(WS_EX_WINDOWEDGE.0 as isize
        | WS_EX_CLIENTEDGE.0 as isize
        | WS_EX_DLGMODALFRAME.0 as isize
        | WS_EX_STATICEDGE.0 as isize)
};

/// 皮肤 id → 建窗时登记的 HWND。销毁漏斗（close_skin_window_nowait）在
/// window.hwnd() 失败（句柄已死）或窗口对象已不在的兜底臂按它摘除
/// HWND 键的三处登记（pinner/吸附/穿透）——HWND 会被系统回收复用，
/// 残留条目会让无关窗口被误当吸附候选/穿透放行（2026-09 审查 A2）。
static SKIN_HWNDS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, isize>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// 建窗时登记 HWND（create_skin_window 在装子类/吸附/穿透登记的同一块调用）
fn record_skin_hwnd(skin_id: &str, hwnd_val: isize) {
    SKIN_HWNDS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(skin_id.to_string(), hwnd_val);
}

/// 销毁时取出并摘除登记
fn take_skin_hwnd(skin_id: &str) -> Option<isize> {
    SKIN_HWNDS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(skin_id)
}

/// 鼠标穿透登记集：开启穿透的皮肤窗口 HWND。无边框子类（force_frameless /
/// WM_STYLECHANGING / WM_STYLECHANGED）对登记窗口改用 FRAMELESS_EXSTYLE_PASSTHROUGH
/// 保留 TRANSPARENT|LAYERED，其余窗口照旧剥净（LAYERED 平时不在我们的
/// WebView2 渲染假设内）。不保留则穿透位会在下一次清理/5 秒自愈周期被摘回。
#[cfg(target_os = "windows")]
static PASSTHROUGH_HWNDS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<isize>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

/// 登记/注销穿透 HWND。必须先登记再调 set_ignore_cursor_events：tao 经
/// execute_in_thread 在主线程落 SetWindowLongPtr，其触发的 WM_STYLECHANGING
/// 由子类按此刻的登记状态决定放行还是剥掉 TRANSPARENT|LAYERED。
#[cfg(target_os = "windows")]
pub fn set_passthrough_hwnd(hwnd_val: isize, on: bool) {
    let mut set = PASSTHROUGH_HWNDS.lock().unwrap_or_else(|e| e.into_inner());
    if on {
        set.insert(hwnd_val);
    } else {
        set.remove(&hwnd_val);
    }
}

#[cfg(target_os = "windows")]
fn is_passthrough(hwnd_val: isize) -> bool {
    PASSTHROUGH_HWNDS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(&hwnd_val)
}

/// Set per-window DWM attributes + strip frame styles once at creation.
/// Call BEFORE the window is shown, ON THE WINDOW'S OWNER THREAD.
///
/// NOTE: we deliberately do NOT call DwmExtendFrameIntoClientArea(-1).
/// The "-1 sheet of glass" hack extends DWM's frame rendering into the
/// whole client area — on a transparent WebView2 window that makes any
/// DWM-drawn frame (caption buttons included) bleed THROUGH the skin.
/// WebView2 compositing handles transparency; the glass extension only
/// acts as a frame amplifier.
#[cfg(target_os = "windows")]
unsafe fn setup_frameless(hwnd_val: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DWM_WINDOW_CORNER_PREFERENCE, DWMWA_BORDER_COLOR, DWMWA_NCRENDERING_POLICY,
        DWMWA_VISIBLE_FRAME_BORDER_THICKNESS, DWMWA_WINDOW_CORNER_PREFERENCE,
        DwmSetWindowAttribute,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DeleteMenu, GWL_EXSTYLE, GWL_STYLE, GetSystemMenu, GetWindowLongPtrW, HWND_TOP,
        MF_BYCOMMAND, SC_CLOSE, SC_MAXIMIZE, SC_MINIMIZE, SC_MOVE, SC_RESTORE, SC_SIZE,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER,
        SetWindowLongPtrW, SetWindowPos, WS_EX_LAYERED, WS_POPUP,
    };

    let hwnd = HWND(hwnd_val as *mut _);

    // DWM per-window overrides
    let no_color: u32 = 0xFFFFFFFE;
    let _ = DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &no_color as *const _ as _, 4);

    let corner = DWM_WINDOW_CORNER_PREFERENCE(1);
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        &corner as *const _ as _,
        std::mem::size_of_val(&corner) as u32,
    );

    let ncrp: u32 = 1;
    let _ = DwmSetWindowAttribute(hwnd, DWMWA_NCRENDERING_POLICY, &ncrp as *const _ as _, 4);

    let border_thickness: u32 = 0;
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_VISIBLE_FRAME_BORDER_THICKNESS,
        &border_thickness as *const _ as _,
        4,
    );

    // Strip frame styles + add WS_POPUP
    let s = GetWindowLongPtrW(hwnd, GWL_STYLE);
    SetWindowLongPtrW(hwnd, GWL_STYLE, (s & FRAMELESS_STYLE) | WS_POPUP.0 as isize);

    // Strip frame ex-styles + WS_EX_LAYERED (layered compositing breaks
    // WebView2/DirectComposition rendering).  WS_EX_TRANSPARENT is also
    // stripped so the skin receives mouse events normally.
    let e = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    SetWindowLongPtrW(
        hwnd,
        GWL_EXSTYLE,
        e & FRAMELESS_EXSTYLE & !(WS_EX_LAYERED.0 as isize),
    );

    let _ = SetWindowPos(
        hwnd,
        Some(HWND_TOP),
        0,
        0,
        0,
        0,
        SWP_FRAMECHANGED
            | SWP_NOMOVE
            | SWP_NOSIZE
            | SWP_NOACTIVATE
            | SWP_NOZORDER
            | SWP_NOOWNERZORDER,
    );

    // Remove all items from the system menu. Even though frame styles are
    // stripped, a system menu may have been created at window creation and
    // can still appear on right-click; deleting the items makes it harmless.
    let sys_menu = GetSystemMenu(hwnd, false);
    if !sys_menu.is_invalid() {
        for cmd in [
            SC_RESTORE,
            SC_MOVE,
            SC_SIZE,
            SC_MINIMIZE,
            SC_MAXIMIZE,
            SC_CLOSE,
        ] {
            let _ = DeleteMenu(sys_menu, cmd, MF_BYCOMMAND);
        }
    }
}

/// Subclass ID for skin windows.
#[cfg(target_os = "windows")]
const SKIN_SUBCLASS_ID: usize = 0x534B4E;

/// Custom message posted by force_clean_skin_window to defer cleanup
/// to the main thread's message queue.  Using PostMessageW guarantees
/// the cleanup runs AFTER all previously-queued messages (including
/// tao's deferred `apply_diff` which re-sets window styles).
#[cfg(target_os = "windows")]
const WM_DESK_CLEANUP: u32 = 0x8001; // WM_APP + 1

/// Window-subclass proc that keeps the window frameless.
///
/// Two layers of defense:
///
///   Layer 1 — Style hygiene (free)
///     force_frameless() strips frame bits + keeps WS_POPUP.
///     Called on WM_ACTIVATE, WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE,
///     WM_WINDOWPOSCHANGING.  Combined with WM_NCCALCSIZE=0,
///     WM_NCPAINT=0, WM_NCACTIVATE=1, no GDI NC paint can happen.
///
///   Layer 2 — DWM attribute reassertion (cheap, no flush)
///     reassert_dwm() rewrites NCRENDERING_POLICY=DISABLED,
///     BORDER_COLOR=NONE, CORNER_PREFERENCE=DONOTROUND.
///     Only DwmSetWindowAttribute calls — no DwmFlush, no SetWindowPos.
///
/// SetWindowPos(SWP_FRAMECHANGED) is only issued when force_frameless
/// actually changed a style bit.  Unconditional FRAMECHANGED calls (the
/// old behaviour) forced DWM to re-evaluate the window frame on every
/// timer tick / activation — each evaluation was a fresh chance for DWM
/// to composite a frame.  In steady state nothing is dirty, so we stay
/// silent and DWM has nothing to do.
///
/// Together: even if DWM asynchronously resets NCRENDERING_POLICY to
/// ENABLED during focus or drag, the reassertion writes it back before
/// the next frame — and even if a frame slips through, WS_POPUP + zero
/// chrome style bits leave DWM nothing to render.
#[cfg(target_os = "windows")]
unsafe extern "system" fn skin_subclass_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    w_param: windows::Win32::Foundation::WPARAM,
    l_param: windows::Win32::Foundation::LPARAM,
    _uidsubclass: usize,
    _dwrefdata: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::Shell::DefSubclassProc;
    // panic 跨 FFI 边界 unwind 是 UB——拦住并回退默认处理（可能短暂
    // 露出边框，5 秒维护定时器会重新兜底无边框状态）。
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        skin_subclass_proc_inner(hwnd, msg, w_param, l_param)
    }))
    .unwrap_or_else(|_| DefSubclassProc(hwnd, msg, w_param, l_param))
}

/// skin_subclass_proc 的主体，拆出来以便 extern 包装层用 catch_unwind
/// 整体防护（内部有锁与 Vec 分配，理论上可能 panic）。
#[cfg(target_os = "windows")]
unsafe fn skin_subclass_proc_inner(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    w_param: windows::Win32::Foundation::WPARAM,
    l_param: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::Shell::DefSubclassProc;
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GWL_STYLE, GetWindowLongPtrW, HTCLIENT, HWND_TOP, STYLESTRUCT,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER,
        SetWindowLongPtrW, SetWindowPos, WM_ACTIVATE, WM_CONTEXTMENU, WM_DISPLAYCHANGE,
        WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_GETMINMAXINFO, WM_MOVING, WM_NCACTIVATE,
        WM_NCCALCSIZE, WM_NCHITTEST, WM_NCPAINT, WM_NCRBUTTONUP, WM_SETTINGCHANGE, WM_SHOWWINDOW,
        WM_STYLECHANGED, WM_STYLECHANGING, WM_SYSCOMMAND, WM_THEMECHANGED, WM_WINDOWPOSCHANGED,
        WM_WINDOWPOSCHANGING, WS_EX_LAYERED, WS_POPUP,
    };
    // DWM-specific window messages (may live under Graphics::Dwm in some crate versions)
    const WM_DWMCOMPOSITIONCHANGED: u32 = 0x031E;
    const WM_DWMNCRENDERINGCHANGED: u32 = 0x031F;
    const WM_DPICHANGED: u32 = 0x02E0;
    use windows::Win32::Foundation::LRESULT;

    // -- Layer 1: strip style bits (free, no DWM calls) --
    // Returns true if any style bit was actually changed.
    unsafe fn force_frameless(hwnd: windows::Win32::Foundation::HWND) -> bool {
        let mut changed = false;
        let s = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let clean_s = (s & FRAMELESS_STYLE) | WS_POPUP.0 as isize;
        if s != clean_s {
            SetWindowLongPtrW(hwnd, GWL_STYLE, clean_s);
            changed = true;
        }
        let e = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        // 穿透登记窗口保留 TRANSPARENT|LAYERED（见 PASSTHROUGH_HWNDS），
        // 其余窗口照旧剥净。
        let clean_e = if is_passthrough(hwnd.0 as isize) {
            e & FRAMELESS_EXSTYLE_PASSTHROUGH
        } else {
            e & FRAMELESS_EXSTYLE & !(WS_EX_LAYERED.0 as isize)
        };
        if e != clean_e {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, clean_e);
            changed = true;
        }
        changed
    }

    // -- Layer 2: reassert DWM per-window attributes (cheap, no flush) --
    // DwmSetWindowAttribute is a per-window property write (does not block on
    // composition).  No DwmFlush, no SetWindowPos — safe on the hot path.
    // NOTE: no DwmExtendFrameIntoClientArea(-1) here — see setup_frameless.
    unsafe fn reassert_dwm(hwnd: windows::Win32::Foundation::HWND) {
        use windows::Win32::Graphics::Dwm::{
            DWM_WINDOW_CORNER_PREFERENCE, DWMWA_BORDER_COLOR, DWMWA_NCRENDERING_POLICY,
            DWMWA_VISIBLE_FRAME_BORDER_THICKNESS, DWMWA_WINDOW_CORNER_PREFERENCE,
            DwmSetWindowAttribute,
        };

        let no_color: u32 = 0xFFFFFFFE;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &no_color as *const _ as _, 4);

        let corner = DWM_WINDOW_CORNER_PREFERENCE(1);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const _ as _,
            std::mem::size_of_val(&corner) as u32,
        );

        let ncrp: u32 = 1;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_NCRENDERING_POLICY, &ncrp as *const _ as _, 4);

        // Zero visible border thickness so DWM renders no border regardless
        // of NCRENDERING_POLICY state.
        let border_thickness: u32 = 0;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_VISIBLE_FRAME_BORDER_THICKNESS,
            &border_thickness as *const _ as _,
            4,
        );
    }

    // Force a frame recalculation — ONLY call when a style actually changed,
    // otherwise it is just another DWM frame-reevaluation trigger.
    unsafe fn frame_changed(hwnd: windows::Win32::Foundation::HWND) {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOP),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED
                | SWP_NOMOVE
                | SWP_NOSIZE
                | SWP_NOACTIVATE
                | SWP_NOZORDER
                | SWP_NOOWNERZORDER,
        );
    }

    // Shared heavy-cleanup path for focus / DWM / system events:
    // style + DWM reassert before AND after DefSubclassProc, then a
    // frame recalculation ONLY if a style bit actually changed.
    unsafe fn heavy(
        hwnd: windows::Win32::Foundation::HWND,
        msg: u32,
        w_param: windows::Win32::Foundation::WPARAM,
        l_param: windows::Win32::Foundation::LPARAM,
    ) -> LRESULT {
        let mut changed = force_frameless(hwnd);
        reassert_dwm(hwnd);
        let result = DefSubclassProc(hwnd, msg, w_param, l_param);
        reassert_dwm(hwnd);
        changed |= force_frameless(hwnd);
        if changed {
            frame_changed(hwnd);
        }
        result
    }

    match msg {
        // -- Focus change: style + DWM before & after --
        // DWM resets NCRENDERING_POLICY asynchronously on activation.
        WM_ACTIVATE => heavy(hwnd, msg, w_param, l_param),

        // -- Drag begin: style + DWM clean before the modal move loop --
        WM_ENTERSIZEMOVE => {
            force_frameless(hwnd);
            reassert_dwm(hwnd);
            // 重置吸附判定的原始拖动轨迹（raw/last_out）——系统 WM_MOVING
            // 提议是相对上一步写回值的增量，吸附判定必须吃按增量累积的
            // 原始轨迹，每次拖动开头重置（见 snap.rs 模块头注释）。
            crate::window::snap::begin_drag(hwnd.0 as isize);
            DefSubclassProc(hwnd, msg, w_param, l_param)
        }

        // -- Drag step: edge snapping (「窗口」页的边缘吸附开关) --
        // The system modal move loop sends WM_MOVING with the proposed
        // screen-coord rect on every step; on_window_moving rewrites it in
        // place when the window is near the monitor's work-area edges or
        // another skin window (screen edges win).  One hash lookup for
        // windows with snapping disabled — safe on the hot path.
        WM_MOVING => {
            crate::window::snap::on_window_moving(hwnd.0 as isize, l_param.0);
            DefSubclassProc(hwnd, msg, w_param, l_param)
        }

        // -- Drag end: DWM + style clean after the move loop --
        WM_EXITSIZEMOVE => {
            let result = DefSubclassProc(hwnd, msg, w_param, l_param);
            reassert_dwm(hwnd);
            if force_frameless(hwnd) {
                frame_changed(hwnd);
            }
            result
        }

        // -- Before every SetWindowPos: style + DWM clean --
        // Every SetWindowPos is a DWM async-reset trigger (drag, focus, eval()).
        // reassert_dwm is ~4x DwmSetWindowAttribute — property writes, no flush,
        // safe on the hot path.
        WM_WINDOWPOSCHANGING => {
            force_frameless(hwnd);
            reassert_dwm(hwnd);
            DefSubclassProc(hwnd, msg, w_param, l_param)
        }

        // -- Zero non-client area --
        // Returning 0 when wParam is TRUE makes the client area cover the
        // entire window rect — the standard frameless-window answer.
        // This also invalidates any DWM-cached NC dimensions.
        WM_NCCALCSIZE => LRESULT(0),

        // -- Skip NC painting --
        WM_NCPAINT => LRESULT(0),

        // -- Suppress title-bar redraw on focus --
        WM_NCACTIVATE => LRESULT(1),

        // -- Report the whole window as client area.  The skin uses a JS-based
        // drag handler (see skin::protocol::inject_bridge) instead of
        // -webkit-app-region, so we no longer need the OS caption region.
        // This prevents the system context menu from appearing on right-click
        // while letting the WebView show its own context menu.
        //
        // NOTE: border-resize hot zones are NOT implemented here — the
        // WebView2 child window covers the entire client area, so this
        // handler never fires in practice.  Resizing lives in the bridge
        // (pointerdown -> start_skin_resize -> WM_NCLBUTTONDOWN(HT*)).
        WM_NCHITTEST => LRESULT(HTCLIENT as isize),

        // -- Floor the border-drag size so the window can't be shrunk into a
        // few ungrabbable pixels.  Values are physical px (DPI-scaled).
        // Override AFTER DefSubclassProc so the default handling can't
        // overwrite our minimum.
        WM_GETMINMAXINFO => {
            use windows::Win32::UI::HiDpi::GetDpiForWindow;
            use windows::Win32::UI::WindowsAndMessaging::MINMAXINFO;
            let result = DefSubclassProc(hwnd, msg, w_param, l_param);
            let dpi = GetDpiForWindow(hwnd) as i32;
            let mmi = &mut *(l_param.0 as *mut MINMAXINFO);
            mmi.ptMinTrackSize.x = (60 * dpi + 48) / 96;
            mmi.ptMinTrackSize.y = (40 * dpi + 48) / 96;
            result
        }

        // -- Suppress the system menu on right-click in the caption/drag region.
        // Without this, right-clicking a draggable skin shows the window's
        // system menu (restore / move / size / close) instead of the webview's
        // context menu. Returning 0 here prevents that menu while leaving the
        // browser's own context menu unaffected.
        WM_NCRBUTTONUP => LRESULT(0),

        // -- Suppress the default system menu that DefWindowProc would show for
        // a popup window when no context menu handler is present. WebView2 shows
        // its own menu on WM_RBUTTONUP, so we drop this fallback.
        WM_CONTEXTMENU => LRESULT(0),

        // -- Intercept style changes: strip frame bits + block LAYERED/TRANSPARENT --
        // WS_EX_LAYERED is stripped because layered compositing breaks
        // WebView2/DirectComposition rendering.  WS_EX_TRANSPARENT is stripped
        // so the skin remains clickable.  例外：穿透登记窗口（PASSTHROUGH_HWNDS）
        // 保留 TRANSPARENT|LAYERED——tao 的 set_ignore_cursor_events 靠这两位
        // 实现命中测试整体跳过。
        WM_STYLECHANGING => {
            let ss = &mut *(l_param.0 as *mut STYLESTRUCT);
            if w_param.0 as i32 == GWL_STYLE.0 {
                ss.styleNew =
                    ((ss.styleNew as isize & FRAMELESS_STYLE) | WS_POPUP.0 as isize) as u32;
            } else if w_param.0 as i32 == GWL_EXSTYLE.0 {
                if is_passthrough(hwnd.0 as isize) {
                    ss.styleNew = (ss.styleNew as isize & FRAMELESS_EXSTYLE_PASSTHROUGH) as u32;
                } else {
                    ss.styleNew &= !(WS_EX_LAYERED.0);
                    ss.styleNew = (ss.styleNew as isize & FRAMELESS_EXSTYLE) as u32;
                }
            }
            DefSubclassProc(hwnd, msg, w_param, l_param)
        }

        // -- After style change: verify no drift --
        WM_STYLECHANGED => {
            let ss = &mut *(l_param.0 as *mut STYLESTRUCT);
            if w_param.0 as i32 == GWL_STYLE.0 {
                let clean = ((ss.styleNew as isize & FRAMELESS_STYLE) | WS_POPUP.0 as isize) as u32;
                if ss.styleNew != clean {
                    SetWindowLongPtrW(hwnd, GWL_STYLE, clean as isize);
                }
            } else if w_param.0 as i32 == GWL_EXSTYLE.0 {
                let mask = if is_passthrough(hwnd.0 as isize) {
                    FRAMELESS_EXSTYLE_PASSTHROUGH
                } else {
                    // 与 force_frameless / WM_STYLECHANGING 同一口径：非穿透
                    // 窗口连 WS_EX_LAYERED 一起剥（layered 合成会破坏
                    // WebView2/DirectComposition 渲染）
                    FRAMELESS_EXSTYLE & !(WS_EX_LAYERED.0 as isize)
                };
                let clean = (ss.styleNew as isize & mask) as u32;
                if ss.styleNew != clean {
                    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, clean as isize);
                }
            }
            DefSubclassProc(hwnd, msg, w_param, l_param)
        }

        // ── DWM / system messages that can reset per-window attributes ──
        // Sent asynchronously by DWM or the window manager when composition,
        // rendering, theme, display, or DPI state changes.  Each is a trigger
        // for DWM to re-read window styles and recomposite — if our styles
        // are dirty at that moment, a titlebar/border appears.

        // DWM composition toggled (e.g. starting/stopping DWM)
        WM_DWMCOMPOSITIONCHANGED => heavy(hwnd, msg, w_param, l_param),

        // DWM non-client rendering policy changed — DIRECT titlebar trigger
        WM_DWMNCRENDERINGCHANGED => heavy(hwnd, msg, w_param, l_param),

        // Visual theme changed
        WM_THEMECHANGED => heavy(hwnd, msg, w_param, l_param),

        // System setting changed (e.g. accessibility, performance options)
        WM_SETTINGCHANGE => heavy(hwnd, msg, w_param, l_param),

        // Display resolution / color depth changed
        WM_DISPLAYCHANGE => heavy(hwnd, msg, w_param, l_param),

        // DPI changed (moving between monitors with different scaling)
        WM_DPICHANGED => heavy(hwnd, msg, w_param, l_param),

        // ── Show / position / syscommand handlers (light cleanup) ──

        // Window about to be shown — clean before it becomes visible
        WM_SHOWWINDOW => {
            if w_param.0 != 0 {
                // wParam == TRUE: window is being shown
                force_frameless(hwnd);
                reassert_dwm(hwnd);
            }
            DefSubclassProc(hwnd, msg, w_param, l_param)
        }

        // After every SetWindowPos — DWM async may have fired during the
        // position change.  Light cleanup only (no nested SetWindowPos).
        WM_WINDOWPOSCHANGED => {
            let result = DefSubclassProc(hwnd, msg, w_param, l_param);
            force_frameless(hwnd);
            reassert_dwm(hwnd);
            result
        }

        // System commands (SC_RESTORE, SC_MOVE, SC_SIZE etc.) can trigger
        // NC rendering.  Light cleanup after — no nested SetWindowPos.
        WM_SYSCOMMAND => {
            let result = DefSubclassProc(hwnd, msg, w_param, l_param);
            force_frameless(hwnd);
            reassert_dwm(hwnd);
            result
        }

        // ── Deferred cleanup (posted by force_clean_skin_window) ──
        // Runs on the window's owner thread AFTER all previously-queued
        // messages, including tao's deferred `apply_diff` which re-sets
        // window styles.  Frame recalculation only when styles changed.
        WM_DESK_CLEANUP => {
            let changed = force_frameless(hwnd);
            reassert_dwm(hwnd);
            if changed {
                frame_changed(hwnd);
            }
            LRESULT(0)
        }

        _ => DefSubclassProc(hwnd, msg, w_param, l_param),
    }
}

/// Install (or refresh) the frameless subclass on a skin window.
/// Idempotent: same subclass id + proc just updates the reference data.
/// MUST be called on the window's OWNER thread — SetWindowSubclass fails
/// when called from a foreign thread (observed: succeeds on the event-loop
/// thread, fails on spawn_blocking worker threads).
/// Returns true if the subclass is installed.
#[cfg(target_os = "windows")]
pub fn ensure_frameless_subclass(hwnd_val: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::SetWindowSubclass;

    unsafe {
        SetWindowSubclass(
            HWND(hwnd_val as *mut _),
            Some(skin_subclass_proc),
            SKIN_SUBCLASS_ID,
            0,
        )
        .as_bool()
    }
}

/// Run setup_frameless + install the frameless subclass ON THE WINDOW'S
/// OWNER THREAD.  When called from the event-loop thread (auto-load) it
/// runs inline; from a worker thread (load/reload commands) it posts the
/// work to the event loop and waits (bounded) for completion.
/// On failure it only logs — the periodic timer re-installs the subclass,
/// so a transient failure self-heals within a few seconds.
#[cfg(target_os = "windows")]
fn install_frameless(app: &AppHandle, hwnd_val: isize, skin_id: &str) {
    let on_main = app
        .try_state::<crate::AppState>()
        .map(|s| s.main_thread_id == std::thread::current().id())
        .unwrap_or(false);

    if on_main {
        unsafe { setup_frameless(hwnd_val) };
        if !ensure_frameless_subclass(hwnd_val) {
            log::error!(
                "SetWindowSubclass failed for skin '{}' on main thread — timer will retry",
                skin_id
            );
        }
        return;
    }

    let (tx, rx) = std::sync::mpsc::channel();
    if let Err(e) = app.run_on_main_thread(move || {
        unsafe { setup_frameless(hwnd_val) };
        let ok = ensure_frameless_subclass(hwnd_val);
        let _ = tx.send(ok);
    }) {
        log::error!(
            "run_on_main_thread failed for skin '{}': {} — timer will retry",
            skin_id,
            e
        );
        return;
    }
    match rx.recv_timeout(std::time::Duration::from_secs(3)) {
        Ok(true) => {}
        Ok(false) => log::error!(
            "SetWindowSubclass failed for skin '{}' — timer will retry",
            skin_id
        ),
        Err(e) => log::error!(
            "subclass install timed out for skin '{}': {} — timer will retry",
            skin_id,
            e
        ),
    }
}

/// Schedule a deferred cleanup on the skin window's main thread.
///
/// Posts WM_DESK_CLEANUP to the window's message queue via PostMessageW.
/// Because PostMessageW adds to the END of the queue, the cleanup runs
/// AFTER all previously-queued messages — critically, after tao's
/// deferred `apply_diff` which calls SetWindowLongW(GWL_STYLE, WS_CAPTION|...).
///
/// The subclass handles WM_DESK_CLEANUP by doing force_frameless +
/// reassert_dwm + SetWindowPos(SWP_FRAMECHANGED).
#[cfg(target_os = "windows")]
pub fn force_clean_skin_window(window: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

    if let Ok(hwnd) = window.hwnd() {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(hwnd.0 as isize as *mut _)),
                WM_DESK_CLEANUP,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

/// Schedule a deferred cleanup on the skin window via raw HWND.
/// Same semantics as force_clean_skin_window — posts WM_DESK_CLEANUP
/// to the window's message queue via PostMessageW.
/// No-op if the HWND is already dead（通用防御：调用方与窗口销毁存在
/// 竞态时句柄可能已失效，PostMessage 到死句柄没有意义）。
#[cfg(target_os = "windows")]
pub fn force_clean_skin_window_by_hwnd(hwnd_val: isize) {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{IsWindow, PostMessageW};

    unsafe {
        let hwnd = HWND(hwnd_val as *mut _);
        if IsWindow(Some(hwnd)).as_bool() {
            let _ = PostMessageW(Some(hwnd), WM_DESK_CLEANUP, WPARAM(0), LPARAM(0));
        }
    }
}

// ── Public API ──────────────────────────────────────────────────────

/// 布局尺 = 物理客户区宽 ÷ 设计逻辑宽（配置基础尺寸 × 有效 zoom）。
/// 背景：tao 的 scale_factor 在「建窗时 DPI 96、随后被系统改派到 120
/// 但不发 WM_DPICHANGED、不重布局」的路径上虚报——窗口物理尺寸=逻辑
/// 值、缓存却是 1.25；按它强制 rasterization scale 会把内容放大 1.25
/// 倍：右侧/下缘出现无内容覆盖的黑条、整体被裁（2026-08-06 实测）。
/// 正确不变量（wry 源码实证）：controller Bounds = 物理客户区，CSS
/// 视口 = Bounds ÷ rasterization scale——所以 rasterization scale 必须
/// 等于 物理 ÷ 设计逻辑，而不是 tao 的 scale_factor。取不到设计逻辑宽
/// 时回退 scale_factor。
#[cfg(target_os = "windows")]
fn layout_scale_factor(window: &tauri::WebviewWindow) -> f64 {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
    let fallback = window.scale_factor().unwrap_or(1.0);
    let Some(skin_id) = window.label().strip_prefix("skin-").map(|s| s.to_string()) else {
        return fallback;
    };
    let physical_w = match window.hwnd() {
        Ok(hwnd) => {
            let mut rc = RECT::default();
            unsafe {
                let _ = GetClientRect(HWND(hwnd.0 as isize as *mut _), &mut rc);
            }
            rc.right - rc.left
        }
        Err(_) => return fallback,
    };
    if physical_w <= 0 {
        return fallback;
    }
    let app = window.app_handle();
    let state = app.state::<crate::AppState>();
    let entry = {
        let cfg = state.config.lock().unwrap_or_else(|e| e.into_inner());
        cfg.skin_settings.get(&skin_id).map(|e| (e.width, e.zoom))
    };
    let Some((base_w, zoom_cfg)) = entry else {
        return fallback;
    };
    let zoom = match zoom_cfg {
        Some(z) => crate::skin::types::clamp_zoom(z),
        None => {
            // None = 跟随 skin.json 的 window.zoom 默认（与建窗/面板同一有效值规则）
            // TODO: 每次调用都重扫皮肤目录。当前调用频率低（仅建窗期
            // force_webview_rasterization_scale 的 1+3 次/窗），缓存的失效
            // 处理（skin.json 热编辑）不划算；若将来挂到高频路径需先缓存
            let skins = crate::skin::loader::scan_skins_directory(&state.skins_dir);
            match skins.iter().find(|s| s.id == skin_id) {
                Some(s) => crate::skin::types::clamp_zoom(s.manifest.window.zoom),
                None => return fallback,
            }
        }
    };
    let logical_w = base_w as f64 * zoom;
    if logical_w <= 0.0 {
        return fallback;
    }
    // 取整到 0.01：物理尺寸是 逻辑×scale 四舍五入来的（247×1.25=308.75→309），
    // 直接除会带回 0.001 级噪声（309/247=1.25101…）——WebView2 按带噪值
    // 计算 CSS 视口会产生非整数裁剪，右/下缘露出几像素黑条（实测）。
    // 真实 DPI 缩放都在 1/96≈0.0104 的网格上，0.01 取整无损。
    let scale = ((physical_w as f64 / logical_w) * 100.0).round() / 100.0;
    if scale.is_finite() && scale > 0.1 && scale < 10.0 {
        scale
    } else {
        fallback
    }
}

/// Moved/Resized 落盘换算（物理→逻辑）用的有效尺：Windows 上与光栅化
/// 同源的布局尺（物理客户区 ÷ 设计逻辑宽，见 layout_scale_factor）。
/// 不再信 tao 的 scale_factor——虚拟屏改派 DPI 路径上它缓存虚报（窗口
/// 物理尺寸=逻辑值、缓存却报 1.25），按它换算会把落盘的逻辑坐标/尺寸
/// 写小一倍，重载后窗口错位。非 Windows 无此虚报路径，沿用 scale_factor。
fn persistence_scale_factor(window: &tauri::WebviewWindow) -> f64 {
    #[cfg(target_os = "windows")]
    {
        // 优先用建窗快照尺：layout_scale_factor 在 resize 期间分子随事件
        // 同步变、分母是已存旧值，换算恒等旧值导致边框拖拽尺寸永不落盘
        //（unchanged 恒真跳过保存）。快照与尺寸无关，天然稳定。
        if let Some(s) = SCALE_SNAPSHOTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(window.label())
            .copied()
        {
            return s;
        }
        layout_scale_factor(window)
    }
    #[cfg(not(target_os = "windows"))]
    {
        window.scale_factor().unwrap_or(1.0)
    }
}

/// 边框拖拽尺寸落盘的换算尺快照（label → 建窗时锁定的「物理客户区 ÷
/// 逻辑宽」）。与尺寸无关——resize 期间不再依赖已存旧值（那场事故的
/// 根源）。销毁时摘除。
#[cfg(target_os = "windows")]
static SCALE_SNAPSHOTS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, f64>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// 建窗时锁定换算尺快照：此刻物理客户区 = 逻辑宽 × 真实尺（刚按
/// inner_size 建完窗，配对天然正确；且不受虚拟屏 DPI 虚报影响——
/// 量的是刚建出来的真实窗口）。
#[cfg(target_os = "windows")]
fn snapshot_scale_factor(window: &tauri::WebviewWindow, logical_w: f64) {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
    let Ok(hwnd) = window.hwnd() else { return };
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(HWND(hwnd.0 as isize as *mut _), &mut rc);
    }
    let physical_w = (rc.right - rc.left) as f64;
    if physical_w <= 0.0 || logical_w <= 0.0 {
        return;
    }
    let scale = physical_w / logical_w;
    if scale.is_finite() && scale > 0.1 && scale < 10.0 {
        SCALE_SNAPSHOTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(window.label().to_string(), scale);
    }
}

/// 把皮肤窗口 WebView2 的 rasterization scale 强制为窗口的**布局尺**
/// （物理客户区 ÷ 设计逻辑宽，见 layout_scale_factor；虚拟显示器 DPI
/// 不一致修复，见 create_skin_window 注释）。
/// 注意：不要在这里调 `NotifyParentWindowPositionChanged`——实测它会让
/// controller 按父链重算呈现布局，窗口右/下缘露出 6×4px 无内容黑条
///（2026-08-06 对照实验定论，原为壁纸层跨进程父链场景；壁纸层虽已
/// 移除，此结论作为通用防御保留，勿回归）。
#[cfg(target_os = "windows")]
fn force_webview_rasterization_scale(window: &tauri::WebviewWindow) {
    let scale = layout_scale_factor(window);
    let label = window.label().to_string();
    // 诊断：布局尺与 tao scale_factor、系统窗口 DPI 三方对照（虚拟屏
    // 改派 DPI 不发 WM_DPICHANGED 时三者会两两不一致）。
    #[cfg(target_os = "windows")]
    if let Ok(hwnd) = window.hwnd() {
        unsafe {
            use windows::Win32::UI::HiDpi::GetDpiForWindow;
            let dpi = GetDpiForWindow(windows::Win32::Foundation::HWND(hwnd.0 as isize as *mut _));
            log::info!(
                "force_webview_rasterization_scale '{}': scale={} tao_scale={:?} win_dpi={}",
                label,
                scale,
                window.scale_factor().ok(),
                dpi
            );
        }
    }
    let _ = window.with_webview(move |webview| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller3;
        use windows::core::Interface;
        if let Ok(c3) = webview.controller().cast::<ICoreWebView2Controller3>() {
            let _ = c3.SetRasterizationScale(scale);
        }
    });
}

/// 调度一组延迟重设（建窗 0.6s / 1.5s / 3s 后各一次）：亮相、DPI 改派
/// 会让 WebView2 运行时把 rasterization scale 重置回窗口 DPI 值，且时序
/// 晚于建窗期的单次重设；多次幂等重设覆盖这些迟到重置（正常显示器两
/// 尺本就相等，无副作用）。
#[cfg(target_os = "windows")]
pub(crate) fn schedule_rasterization_force(app: &AppHandle, label: &str) {
    let handle = app.clone();
    let label = label.to_string();
    std::thread::spawn(move || {
        for wait in [600u64, 900, 1500] {
            std::thread::sleep(std::time::Duration::from_millis(wait));
            if let Some(w) = handle.get_webview_window(&label) {
                force_webview_rasterization_scale(&w);
            }
        }
    });
}

/// Create a new skin window
pub fn create_skin_window(
    app: &AppHandle,
    skin: &Skin,
    config: &SkinRuntimeConfig,
) -> Result<tauri::WebviewWindow, String> {
    let label = skin_window_label(&skin.id);
    let entry = &skin.manifest.entry;

    // 防御性清理：上次关闭若因窗口在 close() 入队后、CloseRequested 处理前
    // 被外部销毁，INTENTIONAL_CLOSES 会残留本 label 的登记；不清掉的话
    // 新窗的第一次用户 Alt+F4 会被误判为程序化关闭而真关窗。正常重载路径
    // （destroy_skin_window 等 label 释放后才重建）走到这里时登记早已被
    // 消费，remove 是 no-op。
    INTENTIONAL_CLOSES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&label);

    // 1. Build the skin:// URL. The custom protocol handler reads the file
    //    from the skins directory, injects the Tauri bridge, and serves it.
    //    Relative resources (images, css, js) inside the skin folder just work.
    //    URL 首段必须是磁盘文件夹名而非皮肤 id：protocol.rs 按首段拼
    //    skins_dir 下的真实文件路径；文件夹直装时 id 可能是 slugify 派生值
    //    （如中文文件夹名），id ≠ 文件夹名，用 id 必 404。前端预览图
    //    （api.js assetUrl 取路径末两段）同样按文件夹名拼 URL。
    //
    //    例外——网页皮肤（entry 为 http(s) URL）：窗口直接加载站点页面，
    //    不走 skin:// 协议、不注入桥（页面上没有拖动区/右键菜单/命令通道；
    //    远程源的自定义命令另有 tauri 的 remote-origin 守卫拦着）。站点登录态
    //    走 WebView2 用户数据目录的 cookie 罐，天然跨重启持久化。
    let is_web = crate::skin::types::is_url_entry(entry);
    let webview_url = if is_web {
        WebviewUrl::External(
            entry
                .trim()
                .parse::<tauri::Url>()
                .map_err(|e| format!("Invalid entry url '{}': {}", entry, e))?,
        )
    } else {
        let folder_name = skin
            .directory
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("Invalid skin directory: {:?}", skin.directory))?;
        let mut skin_url = tauri::Url::parse("skin://localhost")
            .map_err(|e| format!("Failed to build skin URL: {}", e))?;
        {
            let mut segments = skin_url
                .path_segments_mut()
                .map_err(|_| "Cannot build skin URL path".to_string())?;
            segments.extend(&[folder_name, entry]);
        }
        skin_url
            .query_pairs_mut()
            .append_pair("opacity", &config.opacity.to_string());
        if config.position_locked {
            // Position-lock state is served through the URL -> protocol handler
            // bakes it into the injected bridge (__DESK_PP__.positionLocked).
            // A post-creation eval would race the page load and lose the flag.
            skin_url.query_pairs_mut().append_pair("locked", "1");
        }
        // Effective resizable: user's panel toggle (Some) wins; None follows the
        // skin.json default.  Baked through the URL like the lock flag.
        if config.resizable.unwrap_or(skin.manifest.window.resizable) {
            // Same bake-in path as the lock flag: the protocol handler bakes
            // __DESK_PP__.resizable, which enables the bridge's border-resize
            // hot zones (pointermove cursor + pointerdown -> start_skin_resize).
            skin_url.query_pairs_mut().append_pair("resizable", "1");
        }
        WebviewUrl::CustomProtocol(skin_url)
    };

    let x = config.x.unwrap_or(100);
    let y = config.y.unwrap_or(100);

    // 缩放比例（zoom）：实际窗口 = 基础尺寸 × zoom，内容经 WebView2
    // ZoomFactor 同倍缩放——页面 CSS 视口保持设计尺寸，布局不重排，
    // 任何皮肤无需适配即可整体缩放。
    let zoom = crate::skin::types::clamp_zoom(config.zoom.unwrap_or(skin.manifest.window.zoom));

    // 2. Create window via Tauri (hidden until setup is done)
    // Coordinates are logical pixels, matching the values stored in config.
    let window = WebviewWindowBuilder::new(app, &label, webview_url)
        // 窗口标题（任务管理器/Alt+Tab 可见性有限的无框工具窗）：取当前默认
        // 语言的解析名，单语言皮肤回退后的名字也能正确出现
        .title(skin.manifest.display_name(crate::i18n::DEFAULT_LANG))
        // 持久化配置的宽高同钳 [1,10000]——config.json 可手改，钳制不能只
        // 拦 manifest 默认值一侧（复审 B-F5：巨型表面吃 GPU 内存/建窗失败）
        .inner_size(
            config.width.clamp(1, crate::skin::types::MAX_DIMENSION) as f64 * zoom,
            config.height.clamp(1, crate::skin::types::MAX_DIMENSION) as f64 * zoom,
        )
        .position(x as f64, y as f64)
        .decorations(false)
        .transparent(skin.manifest.window.transparent)
        .shadow(false)
        .always_on_top(config.always_on_top)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false);
    // 网页皮肤：桥不注入，初始不透明度经初始化脚本落（纯 ASCII）；本地皮肤
    // 的 opacity 已随协议 query 烘焙。脚本同时落地入场淡入（本地皮肤由桥
    // CSS 烘焙 deskFadeIn 关键帧，见 protocol.rs bridge_css）。
    let window = if is_web {
        window
            .initialization_script(web_fade_in_script(
                config.opacity.clamp(crate::skin::types::MIN_OPACITY, 1.0),
            ))
            .build()
            .map_err(|e| format!("Failed to create window: {}", e))?
    } else {
        window
            .build()
            .map_err(|e| format!("Failed to create window: {}", e))?
    };

    // 远程控制软件的虚拟显示器 DPI 上报不一致（GameViewer/向日葵等虚拟屏：
    // GetDpiForMonitor 报 96、系统 DPI 实为 120——GameViewer 虚拟屏 +
    // Win10 21H2 @125% 实测定论）：皮肤窗口建窗后被系统改派到系统 DPI，
    // WebView2 按新窗口 DPI 重设 rasterization scale(×1.25)，而 tao 按建窗
    // DPI 布局（窗口物理尺寸=逻辑值）——内容按「窗口矩形×1.25」错位合成：
    // 圆角/透明区没有内容覆盖透黑底，错位部分还在桌面上留黑色残块。
    // 对策 = 把 controller 的 rasterization scale 强制回窗口的**布局尺**
    // （物理客户区 ÷ 设计逻辑宽，见 layout_scale_factor——tao 的
    // scale_factor 在该路径上虚报，不能作准），两尺一致后视觉树与宿主
    // 矩形逐像素对齐；正常显示器上两者本就相等（幂等无副作用）。建窗时
    // 设一次，亮相（可能触发 DPI 改派、运行时会重设）后多次补设。
    #[cfg(target_os = "windows")]
    {
        force_webview_rasterization_scale(&window);
        schedule_rasterization_force(app, &label);
    }

    // 内容缩放与窗口尺寸同步（窗口尚隐藏，无 100% 闪现）。失败仅降级为
    // 「尺寸对、内容不缩放」（非 Windows 平台兜底），不影响建窗。
    if let Err(e) = window.set_zoom(zoom) {
        log::warn!("set_zoom({}) failed for '{}': {}", zoom, skin.id, e);
    }

    // 建窗即锁换算尺快照（Moved/Resized 落盘换算用；此时物理客户区 =
    // 逻辑宽 × 真实尺，配对天然正确）
    #[cfg(target_os = "windows")]
    snapshot_scale_factor(&window, config.width as f64 * zoom);

    // 3. Strip DWM chrome + install the frameless subclass (while hidden).
    //    MUST run on the window's OWNER thread: SetWindowSubclass fails
    //    silently when called from a foreign thread (load/reload commands
    //    run on spawn_blocking workers).  Without the subclass, tao's
    //    WS_CAPTION style stays and DWM draws the classic frame — this
    //    was the root cause of the intermittent title-bar bug.
    #[cfg(target_os = "windows")]
    if let Ok(hwnd) = window.hwnd() {
        // 销毁兜底用的 HWND 登记（close_skin_window_nowait 兜底臂按它
        // 摘三处 HWND 键登记）
        record_skin_hwnd(&skin.id, hwnd.0 as isize);
        install_frameless(app, hwnd.0 as isize, &skin.id);
        // 边缘吸附状态登记（子类 WM_MOVING 按 HWND 查询）；None = 跟随 manifest
        crate::window::snap::upsert(
            hwnd.0 as isize,
            config.edge_snap.unwrap_or(skin.manifest.window.edge_snap),
            config.snap_gap.unwrap_or(skin.manifest.window.snap_gap),
        );
        // 鼠标穿透：先登记 HWND（子类随即对 TRANSPARENT|LAYERED 放行），再让
        // tao 置位——顺序反了位会被子类摘回。失效仅降级为「不穿透」，不阻断建窗。
        if config.click_through {
            set_passthrough_hwnd(hwnd.0 as isize, true);
            if let Err(e) = window.set_ignore_cursor_events(true) {
                log::warn!("set_ignore_cursor_events failed for '{}': {}", skin.id, e);
            }
        }
    }

    // 4. Show, then schedule one deferred cleanup (runs on the owner thread
    //    after tao's deferred apply_diff).
    let _ = window.show();
    #[cfg(target_os = "windows")]
    force_clean_skin_window(&window);

    // WebView2 hardening: disable the default browser context menu
    // (right-click shows our own native menu instead — see
    // track_skin_popup_menu) and the browser accelerator keys (F5 refresh
    // family — a skin page must not be reloadable by keystroke).  WebView2
    // initializes asynchronously and is usually NOT ready here, so this
    // retries briefly in the background; the 5s maintenance timer in
    // lib.rs re-applies both afterwards (also covers WebView2 restarts).
    #[cfg(target_os = "windows")]
    spawn_webview_hardening_retry(app, &label);

    // 网页皮肤自动刷新（window.refresh_seconds）：普通线程按间隔 location.reload()
    // ——登录态在 cookie 罐里，重载不掉线。闭包持有**建窗时的窗口句柄**：
    // 皮肤卸载/重载后旧窗销毁，eval 返回 Err 自然退出——按 sid 从注册表重查
    // 会命中重载后的新窗，旧线程叠加存活（每 reload 多一个线程，页面每周
    // 期被刷 N 次），那是一条泄漏，勿改回去。
    if is_web {
        if let Some(secs) = skin.manifest.window.refresh_seconds.filter(|s| *s > 0) {
            let win = window.clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(secs as u64));
                    if win.eval("location.reload()").is_err() {
                        break;
                    }
                }
            });
        }
    }

    // 5. Listen for move events
    // WindowEvent::Moved reports physical (screen) pixels, but the rest of
    // the app stores and applies logical pixels (matching WebviewWindowBuilder::position).
    // Position persistence lives HERE in the backend (debounced disk save) —
    // not in the manager frontend — so a drag is saved even when the skin's
    // config panel is not open.  The emitted event only refreshes the panel's
    // X/Y inputs.
    {
        let app_handle = app.clone();
        let sid = skin.id.clone();
        let label_for_event = label.clone();
        // 缩放比例（zoom）有效值的兜底：配置条目缺失时回退 skin.json 的默认
        let manifest_zoom = crate::skin::types::clamp_zoom(skin.manifest.window.zoom);
        window.on_window_event(move |event| {
            match event {
                tauri::WindowEvent::Moved(position) => {
                    // 换算尺取不到时跳过本次落盘（unwrap_or(1.0) 会在非 96
                    // DPI 屏把物理坐标当逻辑写错——下次移动自会补写）
                    let Some(scale_factor) = app_handle
                        .get_webview_window(&label_for_event)
                        .map(|w| persistence_scale_factor(&w))
                    else {
                        return;
                    };
                    let x = (position.x as f64 / scale_factor).round() as i32;
                    let y = (position.y as f64 / scale_factor).round() as i32;
                    save_dragged_position(&app_handle, &sid, x, y);
                    let _ = app_handle.emit_to(
                        "main",
                        "skin-moved",
                        serde_json::json!({
                            "skinId": sid,
                            "x": x,
                            "y": y,
                        }),
                    );
                }
                // Border-drag resize (window.resizable): persist like Moved —
                // in-memory config immediately, debounced disk flush — and
                // refresh the panel's W/H inputs.  Panel-driven set_skin_size
                // echoes back with identical values and is skipped.
                tauri::WindowEvent::Resized(size) => {
                    // 换算尺取不到时跳过本次落盘（unwrap_or(1.0) 会在非 96
                    // DPI 屏把物理尺寸当逻辑写错）
                    let Some(scale_factor) = app_handle
                        .get_webview_window(&label_for_event)
                        .map(|w| persistence_scale_factor(&w))
                    else {
                        return;
                    };
                    let w = (size.width as f64 / scale_factor).round() as u32;
                    let h = (size.height as f64 / scale_factor).round() as u32;
                    // 缩放比例（zoom）：配置持久化的始终是 100% 基础尺寸 =
                    // 实际尺寸 ÷ 有效 zoom（有效 zoom 从配置现读，条目缺失
                    // 回退 manifest）；而发给面板的是实际尺寸——面板的宽高
                    // 输入框始终显示当前实际窗口大小。
                    let (base_w, base_h, unchanged) = {
                        let state = app_handle.state::<crate::AppState>();
                        let cfg = state.config.lock().unwrap_or_else(|e| e.into_inner());
                        let entry = cfg.skin_settings.get(&sid);
                        // clamp：手改 config.json 可能注入越界 zoom
                        let z = crate::skin::types::clamp_zoom(
                            entry.and_then(|e| e.zoom).unwrap_or(manifest_zoom),
                        );
                        let bw = ((w as f64) / z).round() as u32;
                        let bh = ((h as f64) / z).round() as u32;
                        let same = entry
                            .map(|e| e.width == bw && e.height == bh)
                            .unwrap_or(false);
                        (bw, bh, same)
                    };
                    if !unchanged {
                        save_dragged_size(&app_handle, &sid, base_w, base_h);
                        let _ = app_handle.emit_to(
                            "main",
                            "skin-resized",
                            serde_json::json!({
                                "skinId": sid,
                                "width": w,
                                "height": h,
                            }),
                        );
                    }
                }
                // Alt+F4 / 系统关闭请求：皮肤窗不实现「关闭」语义——窗口生命
                // 周期归管理器（加载/刷新/卸载全由管理器负责），按键关闭
                // 降级为隐藏，由全局快捷键 / 托盘勾选项唤回。系统的强制
                // 关闭不可违背，但 WM_CLOSE 只是「请求」，应用层有权把它
                // 解释为隐藏。程序化关闭（卸载/重载/退出，全部经
                // close_skin_window_nowait 登记 label）照常放行。
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    let intentional = INTENTIONAL_CLOSES
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&label_for_event);
                    if !intentional {
                        api.prevent_close();
                        // 降级隐藏（走出场淡出；落地后漏斗自同步托盘/徽标）
                        hide_skin_window_fade(&app_handle, &sid);
                    }
                }
                _ => {}
            }
        });
    }

    // 6. Apply runtime settings
    if config.on_desktop {
        if let Ok(hwnd) = window.hwnd() {
            app.state::<crate::AppState>()
                .pinner
                .pin(&skin.id, hwnd.0 as isize);
        }
    }

    // NOTE: position lock is applied at serve time (locked=1 URL query ->
    // injected bridge).  Do NOT re-add an eval here: it races the page load
    // and used to drop the lock flag on every window recreation.

    log::debug!("Skin window created: {}", skin.id);
    Ok(window)
}

pub fn skin_window_label(skin_id: &str) -> String {
    format!("skin-{}", skin_id)
}

/// 皮肤完全出屏时的目标归位坐标（主显示器工作区左上 + 24px 边距）。
/// 仅当窗口矩形与**所有**显示器工作区都不相交时返回 Some——部分出屏
/// 是合法摆放（多屏拼接缝、刻意半掩），一律不动。坐标系统一物理像素
/// （outer_position/outer_size 与 Monitor::work_area 同为物理）。
#[cfg(target_os = "windows")]
pub(crate) fn offscreen_target(window: &tauri::WebviewWindow) -> Option<(i32, i32)> {
    // 用被判定皮肤窗自身枚举显示器——不要借管理器窗：管理器是「关窗即
    // 销毁」、常态不存在，借它做探针会让 5 秒救回环在管理器关闭期间
    // 整体空转（2026-09 审查 A2；隐藏的皮肤窗同样可枚举显示器）。
    let monitors = window.available_monitors().ok()?;
    if monitors.is_empty() {
        return None;
    }
    let (Ok(pos), Ok(size)) = (window.outer_position(), window.outer_size()) else {
        return None;
    };
    let (l, t) = (pos.x, pos.y);
    let (r, b) = (l + size.width as i32, t + size.height as i32);
    let all_outside = monitors.iter().all(|m| {
        let wa = m.work_area();
        let (ml, mt) = (wa.position.x, wa.position.y);
        let (mr, mb) = (ml + wa.size.width as i32, mt + wa.size.height as i32);
        r <= ml || l >= mr || b <= mt || t >= mb
    });
    if !all_outside {
        return None;
    }
    let primary = window.primary_monitor().ok().flatten()?;
    let pa = primary.work_area();
    Some((pa.position.x + 24, pa.position.y + 24))
}

/// 把所有完全出屏的皮肤拉回主显示器工作区边缘（拔外接屏 / DPI 拓扑
/// 变更后皮肤落在不可见区域，用户拖不回也删不到）。挂点 = lib.rs 的
/// 5 秒维护定时器（拓扑变化最迟 5 秒自愈，启动自载亦被首 tick 覆盖）。
/// 移动走 set_position → Moved 事件 → 后端既有防抖落盘，新位置即持久化。
#[cfg(target_os = "windows")]
pub fn rescue_offscreen_skins(app: &AppHandle) {
    let state = app.state::<crate::AppState>();
    for id in state.registry.loaded_ids() {
        if let Some(window) = state.registry.get(&id) {
            if let Some((x, y)) = offscreen_target(&window) {
                let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
                log::info!("Rescued off-screen skin '{}': moved to ({}, {})", id, x, y);
            }
        }
    }
}

/// Labels whose in-flight close() is PROGRAMMATIC (unload / reload / exit).
/// Alt+F4 delivers the same WM_CLOSE as close() does and CloseRequested
/// carries no reason — this set is the discriminator: close_skin_window_nowait
/// registers the label right before calling close(), the window's
/// CloseRequested handler consumes it; a user keystroke finds no entry and
/// is downgraded to hide.
static INTENTIONAL_CLOSES: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashSet<String>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

pub fn destroy_skin_window(app: &AppHandle, label: &str) -> Result<(), String> {
    close_skin_window_nowait(app, label)?;
    if app.get_webview_window(label).is_some() {
        // close() returns before the webview is fully torn down on the event
        // loop — the label stays taken for a few more ms.  Reload recreates a
        // window with the SAME label immediately, and without waiting it fails
        // with "already exists" (right-click 刷新皮肤 reliably hit this).
        for _ in 0..100 {
            if app.get_webview_window(label).is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if app.get_webview_window(label).is_some() {
            // OS 模态循环（拖拽中并发卸载）饿死 2s 等待时不得放行——紧随的
            // 重建会以 "already exists" 失败且更难排查，这里直接报错
            return Err(format!(
                "destroy_skin_window: label '{}' still taken after 2s (modal loop starvation?)",
                label
            ));
        }
    }
    Ok(())
}

/// Registry cleanup + close(), WITHOUT waiting for the webview teardown.
/// Used on the exit path: there is no same-label recreation there, so the
/// per-window wait destroy_skin_window does is pure latency (sequential, on
/// the main thread, one wait per loaded skin).
pub fn close_skin_window_nowait(app: &AppHandle, label: &str) -> Result<(), String> {
    // 销毁点必递增显隐代际（勿只停在 fade_out_for_destroy 的起点递增——
    // 淡出等待期间窗口仍可见，Alt+F4/skin_hide 等隐藏入口会再 bump 并
    // 布防 gen-current 定时器；此后销毁/重建不再递增的话，该定时器会以
    // 「current 代数」落到同 label 新窗上误 hide（2026-09 审查 A2）。
    // 递增非清零，无 ABA（见 FADE_GENS 头注）。
    bump_fade_gen(label);
    #[cfg(target_os = "windows")]
    SCALE_SNAPSHOTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(label);
    // 拖动落盘防抖的簿记条目一并回收（此前按皮肤永不回收——复审 B-F6）；
    // 在途定时器把「条目被清」按可写盘处理（见 debounced_config_flush）
    if let Some(skin_id) = label.strip_prefix("skin-") {
        DRAG_SAVE_STATE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(skin_id);
    }
    if let Some(skin_id) = label.strip_prefix("skin-") {
        // 建窗登记的 HWND 优先，window.hwnd() 现取兜底——hwnd() 失败
        //（句柄已死）或窗口对象已不在时，仍能按登记值把 HWND 键的三处
        // 登记（pinner/吸附/穿透）摘干净：吸附登记在建窗时无条件 upsert、
        // 穿透按配置登记，两条兜底臂此前只查 pinner 簿记——从未 pin 的
        // 皮肤 stored=None 整体漏摘，HWND 被系统回收复用后无关进程的
        // 窗口会被误当吸附候选（2026-09 审查 A2）。
        let recorded = take_skin_hwnd(skin_id);
        let hwnd_val = app
            .get_webview_window(label)
            .and_then(|w| w.hwnd().ok().map(|h| h.0 as isize))
            .or(recorded);
        let state = app.state::<crate::AppState>();
        match hwnd_val {
            Some(h) => {
                state.pinner.unpin(skin_id, h);
                crate::window::snap::unregister(h);
                #[cfg(target_os = "windows")]
                set_passthrough_hwnd(h, false);
            }
            None => {
                // 建窗 hwnd() 就失败的极端路径：无登记值可用，至少摘
                // pinner 条目（unpin(0) 只清簿记不动窗口位）
                state.pinner.unpin(skin_id, 0);
            }
        }
    }
    if let Some(window) = app.get_webview_window(label) {
        // No RemoveWindowSubclass: it must run on the owner thread (this
        // usually runs on a worker) and is unnecessary anyway — comctl32
        // destroys the subclass automatically when the window is destroyed.
        #[cfg(target_os = "windows")]
        let hwnd_dead = {
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::WindowsAndMessaging::IsWindow;
            window
                .hwnd()
                .map(|h| !unsafe { IsWindow(Some(HWND(h.0 as *mut _))) }.as_bool())
                .unwrap_or(true)
        };
        #[cfg(not(target_os = "windows"))]
        let hwnd_dead = false;
        if hwnd_dead {
            // hwnd 已死时的兜底路径（原为壁纸层皮肤随 explorer 重启被连坐
            // 销毁的场景；壁纸层已移除，该场景不复存在，此分支保留为通用
            // 防御）：close() 的 label 释放链依赖永远等不到的 tao Destroyed
            // 事件（窗口管理器不给跨进程子窗投递 WM_DESTROY，tao 的
            // Window::drop 也只是 PostMessage 到死句柄）——改走 destroy()：
            // vendored tauri-runtime-wry 的补丁（vendor/tauri-runtime-wry
            // 的 NOTE(driftlet)）发现死句柄会补发 Destroyed 流程，释放
            // 全部注册表条目，label 得以复用。
            window
                .destroy()
                .map_err(|e| format!("Failed to destroy window: {}", e))?;
        } else {
            // 登记程序化关闭：窗口的 CloseRequested 处理据此放行本次关闭；
            // 未登记的用户关闭（Alt+F4）会被拦截降级为隐藏。close() 失败
            // 则撤销登记，残留条目会错误放行同一 label 的下一次用户关闭。
            INTENTIONAL_CLOSES
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(label.to_string());
            if let Err(e) = window.close() {
                INTENTIONAL_CLOSES
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(label);
                return Err(format!("Failed to close window: {}", e));
            }
        }
    }
    Ok(())
}

// ── 皮肤入场/出场（淡入淡出） ─────────────────────────────────────────
//
// 淡入淡出走**页面内容透明度**（documentElement 的 CSS opacity），与皮肤
// 「不透明度」设置同一渲染通道。窗口级透明度（WS_EX_LAYERED +
// SetLayeredWindowAttributes）不在选项内：layered 合成会破坏
// WebView2/DirectComposition 渲染（本文件 FRAMELESS_EXSTYLE 的既有口径）。
//
// 三个落地面：
//   1. 建窗入场：不在此模块——建窗时页面尚未加载，eval 会丢。本地皮肤由
//      桥 CSS 烘焙纯 CSS 关键帧（protocol.rs bridge_css 的 deskFadeIn，
//      不依赖 JS，桥被 CSP 拦下也照常显现）；网页皮肤由
//      initialization_script 落地（web_fade_in_script）。
//   2. 运行时再现（show）：show_skin_window_fade —— show() 后 eval 淡入
//     （隐藏态的内联透明度恒为 0：所有隐藏路径都必经淡出）。
//   3. 出场：隐藏走 hide_skin_window_fade（eval 淡出 + 延迟落地 hide，
//      代际守卫防打断）；销毁走 fade_out_for_destroy（阻塞等动画走完，
//      调用方随后销毁——reload 按同 label 重建，不能异步放手）。

// 出场/入场时长锚点在 skin::types（FADE_OUT_MS/FADE_IN_MS）——销毁前的
// 阻塞等待必须覆盖动画时长，四处共用同一常量（桥 CSS 烘焙、运行时
// eval、网页皮肤 init 脚本、本模块等待），字面量散落会改一处漏其余。

/// 显隐代际登记（label → 代数）：每次显隐/销毁操作递增。淡出的延迟落地
/// 只在代数未变时执行——淡出期间被唤回（show）或销毁重建（reload 按同
/// label 出新窗）时，迟到的 hide 不得再落（无守卫会把新窗误隐藏）。
/// 条目**永不摘除、代数单调递增**（同 label 跨窗世代延续）。代际递增有
/// 两个锚点，缺一即有洞（2026-09 审查 A2）：fade_out_for_destroy 在淡出
/// 起点递增作废「淡出开始前布防」的旧定时器；close_skin_window_nowait 在
/// 销毁点再递增作废「淡出等待期间布防」的定时器（彼时窗口仍可见，
/// Alt+F4/skin_hide 等入口会再 bump 并布防 gen-current 定时器）。
/// 若建窗时清零重数，旧定时器的代数会与重置后的新代数撞车（ABA），
/// 迟到 hide 又会误落新窗。
static FADE_GENS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, u64>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn bump_fade_gen(label: &str) -> u64 {
    let mut map = FADE_GENS.lock().unwrap_or_else(|e| e.into_inner());
    let g = map.entry(label.to_string()).or_insert(0);
    *g += 1;
    *g
}

/// 代数未变（条目只增不删，见 FADE_GENS 头注——单调递增杜绝 ABA 撞车）
fn fade_gen_current(label: &str, gen: u64) -> bool {
    FADE_GENS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(label)
        .copied()
        == Some(gen)
}

/// 出场淡出 JS：直落 documentElement 内联样式——无桥页面（网页皮肤不
/// 注入桥）同样生效。transition 常年保留（后续不透明度调整随之平滑），
/// prefers-reduced-motion 落定最终值不播动画。出场也用 ease-out（起步
/// 即响应用户动作；ease-in 的慢起步会让「隐藏」显得迟钝）。
fn fade_out_js() -> String {
    let ms = crate::skin::types::FADE_OUT_MS;
    format!(
        "(function(){{var el=document.documentElement;if(!el)return;try{{if(window.matchMedia&&window.matchMedia('(prefers-reduced-motion: reduce)').matches){{el.style.opacity='0';return;}}}}catch(e){{}}el.style.transition='opacity {ms}ms ease-out';el.style.opacity='0';}})();"
    )
}

/// 入场淡入 JS：target = 皮肤当前有效不透明度（Rust 侧随配置现算，
/// 与建窗烘焙同一口径）。
fn fade_in_js(target: f64) -> String {
    let ms = crate::skin::types::FADE_IN_MS;
    format!(
        "(function(){{var el=document.documentElement;if(!el)return;var t='{target}';try{{if(window.matchMedia&&window.matchMedia('(prefers-reduced-motion: reduce)').matches){{el.style.transition='';el.style.opacity=t;return;}}}}catch(e){{}}el.style.transition='opacity {ms}ms ease-out';el.style.opacity=t;}})();"
    )
}

/// 网页皮肤的入场淡入初始化脚本（远程页面不注入桥，无 CSS 可烘焙）。
/// documentElement 在 init 脚本执行点已存在（既有 opacity 落地同前提）；
/// 每次导航/刷新都会重播（网页皮肤的「入场」= 页面出现）。
fn web_fade_in_script(target: f64) -> String {
    let ms = crate::skin::types::FADE_IN_MS;
    format!(
        "(function(){{var el=document.documentElement;if(el)el.style.opacity='0';function run(){{var d=document.documentElement;if(!d)return;var t='{target}';try{{if(window.matchMedia&&window.matchMedia('(prefers-reduced-motion: reduce)').matches){{d.style.opacity=t;return;}}}}catch(e){{}}d.style.transition='opacity {ms}ms ease-out';d.style.opacity=t;}}if(document.readyState==='loading'){{document.addEventListener('DOMContentLoaded',function(){{requestAnimationFrame(run);}});}}else{{requestAnimationFrame(run);}}}})();"
    )
}

/// 皮肤当前有效不透明度（运行时再现的淡入目标）：skin_settings 条目优先
///（load 路径必播种），缺失回退 manifest 默认——与建窗烘焙（URL query
/// 的 opacity 参数）同一取值口径。
fn effective_opacity(app: &AppHandle, skin_id: &str) -> f64 {
    let state = app.state::<crate::AppState>();
    {
        let cfg = state.config.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = cfg.skin_settings.get(skin_id) {
            return entry.opacity.clamp(crate::skin::types::MIN_OPACITY, 1.0);
        }
    }
    crate::skin::loader::scan_skins_directory(&state.skins_dir)
        .into_iter()
        .find(|s| s.id == skin_id)
        .map(|s| {
            s.manifest
                .window
                .opacity
                .clamp(crate::skin::types::MIN_OPACITY, 1.0)
        })
        .unwrap_or(1.0)
}

/// 销毁前淡出（unload/reload 共用）：窗口可见时播出场动画并阻塞等它走完
///（FADE_OUT_MS），随后调用方销毁窗口。**调用方必须在阻塞线程跑**
///（内部 sleep；load/unload 的 spawn_blocking 即此约定）。窗口本不可见
/// 或已不在时立即返回（无动画可播）。代际递增无条件执行——即使跳过
/// 动画，也要作废在途的隐藏淡出定时器（防迟到的 hide 落在重建新窗上）。
pub fn fade_out_for_destroy(app: &AppHandle, skin_id: &str) {
    let label = skin_window_label(skin_id);
    bump_fade_gen(&label);
    let Some(window) = app.get_webview_window(&label) else {
        return;
    };
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    let _ = window.eval(fade_out_js());
    std::thread::sleep(std::time::Duration::from_millis(
        crate::skin::types::FADE_OUT_MS,
    ));
}

/// 隐藏（带出场淡出）：可见时先 eval 淡出，动画走完再落地 hide
///（独立线程延迟 + 代际守卫——淡出期间被 show/销毁打断则放弃本次
/// 隐藏）。落地后过 sync_tray_toggle_item 漏斗刷新托盘勾选/管理器徽标
///（调用方紧随的同步读到的是淡出中的真实态，落地这次才把终态对齐）。
/// 已隐藏/未加载的窗口直接 no-op。
pub fn hide_skin_window_fade(app: &AppHandle, skin_id: &str) {
    let label = skin_window_label(skin_id);
    let gen = bump_fade_gen(&label);
    let Some(window) = app.get_webview_window(&label) else {
        return;
    };
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    let _ = window.eval(fade_out_js());
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(
            crate::skin::types::FADE_OUT_MS,
        ));
        if !fade_gen_current(&label, gen) {
            return; // 淡出期间被唤回/销毁——迟到的 hide 不落
        }
        if let Some(w) = handle.get_webview_window(&label) {
            if let Err(e) = w.hide() {
                log::warn!("fade-out hide landing failed for '{}': {}", label, e);
            }
        }
        crate::hotkey::sync_tray_toggle_item(&handle);
    });
}

/// 显示（带入场淡入）：show() 后 eval 淡入到当前有效不透明度。隐藏态的
/// 内联透明度恒为 0（隐藏必经淡出），故再现恒从 0 起步；对可见窗口重复
/// 调用 = 目标值不变的 eval，无视觉变化。只显示不抢焦点（沿用既有语义）。
pub fn show_skin_window_fade(app: &AppHandle, skin_id: &str) {
    let label = skin_window_label(skin_id);
    bump_fade_gen(&label); // 作废在途淡出（迟到的 hide 不落）
    let Some(window) = app.get_webview_window(&label) else {
        return;
    };
    let _ = window.show();
    let target = effective_opacity(app, skin_id);
    let _ = window.eval(fade_in_js(target));
}

// ── Drag persistence (position & size) ──────────────────────────────

/// Debounce state for drag disk saves, keyed by skin id.
/// Moved/Resized events fire continuously during a drag; the config file is
/// only written once the values have been stable for 500ms.
static DRAG_SAVE_STATE: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, DragSaveState>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

#[derive(Default, Clone, Copy)]
struct DragSaveState {
    generation: u64,
    timer_running: bool,
}

/// Debounced disk flush shared by position and size saves: at most one
/// timer thread per skin; the generation counter makes it keep waiting
/// while drag events keep coming.
fn debounced_config_flush(app: &AppHandle, skin_id: &str) {
    let spawn_timer = {
        let mut map = DRAG_SAVE_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let st = map.entry(skin_id.to_string()).or_default();
        st.generation += 1;
        if st.timer_running {
            false
        } else {
            st.timer_running = true;
            true
        }
    };
    if !spawn_timer {
        return;
    }

    let handle = app.clone();
    let sid = skin_id.to_string();
    std::thread::spawn(move || {
        let mut seen = 0u64;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let up_to_date = {
                let map = DRAG_SAVE_STATE.lock().unwrap_or_else(|e| e.into_inner());
                let st = map.get(&sid).copied();
                match st {
                    Some(s) if s.generation == seen => true,
                    Some(s) => {
                        seen = s.generation;
                        false
                    }
                    None => true, // 条目被清（皮肤卸载）——按可写盘处理
                }
            };
            if !up_to_date {
                continue;
            }
            // Holding the config lock during the write serializes this save
            // against every other save_config call site (they all save while
            // holding the same lock), so the temp-file rename cannot race.
            let state = handle.state::<crate::AppState>();
            let app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
            if let Err(e) = crate::skin::config::save_config(&state.config_dir, &app_config) {
                log::warn!("Failed to save dragged state for '{}': {}", sid, e);
            }
            drop(app_config);
            // timer_running 必须在写盘之后才置 false——先置会让退出的
            // flush_pending_drag_saves 判定「无 pending」而跳过，末次落盘
            // 恰好被 exit(0) 抢走（窄竞态）
            {
                let mut map = DRAG_SAVE_STATE.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(st) = map.get_mut(&sid) {
                    st.timer_running = false;
                }
            }
            break;
        }
    });
}

/// 退出兜底：拖动防抖的末次落盘可能还在等定时器（约 0.5–1s），进程退出会
/// 丢掉它——有 pending 定时器时同步落一次盘。内存配置在每次拖动事件时已
/// 即时更新，这里只补文件写入；与定时器线程的写入经同一把 config 锁串行
/// （内容相同，temp+rename 幂等）；无 pending 时零开销。
pub fn flush_pending_drag_saves(app: &AppHandle) {
    let any_pending = {
        let map = DRAG_SAVE_STATE.lock().unwrap_or_else(|e| e.into_inner());
        map.values().any(|st| st.timer_running)
    };
    if !any_pending {
        return;
    }
    let state = app.state::<crate::AppState>();
    let app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = crate::skin::config::save_config(&state.config_dir, &app_config) {
        log::warn!("Failed to flush pending drag saves on exit: {}", e);
    }
}

/// Persist a dragged position (logical pixels).
///
/// The in-memory config is updated immediately — reload reads config from
/// memory, so a reload right after a drag keeps the new position even
/// before the debounced disk flush runs.  This lives in the backend
/// (previously in skin-editor.js) so dragging saves regardless of whether
/// the manager's config panel is open for this skin.
fn save_dragged_position(app: &AppHandle, skin_id: &str, x: i32, y: i32) {
    {
        let state = app.state::<crate::AppState>();
        let mut app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
        let entry = app_config
            .skin_settings
            .entry(skin_id.to_string())
            .or_default();
        entry.x = Some(x);
        entry.y = Some(y);
    }
    debounced_config_flush(app, skin_id);
}

/// Persist a border-dragged size (logical pixels) — same semantics as
/// save_dragged_position: memory first, debounced flush.
fn save_dragged_size(app: &AppHandle, skin_id: &str, width: u32, height: u32) {
    {
        let state = app.state::<crate::AppState>();
        let mut app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
        let entry = app_config
            .skin_settings
            .entry(skin_id.to_string())
            .or_default();
        entry.width = width;
        entry.height = height;
    }
    debounced_config_flush(app, skin_id);
}

// ── Skin right-click menu ───────────────────────────────────────────

/// Menu item ids returned by track_skin_popup_menu.
/// (Unconditional so non-Windows builds can still match on them.)
pub const SKIN_MENU_OPEN_CONFIG: u32 = 1;
pub const SKIN_MENU_RELOAD: u32 = 2;
pub const SKIN_MENU_UNLOAD: u32 = 3;
pub const SKIN_MENU_HIDE: u32 = 4;
/// 窗口行为快捷开关段（勾选态 = 当前态；勾选语义与编辑器「窗口」页一致）
pub const SKIN_MENU_PLACE_TOP: u32 = 10;
pub const SKIN_MENU_LOCK: u32 = 11;
pub const SKIN_MENU_CLICK_THROUGH: u32 = 12;
pub const SKIN_MENU_RESIZABLE: u32 = 13;
pub const SKIN_MENU_EDGE_SNAP: u32 = 14;
/// 自定义菜单项（皮肤经 skin_set_menu_items 运行时注册）：菜单 id =
/// SKIN_MENU_CUSTOM_BASE + 项序号，最多 SKIN_MENU_CUSTOM_MAX 条
pub const SKIN_MENU_CUSTOM_BASE: u32 = 100;
pub const SKIN_MENU_CUSTOM_MAX: usize = 8;

/// 行为快捷开关段的当前态（菜单打开时快照一次）
#[derive(Debug, Clone, Copy)]
pub struct SkinQuickState {
    pub on_top: bool,
    pub locked: bool,
    pub click_through: bool,
    pub resizable: bool,
    pub edge_snap: bool,
}

/// 皮肤自定义右键菜单项（运行时注册，与 manifest 无关——皮肤可随状态改
/// 文案与勾选态，如倒数日的「当前尺寸档位」勾选）
#[derive(Debug, Clone)]
pub struct SkinMenuItem {
    pub id: String,
    pub label_zh: String,
    pub label_en: String,
    pub checked: bool,
}

/// 皮肤自定义菜单项注册表：皮肤 id → 项。皮肤重载后页面重建、自行重注册；
/// 卸载时清除（unload_skin_impl）。
static SKIN_MENU_ITEMS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, Vec<SkinMenuItem>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

pub fn skin_menu_items(skin_id: &str) -> Vec<SkinMenuItem> {
    SKIN_MENU_ITEMS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(skin_id)
        .cloned()
        .unwrap_or_default()
}

pub fn set_skin_menu_items(skin_id: &str, items: Vec<SkinMenuItem>) {
    let mut map = SKIN_MENU_ITEMS.lock().unwrap_or_else(|e| e.into_inner());
    if items.is_empty() {
        map.remove(skin_id);
    } else {
        map.insert(skin_id.to_string(), items);
    }
}

pub fn clear_skin_menu_items(skin_id: &str) {
    SKIN_MENU_ITEMS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(skin_id);
}

/// Disable WebView2's default browser context menu on a skin window —
/// right-click shows our own native menu instead (track_skin_popup_menu).
///
/// Returns a flag that flips to true once applied.  The with_webview
/// closure runs INLINE when called on the main thread, but is only POSTED
/// to the event loop when called from a worker — so a worker-side caller
/// must poll the flag on a later attempt to learn the result.
#[cfg(target_os = "windows")]
pub fn disable_default_context_menu(
    window: &tauri::WebviewWindow,
) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
    let applied = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = applied.clone();
    let _ = window.with_webview(move |webview| unsafe {
        let ok = webview
            .controller()
            .CoreWebView2()
            .and_then(|core| core.Settings())
            .and_then(|settings| settings.SetAreDefaultContextMenusEnabled(false));
        flag.store(ok.is_ok(), std::sync::atomic::Ordering::SeqCst);
    });
    applied
}

/// Disable WebView2's browser accelerator keys (F5 / Ctrl+R / Ctrl+F5
/// refresh, Ctrl+P print, Alt+Home, F12 devtools …).  Neither a skin page
/// nor the manager UI may be reloadable by keystroke — window lifecycle is
/// owned by the manager alone.  Editing keys (Ctrl+C/V/X/Z/A) are DOM-level
/// and keep working.  Same applied-flag contract as
/// disable_default_context_menu.
#[cfg(target_os = "windows")]
pub fn disable_browser_accelerator_keys(
    window: &tauri::WebviewWindow,
) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
    let applied = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = applied.clone();
    let _ = window.with_webview(move |webview| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
        use windows::core::Interface;
        let ok = webview
            .controller()
            .CoreWebView2()
            .and_then(|core| core.Settings())
            .and_then(|settings| settings.cast::<ICoreWebView2Settings3>())
            .and_then(|s3| s3.SetAreBrowserAcceleratorKeysEnabled(false));
        flag.store(ok.is_ok(), std::sync::atomic::Ordering::SeqCst);
    });
    applied
}

/// Open a skin webview's DevTools window (开发模式：桥接捕获 F12 /
/// Ctrl+Shift+I 后经 open_skin_devtools 命令调用)。精确开锁，不动全局禁用
/// 的浏览器加速键——SetAreBrowserAcceleratorKeysEnabled(true) 会连带放回
/// F5/Ctrl+R 刷新键，并与 5 秒维护定时器的自愈重设互踩。
/// with_webview 在主线程内联执行（同 disable_default_context_menu 的契约）。
#[cfg(target_os = "windows")]
pub fn open_devtools(window: &tauri::WebviewWindow) {
    let _ = window.with_webview(move |webview| unsafe {
        let _ = webview
            .controller()
            .CoreWebView2()
            .and_then(|core| core.OpenDevToolsWindow());
    });
}

/// Retry applying webview hardening after window creation (no default
/// context menu, no browser accelerator keys): WebView2 finishes
/// initializing asynchronously, so the first attempts usually fail.  ~6s of
/// retries covers normal startup; for skin windows the 5s
/// frameless-maintenance timer in lib.rs keeps re-applying both afterwards.
#[cfg(target_os = "windows")]
pub fn spawn_webview_hardening_retry(app: &AppHandle, label: &str) {
    let handle = app.clone();
    let label = label.to_string();
    std::thread::spawn(move || {
        let mut menu: Option<std::sync::Arc<std::sync::atomic::AtomicBool>> = None;
        let mut accel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>> = None;
        let done = |f: &Option<std::sync::Arc<std::sync::atomic::AtomicBool>>| {
            f.as_ref()
                .map(|f| f.load(std::sync::atomic::Ordering::SeqCst))
                .unwrap_or(false)
        };
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(300));
            if done(&menu) && done(&accel) {
                break; // both applied by the previous attempts
            }
            let Some(window) = handle.get_webview_window(&label) else {
                break; // window destroyed (unloaded/reloaded) — give up
            };
            if !done(&menu) {
                menu = Some(disable_default_context_menu(&window));
            }
            if !done(&accel) {
                accel = Some(disable_browser_accelerator_keys(&window));
            }
        }
    });
}

/// Show the skin's right-click popup menu at the cursor position and return
/// the chosen SKIN_MENU_* id (0 = cancelled, ≥ SKIN_MENU_CUSTOM_BASE = 自定义
/// 项序号 + BASE)。  Modal — MUST be called on the window's owner (main)
/// thread.  `lang` selects the menu language.
#[cfg(target_os = "windows")]
pub fn track_skin_popup_menu(
    window: &tauri::WebviewWindow,
    lang: &str,
    quick: SkinQuickState,
    custom_items: &[SkinMenuItem],
) -> u32 {
    use crate::i18n::{Key, tr};
    use windows::Win32::Foundation::{HWND, POINT};
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, MF_CHECKED, MF_SEPARATOR,
        MF_STRING, SetForegroundWindow, TPM_NONOTIFY, TPM_RETURNCMD, TrackPopupMenu,
    };
    use windows::core::{HSTRING, PCWSTR};

    let Ok(hwnd) = window.hwnd() else {
        return 0;
    };
    unsafe {
        let hwnd = HWND(hwnd.0 as *mut _);
        let Ok(menu) = CreatePopupMenu() else {
            return 0;
        };
        // AppendMenuW copies the string, but keep the HSTRINGs alive for
        // the whole block anyway.
        let mut keepalive: Vec<HSTRING> = Vec::new();
        let mut append = |menu, flags, id, text: &HSTRING| {
            let _ = AppendMenuW(menu, flags, id, PCWSTR(text.as_ptr()));
            keepalive.push(text.clone());
        };
        let open_config = HSTRING::from(tr(lang, Key::MenuOpenConfig));
        let reload = HSTRING::from(tr(lang, Key::MenuReload));
        let hide = HSTRING::from(tr(lang, Key::MenuHideSkin));
        let unload = HSTRING::from(tr(lang, Key::MenuUnload));
        let place_top = HSTRING::from(tr(lang, Key::MenuPlaceTop));
        let lock_pos = HSTRING::from(tr(lang, Key::MenuLockPosition));
        let click_through = HSTRING::from(tr(lang, Key::MenuClickThrough));
        let resizable = HSTRING::from(tr(lang, Key::MenuResizable));
        let edge_snap = HSTRING::from(tr(lang, Key::MenuEdgeSnap));
        append(
            menu,
            MF_STRING,
            SKIN_MENU_OPEN_CONFIG as usize,
            &open_config,
        );
        append(menu, MF_STRING, SKIN_MENU_RELOAD as usize, &reload);

        // 窗口行为快捷开关段（勾选态 = 当前态；与编辑器「窗口」页同语义同文案）
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let mut quick_item = |on: bool, id: u32, text: &HSTRING| {
            append(
                menu,
                if on {
                    MF_STRING | MF_CHECKED
                } else {
                    MF_STRING
                },
                id as usize,
                text,
            );
        };
        quick_item(quick.on_top, SKIN_MENU_PLACE_TOP, &place_top);
        quick_item(quick.locked, SKIN_MENU_LOCK, &lock_pos);
        quick_item(quick.click_through, SKIN_MENU_CLICK_THROUGH, &click_through);
        quick_item(quick.resizable, SKIN_MENU_RESIZABLE, &resizable);
        quick_item(quick.edge_snap, SKIN_MENU_EDGE_SNAP, &edge_snap);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());

        append(menu, MF_STRING, SKIN_MENU_HIDE as usize, &hide);

        // 自定义项带分隔线自成一段、卸载始终垫底（破坏性操作不动位）
        if !custom_items.is_empty() {
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let zh = lang.starts_with("zh");
            for (i, item) in custom_items.iter().enumerate() {
                // 与 manifest 文案同规则：界面语言优先、缺失回退另一语言
                let label = if zh {
                    if item.label_zh.is_empty() {
                        &item.label_en
                    } else {
                        &item.label_zh
                    }
                } else if item.label_en.is_empty() {
                    &item.label_zh
                } else {
                    &item.label_en
                };
                let text = HSTRING::from(label.as_str());
                let flags = if item.checked {
                    MF_STRING | MF_CHECKED
                } else {
                    MF_STRING
                };
                append(menu, flags, SKIN_MENU_CUSTOM_BASE as usize + i, &text);
            }
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        }

        append(menu, MF_STRING, SKIN_MENU_UNLOAD as usize, &unload);

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // The menu only dismisses correctly (e.g. clicking elsewhere) when
        // its owner window is the foreground window.
        let _ = SetForegroundWindow(hwnd);
        let choice = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY,
            pt.x,
            pt.y,
            None,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
        choice.0 as u32
    }
}
