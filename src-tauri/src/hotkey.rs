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

use crate::AppState;
use crate::i18n::{Key, tr, trf};

/// 当前「实际注册成功」的组合键——**一律存 Shortcut Display 规范化串**
///（crate 的 Display 产出小写修饰词 + keyboard-types Code 名，如
/// `shift+control+alt+KeyD`，与配置里的 `Ctrl+Shift+Alt+D` 原串不等）。
/// 事故依据（审查发现，发布阻断级）：dispatch_hotkey 拿事件 shortcut 的
/// Display 与簿记比对，簿记存配置原串 → 永不命中 → 全局热键静默全死。
/// 凡写 REGISTERED_COMBO 必须写 Display 串，凡比较必须先规范化。
static REGISTERED_COMBO: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// 皮肤专属显隐热键注册表：规范化组合串（Shortcut Display）→ 皮肤 id。
/// 常驻——皮肤未加载时按下静默无效果（无窗可切），故无需加载/卸载钩子；
/// 启动与备份导入后由 sync_skin_hotkeys_from_config 按 config 全量重建。
static SKIN_HOTKEYS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, String>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn registered_combo() -> Option<String> {
    REGISTERED_COMBO
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

fn set_registered_combo(combo: Option<String>) {
    *REGISTERED_COMBO.lock().unwrap_or_else(|e| e.into_inner()) = combo;
}

// 全局热键动作 = 专注模式开关（focus.rs；原「显隐全部皮肤」已升级——
// 见 docs/proposals/专注模式方案-2026-09.md §3）。皮肤显隐子菜单与
// 皮肤专属热键仍走逐窗 toggle_one_skin 不动。

/// Keep the tray "all skins hidden" check item in sync with reality.
/// Clone the handle out of the guard so the MutexGuard drops before we
/// call back into tauri.
/// 本函数是全部皮肤可见性变化的漏斗（全局热键（focus::toggle 专注模式开关）、托盘
/// 勾选项、皮肤窗 Alt+F4 降级隐藏都经这里同步托盘），故同时向管理器
/// 发 skins-visibility-changed——列表/配置面板的「已隐藏」徽标按
/// 真实窗口状态刷新，不靠热键簿记。
/// 「皮肤显隐」子菜单的维护分两层：加载集（皮肤数）变了才重建整个
/// 托盘菜单（tray.rs 按当前清单重新生成子菜单项）；只显隐翻转时仅
/// 按真实可见性 set_checked——热键连发不抖菜单。
pub fn sync_tray_toggle_item(app: &AppHandle) {
    let state = app.state::<AppState>();

    // ① 加载集 vs 子菜单项集合：不一致才重建（rebuild_tray_menu 会顺带
    //    重新 stash toggle_item 与 skin_vis_items）
    let loaded: std::collections::HashSet<String> =
        state.registry.loaded_ids().into_iter().collect();
    let menu_ids: std::collections::HashSet<String> = {
        let items = state
            .skin_vis_items
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        items.keys().cloned().collect()
    };
    if loaded != menu_ids {
        crate::tray::rebuild_tray_menu(app, &state.lang());
    }

    // ② 勾选态同步（重建后句柄是最新的；克隆出锁再回调 tauri）
    let items: Vec<(String, tauri::menu::CheckMenuItem<tauri::Wry>)> = {
        let items = state
            .skin_vis_items
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        items.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    };
    for (id, item) in items {
        let visible = state
            .registry
            .get(&id)
            .map(|w| w.is_visible().unwrap_or(true))
            .unwrap_or(false);
        let _ = item.set_checked(visible);
    }

    let item = state
        .toggle_item
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if let Some(item) = item {
        // 托盘勾选项 = 专注模式（升级后不再是「全部隐藏」簿记）
        let _ = item.set_checked(crate::focus::is_active(app));
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
                *app.state::<AppState>()
                    .hotkey_error
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(combo);
            } else {
                // 簿记存 Display 规范化串（见 REGISTERED_COMBO 注释）
                set_registered_combo(Some(shortcut.to_string()));
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
    // 先解析新组合（Display 规范化串用于全部簿记比对——见 REGISTERED_COMBO
    // 注释）；短路前提 = 组合没变【且】它真的注册成功了——启动时组合被
    // 占用会注册失败，配置值与注册状态脱节，此时重输同一组合必须真走注册。
    let new = if combo.is_empty() {
        None
    } else {
        Some(parse_validated(combo, &lang)?)
    };
    let new_key = new.as_ref().map(|s| s.to_string());
    if combo == old_combo && registered_combo().as_deref() == new_key.as_deref() {
        return Ok(());
    }
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
                // 回滚簿记同样存 Display 规范化串
                old_combo
                    .trim()
                    .parse::<Shortcut>()
                    .ok()
                    .map(|s| s.to_string())
            } else {
                None
            });
            let msg = e.to_string();
            return Err(trf(&lang, Key::HotkeyRegisterFailed, &[&msg]));
        }
    }
    set_registered_combo(if combo.is_empty() { None } else { new_key });
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

