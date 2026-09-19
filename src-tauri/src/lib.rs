mod app_log;
mod backup;
mod commands;
mod desktop;
#[cfg(target_os = "windows")]
mod elevation;
mod focus;
mod hotkey;
// Debug builds only — in release the module is compiled out entirely so its
// watcher/helpers don't trigger dead-code warnings during packaging.
mod capture;
#[cfg(debug_assertions)]
mod hotreload;
mod i18n;
mod policy;
mod skin;
mod skin_api;
mod tray;
mod update;
mod webview2;
mod window;

// Re-export the skin protocol registration helper.
pub use skin::protocol;

use desktop::Pinner;
use skin::config;
use skin::loader;
use skin::types::AppConfig;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use tauri::{Emitter, Manager};
use window::factory;
use window::registry::SkinWindowRegistry;

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
    /// Desktop pinner (Driftlet-style z-order adjacency watcher — deliberately
    /// NO helper window and NO show-desktop state machine; see desktop.rs)
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
    /// Serializes the skin lifecycle commands (load/unload/reload/reset 与
    /// skin_api 的 skin_load/skin_unload/skin_reload/hotreload 触发）：
    /// load/unload 的「查存在→注册」非原子，并发交错会产生 registry 与
    /// loaded_skins 状态分叉。锁序约定：lifecycle_lock → install_lock →
    /// settings_lock（无反向路径）。注：install 侧批操作（备份导入/
    /// reload_all/布局套用/专注模式卸载档）持锁后也会调生命周期 impl
    ///（impl 本身无锁，串行靠外层持锁；批内并发的口径见
    /// commands::run_skins_concurrent 头注）。
    pub lifecycle_lock: tauri::async_runtime::Mutex<()>,
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
    /// 托盘「皮肤显隐」子菜单的每皮肤勾选项（skin_id → CheckMenuItem）：
    /// 加载集（键集合）与 registry.loaded_ids 不一致时重建整个托盘菜单
    ///（顺便更新本表），一致时仅按真实可见性 set_checked。
    pub skin_vis_items:
        Mutex<std::collections::HashMap<String, tauri::menu::CheckMenuItem<tauri::Wry>>>,
    /// Startup hotkey registration failure (the configured combo, e.g.
    /// "Ctrl+Alt+D"). Pulled once by the frontend on init so the user sees
    /// a toast instead of a silent log — mirrors pending_package.
    pub hotkey_error: Mutex<Option<String>>,
    /// 「打开皮肤配置」事件在管理器已销毁（关窗即销毁）时发出会丢——后端
    /// 暂存待选皮肤 id，前端启动时经 take_pending_open_config 幂等拉取
    /// （与 pending_package 同一约定）。
    pub pending_open_config: Mutex<Option<String>>,
    /// 启动自动更新检测每进程只做一次（管理器关窗即销毁后，每次重建都会
    /// 重跑前端 init——没有本标记，开着更新时每次打开管理器都会重查重弹）。
    /// 手动「检查更新」（关于页）不受此限。前端经 take_update_auto_checked
    /// 取走 true 后后续启动跳过。
    pub update_auto_checked: AtomicBool,
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
        self.language
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    app_log::init_logger();
    // 提权启动自动降级：本程序不需要管理员（清单 asInvoker；音量/媒体/
    // run_command 普通权限/电源五条/WebView2 全都不吃提权），提权运行只会
    // 让皮肤拿到高完整性上下文、run_command 子进程全部提权、Explorer 拖放
    // 被 UIPI 拦截。必须在 Builder/single-instance 之前完成——降级 = 注册
    // 提权启动提醒（不自动降级——任务计划/令牌 API 降级机械已全移除，见
    // elevation.rs 头注的废弃路线清单）：检测到提权即弹原生消息框说明影响，
    // 「是」= 写 allow_elevated 持久放行并继续，「否」= 退出。
    #[cfg(target_os = "windows")]
    elevation::startup_elevation_notice();
    // WebView2 运行时版本地板（屿族皮肤的容器查询/color-mix 基线）：过旧即
    // 原生框引导更新——安装器管安装/更新时升旧，这里管「装好之后又变旧」
    webview2::runtime_floor_notice();

    tauri::Builder::default()
        // Must stay the first plugin: a second instance launched by
        // double-clicking a .dskin forwards its args here and exits.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if let Some(path) = dskin_arg(args.into_iter()) {
                log::info!("Second instance handed us a skin package: {}", path);
                // 管理器 webview 未就绪时 emit 的事件会丢 —— 同时写入
                // pending_package 兜底：前端 take_pending_package_install
                // 幂等拉取（与冷启动同一约定）
                *app.state::<AppState>()
                    .pending_package
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(path.clone());
                tray::show_manager_window(app);
                let _ = app.emit_to("main", "open-skin-package", path);
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
                .with_handler(|app, shortcut, event| {
                    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        // 皮肤专属热键与全局热键统一在 dispatch 里分发
                        //（专属优先；日志按真实命中的目标记）
                        hotkey::dispatch_hotkey(app, shortcut);
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
            let app_data_dir = app.path().app_data_dir().unwrap_or_else(|e| {
                fatal_startup_error(&format!("Failed to get app data directory: {}", e))
            });

            // 导入崩溃回滚必须赶在 resolve_portable_dir 之前（它的可写性探测
            // 会创建目录，把「目录不存在」的崩溃现场抹成「两者都在」，回滚分支
            // 沦为死代码——审查 A-H1）。对两种布局的候选位置各跑一次：
            // .import-old 残留只会出现在上次导入实际运行的位置，另一处空转。
            if let Some(exe_dir) = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            {
                crate::backup::rollback_interrupted_import(
                    &exe_dir.join("config"),
                    &exe_dir.join("skins"),
                );
                // 逐文件夹暂存残留恢复（.<folder>.old / .staging-*）——
                // 选择性导入/从源同步/包安装的崩溃现场，同 A-H1 语义
                crate::skin::package::recover_interrupted_folder_ops(&exe_dir.join("skins"));
            }
            crate::backup::rollback_interrupted_import(
                &app_data_dir.join("config"),
                &app_data_dir.join("skins"),
            );
            crate::skin::package::recover_interrupted_folder_ops(&app_data_dir.join("skins"));

            let skins_dir = resolve_portable_dir(&app_data_dir, "skins");
            let config_dir = resolve_portable_dir(&app_data_dir, "config");

            // One-time migration of a config left by older versions in %APPDATA%.
            migrate_legacy_config(&app_data_dir, &config_dir, &skins_dir);

            // （回滚已提前到目录创建之前——见上；勿把 rollback_interrupted_import
            // 挪回 resolve_portable_dir 之后）

            // 更新清理：新版本装上了（当前版本 ≥ 下载标记版本）→ 删掉更新
            // 目录里的安装包与标记（装完不留下次还用不到的旧安装包）
            crate::update::cleanup_downloaded_installer(&config_dir);

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
                &skins
                    .iter()
                    .map(|s| (s.id.clone(), s.directory.clone()))
                    .collect::<Vec<_>>(),
            );

            // Load config
            let mut app_config = config::load_config(&config_dir);

            // 首装种子：内置族皮肤落 skins 目录 + 归入「默认皮肤」组
            //（一次性标记；置位后删过的不复活、组删过不重建）。装了新皮肤
            // 就重扫目录，让下面的 prune 与自动加载看到它们
            let skins = if !app_config.bundled_skins_seeded {
                let installed = seed_bundled_skins(app, &skins_dir, &mut app_config);
                if let Err(e) = config::save_config(&config_dir, &app_config) {
                    log::warn!("Failed to save bundled-skins seed flag: {}", e);
                }
                if installed {
                    loader::scan_skins_directory(&skins_dir)
                } else {
                    skins
                }
            } else {
                skins
            };

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
            let pending_package =
                dskin_arg(std::env::args_os().map(|arg| arg.to_string_lossy().into_owned()));
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
                lifecycle_lock: tauri::async_runtime::Mutex::new(()),
                settings_lock: Mutex::new(()),
                language: Mutex::new(language),
                toggle_item: Mutex::new(None),
                skin_vis_items: Mutex::new(std::collections::HashMap::new()),
                hotkey_error: Mutex::new(None),
                pending_open_config: Mutex::new(None),
                update_auto_checked: AtomicBool::new(false),
                hot_reload_enabled: AtomicBool::new(hot_reload),
            });

            // Auto-load previously loaded skins
            // Read loaded_skins list first (drop lock before creating windows)
            let to_load: Vec<String> = {
                app.handle()
                    .state::<AppState>()
                    .config
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .loaded_skins
                    .clone()
            };

            // 并发自载 + 专注模式残留恢复，一并挪进 setup 后的异步任务：
            // 建窗/取 HWND/装子类等主线程消息要等事件循环起动才被处理，
            // 若在 setup 里同步并发 join 会自锁（主线程被 setup 占着）。
            // 并发让整批皮肤近似同批亮相——串行逐窗创建把「上一窗全部
            // 落地」串进「下一窗开始建」，逐窗登场感由此而来。
            // startup_recover 必须排在自载完成后（卸载档要重载的皮肤走
            // 正常注册表，别与自载竞态），故串在同一任务内。
            {
                let h = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    // 与命令层同两把生命周期锁：自载批与「专注模式退出/
                    // 布局套用/组批量/热重载」等持锁批不交错（2026-09
                    // 审查 A3；锁内一批并发的口径见 run_skins_concurrent
                    // 头注）
                    let state = h.state::<AppState>();
                    let _guards = commands::lifecycle_guards(&state).await;
                    for (id, result) in commands::run_skins_concurrent(
                        h.clone(),
                        to_load,
                        commands::SkinBatchOp::Load,
                    )
                    .await
                    {
                        match result {
                            Ok(()) => log::info!("Auto-loaded skin: {}", id),
                            Err(e) => log::warn!("Failed to auto-load skin '{}': {}", id, e),
                        }
                    }
                    focus::startup_recover(&h).await;
                });
            }

            // Create manager window — center on screen, hidden on startup.
            // User opens it from the tray icon.（关窗即销毁、唤回重建的
            // 机制说明见 create_manager_window 头注）
            let manager = create_manager_window(app.handle())
                .unwrap_or_else(|| fatal_startup_error("Failed to create manager window"));

            // Double-click install: a package was passed on the command
            // line, so show the manager window right away — the frontend
            // will open the install wizard as soon as it loads.
            if pending_package.is_some() {
                log::info!("Opened with skin package: {:?}", pending_package);
                let _ = manager.show();
                let _ = manager.set_focus();
            }

            // Set up system tray
            match tray::create_tray(app.handle()) {
                Ok(()) => {
                    app.handle()
                        .state::<AppState>()
                        .tray_ok
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                }
                Err(e) => {
                    log::warn!("Failed to create system tray: {}", e);
                }
            }

            // Register the configured global hotkey (failures only log).
            hotkey::register_from_config(app.handle());
            // 皮肤专属热键注册表按 config 全量重建（单个失败仅记日志）
            hotkey::sync_skin_hotkeys_from_config(app.handle());

            // 启动序列走完（状态、自载皮肤、管理器窗、托盘、热键全就绪）。
            log::info!("Manager started");

            // 专注模式全屏检测线程（常驻，只在设置开启时干活）
            focus::spawn_detector(app.handle());

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
                        let skin_ids = state.registry.loaded_ids();
                        let h2 = h.clone();
                        let _ = h.run_on_main_thread(move || {
                            // 值守闭包 panic 防护：主线程上 panic 会拖垮
                            // 整个事件循环——捕获留痕，下轮继续（pinner
                            // 值守环同款，2026-09 审查 A2）
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                // 管理器窗同样自愈：它只有建窗后约 6 秒的创建
                                // 重试（见上面 setup），WebView2 初始化慢过该
                                // 窗口期时 F5 等加速键仍可刷新管理器。
                                for label in ["main", "log"] {
                                    if let Some(window) = h2.get_webview_window(label) {
                                        factory::disable_default_context_menu(&window);
                                        factory::disable_browser_accelerator_keys(&window);
                                        // 原生窗框子类一并自愈（管理器 + 日志窗
                                        // 同配方）：唤回重建路径若因故没装上
                                        //（跨线程静默失败/超时），下一轮幂等
                                        // 补装——否则无边框窗的 NCCALCSIZE 归零
                                        // 会让 DWM 一直不画框（Win11 圆角丢失）。
                                        if let Ok(hwnd) = window.hwnd() {
                                            ensure_native_frame(hwnd.0 as isize);
                                        }
                                    }
                                }
                                for skin_id in &skin_ids {
                                    // HWND 必须在主线程闭包内按 label 现取——
                                    // 定时器线程快照与主线程使用之间窗口可能
                                    // 销毁、HWND 被系统回收复用，用快照句柄做
                                    // subclass/PostMessage 会作用到无关窗口
                                    let label = factory::skin_window_label(skin_id);
                                    if let Some(window) = h2.get_webview_window(&label) {
                                        if let Ok(hwnd) = window.hwnd() {
                                            factory::ensure_frameless_subclass(hwnd.0 as isize);
                                            factory::force_clean_skin_window_by_hwnd(
                                                hwnd.0 as isize,
                                            );
                                        }
                                        // Keep WebView2's default context menu and
                                        // browser accelerator keys disabled
                                        // (self-healing; the creation-time retry in
                                        // factory only covers startup).
                                        factory::disable_default_context_menu(&window);
                                        factory::disable_browser_accelerator_keys(&window);
                                    }
                                }
                                // 同 tick 顺带救回完全出屏的皮肤（拔屏/DPI 拓扑
                                // 变化最迟 5 秒自愈；部分出屏不动，见 factory）
                                factory::rescue_offscreen_skins(&h2);
                            }))
                            .map_err(|_| {
                                log::error!(
                                    "5s maintenance timer closure panicked — next tick will retry"
                                );
                            })
                            .ok();
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
            commands::load_skins,
            commands::unload_skins,
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
            commands::bring_skin_onscreen,
            commands::show_skin_context_menu,
            commands::set_skin_size,
            commands::set_skin_custom_setting,
            commands::reset_skin_config,
            commands::pick_skin_package,
            commands::package_skin,
            commands::inspect_skin_package,
            commands::install_skin_package,
            commands::remove_skin,
            commands::duplicate_skin,
            commands::sync_skin_copy,
            commands::get_app_config,
            commands::set_autostart,
            commands::get_autostart,
            commands::set_theme,
            commands::set_language,
            commands::set_skin_visibility,
            commands::set_skin_groups,
            commands::capture_layout,
            commands::apply_layout,
            commands::set_layouts,
            commands::set_hotkey,
            commands::set_skin_hotkey,
            commands::set_hot_reload,
            commands::check_update,
            commands::set_update_check,
            commands::get_titlebar_warnings,
            commands::set_titlebar_warnings,
            commands::open_release_page,
            commands::open_repo_page,
            commands::get_user_agreement,
            commands::download_update,
            commands::install_update,
            commands::take_hotkey_error,
            commands::open_skins_folder,
            commands::pick_path,
            commands::open_skin_folder,
            commands::list_system_fonts,
            commands::list_gpu_adapters,
            commands::capture_skin_preview,
            commands::take_pending_package_install,
            commands::take_pending_open_config,
            commands::take_update_auto_checked,
            commands::get_focus_mode_state,
            commands::toggle_focus_mode,
            commands::set_focus_exempt,
            commands::set_focus_mode_action,
            commands::set_focus_mode_auto_fullscreen,
            commands::export_config,
            commands::inspect_backup,
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
            skin_api::skin_set_menu_items,
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
            skin_api::media_seek,
            skin_api::read_clipboard_text,
            skin_api::write_clipboard_text,
            skin_api::open_external,
            skin_api::show_notification,
            skin_api::lock_workstation,
            skin_api::monitor_off,
            skin_api::sleep,
            skin_api::power_control,
            skin_api::empty_recycle_bin,
            skin_api::get_mic_spectrum,
            skin_api::get_battery_info,
            skin_api::get_idle_time,
            skin_api::get_foreground_window_info,
            skin_api::get_monitors,
            skin_api::get_system_theme,
            skin_api::skin_log,
            skin_api::skin_console_log,
            skin_api::skin_read_any_file,
            skin_api::skin_write_any_file,
            skin_api::skin_list_any_dir,
            skin_api::skin_create_any_dir,
            skin_api::skin_delete_any_path,
            skin_api::skin_get_window_config,
            skin_api::skin_set_window_config,
            skin_api::skin_load,
            skin_api::skin_unload,
            skin_api::skin_reload,
            skin_api::skin_list_skins,
            skin_api::http_request,
            skin_api::skin_broadcast,
            skin_api::skin_hide,
            skin_api::skin_show,
        ])
        .build({
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
        .unwrap_or_else(|e| fatal_startup_error(&format!("Error while building Driftlet: {}", e)))
        // 关窗即销毁后：最后一个窗口销毁会触发 ExitRequested——托盘还在就不
        // 是退出（托盘左键随时唤回重建），否则进程静默死掉（实机踩过）。
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                let state = app_handle.state::<AppState>();
                if !state.exiting.load(std::sync::atomic::Ordering::SeqCst)
                    && state.tray_ok.load(std::sync::atomic::Ordering::SeqCst)
                {
                    api.prevent_exit();
                }
            }
        });
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
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

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

