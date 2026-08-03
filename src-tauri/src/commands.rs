use tauri::{AppHandle, Emitter, Manager};
use crate::AppState;
use crate::backup;
use crate::i18n::{tr, trf, Key};
use crate::skin::types::{SkinInfo, SkinDetail, SkinRuntimeConfig, AppConfig};
use crate::skin::loader;
use crate::skin::config;
use crate::skin::package;
use crate::skin::settings;
use crate::window::factory;

// ─── Capture Preview ───

/// 管理器专属命令的身份校验：capabilities 只约束核心/插件命令，app 自定义
/// 命令对任何窗口开放——皮肤可经注入桥的 __DESK_PP__.invoke 直达任意命令。
/// 故管理器命令在首行按窗口 label 把关；皮肤合法调用的命令
/// （start_skin_drag / start_skin_resize / show_skin_context_menu）不走这里。
fn require_manager(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        let lang = window.app_handle().state::<AppState>().lang();
        Err(tr(&lang, Key::ManagerOnly).to_string())
    }
}

#[tauri::command]
pub fn start_skin_drag(window: tauri::WebviewWindow) -> Result<(), String> {
    window.start_dragging().map_err(|e| e.to_string())
}

/// Border-drag resize for skins with window.resizable: the bridge reports
/// the edge/corner zone (pointerdown) and we synthesize the matching
/// WM_NCLBUTTONDOWN(HT*) — DefWindowProc then runs the system modal size
/// loop, same recipe as tao's start_dragging (ReleaseCapture + PostMessage).
#[tauri::command]
pub fn start_skin_resize(window: tauri::WebviewWindow, direction: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
        use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetCursorPos, PostMessageW, HTBOTTOM, HTBOTTOMLEFT,
            HTBOTTOMRIGHT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT,
            WM_NCLBUTTONDOWN,
        };

        let lang = window.app_handle().state::<AppState>().lang();
        let ht = match direction.as_str() {
            "w" => HTLEFT,
            "e" => HTRIGHT,
            "n" => HTTOP,
            "s" => HTBOTTOM,
            "nw" => HTTOPLEFT,
            "ne" => HTTOPRIGHT,
            "sw" => HTBOTTOMLEFT,
            "se" => HTBOTTOMRIGHT,
            other => return Err(trf(&lang, Key::InvalidResizeDirection, &[other])),
        };

        let hwnd = window.hwnd().map_err(|e| e.to_string())?;
        unsafe {
            let _ = ReleaseCapture();
            let mut pt = POINT::default();
            GetCursorPos(&mut pt).map_err(|e| format!("GetCursorPos failed: {:?}", e))?;
            // Screen coords packed as signed 16-bit halves.
            let packed = ((pt.y as u16 as u32) << 16) | (pt.x as u16 as u32);
            PostMessageW(
                Some(HWND(hwnd.0 as *mut _)),
                WM_NCLBUTTONDOWN,
                WPARAM(ht as usize),
                LPARAM(packed as isize),
            )
            .map_err(|e| format!("PostMessage failed: {:?}", e))?;
        }
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let lang = window.app_handle().state::<AppState>().lang();
        let _ = (window, direction);
        Err(tr(&lang, Key::ResizeWindowsOnly).to_string())
    }
}

/// Resolve a skin's on-disk directory by scanning (skin id 不一定等于文件夹名)
fn find_skin_dir(state: &AppState, skin_id: &str) -> Option<std::path::PathBuf> {
    loader::scan_skins_directory(&state.skins_dir)
        .into_iter()
        .find(|s| s.id == skin_id)
        .map(|s| s.directory)
}

#[tauri::command]
pub async fn capture_skin_preview(window: tauri::WebviewWindow, app: AppHandle, skin_id: String) -> Result<(), String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let lang = state.lang();
    let window = state.registry.get(&skin_id)
        .ok_or_else(|| tr(&lang, Key::PreviewNeedsLoadedSkin).to_string())?;

    let skin_dir = find_skin_dir(&state, &skin_id)
        .ok_or_else(|| trf(&lang, Key::SkinNotFound, &[skin_id.as_str()]))?;
    let preview_path = skin_dir.join("preview.png");

    #[cfg(target_os = "windows")]
    {
        crate::capture::capture_webview_to_png(
            &window,
            &preview_path,
            crate::i18n::normalize(&lang),
        )?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        return Err(tr(&lang, Key::PreviewWindowsOnly).to_string());
    }

    log::info!("Preview captured for skin '{}' → {:?}", skin_id, preview_path);
    Ok(())
}

// ─── Skin Discovery ───

#[tauri::command]
pub fn list_skins(window: tauri::WebviewWindow, app: AppHandle) -> Result<Vec<SkinInfo>, String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let skins = loader::scan_skins_directory(&state.skins_dir);
    let loaded_ids: Vec<String> = state.registry.loaded_ids();
    Ok(loader::build_skin_info_list(&skins, &loaded_ids))
}

#[tauri::command]
pub fn get_skin_detail(window: tauri::WebviewWindow, app: AppHandle, skin_id: String) -> Result<SkinDetail, String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let lang = state.lang();
    let skins = loader::scan_skins_directory(&state.skins_dir);
    let skin = skins.iter()
        .find(|s| s.id == skin_id)
        .ok_or_else(|| trf(&lang, Key::SkinNotFound, &[skin_id.as_str()]))?;

    let loaded = state.registry.is_loaded(&skin_id);
    let mut config = {
        let app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
        app_config.skin_settings.get(&skin_id).cloned()
            .unwrap_or_else(|| SkinRuntimeConfig::from_manifest(&skin.manifest))
    };
    // 面板需要有效值：None = 跟随 skin.json 的 window.resizable 默认
    config.resizable = Some(config.resizable.unwrap_or(skin.manifest.window.resizable));
    // 同上：None = 跟随 skin.json 的 window.zoom 默认
    config.zoom = Some(config.zoom.unwrap_or(skin.manifest.window.zoom));
    // 面板的宽高输入框显示当前实际尺寸 = 基础尺寸 × 有效 zoom
    let z = clamp_zoom(config.zoom.unwrap_or(1.0));
    config.width = ((config.width as f64) * z).round() as u32;
    config.height = ((config.height as f64) * z).round() as u32;
    // 「皮肤设置」页的用户值存在皮肤文件夹的 settings.json
    let overrides = settings::load_skin_settings(&skin.directory);
    let settings_values = loader::effective_settings(&skin.manifest, Some(&overrides));

    Ok(SkinDetail {
        id: skin.id.clone(),
        name: skin.manifest.name.clone(),
        name_en: skin.manifest.name_en.clone(),
        author: skin.manifest.author.clone(),
        version: skin.manifest.version.clone(),
        description: skin.manifest.description.clone(),
        description_en: skin.manifest.description_en.clone(),
        bilingual: skin.manifest.bilingual,
        directory: skin.directory.to_string_lossy().to_string(),
        loaded,
        config,
        settings_schema: skin.manifest.settings.clone(),
        settings_values,
    })
}

#[tauri::command]
pub fn refresh_skins(window: tauri::WebviewWindow, app: AppHandle) -> Result<Vec<SkinInfo>, String> {
    list_skins(window, app)
}

// ─── Skin Lifecycle ───

#[tauri::command]
pub async fn load_skin(window: tauri::WebviewWindow, app: AppHandle, skin_id: String) -> Result<(), String> {
    require_manager(&window)?;
    load_skin_impl(app, skin_id).await
}

/// load_skin 的内部实现。进程内调用方（reload、皮肤右键菜单、包安装）不走
/// IPC、没有可校验的调用窗口——它们的 IPC 入口已各自把关。
pub(crate) async fn load_skin_impl(app: AppHandle, skin_id: String) -> Result<(), String> {
    log::info!("load_skin called: {}", skin_id);
    let handle = app.clone();
    let sid = skin_id.clone();
    let outer_lang = app.state::<AppState>().lang();

    // Run on blocking thread pool to avoid freezing main thread
    tauri::async_runtime::spawn_blocking(move || {
        let state = handle.state::<AppState>();
        let lang = state.lang();

        if state.registry.is_loaded(&sid) {
            return Err(tr(&lang, Key::SkinAlreadyLoaded).to_string());
        }

        let skins = loader::scan_skins_directory(&state.skins_dir);
        let skin = skins.iter()
            .find(|s| s.id == sid)
            .ok_or_else(|| trf(&lang, Key::SkinNotFound, &[sid.as_str()]))?
            .clone();

        let config = {
            let app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
            app_config.skin_settings.get(&sid).cloned()
                .unwrap_or_else(|| SkinRuntimeConfig::from_manifest(&skin.manifest))
        };

        log::info!("Creating skin window for: {}", sid);
        let window = factory::create_skin_window(&handle, &skin, &config)?;
        log::info!("Window created successfully for: {}", sid);

        state.registry.register(sid.clone(), window);

        {
            let mut app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
            if !app_config.loaded_skins.contains(&sid) {
                app_config.loaded_skins.push(sid.clone());
            }
            app_config.skin_settings.entry(sid.clone()).or_insert(config);
            config::save_config(&state.config_dir, &app_config).map_err(|e| trf(&lang, Key::ConfigSaveFailed, &[&e.to_string()]))?;
        }

        let _ = handle.emit("skin-loaded", &sid);
        Ok(())
    }).await.map_err(|e| trf(&outer_lang, Key::TaskFailed, &[&e.to_string()]))?
}

