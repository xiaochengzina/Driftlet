//! 边缘吸附 —— 拖动皮肤窗口靠近屏幕边缘或其他皮肤窗口边缘时自动对齐。
//!
//! 机制概要（勿回归，详见 docs/关键机制.md「边缘吸附」）：
//! - 挂点在窗口子类的 `WM_MOVING`：系统模态拖动循环的每一步都会把待定的
//!   屏幕坐标 RECT 传进来，直接改 RECT 即完成吸附——不经过 Moved 事件，
//!   没有 set_position 回环，也不影响面板输入的精确坐标。
//! - 吸附状态按 HWND 注册：建窗时 `upsert`、销毁时 `unregister`、面板
//!   开关/改间距时 `upsert` 更新。候选边缘 = 注册表内其他皮肤窗口（实时
//!   取矩形）+ 窗口所在显示器的工作区。
//! - 吸附本身是纯距离判定：X/Y 两轴独立，屏幕候选优先，|delta| ≤ 阈值
//!   （SNAP_THRESHOLD）即吸附。贴边后同一拖动内要拖出阈值区才脱开——
//!   手感简单可预期，精细调整走下面的逃逸窗口。
//! - 逃逸 = 「松手再拖」的 1 秒自由窗口：上次拖动以吸附结束、窗口仍停在
//!   原吸附坐标时，`begin_drag` 为下次拖动开启 1 秒逃逸窗口——期间不
//!   吸附，仅把窗口夹取在屏幕工作区 − gap 内（防拖出屏幕）；时间到即
//!   恢复吸附（同一拖动内生效）。
//! - 间距（gap）与触发阈值都是逻辑像素，吸附前按窗口 DPI 换算成物理
//!   像素。

use std::collections::HashMap;
use std::sync::Mutex;

/// 吸附触发距离（逻辑像素）：窗口边缘与目标边缘的距离 ≤ 该值时吸附。
const SNAP_THRESHOLD: i32 = 10;

/// 「松手再拖」逃逸窗口时长：上次拖动以吸附结束时，下次拖动的前 1 秒
/// 不吸附（仅夹取在屏幕内），时间到恢复吸附。
const ESCAPE_WINDOW: std::time::Duration = std::time::Duration::from_millis(1000);

/// 吸附间距上限（逻辑像素），命令侧与前端输入同步 clamp。
pub const MAX_SNAP_GAP: u32 = 200;

/// 单个皮肤窗口的吸附配置（按 HWND 归属）。
#[derive(Debug, Clone, Copy, Default)]
pub struct SnapEntry {
    pub enabled: bool,
    /// 吸附后与边缘保留的间距，逻辑像素
    pub gap: u32,
}

/// 所有已加载皮肤窗口的吸附状态。键是 HWND——子类回调里只有 HWND 可用；
/// 候选窗口的矩形在拖动时实时读取，不在此缓存（无陈旧数据问题）。
static SNAP_WINDOWS: std::sync::LazyLock<Mutex<HashMap<isize, SnapEntry>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// 登记/更新一个皮肤窗口的吸附配置（建窗与面板改设置时调用）。
pub fn upsert(hwnd: isize, enabled: bool, gap: u32) {
    SNAP_WINDOWS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(hwnd, SnapEntry { enabled, gap });
}

/// 窗口销毁时摘除登记。HWND 会被系统回收复用，残留条目可能把无关窗口
/// 误当吸附候选，必须随销毁清理。
pub fn unregister(hwnd: isize) {
    SNAP_WINDOWS.lock().unwrap_or_else(|e| e.into_inner()).remove(&hwnd);
    DRAG_STATES.lock().unwrap_or_else(|e| e.into_inner()).remove(&hwnd);
}

/// 与平台无关的矩形（物理像素），便于纯逻辑测试。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// 拖动运行时状态（跨拖动保留，只随 unregister 清除）：
/// `ended_snapped` = 上次拖动结束时的吸附位置（跨拖动记忆，None = 上次
/// 拖动未以吸附结束）；`escape_until` = 本次拖动的逃逸窗口截止时间
/// （begin_drag 决定）。
#[derive(Debug, Clone, Copy, Default)]
struct DragRuntime {
    ended_snapped: Option<(i32, i32)>,
    escape_until: Option<std::time::Instant>,
}

