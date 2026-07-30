use std::fs;
use std::path::Path;
use crate::i18n::{tr, trf, Key};
use crate::skin::types::{Skin, SkinManifest, SkinInfo, SkinSettingKind};

/// Scan a directory for skin subdirectories containing valid skin.json files
pub fn scan_skins_directory(skins_dir: &Path) -> Vec<Skin> {
    let mut skins = Vec::new();

    if !skins_dir.exists() {
        let _ = fs::create_dir_all(skins_dir);
        return skins;
    }

    let entries = match fs::read_dir(skins_dir) {
        Ok(e) => e,
        Err(e) => {
            log::error!("Cannot read skins directory {:?}: {}", skins_dir, e);
            return skins;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        // 跳过点开头的目录：.staging-<id> / .<id>.old 是安装过程的暂存目录
        //（见 package.rs::install_package），不是皮肤
        let folder_name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        if folder_name.starts_with('.') {
            continue;
        }

        let skin_json_path = path.join("skin.json");
        if !skin_json_path.exists() {
            continue; // Not a skin folder
        }

        match load_skin_manifest(&path) {
            Ok(manifest) => {
                // 皮肤 ID 以 skin.json 的 id 为准；缺省（旧皮肤）按文件夹名派生
                let id = resolve_skin_id(&manifest, folder_name);
                if skins.iter().any(|s: &Skin| s.id == id) {
                    log::warn!("Duplicate skin id '{}' in {:?} — keeping the first", id, path);
                    continue;
                }
                skins.push(Skin {
                    id,
                    manifest,
                    directory: path,
                });
            }
            Err(e) => {
                log::warn!("Skipping skin '{}': {}", folder_name, e);
            }
        }
    }

    // Sort by name for consistent ordering
    skins.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
    skins
}

/// skin.json 体积上限：手写/打包的清单都是小文件，超限即视为异常
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024; // 1 MB

/// Parse skin.json from a skin directory
pub fn load_skin_manifest(skin_dir: &Path) -> Result<SkinManifest, String> {
    let skin_json_path = skin_dir.join("skin.json");

    // 读取前先看体积，避免超大文件拖垮扫描
    let size = fs::metadata(&skin_json_path)
        .map_err(|e| format!("Cannot read skin.json: {}", e))?
        .len();
    if size > MAX_MANIFEST_BYTES {
        return Err(format!("skin.json too large ({} bytes, limit {} bytes)", size, MAX_MANIFEST_BYTES));
    }

    let content = fs::read_to_string(&skin_json_path)
        .map_err(|e| format!("Cannot read skin.json: {}", e))?;

    // Strip a UTF-8 BOM if present — skin.json is often hand-edited and
    // Windows editors save UTF-8 with a BOM, which serde_json rejects.
    let manifest: SkinManifest = serde_json::from_str(content.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("Invalid skin.json: {}", e))?;

    // entry 必须是皮肤文件夹内的单一文件名：拒绝目录穿越（".."）、子目录
    // 分隔符与 ADS/盘符冒号
    if !is_valid_entry_name(&manifest.entry) {
        return Err(format!("Invalid entry file name '{}'", manifest.entry));
    }

    // Validate entry file exists
    let entry_path = skin_dir.join(&manifest.entry);
    if !entry_path.exists() {
        return Err(format!("Entry file '{}' not found", manifest.entry));
    }

    Ok(manifest)
}

/// entry 字段校验：纯文件名，不含路径分隔符、".." 与冒号
fn is_valid_entry_name(entry: &str) -> bool {
    !entry.is_empty()
        && !entry.contains("..")
        && !entry.contains('/')
        && !entry.contains('\\')
        && !entry.contains(':')
}

/// Validate a skin directory has the minimum required files
pub fn validate_skin_directory(skin_dir: &Path, lang: &str) -> Result<(), String> {
    if !skin_dir.exists() {
        return Err(tr(lang, Key::SkinDirNotExist).to_string());
    }

    let skin_json = skin_dir.join("skin.json");
    if !skin_json.exists() {
        return Err(tr(lang, Key::MissingSkinJson).to_string());
    }

    let manifest = load_skin_manifest(skin_dir)?;
    let entry = skin_dir.join(&manifest.entry);
    if !entry.exists() {
        return Err(format!("Entry file '{}' not found", manifest.entry));
    }

    Ok(())
}

/// Install a skin by copying a folder into the skins directory.
/// The destination folder is named after the resolved skin id.
pub fn install_skin(source: &Path, skins_dir: &Path, lang: &str) -> Result<Skin, String> {
    validate_skin_directory(source, lang)?;

    let folder_name = source.file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| tr(lang, Key::InvalidFolderName).to_string())?;
    let manifest = load_skin_manifest(source)?;
    let id = resolve_skin_id(&manifest, folder_name);

    let dest = skins_dir.join(&id);
    if dest.exists() {
        return Err(trf(lang, Key::SkinAlreadyInstalled, &[id.as_str()]));
    }

    copy_dir_recursive(source, &dest)
        .map_err(|e| trf(lang, Key::CopySkinFailed, &[&e.to_string()]))?;

    Ok(Skin {
        id,
        manifest,
        directory: dest,
    })
}