#[tauri::command]
pub async fn unload_skin(window: tauri::WebviewWindow, app: AppHandle, skin_id: String) -> Result<(), String> {
    require_manager(&window)?;
    unload_skin_impl(app, skin_id).await
}

/// unload_skin 的内部实现（进程内调用见 load_skin_impl 注释）。
pub(crate) async fn unload_skin_impl(app: AppHandle, skin_id: String) -> Result<(), String> {
    let handle = app.clone();
    let sid = skin_id.clone();
    let outer_lang = app.state::<AppState>().lang();

    tauri::async_runtime::spawn_blocking(move || {
        let state = handle.state::<AppState>();
        let lang = state.lang();
        let label = factory::skin_window_label(&sid);

        // 皮肤未加载时明确报错，而非静默成功
        if !state.registry.is_loaded(&sid) {
            return Err(tr(&lang, Key::SkinNotLoaded).to_string());
        }

        factory::destroy_skin_window(&handle, &label)?;
        state.registry.unregister(&sid);

        {
            let mut app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
            app_config.loaded_skins.retain(|id| id != &sid);
            config::save_config(&state.config_dir, &app_config).map_err(|e| trf(&lang, Key::ConfigSaveFailed, &[&e.to_string()]))?;
        }

        let _ = handle.emit("skin-unloaded", &sid);
        Ok(())
    }).await.map_err(|e| trf(&outer_lang, Key::TaskFailed, &[&e.to_string()]))?
}

#[tauri::command]
pub async fn reload_skin(window: tauri::WebviewWindow, app: AppHandle, skin_id: String) -> Result<(), String> {
    require_manager(&window)?;
    reload_skin_impl(app, skin_id).await
}

/// reload_skin 的内部实现（进程内调用见 load_skin_impl 注释）。
pub(crate) async fn reload_skin_impl(app: AppHandle, skin_id: String) -> Result<(), String> {
    let was_loaded = {
        let state = app.state::<AppState>();
        state.registry.is_loaded(&skin_id)
    };

    if was_loaded {
        unload_skin_impl(app.clone(), skin_id.clone()).await?;
    }
    load_skin_impl(app, skin_id).await
}

// ─── Configuration ───

#[tauri::command]
pub fn set_skin_opacity(window: tauri::WebviewWindow, app: AppHandle, skin_id: String, opacity: f64) -> Result<(), String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let lang = state.lang();
    let window = state.registry.get(&skin_id)
        .ok_or_else(|| tr(&lang, Key::SkinNotLoaded).to_string())?;

    let clamped = opacity.clamp(0.1, 1.0);
    window.eval(&format!("document.documentElement.style.opacity = '{}';", clamped))
        .map_err(|e| format!("opacity: {}", e))?;

    {
        let mut app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
        app_config.skin_settings.entry(skin_id).or_default().opacity = clamped;
        config::save_config(&state.config_dir, &app_config).map_err(|e| trf(&lang, Key::ConfigSaveFailed, &[&e.to_string()]))?;
    }

    #[cfg(target_os = "windows")]
    factory::force_clean_skin_window(&window);

    Ok(())
}

#[tauri::command]
pub fn set_skin_always_on_top(window: tauri::WebviewWindow, app: AppHandle, skin_id: String, on: bool) -> Result<(), String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let lang = state.lang();
    // The window may be absent (skin not loaded) — still persist the choice
    // so it applies on next load.
    let window = state.registry.get(&skin_id);

    if let Some(window) = &window {
        #[cfg(target_os = "windows")]
        {
            // Unpin from the desktop layer BEFORE making the window topmost.
            // set_always_on_top(true) sets WS_EX_TOPMOST; calling unpin afterwards
            // (SetWindowPos(HWND_BOTTOM)) would strip that flag and leave the
            // window at the bottom instead of on top.
            if on {
                if let Ok(hwnd) = window.hwnd() {
                    app.state::<AppState>().pinner.unpin(&skin_id, hwnd.0 as isize);
                }
            }
        }

        window.set_always_on_top(on).map_err(|e| format!("{}", e))?;

        #[cfg(target_os = "windows")]
        {
            // Turning always-on-top OFF switches the skin to on-desktop mode:
            // the two placements are mutually exclusive and exactly one is
            // always on (there is no "neither" state).
            if !on {
                if let Ok(hwnd) = window.hwnd() {
                    app.state::<AppState>().pinner.pin(&skin_id, hwnd.0 as isize);
                }
            }
        }
    }

    {
        let mut app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
        let entry = app_config.skin_settings.entry(skin_id.clone()).or_default();
        entry.always_on_top = on;
        // Exactly one of always_on_top / on_desktop is active at any time.
        entry.on_desktop = !on;
        config::save_config(&state.config_dir, &app_config).map_err(|e| trf(&lang, Key::ConfigSaveFailed, &[&e.to_string()]))?;
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(window) = &window {
            factory::force_clean_skin_window(window);
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn set_skin_on_desktop(window: tauri::WebviewWindow, app: AppHandle, skin_id: String, on: bool) -> Result<(), String> {
    require_manager(&window)?;
    // Capture the current on-screen position before any reload, so toggling
    // "on desktop" doesn't make the skin jump back to stale config coordinates.
    let current_pos = {
        let state = app.state::<AppState>();
        if let Some(window) = state.registry.get(&skin_id) {
            if let Ok(pos) = window.outer_position() {
                window.scale_factor().ok().map(|sf| (
                    (pos.x as f64 / sf).round() as i32,
                    (pos.y as f64 / sf).round() as i32,
                ))
            } else {
                None
            }
        } else {
            None
        }
    };

    {
        let state = app.state::<AppState>();
        let lang = state.lang();
        let mut app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
        let entry = app_config.skin_settings.entry(skin_id.clone()).or_default();
        if let Some((x, y)) = current_pos {
            entry.x = Some(x);
            entry.y = Some(y);
        }
        entry.on_desktop = on;
        // Exactly one of always_on_top / on_desktop is active at any time.
        entry.always_on_top = !on;
        config::save_config(&state.config_dir, &app_config).map_err(|e| trf(&lang, Key::ConfigSaveFailed, &[&e.to_string()]))?;
    }

    if app.state::<AppState>().registry.is_loaded(&skin_id) {
        reload_skin_impl(app.clone(), skin_id.clone()).await?;
        // reload_skin recreates the window — force-clean the new one
        let state2 = app.state::<AppState>();
        if let Some(window) = state2.registry.get(&skin_id) {
            #[cfg(target_os = "windows")]
            factory::force_clean_skin_window(&window);
        }
    }

    Ok(())
}

/// 位置/尺寸的钳制范围（逻辑像素）：IPC 入口统一把关，防离谱值把窗口
/// 扔到屏幕外或撑爆桌面。
const MAX_COORD: i32 = 32767;
const MAX_DIMENSION: u32 = 10000;

#[tauri::command]
pub fn set_skin_position(window: tauri::WebviewWindow, app: AppHandle, skin_id: String, x: i32, y: i32) -> Result<(), String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let lang = state.lang();
    let window = state.registry.get(&skin_id).ok_or_else(|| tr(&lang, Key::SkinNotLoaded).to_string())?;
    let x = x.clamp(-MAX_COORD, MAX_COORD);
    let y = y.clamp(-MAX_COORD, MAX_COORD);
    // Config and UI store logical pixels.
    window.set_position(tauri::LogicalPosition::new(x as f64, y as f64))
        .map_err(|e| format!("{}", e))?;

    let mut app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
    let entry = app_config.skin_settings.entry(skin_id).or_default();
    entry.x = Some(x);
    entry.y = Some(y);
    config::save_config(&state.config_dir, &app_config).map_err(|e| trf(&lang, Key::ConfigSaveFailed, &[&e.to_string()]))?;

    Ok(())
}

#[tauri::command]
pub fn set_skin_size(window: tauri::WebviewWindow, app: AppHandle, skin_id: String, width: u32, height: u32) -> Result<(), String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let lang = state.lang();
    let window = state.registry.get(&skin_id).ok_or_else(|| tr(&lang, Key::SkinNotLoaded).to_string())?;
    let width = width.clamp(1, MAX_DIMENSION);
    let height = height.clamp(1, MAX_DIMENSION);
    // Config and UI store logical pixels (same convention as position and
    // as create_skin_window's inner_size).  PhysicalSize would disagree with
    // the creation size on scaled displays and "revert" on every reload.
    // 面板输入 = 当前实际尺寸（zoom ≠ 100% 时的所见大小）；配置里持久化
    // 的仍是 100% 基础尺寸 = 实际 ÷ 有效 zoom。
    let zoom = effective_zoom(&state, &skin_id);
    window.set_size(tauri::LogicalSize::new(width as f64, height as f64))
        .map_err(|e| format!("{}", e))?;

    {
        let mut app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
        let entry = app_config.skin_settings.entry(skin_id).or_default();
        entry.width = ((width as f64) / zoom).round() as u32;
        entry.height = ((height as f64) / zoom).round() as u32;
        config::save_config(&state.config_dir, &app_config).map_err(|e| trf(&lang, Key::ConfigSaveFailed, &[&e.to_string()]))?;
    }

    Ok(())
}

