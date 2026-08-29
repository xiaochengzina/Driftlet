//! Layout backup — export `config/` + `skins/` into one zip, import it back.
//!
//! Zip layout:
//!   driftlet-backup.json   manifest { format, app, app_version, created_at }
//!   config/...             mirror of the config dir (config.json, ...)
//!   skins/...              mirror of the skins dir (every skin folder,
//!                          including each folder's settings.json user values)
//!
//! Export is a plain walk + ZipWriter.  Import is the risky half, so it
//! borrows the package installer's defenses (skin/package.rs): extract to a
//! temp dir under size/entry/zip-slip guards, validate the payload, then swap
//! the live dirs via a staged `.import-old` rename that rolls back on any
//! failure — a failed import never leaves the data dirs half-replaced.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};
use zip::write::SimpleFileOptions;

use crate::i18n::{tr, trf, Key};
use crate::skin::{config, loader, package};
use crate::AppState;

const MANIFEST_NAME: &str = "driftlet-backup.json";
const BACKUP_FORMAT: u64 = 1;
// Same defensive limits as the skin package extractor.
const MAX_ZIP_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const MAX_FILES: usize = 5000;

// ─── Export ─────────────────────────────────────────────────────────────

/// 导出 config/ + skins/ 为一个 zip。调用方必须已持有
/// `AppState.install_lock`（guard 由异步命令获取并持有到本函数返回）——
/// 导入 Phase 2/3 的 rename+copy 窗口期 skins/ 缺失或半拷贝，此刻并发
/// 导出会产出不完整备份。
pub fn export_backup(config_dir: &Path, skins_dir: &Path, dest: &Path, lang: &str) -> Result<(), String> {
    // 导出目标不得位于两个源目录内：add_dir 会把正在写入的 zip 自身包进去
    //（自包含、体积失控、产物损坏）。canonicalize dest 的父目录做包含性判定
    //（dest 尚不存在，比父目录）。
    {
        let parent = dest.parent().unwrap_or(Path::new("."));
        let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let parent_c = canon(parent);
        for src in [config_dir, skins_dir] {
            let src_c = canon(src);
            if parent_c == src_c || parent_c.starts_with(&src_c) {
                return Err(trf(lang, Key::ExportBackupFailed,
                    &["destination must not be inside config/ or skins/"]));
            }
        }
    }
    // 先写临时文件再 rename 就位：导出失败不留半截 zip
    let tmp = dest.with_extension("zip.tmp");
    let result = (|| -> Result<(), String> {
        let file = fs::File::create(&tmp)
            .map_err(|e| trf(lang, Key::ExportBackupFailed, &[&e.to_string()]))?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        let manifest = serde_json::json!({
            "format": BACKUP_FORMAT,
            "app": "Driftlet",
            "app_version": env!("CARGO_PKG_VERSION"),
            "created_at": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        });
        let fail = |e: zip::result::ZipError| trf(lang, Key::ExportBackupFailed, &[&e.to_string()]);
        zip.start_file(MANIFEST_NAME, opts).map_err(fail)?;
        zip.write_all(manifest.to_string().as_bytes())
            .map_err(|e| trf(lang, Key::ExportBackupFailed, &[&e.to_string()]))?;

        add_dir(&mut zip, config_dir, "config", opts, lang)?;
        // 首次安装从未改过设置时磁盘上没有 config.json（启动只建目录、首次
        // 改设置才落盘）——直接导出会产出不含 config/config.json 的包，导入
        // 侧 validate_backup 必拒（「不是有效备份」，用户首装即导出即导入
        // 实测命中）。缺则把默认配置补进包——该场景下内存配置与默认等价
        //（所有设置变更即改即存，文件缺失 = 从未变更过）
        if !config_dir.join("config.json").is_file() {
            let default_json = serde_json::to_string(&crate::skin::types::AppConfig::default())
                .map_err(|e| trf(lang, Key::ExportBackupFailed, &[&e.to_string()]))?;
            zip.start_file("config/config.json", opts).map_err(fail)?;
            zip.write_all(default_json.as_bytes())
                .map_err(|e| trf(lang, Key::ExportBackupFailed, &[&e.to_string()]))?;
        }
        add_dir(&mut zip, skins_dir, "skins", opts, lang)?;
        zip.finish().map_err(fail)?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            fs::rename(&tmp, dest)
                .map_err(|e| trf(lang, Key::ExportBackupFailed, &[&e.to_string()]))?;
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Recursively add `base`'s files under the `<prefix>/` zip path.  Staged-
/// replace leftovers (package installs, earlier failed imports) are skipped.
/// junction/symlink 目录不跟随（reparse point 成环会无限递归撑爆磁盘，
/// 指向外部的 junction 还会把 skins 之外的内容带进备份）；条目数/总字节
/// 与导入侧同一套防御上限。
fn add_dir(
    zip: &mut zip::ZipWriter<fs::File>,
    base: &Path,
    prefix: &str,
    opts: SimpleFileOptions,
    lang: &str,
) -> Result<(), String> {
    if !base.is_dir() {
        return Ok(());
    }
    let fail = |e: io::Error| trf(lang, Key::ExportBackupFailed, &[&e.to_string()]);
    let mut stack = vec![base.to_path_buf()];
    let mut files = 0usize;
    let mut total: u64 = 0;
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).map_err(fail)? {
            let entry = entry.map_err(fail)?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(".staging-") || name.ends_with(".old") || name.ends_with(".import-old") {
                continue;
            }
            // reparse point（junction/symlink）不跟随：跳过整个目录/文件
            let meta = fs::symlink_metadata(&path).map_err(fail)?;
            if meta.file_type().is_symlink() {
                continue;
            }
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::fs::MetadataExt;
                // FILE_ATTRIBUTE_REPARSE_POINT：junction 在 Windows 上不
                // 被 is_symlink 标记，须按属性位判
                if meta.file_attributes() & 0x400 != 0 {
                    continue;
                }
            }
            let rel = path.strip_prefix(base).map_err(|e| e.to_string())?;
            let rel_slash = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            files += 1;
            total = total.saturating_add(meta.len());
            if files > MAX_FILES {
                return Err(trf(lang, Key::ExportBackupFailed,
                    &[&format!("too many files (> {})", MAX_FILES)]));
            }
            if total > MAX_TOTAL_BYTES {
                return Err(trf(lang, Key::ExportBackupFailed,
                    &[&format!("backup too large (> {} bytes)", MAX_TOTAL_BYTES)]));
            }
            zip.start_file(format!("{}/{}", prefix, rel_slash), opts)
                .map_err(|e| trf(lang, Key::ExportBackupFailed, &[&e.to_string()]))?;
            let mut f = fs::File::open(&path).map_err(fail)?;
            io::copy(&mut f, zip).map_err(fail)?;
        }
    }
    Ok(())
}