/// Resolve the canonical skin id: the manifest's `id` when present (and
/// valid), otherwise a slug derived from the folder name (legacy skins
/// without an id field).
pub fn resolve_skin_id(manifest: &SkinManifest, folder_name: &str) -> String {
    match manifest.id.as_deref() {
        // The error text is discarded here — language is irrelevant.
        Some(id) if validate_skin_id(id, crate::i18n::DEFAULT_LANG).is_ok() => id.to_string(),
        Some(id) => {
            log::warn!("Invalid skin id '{}' — falling back to folder slug", id);
            slugify_skin_id(folder_name)
        }
        None => slugify_skin_id(folder_name),
    }
}

/// Skin ids are kebab-case: lowercase letters, digits, dashes; must start
/// with a letter or digit.  They become folder names and config keys.
pub fn validate_skin_id(id: &str, lang: &str) -> Result<(), String> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && id.chars().next().map_or(false, |c| c.is_ascii_alphanumeric())
        && !is_reserved_device_name(id);
    if ok {
        Ok(())
    } else {
        Err(trf(lang, Key::InvalidSkinId, &[id]))
    }
}

/// Windows 保留设备名黑名单（大小写不敏感）：这些名字不能作文件夹名，
/// 连"加扩展名"的形式（con.txt）同样被系统保留，故按基名判断
fn is_reserved_device_name(id: &str) -> bool {
    let base = id.split('.').next().unwrap_or(id).to_ascii_lowercase();
    matches!(
        base.as_str(),
        "con" | "prn" | "aux" | "nul"
            | "com1" | "com2" | "com3" | "com4" | "com5" | "com6" | "com7" | "com8" | "com9"
            | "lpt1" | "lpt2" | "lpt3" | "lpt4" | "lpt5" | "lpt6" | "lpt7" | "lpt8" | "lpt9"
    )
}

