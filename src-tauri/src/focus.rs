//! 专注模式：一键（热键/托盘/面板）或全屏自动进入「免打扰桌面」——
//! 按动作档（隐藏/卸载）关闭所有非白名单皮肤，退出时按进入时的快照还原。
//! 方案：docs/proposals/专注模式方案-2026-09.md（语义定案都在那里）。
//!
//! 关键口径：
//! - 持久化面 = config.focus_mode { active, action（进入档）, snapshot }：
//!   模式激活期间应用退出/崩溃，下次启动按快照动作恢复并清除
//!   （startup_recover，启动 = 全新桌面，不带着模式复活）。
//! - 会话面 = RUNTIME（trigger/suppressed/busy/全屏沿跟踪），不落盘。
//! - 动作分流按「进入时生效档」还原（模式期间改偏好档不错配）。
//! - 白名单（skin_settings.*.focus_exempt）两档都豁免；模式期间用户手动
//!   加载/卸载优先于模式（新加载放行、快照里被卸载的恢复时跳过）。

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;
use crate::skin::config;

/// 激活来源：全屏进的才随全屏消失自动退；手动进的只能手动退
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trigger {
    Manual,
    Fullscreen,
}

struct Runtime {
    trigger: Option<Trigger>,
    /// 当前全屏周期起点（None = 当前无全屏）；新周期开始 = 解除抑制
    fs_since: Option<Instant>,
    /// 非全屏起点（全屏档的 5s 防抖退出用）
    nonfs_since: Option<Instant>,
    /// 手动退出抑制：全屏期间手动退出后，本次全屏周期不再自动进入
    suppressed: bool,
    /// enter/exit 互斥（连按热键/面板连点排队而非交错）
    busy: bool,
}

static RUNTIME: Mutex<Runtime> = Mutex::new(Runtime {
    trigger: None,
    fs_since: None,
    nonfs_since: None,
    suppressed: false,
    busy: false,
});

/// 当前动作偏好档（"unload" 之外一律按 hide 归一）
fn current_action(app: &AppHandle) -> &'static str {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().unwrap_or_else(|e| e.into_inner());
    if cfg.focus_mode_action == "unload" {
        "unload"
    } else {
        "hide"
    }
}

/// 模式是否激活（读持久态——管理器/托盘初始化都读这里）
pub fn is_active(app: &AppHandle) -> bool {
    app.state::<AppState>()
        .config
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .focus_mode
        .active
}

/// 白名单集合（focus_exempt 的皮肤 id）
fn exempt_ids(app: &AppHandle) -> HashSet<String> {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().unwrap_or_else(|e| e.into_inner());
    cfg.skin_settings
        .iter()
        .filter(|(_, c)| c.focus_exempt)
        .map(|(id, _)| id.clone())
        .collect()
}

/// 模式变更的对外信号：管理器面板联动刷新（开着时）+ 托盘勾选。
/// 管理器已销毁（关窗即销毁）时事件自然丢弃——面板重开时经
/// get_focus_mode_state 现拉真实状态。
fn emit_changed(
    app: &AppHandle,
    active: bool,
    action: &str,
    trigger: &str,
    affected: usize,
    restored: usize,
    skipped: usize,
) {
    let _ = app.emit(
        "focus-mode-changed",
        serde_json::json!({
            "active": active,
            "action": action,
            "trigger": trigger,
            "affected": affected,
            "restored": restored,
            "skipped": skipped,
        }),
    );
    // 托盘「专注模式」勾选同步（复用 toggle_item 句柄）
    let state = app.state::<AppState>();
    let item = state
        .toggle_item
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if let Some(item) = item {
        let _ = item.set_checked(active);
    }
}

