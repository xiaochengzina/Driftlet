mod commands;
mod desktop;
mod hotkey;
mod i18n;
mod skin;
mod skin_api;
mod tray;
mod window;
mod capture;

// Re-export the skin protocol registration helper.
pub use skin::protocol;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use skin::config;
use skin::loader;
use skin::types::AppConfig;
use window::registry::SkinWindowRegistry;
use window::factory;
use desktop::Pinner;

/// Shared application state accessible from all commands
pub struct AppState {
    pub registry: SkinWindowRegistry,
    pub config: Mutex<AppConfig>,
    pub config_dir: PathBuf,
    pub skins_dir: PathBuf,
    /// Set to true when the user requests a real exit (tray "quit")
    /// so the main window close handler knows not to hide-to-tray.
    pub exiting: AtomicBool,
    /// Set to true once the system tray is created successfully.  When the
    /// tray is missing, closing the main window must exit the app instead of
    /// hiding — there is no other UI entry to bring the window back.
    pub tray_ok: AtomicBool,
    /// Desktop pinner (Driftlet-style z-order helper window + Show Desktop hook)
    pub pinner: Pinner,
    /// ThreadId of the event-loop (main) thread.  Win32 window work such as
    /// SetWindowSubclass must run on this thread — see window::factory.
    pub main_thread_id: std::thread::ThreadId,
    /// .dskin package passed on the command line at cold start (double-click
    /// install).  Stored here because the frontend may not be ready to
    /// receive an event yet — it pulls this once on startup.
    pub pending_package: Mutex<Option<String>>,
    /// Serializes package installs: a second double-click install while one
    /// is still running would otherwise race remove_dir_all vs copy on the
    /// same skin directory (IO error, not a hang — but let's be correct).
    /// tokio Mutex：guard 是 Send，可以持有跨过 .await（装完还要 reload）。
    pub install_lock: tauri::async_runtime::Mutex<()>,
    /// Serializes load-modify-save on a skin folder's `settings.json`, which
    /// has two writers: the manager (`set_skin_custom_setting`) and the skin
    /// itself (`skin_api::skin_set_setting`).  std Mutex is fine — both
    /// commands are sync fns, the guard never crosses an .await.
    pub settings_lock: Mutex<()>,
    /// Current UI language ("zh-CN" | "en"), mirrored from config.language.
    /// Read by every command that produces user-facing strings; updated by
    /// the set_language command (which also rebuilds the tray menu).
    pub language: Mutex<String>,
    /// Tray "hide all skins" check item, kept so the global-hotkey toggle
    /// can sync its checked state. Replaced whenever the tray menu is
    /// rebuilt (language switch).
    pub toggle_item: Mutex<Option<tauri::menu::CheckMenuItem<tauri::Wry>>>,
    /// Startup hotkey registration failure (the configured combo, e.g.
    /// "Ctrl+Alt+D"). Pulled once by the frontend on init so the user sees
    /// a toast instead of a silent log — mirrors pending_package.
    pub hotkey_error: Mutex<Option<String>>,
}