/// Derive a valid skin id from an arbitrary folder/zip name.  Non-ASCII
/// characters (e.g. Chinese names) are dropped; if nothing usable remains,
/// fall back to a short hash so the id is still stable and unique-ish.
pub fn slugify_skin_id(name: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for c in name.chars().flat_map(|c| c.to_lowercase()) {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-').chars().take(64).collect::<String>();
    if !slug.is_empty() {
        return slug;
    }
    // All-non-ASCII name → stable hash-based id
    let mut hash: u32 = 2166136261;
    for b in name.as_bytes() {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(16777619);
    }
    format!("skin-{:08x}", hash)
}

/// Build the effective custom setting values for a skin: for each declared
/// setting, take the persisted override when it is type-compatible, else the
/// declared default, else a per-type fallback.  Used both for the config
/// panel (get_skin_detail) and for baking values into the skin:// bridge.
/// `overrides` 是皮肤文件夹 settings.json 里的用户覆盖值（key → value）。
pub fn effective_settings(
    manifest: &SkinManifest,
    overrides: Option<&serde_json::Map<String, serde_json::Value>>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut out = serde_json::Map::new();
    for def in &manifest.settings {
        let persisted = overrides
            .and_then(|o| o.get(&def.key))
            .filter(|v| setting_value_matches(def.kind, v));
        let value = persisted
            .cloned()
            .or_else(|| def.default.clone().filter(|v| setting_value_matches(def.kind, v)))
            .unwrap_or_else(|| type_fallback(def));
        out.insert(def.key.clone(), value);
    }
    out
}

/// Fallback when neither a persisted override nor a declared default exists.
fn type_fallback(def: &crate::skin::types::SkinSettingDef) -> serde_json::Value {
    use serde_json::Value;
    match def.kind {
        SkinSettingKind::Boolean => Value::Bool(false),
        SkinSettingKind::Number | SkinSettingKind::Slider => Value::from(0),
        SkinSettingKind::Palette => Value::from("#ffffff"),
        SkinSettingKind::Text
        | SkinSettingKind::LongText
        | SkinSettingKind::Time
        | SkinSettingKind::Date
        | SkinSettingKind::DateTime
        | SkinSettingKind::Password
        | SkinSettingKind::Font => Value::from(""),
        SkinSettingKind::Select | SkinSettingKind::Radio => def
            .options
            .first()
            .map(|o| Value::from(o.value.clone()))
            .unwrap_or(Value::Null),
        SkinSettingKind::MultiSelect
        | SkinSettingKind::TaskList
        | SkinSettingKind::TodoList
        | SkinSettingKind::Weekdays
        | SkinSettingKind::DateTaskList => Value::Array(Vec::new()),
        SkinSettingKind::TimeRange => serde_json::json!({"start": "", "end": ""}),
    }
}

/// Persisted values are only honored when their JSON type matches the
/// declared setting kind — guards against stale values after the skin author
/// changes a setting's type.
fn setting_value_matches(kind: SkinSettingKind, v: &serde_json::Value) -> bool {
    match kind {
        SkinSettingKind::Boolean => v.is_boolean(),
        SkinSettingKind::Number | SkinSettingKind::Slider => v.is_number(),
        SkinSettingKind::Text
        | SkinSettingKind::LongText
        | SkinSettingKind::Time
        | SkinSettingKind::Date
        | SkinSettingKind::DateTime
        | SkinSettingKind::Password
        | SkinSettingKind::Palette
        | SkinSettingKind::Select
        | SkinSettingKind::Radio
        | SkinSettingKind::Font => v.is_string(),
        SkinSettingKind::MultiSelect
        | SkinSettingKind::TaskList
        | SkinSettingKind::TodoList
        | SkinSettingKind::Weekdays
        | SkinSettingKind::DateTaskList => v.is_array(),
        SkinSettingKind::TimeRange => v.is_object(),
    }
}

/// Build SkinInfo list for frontend display
pub fn build_skin_info_list(skins: &[Skin], loaded_ids: &[String]) -> Vec<SkinInfo> {
    skins.iter().map(|skin| {
        let loaded = loaded_ids.contains(&skin.id);

        // Auto-detect preview image in skin folder
        let preview = find_preview_image(&skin.directory);

        SkinInfo {
            id: skin.id.clone(),
            name: skin.manifest.name.clone(),
            name_en: skin.manifest.name_en.clone(),
            author: skin.manifest.author.clone(),
            version: skin.manifest.version.clone(),
            description: skin.manifest.description.clone(),
            description_en: skin.manifest.description_en.clone(),
            bilingual: skin.manifest.bilingual,
            loaded,
            has_error: false,
            error_msg: None,
            preview,
        }
    }).collect()
}

/// Look for preview.png or preview.jpg in the skin directory.
/// Returns the absolute path as a string if found.
pub fn find_preview_image(skin_dir: &std::path::Path) -> Option<String> {
    for name in &["preview.png", "preview.jpg", "preview.jpeg"] {
        let path = skin_dir.join(name);
        if path.exists() {
            return Some(path.to_string_lossy().to_string());
        }
    }
    None
}

/// 目录递归复制限深：防恶意构造的超深嵌套耗尽路径/栈
const MAX_COPY_DEPTH: u32 = 32;

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    copy_dir_recursive_inner(src, dst, 0)
}