#[tauri::command]
pub fn update_skin_config(
    window: tauri::WebviewWindow,
    app: AppHandle,
    skin_id: String,
    config_update: SkinRuntimeConfig,
) -> Result<(), String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let lang = state.lang();
    // 与单项 setter 同一套钳制：opacity 0.1–1.0、zoom 过 clamp_zoom、
    // 位置 ±MAX_COORD、宽高 1–MAX_DIMENSION。
    let mut config_update = config_update;
    config_update.opacity = config_update.opacity.clamp(0.1, 1.0);
    config_update.zoom = config_update.zoom.map(clamp_zoom);
    config_update.x = config_update.x.map(|x| x.clamp(-MAX_COORD, MAX_COORD));
    config_update.y = config_update.y.map(|y| y.clamp(-MAX_COORD, MAX_COORD));
    config_update.width = config_update.width.clamp(1, MAX_DIMENSION);
    config_update.height = config_update.height.clamp(1, MAX_DIMENSION);
    // 先存配置再应用窗口：应用失败（如窗口恰好已关闭）不致丢持久化。
    {
        let mut app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
        app_config.skin_settings.insert(skin_id.clone(), config_update.clone());
        config::save_config(&state.config_dir, &app_config).map_err(|e| trf(&lang, Key::ConfigSaveFailed, &[&e.to_string()]))?;
    }
    if let Some(window) = state.registry.get(&skin_id) {
        factory::update_window_config(&window, &config_update)?;
    }
    Ok(())
}

/// Reset one skin's persisted data — both the window config (opacity, mode,
/// position/size, lock) and the custom settings. Dropping the whole
/// `skin_settings` entry plus deleting the skin folder's `settings.json`
/// reverts everything to the skin.json defaults.
/// A loaded skin is reloaded so its window picks up the defaults (position,
/// size, and the rebaked __DESK_PP__.settings) right away.
#[tauri::command]
pub async fn reset_skin_config(window: tauri::WebviewWindow, app: AppHandle, skin_id: String) -> Result<(), String> {
    require_manager(&window)?;
    {
        let state = app.state::<AppState>();
        let lang = state.lang();
        let mut app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
        app_config.skin_settings.remove(&skin_id);
        config::save_config(&state.config_dir, &app_config).map_err(|e| trf(&lang, Key::ConfigSaveFailed, &[&e.to_string()]))?;
    }

    // 「皮肤设置」页的用户值在皮肤文件夹的 settings.json：重置 = 连文件一起删
    {
        let state = app.state::<AppState>();
        let lang = state.lang();
        if let Some(skin_dir) = find_skin_dir(&state, &skin_id) {
            match std::fs::remove_file(skin_dir.join(settings::SETTINGS_FILENAME)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(trf(&lang, Key::ConfigSaveFailed, &[&e.to_string()])),
            }
        }
    }

    if app.state::<AppState>().registry.is_loaded(&skin_id) {
        reload_skin_impl(app.clone(), skin_id.clone()).await?;
        // reload_skin recreates the window — force-clean the new one
        let state = app.state::<AppState>();
        if let Some(window) = state.registry.get(&skin_id) {
            #[cfg(target_os = "windows")]
            factory::force_clean_skin_window(&window);
        }
    }

    Ok(())
}

// ─── Custom Skin Settings ───

#[tauri::command]
pub fn set_skin_custom_setting(
    window: tauri::WebviewWindow,
    app: AppHandle,
    skin_id: String,
    key: String,
    value: serde_json::Value,
) -> Result<(), String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let lang = state.lang();
    let skins = loader::scan_skins_directory(&state.skins_dir);
    let skin = skins.iter()
        .find(|s| s.id == skin_id)
        .ok_or_else(|| trf(&lang, Key::SkinNotFound, &[skin_id.as_str()]))?;
    let def = skin.manifest.settings.iter()
        .find(|d| d.key == key)
        .ok_or_else(|| trf(&lang, Key::SkinHasNoSetting, &[skin_id.as_str(), key.as_str()]))?;

    let value = validate_custom_setting(def, &value, &lang)?;

    // 覆盖值写进皮肤文件夹的 settings.json，不再触碰全局 config。
    // 持锁覆盖 load→save 全程：settings.json 有两个写入方（管理器与皮肤
    // 自身的 skin_set_setting），不持锁会互相丢更新。
    let _guard = state.settings_lock.lock().unwrap_or_else(|e| e.into_inner());
    let mut overrides = settings::load_skin_settings(&skin.directory);
    overrides.insert(key.clone(), value.clone());
    settings::save_skin_settings(&skin.directory, &overrides)
        .map_err(|e| trf(&lang, Key::ConfigSaveFailed, &[&e.to_string()]))?;
    drop(_guard);

    // Push to the live window so the skin can apply the change without a
    // reload: update the baked __DESK_PP__.settings and notify listeners.
    if let Some(window) = state.registry.get(&skin_id) {
        let key_json = serde_json::to_string(&key).map_err(|e| e.to_string())?;
        let val_json = serde_json::to_string(&value).map_err(|e| e.to_string())?;
        let script = format!(
            "(function(){{var k={key},v={val};var b=window.__DESK_PP__;if(b){{b.settings=b.settings||{{}};b.settings[k]=v;}}document.dispatchEvent(new CustomEvent('desk-setting-changed',{{detail:{{key:k,value:v}}}}));}})();",
            key = key_json, val = val_json
        );
        let _ = window.eval(&script);
    }

    Ok(())
}