/// 进入专注模式。幂等（已激活则按当前态返回计数 0）。
/// 返回 (affected,)——关闭了多少张。
pub async fn enter(app: &AppHandle, trigger: Trigger) -> Result<usize, String> {
    {
        let mut rt = RUNTIME.lock().unwrap_or_else(|e| e.into_inner());
        if rt.busy || is_active(app) {
            return Ok(0);
        }
        rt.busy = true;
    }
    let result = enter_inner(app, trigger).await;
    RUNTIME.lock().unwrap_or_else(|e| e.into_inner()).busy = false;
    result
}

async fn enter_inner(app: &AppHandle, trigger: Trigger) -> Result<usize, String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    let action = current_action(app);
    let exempt = exempt_ids(app);
    let loaded = state.registry.loaded_ids();

    // 进入快照：隐藏档只记「当前可见」的（已隐藏的恢复时无需碰）；
    // 卸载档记全部已加载的（隐藏的也是驻留内存，卸载档的本职就是回收）
    let snapshot: Vec<String> = loaded
        .iter()
        .filter(|id| !exempt.contains(*id))
        .filter(|id| {
            action == "unload"
                || state
                    .registry
                    .get(id)
                    .map(|w| w.is_visible().unwrap_or(true))
                    .unwrap_or(false)
        })
        .cloned()
        .collect();

    // 先落盘（崩溃安全），再动窗口
    {
        let mut cfg = state.config.lock().unwrap_or_else(|e| e.into_inner());
        cfg.focus_mode.active = true;
        cfg.focus_mode.action = action.to_string();
        cfg.focus_mode.snapshot = snapshot.clone();
        config::save_config(&state.config_dir, &cfg).map_err(|e| {
            crate::i18n::trf(&lang, crate::i18n::Key::ConfigSaveFailed, &[&e.to_string()])
        })?;
    }
    RUNTIME.lock().unwrap_or_else(|e| e.into_inner()).trigger = Some(trigger);

    let mut affected = 0;
    if action == "hide" {
        for id in &snapshot {
            if let Some(w) = state.registry.get(id) {
                let _ = w.hide();
                affected += 1;
            }
        }
        crate::hotkey::sync_tray_toggle_item(app);
    } else {
        // 与命令层同两把生命周期锁（串行卸载，不进并发）
        let _a = state.lifecycle_lock.lock().await;
        let _b = state.install_lock.lock().await;
        for id in &snapshot {
            match crate::commands::unload_skin_impl(app.clone(), id.clone()).await {
                Ok(()) => affected += 1,
                Err(e) => log::warn!("focus: unload '{}' failed: {}", id, e),
            }
        }
    }
    emit_changed(
        app,
        true,
        action,
        if trigger == Trigger::Fullscreen {
            "fullscreen"
        } else {
            "manual"
        },
        affected,
        0,
        0,
    );
    Ok(affected)
}

/// 退出专注模式（manual = 用户主动退；false = 全屏消失自动退）。
/// 返回 (restored, skipped)。
pub async fn exit(app: &AppHandle, manual: bool) -> Result<(usize, usize), String> {
    {
        let mut rt = RUNTIME.lock().unwrap_or_else(|e| e.into_inner());
        if rt.busy || !is_active(app) {
            return Ok((0, 0));
        }
        rt.busy = true;
        // 手动退出且当前在全屏周期 → 本周期抑制自动再进（沿触发定案）
        if manual && rt.fs_since.is_some() {
            rt.suppressed = true;
        }
    }
    let result = exit_inner(app).await;
    let mut rt = RUNTIME.lock().unwrap_or_else(|e| e.into_inner());
    rt.busy = false;
    rt.trigger = None;
    result
}