static DRAG_STATES: std::sync::LazyLock<Mutex<HashMap<isize, DragRuntime>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// 两段区间是否重叠或间距不超过 slack。
fn ranges_near(a1: i32, a2: i32, b1: i32, b2: i32, slack: i32) -> bool {
    (a1 - b2) <= slack && (b1 - a2) <= slack
}

/// 单轴候选选取（纯函数）：屏幕候选优先，|delta| ≤ threshold 中取
/// |delta| 最小者；屏幕无候选才看窗口候选。返回吸附后的轴坐标。
fn pick_axis(raw: i32, screen: &[i32], windows: &[i32], threshold: i32) -> Option<i32> {
    let pick = |cands: &[i32]| {
        cands
            .iter()
            .copied()
            .filter(|c| (c - raw).abs() <= threshold)
            .min_by_key(|c| (c - raw).abs())
    };
    pick(screen).or_else(|| pick(windows))
}

/// 整窗距离吸附（纯函数）：X/Y 两轴独立判定，屏幕边缘优先。
/// `moving` = 拖动中的待定矩形，`work` = 屏幕工作区，`others` = 其他皮肤
/// 窗口矩形，`gap` / `threshold` 均为物理像素。窗口候选要求两窗口在垂直
/// 于该轴的方向上重叠或足够近，否则远处窗口会造成「幻影吸附」。
/// 返回 (吸附后矩形, x 轴是否吸附, y 轴是否吸附)。
pub fn snap_drag(
    moving: SnapRect,
    work: SnapRect,
    others: &[SnapRect],
    gap: i32,
    threshold: i32,
) -> (SnapRect, bool, bool) {
    let w = moving.right - moving.left;
    let h = moving.bottom - moving.top;

    let screen_x = [work.left + gap, work.right - gap - w];
    let screen_y = [work.top + gap, work.bottom - gap - h];

    let mut win_x = Vec::new();
    let mut win_y = Vec::new();
    for o in others {
        if ranges_near(moving.top, moving.bottom, o.top, o.bottom, threshold) {
            // 左缘对齐 / 右缘对齐 / 贴到它左侧 / 贴到它右侧
            win_x.extend([o.left, o.right - w, o.left - gap - w, o.right + gap]);
        }
        if ranges_near(moving.left, moving.right, o.left, o.right, threshold) {
            win_y.extend([o.top, o.bottom - h, o.top - gap - h, o.bottom + gap]);
        }
    }

    let left = pick_axis(moving.left, &screen_x, &win_x, threshold);
    let top = pick_axis(moving.top, &screen_y, &win_y, threshold);

    let new_left = left.unwrap_or(moving.left);
    let new_top = top.unwrap_or(moving.top);
    (
        SnapRect {
            left: new_left,
            top: new_top,
            right: new_left + w,
            bottom: new_top + h,
        },
        left.is_some(),
        top.is_some(),
    )
}

/// 逃逸窗口内的夹取（纯函数）：窗口不得超出屏幕工作区 − gap
/// （「不能超出屏幕 + 自定义间距」）。窗口比工作区还宽/高时钉在 min 边。
pub fn clamp_to_work(moving: SnapRect, work: SnapRect, gap: i32) -> SnapRect {
    let w = moving.right - moving.left;
    let h = moving.bottom - moving.top;
    let min_x = work.left + gap;
    let min_y = work.top + gap;
    let max_x = (work.right - gap - w).max(min_x);
    let max_y = (work.bottom - gap - h).max(min_y);
    let left = moving.left.max(min_x).min(max_x);
    let top = moving.top.max(min_y).min(max_y);
    SnapRect {
        left,
        top,
        right: left + w,
        bottom: top + h,
    }
}

/// 是否为新拖动开启逃逸窗口（纯函数）：上次拖动以吸附结束且窗口仍停在
/// 原吸附坐标（±1px 容差）。位置已被面板/程序改动则不开启。
pub fn should_arm_escape(ended_snapped: Option<(i32, i32)>, current: SnapRect) -> bool {
    match ended_snapped {
        Some((lx, ly)) => (current.left - lx).abs() <= 1 && (current.top - ly).abs() <= 1,
        None => false,
    }
}