// ─── 皮肤专属显隐热键 ───

/// 全局热键事件分发（lib.rs 的 handler 唯一入口）：皮肤专属热键优先
///（专属比全局更具体），未命中再按全局热键处理。
pub fn dispatch_hotkey(app: &AppHandle, shortcut: &Shortcut) {
    let key = shortcut.to_string();
    let skin_id = SKIN_HOTKEYS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
        .cloned();
    if let Some(skin_id) = skin_id {
        log::info!("Skin hotkey triggered: {} → {}", key, skin_id);
        toggle_one_skin(app, &skin_id);
        return;
    }
    if registered_combo().as_deref() == Some(key.as_str()) {
        log::info!("Global hotkey triggered");
        // 全局热键 = 专注模式开关（方案 §3 定案；隐藏/卸载动作随设置档分流）
        crate::focus::toggle(app);
    }
}

/// 切换单个皮肤的显隐（热键触发）：按真实窗口可见性取反；皮肤未加载
///（无窗）静默 no-op。显隐走淡入淡出（factory 淡化助手；hide 侧异步
/// 落地，完成后再过一次漏斗对齐终态）。同步走 sync_tray_toggle_item 漏斗。
/// pub(crate)：托盘「皮肤显隐」勾选项点击走同一路径。
pub(crate) fn toggle_one_skin(app: &AppHandle, skin_id: &str) {
    let state = app.state::<AppState>();
    let Some(win) = state.registry.get(skin_id) else {
        return;
    };
    let visible = win.is_visible().unwrap_or(true);
    if visible {
        crate::window::factory::hide_skin_window_fade(app, skin_id);
    } else {
        crate::window::factory::show_skin_window_fade(app, skin_id);
    }
    sync_tray_toggle_item(app);
}

/// 设置/更新某皮肤的专属热键（"" = 清除）。先注册后落盘（命令层保证
/// 失败不写配置）：冲突（全局热键或其他皮肤已占用）直接拒绝；OS 注册
/// 失败回滚旧组合。注册表常驻——皮肤加载状态不影响本函数行为。
pub fn set_skin_hotkey(app: &AppHandle, skin_id: &str, combo: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    let combo = combo.trim();
    let new_shortcut = if combo.is_empty() {
        None
    } else {
        Some(parse_validated(combo, &lang)?)
    };
    let new_key = new_shortcut.as_ref().map(|s| s.to_string());

    // 冲突检查：全局热键或其他皮肤已占用（自己复用原组合 = 合法重设）
    if let Some(k) = &new_key {
        if registered_combo().as_deref() == Some(k.as_str()) {
            return Err(trf(&lang, Key::HotkeyConflict, &[k]));
        }
        let taken_by_other = {
            let map = SKIN_HOTKEYS.lock().unwrap_or_else(|e| e.into_inner());
            map.get(k)
                .filter(|other| other.as_str() != skin_id)
                .is_some()
        };
        if taken_by_other {
            return Err(trf(&lang, Key::HotkeyConflict, &[k]));
        }
    }

    let mut map = SKIN_HOTKEYS.lock().unwrap_or_else(|e| e.into_inner());
    // 摘除该皮肤的旧组合（注销失败仅告警留痕——旧注册若真泄漏，后续
    // register 会以 OS 占用报错自然暴露）
    let old_key = map
        .iter()
        .find(|(_, v)| v.as_str() == skin_id)
        .map(|(k, _)| k.clone());
    let mut old_alive = false;
    if let Some(old) = &old_key {
        if let Ok(sc) = old.parse::<Shortcut>() {
            if let Err(e) = app.global_shortcut().unregister(sc) {
                log::warn!("failed to unregister skin hotkey '{}': {}", old, e);
                old_alive = true;
            }
        }
        map.remove(old);
    }

    if let (Some(sc), Some(k)) = (new_shortcut, &new_key) {
        if let Err(e) = app.global_shortcut().register(sc) {
            // 回滚旧组合（注销已失败 = 旧注册仍存活，不必重注）
            let restored = if old_alive {
                true
            } else {
                old_key
                    .as_ref()
                    .and_then(|old| old.parse::<Shortcut>().ok())
                    .map(|old_sc| app.global_shortcut().register(old_sc).is_ok())
                    .unwrap_or(false)
            };
            if restored {
                if let Some(old) = old_key {
                    map.insert(old, skin_id.to_string());
                }
            }
            let msg = e.to_string();
            return Err(trf(&lang, Key::HotkeyRegisterFailed, &[&msg]));
        }
        map.insert(k.clone(), skin_id.to_string());
    }
    Ok(())
}

