//! 皮肤设置页用户值的读写（`<皮肤文件夹>/settings.json`）。
//!
//! 该文件只存用户在「皮肤设置」页改过的覆盖值（key → value）；
//! schema 与默认值仍在 skin.json（应用永不重写）。「重置」= 删除该文件。
//! 读写行为镜像 config.rs：缺失→空、容忍 BOM、损坏→改名 .bak、原子写。

use std::fs;
use std::io::Write;
use std::path::Path;

/// 「皮肤设置」页用户值的文件名（与 skin.json 同级）
pub const SETTINGS_FILENAME: &str = "settings.json";

/// 读取皮肤的设置覆盖值。文件缺失返回空 map；容忍 UTF-8 BOM；
/// 损坏时改名 settings.json.bak 备份并返回空（与 config.rs 同策略）。
pub fn load_skin_settings(skin_dir: &Path) -> serde_json::Map<String, serde_json::Value> {
    let path = skin_dir.join(SETTINGS_FILENAME);

    if !path.exists() {
        return serde_json::Map::new();
    }

    match fs::read_to_string(&path) {
        Ok(content) => {
            // 容忍 UTF-8 BOM（Windows 编辑器常会写入）；否则 serde_json
            // 会拒绝解析，文件会被误判为损坏而重置。
            let content = content.trim_start_matches('\u{feff}');
            match serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(content) {
                Ok(values) => values,
                Err(e) => {
                    // 损坏 —— 备份后按空值处理
                    let backup = skin_dir.join(format!("{}.bak", SETTINGS_FILENAME));
                    let _ = fs::rename(&path, &backup);
                    log::warn!("Skin settings corrupt, backed up: {}", e);
                    serde_json::Map::new()
                }
            }
        }
        Err(e) => {
            log::error!("Cannot read skin settings: {}", e);
            serde_json::Map::new()
        }
    }
}

/// 原子写入皮肤的设置覆盖值（写临时文件 + sync + rename，同 config.rs）
pub fn save_skin_settings(
    skin_dir: &Path,
    values: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    let final_path = skin_dir.join(SETTINGS_FILENAME);
    let temp_path = skin_dir.join(format!("{}.tmp", SETTINGS_FILENAME));

    let json = serde_json::to_string_pretty(values)
        .map_err(|e| format!("Serialization error: {}", e))?;

    let mut tmp = fs::File::create(&temp_path)
        .map_err(|e| format!("Cannot create temp file: {}", e))?;
    tmp.write_all(json.as_bytes())
        .map_err(|e| format!("Cannot write skin settings: {}", e))?;
    tmp.sync_all()
        .map_err(|e| format!("Cannot sync skin settings: {}", e))?;

    fs::rename(&temp_path, &final_path)
        .map_err(|e| format!("Cannot finalize skin settings: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "driftlet-skinsettings-{}-{}-{}",
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

    #[test]
    fn missing_file_returns_empty() {
        let dir = unique_dir("missing");
        assert!(load_skin_settings(&dir).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_backed_up_and_returns_empty() {
        let dir = unique_dir("corrupt");
        fs::write(dir.join(SETTINGS_FILENAME), b"not json {").unwrap();

        assert!(load_skin_settings(&dir).is_empty());
        assert!(
            dir.join("settings.json.bak").exists(),
            "corrupt settings must be renamed to .bak"
        );
        assert!(!dir.join(SETTINGS_FILENAME).exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tolerates_utf8_bom() {
        let dir = unique_dir("bom");
        fs::write(
            dir.join(SETTINGS_FILENAME),
            "\u{feff}{\"accent\": \"#ff3333\"}",
        )
        .unwrap();

        let values = load_skin_settings(&dir);
        assert_eq!(values["accent"], "#ff3333");
        assert!(
            !dir.join("settings.json.bak").exists(),
            "BOM'd settings must not be judged corrupt"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_then_load_roundtrip() {
        let dir = unique_dir("roundtrip");
        let mut values = serde_json::Map::new();
        values.insert("accent".into(), serde_json::json!("#00ff00"));
        values.insert("count".into(), serde_json::json!(3));

        save_skin_settings(&dir, &values).unwrap();
        assert_eq!(load_skin_settings(&dir), values);
        // 原子写不留临时文件
        assert!(!dir.join("settings.json.tmp").exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