/// WM_ENTERSIZEMOVE：一次拖动/缩放循环开始，决定是否开启逃逸窗口。
///
/// 上次拖动以吸附结束、窗口仍停在原位 → 本次拖动的前 ESCAPE_WINDOW 时间
/// 内不吸附（「松手再拖 = 逃逸」）。注意只设置逃逸窗口，不碰
/// ended_snapped——它要跨拖动保留，不要在 WM_EXITSIZEMOVE 清理。
#[cfg(target_os = "windows")]
pub fn begin_drag(hwnd_val: isize) {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

    let mut r = RECT::default();
    let current = unsafe { GetWindowRect(HWND(hwnd_val as *mut _), &mut r) }
        .ok()
        .map(|_| SnapRect {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        });
    let mut states = DRAG_STATES.lock().unwrap_or_else(|e| e.into_inner());
    let st = states.entry(hwnd_val).or_default();
    st.escape_until = match current {
        Some(rect) if should_arm_escape(st.ended_snapped, rect) => {
            Some(std::time::Instant::now() + ESCAPE_WINDOW)
        }
        _ => None,
    };
}

/// WM_MOVING 处理：把待定 RECT 就地改成吸附后的位置。
/// 未开启吸附的窗口在注册表查不到 enabled 条目，一次哈希查找即返回。
#[cfg(target_os = "windows")]
pub fn on_window_moving(hwnd_val: isize, l_param: isize) {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, IsIconic, IsWindow, IsWindowVisible,
    };

    let entry = SNAP_WINDOWS.lock().unwrap_or_else(|e| e.into_inner()).get(&hwnd_val).copied();
    let Some(entry) = entry else {
        return;
    };
    if !entry.enabled {
        return;
    }

    unsafe {
        let hwnd = HWND(hwnd_val as *mut _);
        let rect = &mut *(l_param as *mut RECT);
        let moving = SnapRect {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        };

        // 逻辑像素 → 物理像素（WM_MOVING 的 RECT 是屏幕物理坐标）
        let dpi = GetDpiForWindow(hwnd) as i32;
        let to_physical = |v: i32| (v * dpi + 48) / 96;
        let threshold = to_physical(SNAP_THRESHOLD);
        let gap = to_physical(entry.gap as i32);

        // 工作区（不含任务栏），多屏时取窗口当前所在的显示器
        let monitor = MonitorFromRect(rect, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO::default();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let work = SnapRect {
            left: info.rcWork.left,
            top: info.rcWork.top,
            right: info.rcWork.right,
            bottom: info.rcWork.bottom,
        };

        // 其他皮肤窗口的实时矩形（跳过自己、已销毁、不可见、最小化的）
        let hwnds: Vec<isize> = SNAP_WINDOWS.lock().unwrap_or_else(|e| e.into_inner()).keys().copied().collect();
        let mut others = Vec::with_capacity(hwnds.len().saturating_sub(1));
        for other_val in hwnds {
            if other_val == hwnd_val {
                continue;
            }
            let other = HWND(other_val as *mut _);
            if !IsWindow(Some(other)).as_bool()
                || !IsWindowVisible(other).as_bool()
                || IsIconic(other).as_bool()
            {
                continue;
            }
            let mut r = RECT::default();
            if GetWindowRect(other, &mut r).is_ok() {
                others.push(SnapRect {
                    left: r.left,
                    top: r.top,
                    right: r.right,
                    bottom: r.bottom,
                });
            }
        }

        let out = {
            let mut states = DRAG_STATES.lock().unwrap_or_else(|e| e.into_inner());
            let st = states.entry(hwnd_val).or_default();
            let in_escape = st
                .escape_until
                .map_or(false, |t| std::time::Instant::now() < t);
            if in_escape {
                // 逃逸窗口内：不吸附，仅夹取在屏幕工作区 − gap 内
                clamp_to_work(moving, work, gap)
            } else {
                // 到期（或未开启）：正常吸附，并记录本次拖动的吸附状态
                // ——拖动结束时停在哪，决定下次拖动是否给逃逸窗口
                st.escape_until = None;
                let (snapped, sx, sy) = snap_drag(moving, work, &others, gap, threshold);
                st.ended_snapped = if sx || sy {
                    Some((snapped.left, snapped.top))
                } else {
                    None
                };
                snapped
            }
        };

        rect.left = out.left;
        rect.top = out.top;
        rect.right = out.right;
        rect.bottom = out.bottom;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: SnapRect = SnapRect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1040, // 扣除任务栏
    };
    const T: i32 = 10;

    fn rect(left: i32, top: i32, w: i32, h: i32) -> SnapRect {
        SnapRect {
            left,
            top,
            right: left + w,
            bottom: top + h,
        }
    }

    /// gap = 0 的整窗吸附
    fn drag(moving: SnapRect, others: &[SnapRect]) -> (SnapRect, bool, bool) {
        snap_drag(moving, WORK, others, 0, T)
    }

    #[test]
    fn snaps_to_screen_edges() {
        let (out, sx, _) = drag(rect(10, 300, 200, 100), &[]);
        assert_eq!(out.left, 0);
        assert!(sx);
        let (out, _, _) = drag(rect(1712, 300, 200, 100), &[]);
        assert_eq!(out.right, 1920);
        let (out, _, sy) = drag(rect(300, 933, 200, 100), &[]);
        assert_eq!(out.bottom, 1040);
        assert!(sy);
    }

    #[test]
    fn no_snap_beyond_threshold() {
        let m = rect(100, 300, 200, 100);
        let (out, sx, sy) = drag(m, &[]);
        assert_eq!(out, m);
        assert!(!sx && !sy);
    }

    #[test]
    fn gap_is_kept() {
        let (out, _, _) = snap_drag(rect(10, 8, 200, 100), WORK, &[], 12, T);
        assert_eq!(out.left, 12);
        assert_eq!(out.top, 12);
    }

    #[test]
    fn screen_beats_window_even_if_window_is_closer() {
        // 屏幕候选（delta 10）与窗口候选（delta 0）同时在阈值内 → 屏幕优先
        let other = rect(-190, 320, 200, 100); // 右缘 = 10，与 raw 重合
        let (out, _, _) = drag(rect(10, 320, 200, 100), &[other]);
        assert_eq!(out.left, 0, "屏幕边缘必须优先于窗口边缘");
    }

    #[test]
    fn snaps_to_window_edges() {
        let other = rect(500, 400, 200, 100);
        // 左缘对齐（垂直区间有交叠）
        let (out, sx, _) = drag(rect(506, 420, 200, 100), &[other]);
        assert_eq!(out.left, 500);
        assert!(sx);
        // 相邻贴合（贴到它右侧，gap 10）
        let (out, _, _) = snap_drag(rect(718, 420, 200, 100), WORK, &[other], 10, T);
        assert_eq!(out.left, 710);
        // 上缘对齐
        let (out, _, sy) = drag(rect(520, 395, 200, 100), &[other]);
        assert_eq!(out.top, 400);
        assert!(sy);
    }

    #[test]
    fn distant_window_causes_no_phantom_snap() {
        // 垂直方向相距很远（> 阈值）→ 该窗口不参与 X 轴吸附
        let other = rect(500, 900, 200, 100);
        let m = rect(506, 100, 200, 100);
        let (out, sx, sy) = drag(m, &[other]);
        assert_eq!(out, m);
        assert!(!sx && !sy);
    }

    #[test]
    fn clamp_keeps_window_inside_work() {
        // 左边越界 → 夹回 0
        assert_eq!(clamp_to_work(rect(-50, 500, 200, 100), WORK, 0).left, 0);
        // 右/上越界 → 夹回
        let out = clamp_to_work(rect(2000, -20, 200, 100), WORK, 0);
        assert_eq!(out.left, 1720);
        assert_eq!(out.top, 0);
        // gap 生效：最小位置 = gap
        let out = clamp_to_work(rect(2, 2, 200, 100), WORK, 10);
        assert_eq!(out.left, 10);
        assert_eq!(out.top, 10);
        // 窗口比屏幕宽 → 钉在 min 边（不 panic）
        assert_eq!(clamp_to_work(rect(-100, 0, 3000, 100), WORK, 0).left, 0);
    }

    #[test]
    fn escape_window_armed_only_when_still_at_snap() {
        // 上次以吸附结束且窗口仍在原位（±1px 容差）→ 开启
        assert!(should_arm_escape(Some((0, 300)), rect(0, 300, 200, 100)));
        assert!(should_arm_escape(Some((0, 300)), rect(1, 300, 200, 100)));
        // 窗口已被移走 → 不开启
        assert!(!should_arm_escape(Some((0, 300)), rect(5, 300, 200, 100)));
        // 上次拖动未以吸附结束 → 不开启
        assert!(!should_arm_escape(None, rect(0, 300, 200, 100)));
    }
}
