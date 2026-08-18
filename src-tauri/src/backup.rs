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

/// 导入崩溃窗口的启动回滚：②就位阶段断电/崩溃后，可能只剩 `<name>.import-old`
/// 而 `<name>` 缺失或残缺——数据目录「凭空消失」。启动时检测：<name>.import-old
/// 存在且 <name> 不存在则 rename 回滚；两者都在则视为已成功完成，丢弃 .import-old。
/// 返回回滚条数（日志用）。
pub fn rollback_interrupted_import(config_dir: &Path, skins_dir: &Path) -> usize {
    let mut rolled = 0;
    for dir in [config_dir, skins_dir] {
        let old = import_old_sibling(dir);
        if !old.exists() {
            continue;
        }
        if !dir.exists() {
            match fs::rename(&old, dir) {
                Ok(()) => {
                    rolled += 1;
                    log::warn!("import rollback: {:?} restored from interrupted import", dir);
                }
                Err(e) => log::error!("import rollback failed for {:?}: {}", dir, e),
            }
        } else {
            let _ = fs::remove_dir_all(&old);
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
        use tauri_plugin_autostart::ManagerExt;
        let r = if autostart {
            app.autolaunch().enable()
        } else {
            app.autolaunch().disable()
        };
        if let Err(e) = r {
            log::warn!("import: failed to sync autostart: {}", e);
        }
    }

    crate::hotkey::reregister_from_config(app);

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
        fs::write(skins_dir.join("skin.json"), r#"{"id":"clock"}"#).unwrap();
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
}
