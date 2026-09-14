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
//!   （SNAP_THRESHOLD）即吸附。贴边后同一拖动内拖出阈值区即脱开——
//!   手感简单可预期，精细调整靠拖出阈值区或管理器面板的精确坐标。
//! - 勿回归（慢拖粘死事故）：WM_MOVING 的提议矩形是相对「上一步写回值」
//!   的增量（实机注入轨迹实证）——吸附改写窗口后，下一步提议仍贴着吸附
//!   位，直接判定提议会把慢拖粘死在阈值区内（只有单步 >阈值的快甩能脱
//!   开）。吸附判定必须针对按增量累积的原始轨迹（DragRuntime 的
//!   raw/last_out，advance_raw 推进），拖出阈值区即脱开。
//! - 「松手再拖」逃逸窗口已移除（勿再加回）：阈值脱开修复前它是精细
//!   调整的唯一出口；修复后拖出阈值区即可脱开，微调走管理器面板精确
//!   坐标。曾同期回退删除的还有方向感知逃逸与 Shift 临时绕过。
//! - 间距（gap）与触发阈值都是逻辑像素，吸附前按窗口 DPI 换算成物理
//!   像素。

use std::collections::HashMap;
use std::sync::Mutex;

/// 吸附触发距离（逻辑像素）：窗口边缘与目标边缘的距离 ≤ 该值时吸附。
const SNAP_THRESHOLD: i32 = 10;

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
/// gap 在登记处统一钳制——手改 config.json 注入的越界值不能直达
/// 吸附判定（命令路径 set_skin_snap_gap 已钳，这里是消费侧总闸）
pub fn upsert(hwnd: isize, enabled: bool, gap: u32) {
    SNAP_WINDOWS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(
            hwnd,
            SnapEntry {
                enabled,
                gap: gap.min(MAX_SNAP_GAP),
            },
        );
}

/// 窗口销毁时摘除登记。HWND 会被系统回收复用，残留条目可能把无关窗口
/// 误当吸附候选，必须随销毁清理。
pub fn unregister(hwnd: isize) {
    SNAP_WINDOWS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&hwnd);
    DRAG_STATES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&hwnd);
}

/// 与平台无关的矩形（物理像素），便于纯逻辑测试。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// 拖动运行时状态（只服务本次拖动，begin_drag 重置，随 unregister 清除）：
/// `raw` = 未被吸附改写的原始轨迹矩形；`last_out` = 上一步实际写回系统的
/// 矩形（算增量用）。每步按「提议 − 上一步写回」推进 raw（勿回归——
/// 系统提议是增量制，见模块头注释）。
#[derive(Debug, Clone, Copy, Default)]
struct DragRuntime {
    raw: Option<SnapRect>,
    last_out: Option<SnapRect>,
}

static DRAG_STATES: std::sync::LazyLock<Mutex<HashMap<isize, DragRuntime>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// 两段区间是否重叠或间距不超过 slack。
fn ranges_near(a1: i32, a2: i32, b1: i32, b2: i32, slack: i32) -> bool {
    (a1 - b2) <= slack && (b1 - a2) <= slack
}

/// 原始拖动轨迹推进（纯函数）：raw += 本步提议 − 上一步写回。
/// 系统模态移动循环的 WM_MOVING 提议矩形是相对上一步「写回值」的增量，
/// 吸附改写窗口后提议仍贴着吸附位——判定若直接吃提议，慢拖时提议永远
/// 落在阈值区内（粘死）；按增量自行累积的 raw 才是真实拖动轨迹。
fn advance_raw(raw: SnapRect, last_out: SnapRect, proposed: SnapRect) -> SnapRect {
    let w = raw.right - raw.left;
    let h = raw.bottom - raw.top;
    let left = raw.left + (proposed.left - last_out.left);
    let top = raw.top + (proposed.top - last_out.top);
    SnapRect {
        left,
        top,
        right: left + w,
        bottom: top + h,
    }
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
/// `moving` = 原始拖动轨迹矩形（未被吸附改写的 raw，勿传 WM_MOVING 的
/// 增量提议矩形——见模块头注释），`work` = 屏幕工作区，`others` = 其他皮肤
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

    // 超宽/超高兜底：窗口 ≥ 工作区 − 2·gap 时 `right−gap−w` 落在工作区
    // 左侧之外——吸过去窗口被推出屏（报告实证：超宽窗拖近右缘被吸出屏）。
    // 放不下时屏幕候选整轴剔除（窗口间候选不受影响）。
    // 复审 B-F2：剔除用空切片表达——曾用 [i32::MIN, i32::MIN] 哨兵，
    // pick_axis 的 (c - raw).abs() 在 raw ≥ 0 时溢出（release 环绕成负值
    // 反过阈值过滤，窗口被吸附到 -2³¹；debug 直接 panic）
    let fits_w = w + 2 * gap <= work.right - work.left;
    let fits_h = h + 2 * gap <= work.bottom - work.top;
    let screen_x: &[i32] = if fits_w {
        &[work.left + gap, work.right - gap - w]
    } else {
        &[]
    };
    let screen_y: &[i32] = if fits_h {
        &[work.top + gap, work.bottom - gap - h]
    } else {
        &[]
    };

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

    let left = pick_axis(moving.left, screen_x, &win_x, threshold);
    let top = pick_axis(moving.top, screen_y, &win_y, threshold);

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

/// WM_ENTERSIZEMOVE：一次拖动/缩放循环开始，重置原始拖动轨迹。
/// （缩放循环也走此消息：raw/last_out 只服务 WM_MOVING，下次拖动重写，
/// 无需在退出消息里清理。）
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
    st.raw = current;
    st.last_out = current;
}

