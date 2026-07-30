use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use crate::skin::types::AppConfig;

const CONFIG_FILENAME: &str = "config.json";

/// Load app config from disk. Returns default on missing/corrupt file.
pub fn load_config(config_dir: &Path) -> AppConfig {
    let path = config_dir.join(CONFIG_FILENAME);

    if !path.exists() {
        return AppConfig::default();
    }

    match fs::read_to_string(&path) {
        Ok(content) => {
            // Strip a UTF-8 BOM if present (Windows editors often save one);
            // otherwise serde_json rejects the file and the config would be
            // wrongly judged corrupt and reset.
            let content = content.trim_start_matches('\u{feff}');
            match serde_json::from_str::<AppConfig>(content) {
                Ok(mut config) => {
                    normalize_mode_flags(&mut config);
                    config
                }
                Err(e) => {
                    // Corrupt — back up and start fresh
                    let backup = config_dir.join("config.json.bak");
                    let _ = fs::rename(&path, &backup);
                    log::warn!("Config corrupt, backed up: {}", e);
                    AppConfig::default()
                }
            }
        }
        Err(e) => {
            log::error!("Cannot read config: {}", e);
            AppConfig::default()
        }
    }
}

/// "always_on_top" and "on_desktop" are mutually exclusive and exactly one
/// must be on.  Older configs could persist a both-off (or both-on) state;
/// normalize any invalid combination to the default placement (on-desktop).
pub(crate) fn normalize_mode_flags(config: &mut AppConfig) {
    for skin_cfg in config.skin_settings.values_mut() {
        if skin_cfg.always_on_top == skin_cfg.on_desktop {
            skin_cfg.always_on_top = false;
            skin_cfg.on_desktop = true;
        }
    }
}

/// Save config to disk atomically (write temp, then rename)
pub fn save_config(config_dir: &Path, config: &AppConfig) -> Result<(), String> {
    fs::create_dir_all(config_dir)
        .map_err(|e| format!("Cannot create config dir: {}", e))?;

    let final_path = config_dir.join(CONFIG_FILENAME);
    let temp_path = config_dir.join(format!("{}.tmp", CONFIG_FILENAME));

    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Serialization error: {}", e))?;

    let mut tmp = fs::File::create(&temp_path)
        .map_err(|e| format!("Cannot create temp file: {}", e))?;
    tmp.write_all(json.as_bytes())
        .map_err(|e| format!("Cannot write config: {}", e))?;
    tmp.sync_all()
        .map_err(|e| format!("Cannot sync config: {}", e))?;

    fs::rename(&temp_path, &final_path)
        .map_err(|e| format!("Cannot finalize config: {}", e))?;

    Ok(())
}