impl AppState {
    /// Snapshot of the current UI language ("zh-CN" | "en").
    pub fn lang(&self) -> String {
        // Mutex 中毒不等于数据损坏：取回内部值继续运行
        self.language.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        // Must stay the first plugin: a second instance launched by
        // double-clicking a .dskin forwards its args here and exits.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if let Some(path) = dskin_arg(args.into_iter()) {
                log::info!("Second instance handed us a skin package: {}", path);
                tray::show_manager_window(app);
                let _ = app.emit("open-skin-package", path);
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::default(),
            None::<Vec<&str>>,
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        // Global hotkey hides/shows all skin windows. Only the Pressed edge
        // acts — the handler fires on release too, which would double-toggle.
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        hotkey::toggle_all_skins(app);
                    }
                })
                .build(),
        )
        .register_uri_scheme_protocol("skin", skin::protocol::handle_skin_request)
        .setup(|app| {
            // Unpackaged-app toast prerequisite #1: give the process an
            // AppUserModelID (skin_api::notify relies on it — its shortcut
            // carries the same ID).
            #[cfg(target_os = "windows")]
            unsafe {
                let _ = windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(
                    windows::core::w!("Driftlet"),
                );
            }
            // Unpackaged-app toast prerequisite #2: the AUMID shortcut must
            // exist and point at this exe (its icon feeds the taskbar too).
            // MUST run off the main thread: it CoInitializeEx(MTA)s, and an
            // MTA main thread breaks tao's OleInitialize (STA) at window
            // creation — RPC_E_CHANGED_MODE panic (seen live).
            #[cfg(target_os = "windows")]
            std::thread::spawn(skin_api::ensure_notification_identity);

            // Set up directories — portable layout: all app data lives next
            // to the executable, so the install location the user picks in
            // the installer decides where everything is stored.
            let app_data_dir = app.path().app_data_dir()
                .unwrap_or_else(|e| {
                    fatal_startup_error(&format!("Failed to get app data directory: {}", e))
                });
            let skins_dir = resolve_portable_dir(&app_data_dir, "skins");
            let config_dir = resolve_portable_dir(&app_data_dir, "config");

            // One-time migration of a config left by older versions in %APPDATA%.
            migrate_legacy_config(&app_data_dir, &config_dir, &skins_dir);

            // Copy example skins if the skins directory is empty
            // This helps new users see the example clock
            copy_example_skins(&skins_dir);

            // Scan skins
            let skins = loader::scan_skins_directory(&skins_dir);
            log::info!("Found {} skins in {:?}", skins.len(), skins_dir);

            // v1→v2 迁移：皮肤设置页用户值从 config.json 的
            // skin_settings[id].custom 迁到各皮肤文件夹的 settings.json。
            // 必须在 load_config 之前跑，之后读入的配置即无 custom 键。
            config::migrate_v1_custom_settings(
                &config_dir,
                &skins.iter()
                    .map(|s| (s.id.clone(), s.directory.clone()))
                    .collect::<Vec<_>>(),
            );

            // Load config
            let mut app_config = config::load_config(&config_dir);

            // Prune persisted entries for skins that no longer exist on disk
            // (folder deleted outside the app, or the author changed the id) —
            // otherwise residue accumulates in config.json forever.
            {
                let valid: std::collections::HashSet<&str> =
                    skins.iter().map(|s| s.id.as_str()).collect();
                let settings_before = app_config.skin_settings.len();
                let loaded_before = app_config.loaded_skins.len();
                app_config.skin_settings.retain(|id, _| valid.contains(id.as_str()));
                app_config.loaded_skins.retain(|id| valid.contains(id.as_str()));
                let removed = (settings_before - app_config.skin_settings.len())
                    + (loaded_before - app_config.loaded_skins.len());
                if removed > 0 {
                    log::info!("Pruned {} config entries of missing skins", removed);
                    if let Err(e) = config::save_config(&config_dir, &app_config) {
                        log::warn!("Failed to save pruned config: {}", e);
                    }
                }
            }

            // Manage state BEFORE auto-load so apply_on_desktop
            // can access pinner via app.state()
            let main_thread_id = std::thread::current().id();
            // Cold-start double-click install: the frontend pulls this
            // via take_pending_package_install once it is ready.
            let pending_package = dskin_arg(std::env::args());
            let language = app_config.language.clone();
            app.manage(AppState {
                registry: SkinWindowRegistry::new(),
                config: Mutex::new(app_config),
                config_dir: config_dir.clone(),
                skins_dir: skins_dir.clone(),
                exiting: AtomicBool::new(false),
                tray_ok: AtomicBool::new(false),
                pinner: Pinner::new(app.handle().clone(), main_thread_id),
                main_thread_id,
                pending_package: Mutex::new(pending_package.clone()),
                install_lock: tauri::async_runtime::Mutex::new(()),
                settings_lock: Mutex::new(()),
                language: Mutex::new(language),
                toggle_item: Mutex::new(None),
                hotkey_error: Mutex::new(None),
            });

            // Auto-load previously loaded skins
            // Read loaded_skins list first (drop lock before creating windows)
            let to_load: Vec<String> = {
                app.handle().state::<AppState>()
                    .config.lock().unwrap_or_else(|e| e.into_inner())
                    .loaded_skins.clone()
            };

            for skin_id in &to_load {
                if let Some(skin) = skins.iter().find(|s| &s.id == skin_id) {
                    // Read saved config (drop lock before creating window)
                    let skin_config = {
                        app.handle().state::<AppState>()
                            .config.lock().unwrap_or_else(|e| e.into_inner())
                            .skin_settings.get(skin_id).cloned()
                            .unwrap_or_else(|| skin::types::SkinRuntimeConfig::from_manifest(&skin.manifest))
                    };

                    match factory::create_skin_window(app.handle(), skin, &skin_config) {
                        Ok(window) => {
                            app.handle().state::<AppState>()
                                .registry.register(skin_id.clone(), window);
                            log::info!("Auto-loaded skin: {}", skin_id);
                        }
                        Err(e) => {
                            log::warn!("Failed to auto-load skin '{}': {}", skin_id, e);
                        }
                    }
                }
            }

            // Create manager window — center on screen, hidden on startup.
            // User opens it from the tray icon.
            // frameless with custom title bar (min/max/close buttons in UI)
            let manager = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("Driftlet")
            .inner_size(960.0, 640.0)
            .min_inner_size(640.0, 460.0)
            .decorations(false)
            .shadow(false)
            .resizable(true)
            .center()
            .visible(false)
            .build()
            .unwrap_or_else(|e| {
                fatal_startup_error(&format!("Failed to create manager window: {}", e))
            });

            // Apply rounded corners (Win11 DWM) on the frameless window
            #[cfg(target_os = "windows")]
            {
                if let Ok(hwnd) = manager.hwnd() {
                    round_window_corners(hwnd.0 as isize);
                }
            }

            // Double-click install: a package was passed on the command
            // line, so show the manager window right away — the frontend
            // will open the install wizard as soon as it loads.
            if pending_package.is_some() {
                log::info!("Opened with skin package: {:?}", pending_package);
                let _ = manager.show();
                let _ = manager.set_focus();
            }

            // Window event handler: close-to-tray
            let h = app.handle().clone();
            manager.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    let state = h.state::<AppState>();
                    if state.exiting.load(std::sync::atomic::Ordering::SeqCst) {
                        // Real exit
                    } else if !state.tray_ok.load(std::sync::atomic::Ordering::SeqCst) {
                        // 托盘没建起来：隐藏到托盘会彻底失去 UI 入口 ——
                        // 关窗直接走退出流程
                        state.exiting.store(true, std::sync::atomic::Ordering::SeqCst);
                        h.exit(0);
                    } else {
                        api.prevent_close();
                        let _ = h.get_webview_window("main").map(|w| w.hide());
                    }
                }
            });

            // Set up system tray
            match tray::create_tray(app.handle()) {
                Ok(()) => {
                    app.handle().state::<AppState>()
                        .tray_ok.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                Err(e) => {
                    log::warn!("Failed to create system tray: {}", e);
                }
            }

            // Register the configured global hotkey (failures only log).
            hotkey::register_from_config(app.handle());

            // Periodic frameless maintenance timer.
            //
            // SetWindowSubclass only works on the window's OWNER thread, and
            // a failed/somehow-removed subclass leaves the window with tao's
            // WS_CAPTION style — DWM then draws the classic frame (the
            // intermittent title-bar bug).  Every 5 seconds, on the event-loop
            // thread: idempotently re-install the subclass on every skin
            // window (self-healing), then post a deferred cleanup.  The
            // cleanup itself is a near-no-op when styles are clean (no
            // SWP_FRAMECHANGED storm → DWM is never re-triggered).
            {
                let h = app.handle().clone();
                std::thread::spawn(move || {
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(5));
                        let state = h.state::<AppState>();
                        if state.exiting.load(std::sync::atomic::Ordering::Relaxed) {
                            break;
                        }
                        let windows = state.registry.all_hwnds();
                        if windows.is_empty() {
                            continue;
                        }
                        #[cfg(target_os = "windows")]
                        {
                            let h2 = h.clone();
                            let _ = h.run_on_main_thread(move || {
                                for (skin_id, hwnd) in &windows {
                                    factory::ensure_frameless_subclass(*hwnd);
                                    factory::force_clean_skin_window_by_hwnd(*hwnd);
                                    // Keep WebView2's default context menu
                                    // disabled (self-healing; the creation-time
                                    // retry in factory only covers startup).
                                    if let Some(window) = h2.get_webview_window(
                                        &factory::skin_window_label(skin_id),
                                    ) {
                                        factory::disable_default_context_menu(&window);
                                    }
                                }
                            });
                        }
                        #[cfg(not(target_os = "windows"))]
                        let _ = windows;
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_skin_drag,
            commands::start_skin_resize,
            commands::list_skins,
            commands::get_skin_detail,
            commands::refresh_skins,
            commands::load_skin,
            commands::unload_skin,
            commands::reload_skin,
            commands::set_skin_opacity,
            commands::set_skin_always_on_top,
            commands::set_skin_on_desktop,
            commands::set_skin_position_locked,
            commands::set_skin_resizable,
            commands::set_skin_zoom,
            commands::set_skin_edge_snap,
            commands::set_skin_snap_gap,
            commands::set_skin_position,
            commands::show_skin_context_menu,
            commands::set_skin_size,
            commands::update_skin_config,
            commands::set_skin_custom_setting,
            commands::reset_skin_config,
            commands::pick_skin_folder,
            commands::install_skin,
            commands::pick_skin_package,
            commands::inspect_skin_package,
            commands::install_skin_package,
            commands::remove_skin,
            commands::get_app_config,
            commands::save_app_config,
            commands::set_autostart,
            commands::get_autostart,
            commands::set_theme,
            commands::set_language,
            commands::set_hotkey,
            commands::take_hotkey_error,
            commands::is_windows_11_or_newer,
            commands::open_skins_folder,
            commands::open_skin_folder,
            commands::get_system_stats,
            commands::list_system_fonts,
            commands::capture_skin_preview,
            commands::take_pending_package_install,
            skin_api::get_cpu_info,
            skin_api::get_gpu_info,
            skin_api::get_memory_info,
            skin_api::get_disks_info,
            skin_api::get_disk_space,
            skin_api::get_network_info,
            skin_api::get_public_ip,
            skin_api::get_audio_spectrum,
            skin_api::skin_read_file,
            skin_api::skin_write_file,
            skin_api::skin_list_dir,
            skin_api::skin_delete_file,
            skin_api::skin_set_setting,
            skin_api::skin_get_setting,
            skin_api::read_registry_value,
            skin_api::run_command,
            skin_api::get_os_info,
            skin_api::get_processes,
            skin_api::get_volume,
            skin_api::set_volume,
            skin_api::set_mute,
            skin_api::get_media_info,
            skin_api::media_control,
            skin_api::read_clipboard_text,
            skin_api::write_clipboard_text,
            skin_api::open_external,
            skin_api::show_notification,
            skin_api::get_mic_spectrum,
            skin_api::get_battery_info,
            skin_api::get_idle_time,
            skin_api::get_foreground_window_info,
            skin_api::get_monitors,
        ])
        .run(tauri::generate_context!())
        .expect("Error while running Driftlet");
}