/// Validate / coerce a custom setting value against its declared kind.
/// Shared by the manager command (`set_skin_custom_setting`) and the
/// skin-facing command (`skin_api::skin_set_setting`).
pub(crate) fn validate_custom_setting(
    def: &crate::skin::types::SkinSettingDef,
    value: &serde_json::Value,
    lang: &str,
) -> Result<serde_json::Value, String> {
    use crate::skin::types::SkinSettingKind;
    use serde_json::Value;
    match def.kind {
        SkinSettingKind::Boolean => value.as_bool()
            .map(Value::Bool)
            .ok_or_else(|| trf(lang, Key::SettingNeedsBool, &[def.key.as_str()])),
        SkinSettingKind::Number | SkinSettingKind::Slider => {
            let mut n = value.as_f64()
                .ok_or_else(|| trf(lang, Key::SettingNeedsNumber, &[def.key.as_str()]))?;
            if let Some(min) = def.min { n = n.max(min); }
            if let Some(max) = def.max { n = n.min(max); }
            Ok(serde_json::json!(n))
        }
        SkinSettingKind::Text => {
            let s = require_str(def, value, Key::WhatText, lang)?;
            Ok(Value::String(s.chars().take(256).collect()))
        }
        SkinSettingKind::LongText => {
            let s = require_str(def, value, Key::WhatText, lang)?;
            Ok(Value::String(s.chars().take(4000).collect()))
        }
        SkinSettingKind::Time => {
            let s = require_str(def, value, Key::WhatTime, lang)?;
            if is_valid_time(s) {
                Ok(Value::String(s.to_string()))
            } else {
                Err(trf(lang, Key::SettingNeedsTime, &[def.key.as_str()]))
            }
        }
        SkinSettingKind::Date => {
            let s = require_str(def, value, Key::WhatDate, lang)?;
            if is_valid_ymd(s) {
                Ok(Value::String(s.to_string()))
            } else {
                Err(trf(lang, Key::SettingNeedsDate, &[def.key.as_str()]))
            }
        }
        SkinSettingKind::DateTime => {
            // 单点日期时间；与 TimeRange 同一套归一化（空串 = 未设）
            let s = require_str(def, value, Key::WhatTime, lang)?;
            match normalize_datetime(s) {
                Some(norm) => Ok(Value::String(norm)),
                None => Err(trf(lang, Key::SettingNeedsDateTime, &[def.key.as_str()])),
            }
        }
        SkinSettingKind::Password => {
            // 掩码只是显示形态；校验与 text 完全一致
            let s = require_str(def, value, Key::WhatText, lang)?;
            Ok(Value::String(s.chars().take(256).collect()))
        }
        SkinSettingKind::Palette => {
            // 调色板带透明度调整：#rrggbb（视为不透明）或 #rrggbbaa
            let s = require_str(def, value, Key::WhatColor, lang)?;
            let valid = (s.len() == 7 || s.len() == 9) && s.starts_with('#')
                && s[1..].chars().all(|c| c.is_ascii_hexdigit());
            if valid {
                Ok(Value::String(s.to_string()))
            } else {
                Err(trf(lang, Key::SettingNeedsColor, &[def.key.as_str()]))
            }
        }
        SkinSettingKind::Select | SkinSettingKind::Radio => {
            let s = require_str(def, value, Key::WhatOption, lang)?;
            if def.options.iter().any(|o| o.value == s) {
                Ok(Value::String(s.to_string()))
            } else {
                Err(trf(lang, Key::SettingValueNotAllowed, &[def.key.as_str(), s]))
            }
        }
        SkinSettingKind::MultiSelect => {
            let arr = value.as_array()
                .ok_or_else(|| trf(lang, Key::SettingNeedsArray, &[def.key.as_str()]))?;
            let mut seen = std::collections::HashSet::new();
            let mut out = Vec::new();
            for item in arr {
                let s = item.as_str()
                    .ok_or_else(|| trf(lang, Key::SettingNeedsStringArray, &[def.key.as_str()]))?;
                if !def.options.iter().any(|o| o.value == s) {
                    return Err(trf(lang, Key::SettingValueNotAllowed, &[def.key.as_str(), s]));
                }
                if seen.insert(s) {
                    out.push(Value::String(s.to_string()));
                }
            }
            Ok(Value::Array(out))
        }
        SkinSettingKind::TimeRange => {
            let obj = value.as_object()
                .ok_or_else(|| trf(lang, Key::SettingNeedsTimeRange, &[def.key.as_str()]))?;
            let start = obj.get("start").and_then(|v| v.as_str())
                .ok_or_else(|| trf(lang, Key::SettingMissingStart, &[def.key.as_str()]))?;
            let end = obj.get("end").and_then(|v| v.as_str())
                .ok_or_else(|| trf(lang, Key::SettingMissingEnd, &[def.key.as_str()]))?;
            let (Some(start), Some(end)) =
                (normalize_datetime(start), normalize_datetime(end))
            else {
                return Err(trf(lang, Key::SettingNeedsDateTime, &[def.key.as_str()]));
            };
            Ok(serde_json::json!({"start": start, "end": end}))
        }
        SkinSettingKind::TaskList => {
            // Internal caps — intentionally not surfaced to users: the add
            // button simply disappears at 500 items, overlong items and
            // overflow entries are silently truncated.
            const MAX_TASKS: usize = 500;
            const MAX_ITEM_LEN: usize = 200;
            let arr = value.as_array()
                .ok_or_else(|| trf(lang, Key::SettingNeedsArray, &[def.key.as_str()]))?;
            let mut out = Vec::new();
            for item in arr.iter().take(MAX_TASKS) {
                let s = item.as_str()
                    .ok_or_else(|| trf(lang, Key::SettingNeedsStringArray, &[def.key.as_str()]))?;
                out.push(Value::String(s.chars().take(MAX_ITEM_LEN).collect()));
            }
            Ok(Value::Array(out))
        }
        SkinSettingKind::TodoList => {
            // Same silent caps as TaskList
            const MAX_TASKS: usize = 500;
            const MAX_ITEM_LEN: usize = 200;
            let arr = value.as_array()
                .ok_or_else(|| trf(lang, Key::SettingNeedsArray, &[def.key.as_str()]))?;
            let mut out = Vec::new();
            for item in arr.iter().take(MAX_TASKS) {
                let obj = item.as_object()
                    .ok_or_else(|| trf(lang, Key::EntryNeedsObject, &[def.key.as_str()]))?;
                let text = obj.get("text").and_then(|v| v.as_str())
                    .ok_or_else(|| trf(lang, Key::EntryMissingText, &[def.key.as_str()]))?;
                let done = obj.get("done").and_then(|v| v.as_bool()).unwrap_or(false);
                out.push(serde_json::json!({
                    "text": text.chars().take(MAX_ITEM_LEN).collect::<String>(),
                    "done": done,
                }));
            }
            Ok(Value::Array(out))
        }
        SkinSettingKind::DateTaskList => {
            // Same silent caps as TaskList
            const MAX_TASKS: usize = 500;
            const MAX_ITEM_LEN: usize = 200;
            let arr = value.as_array()
                .ok_or_else(|| trf(lang, Key::SettingNeedsArray, &[def.key.as_str()]))?;
            let mut out = Vec::new();
            for item in arr.iter().take(MAX_TASKS) {
                let obj = item.as_object()
                    .ok_or_else(|| trf(lang, Key::EntryNeedsObject, &[def.key.as_str()]))?;
                let time = obj.get("time").and_then(|v| v.as_str())
                    .ok_or_else(|| trf(lang, Key::EntryMissingTime, &[def.key.as_str()]))?;
                let text = obj.get("text").and_then(|v| v.as_str())
                    .ok_or_else(|| trf(lang, Key::EntryMissingText, &[def.key.as_str()]))?;
                let Some(time) = normalize_datetime(time) else {
                    return Err(trf(lang, Key::EntryTimeFormat, &[def.key.as_str()]));
                };
                out.push(serde_json::json!({
                    "time": time,
                    "text": text.chars().take(MAX_ITEM_LEN).collect::<String>(),
                }));
            }
            Ok(Value::Array(out))
        }
        SkinSettingKind::Weekdays => {
            // Fixed set, normalized to Monday-first order
            const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
            let arr = value.as_array()
                .ok_or_else(|| trf(lang, Key::SettingNeedsArray, &[def.key.as_str()]))?;
            let mut picked = Vec::new();
            for item in arr {
                let s = item.as_str()
                    .ok_or_else(|| trf(lang, Key::SettingNeedsStringArray, &[def.key.as_str()]))?;
                if !DAYS.contains(&s) {
                    return Err(trf(lang, Key::InvalidWeekday, &[def.key.as_str(), s]));
                }
                if !picked.contains(&s) {
                    picked.push(s);
                }
            }
            Ok(Value::Array(
                DAYS.iter()
                    .filter(|d| picked.contains(d))
                    .map(|d| Value::String(d.to_string()))
                    .collect(),
            ))
        }
        SkinSettingKind::Font => {
            let s = require_str(def, value, Key::WhatFont, lang)?;
            Ok(Value::String(s.chars().take(128).collect()))
        }
    }
}

fn require_str<'a>(
    def: &crate::skin::types::SkinSettingDef,
    value: &'a serde_json::Value,
    what: Key,
    lang: &str,
) -> Result<&'a str, String> {
    value.as_str()
        .ok_or_else(|| trf(lang, Key::SettingNeedsWhat, &[def.key.as_str(), tr(lang, what)]))
}

/// "HH:MM" or "HH:MM:SS", 24-hour.
fn is_valid_time(s: &str) -> bool {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 && parts.len() != 3 {
        return false;
    }
    is_fixed_uint(parts[0], 2, 0, 23)
        && is_fixed_uint(parts[1], 2, 0, 59)
        && (parts.len() == 2 || is_fixed_uint(parts[2], 2, 0, 59))
}