/// 创建管理器窗口（启动建窗与销毁后唤回重建共用）。**关窗即销毁、唤回重建**：
/// 管理器渲染进程 ~80MB 是皮肤之外最大的常驻块；「隐藏常驻秒开」的另一条路
/// （隐藏时 TrySuspend 挂起渲染进程）已实机证伪（docs/已知问题.md），故选
/// 销毁回收——重建代价 = WebView2 重初始化 + 页面重载（约 1s 级，用户已接受）。
/// 注意：窗口事件处理（关窗销毁）随窗重建重挂；前端瞬态（选中皮肤、搜索词）
/// 不保留——列表/主题/权限等全部从后端配置重建。
///
/// 返回 None = 建窗失败（仅记日志；启动期由调用方转 fatal_startup_error）。
pub(crate) fn create_manager_window(app: &tauri::AppHandle) -> Option<tauri::WebviewWindow> {
    // 首帧主题防闪白：当前生效主题烘进入口 URL query（auto 已按小时折算
    // 成具体值，与日志窗 log.html?theme=… 同一模式）——页面 theme-boot.js
    // 在样式表加载前同步把 data-theme 落到 <html>，首帧即是正确主题
    //（此前 renderShell 先画浅色默认主题、initTheme 的 IPC 往返后才切深色，
    // 深色主题下唤出必闪一帧浅色）。窗口背景刷与 WebView2 默认背景一并设
    // 成该主题的 --bg-app：WebView2 异步初始化 + 页面加载期间（唤回重建
    // 约 1s）不再露白色底。配色值与 style.css :root / [data-theme="dark"]
    // 的 --bg-app 保持同值。
    let theme = config::current_theme(&app.state::<AppState>());
    let (bg_r, bg_g, bg_b) = if theme == "dark" {
        (0x26, 0x2e, 0x39)
    } else {
        (0xf4, 0xf8, 0xfb)
    };
    // frameless with custom title bar (min/max/close buttons in UI)
    let manager = tauri::WebviewWindowBuilder::new(
        app,
        "main",
        tauri::WebviewUrl::App(format!("index.html?theme={}", theme).into()),
    )
    .background_color(tauri::webview::Color(bg_r, bg_g, bg_b, 255))
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
    .build();
    let manager = match manager {
        Ok(w) => w,
        Err(e) => {
            log::error!("Failed to create manager window: {}", e);
            return None;
        }
    };

    // 补回「无标题栏原生窗框」样式 + 任务栏/悬停预览/Alt+Tab 图标。
    // apply_native_frame 内含 SetWindowSubclass——必须在窗口属主线程
    //（主线程）调用，跨线程静默失败：皮肤右键菜单唤出管理器走的是
    // tokio worker 线程（show_skin_context_menu → show_manager_window
    // → 本函数），子类装不上则 tao 对无边框窗的 NCCALCSIZE 归零处理
    // 无人拦截，DWM 不画窗框——Win11 原生圆角（DWM 非客户区渲染的
    // 一部分）随之消失，而托盘路径本就在主线程所以一直正常。
    #[cfg(target_os = "windows")]
    {
        if let Ok(hwnd) = manager.hwnd() {
            apply_native_frame_on_owner_thread(app, hwnd.0 as isize);
            apply_window_icon(hwnd.0 as isize);
        }
    }

    // WebView2 hardening on the manager too: no browser context menu,
    // no F5/Ctrl+R refresh keys (the manager UI must not be
    // reloadable by keystroke).  Same async-init retry as skins;
    // afterwards the 5s maintenance timer keeps re-applying both.
    #[cfg(target_os = "windows")]
    factory::spawn_webview_hardening_retry(app, "main");

    // Window event handler: close = destroy（回收渲染进程内存），唤回 = 重建
    let h = app.clone();
    manager.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            let state = h.state::<AppState>();
            if state.exiting.load(std::sync::atomic::Ordering::SeqCst) {
                // Real exit（放行：退出流程统一回收）
            } else if !state.tray_ok.load(std::sync::atomic::Ordering::SeqCst) {
                // 托盘没建起来：销毁管理器会彻底失去 UI 入口 ——
                // 关窗直接走退出流程
                log::info!("Manager window closed (no tray, exiting)");
                state
                    .exiting
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                h.exit(0);
            } else {
                api.prevent_close();
                log::info!("Manager window destroyed (will recreate on tray open)");
                let _ = h.get_webview_window("main").map(|w| w.destroy());
            }
        }
    });

    Some(manager)
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
        log::warn!(
            "Cannot resolve exe path, {} dir falls back to {:?}",
            name,
            fallback
        );
        let _ = std::fs::create_dir_all(&fallback);
        return fallback;
    };

    let dir = exe_dir.join(name);
    // 受保护系统根下跳过探针直接回退：探针以当前令牌实写文件，提权
    // 运行会误判 Program Files 类目录可写——数据根随「本次是否提权」
    // 漂移（提权运行选中便携布局并把 %APPDATA% 的皮肤搬进 <exe>/skins，
    // 正常运行再回退 %APPDATA%，用户皮肤「消失」；2026-09 审查 F1）。
    // 布局判定必须与提权态无关。
    #[cfg(target_os = "windows")]
    if is_under_protected_root(&exe_dir, &protected_root_dirs()) {
        log::info!(
            "{} dir is under a protected root; using {:?} (no probe — probe would pass when elevated)",
            name,
            fallback
        );
        let _ = std::fs::create_dir_all(&fallback);
        return fallback;
    }
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
                name,
                dir,
                e,
                fallback
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

