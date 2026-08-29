use tauri::{
    AppHandle, Manager,
    image::Image,
    menu::{CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState},
};
use crate::i18n::{tr, Key};

/// Tray icon artwork at every size the Windows notification area uses for
/// the standard DPI scales (100/125/150/175/200/250/300% → 16/20/24/28/32/
/// 40/48 px). All sizes are rendered from grid-snapped parametric geometry
/// by tools/make-tray-icon.py — never downscaled from a large master, which
/// was unrecognizable at tray sizes.
///
/// `Image::from_bytes` decodes only a single frame, so a multi-size .ico
/// gains nothing here — we pick the exact size ourselves at runtime instead
/// (see `tray_icon_bytes`). Without this, Windows rescales one bitmap to
/// 20/24 px on 125%/150% displays and the icon goes soft.
#[cfg(target_os = "windows")]
const TRAY_ICON_SIZES: &[(u32, &[u8])] = &[
    (16, include_bytes!("../icons/tray-16.png")),
    (20, include_bytes!("../icons/tray-20.png")),
    (24, include_bytes!("../icons/tray-24.png")),
    (28, include_bytes!("../icons/tray-28.png")),
    (32, include_bytes!("../icons/tray-32.png")),
    (40, include_bytes!("../icons/tray-40.png")),
    (48, include_bytes!("../icons/tray-48.png")),
];

/// Pick the PNG matching the notification area's current icon size:
/// SM_CXSMICON tracks the primary display's DPI (16 px @100%, 20 @125%,
/// 24 @150%…). Exact match first, else the smallest larger size, else the
/// largest we have. Resolved once at tray creation — a system DPI change
/// takes effect after an app restart.
#[cfg(target_os = "windows")]
fn tray_icon_bytes() -> &'static [u8] {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSMICON};
    let target = unsafe { GetSystemMetrics(SM_CXSMICON) } as u32;
    let idx = TRAY_ICON_SIZES
        .iter()
        .position(|(size, _)| *size >= target)
        .unwrap_or(TRAY_ICON_SIZES.len() - 1);
    TRAY_ICON_SIZES[idx].1
}

/// Other platforms show the icon at one size; hand them the 32 px artwork.
#[cfg(not(target_os = "windows"))]
fn tray_icon_bytes() -> &'static [u8] {
    include_bytes!("../icons/tray-32.png")
}

/// Fixed tray id so set_language can find the icon and rebuild its menu.
const TRAY_ID: &str = "main-tray";

/// Build the tray menu in the given language.
fn build_menu(app: &AppHandle, lang: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let show_manager = MenuItemBuilder::new(tr(lang, Key::TrayShowManager))
        .id("show_manager")
        .build(app)?;

    let reload_all = MenuItemBuilder::new(tr(lang, Key::TrayReloadAll))
        .id("reload_all")
        .build(app)?;

    // Checked while every skin window is hidden (via the global hotkey or
    // this item itself). The handle is stashed in AppState so the hotkey
    // path can keep the checkmark in sync.
    let toggle_skins = CheckMenuItemBuilder::new(tr(lang, Key::TrayToggleSkins))
        .id("toggle_skins")
        .checked(crate::hotkey::all_skins_hidden(app))
        .build(app)?;
    *app.state::<crate::AppState>().toggle_item.lock().unwrap_or_else(|e| e.into_inner()) = Some(toggle_skins.clone());

    let separator = tauri::menu::PredefinedMenuItem::separator(app)?;

    let quit = MenuItemBuilder::new(tr(lang, Key::TrayQuit))
        .id("quit")
        .build(app)?;

    // 「皮肤显隐」子菜单：每个已加载皮肤一个勾选项（勾选 = 窗口可见），
    // 按显示名排序（双语皮肤随语言取 name/name_en）；句柄 stash 进
    // AppState.skin_vis_items 供 sync_tray_toggle_item 勾选同步/加载集
    // 比对。空集时只放一行禁用占位（空子菜单观感差）
    let skins_submenu = build_skins_submenu(app, lang)?;

    // 「布局方案」子菜单：每个方案一项，点击即应用（布局变更后
    // capture_layout/set_layouts 会重建托盘菜单同步本清单）
    let layouts_submenu = build_layouts_submenu(app, lang)?;

    MenuBuilder::new(app)
        .item(&show_manager)
        .item(&reload_all)
        .item(&toggle_skins)
        .item(&skins_submenu)
        .item(&layouts_submenu)
        .item(&separator)
        .item(&quit)
        .build()
}