// ─── Import ─────────────────────────────────────────────────────────────

/// 导入前审查：备份包内的皮肤清单及其权限声明（复审 A-M2：备份导入曾绕过
/// .dskin 安装引导页的权限展示，高权限皮肤可静默落地）。解包+校验与导入
/// 同一条防线（extract_backup / validate_backup），只读不写。
#[derive(serde::Serialize)]
pub struct BackupSkinInfo {
    pub id: String,
    pub name: String,
    pub name_en: Option<String>,
    /// 双语皮肤标志（前端 dispName 按管理器语言选取 name/name_en 的依据）
    pub bilingual: bool,
    pub version: Option<String>,
    pub permissions: Vec<String>,
}

#[derive(serde::Serialize)]
pub struct BackupInspection {
    /// 所选备份文件路径（确认后回传 import_config 执行）
    pub path: String,
    pub skins: Vec<BackupSkinInfo>,
    /// 备份生成时的宿主版本（备份清单内记录；旧备份可能缺）
    pub app_version: Option<String>,
}

pub fn inspect_backup(package_path: &Path, lang: &str) -> Result<BackupInspection, String> {
    let extracted = extract_backup(package_path, lang)?;
    validate_backup(extracted.path(), lang)?;
    // 备份清单的应用版本（导出时写入，见 export_backup；缺失容忍）
    let app_version = fs::read_to_string(extracted.path().join(MANIFEST_NAME))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|v| v.get("app_version")?.as_str().map(String::from));
    let skins_root = extracted.path().join("skins");
    let mut skins = Vec::new();
    if skins_root.is_dir() {
        for entry in fs::read_dir(&skins_root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            // 暂存残留（.staging-* / .old / .import-old）不参与展示
            let folder = entry.file_name().to_string_lossy().to_string();
            if folder.starts_with('.') {
                continue;
            }
            // 审查清单要如数列出包内皮肤——包括入口文件损坏/缺失、导入后
            // 加载不上的（用户有权知道包里有它）：不走 loader 的完整装载校验
            //（load_skin_manifest 会查 entry 存在性），只读 manifest 本体
            let text = match fs::read_to_string(dir.join("skin.json")) {
                Ok(t) if t.len() <= loader::MAX_MANIFEST_BYTES as usize => t,
                _ => continue,
            };
            let Ok(manifest) =
                serde_json::from_str::<crate::skin::types::SkinManifest>(text.trim_start_matches('\u{feff}'))
            else {
                continue;
            };
            skins.push(BackupSkinInfo {
                id: loader::resolve_skin_id(&manifest, &folder),
                name: manifest.name,
                name_en: manifest.name_en,
                bilingual: manifest.bilingual,
                version: manifest.version,
                permissions: manifest.permissions,
            });
        }
    }
    skins.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(BackupInspection {
        path: package_path.to_string_lossy().into_owned(),
        skins,
        app_version,
    })
}

pub async fn import_backup(app: AppHandle, package_path: &Path) -> Result<(), String> {
    let lang = app.state::<AppState>().lang();
    // Phase 1: extract to a temp dir under the package extractor's guards,
    // then validate — the live data dirs are untouched until everything
    // about the payload checks out.（重 IO 挪 spawn_blocking，不占 async worker）
    let pkg = package_path.to_path_buf();
    let lang1 = lang.clone();
    let extracted = tauri::async_runtime::spawn_blocking(move || {
        let extracted = extract_backup(&pkg, &lang1)?;
        validate_backup(extracted.path(), &lang1)?;
        Ok::<_, String>(extracted)
    })
    .await
    .map_err(|e| trf(&lang, Key::TaskFailed, &[&e.to_string()]))??;

    // Phase 2: serialize with package installs, then unload every skin —
    // a loaded skin's folder is locked by WebView2 on Windows and the
    // rename below would fail.
    let state = app.state::<AppState>();
    let _install_guard = state.install_lock.lock().await;
    // 逐个卸载；中途失败则导入整体中止 —— 数据目录尚未被触碰，但已卸载
    // 的皮肤不会自己回来：把已卸载的 id 写进错误信息，用户重新加载即可恢复
    let mut unloaded: Vec<String> = Vec::new();
    for id in state.registry.loaded_ids() {
        if let Err(e) = crate::commands::unload_skin_impl(app.clone(), id.clone()).await {
            if unloaded.is_empty() {
                return Err(e);
            }
            let ids = unloaded.join(", ");
            return Err(format!("{} {}", e, trf(&lang, Key::ImportPartialUnloaded, &[&ids])));
        }
        unloaded.push(id);
    }

    // Phase 3: staged replace with rollback（持 settings_lock 与设置写入
    // 互斥；重 IO 挪 spawn_blocking）。失败时数据已回滚，但皮肤已全部
    // 卸载——与 Phase 2 同款提示，告知用户重新加载即可恢复。
    let app2 = app.clone();
    let extracted_path = extracted.path().to_path_buf();
    let lang2 = lang.clone();
    let phase3 = tauri::async_runtime::spawn_blocking(move || {
        let state2 = app2.state::<AppState>();
        let _settings_guard = state2.settings_lock.lock().unwrap_or_else(|e| e.into_inner());
        replace_data_dirs(&state2.config_dir, &state2.skins_dir, &extracted_path, &lang2)
    })
    .await
    .map_err(|e| trf(&lang, Key::TaskFailed, &[&e.to_string()]))?;
    if let Err(e) = phase3 {
        if unloaded.is_empty() {
            return Err(e);
        }
        let ids = unloaded.join(", ");
        return Err(format!("{} {}", e, trf(&lang, Key::ImportPartialUnloaded, &[&ids])));
    }

    // Phase 4: rebuild runtime state from the imported files.  Individual
    // steps only log — the data itself is already safely in place.
    rebuild_runtime(&app).await;
    Ok(())
}

