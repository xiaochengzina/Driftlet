use std::fs;
use std::path::Path;
use crate::i18n::{trf, Key};
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

    // read_dir 的返回顺序随文件系统而变（无字典序保证）——先按文件夹名
    // 排序再扫描，下面重复 id 去重的「保留第一个」才有确定性
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
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
                    origin: read_origin_marker(&path),
                    directory: path,
                });
            }
            Err(e) => {
                log::warn!("Skipping skin '{}': {}", folder_name, e);
            }
        }
    }

    // Sort by display name for consistent ordering（按当前默认语言的解析名排，
    // 单语言皮肤回退后的名字也参与排序，不会沉底/乱序）
    skins.sort_by(|a, b| {
        a.manifest.display_name(crate::i18n::DEFAULT_LANG).cmp(&b.manifest.display_name(crate::i18n::DEFAULT_LANG))
    });
    skins
}

/// skin.json 体积上限：手写/打包的清单都是小文件，超限即视为异常
pub(crate) const MAX_MANIFEST_BYTES: u64 = 1024 * 1024; // 1 MB

/// 提取 skin.json 的 `x-driftlet-origin`（副本来源标记，见 types.rs Skin.origin）。
/// 单独以 Value 解析——manifest 结构体不含此字段（pack-skin 镜像零扰动）；
/// 读失败/字段缺失/非字符串一律 None（宽容：手写的皮肤没有它）。
fn read_origin_marker(skin_dir: &Path) -> Option<String> {
    let text = fs::read_to_string(skin_dir.join("skin.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    value.get("x-driftlet-origin")?.as_str().map(str::to_string)
}

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
    let mut manifest: SkinManifest = serde_json::from_str(content.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("Invalid skin.json: {}", e))?;

    // 两真归一化：on_desktop 是 serde default_true（types.rs），skin.json
    // 只写 "always_on_top": true 会读出两真。manifest 是作者源文件，显式
    // 写 always_on_top 即作者意图 → aot 赢，on_desktop 置回 false。
    // 注意这与 config.rs normalize_mode_flags 的 desktop-wins 语境不同：
    // 那里处理的是持久化配置，serde 读回后无法区分「用户显式写 true」与
    // 「default_true 补的 true」，只能保守地 desktop 赢。
    if manifest.window.always_on_top {
        manifest.window.on_desktop = false;
    }

    // window 默认值归一化（作者手写的 skin.json 不做任何信任假设）：
    // 宽高钳到 [1, MAX_DIMENSION]（超大值会建出巨型表面吃 GPU 内存）；
    // opacity 非有限/越界回落 [0.1, 1.0]（opacity:0 + 置顶 + 巨尺寸 =
    // 隐形置顶吃点击窗口——安装向导不展示 window 默认值，必须在这里拦）。
    // 注意：此钳制是安装端加载时的归一化（非拒绝）；pack-skin 有提示式
    // 镜像（声明值会被钳时打印提示，不改包内容、不拦截——给创作者
    // 「所见即所得」，无「打包放行、安装拒载」分歧）。
    // 改动必须同步镜像到 tools/pack-skin 并重建 exe。
    manifest.window.width = manifest.window.width.clamp(1, 10000);
    manifest.window.height = manifest.window.height.clamp(1, 10000);
    let op = manifest.window.opacity;
    manifest.window.opacity = if op.is_finite() { op.clamp(0.1, 1.0) } else { 1.0 };
    manifest.window.zoom = crate::commands::clamp_zoom(manifest.window.zoom);
    // 网页皮肤自动刷新间隔钳到 ≤24h（作者手滑写大值的兜底）
    manifest.window.refresh_seconds = manifest.window.refresh_seconds.map(|s| s.min(86400));

    // entry 必须是皮肤文件夹内的单一文件名：拒绝目录穿越（".."）、子目录
    // 分隔符与 ADS/盘符冒号。例外：http(s) URL = 网页皮肤（窗口直接加载站点
    // 页面），无本地文件可查
    if !crate::skin::types::is_url_entry(&manifest.entry) {
        if !is_valid_entry_name(&manifest.entry) {
            return Err(format!("Invalid entry file name '{}'", manifest.entry));
        }

        // Validate entry file exists
        let entry_path = skin_dir.join(&manifest.entry);
        if !entry_path.exists() {
            return Err(format!("Entry file '{}' not found", manifest.entry));
        }
    }

    Ok(manifest)
}

// 注意：以下校验函数（is_valid_entry_name / validate_skin_id /
// is_reserved_device_name）在 tools/pack-skin/src/main.rs 有手工镜像——
// 改动必须同步并重建 pack-skin.exe。

/// entry 字段校验：纯文件名，不含路径分隔符、".." 与冒号
pub(crate) fn is_valid_entry_name(entry: &str) -> bool {
    !entry.is_empty()
        && !entry.contains("..")
        && !entry.contains('/')
        && !entry.contains('\\')
        && !entry.contains(':')
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
    // 保留设备名兜底（审查 L5）：文件夹叫 "con" 会滑出保留名 id，下游
    // validate_skin_id 会拒——这里直接避开，落哈希形态
    if !slug.is_empty() && !is_reserved_device_name(&slug) {
        return slug;
    }
    // All-non-ASCII name（或滑出保留设备名）→ stable hash-based id
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
        SkinSettingKind::Number | SkinSettingKind::Slider | SkinSettingKind::Stepper => Value::from(0),
        SkinSettingKind::Palette => Value::from("#ffffff"),
        SkinSettingKind::Text
        | SkinSettingKind::LongText
        | SkinSettingKind::Time
        | SkinSettingKind::Date
        | SkinSettingKind::DateTime
        | SkinSettingKind::Password
        | SkinSettingKind::Font
        | SkinSettingKind::File
        | SkinSettingKind::Directory => Value::from(""),
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
        SkinSettingKind::Number | SkinSettingKind::Slider | SkinSettingKind::Stepper => v.is_number(),
        SkinSettingKind::Text
        | SkinSettingKind::LongText
        | SkinSettingKind::Time
        | SkinSettingKind::Date
        | SkinSettingKind::DateTime
        | SkinSettingKind::Password
        | SkinSettingKind::Palette
        | SkinSettingKind::Select
        | SkinSettingKind::Radio
        | SkinSettingKind::Font
        | SkinSettingKind::File
        | SkinSettingKind::Directory => v.is_string(),
        SkinSettingKind::MultiSelect
        | SkinSettingKind::TaskList
        | SkinSettingKind::TodoList
        | SkinSettingKind::Weekdays
        | SkinSettingKind::DateTaskList => v.is_array(),
        SkinSettingKind::TimeRange => v.is_object(),
    }
}

