//! Global hotkey: one keystroke hides or shows all LOADED skin windows.
//! Skins that are not loaded have no window and are not affected.
//!
//! Toggle semantics are deliberately STATELESS: window visibility itself is
//! the state — if any skin window is visible, hide them all; if none is,
//! show them all. Loading / unloading / reloading skins at any moment can
//! never drift a "hidden" flag, and no hook in the load/reload paths is
//! needed. The manager window never participates.
//!
//! Pinned (on-desktop) skins are unaffected by the pinner while hidden:
//! the enforcement loop only maintains z-order (desktop.rs), and
//! `hide()`/`show()` keep the HWND and its frameless subclass intact.

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::i18n::{tr, trf, Key};
use crate::AppState;

/// 当前「实际注册成功」的组合键。配置里的 hotkey_toggle_skins 可能因注册
/// 失败（组合被别的程序占用）与真实注册状态脱节——「输入的组合 == 配置值」
/// 推不出「已注册」，apply_hotkey 的短路必须看这个。
static REGISTERED_COMBO: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn registered_combo() -> Option<String> {
    REGISTERED_COMBO
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

fn set_registered_combo(combo: Option<String>) {
    *REGISTERED_COMBO.lock().unwrap_or_else(|e| e.into_inner()) = combo;
}

/// True when at least one skin is loaded and every skin window is hidden.
pub fn all_skins_hidden(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let ids = state.registry.loaded_ids();
    !ids.is_empty()
        && ids.iter().all(|id| {
            state
                .registry
                .get(id)
                .map(|w| !w.is_visible().unwrap_or(true))
                .unwrap_or(true)
        })
}

/// Hide all skin windows if any is visible; otherwise show them all.
pub fn toggle_all_skins(app: &AppHandle) {
    let state = app.state::<AppState>();
    let ids = state.registry.loaded_ids();
    if ids.is_empty() {
        return;
    }
    let hide = !all_skins_hidden(app);
    for id in &ids {
        if let Some(window) = state.registry.get(id) {
            let result = if hide { window.hide() } else { window.show() };
            if let Err(e) = result {
                log::warn!("Failed to {} skin '{}': {}", if hide { "hide" } else { "show" }, id, e);
            }
        }
    }
    // Keep the tray check item in sync with reality.
    sync_tray_toggle_item(app);
}

/// Keep the tray "all skins hidden" check item in sync with reality.
/// Clone the handle out of the guard so the MutexGuard drops before we
/// call back into tauri.
/// 本函数是全部皮肤可见性变化的漏斗（全局热键 toggle_all_skins、托盘
/// 勾选项、皮肤窗 Alt+F4 降级隐藏都经这里同步托盘），故同时向管理器
/// 发 skins-visibility-changed——列表/配置面板的「已隐藏」徽标按
/// 真实窗口状态刷新，不靠热键簿记。
pub fn sync_tray_toggle_item(app: &AppHandle) {
    let state = app.state::<AppState>();
    let item = state.toggle_item.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(item) = item {
        let _ = item.set_checked(all_skins_hidden(app));
    }
    let _ = app.emit_to("main", "skins-visibility-changed", ());
}

/// Register the configured hotkey at startup. A parse failure only logs
/// (the config was validated when set); a registration failure — the combo
/// is taken by another app (RegisterHotKey is first-come-first-served) —
/// is ALSO stashed in AppState::hotkey_error so the frontend can surface it
/// with a toast instead of leaving the user wondering why the key is dead.
pub fn register_from_config(app: &AppHandle) {
    let combo = {
        app.state::<AppState>()
            .config
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .hotkey_toggle_skins
            .clone()
    };
    if combo.is_empty() {
        return;
    }
    match parse_validated(&combo, &app.state::<AppState>().lang()) {
        Ok(shortcut) => {
            if let Err(e) = app.global_shortcut().register(shortcut) {
                log::warn!("Failed to register hotkey '{}': {}", combo, e);
                set_registered_combo(None);
                *app.state::<AppState>().hotkey_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(combo);
            } else {
                set_registered_combo(Some(combo));
            }
        }
        Err(e) => log::warn!("Invalid hotkey in config '{}': {}", combo, e),
    }
}

/// Re-sync the registration after the config was swapped wholesale (backup
/// import): unregister whatever is ACTUALLY registered right now (tracked in
/// REGISTERED_COMBO — the new config's value can't tell us that), then
/// register the configured combo fresh.  Plain `register_from_config` must
/// not be used here: it never unregisters, so a live registration makes the
/// OS call fail and the error stash would toast "hotkey taken" on the next
/// manager reload even though nothing is wrong.
pub fn reregister_from_config(app: &AppHandle) {
    if let Some(old) = registered_combo() {
        if let Ok(shortcut) = old.parse::<Shortcut>() {
            let _ = app.global_shortcut().unregister(shortcut);
        }
        set_registered_combo(None);
    }
    register_from_config(app);
}

/// Swap the registered hotkey for `combo` ("" = disable).
///
/// On registration failure (typically the combo is taken by another app)
/// the previous hotkey is restored so the user is never silently left with
/// none. Caller persists the new value to config after this succeeds.
pub fn apply_hotkey(app: &AppHandle, combo: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    let old_combo = {
        state
            .config
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .hotkey_toggle_skins
            .clone()
    };
    let combo = combo.trim();
    // 短路前提 = 组合没变【且】它真的注册成功了——启动时组合被占用会注册
    // 失败，配置值与注册状态脱节，此时重新输入同一组合必须真正再走注册。
    if combo == old_combo && registered_combo().as_deref() == Some(combo) {
        return Ok(());
    }
    // Validate the NEW combo before touching the old registration.
    let new = if combo.is_empty() {
        None
    } else {
        Some(parse_validated(combo, &lang)?)
    };
    let old = if old_combo.is_empty() {
        None
    } else {
        old_combo.trim().parse::<Shortcut>().ok()
    };

    let mut old_still_registered = false;
    if let Some(old) = old {
        // 注销失败不阻断换绑，但必须留痕：否则旧注册泄漏（REGISTERED_COMBO
        // 只记新键，旧组合要到重启才释放）
        match app.global_shortcut().unregister(old) {
            Ok(()) => {}
            Err(e) => {
                log::warn!("failed to unregister previous hotkey: {}", e);
                old_still_registered = true;
            }
        }
    }
    if let Some(new) = new {
        if let Err(e) = app.global_shortcut().register(new) {
            // Roll back so the previous hotkey keeps working.
            // 簿记按「旧注册真实存活状态」记账：注销旧键失败时旧注册还
            // 活着（不必也无法回滚），回滚失败不等于旧键失效——
            // REGISTERED_COMBO 置 None 会与「旧键仍生效」脱节
            let restored = if old_still_registered {
                true
            } else {
                old.map(|o| app.global_shortcut().register(o).is_ok())
                    .unwrap_or(false)
            };
            set_registered_combo(if restored {
                Some(old_combo.trim().to_string())
            } else {
                None
            });
            let msg = e.to_string();
            return Err(trf(&lang, Key::HotkeyRegisterFailed, &[&msg]));
        }
    }
    set_registered_combo(if combo.is_empty() {
        None
    } else {
        Some(combo.to_string())
    });
    Ok(())
}

/// Parse a "Ctrl+Alt+D" style combo. A bare key with no modifier is rejected
/// — globally hijacking an unmodified key would break normal typing.
fn parse_validated(combo: &str, lang: &str) -> Result<Shortcut, String> {
    let shortcut: Shortcut = combo
        .trim()
        .parse()
        .map_err(|_| tr(lang, Key::HotkeyInvalid).to_string())?;
    if shortcut.mods.is_empty() {
        return Err(tr(lang, Key::HotkeyInvalid).to_string());
    }
    Ok(shortcut)
}