/// "YYYY-MM-DD" (day range is 1-31 regardless of month — good enough for a
/// widget setting, keeps us dependency-free).
fn is_valid_ymd(s: &str) -> bool {
    let mut parts = s.split('-');
    let (Some(y), Some(m), Some(d), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    is_fixed_uint(y, 4, 0, 9999) && is_fixed_uint(m, 2, 1, 12) && is_fixed_uint(d, 2, 1, 31)
}

/// "YYYY-MM-DD HH:MM" or "YYYY-MM-DD HH:MM:SS" → canonical
/// "YYYY-MM-DD HH:MM:SS".  Empty string stays empty ("unset").
/// (datetime-local inputs omit the seconds part when it is 00.)
fn normalize_datetime(s: &str) -> Option<String> {
    if s.is_empty() {
        return Some(String::new());
    }
    let (date, time) = s.split_once(' ')?;
    if !is_valid_ymd(date) {
        return None;
    }
    let parts: Vec<&str> = time.split(':').collect();
    if parts.len() != 2 && parts.len() != 3 {
        return None;
    }
    if !is_fixed_uint(parts[0], 2, 0, 23) || !is_fixed_uint(parts[1], 2, 0, 59) {
        return None;
    }
    let secs = if parts.len() == 3 {
        if !is_fixed_uint(parts[2], 2, 0, 59) {
            return None;
        }
        parts[2].to_string()
    } else {
        "00".to_string()
    };
    Some(format!("{} {}:{}:{}", date, parts[0], parts[1], secs))
}

/// True when `s` is exactly `width` ASCII digits and within [min, max].
fn is_fixed_uint(s: &str, width: usize, min: u32, max: u32) -> bool {
    s.len() == width
        && s.chars().all(|c| c.is_ascii_digit())
        && s.parse::<u32>().map(|n| (min..=max).contains(&n)).unwrap_or(false)
}

/// Show the skin window's right-click popup menu and run the chosen action.
/// Invoked from the injected bridge's contextmenu handler.
#[tauri::command]
pub async fn show_skin_context_menu(app: AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    let lang = app.state::<AppState>().lang();
    let Some(skin_id) = window.label().strip_prefix("skin-").map(str::to_string) else {
        return Err(tr(&lang, Key::NotASkinWindow).to_string());
    };

    // TrackPopupMenu is modal and must run on the window's owner (main) thread.
    #[cfg(target_os = "windows")]
    let choice = {
        let (tx, rx) = std::sync::mpsc::channel();
        app.run_on_main_thread(move || {
            let _ = tx.send(factory::track_skin_popup_menu(&window, &lang));
        })
        .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    };
    #[cfg(not(target_os = "windows"))]
    let choice = 0u32;

    match choice {
        factory::SKIN_MENU_OPEN_CONFIG => {
            // Surface the manager and tell it to open this skin's config page.
            crate::tray::show_manager_window(&app);
            app.emit("open-skin-config", &skin_id).map_err(|e| e.to_string())?;
        }
        factory::SKIN_MENU_RELOAD => {
            // Fire-and-forget: the reload destroys the invoking webview
            // itself.  Awaiting it here would let the command return AFTER
            // the window is gone, and wry would deliver the invoke response
            // to a dead hwnd (harmless but noisy PostMessage warning in
            // debug builds).  Errors are logged instead of returned.
            tauri::async_runtime::spawn(async move {
                if let Err(e) = reload_skin_impl(app, skin_id).await {
                    log::error!("skin menu reload failed: {}", e);
                }
            });
        }
        factory::SKIN_MENU_UNLOAD => {
            // Fire-and-forget — same reason as SKIN_MENU_RELOAD.
            tauri::async_runtime::spawn(async move {
                if let Err(e) = unload_skin_impl(app, skin_id).await {
                    log::error!("skin menu unload failed: {}", e);
                }
            });
        }
        _ => {} // menu cancelled
    }
    Ok(())
}

#[tauri::command]
pub fn set_skin_position_locked(window: tauri::WebviewWindow, app: AppHandle, skin_id: String, locked: bool) -> Result<(), String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let lang = state.lang();
    let window = state.registry.get(&skin_id).ok_or_else(|| tr(&lang, Key::SkinNotLoaded).to_string())?;

    if locked {
        let _ = window.eval(r#"
            (function(){
                if(window.__DESK_PP__) window.__DESK_PP__.positionLocked = true;
                var s=document.createElement('style');
                s.id='desk-lock-style';
                s.textContent='.drag-region{-webkit-app-region:no-drag!important;cursor:default!important}';
                document.head.appendChild(s);
            })();
        "#);
    } else {
        let _ = window.eval(r#"
            (function(){
                if(window.__DESK_PP__) window.__DESK_PP__.positionLocked = false;
                var s=document.getElementById('desk-lock-style');
                if(s) s.remove();
            })();
        "#);
    }

    {
        let mut app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
        app_config.skin_settings.entry(skin_id).or_default().position_locked = locked;
        config::save_config(&state.config_dir, &app_config).map_err(|e| trf(&lang, Key::ConfigSaveFailed, &[&e.to_string()]))?;
    }

    #[cfg(target_os = "windows")]
    factory::force_clean_skin_window(&window);

    Ok(())
}

/// 边框缩放开关（「窗口」页）：即时生效——桥接的 setResizable 翻转标志并
/// 同步边框提示层；同时持久化用户选择（Some(v)），None 语义为跟随 skin.json。
#[tauri::command]
pub fn set_skin_resizable(window: tauri::WebviewWindow, app: AppHandle, skin_id: String, resizable: bool) -> Result<(), String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let lang = state.lang();
    let window = state.registry.get(&skin_id).ok_or_else(|| tr(&lang, Key::SkinNotLoaded).to_string())?;

    let _ = window.eval(&format!(
        "window.__DESK_PP__ && window.__DESK_PP__.setResizable({});",
        resizable
    ));

    {
        let mut app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
        app_config.skin_settings.entry(skin_id).or_default().resizable = Some(resizable);
        config::save_config(&state.config_dir, &app_config).map_err(|e| trf(&lang, Key::ConfigSaveFailed, &[&e.to_string()]))?;
    }

    Ok(())
}

/// 缩放比例上下限（「窗口」页滑块同范围）。
pub const MIN_ZOOM: f64 = 0.5;
pub const MAX_ZOOM: f64 = 2.0;

/// 把缩放比例钳制到支持范围；NaN/无穷回落 1.0。
pub(crate) fn clamp_zoom(z: f64) -> f64 {
    if z.is_finite() {
        z.clamp(MIN_ZOOM, MAX_ZOOM)
    } else {
        1.0
    }
}

/// 缩放比例（「窗口」页）：内容经 WebView2 ZoomFactor 与窗口同倍缩放——
/// 实际窗口 = 基础尺寸（config.width/height）× zoom，页面 CSS 视口保持
/// 设计尺寸，布局不重排。皮肤未加载时仅持久化，下次建窗生效（与
/// set_skin_edge_snap 同款语义）。
#[tauri::command]
pub fn set_skin_zoom(window: tauri::WebviewWindow, app: AppHandle, skin_id: String, zoom: f64) -> Result<(), String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let lang = state.lang();
    let zoom = clamp_zoom(zoom);

    // entry 缺失时按 manifest 尺寸播种（否则 or_default 的 300×200 会把
    // 下面的 set_size 缩错窗口；条目缺失 ⟹ 从未拖过 ⟹ 实际尺寸 = manifest）。
    let skins = loader::scan_skins_directory(&state.skins_dir);
    let skin = skins.iter()
        .find(|s| s.id == skin_id)
        .ok_or_else(|| trf(&lang, Key::SkinNotFound, &[skin_id.as_str()]))?;

    let (w, h) = {
        let mut app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
        let entry = app_config.skin_settings.entry(skin_id.clone()).or_insert_with(|| {
            SkinRuntimeConfig {
                width: skin.manifest.window.width,
                height: skin.manifest.window.height,
                ..Default::default()
            }
        });
        entry.zoom = Some(zoom);
        let size = (entry.width, entry.height);
        config::save_config(&state.config_dir, &app_config).map_err(|e| trf(&lang, Key::ConfigSaveFailed, &[&e.to_string()]))?;
        size
    };

    if let Some(window) = state.registry.get(&skin_id) {
        // 内容缩放失败仅降级（尺寸仍正确），不阻断
        if let Err(e) = window.set_zoom(zoom) {
            log::warn!("set_zoom({}) failed for '{}': {}", zoom, skin_id, e);
        }
        let actual_w = (w as f64 * zoom).round() as u32;
        let actual_h = (h as f64 * zoom).round() as u32;
        window.set_size(tauri::LogicalSize::new(actual_w as f64, actual_h as f64))
            .map_err(|e| format!("{}", e))?;
        // 基础尺寸没变 → Resized 的「未变化跳过」不会回发事件；面板的宽高
        // 输入框显示的是实际尺寸，这里显式通知刷新。
        let _ = app.emit("skin-resized", serde_json::json!({
            "skinId": skin_id,
            "width": actual_w,
            "height": actual_h,
        }));
    }

    Ok(())
}

/// 有效缩放比例：面板覆盖（config）优先，否则 skin.json 默认，全程钳制。
fn effective_zoom(state: &AppState, skin_id: &str) -> f64 {
    let override_zoom = {
        let app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
        app_config.skin_settings.get(skin_id).and_then(|e| e.zoom)
    };
    match override_zoom {
        Some(z) => clamp_zoom(z),
        None => {
            let skins = loader::scan_skins_directory(&state.skins_dir);
            skins.iter().find(|s| s.id == skin_id)
                .map(|s| clamp_zoom(s.manifest.window.zoom))
                .unwrap_or(1.0)
        }
    }
}

/// 边缘吸附开关（「窗口」页）：即时生效——更新吸附注册表（窗口子类的
/// WM_MOVING 处理按 HWND 查该表）；同时持久化。皮肤未加载时仅持久化，
/// 下次加载建窗时随 SkinRuntimeConfig 生效（与 set_always_on_top 同款语义）。
#[tauri::command]
pub fn set_skin_edge_snap(window: tauri::WebviewWindow, app: AppHandle, skin_id: String, on: bool) -> Result<(), String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let lang = state.lang();

    let gap = {
        let mut app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
        let entry = app_config.skin_settings.entry(skin_id.clone()).or_default();
        entry.edge_snap = on;
        let gap = entry.snap_gap;
        config::save_config(&state.config_dir, &app_config).map_err(|e| trf(&lang, Key::ConfigSaveFailed, &[&e.to_string()]))?;
        gap
    };

    if let Some(window) = state.registry.get(&skin_id) {
        if let Ok(hwnd) = window.hwnd() {
            crate::window::snap::upsert(hwnd.0 as isize, on, gap);
        }
    }

    Ok(())
}