/// Build SkinInfo list for frontend display
pub fn build_skin_info_list(skins: &[Skin], loaded_ids: &[String], hidden_ids: &[String]) -> Vec<SkinInfo> {
    skins.iter().map(|skin| {
        let loaded = loaded_ids.contains(&skin.id);
        let hidden = hidden_ids.contains(&skin.id);

        // Auto-detect preview image in skin folder
        let preview = find_preview_image(&skin.directory);

        SkinInfo {
            id: skin.id.clone(),
            name_zh: skin.manifest.name_zh.clone().unwrap_or_default(),
            name_en: skin.manifest.name_en.clone(),
            author: skin.manifest.author.clone(),
            version: skin.manifest.version.clone(),
            description_zh: skin.manifest.description_zh.clone(),
            description_en: skin.manifest.description_en.clone(),
            loaded,
            hidden,
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
        assert_eq!(manifest.name_zh.as_deref(), Some("BOM Skin"));

        let _ = fs::remove_dir_all(&dir);
    }

    /// 两个测试用单语言皮肤（临时夹具，测完即删；等价实机清单「单语言皮肤
    /// 文案回退」条目）：纯英文皮肤 name_zh 字段整体省略、只填 name_en/description_en；
    /// 纯中文皮肤只填 name_zh/description_zh。铁律三条：
    /// ① 都能经 load_skin_manifest 正常装载（name_zh/name_en 均非必填）；
    /// ② display_name/display_description 在两种界面语言下都显示创作者
    ///    提供的那种语言（单语言回退规则，勿回归）；
    /// ③ 旧字段名 name/description/label/group 经 serde alias 继续被接受
    ///    （存量皮肤零迁移——解析后落入新字段）。
    #[test]
    fn single_language_skins_load_and_display_without_bilingual() {
        let base = std::env::temp_dir().join(format!(
            "driftlet-singlelang-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let en_dir = base.join("test-en-only");
        let zh_dir = base.join("test-zh-only");
        fs::create_dir_all(&en_dir).unwrap();
        fs::create_dir_all(&zh_dir).unwrap();
        fs::write(en_dir.join("index.html"), "<html></html>").unwrap();
        fs::write(zh_dir.join("index.html"), "<html></html>").unwrap();
        // 英文单语：没有 name_zh 字段（字段整体缺席才考 serde default）
        fs::write(
            en_dir.join("skin.json"),
            r#"{"id":"test-en-only","name_en":"EN Only Skin","description_en":"English-only test skin","version":"1.0.0","entry":"index.html"}"#,
        )
        .unwrap();
        fs::write(
            zh_dir.join("skin.json"),
            r#"{"id":"test-zh-only","name_zh":"中文单语测试","description_zh":"只填中文的单语测试皮肤","version":"1.0.0","entry":"index.html"}"#,
        )
        .unwrap();

        let en = load_skin_manifest(&en_dir).expect("name-less English-only skin.json must parse");
        assert_eq!(en.name_zh, None, "name_zh 缺省必须归一 None");
        let zh = load_skin_manifest(&zh_dir).expect("Chinese-only skin.json must parse");

        // 显示回退矩阵：单语言皮肤在中/英界面都显示创作者提供的那种语言
        assert_eq!(en.display_name("en"), "EN Only Skin");
        assert_eq!(en.display_name("zh-CN"), "EN Only Skin");
        assert_eq!(en.display_description("en").as_deref(), Some("English-only test skin"));
        assert_eq!(en.display_description("zh-CN").as_deref(), Some("English-only test skin"));
        assert_eq!(zh.display_name("en"), "中文单语测试");
        assert_eq!(zh.display_name("zh-CN"), "中文单语测试");
        assert_eq!(zh.display_description("en").as_deref(), Some("只填中文的单语测试皮肤"));

        // 旧字段名 alias：存量皮肤（name/description/label/group 无后缀写法）
        // 必须零迁移解析进新字段
        let legacy: SkinManifest = serde_json::from_str(
            r#"{"name":"旧字段名皮肤","description":"旧简介","settings":[{"key":"k","type":"select","label":"旧标签","group":"旧组","description":"旧说明","options":[{"value":"a","label":"旧选项"}]}]}"#,
        )
        .unwrap();
        assert_eq!(legacy.name_zh.as_deref(), Some("旧字段名皮肤"));
        assert_eq!(legacy.description_zh.as_deref(), Some("旧简介"));
        let def = &legacy.settings[0];
        assert_eq!(def.label_zh.as_deref(), Some("旧标签"));
        assert_eq!(def.group_zh.as_deref(), Some("旧组"));
        assert_eq!(def.description_zh.as_deref(), Some("旧说明"));
        assert_eq!(def.options[0].label_zh.as_deref(), Some("旧选项"));

        // 双语对照：两语言都提供时随界面切换
        let both: SkinManifest = serde_json::from_str(
            r#"{"name_zh":"双语皮肤","name_en":"Bilingual Skin","description_zh":"中文简介","description_en":"EN desc"}"#,
        )
        .unwrap();
        assert_eq!(both.display_name("en"), "Bilingual Skin");
        assert_eq!(both.display_name("zh-CN"), "双语皮肤");
        assert_eq!(both.display_description("en").as_deref(), Some("EN desc"));
        assert_eq!(both.display_description("zh-CN").as_deref(), Some("中文简介"));

        // 半翻译：字段留空回退另一种语言
        let half: SkinManifest = serde_json::from_str(
            r#"{"name_zh":"双语皮肤","name_en":""}"#,
        )
        .unwrap();
        assert_eq!(half.display_name("en"), "双语皮肤", "空串 name_en 必须回退 name_zh");
        let half_zh: SkinManifest = serde_json::from_str(
            r#"{"name_zh":"","name_en":"EN Only"}"#,
        )
        .unwrap();
        assert_eq!(half_zh.display_name("zh-CN"), "EN Only", "空串 name_zh 必须回退 name_en");

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn slugify_avoids_reserved_device_names() {
        // 保留设备名不落 slug（审查 L5）：改落哈希形态，且产物须过
        // validate_skin_id
        for name in ["con", "CON", "nul", "com1", "LPT9"] {
            let id = slugify_skin_id(name);
            assert!(!is_reserved_device_name(&id), "{name} slugged to reserved {id}");
            assert!(validate_skin_id(&id, "zh-CN").is_ok(), "{id} must be a valid id");
        }
        // 正常名字照旧走 slug
        assert_eq!(slugify_skin_id("My Clock"), "my-clock");
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
}