/// Find the first .dskin package path in command-line arguments.
/// argv[0] (the exe path) is skipped; the extension check is
/// case-insensitive and the file must exist.
fn dskin_arg(args: impl Iterator<Item = String>) -> Option<String> {
    args.skip(1).find(|arg| {
        std::path::Path::new(arg)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("dskin"))
            && std::path::Path::new(arg).is_file()
    })
}

/// 启动阶段的致命错误：release 是无控制台 GUI，panic 等于静默闪退 ——
/// 记录日志并弹出可读错误框，然后退出进程
#[cfg(target_os = "windows")]
fn fatal_startup_error(msg: &str) -> ! {
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    log::error!("Fatal startup error: {}", msg);
    let text = windows::core::HSTRING::from(msg);
    unsafe {
        let _ = MessageBoxW(
            None,
            windows::core::PCWSTR(text.as_ptr()),
            windows::core::w!("Driftlet"),
            MB_OK | MB_ICONERROR,
        );
    }
    std::process::exit(1);
}

#[cfg(not(target_os = "windows"))]
fn fatal_startup_error(msg: &str) -> ! {
    log::error!("Fatal startup error: {}", msg);
    eprintln!("Fatal startup error: {}", msg);
    std::process::exit(1);
}

/// Resolve a data directory as `<exe dir>/<name>` (portable layout).
///
/// All app data lives next to the executable, so the install location the
/// user picks in the installer is the single place holding the app and its
/// data.  If the directory cannot be created (e.g. the app was installed
/// into a protected location such as Program Files), fall back to the
/// per-user app-data directory so the app stays usable.
fn resolve_portable_dir(app_data_dir: &std::path::Path, name: &str) -> PathBuf {
    let fallback = app_data_dir.join(name);

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));
    let Some(exe_dir) = exe_dir else {
        log::warn!("Cannot resolve exe path, {} dir falls back to {:?}", name, fallback);
        let _ = std::fs::create_dir_all(&fallback);
        return fallback;
    };

    let dir = exe_dir.join(name);
    // create_dir_all 对已存在但只读的目录（如 Program Files 下安装器预置的
    // skins/）也返回 Ok —— 必须实际写入一个隐藏临时文件来验证可写性
    match probe_writable_dir(&dir) {
        Ok(()) => {
            log::info!("{} directory: {:?}", name, dir);
            dir
        }
        Err(e) => {
            log::warn!(
                "Cannot write {} dir {:?} ({}), falling back to {:?}",
                name, dir, e, fallback
            );
            let _ = std::fs::create_dir_all(&fallback);
            fallback
        }
    }
}