/// 吸附间距（逻辑像素，「窗口」页）：clamp 上限后持久化并同步到吸附注册表。
#[tauri::command]
pub fn set_skin_snap_gap(window: tauri::WebviewWindow, app: AppHandle, skin_id: String, gap: u32) -> Result<(), String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let lang = state.lang();
    let gap = gap.min(crate::window::snap::MAX_SNAP_GAP);

    let enabled = {
        let mut app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
        let entry = app_config.skin_settings.entry(skin_id.clone()).or_default();
        entry.snap_gap = gap;
        let enabled = entry.edge_snap;
        config::save_config(&state.config_dir, &app_config).map_err(|e| trf(&lang, Key::ConfigSaveFailed, &[&e.to_string()]))?;
        enabled
    };

    if let Some(window) = state.registry.get(&skin_id) {
        if let Ok(hwnd) = window.hwnd() {
            crate::window::snap::upsert(hwnd.0 as isize, enabled, gap);
        }
    }

    Ok(())
}

// ─── Skin Package (.dskin / .zip) ───

/// 打开文件选择器，只过滤 .dskin 皮肤包（避免用户在一堆 zip 里挑错文件）
#[tauri::command]
pub async fn pick_skin_package(window: tauri::WebviewWindow, app: AppHandle) -> Result<Option<String>, String> {
    require_manager(&window)?;
    use tauri_plugin_dialog::DialogExt;
    let handle = app.clone();
    let lang = app.state::<AppState>().lang();
    let filter_name = tr(&lang, Key::DskinFilterName);
    tauri::async_runtime::spawn_blocking(move || {
        let path = handle.dialog().file()
            .add_filter(filter_name, &["dskin"])
            .blocking_pick_file();
        path.map(|p| p.to_string())
    }).await.map_err(|e| trf(&lang, Key::DialogError, &[&e.to_string()]))
}

// ─── Layout Backup (export / import) ───

/// 导出布局备份：保存对话框选路径后，把 config/ + skins/ 打成一个 zip
/// （含备份清单 driftlet-backup.json）。返回保存路径；用户取消返回 None。
#[tauri::command]
pub async fn export_config(window: tauri::WebviewWindow, app: AppHandle) -> Result<Option<String>, String> {
    require_manager(&window)?;
    use tauri_plugin_dialog::DialogExt;
    let handle = app.clone();
    let lang = app.state::<AppState>().lang();
    let filter_name = tr(&lang, Key::BackupFilterName);
    let dest = tauri::async_runtime::spawn_blocking(move || {
        handle.dialog().file()
            .add_filter(filter_name, &["zip"])
            .set_file_name("driftlet-backup.zip")
            .blocking_save_file()
            .map(|p| p.to_string())
    }).await.map_err(|e| trf(&lang, Key::DialogError, &[&e.to_string()]))?;
    let Some(dest) = dest else { return Ok(None) };
    backup::export_backup(&app, std::path::Path::new(&dest))?;
    Ok(Some(dest))
}

/// 导入布局备份：校验（体积/条目/zip-slip/必须含 config/config.json）→
/// 卸载全部皮肤 → 暂存替换 config/ 与 skins/（失败整体回滚）→ 重建运行时
/// 状态并按备份加载皮肤。用户取消返回 false。
#[tauri::command]
pub async fn import_config(window: tauri::WebviewWindow, app: AppHandle) -> Result<bool, String> {
    require_manager(&window)?;
    use tauri_plugin_dialog::DialogExt;
    let handle = app.clone();
    let lang = app.state::<AppState>().lang();
    let filter_name = tr(&lang, Key::BackupFilterName);
    let picked = tauri::async_runtime::spawn_blocking(move || {
        handle.dialog().file()
            .add_filter(filter_name, &["zip"])
            .blocking_pick_file()
            .map(|p| p.to_string())
    }).await.map_err(|e| trf(&lang, Key::DialogError, &[&e.to_string()]))?;
    let Some(picked) = picked else { return Ok(false) };
    backup::import_backup(app, std::path::Path::new(&picked)).await?;
    Ok(true)
}

/// 检查皮肤包：不是合法皮肤包时返回错误提示；
/// 合法时返回包信息与安装状态（new / update / reinstall / downgrade），
/// 前端据此弹确认框。
#[tauri::command]
pub fn inspect_skin_package(window: tauri::WebviewWindow, app: AppHandle, package_path: String) -> Result<package::PackageInfo, String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let lang = state.lang();
    package::inspect_package(std::path::Path::new(&package_path), &state.skins_dir, &lang)
}

/// 安装/更新皮肤包（用户已确认）。已加载的皮肤先卸载，安装后**保持卸载**
/// （更新/重装/回退的默认行为——新版本的窗口配置/代码差异由用户显式加载
/// 时生效）；用户数据（skin_settings[id]）原样保留 —— 配置按皮肤 id 归属，
/// 与文件解耦。
#[tauri::command]
pub async fn install_skin_package(window: tauri::WebviewWindow, app: AppHandle, package_path: String) -> Result<SkinInfo, String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let lang = state.lang();

    // 串行化安装：并发装同一个包会互踩目录（remove vs copy），
    // 第二个请求在此排队等前一个装完，随后按 reinstall 正常走完。
    let _install_guard = state.install_lock.lock().await;

    // 先检查一次，拿到皮肤 id 判断是否需要卸载
    let info = package::inspect_package(std::path::Path::new(&package_path), &state.skins_dir, &lang)?;
    let was_loaded = state.registry.is_loaded(&info.id);

    if was_loaded {
        unload_skin_impl(app.clone(), info.id.clone()).await?;
    }

    let skin = package::install_package(std::path::Path::new(&package_path), &state.skins_dir, &lang)?;
    let preview = loader::find_preview_image(&skin.directory);
    let info_out = SkinInfo {
        id: skin.id.clone(),
        name: skin.manifest.name.clone(),
        name_en: skin.manifest.name_en.clone(),
        author: skin.manifest.author.clone(),
        version: skin.manifest.version.clone(),
        description: skin.manifest.description.clone(),
        description_en: skin.manifest.description_en.clone(),
        bilingual: skin.manifest.bilingual,
        loaded: false,
        has_error: false,
        error_msg: None,
        preview,
    };

    Ok(info_out)
}

/// 取走冷启动时命令行传入的待安装皮肤包路径（双击 .dskin 启动）。
/// 用 take() 消费掉，保证引导页只弹一次；热启动路径走的是
/// `open-skin-package` 事件，不经过这里。
#[tauri::command]
pub fn take_pending_package_install(window: tauri::WebviewWindow, app: AppHandle) -> Option<String> {
    require_manager(&window).ok()?;
    app.state::<AppState>()
        .pending_package
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
}

#[tauri::command]
pub fn remove_skin(window: tauri::WebviewWindow, app: AppHandle, skin_id: String) -> Result<(), String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let lang = state.lang();
    if state.registry.is_loaded(&skin_id) {
        return Err(tr(&lang, Key::UnloadBeforeRemove).to_string());
    }
    if let Some(skin_dir) = find_skin_dir(&state, &skin_id) {
        std::fs::remove_dir_all(&skin_dir)
            .map_err(|e| trf(&lang, Key::RemoveSkinFailed, &[&e.to_string()]))?;
    }
    {
        let mut app_config = state.config.lock().unwrap_or_else(|e| e.into_inner());
        app_config.loaded_skins.retain(|id| id != &skin_id);
        app_config.skin_settings.remove(&skin_id);
        config::save_config(&state.config_dir, &app_config).map_err(|e| trf(&lang, Key::ConfigSaveFailed, &[&e.to_string()]))?;
    }
    Ok(())
}

// ─── App Config ───

#[tauri::command]
pub fn get_app_config(window: tauri::WebviewWindow, app: AppHandle) -> Result<AppConfig, String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let config = state.config.lock().unwrap_or_else(|e| e.into_inner());
    Ok(config.clone())
}

#[tauri::command]
pub fn save_app_config(window: tauri::WebviewWindow, app: AppHandle, new_config: AppConfig) -> Result<(), String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let mut new_config = new_config;
    // 保存前归一化：always_on_top/on_desktop 恰好一真（与 load 时同一规则）。
    config::normalize_mode_flags(&mut new_config);
    // version 强制写当前版本：写回旧版本（如 1）会在下次启动触发重复迁移。
    new_config.version = AppConfig::default().version;
    // 非法语言值回退 zh-CN，并同步 state.language 运行时镜像。
    new_config.language = crate::i18n::normalize(&new_config.language).to_string();
    {
        let mut config = state.config.lock().unwrap_or_else(|e| e.into_inner());
        *config = new_config.clone();
    }
    *state.language.lock().unwrap_or_else(|e| e.into_inner()) = new_config.language.clone();
    config::save_config(&state.config_dir, &new_config)
}

// ─── Settings ───