/// v1 → v2 迁移：「皮肤设置」页用户值从 config.json 的 skin_settings[id].custom
/// 迁到各皮肤文件夹的 settings.json（见 skin/settings.rs）。
///
/// 在扫描皮肤之后、load_config 之前调用。规则：
/// - version >= 2 或文件缺失/损坏 → 直接返回（损坏由 load_config 兜底处理）。
/// - 逐个非空 custom：若目标 settings.json 已存在则以现有文件为准（不覆盖），
///   否则原子写入；成功后从原始 JSON 抹掉该 custom。
/// - 全部成功 → version 升为 2 并原子写回；某皮肤写入失败 → 保留其 custom 且
///   不升版本号，下次启动重试（防数据丢失）。已成功的条目照常写回。
/// - 孤儿条目（皮肤已不存在）：custom 直接随版本升级丢弃——与启动时 prune
///   删除孤儿条目的语义一致。
pub fn migrate_v1_custom_settings(config_dir: &Path, skin_dirs: &[(String, PathBuf)]) {
    let path = config_dir.join(CONFIG_FILENAME);
    let Ok(raw) = fs::read_to_string(&path) else {
        return; // 缺失或不可读：无需迁移（损坏交给 load_config 备份重置）
    };
    let Ok(mut json) = serde_json::from_str::<serde_json::Value>(raw.trim_start_matches('\u{feff}')) else {
        return; // 损坏：由 load_config 备份重置
    };
    // 合法 JSON 但非对象（如 `[]`）：结构不符，同样交给 load_config 兜底。
    // 不能往下走——末尾的 json["version"]=... 对非对象会 panic。
    if !json.is_object() {
        return;
    }
    let version = json.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
    if version >= 2 {
        return;
    }

    let mut all_ok = true;
    if let Some(skin_settings) = json.get_mut("skin_settings").and_then(|v| v.as_object_mut()) {
        for (id, entry) in skin_settings.iter_mut() {
            let custom = entry.get_mut("custom").and_then(|c| c.as_object_mut());
            let Some(custom) = custom else { continue };
            if custom.is_empty() {
                entry.as_object_mut().unwrap().remove("custom");
                continue;
            }
            match skin_dirs.iter().find(|(sid, _)| sid == id) {
                Some((_, dir)) => {
                    let target = dir.join(crate::skin::settings::SETTINGS_FILENAME);
                    // 已存在的 settings.json 以现有文件为准，不覆盖
                    let migrated = target.exists() || {
                        let values: serde_json::Map<String, serde_json::Value> = custom.clone();
                        match crate::skin::settings::save_skin_settings(dir, &values) {
                            Ok(()) => true,
                            Err(e) => {
                                log::warn!("Failed to migrate custom settings for '{}': {}", id, e);
                                false
                            }
                        }
                    };
                    if migrated {
                        entry.as_object_mut().unwrap().remove("custom");
                    } else {
                        all_ok = false; // 保留 custom，下次启动重试
                    }
                }
                None => {
                    // 孤儿条目：皮肤已不存在，custom 随版本升级丢弃
                    entry.as_object_mut().unwrap().remove("custom");
                }
            }
        }
    }

    if all_ok {
        json["version"] = serde_json::json!(2);
    }
    if let Err(e) = write_raw_config(config_dir, &json) {
        log::warn!("Failed to write back migrated config: {}", e);
    }
}