/// 选择性导入（合并模式，import_config 的 skin_ids 分支）：只导入指定
/// 皮肤，其余皮肤与全局布局（语言/主题/自启/热键/分组）一律不动。
/// 与全量替换式 import_backup 的语义边界：
/// - 目录：不整体换 skins/，按皮肤 id 逐个替换（同 id 不同文件夹名时
///   替换本地既有文件夹，否则用备份文件夹名）；每个替换自带 .import-old
///   让位备份，中途失败把已换的逐个还原——config 在目录全部就位前不动。
/// - 配置：只并入选中皮肤的 skin_settings 条目与 loaded_skins 成员 +
///   布局方案（按 id 并集、备份版胜——维护者实机定案：备份 = 用户全量
///   数据，方案不随选择性导入丢失；引用未导入皮肤的条目保留，应用时
///   skipped 提示）；分组归属不导入（本地组保留，选中皮肤落「未分组」）。
/// 返回（成功导入的 id 列表，选中但备份中不存在的 id 列表）。
pub async fn import_backup_selective(
    app: AppHandle,
    package_path: &Path,
    skin_ids: Vec<String>,
) -> Result<(Vec<String>, Vec<String>), String> {
    let lang = app.state::<AppState>().lang();

    // Phase 1: extract + validate（与全量同一防线）
    let pkg = package_path.to_path_buf();
    let lang1 = lang.clone();
    let extracted = tauri::async_runtime::spawn_blocking(move || {
        let extracted = extract_backup(&pkg, &lang1)?;
        validate_backup(extracted.path(), &lang1)?;
        Ok::<_, String>(extracted)
    })
    .await
    .map_err(|e| trf(&lang, Key::TaskFailed, &[&e.to_string()]))??;

    // 选中集合 ∩ 备份内皮肤（备份文件夹名 → 皮肤 id 解析；选中但不在
    // 备份的记为 skipped 返回给前端提示）
    let backup_skins_root = extracted.path().join("skins");
    let mut to_import: Vec<(String, std::path::PathBuf)> = Vec::new(); // (id, 备份内文件夹)
    let mut skipped: Vec<String> = Vec::new();
    'outer: for id in &skin_ids {
        if backup_skins_root.is_dir() {
            for entry in fs::read_dir(&backup_skins_root).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let dir = entry.path();
                if !dir.is_dir() {
                    continue;
                }
                let folder = entry.file_name().to_string_lossy().to_string();
                if folder.starts_with('.') {
                    continue;
                }
                let text = match fs::read_to_string(dir.join("skin.json")) {
                    Ok(t) if t.len() <= loader::MAX_MANIFEST_BYTES as usize => t,
                    _ => continue,
                };
                let Ok(manifest) = serde_json::from_str::<crate::skin::types::SkinManifest>(
                    text.trim_start_matches('\u{feff}'),
                ) else {
                    continue;
                };
                if &loader::resolve_skin_id(&manifest, &folder) == id {
                    to_import.push((id.clone(), dir));
                    continue 'outer;
                }
            }
        }
        skipped.push(id.clone());
    }
    if to_import.is_empty() {
        return Err(tr(&lang, Key::InvalidBackup).to_string());
    }

    // Phase 2: 只卸载「选中且已加载」的皮肤（文件夹被 WebView2 占用，
    // 替换会失败）；其余已加载皮肤不动
    let state = app.state::<AppState>();
    let _install_guard = state.install_lock.lock().await;
    let selected: std::collections::HashSet<&str> =
        to_import.iter().map(|(id, _)| id.as_str()).collect();
    let mut unloaded: Vec<String> = Vec::new();
    for id in state.registry.loaded_ids() {
        if !selected.contains(id.as_str()) {
            continue;
        }
        if let Err(e) = crate::commands::unload_skin_impl(app.clone(), id.clone()).await {
            if unloaded.is_empty() {
                return Err(e);
            }
            let ids = unloaded.join(", ");
            return Err(format!("{} {}", e, trf(&lang, Key::ImportPartialUnloaded, &[&ids])));
        }
        unloaded.push(id);
    }

    // Phase 3: 按 id 逐个替换（持 settings_lock 与设置写入互斥；重 IO 挪
    // spawn_blocking）。每个替换 = 既有文件夹挪为 <name>.import-old →
    // 备份文件夹拷入；任何一步失败：已换的逐个还原（config 尚未触碰）
    let app2 = app.clone();
    let to_import2 = to_import.clone();
    let lang2 = lang.clone();
    let phase3 = tauri::async_runtime::spawn_blocking(move || {
        let state2 = app2.state::<AppState>();
        let _settings_guard = state2.settings_lock.lock().unwrap_or_else(|e| e.into_inner());
        swap_selected_skins(&state2.skins_dir, &to_import2, &lang2)
    })
    .await
    .map_err(|e| trf(&lang, Key::TaskFailed, &[&e.to_string()]))?;
    if let Err(e) = phase3 {
        if unloaded.is_empty() {
            return Err(e);
        }
        let ids = unloaded.join(", ");
        return Err(format!("{} {}", e, trf(&lang, Key::ImportPartialUnloaded, &[&ids])));
    }

    // Phase 4: config 只并入选中项（skin_settings 覆盖、loaded_skins 并集），
    // 然后只加载「备份里处于加载态」的选中皮肤；全局项与分组不动。
    // 布局方案例外按维护者实机定案并入（备份语义 = 用户全量数据）：按 id
    // 并集、备份版胜——方案引用未导入皮肤的条目保留，应用时 skipped 机制
    // 单列提示，不在这里剃掉。
    let loaded_now: Vec<String> = {
        let backup_cfg_text = fs::read_to_string(extracted.path().join("config").join("config.json"))
            .map_err(|e| trf(&lang, Key::ImportBackupFailed, &[&e.to_string()]))?;
        let mut backup_cfg: crate::skin::types::AppConfig =
            serde_json::from_str(backup_cfg_text.trim_start_matches('\u{feff}'))
                .map_err(|e| trf(&lang, Key::ImportBackupFailed, &[&e.to_string()]))?;
        let backup_loaded: std::collections::HashSet<String> =
            backup_cfg.loaded_skins.iter().cloned().collect();
        let mut cfg = state.config.lock().unwrap_or_else(|e| e.into_inner());
        for (id, _) in &to_import {
            if let Some(rc) = backup_cfg.skin_settings.remove(id) {
                cfg.skin_settings.insert(id.clone(), rc);
            }
            if backup_loaded.contains(id) && !cfg.loaded_skins.contains(id) {
                cfg.loaded_skins.push(id.clone());
            }
        }
        merge_backup_layouts(&mut cfg, backup_cfg.layouts);
        // 选中皮肤的文件夹已在位， prune 只清真正的孤儿（防手改/半同步残留）
        let disk_skins = loader::scan_skins_directory(&state.skins_dir);
        config::prune_stale_entries(&mut cfg, &disk_skins);
        if let Err(e) = config::save_config(&state.config_dir, &cfg) {
            log::warn!("selective import: failed to save merged config: {}", e);
        }
        // 块尾表达式 = 备份里处于加载态的选中皮肤（下面逐个加载）
        cfg.loaded_skins
            .iter()
            .filter(|id| selected.contains(id.as_str()))
            .cloned()
            .collect()
    };
    // 布局方案并入后托盘「布局方案」子菜单同步重建——必须在 config 守卫
    // 落地后调用（rebuild 链路要取同一把锁，持锁 = 同线程死锁，同
    // capture_layout 教训）
    crate::tray::rebuild_tray_menu(&app, &lang);
    // 并入的皮肤热键同步注册（注册表常驻——与全量导入/启动同一路径，
    // 否则合并进来的热键要等重启才生效）
    crate::hotkey::sync_skin_hotkeys_from_config(&app);
    for id in loaded_now {
        if let Err(e) = crate::commands::load_skin_impl(app.clone(), id.clone()).await {
            log::warn!("selective import: failed to load skin '{}': {}", id, e);
        }
    }

    let imported: Vec<String> = to_import.into_iter().map(|(id, _)| id).collect();
    Ok((imported, skipped))
}

