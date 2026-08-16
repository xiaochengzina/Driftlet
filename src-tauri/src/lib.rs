mod app_log;
mod commands;
mod backup;
mod desktop;
mod hotkey;
// Debug builds only — in release the module is compiled out entirely so its
// watcher/helpers don't trigger dead-code warnings during packaging.
#[cfg(debug_assertions)]
mod hotreload;
mod i18n;
mod skin;
mod skin_api;
mod tray;
mod update;
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
    /// .dskin package handed to us either on the command line at cold start
    /// or forwarded by a second instance (single_instance callback).  Stored
    /// here because the frontend may not be ready to receive an event yet —
    /// it pulls this via take_pending_package_install once ready.
    pub pending_package: Mutex<Option<String>>,
    /// Serializes mutations of the data dirs: package installs (a second
    /// double-click install while one is still running would otherwise race
    /// remove_dir_all vs copy on the same skin directory — IO error, not a
    /// hang — but let's be correct) and backup import/export (the import's
    /// staged rename/copy must not overlap an export's read, or the backup
    /// would capture a half-swapped skins/).
    /// tokio Mutex：guard 是 Send，可以持有跨过 .await（装完还要 reload）。
    pub install_lock: tauri::async_runtime::Mutex<()>,
    /// Serializes load-modify-save on a skin folder's `settings.json`, which
    /// has two writers: the manager (`set_skin_custom_setting`) and the skin
    /// itself (`skin_api::skin_set_setting`).  std Mutex is fine — both
    /// commands are sync fns, the guard never crosses an .await.
    /// 目录替换方（install_package / 备份导入 Phase 3）也持这把锁——与设置
    /// 写入互斥，防写进刚被替换掉的旧目录。锁序约定：install_lock →
    /// settings_lock（无反向路径，settings 命令从不取 install_lock）。
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
    /// Skin hot reload master switch (mirrors config.hot_reload; toggled by
    /// the set_hot_reload command, and re-synced from the imported config by
    /// backup::rebuild_runtime).  The watcher thread reads it every tick;
    /// when false, file events are drained without reloading anything.
    pub hot_reload_enabled: AtomicBool,
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
    app_log::init_logger();

    tauri::Builder::default()
        // Must stay the first plugin: a second instance launched by
        // double-clicking a .dskin forwards its args here and exits.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if let Some(path) = dskin_arg(args.into_iter()) {
                log::info!("Second instance handed us a skin package: {}", path);
                // 管理器 webview 未就绪时 emit 的事件会丢 —— 同时写入
                // pending_package 兜底：前端 take_pending_package_install
                // 幂等拉取（与冷启动同一约定）
                *app.state::<AppState>().pending_package.lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(path.clone());
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
                        // 只记「用了快捷键」本身：托盘菜单 toggle 走 toggle_all_skins
                        // 同函数但不该被记成快捷键，所以挂点放 handler 里。
                        log::info!("Global hotkey triggered");
                        hotkey::toggle_all_skins(app);
                    }
                })
                .build(),
        )
        .register_uri_scheme_protocol("skin", skin::protocol::handle_skin_request)
        .setup(|app| {
            // 日志模块持有 AppHandle：push 时向日志窗口（若开着）定向 emit。
            app_log::set_app_handle(app.handle().clone());

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

            // 同步开发期示例皮肤（release 构建在函数内直接返回）
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
            // (folder deleted outside the app, or the author changed the id).
            {
                let removed = config::prune_stale_entries(&mut app_config, &skins);
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
            // args() 对非 UTF-8 参数直接 panic（release 无控制台 GUI =
            // 静默闪退），这里用 args_os() + to_string_lossy() 容错
            let pending_package = dskin_arg(std::env::args_os().map(|arg| arg.to_string_lossy().into_owned()));
            let language = app_config.language.clone();
            let hot_reload = app_config.hot_reload;
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
                hot_reload_enabled: AtomicBool::new(hot_reload),
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
            // 「无标题栏原生窗框」：无边框建窗（tao 剥 WS_CAPTION|WS_THICKFRAME）
            // + 建窗后补回 WS_THICKFRAME|WS_BORDER（见 apply_native_frame）。
            // 保持 shadow(false)——那是 DwmExtendFrameIntoClientArea 玻璃延伸
            // 路径，与真实窗框叠加会在窗缘留 1px 玻璃线
            .decorations(false)
            .shadow(false)
            .resizable(true)
            .center()
            .visible(false)
            .build()
            .unwrap_or_else(|e| {
                fatal_startup_error(&format!("Failed to create manager window: {}", e))
            });

            // 补回「无标题栏原生窗框」样式 + 任务栏/悬停预览/Alt+Tab 图标
            #[cfg(target_os = "windows")]
            {
                if let Ok(hwnd) = manager.hwnd() {
                    if !apply_native_frame(hwnd.0 as isize) {
                        log::error!("apply_native_frame: SetWindowSubclass failed for manager window");
                    }
                    apply_window_icon(hwnd.0 as isize);
                }
            }

            // WebView2 hardening on the manager too: no browser context menu,
            // no F5/Ctrl+R refresh keys (the manager UI must not be
            // reloadable by keystroke).  Same async-init retry as skins;
            // afterwards the 5s maintenance timer keeps re-applying both.
            #[cfg(target_os = "windows")]
            factory::spawn_webview_hardening_retry(app.handle(), "main");

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
                        log::info!("Manager window closed (no tray, exiting)");
                        state.exiting.store(true, std::sync::atomic::Ordering::SeqCst);
                        h.exit(0);
                    } else {
                        api.prevent_close();
                        log::info!("Manager window hidden to tray");
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

            // 启动序列走完（状态、自载皮肤、管理器窗、托盘、热键全就绪）。
            log::info!("Manager started");

            // Periodic frameless maintenance timer (Windows only).
            //
            // SetWindowSubclass only works on the window's OWNER thread, and
            // a failed/somehow-removed subclass leaves the window with tao's
            // WS_CAPTION style — DWM then draws the classic frame (the
            // intermittent title-bar bug).  Every 5 seconds, on the event-loop
            // thread: idempotently re-install the subclass on every skin
            // window (self-healing), then post a deferred cleanup.  The
            // cleanup itself is a near-no-op when styles are clean (no
            // SWP_FRAMECHANGED storm → DWM is never re-triggered).
            // 所有实际工作都是 Win32 窗口修补，非 Windows 不建线程空转。
            #[cfg(target_os = "windows")]
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
                        let h2 = h.clone();
                        let _ = h.run_on_main_thread(move || {
                            // 管理器窗同样自愈：它只有建窗后约 6 秒的创建
                            // 重试（见上面 setup），WebView2 初始化慢过该
                            // 窗口期时 F5 等加速键仍可刷新管理器。
                            if let Some(window) = h2.get_webview_window("main") {
                                factory::disable_default_context_menu(&window);
                                factory::disable_browser_accelerator_keys(&window);
                            }
                            for (skin_id, hwnd) in &windows {
                                factory::ensure_frameless_subclass(*hwnd);
                                factory::force_clean_skin_window_by_hwnd(*hwnd);
                                // Keep WebView2's default context menu and
                                // browser accelerator keys disabled
                                // (self-healing; the creation-time retry in
                                // factory only covers startup).
                                if let Some(window) = h2.get_webview_window(
                                    &factory::skin_window_label(skin_id),
                                ) {
                                    factory::disable_default_context_menu(&window);
                                    factory::disable_browser_accelerator_keys(&window);
                                }
                            }
                        });
                    }
                });
            }

            // Skin hot reload for development: watch the skins directory and
            // reload loaded skins on file changes.  Debug builds only — the
            // watcher is pure overhead in production.
            #[cfg(debug_assertions)]
            hotreload::start(app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_skin_drag,
            commands::start_skin_resize,
            commands::list_skins,
            commands::get_skin_detail,
            commands::load_skin,
            commands::unload_skin,
            commands::reload_skin,
            commands::set_skin_opacity,
            commands::set_skin_placement,
            commands::set_skin_click_through,
            commands::set_skin_position_locked,
            commands::set_skin_resizable,
            commands::set_skin_zoom,
            commands::set_skin_edge_snap,
            commands::set_skin_snap_gap,
            commands::set_skin_position,
            commands::show_skin_context_menu,
            commands::set_skin_size,
            commands::set_skin_custom_setting,
            commands::reset_skin_config,
            commands::pick_skin_package,
            commands::inspect_skin_package,
            commands::install_skin_package,
            commands::remove_skin,
            commands::get_app_config,
            commands::set_autostart,
            commands::get_autostart,
            commands::set_theme,
            commands::set_language,
            commands::set_hotkey,
            commands::set_hot_reload,
            commands::check_update,
            commands::set_update_check,
            commands::open_release_page,
            commands::take_hotkey_error,
            commands::open_skins_folder,
            commands::open_skin_folder,
            commands::list_system_fonts,
            commands::capture_skin_preview,
            commands::take_pending_package_install,
            commands::export_config,
            commands::import_config,
            commands::open_log_window,
            commands::get_app_log,
            commands::clear_app_log,
            commands::open_skin_devtools,
            skin_api::get_cpu_info,
            skin_api::get_gpu_info,
            skin_api::get_memory_info,
            skin_api::get_disks_info,
            skin_api::get_disk_space,
            skin_api::get_network_info,
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
            skin_api::skin_log,
            skin_api::skin_console_log,
        ])
        .run({
            // NOTE(driftlet): never let Tauri hand windows a runtime icon.
            // default_window_icon (icons/icon.ico) is decoded to RGBA and
            // turned into an HICON by tao's RgbaIcon::into_windows_icon, which
            // passes a 1-byte-per-pixel buffer where CreateIcon expects a 1bpp
            // monochrome AND mask — the exact bug we patched in the vendored
            // tray-icon (garbage-mask HICON renders as striped garbage in
            // mask-aware consumers like Task Manager). Window icons are instead
            // set explicitly by apply_window_icon() from the multi-size .ico —
            // the exe-resource fallback does NOT cover the taskbar button /
            // hover preview / Alt+Tab (observed generic default icon there).
            let mut context = tauri::generate_context!();
            context.set_default_window_icon(None);
            context
        })
        .unwrap_or_else(|e| fatal_startup_error(&format!("Error while running Driftlet: {}", e)));
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
/// 示例皮肤只随仓库分发（`examples/`，以独立 .dskin 另行发布），安装包不打包。
/// Skins that don't exist yet are copied; existing skins are updated if the source is newer.
fn copy_example_skins(skins_dir: &PathBuf) {
    // release 构建直接返回：示例源是相对 CWD 的仓库 examples/ 路径，生产环境
    // CWD 不可控（快捷方式启动常落在 System32），撞上同名目录会被静默误装
    if !cfg!(debug_assertions) {
        return;
    }
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

/// 管理器类窗口（管理器/日志窗）的「无标题栏原生窗框」子类。
///
/// 背景：tao 0.35 对 decorations(false) 的窗口自建 NCCALCSIZE=0（client=
/// 整个窗口矩形，见其 event_loop.rs 对 WM_NCCALCSIZE 的处理）——DWM 据
/// 「非客户区为空」判定无框可画，边框与阴影都不出现。本子类把 WM_NCCALCSIZE
/// 绕开 tao 直接交给 DefWindowProcW 默认处理：非客户区回到真实窗框厚度
/// （配合 apply_native_frame 剥掉 WS_CAPTION，无标题栏高度），DWM 随即画出
/// 原生 1px 边框、阴影与最大化/贴靠动画。默认处理后还要按状态修正几何：
/// 还原态把 client 顶边拉回「窗口顶 + 1px」（默认结果把可缩放边框厚度 inset
/// 留在可视区顶部，DWM 只画 1px 顶边框、其余 ~7px 成死白边）；最大化把
/// client 改为显示器 work area、并拦 WM_GETMINMAXINFO 把最大化摆窗改为
/// 「work area + 边框膨胀」（无 WS_CAPTION 时系统按显示器全矩形摆窗、不做
/// work area 收缩——client 盖住任务栏区，窗口矩形底边非客户区还会把非置顶
/// 任务栏整段盖掉）。探针实证见 apply_native_frame。
///
/// 第二个坑：tao 的 WindowState::apply_diff 在任何窗口标志变化时
/// （set_visible、最大化/还原等）用 to_window_styles() 全量重写
/// GWL_STYLE，而该函数无条件带上 WS_CAPTION（仅 CHILD 窗口才按
/// decorations 剥除）——建窗时剥掉的标题栏会被 show() 静默加回。
/// 故子类同时拦截 WM_STYLECHANGING，在样式落地前就地改写 styleNew，
/// 让 WS_CAPTION 永远落不了地（同 factory.rs 皮肤子类的既有手法）。
///
/// 第四个坑（Win10 失焦丢顶边描边）：这套无 CAPTION 配方下 DWM 只在窗口
/// 活动态画顶边 1px 描边，失焦根本不画；子类拦 WM_NCACTIVATE，原参数先放行
/// 给 tao 做焦点簿记，再对 DefWindowProcW 谎报 (TRUE, -1) 把帧外观钉在活动态
/// （失焦后描边/阴影不再消失；探针 focus-frame-probe.ps1 / manager-focus-frame.ps1
/// 实证，详见分支内注释）。
#[cfg(target_os = "windows")]
const NATIVE_FRAME_SUBCLASS_ID: usize = 0x4E4652; // "NFR"；皮肤无边框子类见 factory.rs

#[cfg(target_os = "windows")]
unsafe extern "system" fn native_frame_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    w_param: windows::Win32::Foundation::WPARAM,
    l_param: windows::Win32::Foundation::LPARAM,
    _uidsubclass: usize,
    _dwrefdata: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::Shell::DefSubclassProc;
    use windows::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, GWL_STYLE, IsZoomed, MINMAXINFO, NCCALCSIZE_PARAMS, STYLESTRUCT,
        WM_GETMINMAXINFO, WM_NCCALCSIZE, WM_NCACTIVATE, WM_STYLECHANGING, WS_BORDER,
        WS_CAPTION, WS_THICKFRAME,
    };
    // 纯 FFI 转发/就地改写、无锁无分配，不存在 panic 路径（无需 catch_unwind 包装）
    if msg == WM_NCACTIVATE {
        // 失焦丢顶边描边的修法：这套「无 CAPTION 的 THICKFRAME|BORDER」配方下，
        // DWM 只在窗口活动态画顶边 1px 描边——失焦时根本不画（不是画成浅色，
        // 探针实证 tools/win32-probes/focus-frame-probe.ps1 与
        // manager-focus-frame.ps1：失焦后窗口顶行像素=背景）。这套配方的帧外观
        // 完全由 WM_NCACTIVATE 的默认处理上报驱动，故先放行原参数——tao 用它做
        // 焦点簿记（window_state.set_active + focus 事件，管理器 hover-ok 门控
        // 依赖这些事件；其 ProcResult::DefWindowProc 会把真实状态报给 DWM）——
        // 再对 DefWindowProcW 谎报 (TRUE, -1) 把帧外观钉回活动态（后写覆盖，
        // 探针 N2 方案实证两态描边俱在）。只改 NC 外观，不动真实激活状态；
        // 反方向「跳过 DefWindowProc 直接返回 1」会连活动态描边一起丢掉
        // （DWM 永远收不到帧状态通知，探针 N1 方案实证），勿用。
        let _ = DefSubclassProc(hwnd, msg, w_param, l_param);
        return DefWindowProcW(
            hwnd,
            msg,
            windows::Win32::Foundation::WPARAM(1),
            windows::Win32::Foundation::LPARAM(-1),
        );
    }
    if msg == WM_GETMINMAXINFO {
        // 先让 tao 套它自己的 min/max 约束，再修正最大化摆放：无 WS_CAPTION 的
        // THICKFRAME 窗口，系统默认按显示器全矩形 + 边框膨胀摆放最大化
        // （实测 (-7,-7)-(1543,871)，显示器 1536x864）——client 修正只保住了
        // 内容区，窗口矩形的底边非客户区仍会盖住非置顶任务栏。改成与带标题栏
        // 窗口一致的「work area + 边框膨胀」口径（(-7,-7)-(1543,831)）。
        // 膨胀量不硬编码：从 tao/默认填好的 ptMaxSize 与显示器尺寸反推（跨 DPI）。
        let result = DefSubclassProc(hwnd, msg, w_param, l_param);
        let mmi = &mut *(l_param.0 as *mut MINMAXINFO);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool()
        {
            let inflate_x =
                (mmi.ptMaxSize.x - (mi.rcMonitor.right - mi.rcMonitor.left)) / 2;
            let inflate_y =
                (mmi.ptMaxSize.y - (mi.rcMonitor.bottom - mi.rcMonitor.top)) / 2;
            mmi.ptMaxPosition.x = mi.rcWork.left - inflate_x;
            mmi.ptMaxPosition.y = mi.rcWork.top - inflate_y;
            mmi.ptMaxSize.x = (mi.rcWork.right - mi.rcWork.left) + 2 * inflate_x;
            mmi.ptMaxSize.y = (mi.rcWork.bottom - mi.rcWork.top) + 2 * inflate_y;
        }
        return result;
    }
    if msg == WM_NCCALCSIZE {
        if w_param.0 == 0 {
            return DefWindowProcW(hwnd, msg, w_param, l_param);
        }
        // 默认处理把可缩放边框厚度 inset 加在四边，两处都需修正：
        // 1) 非最大化：左/右/下 inset 被 DWM 放到可视区外（GetWindowRect 比
        //    可视框大约 7px），唯独顶部留在可视区内——DWM 只画 1px 顶边框，
        //    其余 ~7px 成死白边。故把 client 顶边拉回「窗口顶 + 1px」。
        // 2) 最大化：无 WS_CAPTION 的 THICKFRAME 窗口最大化时系统按显示器
        //    全矩形摆窗（不做 work area 收缩），默认结果 client 盖住任务栏
        //    区域（实测 client=显示器全尺寸，底部 40px 藏进任务栏下）——
        //    这里把 client 矩形改写为显示器 work area；窗口矩形本身的摆放
        //    由上面的 WM_GETMINMAXINFO 分支同步修正（否则底边非客户区会把
        //    非置顶任务栏整段盖掉）。
        // 探针实证 tools/win32-probes/top-inset-probe.ps1：还原态 clientTop
        // 8px→1px、最大化 client=rcWork 精确贴合，两种状态下边框/阴影/无
        // 标题栏均完好（不触发细框环那条「DWM 画出完整标题栏」的坑）。
        let params = &mut *(l_param.0 as *mut NCCALCSIZE_PARAMS);
        let win_top = params.rgrc[0].top;
        let result = DefWindowProcW(hwnd, msg, w_param, l_param);
        if IsZoomed(hwnd).as_bool() {
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut mi)
                .as_bool()
            {
                params.rgrc[0] = mi.rcWork;
            }
        } else {
            params.rgrc[0].top = win_top + 1;
        }
        return result;
    }
    if msg == WM_STYLECHANGING && w_param.0 as i32 == GWL_STYLE.0 {
        // tao apply_diff 全量重写样式时会带回 WS_CAPTION（见上注释）——
        // 落地前剥掉标题栏、保住可缩放边框，之后的 WM_STYLECHANGED /
        // NCCALCSIZE 都按净化后的样式走，无需二次 SetWindowPos
        let ss = &mut *(l_param.0 as *mut STYLESTRUCT);
        ss.styleNew = (ss.styleNew & !WS_CAPTION.0) | WS_THICKFRAME.0 | WS_BORDER.0;
    }
    DefSubclassProc(hwnd, msg, w_param, l_param)
}