/// WM_MOVING 处理：把待定 RECT 就地改成吸附后的位置。
/// 未开启吸附的窗口在注册表查不到 enabled 条目，一次哈希查找即返回。
#[cfg(target_os = "windows")]
pub fn on_window_moving(hwnd_val: isize, l_param: isize) {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
    };
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, IsIconic, IsWindow, IsWindowVisible,
    };

    let entry = SNAP_WINDOWS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&hwnd_val)
        .copied();
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
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
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
        let hwnds: Vec<isize> = SNAP_WINDOWS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .copied()
            .collect();
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
            // 还原未被吸附改写的原始轨迹：提议矩形是相对上一步写回值
            // 的增量，直接判定会把慢拖粘死在阈值区内（勿回归，见模块头注释）。
            // begin_drag 未跑过的兜底（理论不可达：模态循环先发
            // WM_ENTERSIZEMOVE）：本步按提议矩形原样处理，从下一步起正常累积。
            let raw = match (st.raw, st.last_out) {
                (Some(r), Some(l)) => advance_raw(r, l, moving),
                _ => moving,
            };
            st.raw = Some(raw);
            let (out, _, _) = snap_drag(raw, work, &others, gap, threshold);
            st.last_out = Some(out);
            out
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
    fn oversized_window_screen_candidates_excluded() {
        // B-F2：窗口比工作区宽时屏幕候选整轴剔除——拖到 left=0 不得被吸到
        // 哨兵位（旧实现用 i32::MIN，(c-raw).abs() 溢出后反而通过阈值过滤）
        let huge = rect(0, 300, 4000, 100); // 4000 > 1920 工作区宽
        let (out, sx, _) = drag(huge, &[]);
        assert_eq!(out.left, 0, "超宽窗不得吸附（候选剔除）");
        assert!(!sx);
        // 窗口间候选不受超宽影响
        let other = rect(500, 300, 200, 100);
        let (out, sx, _) = drag(rect(505, 305, 4000, 100), &[other]);
        assert_eq!(out.left, 500, "窗口候选对齐照旧");
        assert!(sx);
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
    fn advance_raw_accumulates_deltas() {
        // raw 按「提议 − 上一步写回」推进，与吸附改写解耦
        let raw = rect(100, 100, 200, 100);
        let last = rect(90, 104, 200, 100); // 上一步吸附改写后的写回
        let proposed = rect(96, 108, 200, 100); // 系统下一步提议 = 写回 + 鼠标增量
        let out = advance_raw(raw, last, proposed);
        assert_eq!(out, rect(106, 104, 200, 100)); // raw(100,100) + (6,4)
    }

    /// ⭑ 慢拖粘死事故回归：吸附后慢拖（每步增量 ≤ 阈值）必须在 raw 拖出
    /// 阈值区后脱开并跟随鼠标。模拟系统移动循环的增量提议：每步提议 =
    /// 上一步输出 + 6px（实机注入轨迹实证的系统行为）。
    #[test]
    fn slow_drag_detaches_once_raw_leaves_threshold_zone() {
        let other = rect(900, 400, 200, 100);
        let gap = 20;
        let candidate = 900 - gap - 200; // 贴到它左侧 = 680
        let mut raw = rect(600, 420, 200, 100);
        let mut last_out = raw;
        let mut snaps = Vec::new(); // (raw.left, out.left, x吸附?)
        for _ in 0..40 {
            let proposed = rect(last_out.left + 6, last_out.top, 200, 100);
            raw = advance_raw(raw, last_out, proposed);
            let (out, sx, _sy) = snap_drag(raw, WORK, &[other], gap, T);
            snaps.push((raw.left, out.left, sx));
            last_out = out;
        }
        // raw 进入阈值区即吸附到候选位
        let first_snap = snaps
            .iter()
            .position(|&(_, o, s)| s && o == candidate)
            .expect("raw 进阈值区应吸附到候选位");
        // 吸附期间窗口钉在候选位（raw 仍被鼠标推进）
        for &(_, o, s) in &snaps[first_snap..] {
            if s {
                assert_eq!(o, candidate, "吸附期间必须钉在候选位");
            }
        }
        // raw 越过 candidate+T 后必须脱开：输出跟随 raw（旧行为是永久钉死）
        let detach = snaps
            .iter()
            .position(|&(r, o, s)| !s && r > candidate + T && o == r)
            .expect("raw 拖出阈值区后必须脱开并跟随原始轨迹");
        assert!(detach > first_snap, "必须先吸附后脱开");
        for &(r, o, s) in &snaps[detach..] {
            assert!(!s && o == r, "脱开后不得再被吸回（raw 已远离候选位）");
        }
    }
}