fn copy_dir_recursive_inner(src: &Path, dst: &Path, depth: u32) -> std::io::Result<()> {
    if depth > MAX_COPY_DEPTH {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("directory nesting exceeds {} levels", MAX_COPY_DEPTH),
        ));
    }
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        // symlink_metadata 不跟随链接：符号链接一律跳过（防穿越出皮肤
        // 目录、防链接环导致的无限递归）
        let meta = fs::symlink_metadata(&src_path)?;
        if meta.file_type().is_symlink() {
            log::warn!("Skipping symlink during skin copy: {:?}", src_path);
            continue;
        }
        if meta.is_dir() {
            copy_dir_recursive_inner(&src_path, &dst_path, depth + 1)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_skin_manifest_tolerates_utf8_bom() {
        let dir = std::env::temp_dir().join(format!(
            "driftlet-bom-skin-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("index.html"), "<html></html>").unwrap();
        fs::write(dir.join("skin.json"), "\u{feff}{\"name\": \"BOM Skin\"}").unwrap();

        let manifest = load_skin_manifest(&dir).expect("BOM'd skin.json must parse");
        assert_eq!(manifest.name, "BOM Skin");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn effective_settings_merges_defaults_and_overrides() {
        let manifest: SkinManifest = serde_json::from_str(r##"{
            "name": "T",
            "settings": [
                { "key": "accent", "type": "palette", "default": "#ff3333" },
                { "key": "count", "type": "number", "default": 3 },
                { "key": "flag", "type": "boolean" }
            ]
        }"##).unwrap();

        // No persisted config → declared defaults (missing default → type fallback)
        let values = effective_settings(&manifest, None);
        assert_eq!(values["accent"], "#ff3333");
        assert_eq!(values["count"], 3);
        assert_eq!(values["flag"], false);

        // Type-compatible override wins; type-mismatched override is ignored
        let mut overrides = serde_json::Map::new();
        overrides.insert("accent".into(), serde_json::json!("#00ff00"));
        overrides.insert("count".into(), serde_json::json!("not a number"));
        let values = effective_settings(&manifest, Some(&overrides));
        assert_eq!(values["accent"], "#00ff00");
        assert_eq!(values["count"], 3);
    }

    #[test]
    fn shipped_example_skins_parse() {
        let skins_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples");
        let skins = scan_skins_directory(&skins_dir);
        assert!(skins.len() >= 1, "example skins failed to parse in {}", skins_dir.display());

        // controls-demo declares every supported setting kind; each must get
        // an effective value (declared default or type fallback)
        let demo = skins.iter()
            .find(|s| s.id == "controls-demo")
            .expect("controls-demo must parse");
        assert!(demo.manifest.settings.len() >= 11);
        let values = effective_settings(&demo.manifest, None);
        for def in &demo.manifest.settings {
            assert!(values.contains_key(&def.key), "missing value for {}", def.key);
        }
    }

    fn unique_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "driftlet-loader-{}-{}-{}",
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

    fn make_skin(dir: &std::path::Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("index.html"), "<html></html>").unwrap();
        fs::write(dir.join("skin.json"), r#"{"name":"T"}"#).unwrap();
    }

    #[test]
    fn scan_skips_dot_prefixed_directories() {
        let skins = unique_dir("dotscan");
        make_skin(&skins.join("normal-skin"));
        // 安装暂存目录（.staging-<id> / .<id>.old）即使内容齐全也不得被当皮肤
        make_skin(&skins.join(".staging-normal-skin"));
        make_skin(&skins.join(".normal-skin.old"));

        let found = scan_skins_directory(&skins);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "normal-skin");

        let _ = fs::remove_dir_all(&skins);
    }

    #[test]
    fn manifest_rejects_unsafe_entry() {
        for entry in ["../outside.html", "sub/index.html", "sub\\index.html", "index.html:$DATA", ""] {
            let dir = unique_dir("badentry");
            fs::write(dir.join("index.html"), "<html></html>").unwrap();
            let json = format!(r#"{{"name":"T","entry":"{}"}}"#, entry.replace('\\', "\\\\"));
            fs::write(dir.join("skin.json"), json).unwrap();

            let err = load_skin_manifest(&dir).unwrap_err();
            assert!(err.contains("Invalid entry"), "entry {:?}: unexpected error: {}", entry, err);

            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn manifest_over_size_limit_rejected() {
        let dir = unique_dir("toobig");
        fs::write(dir.join("index.html"), "<html></html>").unwrap();
        // 1 MB 上限 + 1 字节的合法 JSON（注释位用空格填充）
        let mut json = String::from(r#"{"name":"T","padding":""#);
        json.push_str(&" ".repeat((MAX_MANIFEST_BYTES as usize) - json.len() + 1));
        json.push_str("\"\"}");
        fs::write(dir.join("skin.json"), json).unwrap();

        let err = load_skin_manifest(&dir).unwrap_err();
        assert!(err.contains("too large"), "unexpected error: {}", err);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn skin_id_rejects_reserved_device_names() {
        for id in ["con", "prn", "aux", "nul", "com1", "com9", "lpt1", "lpt9", "con.txt", "CON"] {
            assert!(validate_skin_id(id, "zh-CN").is_err(), "reserved id {:?} must be rejected", id);
        }
        for id in ["console", "com10", "con-host", "my-skin"] {
            assert!(validate_skin_id(id, "zh-CN").is_ok(), "id {:?} must be accepted", id);
        }
    }

    #[test]
    fn copy_dir_recursive_limits_depth() {
        let src = unique_dir("deep-src");
        let mut deep = src.clone();
        for _ in 0..(MAX_COPY_DEPTH + 2) {
            deep = deep.join("d");
        }
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("leaf.txt"), "x").unwrap();
        let dst = unique_dir("deep-dst");

        let err = copy_dir_recursive(&src, &dst).unwrap_err();
        assert!(err.to_string().contains("nesting"), "unexpected error: {}", err);

        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);
    }
}