async fn exit_inner(app: &AppHandle) -> Result<(usize, usize), String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    // 按进入档还原（模式期间改偏好档不错配）
    let (action, snapshot) = {
        let mut cfg = state.config.lock().unwrap_or_else(|e| e.into_inner());
        let action = cfg.focus_mode.action.clone();
        let snapshot = std::mem::take(&mut cfg.focus_mode.snapshot);
        cfg.focus_mode.active = false;
        config::save_config(&state.config_dir, &cfg).map_err(|e| {
            crate::i18n::trf(&lang, crate::i18n::Key::ConfigSaveFailed, &[&e.to_string()])
        })?;
        (action, snapshot)
    };

    let mut restored = 0;
    let mut skipped = 0;
    if action == "hide" {
        for id in &snapshot {
            match state.registry.get(id) {
                Some(w) => {
                    let _ = w.show();
                    restored += 1;
                }
                None => skipped += 1, // 模式期间被卸载——跳过
            }
        }
        crate::hotkey::sync_tray_toggle_item(app);
    } else {
        let _a = state.lifecycle_lock.lock().await;
        let _b = state.install_lock.lock().await;
        for id in &snapshot {
            if state.registry.is_loaded(id) {
                continue; // 用户已手动加载回来——手动动作优先
            }
            match crate::commands::load_skin_impl(app.clone(), id.clone()).await {
                Ok(()) => restored += 1,
                Err(e) => {
                    log::warn!("focus: reload '{}' failed: {}", id, e);
                    skipped += 1;
                }
            }
        }
    }
    emit_changed(app, false, &action, "", 0, restored, skipped);
    Ok((restored, skipped))
}

/// 切换（热键/托盘/面板同一路径）
pub fn toggle(app: &AppHandle) {
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        if is_active(&app2) {
            if let Err(e) = exit(&app2, true).await {
                log::warn!("focus exit failed: {}", e);
            }
        } else if let Err(e) = enter(&app2, Trigger::Manual).await {
            log::warn!("focus enter failed: {}", e);
        }
    });
}

/// 启动恢复：模式态残留（上次退出/崩溃在模式激活期间）→ 按快照动作恢复
/// 并清除。在自载皮肤之后调用（lib.rs setup）。
pub async fn startup_recover(app: &AppHandle) {
    let state = app.state::<AppState>();
    let (active, action, snapshot) = {
        let cfg = state.config.lock().unwrap_or_else(|e| e.into_inner());
        (
            cfg.focus_mode.active,
            cfg.focus_mode.action.clone(),
            cfg.focus_mode.snapshot.clone(),
        )
    };
    if !active {
        return;
    }
    log::info!(
        "focus mode state left over from previous run — restoring snapshot ({} skins, action={})",
        snapshot.len(),
        action
    );
    if action == "unload" {
        for id in &snapshot {
            if !state.registry.is_loaded(id) {
                if let Err(e) = crate::commands::load_skin_impl(app.clone(), id.clone()).await {
                    log::warn!("focus startup_recover: reload '{}' failed: {}", id, e);
                }
            }
        }
    }
    // 隐藏档无需动作（重启后皮肤照常加载显示，已是正确终态）
    let mut cfg = state.config.lock().unwrap_or_else(|e| e.into_inner());
    cfg.focus_mode.active = false;
    cfg.focus_mode.snapshot.clear();
    if let Err(e) = config::save_config(&state.config_dir, &cfg) {
        log::warn!("focus startup_recover: config save failed: {}", e);
    }
}

// ─── 全屏检测（2.5s 轮询兜底制——v1 不用 SetWinEventHook：消息泵线程亲和
// 的复杂度不值，进入延迟 ≤2.5s 可接受，轮询成本≈0） ───

/// 检测线程：常驻，但只在 auto_fullscreen 开启时干活（每拍现读配置）。
pub fn spawn_detector(app: &AppHandle) {
    let h = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(2500));
            {
                let state = h.state::<AppState>();
                if state.exiting.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                let enabled = state
                    .config
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .focus_mode_auto_fullscreen;
                if !enabled {
                    continue;
                }
            }

            let fs = is_fullscreen_now();
            let mut rt = RUNTIME.lock().unwrap_or_else(|e| e.into_inner());
            let active = is_active(&h);
            match detector_step(fs, active, &mut rt, Instant::now()) {
                DetectorAction::None => {}
                DetectorAction::Enter => {
                    drop(rt);
                    let h2 = h.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = enter(&h2, Trigger::Fullscreen).await {
                            log::warn!("focus auto-enter failed: {}", e);
                        }
                    });
                }
                DetectorAction::Exit => {
                    drop(rt);
                    let h2 = h.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = exit(&h2, false).await {
                            log::warn!("focus auto-exit failed: {}", e);
                        }
                    });
                }
            }
        }
    });
}