#[tauri::command]
pub fn set_autostart(window: tauri::WebviewWindow, app: AppHandle, on: bool) -> Result<(), String> {
    require_manager(&window)?;
    use tauri_plugin_autostart::ManagerExt;

    if on {
        app.autolaunch().enable().map_err(|e| e.to_string())?;
    } else {
        app.autolaunch().disable().map_err(|e| e.to_string())?;
    }

    let state = app.state::<AppState>();
    let mut config = state.config.lock().unwrap_or_else(|e| e.into_inner());
    config.autostart = on;
    config::save_config(&state.config_dir, &config)
}

#[tauri::command]
pub fn get_autostart(window: tauri::WebviewWindow, app: AppHandle) -> Result<bool, String> {
    require_manager(&window)?;
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_theme(window: tauri::WebviewWindow, app: AppHandle, theme: String) -> Result<(), String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let mut config = state.config.lock().unwrap_or_else(|e| e.into_inner());
    config.theme = theme;
    config::save_config(&state.config_dir, &config)
}

/// 皮肤热重载总开关（持久化 config.hot_reload + 即时翻转运行时标志）。
/// 仅影响 debug 构建的 watcher（release 从不启动 watcher）。
#[tauri::command]
pub fn set_hot_reload(window: tauri::WebviewWindow, app: AppHandle, on: bool) -> Result<(), String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    state.hot_reload_enabled.store(on, std::sync::atomic::Ordering::Relaxed);
    let mut config = state.config.lock().unwrap_or_else(|e| e.into_inner());
    config.hot_reload = on;
    config::save_config(&state.config_dir, &config)
}

/// Persist the UI language ("zh-CN" | "en"), update runtime state, and
/// rebuild the tray menu so it switches language immediately. 已加载的皮肤
/// 窗口同步收到 desk-language-changed 事件（皮肤可让自己的界面跟随切换）。
#[tauri::command]
pub fn set_language(window: tauri::WebviewWindow, app: AppHandle, language: String) -> Result<(), String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    // 非法语言值归一到支持的语言（未知一律回 zh-CN）
    let language = crate::i18n::normalize(&language).to_string();
    {
        let mut config = state.config.lock().unwrap_or_else(|e| e.into_inner());
        config.language = language.clone();
        config::save_config(&state.config_dir, &config)?;
    }
    *state.language.lock().unwrap_or_else(|e| e.into_inner()) = language.clone();
    crate::tray::rebuild_tray_menu(&app, &language);

    // 推送已加载皮肤：更新桥成员并派发事件（皮肤不监听也不受影响，下次
    // reload 时桥会烘焙新语言）。eval 失败（窗口正在销毁）仅忽略。
    let lang_json = serde_json::to_string(&language).unwrap_or_else(|_| "\"zh-CN\"".into());
    let script = format!(
        r#"(function(){{if(!window.__DESK_PP__)return;window.__DESK_PP__.language={l};document.dispatchEvent(new CustomEvent('desk-language-changed',{{detail:{{language:{l}}}}}));}})();"#,
        l = lang_json
    );
    for id in state.registry.loaded_ids() {
        if let Some(win) = state.registry.get(&id) {
            let _ = win.eval(&script);
        }
    }
    Ok(())
}

/// Persist the global toggle-visibility hotkey ("" = disabled) and swap the
/// live registration; on registration failure the previous hotkey is
/// restored and the new value is NOT persisted.
#[tauri::command]
pub fn set_hotkey(window: tauri::WebviewWindow, app: AppHandle, hotkey: String) -> Result<(), String> {
    require_manager(&window)?;
    crate::hotkey::apply_hotkey(&app, hotkey.trim())?;
    let state = app.state::<AppState>();
    let mut config = state.config.lock().unwrap_or_else(|e| e.into_inner());
    config.hotkey_toggle_skins = hotkey.trim().to_string();
    config::save_config(&state.config_dir, &config)
}

/// Startup hotkey-registration failure, if any (the configured combo).
/// Consumed once by the frontend on init, like take_pending_package_install.
#[tauri::command]
pub fn take_hotkey_error(window: tauri::WebviewWindow, app: AppHandle) -> Option<String> {
    require_manager(&window).ok()?;
    app.state::<AppState>()
        .hotkey_error
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
}

/// Whether the OS draws its own window frame accents (Win11+ DWM rounds
/// corners and strokes a 1px outline; Win10 does neither for frameless
/// windows). The frontend uses this to decide if it must paint its own 1px
/// border. sysinfo's os_version is RtlGetVersion-based, so the build number
/// is truthful (unlike manifest-capped GetVersionEx); Win11 = build 22000+.
#[tauri::command]
pub fn is_windows_11_or_newer(window: tauri::WebviewWindow) -> Result<bool, String> {
    require_manager(&window)?;
    Ok(sysinfo::System::os_version()
        .and_then(|v| {
            v.split('.')
                .nth(2)
                .and_then(|build| build.parse::<u32>().ok())
        })
        .map(|build| build >= 22000)
        // Unparseable / non-Windows: assume modern so we don't paint a
        // border where the OS may already draw one.
        .unwrap_or(true))
}

// ─── Utility ───

#[tauri::command]
pub fn open_skins_folder(window: tauri::WebviewWindow, app: AppHandle) -> Result<(), String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let lang = state.lang();
    // Ensure directory exists
    let _ = std::fs::create_dir_all(&state.skins_dir);
    let path = state.skins_dir.to_string_lossy().to_string();
    open_path(&path, &lang)
}

#[tauri::command]
pub fn open_skin_folder(window: tauri::WebviewWindow, app: AppHandle, skin_id: String) -> Result<(), String> {
    require_manager(&window)?;
    let state = app.state::<AppState>();
    let lang = state.lang();
    let skin_path = find_skin_dir(&state, &skin_id)
        .ok_or_else(|| trf(&lang, Key::SkinFolderNotFound, &[skin_id.as_str()]))?;
    open_path(&skin_path.to_string_lossy(), &lang)
}

/// Cross-platform "open in file manager" using std::process::Command
fn open_path(path: &str, lang: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|e| trf(lang, Key::OpenFailed, &[&e.to_string()]))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| trf(lang, Key::OpenFailed, &[&e.to_string()]))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| trf(lang, Key::OpenFailed, &[&e.to_string()]))?;
    }
    Ok(())
}

// ─── System Fonts ───

/// Enumerate installed font families (Windows: GDI EnumFontFamiliesExW).
/// Feeds the "font" custom-setting control in the manager.
#[tauri::command]
pub fn list_system_fonts(window: tauri::WebviewWindow) -> Result<Vec<String>, String> {
    require_manager(&window)?;
    #[cfg(target_os = "windows")]
    {
        Ok(enum_system_fonts())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(Vec::new())
    }
}