/// 给无边框建窗的管理器类窗口补回「无标题栏的原生窗框」：
/// 样式剥 WS_CAPTION（标题栏）、加回 WS_THICKFRAME|WS_BORDER（可缩放框），
/// 并装上 native_frame_proc 子类把 NCCALCSIZE 交还默认处理、再按状态修正
/// 几何（还原态 client 顶边拉回「窗口顶 + 1px」消除顶部死白边；最大化
/// client 改为显示器 work area，并拦 WM_GETMINMAXINFO 把最大化摆窗改为
/// 「work area + 边框膨胀」——无 WS_CAPTION 时系统按全矩形摆窗，client 与
/// 窗口底边非客户区都会盖住任务栏）——DWM 随即绘制
/// 原生 1px 边框与阴影而无标题栏。Win10 探针实证
/// （tools/win32-probes/frame-probe.ps1）：
///   - NCCALCSIZE 归零（tao 对无边框窗的默认行为）→ DWM 什么都不画；
///   - 部分非客户区（细框环）→ DWM 画出完整标题栏；
///   - 唯有「WS_THICKFRAME|WS_BORDER、无 WS_CAPTION、NCCALCSIZE 默认」
///     = 原生框+影+无标题栏。
/// client/摆窗修正方案另经 tools/win32-probes/top-inset-probe.ps1 实证：
/// 还原态 clientTop 8px→1px、最大化 rect=work+膨胀 且 client=rcWork 精确
/// 贴合、边框+阴影完好、不触发细框环的标题栏坑。
/// SetWindowSubclass 必须在窗口属主线程调用（跨线程失败，见 factory.rs）。
#[cfg(target_os = "windows")]
pub(crate) fn apply_native_frame(hwnd_val: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::SetWindowSubclass;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos,
        GWL_STYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER,
        SWP_NOSIZE, SWP_NOZORDER, WS_BORDER, WS_CAPTION, WS_THICKFRAME,
    };

    let hwnd = HWND(hwnd_val as *mut _);
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        SetWindowLongPtrW(
            hwnd,
            GWL_STYLE,
            (style & !(WS_CAPTION.0 as isize))
                | WS_THICKFRAME.0 as isize
                | WS_BORDER.0 as isize,
        );
        let ok = SetWindowSubclass(
            hwnd,
            Some(native_frame_proc),
            NATIVE_FRAME_SUBCLASS_ID,
            0,
        )
        .as_bool();
        // 让 DWM 按新样式 + 新 NCCALCSIZE 口径重估窗框
        let _ = SetWindowPos(
            hwnd,
            None,
            0, 0, 0, 0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE,
        );
        ok
    }
}