/// 检测器单拍决策（纯函数，可单测）：输入「当前是否全屏 / 模式是否激活 /
/// 运行态 / 当前时刻」，输出本拍动作。沿触发 + 5s 防抖 + 手动退出抑制
/// 的全部语义都在这里（线程本体只负责轮询与 spawn）。
#[derive(Debug, PartialEq, Eq)]
enum DetectorAction {
    None,
    Enter,
    Exit,
}

fn detector_step(fs: bool, active: bool, rt: &mut Runtime, now: Instant) -> DetectorAction {
    if fs {
        rt.nonfs_since = None;
        if rt.fs_since.is_none() {
            // 新全屏周期开始：解除手动退出抑制
            rt.fs_since = Some(now);
            rt.suppressed = false;
        }
        if !active && !rt.suppressed && !rt.busy {
            return DetectorAction::Enter;
        }
        DetectorAction::None
    } else {
        rt.fs_since = None;
        rt.suppressed = false;
        if rt.trigger == Some(Trigger::Fullscreen) && active && !rt.busy {
            match rt.nonfs_since {
                None => {
                    rt.nonfs_since = Some(now);
                    DetectorAction::None
                }
                Some(t) if now.duration_since(t) >= Duration::from_secs(5) => {
                    // 非全屏持续 5s 才退（防抖：Alt+Tab 闪切不误退）
                    rt.nonfs_since = None;
                    DetectorAction::Exit
                }
                Some(_) => DetectorAction::None,
            }
        } else {
            rt.nonfs_since = None;
            DetectorAction::None
        }
    }
}

/// 全屏判定（启发式）：前台窗口铺满其所在显示器（容差 ≤4px），排除自家
/// 进程、系统壳与工具窗。独家全屏（D3D 游戏）与无边框全屏（播放器）都
/// 覆盖；最大化的普通窗口不算（任务栏在时最大化只铺满工作区 ≠ 显示器）。
#[cfg(target_os = "windows")]
fn is_fullscreen_now() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HWND};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect,
        GetWindowThreadProcessId, IsWindowVisible, WS_EX_TOOLWINDOW,
    };

    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return false;
        }
        // 排除自家进程（皮肤/管理器/日志窗）
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == std::process::id() {
            return false;
        }
        // 排除工具窗
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if ex & (WS_EX_TOOLWINDOW.0 as isize) != 0 {
            return false;
        }
        if !IsWindowVisible(hwnd).as_bool() {
            return false;
        }
        // 铺满其显示器？
        let mut rect = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return false;
        }
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if !GetMonitorInfoW(mon, &mut mi).as_bool() {
            return false;
        }
        let m = mi.rcMonitor;
        let tol = 4;
        let covers = (rect.left - m.left).abs() <= tol
            && (rect.top - m.top).abs() <= tol
            && (rect.right - m.right).abs() <= tol
            && (rect.bottom - m.bottom).abs() <= tol;
        if !covers {
            return false;
        }
        // 排除系统壳（桌面/任务栏/开始菜单/锁屏/搜索/输入法 UI）
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        let name = if QueryFullProcessImageNameW(
            process,
            windows::Win32::System::Threading::PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok()
        {
            let full = String::from_utf16_lossy(&buf[..len as usize]);
            full.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase()
        } else {
            String::new()
        };
        let _ = CloseHandle(process);
        !matches!(
            name.as_str(),
            "explorer.exe"
                | "lockapp.exe"
                | "shellexperiencehost.exe"
                | "searchhost.exe"
                | "startmenuexperiencehost.exe"
                | "textinputhost.exe"
                | "sihost.exe"
        )
    }
}