/// 选择性导入的布局并入：按 id 并集、备份版胜（与 loaded_skins 并集同款
/// 哲学）。方案引用未导入皮肤的条目保留——应用时 skipped 机制单列提示，
/// 不在导入侧剃掉（备份 = 用户全量数据，维护者实机定案）。
fn merge_backup_layouts(
    cfg: &mut crate::skin::types::AppConfig,
    backup: Vec<crate::skin::types::LayoutPreset>,
) {
    for l in backup {
        if let Some(slot) = cfg.layouts.iter_mut().find(|x| x.id == l.id) {
            *slot = l;
        } else {
            cfg.layouts.push(l);
        }
    }
}

/// 按 (id, 备份内文件夹) 列表逐个替换皮肤文件夹：目标 = 本地同 id 文件夹
/// （保用户既有命名），否则 skins_dir 下同备份名文件夹。让位备份用
/// `.<folder>.old`（点前缀——皮肤扫描器跳过点开头目录，与安装暂存同约定，
/// 不用 import-old 后缀：那会进 skins/ 被扫描成重复 id）。任一步失败把
/// 已换的逐个还原并清理半成品；成功时清理全部让位备份。
fn swap_selected_skins(
    skins_dir: &Path,
    to_import: &[(String, std::path::PathBuf)],
    lang: &str,
) -> Result<(), String> {
    let fail = |e: io::Error| trf(lang, Key::ImportBackupFailed, &[&e.to_string()]);
    let local = loader::scan_skins_directory(skins_dir);

    // 预检（审查高危）：回退落位（本地无此 id → 用备份文件夹名）撞上既有
    // 文件夹时直接报错——本地扫描没找到本 id，占用者必承载别的 id，让位
    // 拷换会在成功路径把它的文件夹连 config 一起抹掉（未选中皮肤静默销毁）。
    // 在任何改名/拷贝前整体拒绝，用户处理冲突后重导
    for (id, backup_dir) in to_import {
        if local.iter().any(|s| &s.id == id) {
            continue; // 按 id 落位，无撞名问题
        }
        let fallback = skins_dir.join(
            backup_dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        if fallback.exists() {
            let name = fallback
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            return Err(trf(lang, Key::ImportFolderConflict, &[&name]));
        }
    }

    let mut swapped: Vec<(std::path::PathBuf, std::path::PathBuf)> = Vec::new(); // (target, aside)

    for (id, backup_dir) in to_import {
        let target = local
            .iter()
            .find(|s| &s.id == id)
            .map(|s| s.directory.clone())
            .unwrap_or_else(|| {
                skins_dir.join(
                    backup_dir
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                )
            });
        let folder_name = target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let aside = target.with_file_name(format!(".{}.old", folder_name));
        if aside.exists() {
            let _ = fs::remove_dir_all(&aside);
        }
        if target.exists() {
            if let Err(e) = fs::rename(&target, &aside) {
                rollback_swapped(&swapped);
                return Err(fail(e));
            }
        }
        if let Err(e) = package::copy_dir_recursive(backup_dir, &target) {
            let _ = fs::remove_dir_all(&target);
            if aside.exists() {
                let _ = fs::rename(&aside, &target);
            }
            rollback_swapped(&swapped);
            return Err(fail(e));
        }
        swapped.push((target, aside));
    }

    for (_, aside) in &swapped {
        let _ = fs::remove_dir_all(aside);
    }
    Ok(())
}

/// swap_selected_skins 的中途回滚：已就位的目标目录删除半成品并还原让位备份
fn rollback_swapped(swapped: &[(std::path::PathBuf, std::path::PathBuf)]) {
    for (target, aside) in swapped.iter().rev() {
        let _ = fs::remove_dir_all(target);
        if aside.exists() {
            let _ = fs::rename(aside, target);
        }
    }
}

/// Extract to a temp dir with size/entry/zip-slip guards (mirrors
/// skin/package.rs::extract_package).  The guard cleans up on drop.
fn extract_backup(package_path: &Path, lang: &str) -> Result<TempDirGuard, String> {
    let file = fs::File::open(package_path)
        .map_err(|e| trf(lang, Key::ReadBackupFailed, &[&e.to_string()]))?;
    if file.metadata().map(|m| m.len()).unwrap_or(0) > MAX_ZIP_BYTES {
        return Err(tr(lang, Key::BackupTooLarge).to_string());
    }

    let mut archive = zip::ZipArchive::new(file)
        .map_err(|_| tr(lang, Key::BackupNotZip).to_string())?;
    if archive.len() > MAX_FILES {
        return Err(tr(lang, Key::BackupTooManyFiles).to_string());
    }

    let temp_dir = std::env::temp_dir().join(format!(
        "driftlet-backup-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&temp_dir).map_err(|e| trf(lang, Key::CreateTempDirFailed, &[&e.to_string()]))?;

    let guard = TempDirGuard(temp_dir.clone());
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| trf(lang, Key::ReadBackupFailed, &[&e.to_string()]))?;
        // enclosed_name 拒绝绝对路径与 ".."，防 zip slip
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        if entry.is_dir() {
            continue;
        }
        let out_path = temp_dir.join(&rel);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = fs::File::create(&out_path).map_err(|e| e.to_string())?;
        // 不信任 zip 头声明的解压大小：按实际写出字节数累计并截断读取
        let remaining = MAX_TOTAL_BYTES - total;
        let mut limited = entry.by_ref().take(remaining + 1);
        let written = io::copy(&mut limited, &mut out).map_err(|e| e.to_string())?;
        total += written;
        if total > MAX_TOTAL_BYTES {
            return Err(tr(lang, Key::BackupExtractedTooLarge).to_string());
        }
    }
    Ok(guard)
}

/// A backup must look like one: `config/config.json` present, and if there
/// is a manifest its format must be one we understand.
fn validate_backup(dir: &Path, lang: &str) -> Result<(), String> {
    if !dir.join("config").join("config.json").is_file() {
        return Err(tr(lang, Key::InvalidBackup).to_string());
    }
    let manifest_path = dir.join(MANIFEST_NAME);
    if manifest_path.is_file() {
        let text = fs::read_to_string(&manifest_path)
            .map_err(|e| trf(lang, Key::ReadBackupFailed, &[&e.to_string()]))?;
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|_| tr(lang, Key::InvalidBackup).to_string())?;
        let format = value.get("format").and_then(|f| f.as_u64()).unwrap_or(0);
        if format != BACKUP_FORMAT {
            return Err(trf(lang, Key::BackupFormatUnsupported, &[&format.to_string()]));
        }
    }
    Ok(())
}