/// 构建「布局方案」子菜单（无方案时一行禁用占位）
fn build_layouts_submenu(app: &AppHandle, lang: &str) -> tauri::Result<tauri::menu::Submenu<tauri::Wry>> {
    let state = app.state::<crate::AppState>();
    let layouts = {
        let cfg = state.config.lock().unwrap_or_else(|e| e.into_inner());
        cfg.layouts.clone()
    };
    let mut builder = SubmenuBuilder::new(app, tr(lang, Key::TrayLayouts));
    if layouts.is_empty() {
        let empty = MenuItemBuilder::new(tr(lang, Key::TrayLayoutsEmpty))
            .enabled(false)
            .build(app)?;
        return builder.item(&empty).build();
    }
    for l in layouts {
        let item = MenuItemBuilder::new(l.name)
            .id(format!("layout:{}", l.id))
            .build(app)?;
        builder = builder.item(&item);
    }
    builder.build()
}

/// 构建「皮肤显隐」子菜单并 stash 每皮肤勾选项句柄
fn build_skins_submenu(app: &AppHandle, lang: &str) -> tauri::Result<tauri::menu::Submenu<tauri::Wry>> {
    let state = app.state::<crate::AppState>();
    let loaded: std::collections::HashSet<String> =
        state.registry.loaded_ids().into_iter().collect();

    let mut builder = SubmenuBuilder::new(app, tr(lang, Key::TraySkinsMenu));
    let mut vis_items = state.skin_vis_items.lock().unwrap_or_else(|e| e.into_inner());
    vis_items.clear();

    if loaded.is_empty() {
        let empty = MenuItemBuilder::new(tr(lang, Key::TraySkinsEmpty))
            .enabled(false)
            .build(app)?;
        return builder.item(&empty).build();
    }

    // 已加载皮肤按显示名排序（名称取自 manifest；扫描按文件夹名稳定序后
    // 再按名称排，跨语言下顺序确定）
    let mut named: Vec<(String, String)> = crate::skin::loader::scan_skins_directory(&state.skins_dir)
        .into_iter()
        .filter(|s| loaded.contains(&s.id))
        .map(|s| {
            // 显示名走 SkinManifest::display_name 单一口源（界面语言优先、
            // 缺失回退另一语言——单语言皮肤两种界面都显示其提供的文案）
            (s.id, s.manifest.display_name(&lang))
        })
        .collect();
    named.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));

    // 扫描不到但已加载的（文件夹被外部删除的僵尸窗口）：以 id 兜底名称
    // ——漏掉它们会让「加载集 ≠ 菜单项集合」恒成立，每次可见性同步都
    // 误触发整菜单重建
    for id in &loaded {
        if !named.iter().any(|(nid, _)| nid == id) {
            named.push((id.clone(), id.clone()));
        }
    }

    for (id, name) in named {
        let visible = state
            .registry
            .get(&id)
            .map(|w| w.is_visible().unwrap_or(true))
            .unwrap_or(false);
        let item = CheckMenuItemBuilder::new(name)
            .id(format!("skinvis:{}", id))
            .checked(visible)
            .build(app)?;
        vis_items.insert(id, item.clone());
        builder = builder.item(&item);
    }
    builder.build()
}

/// Rebuild the tray menu and tooltip after a language change.
pub fn rebuild_tray_menu(app: &AppHandle, lang: &str) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    match build_menu(app, lang) {
        Ok(menu) => {
            if let Err(e) = tray.set_menu(Some(menu)) {
                log::warn!("Failed to rebuild tray menu: {}", e);
            }
        }
        Err(e) => log::warn!("Failed to build tray menu: {}", e),
    }
    let _ = tray.set_tooltip(Some(tr(lang, Key::TrayTooltip)));
}