/// 启动与备份导入后：按 config 全量重建皮肤热键注册表（先清后注；
/// 单个失败仅记日志不阻断——与 register_from_config 的容忍同款）。
/// 在 backup.rs 的 rebuild_runtime 与 lib.rs 启动 setup 中调用。
pub fn sync_skin_hotkeys_from_config(app: &AppHandle) {
    {
        // 先清：注销全部现注册（配置无法告诉你谁还在册——注册表才是）
        let mut map = SKIN_HOTKEYS.lock().unwrap_or_else(|e| e.into_inner());
        for combo in map.keys() {
            if let Ok(sc) = combo.parse::<Shortcut>() {
                let _ = app.global_shortcut().unregister(sc);
            }
        }
        map.clear();
    }
    let combos: Vec<(String, String)> = {
        let state = app.state::<AppState>();
        let cfg = state.config.lock().unwrap_or_else(|e| e.into_inner());
        cfg.skin_settings
            .iter()
            .filter(|(_, c)| !c.hotkey.trim().is_empty())
            .map(|(id, c)| (id.clone(), c.hotkey.trim().to_string()))
            .collect()
    };
    for (skin_id, combo) in combos {
        if let Err(e) = set_skin_hotkey(app, &skin_id, &combo) {
            log::warn!(
                "skin hotkey '{}' for '{}' not registered: {}",
                combo,
                skin_id,
                e
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 审查高危（发布阻断级）的口径钉：REGISTERED_COMBO 簿记一律存
    /// Shortcut Display 规范化串——parse↔Display 往返必须统一（大小写/
    /// 修饰词顺序变体归一），且 Display 串绝不等于常见配置原串形态
    ///（正是「配置原串 vs Display 串」的比较把全局热键静默打死）
    #[test]
    fn shortcut_display_normalizes_user_spellings() {
        let a: Shortcut = "Ctrl+Alt+D".parse().unwrap();
        let b: Shortcut = "ctrl+alt+d".parse().unwrap();
        let c: Shortcut = "Alt+Ctrl+D".parse().unwrap();
        assert_eq!(a.to_string(), b.to_string(), "大小写变体必须归一");
        assert_eq!(a.to_string(), c.to_string(), "修饰词顺序变体必须归一");
        // 簿记与 dispatch 事件 shortcut.to_string() 同口径才可比
        assert_eq!(a.to_string(), a.to_string());
        // 反证：Display 串不等于配置原串（误用原串比较即死——勿回归）
        assert_ne!(a.to_string(), "Ctrl+Alt+D");
    }

    /// parse_validated 拒绝裸键（全局劫持打字防护）与接受带修饰组合
    #[test]
    fn parse_validated_requires_modifier() {
        assert!(parse_validated("D", "zh-CN").is_err());
        assert!(parse_validated("Ctrl+Alt+D", "zh-CN").is_ok());
    }
}