/// Swap both data dirs with the extracted backup.  Both live dirs are first
/// renamed aside (`<name>.import-old`); any failure removes the partials and
/// renames them back, so a failed import leaves the original state intact.
fn replace_data_dirs(config_dir: &Path, skins_dir: &Path, extracted: &Path, lang: &str) -> Result<(), String> {
    let fail = |e: io::Error| trf(lang, Key::ImportBackupFailed, &[&e.to_string()]);
    let cfg_old = import_old_sibling(config_dir);
    let sk_old = import_old_sibling(skins_dir);
    for old in [&cfg_old, &sk_old] {
        if old.exists() {
            let _ = fs::remove_dir_all(old);
        }
    }
    let had_cfg = config_dir.exists();
    let had_skins = skins_dir.exists();

    // ① 两个目录都让位；第二个让位失败时把第一个挪回去
    if had_cfg {
        fs::rename(config_dir, &cfg_old).map_err(fail)?;
    }
    if had_skins {
        if let Err(e) = fs::rename(skins_dir, &sk_old) {
            if had_cfg {
                let _ = fs::rename(&cfg_old, config_dir);
            }
            return Err(fail(e));
        }
    }

    // ② 新内容就位；任一步失败 → 清半成品并整体回滚
    let result = copy_or_create(&extracted.join("config"), config_dir)
        .and_then(|_| copy_or_create(&extracted.join("skins"), skins_dir));
    if let Err(e) = result {
        let _ = fs::remove_dir_all(config_dir);
        let _ = fs::remove_dir_all(skins_dir);
        if had_cfg {
            let _ = fs::rename(&cfg_old, config_dir);
        }
        if had_skins {
            let _ = fs::rename(&sk_old, skins_dir);
        }
        return Err(fail(e));
    }

    // ③ 成功，丢弃旧数据
    let _ = fs::remove_dir_all(&cfg_old);
    let _ = fs::remove_dir_all(&sk_old);
    Ok(())
}

fn import_old_sibling(dir: &Path) -> PathBuf {
    dir.with_file_name(format!(
        "{}.import-old",
        dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    ))
}

/// 导入崩溃窗口的启动回滚。`.import-old` 存在 = 上次导入在就位/收尾阶段
/// 中断（成功路径③会删掉它们）。此时 `<name>` 目录可能是缺失（①后未②）、
/// 半成品（②中途崩）或完好（③删旧前崩）——三种形态一律把旧副本挪回去：
/// 完好副本下用户重导一次即可；半成品副本下这是唯一的完好数据。
/// （旧语义「两者都在 = 已成功，删 .import-old」会把②中途崩的半成品当成功、
/// 把唯一完好数据静默删除——审查 A-H1。）
/// **调用时必须赶在「会创建数据目录」的代码之前**（resolve_portable_dir 的
/// 可写性探测会 create_dir_all）——否则目录恒存在，①后未②的崩溃现场被
/// 抹成「两者都在」，回滚沦为死代码（A-H1 的另一半根因）。
/// 返回回滚条数（日志用）。
pub fn rollback_interrupted_import(config_dir: &Path, skins_dir: &Path) -> usize {
    let mut rolled = 0;
    for dir in [config_dir, skins_dir] {
        let old = import_old_sibling(dir);
        if !old.exists() {
            continue;
        }
        if dir.exists() {
            let _ = fs::remove_dir_all(dir);
        }
        match fs::rename(&old, dir) {
            Ok(()) => {
                rolled += 1;
                log::warn!("import rollback: {:?} restored from interrupted import", dir);
            }
            Err(e) => log::error!("import rollback failed for {:?}: {}", dir, e),
        }
    }
    rolled
}

/// 备份里可能合法地缺 `skins/`（导出时一个皮肤都没装）——缺则建空目录
fn copy_or_create(src: &Path, dst: &Path) -> io::Result<()> {
    if src.is_dir() {
        package::copy_dir_recursive(src, dst)
    } else {
        fs::create_dir_all(dst)
    }
}

/// Rebuild every runtime mirror of the on-disk state after the swap:
/// in-memory config (pruned), language + tray, autostart, global hotkey,
/// then load the skins the imported config had loaded.
async fn rebuild_runtime(app: &AppHandle) {
    let state = app.state::<AppState>();
    let skins = loader::scan_skins_directory(&state.skins_dir);
    let mut cfg = config::load_config(&state.config_dir);
    let removed = config::prune_stale_entries(&mut cfg, &skins);
    if removed > 0 {
        log::info!("import: pruned {} config entries of missing skins", removed);
    }
    if let Err(e) = config::save_config(&state.config_dir, &cfg) {
        log::warn!("import: failed to save pruned config: {}", e);
    }
    let language = cfg.language.clone();
    let autostart = cfg.autostart;
    // 替换内存配置前先取出要同步的运行时镜像值（autostart 同款模式）：
    // hot_reload_enabled 原子量声明为 config.hot_reload 的镜像，双写
    let hot_reload = cfg.hot_reload;
    let to_load = cfg.loaded_skins.clone();
    *state.config.lock().unwrap_or_else(|e| e.into_inner()) = cfg;
    state.hot_reload_enabled.store(hot_reload, std::sync::atomic::Ordering::Relaxed);
    *state.language.lock().unwrap_or_else(|e| e.into_inner()) = language.clone();
    crate::tray::rebuild_tray_menu(app, &language);

    {
        // 与 set_autostart 命令同一入口：并发/残留态下的幂等同步
        //（disable 在 Run 值不存在时报 os error 2，统一按目标已达处理）
        if let Err(e) = crate::commands::sync_autostart(app, autostart) {
            log::warn!("import: failed to sync autostart: {}", e);
        }
    }

    crate::hotkey::reregister_from_config(app);
    // 皮肤专属热键注册表随备份 config 一并重建（与全局热键同节奏）
    crate::hotkey::sync_skin_hotkeys_from_config(app);

    for id in to_load {
        if let Err(e) = crate::commands::load_skin_impl(app.clone(), id.clone()).await {
            log::warn!("import: failed to load skin '{}': {}", id, e);
        }
    }
}