/// 把迁移后的原始 JSON 原子写回 config.json（tmp + rename + sync，同 save_config）
fn write_raw_config(config_dir: &Path, json: &serde_json::Value) -> Result<(), String> {
    let final_path = config_dir.join(CONFIG_FILENAME);
    let temp_path = config_dir.join(format!("{}.tmp", CONFIG_FILENAME));

    let text = serde_json::to_string_pretty(json)
        .map_err(|e| format!("Serialization error: {}", e))?;

    let mut tmp = fs::File::create(&temp_path)
        .map_err(|e| format!("Cannot create temp file: {}", e))?;
    tmp.write_all(text.as_bytes())
        .map_err(|e| format!("Cannot write config: {}", e))?;
    tmp.sync_all()
        .map_err(|e| format!("Cannot sync config: {}", e))?;

    fs::rename(&temp_path, &final_path)
        .map_err(|e| format!("Cannot finalize config: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_config_tolerates_utf8_bom() {
        let dir = std::env::temp_dir().join(format!(
            "driftlet-bom-config-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();

        let json = serde_json::to_string(&AppConfig {
            theme: "dark".to_string(),
            ..AppConfig::default()
        })
        .unwrap();
        // Windows editors often save UTF-8 with a BOM.
        fs::write(dir.join(CONFIG_FILENAME), format!("\u{feff}{}", json)).unwrap();

        let config = load_config(&dir);
        assert_eq!(config.theme, "dark", "BOM'd config must parse, not reset");
        assert!(
            !dir.join("config.json.bak").exists(),
            "BOM'd config must not be judged corrupt"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    fn unique_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "driftlet-migrate-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn read_raw(dir: &Path) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(dir.join(CONFIG_FILENAME)).unwrap()).unwrap()
    }

    #[test]
    fn migrate_moves_custom_to_settings_json() {
        let dir = unique_dir("move");
        let skin_dir = unique_dir("move-skin");
        fs::write(
            dir.join(CONFIG_FILENAME),
            r##"{"version":1,"skin_settings":{"my-skin":{"opacity":0.8,"custom":{"accent":"#00ff00"}}}}"##,
        )
        .unwrap();

        migrate_v1_custom_settings(&dir, &[("my-skin".to_string(), skin_dir.clone())]);

        // 值写入皮肤文件夹，config.json 抹掉 custom 并升版本
        let values = crate::skin::settings::load_skin_settings(&skin_dir);
        assert_eq!(values["accent"], "#00ff00");
        let raw = read_raw(&dir);
        assert_eq!(raw["version"], 2);
        assert!(raw["skin_settings"]["my-skin"].get("custom").is_none());
        // 窗口页数据原样保留
        assert_eq!(raw["skin_settings"]["my-skin"]["opacity"], 0.8);

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&skin_dir);
    }

    #[test]
    fn migrate_keeps_existing_settings_file() {
        let dir = unique_dir("keep");
        let skin_dir = unique_dir("keep-skin");
        // settings.json 已存在 → 以现有文件为准，不覆盖
        fs::write(skin_dir.join("settings.json"), r##"{"accent":"#ffffff"}"##).unwrap();
        fs::write(
            dir.join(CONFIG_FILENAME),
            r##"{"version":1,"skin_settings":{"my-skin":{"custom":{"accent":"#00ff00"}}}}"##,
        )
        .unwrap();

        migrate_v1_custom_settings(&dir, &[("my-skin".to_string(), skin_dir.clone())]);

        let values = crate::skin::settings::load_skin_settings(&skin_dir);
        assert_eq!(values["accent"], "#ffffff", "existing settings.json must win");
        let raw = read_raw(&dir);
        assert_eq!(raw["version"], 2);
        assert!(raw["skin_settings"]["my-skin"].get("custom").is_none());

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&skin_dir);
    }

    #[test]
    fn migrate_drops_orphan_custom_and_bumps_version() {
        let dir = unique_dir("orphan");
        fs::write(
            dir.join(CONFIG_FILENAME),
            r#"{"version":1,"skin_settings":{"gone-skin":{"custom":{"a":1}}}}"#,
        )
        .unwrap();

        migrate_v1_custom_settings(&dir, &[]);

        let raw = read_raw(&dir);
        assert_eq!(raw["version"], 2);
        assert!(raw["skin_settings"]["gone-skin"].get("custom").is_none());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_retries_failed_skin_without_bumping_version() {
        let dir = unique_dir("retry");
        // 皮肤目录路径上是一个普通文件 → 写入必然失败
        let blocker = unique_dir("retry-block");
        let bad_dir = blocker.join("not-a-dir");
        fs::write(&bad_dir, b"file, not a directory").unwrap();
        fs::write(
            dir.join(CONFIG_FILENAME),
            r##"{"version":1,"skin_settings":{"my-skin":{"custom":{"accent":"#00ff00"}}}}"##,
        )
        .unwrap();

        migrate_v1_custom_settings(&dir, &[("my-skin".to_string(), bad_dir)]);

        // 写入失败：custom 保留、版本不升，下次启动重试
        let raw = read_raw(&dir);
        assert_eq!(raw["version"], 1);
        assert_eq!(raw["skin_settings"]["my-skin"]["custom"]["accent"], "#00ff00");

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&blocker);
    }

    #[test]
    fn migrate_skips_v2_config() {
        let dir = unique_dir("v2");
        let original = r##"{"version":2,"skin_settings":{"my-skin":{"custom":{"accent":"#00ff00"}}}}"##;
        fs::write(dir.join(CONFIG_FILENAME), original).unwrap();

        migrate_v1_custom_settings(&dir, &[]);

        // v2 直接返回，文件一字节不动（custom 键如出现也原样保留，
        // serde 读入 AppConfig 时会忽略它）
        assert_eq!(fs::read_to_string(dir.join(CONFIG_FILENAME)).unwrap(), original);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_ignores_non_object_config() {
        let dir = unique_dir("nonobj");
        // 合法 JSON 但非对象：不得 panic（此前 json["version"]=... 会崩），
        // 文件原样保留，交给 load_config 备份重置
        fs::write(dir.join(CONFIG_FILENAME), "[]").unwrap();

        migrate_v1_custom_settings(&dir, &[]);

        assert_eq!(fs::read_to_string(dir.join(CONFIG_FILENAME)).unwrap(), "[]");

        let _ = fs::remove_dir_all(&dir);
    }
}