/// Create and configure the system tray icon
pub fn create_tray(app: &AppHandle) -> Result<(), String> {
    let lang = app.state::<crate::AppState>().lang();
    let menu = build_menu(app, &lang).map_err(|e| e.to_string())?;

    let _tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(tray_icon_bytes()).map_err(|e| e.to_string())?)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip(tr(&lang, Key::TrayTooltip))
        .on_menu_event(move |app, event| {
            let id = event.id().as_ref();
            match id {
                "show_manager" => {
                    show_manager_window(app);
                }
                "reload_all" => {
                    reload_all_skins(app);
                }
                "toggle_skins" => {
                    crate::hotkey::toggle_all_skins(app);
                }
                "quit" => {
                    graceful_exit(app);
                }
                other => {
                    // 「皮肤显隐」子菜单项：skinvis:<skin_id> → 切换该皮肤显隐
                    //（与皮肤专属热键同一路径；勾选态由 sync_tray_toggle_item
                    // 漏斗按真实窗口状态回写）
                    if let Some(skin_id) = other.strip_prefix("skinvis:") {
                        crate::hotkey::toggle_one_skin(app, skin_id);
                    } else if let Some(layout_id) = other.strip_prefix("layout:") {
                        // 「布局方案」子菜单项：layout:<layout_id> → 应用该方案
                        let app2 = app.clone();
                        let lid = layout_id.to_string();
                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = crate::commands::apply_layout_impl(&app2, &lid).await {
                                log::warn!("tray apply layout failed: {}", e);
                            }
                        });
                    }
                }
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                toggle_manager_window(app);
            }
        })
        .build(app)
        .map_err(|e| e.to_string())?;

    Ok(())
}

fn toggle_manager_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false) {
            let _ = window.hide();
        } else {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}

pub(crate) fn show_manager_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn reload_all_skins(app: &AppHandle) {
    let handle = app.clone();
    // Reuse the manager's per-skin reload path (commands::reload_skin_impl =
    // unload + load as two separately-awaited blocking steps).  The old
    // synchronous destroy+create loop ran entirely on this (main) thread:
    // the old window's label is only freed once the event loop processes
    // the close, so create_skin_window failed with a duplicate label and
    // the error was swallowed — every skin window disappeared until a
    // manual reload.
    tauri::async_runtime::spawn(async move {
        // 与备份导入/包安装互斥：它们整树替换 skins/ 期间重载会在半替换
        // 状态的目录上扫描/建窗（可与安装方同排队，自愈但避免无谓报错）
        let state = handle.state::<crate::AppState>();
        let _install_guard = state.install_lock.lock().await;
        // 快照在拿到锁之后再取：排队等待期间被卸载的皮肤会被
        // reload_skin_impl 当「未加载→直接 load」重新拉起
        for skin_id in state.registry.loaded_ids() {
            if let Err(e) = crate::commands::reload_skin_impl(handle.clone(), skin_id.clone()).await {
                log::error!("reload_all_skins: failed to reload '{}': {}", skin_id, e);
            }
        }
    });
}

/// Gracefully shut down: signal real exit so the main window actually closes
/// instead of hiding to tray, then close all windows and exit.
/// pub(crate)：自动更新的「立即安装」也走这条退出路径（启动安装器后
/// 整站退出，让 NSIS 能覆盖 exe）。
pub(crate) fn graceful_exit(app: &AppHandle) {
    log::info!("Graceful exit: closing all windows...");

    let state = app.state::<crate::AppState>();

    // 1. Signal that this is a real exit — the main window's CloseRequested
    //    handler will let the close go through instead of hiding to tray.
    state.exiting.store(true, std::sync::atomic::Ordering::SeqCst);

    // 2. Close every loaded skin window.  No per-window teardown wait here —
    //    destroy_skin_window's polling exists for same-label recreation on
    //    reload; on exit nothing is recreated, and the sequential waits were
    //    the visible stutter in tray quit.
    let loaded_ids = state.registry.loaded_ids();
    for skin_id in &loaded_ids {
        let label = crate::window::factory::skin_window_label(skin_id);
        let _ = crate::window::factory::close_skin_window_nowait(app, &label);
        state.registry.unregister(skin_id);
    }
    log::info!("Closed {} skin window(s)", loaded_ids.len());

    // 3. Close the manager window (this time prevent_close won't fire).
    if let Some(main_win) = app.get_webview_window("main") {
        let _ = main_win.close();
    }

    // 3.5 拖动防抖的末次落盘可能还在等定时器——同步 flush 后再退，
    //     否则「拖完皮肤立即退托盘」会丢最终位置/尺寸（内存已更新、磁盘未写）。
    crate::window::factory::flush_pending_drag_saves(app);

    // 4. Now exit. All WebviewWindow handles have been dropped / close() called,
    //    so WebView2 has had time to begin its async teardown.
    //    We call exit(0) on the current (main) thread — not a spawned thread.
    app.exit(0);
}