/// 确保目录存在且可写：创建目录后写入再删除一个隐藏临时文件
fn probe_writable_dir(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let probe = dir.join(".driftlet-write-probe");
    std::fs::write(&probe, b"")?;
    std::fs::remove_file(&probe)?;
    Ok(())
}

/// One-time migration: older versions kept the config and skins in %APPDATA%.
/// If the portable config does not exist yet but a legacy one does, copy it
/// over so settings survive the move; legacy skin folders are moved into the
/// portable skins directory (an existing same-name skin is kept, not
/// overwritten).  Must run before the skin scan and the startup prune —
/// otherwise upgraded users lose their skins and the prune would then drop
/// their config entries too.
fn migrate_legacy_config(
    app_data_dir: &std::path::Path,
    config_dir: &std::path::Path,
    skins_dir: &std::path::Path,
) {
    let legacy = app_data_dir.join("config").join("config.json");
    let current = config_dir.join("config.json");
    if !current.exists() && legacy.exists() && legacy != current {
        match std::fs::copy(&legacy, &current) {
            Ok(_) => log::info!("Migrated legacy config from {:?}", legacy),
            Err(e) => log::warn!("Failed to migrate legacy config: {}", e),
        }
    }

    // 旧版 skins 在 %APPDATA%/<app>/skins：逐个移动皮肤文件夹到便携 skins
    // 目录。便携目录本身就回退到 %APPDATA% 时两者相同，无需迁移。
    let legacy_skins = app_data_dir.join("skins");
    if let (Ok(a), Ok(b)) = (legacy_skins.canonicalize(), skins_dir.canonicalize()) {
        if a == b {
            return;
        }
    }
    let entries = match std::fs::read_dir(&legacy_skins) {
        Ok(entries) => entries,
        Err(_) => return, // 没有旧 skins 目录：无需迁移
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let dest = skins_dir.join(entry.file_name());
        if dest.exists() {
            log::info!("Legacy skin {:?} skipped: {:?} already exists", entry.file_name(), dest);
            continue;
        }
        // 优先 rename；跨卷（如装在 D:\ 而 %APPDATA% 在 C:\）退化为复制+删除
        let migrated = std::fs::rename(&path, &dest).is_ok() || {
            copy_dir_recursive(&path, &dest)
                .and_then(|_| std::fs::remove_dir_all(&path))
                .is_ok()
        };
        if migrated {
            log::info!("Migrated legacy skin {:?}", entry.file_name());
        } else {
            log::warn!("Failed to migrate legacy skin {:?}", entry.file_name());
        }
    }
}