#[cfg(target_os = "windows")]
fn enum_system_fonts() -> Vec<String> {
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, EnumFontFamiliesExW, LOGFONTW, TEXTMETRICW,
        DEFAULT_CHARSET,
    };

    unsafe extern "system" fn collect(
        lf: *const LOGFONTW,
        _tm: *const TEXTMETRICW,
        _font_type: u32,
        lparam: LPARAM,
    ) -> i32 {
        let fonts = &mut *(lparam.0 as *mut Vec<String>);
        let face = &(*lf).lfFaceName;
        let len = face.iter().position(|&c| c == 0).unwrap_or(face.len());
        let name = String::from_utf16_lossy(&face[..len]);
        // Skip "@"-prefixed vertical (rotated) font variants
        if !name.starts_with('@') {
            fonts.push(name);
        }
        1
    }

    let mut fonts: Vec<String> = Vec::new();
    unsafe {
        let hdc = CreateCompatibleDC(None);
        if !hdc.0.is_null() {
            let mut lf = LOGFONTW::default();
            lf.lfCharSet = DEFAULT_CHARSET;
            EnumFontFamiliesExW(
                hdc,
                &lf,
                Some(collect),
                LPARAM(&mut fonts as *mut _ as isize),
                0,
            );
            let _ = DeleteDC(hdc);
        }
    }
    fonts.sort();
    fonts.dedup();
    fonts
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::skin::types::{SkinSettingDef, SkinSettingKind, SkinSettingOption};

    /// Tests exercise validation logic, not wording — always use zh-CN.
    fn validate_custom_setting_zh(
        def: &SkinSettingDef,
        value: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        validate_custom_setting(def, value, "zh-CN")
    }

    fn def(kind: SkinSettingKind) -> SkinSettingDef {
        SkinSettingDef {
            key: "k".into(),
            kind,
            label: None,
            label_en: None,
            description: None,
            description_en: None,
            group: None,
            group_en: None,
            default: None,
            min: None,
            max: None,
            step: None,
            options: vec![],
        }
    }

    fn with_options(kind: SkinSettingKind, values: &[&str]) -> SkinSettingDef {
        SkinSettingDef {
            options: values
                .iter()
                .map(|v| SkinSettingOption { value: v.to_string(), label: None, label_en: None })
                .collect(),
            ..def(kind)
        }
    }

    #[test]
    fn validates_time() {
        let d = def(SkinSettingKind::Time);
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("07:30")).is_ok());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("23:59")).is_ok());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("07:30:05")).is_ok());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("23:59:59")).is_ok());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("25:00")).is_err());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("7:30")).is_err());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("12:60")).is_err());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("12:00:60")).is_err());
    }

    #[test]
    fn validates_weekdays() {
        let d = def(SkinSettingKind::Weekdays);
        // 去重并按周一至周日归一化顺序
        let v = validate_custom_setting_zh(&d, &serde_json::json!(["sun", "mon", "sun"])).unwrap();
        assert_eq!(v, serde_json::json!(["mon", "sun"]));
        assert!(validate_custom_setting_zh(&d, &serde_json::json!([])).is_ok());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!(["funday"])).is_err());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("mon")).is_err());
    }

    #[test]
    fn validates_datetasklist() {
        let d = def(SkinSettingKind::DateTaskList);
        let ok = serde_json::json!([
            {"time": "2026-07-20 12:00:01", "text": "写周报"},
            {"time": "", "text": "未定时任务"}
        ]);
        assert!(validate_custom_setting_zh(&d, &ok).is_ok());
        // 秒为 00 时 datetime-local 会省略秒段 —— 归一化为 HH:MM:SS
        let short = serde_json::json!([{"time": "2026-07-22 09:00", "text": "晨会"}]);
        let v = validate_custom_setting_zh(&d, &short).unwrap();
        assert_eq!(v[0]["time"], "2026-07-22 09:00:00");
        let bad_time = serde_json::json!([{"time": "2026-07-20", "text": "x"}]);
        assert!(validate_custom_setting_zh(&d, &bad_time).is_err());
        let missing_text = serde_json::json!([{"time": "2026-07-20 12:00:01"}]);
        assert!(validate_custom_setting_zh(&d, &missing_text).is_err());
        let not_obj = serde_json::json!(["x"]);
        assert!(validate_custom_setting_zh(&d, &not_obj).is_err());
        // 超长文本截断
        let long = "x".repeat(300);
        let v = validate_custom_setting_zh(&d, &serde_json::json!([{"time": "", "text": long}])).unwrap();
        assert_eq!(v[0]["text"].as_str().unwrap().chars().count(), 200);
    }

    #[test]
    fn clamps_zoom() {
        assert_eq!(clamp_zoom(1.0), 1.0);
        assert_eq!(clamp_zoom(0.9), 0.9);
        assert_eq!(clamp_zoom(0.1), MIN_ZOOM);
        assert_eq!(clamp_zoom(5.0), MAX_ZOOM);
        assert_eq!(clamp_zoom(f64::NAN), 1.0);
        assert_eq!(clamp_zoom(f64::INFINITY), 1.0);
    }

    #[test]
    fn validates_todolist() {
        let d = def(SkinSettingKind::TodoList);
        let ok = serde_json::json!([
            {"text": "写周报", "done": true},
            {"text": "给绿植浇水", "done": false}
        ]);
        assert_eq!(validate_custom_setting_zh(&d, &ok).unwrap(), ok);
        // done 缺省补 false
        let v = validate_custom_setting_zh(&d, &serde_json::json!([{"text": "x"}])).unwrap();
        assert_eq!(v, serde_json::json!([{"text": "x", "done": false}]));
        // 非数组 / 非 object / 缺 text 均报错
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("x")).is_err());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!(["x"])).is_err());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!([{"done": true}])).is_err());
        // 超长文本截断
        let long = "x".repeat(300);
        let v = validate_custom_setting_zh(&d, &serde_json::json!([{"text": long, "done": false}])).unwrap();
        assert_eq!(v[0]["text"].as_str().unwrap().chars().count(), 200);
    }

    #[test]
    fn validates_datetime() {
        let d = def(SkinSettingKind::DateTime);
        assert_eq!(
            validate_custom_setting_zh(&d, &serde_json::json!("2026-07-20 12:00")).unwrap(),
            serde_json::json!("2026-07-20 12:00:00")
        );
        // 空串 = 未设
        assert_eq!(
            validate_custom_setting_zh(&d, &serde_json::json!("")).unwrap(),
            serde_json::json!("")
        );
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("2026-07-20")).is_err());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("2026-13-01 00:00")).is_err());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!(42)).is_err());
    }

    #[test]
    fn validates_password() {
        let d = def(SkinSettingKind::Password);
        assert_eq!(
            validate_custom_setting_zh(&d, &serde_json::json!("s3cret")).unwrap(),
            serde_json::json!("s3cret")
        );
        assert!(validate_custom_setting_zh(&d, &serde_json::json!(42)).is_err());
        // 与 text 同款 256 字符截断
        let long = "x".repeat(300);
        let v = validate_custom_setting_zh(&d, &serde_json::json!(long)).unwrap();
        assert_eq!(v.as_str().unwrap().chars().count(), 256);
    }

    #[test]
    fn validates_font() {
        let d = def(SkinSettingKind::Font);
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("Microsoft YaHei UI")).is_ok());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!(42)).is_err());
    }

    #[test]
    fn validates_date() {
        let d = def(SkinSettingKind::Date);
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("2026-02-29")).is_ok());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("2026-13-01")).is_err());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("2026-01-32")).is_err());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("2026-1-01")).is_err());
    }

    #[test]
    fn validates_timerange() {
        let d = def(SkinSettingKind::TimeRange);
        let ok = serde_json::json!({"start": "2026-07-20 12:00:01", "end": "2026-08-20 00:00:01"});
        assert!(validate_custom_setting_zh(&d, &ok).is_ok());
        // 秒为 00 时 datetime-local 会省略秒段 —— 归一化为 HH:MM:SS
        let short = serde_json::json!({"start": "2026-07-20 12:00", "end": "2026-08-20 00:00"});
        let v = validate_custom_setting_zh(&d, &short).unwrap();
        assert_eq!(v["start"], "2026-07-20 12:00:00");
        assert_eq!(v["end"], "2026-08-20 00:00:00");
        // empty = unset, allowed
        let unset = serde_json::json!({"start": "", "end": ""});
        assert!(validate_custom_setting_zh(&d, &unset).is_ok());
        let bad = serde_json::json!({"start": "2026-07-20", "end": "2026-08-20 00:00:01"});
        assert!(validate_custom_setting_zh(&d, &bad).is_err());
        let bad_sec = serde_json::json!({"start": "2026-07-20 12:00:60", "end": ""});
        assert!(validate_custom_setting_zh(&d, &bad_sec).is_err());
        let missing = serde_json::json!({"start": "2026-07-20 12:00:01"});
        assert!(validate_custom_setting_zh(&d, &missing).is_err());
    }

    #[test]
    fn validates_multiselect() {
        let d = with_options(SkinSettingKind::MultiSelect, &["a", "b"]);
        // duplicates removed, order kept
        let v = validate_custom_setting_zh(&d, &serde_json::json!(["a", "b", "a"])).unwrap();
        assert_eq!(v, serde_json::json!(["a", "b"]));
        assert!(validate_custom_setting_zh(&d, &serde_json::json!([])).is_ok());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!(["c"])).is_err());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("a")).is_err());
    }

    #[test]
    fn validates_radio() {
        let d = with_options(SkinSettingKind::Radio, &["day", "night"]);
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("day")).is_ok());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("dusk")).is_err());
    }

    #[test]
    fn validates_palette_with_alpha() {
        let d = def(SkinSettingKind::Palette);
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("#ff3333")).is_ok());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("#ff3333cc")).is_ok());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("#FF3333FF")).is_ok());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("#ff333")).is_err());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("#ff3333c")).is_err());
        assert!(validate_custom_setting_zh(&d, &serde_json::json!("#gg3333ff")).is_err());
    }

    #[test]
    fn slider_clamps_to_bounds() {
        let mut d = def(SkinSettingKind::Slider);
        d.min = Some(0.0);
        d.max = Some(100.0);
        assert_eq!(validate_custom_setting_zh(&d, &serde_json::json!(150)).unwrap(), serde_json::json!(100.0));
        assert_eq!(validate_custom_setting_zh(&d, &serde_json::json!(-5)).unwrap(), serde_json::json!(0.0));
        assert_eq!(validate_custom_setting_zh(&d, &serde_json::json!(60)).unwrap(), serde_json::json!(60.0));
    }

    #[test]
    fn tasklist_truncates_silently() {
        let d = def(SkinSettingKind::TaskList);
        // 501 items → capped at 500
        let many: Vec<String> = (0..501).map(|i| i.to_string()).collect();
        let v = validate_custom_setting_zh(&d, &serde_json::json!(many)).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 500);
        // overlong item → 200 chars
        let long = "x".repeat(300);
        let v = validate_custom_setting_zh(&d, &serde_json::json!([long])).unwrap();
        assert_eq!(v[0].as_str().unwrap().chars().count(), 200);
        // non-string item → error
        assert!(validate_custom_setting_zh(&d, &serde_json::json!([1])).is_err());
    }
}