/// RAII 守卫：临时解压目录在离开作用域时清理（同 skin/package.rs）
struct TempDirGuard(PathBuf);
impl TempDirGuard {
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 审查 A-H1：`.import-old` 存在即回滚——覆盖三种崩溃形态（目录缺失 /
    /// 半成品并存 / 完好并存），不再以「两者都在」推断成功而删旧数据。
    #[test]
    fn rollback_restores_on_old_residue_in_all_crash_shapes() {
        let root = TestDir::new("rollback");
        let config_dir = root.0.join("config");
        let skins_dir = root.0.join("skins");

        // 形态 1：①后未②——目录缺失、只剩 .import-old
        let old_cfg = import_old_sibling(&config_dir);
        fs::create_dir_all(&old_cfg).unwrap();
        fs::write(old_cfg.join("config.json"), r#"{"version":2}"#).unwrap();
        assert_eq!(rollback_interrupted_import(&config_dir, &skins_dir), 1);
        assert!(config_dir.join("config.json").is_file(), "旧数据必须回滚就位");
        assert!(!old_cfg.exists(), "回滚后 .import-old 必须消失");

        // 形态 2：②中途崩——半成品新目录与完好旧副本并存（旧语义会删旧留半）
        fs::create_dir_all(&config_dir).unwrap(); // 半成品（无 config.json）
        fs::create_dir_all(&old_cfg).unwrap();
        fs::write(old_cfg.join("config.json"), r#"{"version":2}"#).unwrap();
        assert_eq!(rollback_interrupted_import(&config_dir, &skins_dir), 1);
        assert!(config_dir.join("config.json").is_file(), "半成品必须被旧副本覆盖");
        assert!(!old_cfg.exists());

        // 形态 3：③删旧前崩——新目录完好、旧副本也在（回滚旧副本=丢已完成
        // 的导入，用户重导一次即可；这是安全方向的取舍）
        fs::write(config_dir.join("config.json"), r#"{"version":3}"#).unwrap();
        fs::create_dir_all(&old_cfg).unwrap();
        fs::write(old_cfg.join("config.json"), r#"{"version":2}"#).unwrap();
        assert_eq!(rollback_interrupted_import(&config_dir, &skins_dir), 1);
        let text = fs::read_to_string(config_dir.join("config.json")).unwrap();
        assert!(text.contains("\"version\":2"), "并存时旧副本优先（安全方向）");

        // 无残留 = 零动作
        assert_eq!(rollback_interrupted_import(&config_dir, &skins_dir), 0);
    }

    /// 审查 A-M2：导出→审查 往返——inspect_backup 必须能读回本应用导出的包
    ///（含皮肤清单与权限声明），且通过 validate_backup 的 config 校验
    #[test]
    fn inspect_backup_roundtrips_export() {
        let root = TestDir::new("inspect");
        let (config_dir, skins_dir) = make_data_dirs(&root.0);
        let dest = root.0.join("backup.zip");
        export_backup(&config_dir, &skins_dir, &dest, "zh-CN").unwrap();
        let info = inspect_backup(&dest, "zh-CN").unwrap_or_else(|e| panic!("inspect failed: {}", e));
        assert_eq!(info.path, dest.to_string_lossy());
        assert!(info.skins.iter().any(|s| s.id == "clock"), "包内皮肤必须列出");
        assert_eq!(
            info.app_version.as_deref(),
            Some(env!("CARGO_PKG_VERSION")),
            "宿主版本必须读回"
        );
    }

    /// 首装路径（用户实测报告）：从未改过设置时 config.json 不在盘上——
    /// 导出仍须产出含 config/config.json 的有效备份（导入校验的合法性标志）
    #[test]
    fn export_includes_config_json_when_never_saved() {
        let root = TestDir::new("fresh-export");
        let config_dir = root.0.join("config");
        let skins_dir = root.0.join("skins");
        fs::create_dir_all(&config_dir).unwrap(); // 无 config.json
        fs::create_dir_all(&skins_dir).unwrap(); // 无皮肤
        let dest = root.0.join("backup.zip");
        export_backup(&config_dir, &skins_dir, &dest, "zh-CN").unwrap();
        // inspect 内部即过 validate_backup——通过即有效备份
        let info = inspect_backup(&dest, "zh-CN").unwrap();
        assert!(info.skins.is_empty(), "无皮肤的备份清单为空");
    }

    struct TestDir(PathBuf);
    impl TestDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "driftlet-backup-test-{}-{}-{}",
                name,
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// 造一份最小的 config + skins 数据目录
    fn make_data_dirs(root: &Path) -> (PathBuf, PathBuf) {
        let config_dir = root.join("config");
        let skins_dir = root.join("skins").join("clock");
        fs::create_dir_all(&config_dir).unwrap();
        fs::create_dir_all(&skins_dir).unwrap();
        fs::write(config_dir.join("config.json"), r#"{"version":2}"#).unwrap();
        fs::write(skins_dir.join("skin.json"), r#"{"id":"clock","name":"Clock"}"#).unwrap();
        fs::write(skins_dir.join("index.html"), "<html></html>").unwrap();
        fs::write(skins_dir.join("settings.json"), r#"{"city":"shanghai"}"#).unwrap();
        // 应被跳过的暂存残留
        fs::create_dir_all(root.join("skins").join(".staging-junk")).unwrap();
        fs::write(root.join("skins").join(".staging-junk").join("x"), "x").unwrap();
        (config_dir, root.join("skins"))
    }

    fn zip_of(src: &Path, dest: &Path, skip_manifest: bool) {
        let file = fs::File::create(dest).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = SimpleFileOptions::default();
        if !skip_manifest {
            zip.start_file(MANIFEST_NAME, opts).unwrap();
            zip.write_all(br#"{"format":1}"#).unwrap();
        }
        let mut stack = vec![src.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                let rel = path.strip_prefix(src).unwrap().to_string_lossy().replace('\\', "/");
                if path.is_dir() {
                    stack.push(path);
                } else {
                    zip.start_file(rel, opts).unwrap();
                    zip.write_all(&fs::read(&path).unwrap()).unwrap();
                }
            }
        }
        zip.finish().unwrap();
    }

    #[test]
    fn extract_and_validate_round_trip() {
        let root = TestDir::new("roundtrip");
        let (config_dir, _) = make_data_dirs(&root.0);
        let zip_path = root.0.join("backup.zip");
        // payload 目录结构：config/ + skins/
        let payload = root.0.join("payload");
        fs::create_dir_all(&payload).unwrap();
        package::copy_dir_recursive(&config_dir, &payload.join("config")).unwrap();
        package::copy_dir_recursive(&root.0.join("skins"), &payload.join("skins")).unwrap();
        zip_of(&payload, &zip_path, false);

        let extracted = extract_backup(&zip_path, "zh-CN").unwrap();
        validate_backup(extracted.path(), "zh-CN").unwrap();
        assert!(extracted.path().join("skins/clock/settings.json").is_file());
        // 暂存残留不应进入导出——这里是手动构造的 zip 含有它（因为 zip_of 不过滤），
        // 导出侧过滤由 add_dir 的 skip 规则覆盖（见 export 逻辑）。
    }

    #[test]
    fn add_dir_skips_staging_leftovers() {
        let root = TestDir::new("adddir");
        let (config_dir, _) = make_data_dirs(&root.0);
        // .old 残留目录
        let old = root.0.join("skins").join(".clock.old");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("skin.json"), "{}").unwrap();

        let zip_path = root.0.join("out.zip");
        let file = fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = SimpleFileOptions::default();
        add_dir(&mut zip, &config_dir, "config", opts, "zh-CN").unwrap();
        add_dir(&mut zip, &root.0.join("skins"), "skins", opts, "zh-CN").unwrap();
        zip.finish().unwrap();

        // 重新打开校验条目名
        let archive = zip::ZipArchive::new(fs::File::open(&zip_path).unwrap()).unwrap();
        let names: Vec<String> = (0..archive.len()).map(|i| archive.name_for_index(i).unwrap().to_string()).collect();
        assert!(names.contains(&"config/config.json".to_string()), "{:?}", names);
        assert!(names.contains(&"skins/clock/skin.json".to_string()), "{:?}", names);
        assert!(names.contains(&"skins/clock/settings.json".to_string()), "{:?}", names);
        assert!(!names.iter().any(|n| n.contains(".staging-")), "{:?}", names);
        assert!(!names.iter().any(|n| n.contains(".old")), "{:?}", names);
    }

    #[test]
    fn rejects_missing_config_json() {
        let root = TestDir::new("nocfg");
        let payload = root.0.join("payload");
        fs::create_dir_all(payload.join("skins")).unwrap();
        let zip_path = root.0.join("bad.zip");
        zip_of(&payload, &zip_path, false);
        let extracted = extract_backup(&zip_path, "zh-CN").unwrap();
        assert!(validate_backup(extracted.path(), "zh-CN").is_err());
    }

    #[test]
    fn rejects_unsupported_format() {
        let root = TestDir::new("format");
        let payload = root.0.join("payload");
        fs::create_dir_all(payload.join("config")).unwrap();
        fs::write(payload.join("config").join("config.json"), "{}").unwrap();
        let zip_path = root.0.join("new.zip");
        // format=99 的清单
        let file = fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = SimpleFileOptions::default();
        zip.start_file(MANIFEST_NAME, opts).unwrap();
        zip.write_all(br#"{"format":99}"#).unwrap();
        zip.start_file("config/config.json", opts).unwrap();
        zip.write_all(b"{}").unwrap();
        zip.finish().unwrap();

        let extracted = extract_backup(&zip_path, "zh-CN").unwrap();
        assert!(validate_backup(extracted.path(), "zh-CN").is_err());
    }

    #[test]
    fn replace_data_dirs_rolls_back_on_copy_failure() {
        let root = TestDir::new("rollback");
        let (config_dir, skins_dir) = make_data_dirs(&root.0);
        let extracted = root.0.join("extracted");
        fs::create_dir_all(extracted.join("config")).unwrap();
        fs::write(extracted.join("config").join("config.json"), r#"{"version":3}"#).unwrap();
        // skins 里埋一个超过 32 层的嵌套，触发 copy_dir_recursive 限深失败
        let mut deep = extracted.join("skins");
        for _ in 0..40 {
            deep = deep.join("a");
        }
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("boom.txt"), "x").unwrap();

        assert!(replace_data_dirs(&config_dir, &skins_dir, &extracted, "zh-CN").is_err());
        // 回滚：原数据原样恢复，无暂存残留
        assert_eq!(fs::read_to_string(config_dir.join("config.json")).unwrap(), r#"{"version":2}"#);
        assert!(skins_dir.join("clock").join("skin.json").is_file());
        assert!(!import_old_sibling(&config_dir).exists());
        assert!(!import_old_sibling(&skins_dir).exists());
    }

    #[test]
    fn replace_data_dirs_swaps_content() {
        let root = TestDir::new("swap");
        let (config_dir, skins_dir) = make_data_dirs(&root.0);
        let extracted = root.0.join("extracted");
        fs::create_dir_all(extracted.join("config")).unwrap();
        fs::create_dir_all(extracted.join("skins").join("dock")).unwrap();
        fs::write(extracted.join("config").join("config.json"), r#"{"version":3}"#).unwrap();
        fs::write(extracted.join("skins").join("dock").join("skin.json"), r#"{"id":"dock"}"#).unwrap();

        replace_data_dirs(&config_dir, &skins_dir, &extracted, "zh-CN").unwrap();
        assert_eq!(fs::read_to_string(config_dir.join("config.json")).unwrap(), r#"{"version":3}"#);
        assert!(skins_dir.join("dock").join("skin.json").is_file());
        assert!(!skins_dir.join("clock").exists());
        assert!(!import_old_sibling(&config_dir).exists());
        assert!(!import_old_sibling(&skins_dir).exists());
    }

    /// 实机报告核对：布局方案必须随全量备份导出/导入往返存活——备份语义是
    /// 「用户全量数据」。覆盖 export → extract → validate → replace →
    /// load_config 文件层全链路（rebuild_runtime 的内存换装一行直赋值，
    /// 不另测）。回归锚点：布局面板数据 = config.json 的 layouts 字段，
    /// 任一环节丢字段此测试即红。
    #[test]
    fn export_import_roundtrip_preserves_layouts() {
        let root = TestDir::new("layouts-roundtrip");
        let (config_dir, skins_dir) = make_data_dirs(&root.0);
        // 与运行时落盘同路径：AppConfig 真实序列化一份带布局方案的 config
        let mut cfg = crate::skin::types::AppConfig::default();
        cfg.layouts.push(crate::skin::types::LayoutPreset {
            id: "l-1".to_string(),
            name: "工作桌".to_string(),
            skins: std::collections::HashMap::from([(
                "clock".to_string(),
                crate::skin::types::LayoutSkin {
                    x: Some(10),
                    y: Some(20),
                    width: 300,
                    height: 200,
                    visible: false,
                },
            )]),
        });
        config::save_config(&config_dir, &cfg).unwrap();

        // 备份机导出
        let dest = root.0.join("backup.zip");
        export_backup(&config_dir, &skins_dir, &dest, "zh-CN").unwrap();

        // 新机导入：解包校验 + 目录替换 + 重载 config
        let live = TestDir::new("layouts-live");
        let live_cfg = live.0.join("config");
        let live_skins = live.0.join("skins");
        fs::create_dir_all(&live_cfg).unwrap();
        fs::create_dir_all(&live_skins).unwrap();
        fs::write(live_cfg.join("config.json"), r#"{"version":2}"#).unwrap();

        let extracted = extract_backup(&dest, "zh-CN").unwrap();
        validate_backup(extracted.path(), "zh-CN").unwrap();
        replace_data_dirs(&live_cfg, &live_skins, extracted.path(), "zh-CN").unwrap();

        let restored = config::load_config(&live_cfg);
        assert_eq!(restored.layouts.len(), 1, "布局方案必须随全量导入恢复");
        assert_eq!(restored.layouts[0].name, "工作桌");
        assert_eq!(
            restored.layouts[0].skins.get("clock").map(|s| s.visible),
            Some(false),
            "方案内皮肤快照（含显隐）必须原样存活"
        );
    }

    /// 选择性导入的布局并入语义：按 id 并集、备份版胜；本地独有方案保留；
    /// 引用未导入皮肤的方案原样保留（应用侧 skipped 兜底，不在导入侧剃掉）。
    #[test]
    fn merge_backup_layouts_unions_by_id_and_backup_wins() {
        let preset = |id: &str, name: &str| crate::skin::types::LayoutPreset {
            id: id.to_string(),
            name: name.to_string(),
            skins: std::collections::HashMap::new(),
        };
        let mut cfg = crate::skin::types::AppConfig::default();
        cfg.layouts.push(preset("l-1", "本地方案"));
        cfg.layouts.push(preset("l-2", "将被覆盖"));
        let backup = vec![preset("l-2", "备份版"), preset("l-3", "备份独有")];

        merge_backup_layouts(&mut cfg, backup);

        assert_eq!(cfg.layouts.len(), 3);
        let name = |id: &str| cfg.layouts.iter().find(|l| l.id == id).map(|l| l.name.as_str());
        assert_eq!(name("l-1"), Some("本地方案"), "本地独有方案保留");
        assert_eq!(name("l-2"), Some("备份版"), "同 id 备份版胜");
        assert_eq!(name("l-3"), Some("备份独有"), "备份独有方案并入");
    }

    /// 选择性合并导入的目录替换：同 id 时替换本地既有文件夹（保命名），
    /// 新皮肤用备份文件夹名；让位备份（.<folder>.old，扫描器安全）成功即清
    #[test]
    fn swap_selected_skins_replaces_by_id_and_adds_new() {
        let root = TestDir::new("sel-swap");
        let skins_dir = root.0.join("skins");
        // 本地：文件夹名与 id 不同名的既有皮肤（替换时必须保这个命名）
        let local_dir = skins_dir.join("clock-old");
        fs::create_dir_all(&local_dir).unwrap();
        fs::write(local_dir.join("skin.json"), r#"{"id":"clock","name":"Clock"}"#).unwrap();
        fs::write(local_dir.join("index.html"), "<html>local</html>").unwrap();
        fs::write(local_dir.join("local.txt"), "keep-out").unwrap();
        // 备份内容：同 id（clock）+ 一个新皮肤（dock）
        let pkg = root.0.join("pkg");
        let pkg_clock = pkg.join("clock-pkg");
        fs::create_dir_all(&pkg_clock).unwrap();
        fs::write(pkg_clock.join("skin.json"), r#"{"id":"clock","name":"Clock"}"#).unwrap();
        fs::write(pkg_clock.join("index.html"), "<html>pkg</html>").unwrap();
        let pkg_dock = pkg.join("dock");
        fs::create_dir_all(&pkg_dock).unwrap();
        fs::write(pkg_dock.join("skin.json"), r#"{"id":"dock","name":"Dock"}"#).unwrap();
        fs::write(pkg_dock.join("index.html"), "<html></html>").unwrap();

        swap_selected_skins(
            &skins_dir,
            &[("clock".to_string(), pkg_clock), ("dock".to_string(), pkg_dock)],
            "zh-CN",
        )
        .unwrap();

        // clock：本地文件夹名保留、内容换为备份版、本地旧文件被替掉、暂存清空
        assert_eq!(fs::read_to_string(local_dir.join("index.html")).unwrap(), "<html>pkg</html>");
        assert!(!local_dir.join("local.txt").exists());
        assert!(!skins_dir.join(".clock-old.old").exists());
        // dock：备份文件夹名落位
        assert!(skins_dir.join("dock").join("skin.json").is_file());
        assert!(!skins_dir.join(".dock.old").exists());
    }

    /// 拷贝失败时：已换的逐个还原、半成品清理（备份来源不存在即触发）
    #[test]
    fn swap_selected_skins_rolls_back_on_copy_failure() {
        let root = TestDir::new("sel-swap-fail");
        let skins_dir = root.0.join("skins");
        let local_dir = skins_dir.join("clock");
        fs::create_dir_all(&local_dir).unwrap();
        fs::write(local_dir.join("skin.json"), r#"{"id":"clock","name":"Clock"}"#).unwrap();
        fs::write(local_dir.join("index.html"), "<html>local</html>").unwrap();
        let missing = root.0.join("no-such-dir");

        assert!(swap_selected_skins(&skins_dir, &[("clock".to_string(), missing)], "zh-CN").is_err());
        // 原皮肤原样恢复，暂存无残留
        assert_eq!(fs::read_to_string(local_dir.join("index.html")).unwrap(), "<html>local</html>");
        assert!(!skins_dir.join(".clock.old").exists());
    }

    /// 审查高危：回退落位撞上承载不同 id 的既有文件夹——预检整体拒绝，
    /// 且被占文件夹分毫不动（让位拷换会把无关皮肤静默销毁）
    #[test]
    fn swap_selected_skins_rejects_folder_occupied_by_other_id() {
        let root = TestDir::new("sel-swap-conflict");
        let skins_dir = root.0.join("skins");
        // 本地：文件夹 clock 承载 id=my-clock（文件夹名 ≠ id 的直装皮肤）
        let local_dir = skins_dir.join("clock");
        fs::create_dir_all(&local_dir).unwrap();
        fs::write(local_dir.join("skin.json"), r#"{"id":"my-clock","name":"My Clock"}"#).unwrap();
        fs::write(local_dir.join("index.html"), "<html>mine</html>").unwrap();
        // 备份：id=clock、文件夹 clock（本地无 id=clock → 走回退落位 skins/clock）
        let pkg_clock = root.0.join("pkg").join("clock");
        fs::create_dir_all(&pkg_clock).unwrap();
        fs::write(pkg_clock.join("skin.json"), r#"{"id":"clock","name":"Clock"}"#).unwrap();
        fs::write(pkg_clock.join("index.html"), "<html>pkg</html>").unwrap();

        let result = swap_selected_skins(&skins_dir, &[("clock".to_string(), pkg_clock)], "zh-CN");
        assert!(result.is_err(), "撞名必须报错");
        // 被占文件夹原样保留（内容、结构均未动）
        assert_eq!(fs::read_to_string(local_dir.join("index.html")).unwrap(), "<html>mine</html>");
        let manifest = fs::read_to_string(local_dir.join("skin.json")).unwrap();
        assert!(manifest.contains("my-clock"), "占用者 manifest 不得被改写");
        assert!(!skins_dir.join(".clock.old").exists(), "不得产生让位残留");
    }
}