/// 已知受保护系统根（写入需提权）：Program Files 两槽 + Windows 目录。
/// 读环境变量现取（不硬编码盘符——系统可装非 C 盘）。
#[cfg(target_os = "windows")]
fn protected_root_dirs() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for key in [
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "windir",
    ] {
        if let Some(v) = std::env::var_os(key) {
            roots.push(PathBuf::from(v));
        }
    }
    roots
}

/// dir 是否位于受保护根之下（大小写不敏感、按分隔符边界比前缀——
/// 「C:\Program Files-x」不能误中「C:\Program Files」）。抽成纯函数以便
/// 测试钉住（F1 回归）。
#[cfg(target_os = "windows")]
fn is_under_protected_root(dir: &std::path::Path, roots: &[PathBuf]) -> bool {
    let d = dir.to_string_lossy().to_lowercase();
    roots.iter().any(|r| {
        let r = r.to_string_lossy().to_lowercase();
        let r = r.trim_end_matches('\\');
        d == r || d.starts_with(&format!("{}\\", r))
    })
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
        // 原子就位（copy 到 tmp 再 rename）：copy 半截崩溃会留下「已存在
        // 的坏 config.json」——存在即不再重试、随后被判损坏重置（legacy
        // 原件仍在可手工救回，但配置实质丢失；2026-09 审查 F10）
        let tmp = current.with_extension("tmp");
        match std::fs::copy(&legacy, &tmp).and_then(|_| std::fs::rename(&tmp, &current)) {
            Ok(_) => log::info!("Migrated legacy config from {:?}", legacy),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                log::warn!("Failed to migrate legacy config: {}", e);
            }
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
            log::info!(
                "Legacy skin {:?} skipped: {:?} already exists",
                entry.file_name(),
                dest
            );
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

/// 安装包内置的屿族皮肤 id（源在仓库 examples/，经 tauri bundle resources
/// 装进安装目录 bundled-skins/）。组内显示顺序 = 此数组序。
const BUNDLED_SKIN_IDS: [&str; 6] = [
    "isles-countdown",
    "isles-calendar",
    "isles-clock",
    "isles-monitor",
    "isles-weather",
    "isles-timer",
];

/// 首装种子：把安装包内置族皮肤装进 skins 目录并归入「默认皮肤」组。
/// 一次性——配置标记 bundled_skins_seeded 置位后不再执行（用户删过的皮肤
/// 不复活、组被删过不重建）。补缺不覆盖：盘上已有的同名皮肤（用户更新/
/// 修改过的拷贝）一律不动。返回是否有皮肤新装（需要重扫目录）。
fn seed_bundled_skins(
    app: &tauri::App,
    skins_dir: &std::path::Path,
    config: &mut skin::types::AppConfig,
) -> bool {
    let mut installed = false;
    match app.path().resource_dir() {
        Ok(rdir) => {
            let src_root = rdir.join("bundled-skins");
            if src_root.is_dir() {
                for id in BUNDLED_SKIN_IDS {
                    let src = src_root.join(id);
                    let dest = skins_dir.join(id);
                    if src.is_dir() && !dest.exists() {
                        // staging + rename：中途失败/崩溃不留半成品目录（残留
                        // .staging-* 由启动 recover_interrupted_folder_ops 清理）；
                        // 直写最终目录会让 dest.exists() 把残缺目录当成「已装」
                        // 永久挡住补拷（审查 F1-A）
                        let staging = skins_dir.join(format!(".staging-{}", id));
                        let _ = std::fs::remove_dir_all(&staging);
                        let res = copy_dir_recursive(&src, &staging)
                            .and_then(|_| std::fs::rename(&staging, &dest));
                        match res {
                            Ok(_) => {
                                installed = true;
                                log::info!("Installed bundled skin: {}", id);
                            }
                            Err(e) => {
                                let _ = std::fs::remove_dir_all(&staging);
                                log::warn!("Failed to install bundled skin {}: {}", id, e);
                            }
                        }
                    }
                }
            } else if !cfg!(debug_assertions) {
                // 生产安装包漏带资源 = 打包事故——响亮记一笔（审查 F1-C）；
                // dev 下资源可能尚未被 tauri-build 拷贝，静默
                log::warn!("bundled skins resource dir missing: {:?}", src_root);
            }
        }
        Err(e) => {
            if !cfg!(debug_assertions) {
                log::warn!("resource_dir unavailable for bundled skins seeding: {}", e);
            }
        }
    }
    // 归组：盘上存在的内置皮肤里**尚未归组**的进「默认皮肤」组——升级用户
    // 自建组里的归属保持不动（审查 F1-B）；全部已有归属则不建空组
    let ungrouped: Vec<&str> = BUNDLED_SKIN_IDS
        .iter()
        .copied()
        .filter(|id| skins_dir.join(id).join("skin.json").is_file())
        .filter(|id| !config.skin_group_map.contains_key(*id))
        .collect();
    if !ungrouped.is_empty() {
        config.skin_groups.push(skin::types::SkinGroup {
            id: "g-default-skins".to_string(),
            name: crate::i18n::tr(&config.language, crate::i18n::Key::DefaultSkinsGroup)
                .to_string(),
            collapsed: false,
        });
        for id in ungrouped {
            config
                .skin_group_map
                .insert(id.to_string(), "g-default-skins".to_string());
        }
    }
    config.bundled_skins_seeded = true;
    installed
}

/// Sync example skins into the skins directory (development only).
/// 屿族官方皮肤（`examples/`）随安装包打包（资源 bundled-skins/，首装种子
/// 见 seed_bundled_skins）；开发期从这里同步进 skins 目录保持最新。
/// 演示皮肤（`demos/`）不再同步——它们只是接口演示，装即用的族皮肤才是默认内容。
/// Skins that don't exist yet are copied; existing skins are updated if the source is newer.
fn copy_example_skins(skins_dir: &Path) {
    // release 构建直接返回：示例源是相对 CWD 的仓库 examples/ 路径，生产环境
    // CWD 不可控（快捷方式启动常落在 System32），撞上同名目录会被静默误装
    if !cfg!(debug_assertions) {
        return;
    }
    // 开发期的皮肤源（仓库 examples/ 目录；demos/ 已停同步——接口演示不属于
    // 开发期默认内容）
    let example_dirs = ["examples"];
    let prefixes = ["..", "."]; // ..: CWD=src-tauri；.: CWD=项目根

    let canonical_dest = skins_dir.canonicalize().ok();
    let mut synced_any = false;

    for dir in example_dirs {
        for prefix in prefixes {
            let source = PathBuf::from(prefix).join(dir);
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
            synced_any = true;
            log::info!("Syncing example skins from {:?}", source);
            if let Ok(entries) = std::fs::read_dir(source) {
                for entry in entries.flatten() {
                    // 只同步「皮肤目录」：含 skin.json 且非 `_` 开头（_ 开头 =
                    // 基建/模板，如 examples/_template、shared/ 不同步）
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if name.starts_with('_') || !entry.path().join("skin.json").is_file() {
                        continue;
                    }
                    let dest = skins_dir.join(entry.file_name());
                    if !dest.exists() {
                        let _ = copy_dir_recursive(&entry.path(), &dest);
                        log::info!("  Installed example skin: {:?}", entry.file_name());
                    } else if source_is_newer(&entry.path(), &dest) {
                        // 更新示例皮肤时保留用户设置值：settings.json 先取出，
                        // 拷贝后写回（与 package.rs 的安装保留同一约定）。
                        // staging + rename：直删直拷中途失败/崩溃会留下被删或
                        // 半拷的开发皮肤（审查 F1-D）——.staging-* 残留由启动
                        // recover_interrupted_folder_ops 清理
                        let saved_settings =
                            std::fs::read(dest.join(skin::settings::SETTINGS_FILENAME)).ok();
                        let staging = skins_dir.join(format!(".staging-{}", name.as_ref()));
                        let _ = std::fs::remove_dir_all(&staging);
                        match copy_dir_recursive(&entry.path(), &staging).and_then(|_| {
                            std::fs::remove_dir_all(&dest)?;
                            std::fs::rename(&staging, &dest)
                        }) {
                            Ok(_) => {
                                if let Some(bytes) = saved_settings {
                                    let _ = std::fs::write(
                                        dest.join(skin::settings::SETTINGS_FILENAME),
                                        bytes,
                                    );
                                }
                                log::info!("  Updated example skin: {:?}", entry.file_name());
                            }
                            Err(e) => {
                                let _ = std::fs::remove_dir_all(&staging);
                                log::warn!(
                                    "  Failed to update example skin {:?}: {}",
                                    entry.file_name(),
                                    e
                                );
                            }
                        }
                    }
                }
            }
            break; // 本目录已就位，不再看另一候选前缀
        }
    }
    if !synced_any {
        log::info!("No example skins found to copy");
    }
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
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows::Win32::UI::Shell::DefSubclassProc;
    use windows::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, GWL_STYLE, IsZoomed, MINMAXINFO, NCCALCSIZE_PARAMS, STYLESTRUCT,
        WM_GETMINMAXINFO, WM_NCACTIVATE, WM_NCCALCSIZE, WM_STYLECHANGING, WS_BORDER, WS_CAPTION,
        WS_THICKFRAME,
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
        if GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool() {
            let inflate_x = (mmi.ptMaxSize.x - (mi.rcMonitor.right - mi.rcMonitor.left)) / 2;
            let inflate_y = (mmi.ptMaxSize.y - (mi.rcMonitor.bottom - mi.rcMonitor.top)) / 2;
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
            if GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool()
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
///
/// client/摆窗修正方案另经 tools/win32-probes/top-inset-probe.ps1 实证：
/// 还原态 clientTop 8px→1px、最大化 rect=work+膨胀 且 client=rcWork 精确
/// 贴合、边框+阴影完好、不触发细框环的标题栏坑。
/// SetWindowSubclass 必须在窗口属主线程调用（跨线程失败，见 factory.rs）。
#[cfg(target_os = "windows")]
pub(crate) fn apply_native_frame(hwnd_val: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::SetWindowSubclass;
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_STYLE, GetWindowLongPtrW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WS_BORDER,
        WS_CAPTION, WS_THICKFRAME,
    };

    let hwnd = HWND(hwnd_val as *mut _);
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        SetWindowLongPtrW(
            hwnd,
            GWL_STYLE,
            (style & !(WS_CAPTION.0 as isize)) | WS_THICKFRAME.0 as isize | WS_BORDER.0 as isize,
        );
        let ok =
            SetWindowSubclass(hwnd, Some(native_frame_proc), NATIVE_FRAME_SUBCLASS_ID, 0).as_bool();
        // 让 DWM 按新样式 + 新 NCCALCSIZE 口径重估窗框
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED
                | SWP_NOMOVE
                | SWP_NOSIZE
                | SWP_NOZORDER
                | SWP_NOOWNERZORDER
                | SWP_NOACTIVATE,
        );
        ok
    }
}