#[cfg(not(target_os = "windows"))]
fn is_fullscreen_now() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt() -> Runtime {
        Runtime {
            trigger: None,
            fs_since: None,
            nonfs_since: None,
            suppressed: false,
            busy: false,
        }
    }

    /// 沿触发：非全屏→全屏进入；已激活不重复进；抑制期不进
    #[test]
    fn detector_enter_edges() {
        let mut r = rt();
        let t0 = Instant::now();
        // 全屏且未激活未抑制 → 进入
        assert_eq!(
            detector_step(true, false, &mut r, t0),
            DetectorAction::Enter
        );
        // 已激活 → 不再进
        assert_eq!(detector_step(true, true, &mut r, t0), DetectorAction::None);
        // 抑制期（手动退出过）→ 不进
        r.trigger = None;
        r.suppressed = true;
        assert_eq!(detector_step(true, false, &mut r, t0), DetectorAction::None);
        // 全屏周期结束（非全屏一拍）→ 抑制解除，下一轮全屏又能进
        assert_eq!(
            detector_step(false, false, &mut r, t0),
            DetectorAction::None
        );
        assert!(!r.suppressed);
        assert_eq!(
            detector_step(true, false, &mut r, t0),
            DetectorAction::Enter
        );
    }

    /// 防抖退出：全屏档激活 + 非全屏 <5s 不退；≥5s 退一次；手动档不自动退
    #[test]
    fn detector_exit_debounce() {
        let mut r = rt();
        let t0 = Instant::now();
        r.trigger = Some(Trigger::Fullscreen);
        // 首次转非全屏：只记起点，不退
        assert_eq!(detector_step(false, true, &mut r, t0), DetectorAction::None);
        // 3s 后仍未到防抖窗
        assert_eq!(
            detector_step(false, true, &mut r, t0 + Duration::from_secs(3)),
            DetectorAction::None
        );
        // 6s 后防抖窗过 → 退
        assert_eq!(
            detector_step(false, true, &mut r, t0 + Duration::from_secs(6)),
            DetectorAction::Exit
        );
        // 退出后 trigger 由 exit() 清空（此处模拟）→ 后续不再退
        r.trigger = None;
        assert_eq!(
            detector_step(false, true, &mut r, t0 + Duration::from_secs(8)),
            DetectorAction::None
        );
    }

    /// 闪切防抖：非全屏中途又回全屏，起点作废不退
    #[test]
    fn detector_alt_tab_flicker_resets_debounce() {
        let mut r = rt();
        let t0 = Instant::now();
        r.trigger = Some(Trigger::Fullscreen);
        assert_eq!(detector_step(false, true, &mut r, t0), DetectorAction::None); // 起点
        assert_eq!(
            detector_step(true, true, &mut r, t0 + Duration::from_secs(2)),
            DetectorAction::None
        ); // 闪回全屏
        assert!(r.nonfs_since.is_none(), "回全屏后防抖起点必须作废");
        // 再转非全屏：重新计起点
        assert_eq!(
            detector_step(false, true, &mut r, t0 + Duration::from_secs(3)),
            DetectorAction::None
        );
        assert_eq!(
            detector_step(false, true, &mut r, t0 + Duration::from_secs(9)),
            DetectorAction::Exit
        );
    }

    /// 手动档激活不受自动退出影响（手动进的只能手动退）
    #[test]
    fn detector_manual_trigger_never_auto_exits() {
        let mut r = rt();
        let t0 = Instant::now();
        r.trigger = Some(Trigger::Manual);
        assert_eq!(detector_step(false, true, &mut r, t0), DetectorAction::None);
        assert_eq!(
            detector_step(false, true, &mut r, t0 + Duration::from_secs(60)),
            DetectorAction::None
        );
    }
}