/// Sync example skins into the skins directory (development only).
/// 示例皮肤只随仓库分发（`examples/`，以独立 .dskin 另行发布），安装包不
/// 打包——生产环境没有示例源，本函数直接打日志返回。
/// Skins that don't exist yet are copied; existing skins are updated if the source is newer.
fn copy_example_skins(skins_dir: &PathBuf) {
    // 开发期的示例皮肤源（仓库 examples/ 目录）
    let example_sources: Vec<PathBuf> = vec![
        // Development: check ../examples (when running from src-tauri/)
        PathBuf::from("../examples"),
        // Development: check ./examples (when running from project root)
        PathBuf::from("examples"),
    ];

    let canonical_dest = skins_dir.canonicalize().ok();

    for source in &example_sources {
        if !source.exists() || !source.is_dir() {
            continue;
        }
        // Skip when the source IS the skins directory itself — in the
        // production layout the bundled example skins already live next to
        // the executable, so there is nothing to copy.
        if let (Some(dest), Ok(src)) = (&canonical_dest, source.canonicalize()) {
            if src == *dest {
                continue;
            }
        }
        log::info!("Syncing example skins from {:?}", source);
        if let Ok(entries) = std::fs::read_dir(source) {
            for entry in entries.flatten() {
                let dest = skins_dir.join(entry.file_name());
                if !dest.exists() {
                    let _ = copy_dir_recursive(&entry.path(), &dest);
                    log::info!("  Installed example skin: {:?}", entry.file_name());
                } else if source_is_newer(&entry.path(), &dest) {
                    // 更新示例皮肤时保留用户设置值：settings.json 先取出，
                    // 拷贝后写回（与 package.rs 的安装保留同一约定）
                    let saved_settings =
                        std::fs::read(dest.join(skin::settings::SETTINGS_FILENAME)).ok();
                    let _ = std::fs::remove_dir_all(&dest);
                    let _ = copy_dir_recursive(&entry.path(), &dest);
                    if let Some(bytes) = saved_settings {
                        let _ = std::fs::write(
                            dest.join(skin::settings::SETTINGS_FILENAME),
                            bytes,
                        );
                    }
                    log::info!("  Updated example skin: {:?}", entry.file_name());
                }
            }
        }
        return; // Successfully found and synced
    }
    log::info!("No example skins found to copy");
}