/// apply_native_frame 的属主线程分发（SetWindowSubclass 跨线程静默失败，
/// 同 factory.rs install_frameless 的坑）：主线程内联执行；其他线程
///（皮肤右键菜单唤出管理器的命令路径跑在 tokio worker 上）转交主线程
/// 并有界等待——赶在 show() 前落框，避免先显后补的几帧无边框观感。
/// 失败仅记日志：5 秒维护定时器经 ensure_native_frame 兜底自愈。
#[cfg(target_os = "windows")]
pub(crate) fn apply_native_frame_on_owner_thread(app: &tauri::AppHandle, hwnd_val: isize) {
    let on_main = app
        .try_state::<AppState>()
        .map(|s| s.main_thread_id == std::thread::current().id())
        .unwrap_or(false);
    if on_main {
        if !apply_native_frame(hwnd_val) {
            log::error!("apply_native_frame: SetWindowSubclass failed for manager window");
        }
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    if let Err(e) = app.run_on_main_thread(move || {
        let ok = apply_native_frame(hwnd_val);
        let _ = tx.send(ok);
    }) {
        log::error!("run_on_main_thread failed for manager native frame: {}", e);
        return;
    }
    match rx.recv_timeout(std::time::Duration::from_secs(3)) {
        Ok(true) => {}
        Ok(false) => {
            log::error!("apply_native_frame: SetWindowSubclass failed for manager window")
        }
        Err(e) => log::error!(
            "apply_native_frame dispatch timed out for manager window: {} — 5s 维护定时器兜底",
            e
        ),
    }
}

/// 管理器窗原生窗框的轻量自愈（5 秒维护定时器用，必须在主线程调用）：
/// 子类同 id 重装幂等（只刷新引用数据，不触发 DWM 重估）；样式位被
/// tao apply_diff 带回 WS_CAPTION 等真脏时才剥净并补一次 FRAMECHANGED
///（皮肤侧「无条件 FRAMECHANGED 会不停给 DWM 画框机会」的同一口径）。
#[cfg(target_os = "windows")]
fn ensure_native_frame(hwnd_val: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::SetWindowSubclass;
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_STYLE, GetWindowLongPtrW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WS_BORDER,
        WS_CAPTION, WS_THICKFRAME,
    };

    let hwnd = HWND(hwnd_val as *mut _);
    unsafe {
        // 子类幂等重装（失败仅本轮错过，下轮再来）
        let _ = SetWindowSubclass(hwnd, Some(native_frame_proc), NATIVE_FRAME_SUBCLASS_ID, 0);
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let clean =
            (style & !(WS_CAPTION.0 as isize)) | WS_THICKFRAME.0 as isize | WS_BORDER.0 as isize;
        if style != clean {
            SetWindowLongPtrW(hwnd, GWL_STYLE, clean);
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED
                    | SWP_NOMOVE
                    | SWP_NOSIZE
                    | SWP_NOZORDER
                    | SWP_NOOWNERZORDER
                    | SWP_NOACTIVATE,
            );
        }
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
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateIconFromResourceEx, HICON, ICON_BIG, ICON_SMALL, LR_DEFAULTCOLOR, SM_CXICON,
        SM_CXSMICON, SM_CYICON, SM_CYSMICON, SendMessageW, WM_SETICON,
    };

    /// 打包进二进制的多尺寸 .ico（与 exe 资源图标同一份文件）
    const ICO: &[u8] = include_bytes!("../icons/icon.ico");
    // (small, big) 两枚 HICON 的句柄值，按 DPI 分档缓存——OnceLock 按首次
    // 调用窗的 DPI 建后永久缓存，DPI 变更（跨屏拖动/系统改缩放）后图标
    // 尺寸档失配
    type IconCache = std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<u32, Option<(usize, usize)>>>,
    >;
    static ICONS: IconCache =
        std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

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

    let dpi = unsafe { GetDpiForWindow(HWND(hwnd_val as *mut _)) };
    let icons = {
        let mut map = ICONS.lock().unwrap_or_else(|e| e.into_inner());
        *map.entry(dpi).or_insert_with(|| {
            let small = load_entry(
                unsafe { GetSystemMetricsForDpi(SM_CXSMICON, dpi) },
                unsafe { GetSystemMetricsForDpi(SM_CYSMICON, dpi) },
            );
            let big = load_entry(unsafe { GetSystemMetricsForDpi(SM_CXICON, dpi) }, unsafe {
                GetSystemMetricsForDpi(SM_CYICON, dpi)
            });
            small.zip(big).map(|(s, b)| (s.0 as usize, b.0 as usize))
        })
    };
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

    /// F1 回归：受保护系统根判定——提权态不得改变布局判定结果
    #[cfg(target_os = "windows")]
    #[test]
    fn protected_root_detection() {
        use super::is_under_protected_root;
        use std::path::PathBuf;
        let roots = vec![
            PathBuf::from(r"C:\Program Files"),
            PathBuf::from(r"C:\Program Files (x86)"),
            PathBuf::from(r"C:\Windows"),
        ];
        assert!(is_under_protected_root(
            &PathBuf::from(r"C:\Program Files\Driftlet"),
            &roots
        ));
        assert!(is_under_protected_root(
            &PathBuf::from(r"c:\program files (x86)\app"),
            &roots
        ));
        assert!(is_under_protected_root(
            &PathBuf::from(r"C:\Windows\Temp\x"),
            &roots
        ));
        // 边界：根本身、非分隔符前缀、普通目录
        assert!(is_under_protected_root(
            &PathBuf::from(r"C:\Program Files"),
            &roots
        ));
        assert!(!is_under_protected_root(
            &PathBuf::from(r"C:\Program Files-x\app"),
            &roots
        ));
        assert!(!is_under_protected_root(
            &PathBuf::from(r"D:\Tools\Driftlet"),
            &roots
        ));
    }

    fn args(v: &[&str]) -> impl Iterator<Item = String> {
        v.iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
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
        assert!(
            !legacy_skins.join("old-skin").exists(),
            "moved skin must leave the legacy dir"
        );
        assert_eq!(
            std::fs::read_to_string(skins_dir.join("clash-skin").join("index.html")).unwrap(),
            "new",
            "existing same-name skin must be kept"
        );

        let _ = std::fs::remove_dir_all(&app_data);
        let _ = std::fs::remove_dir_all(&portable);
    }
}