/// 给窗口补任务栏按钮 / 悬停预览 / Alt+Tab 图标（ICON_SMALL + ICON_BIG）。
///
/// 不能走 tauri 的 `default_window_icon`/`set_icon`：那条路把 icon.ico 解码成
/// RGBA 再经 tao `RgbaIcon::into_windows_icon` 重建 HICON，而 tao 的 AND mask
/// 缓冲是 1 字节/像素、`CreateIcon` 期望 1bpp 单色掩码（vendored tray-icon 里
/// 修的就是同一个 bug）——产物在任务管理器等 mask 敏感消费者手里渲染成花屏。
/// 这里对打包进二进制的多尺寸 icon.ico 直接 `CreateIconFromResourceEx`，由系统
/// 按目标尺寸挑目录内最佳条目，掩码与尺寸都正确；两枚 HICON 进程级缓存复用
/// （日志窗反复开关不重复建、不泄漏）。
#[cfg(target_os = "windows")]
pub(crate) fn apply_window_icon(hwnd_val: isize) {
    use std::sync::OnceLock;
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateIconFromResourceEx, SendMessageW, HICON, ICON_BIG, ICON_SMALL, LR_DEFAULTCOLOR,
        SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, WM_SETICON,
    };

    /// 打包进二进制的多尺寸 .ico（与 exe 资源图标同一份文件）
    const ICO: &[u8] = include_bytes!("../icons/icon.ico");
    // (small, big) 两枚 HICON 的句柄值，建一次全进程复用
    static ICONS: OnceLock<Option<(usize, usize)>> = OnceLock::new();

    /// 从 .ico 目录里挑最贴合 (cx, cy) 的条目建 HICON。
    /// 目录自己解析：LookupIconIdFromDirectoryEx 的返回值是「资源 ID」语义——
    /// 喂文件版目录时读到的是 dwImageOffset 的低 16 位，不能当条目索引用
    /// （实测返回 102 = 首条目文件偏移，索引化直接越界）。挑选规则：图标
    /// 为正方形只比高度，够大（h >= cy）里取最小，都偏小取最大——缩小清晰、
    /// 放大糊。
    fn load_entry(cx: i32, cy: i32) -> Option<HICON> {
        let count = u16::from_le_bytes(ICO.get(4..6)?.try_into().ok()?) as usize;
        let mut best: Option<(usize, i32)> = None;
        for i in 0..count {
            let base = 6usize.checked_add(i.checked_mul(16)?)?;
            let raw_h = *ICO.get(base + 1)?; // ICONDIRENTRY.bHeight，0 表示 256
            let h = if raw_h == 0 { 256 } else { raw_h as i32 };
            let take = match best {
                None => true,
                Some((_, bh)) => match (h >= cy, bh >= cy) {
                    (true, true) => h < bh,
                    (true, false) => true,
                    (false, true) => false,
                    (false, false) => h > bh,
                },
            };
            if take {
                best = Some((i, h));
            }
        }
        let (i, _) = best?;
        let base = 6 + i * 16;
        // ICONDIRENTRY：dwBytesInRes 在 +8、dwImageOffset 在 +12
        let size = u32::from_le_bytes(ICO.get(base + 8..base + 12)?.try_into().ok()?) as usize;
        let offset = u32::from_le_bytes(ICO.get(base + 12..base + 16)?.try_into().ok()?) as usize;
        let image = ICO.get(offset..offset.checked_add(size)?)?;
        unsafe { CreateIconFromResourceEx(image, true, 0x00030000, cx, cy, LR_DEFAULTCOLOR).ok() }
    }

    let icons = ICONS.get_or_init(|| {
        let dpi = unsafe { GetDpiForWindow(HWND(hwnd_val as *mut _)) };
        let small = load_entry(
            unsafe { GetSystemMetricsForDpi(SM_CXSMICON, dpi) },
            unsafe { GetSystemMetricsForDpi(SM_CYSMICON, dpi) },
        );
        let big = load_entry(
            unsafe { GetSystemMetricsForDpi(SM_CXICON, dpi) },
            unsafe { GetSystemMetricsForDpi(SM_CYICON, dpi) },
        );
        small.zip(big).map(|(s, b)| (s.0 as usize, b.0 as usize))
    });
    let Some(&(small, big)) = icons.as_ref() else {
        log::warn!("apply_window_icon: failed to create HICONs from icon.ico");
        return;
    };
    unsafe {
        let hwnd = HWND(hwnd_val as *mut _);
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_SMALL as usize)),
            Some(LPARAM(small as isize)),
        );
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_BIG as usize)),
            Some(LPARAM(big as isize)),
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