/// Check if any file in `src` has a newer modification time than the corresponding file in `dst`.
fn source_is_newer(src: &std::path::Path, dst: &std::path::Path) -> bool {
    if !dst.exists() {
        return true;
    }
    if src.is_file() {
        return file_modified(src) > file_modified(dst);
    }
    if src.is_dir() {
        if let Ok(entries) = std::fs::read_dir(src) {
            for entry in entries.flatten() {
                let dst_path = dst.join(entry.file_name());
                if source_is_newer(&entry.path(), &dst_path) {
                    return true;
                }
            }
        }
    }
    false
}

fn file_modified(path: &std::path::Path) -> std::time::SystemTime {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
}

#[cfg(target_os = "windows")]
fn round_window_corners(hwnd_val: isize) {
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute,
        DWMWA_WINDOW_CORNER_PREFERENCE, DWM_WINDOW_CORNER_PREFERENCE,
    };
    use windows::Win32::Foundation::HWND;

    let h = HWND(hwnd_val as *mut _);
    // DWMWCP_ROUND = 2 — enable rounded corners (Win11 only, no-op on Win10)
    let corner_pref = DWM_WINDOW_CORNER_PREFERENCE(2);
    unsafe {
        let _ = DwmSetWindowAttribute(
            h,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner_pref as *const _ as *const std::ffi::c_void,
            std::mem::size_of_val(&corner_pref) as u32,
        );
    }
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{dskin_arg, migrate_legacy_config};

    fn args(v: &[&str]) -> impl Iterator<Item = String> {
        v.iter().map(|s| s.to_string()).collect::<Vec<_>>().into_iter()
    }

    #[test]
    fn skips_argv0() {
        // argv0 即使是 .dskin 后缀也不会被当成参数（直接跳过，不看存在性）
        assert_eq!(dskin_arg(args(&["anything.dskin"])), None);
    }

    #[test]
    fn finds_existing_dskin_case_insensitive() {
        let dir = std::env::temp_dir().join(format!("driftlet-argtest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pkg = dir.join("My Skin.DSKIN");
        std::fs::write(&pkg, b"").unwrap();

        let found = dskin_arg(args(&["app", pkg.to_str().unwrap()]));
        assert_eq!(found.as_deref(), pkg.to_str());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_missing_or_non_dskin() {
        assert_eq!(dskin_arg(args(&["app", "no-such-file.dskin"])), None);
        assert_eq!(dskin_arg(args(&["app", "README.md"])), None);
        assert_eq!(dskin_arg(args(&["app"])), None);
    }

    fn unique_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "driftlet-legacy-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn legacy_migration_moves_skins_and_config() {
        // 旧版布局：%APPDATA%/<app>/{config/config.json, skins/<id>}
        let app_data = unique_dir("appdata");
        let legacy_config_dir = app_data.join("config");
        std::fs::create_dir_all(&legacy_config_dir).unwrap();
        std::fs::write(legacy_config_dir.join("config.json"), r#"{"version":2}"#).unwrap();
        let legacy_skins = app_data.join("skins");
        std::fs::create_dir_all(legacy_skins.join("old-skin")).unwrap();
        std::fs::write(legacy_skins.join("old-skin").join("index.html"), "old").unwrap();
        std::fs::create_dir_all(legacy_skins.join("clash-skin")).unwrap();
        std::fs::write(legacy_skins.join("clash-skin").join("index.html"), "legacy").unwrap();

        // 便携布局：config/skins 已就位，且已有同名皮肤（保留新者）
        let portable = unique_dir("portable");
        let config_dir = portable.join("config");
        let skins_dir = portable.join("skins");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(skins_dir.join("clash-skin")).unwrap();
        std::fs::write(skins_dir.join("clash-skin").join("index.html"), "new").unwrap();

        migrate_legacy_config(&app_data, &config_dir, &skins_dir);

        // config.json 被复制；旧皮肤被移入便携目录；同名冲突保留新者
        assert_eq!(
            std::fs::read_to_string(config_dir.join("config.json")).unwrap(),
            r#"{"version":2}"#
        );
        assert_eq!(
            std::fs::read_to_string(skins_dir.join("old-skin").join("index.html")).unwrap(),
            "old"
        );
        assert!(!legacy_skins.join("old-skin").exists(), "moved skin must leave the legacy dir");
        assert_eq!(
            std::fs::read_to_string(skins_dir.join("clash-skin").join("index.html")).unwrap(),
            "new",
            "existing same-name skin must be kept"
        );

        let _ = std::fs::remove_dir_all(&app_data);
        let _ = std::fs::remove_dir_all(&portable);
    }
}
