//! .dskin / .zip 皮肤包的校验与安装。
//!
//! 皮肤包是一个 zip 压缩包，skin.json 位于根目录或唯一的一级子目录中。
//! 打包分发时 skin.json 必须声明合法的 `id` 字段 —— 它决定安装文件夹名
//! 和用户数据的归属键，保证更新时用户数据能保留下来。

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use serde::Serialize;

use crate::i18n::{tr, trf, Key};
use crate::skin::loader::{self, validate_skin_id};
use crate::skin::types::{Skin, SkinManifest};

/// 安全上限：防恶意/损坏包耗尽磁盘
const MAX_PACKAGE_BYTES: u64 = 64 * 1024 * 1024; // 压缩包 64 MB
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024; // 解压后合计 256 MB
const MAX_FILES: usize = 5000;

/// 包检查结果，发给前端用于确认弹窗
#[derive(Debug, Clone, Serialize)]
pub struct PackageInfo {
    pub id: String,
    pub name: String,
    /// 英文皮肤名（bilingual 皮肤专用；前端按语言选取，留空回退 name）
    pub name_en: Option<String>,
    pub author: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    /// 英文简介（同 name_en 的选取规则）
    pub description_en: Option<String>,
    /// skin.json 声明的中英双语开关：决定前端是否启用 *_en 文案
    pub bilingual: bool,
    /// skin.json 声明的敏感能力（"registry" / "shell" / "system" /
    /// "clipboard" / "mic"，对应 skin_api 的 PERM_* 常量），
    /// 安装向导展示给用户确认
    pub permissions: Vec<String>,
    /// "new" | "update" | "reinstall" | "downgrade"
    pub status: String,
    /// 已安装的版本（未安装为 None）
    pub installed_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionRelation {
    Same,
    Newer,
    Older,
}

/// 检查一个皮肤包：解析并校验，返回包信息与安装状态。
/// 不是合法皮肤包时返回错误提示。
pub fn inspect_package(package_path: &Path, skins_dir: &Path, lang: &str) -> Result<PackageInfo, String> {
    let extracted = extract_package(package_path, lang)?;
    let result = (|| {
        let base = find_skin_root(extracted.path(), lang)?;
        let manifest = read_manifest(&base, lang)?;
        let id = require_package_id(&manifest, lang)?;
        check_entry_exists(&base, &manifest, lang)?;

        let installed = loader::load_skin_manifest(&skins_dir.join(&id)).ok();
        let (status, installed_version) = match &installed {
            None => ("new", None),
            Some(inst) => {
                let rel = compare_versions(
                    manifest.version.as_deref(),
                    inst.version.as_deref(),
                );
                let status = match rel {
                    VersionRelation::Same => "reinstall",
                    VersionRelation::Newer => "update",
                    VersionRelation::Older => "downgrade",
                };
                (status, inst.version.clone())
            }
        };

        Ok(PackageInfo {
            id,
            name_en: manifest.name_en.clone(),
            description_en: manifest.description_en.clone(),
            bilingual: manifest.bilingual,
            permissions: manifest.permissions.clone(),
            name: manifest.name,
            author: manifest.author,
            version: manifest.version,
            description: manifest.description,
            status: status.to_string(),
            installed_version,
        })
    })();
    result
}

/// 安装（或更新）皮肤包。调用方负责：已加载的皮肤先卸载、安装后再加载。
/// 用户数据不受影响：skin_settings[id] 按 id 归属与文件解耦；皮肤文件夹里的
/// settings.json（皮肤设置页用户值）在整体替换后从旧目录写回（用户值
/// 优先于包内同名文件）。
///
/// 三段式替换，IO 失败不毁已安装皮肤：
/// ① 解压内容完整复制到 `skins/.staging-<id>`；
/// ② 已存在的 `<id>` rename 为 `.<id>.old`；
/// ③ staging rename 为 `<id>`（失败则把 .old rename 回去）；
/// ④ 从 .old 恢复 settings.json，删除 .old。
pub fn install_package(package_path: &Path, skins_dir: &Path, lang: &str) -> Result<Skin, String> {
    let extracted = extract_package(package_path, lang)?;
    let base = find_skin_root(extracted.path(), lang)?;
    let manifest = read_manifest(&base, lang)?;
    let id = require_package_id(&manifest, lang)?;
    check_entry_exists(&base, &manifest, lang)?;

    let dest = skins_dir.join(&id);
    let staging = skins_dir.join(format!(".staging-{}", id));
    let old = skins_dir.join(format!(".{}.old", id));

    // 清理上次安装失败可能留下的暂存目录
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    if old.exists() {
        let _ = fs::remove_dir_all(&old);
    }

    // ① 先完整复制到暂存目录——解压/复制出问题时尚未动已安装的皮肤
    if let Err(e) = copy_dir_recursive(&base, &staging) {
        let _ = fs::remove_dir_all(&staging);
        return Err(trf(lang, Key::InstallSkinFailed, &[&e.to_string()]));
    }

    // ② 旧目录改名让位（失败则丢弃暂存，原皮肤原样保留）
    let had_dest = dest.exists();
    if had_dest {
        if let Err(e) = fs::rename(&dest, &old) {
            let _ = fs::remove_dir_all(&staging);
            return Err(trf(lang, Key::ReplaceOldDirFailed, &[&e.to_string()]));
        }
    }

    // ③ 暂存目录就位（失败则把旧目录 rename 回去，回滚到安装前）
    if let Err(e) = fs::rename(&staging, &dest) {
        if had_dest {
            let _ = fs::rename(&old, &dest);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(trf(lang, Key::InstallSkinFailed, &[&e.to_string()]));
    }

    // ④ 恢复用户设置值，然后删除旧目录。恢复写回失败时保留 .old，
    // 用户数据不丢，可手动找回。
    if had_dest {
        // 读旧设置失败要区分：NotFound = 本来就没有用户设置，跳过正常收尾；
        // 其他错误（权限/占用等）保留 .old 并告警——静默删掉 .old 会把用户
        // 设置一起带走
        match fs::read(old.join(crate::skin::settings::SETTINGS_FILENAME)) {
            Ok(bytes) => {
                fs::write(dest.join(crate::skin::settings::SETTINGS_FILENAME), bytes)
                    .map_err(|e| trf(lang, Key::RestoreSettingsFailed, &[&e.to_string()]))?;
                let _ = fs::remove_dir_all(&old);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let _ = fs::remove_dir_all(&old);
            }
            Err(e) => {
                log::warn!(
                    "Failed to read old settings.json for skin '{}' ({}), keeping {:?} for manual recovery",
                    id, e, old
                );
            }
        }
    }

    Ok(Skin {
        id,
        manifest,
        directory: dest,
    })
}

/// 解压到临时目录，做 zip-slip 与体积防护。返回的守卫在 drop 时清理临时目录。
fn extract_package(package_path: &Path, lang: &str) -> Result<TempDirGuard, String> {
    let file = fs::File::open(package_path)
        .map_err(|e| trf(lang, Key::OpenPackageFailed, &[&e.to_string()]))?;
    let package_size = file.metadata().map(|m| m.len()).unwrap_or(0);
    if package_size > MAX_PACKAGE_BYTES {
        return Err(tr(lang, Key::PackageTooLarge).to_string());
    }

    let mut archive = zip::ZipArchive::new(file)
        .map_err(|_| tr(lang, Key::NotValidZip).to_string())?;
    if archive.len() > MAX_FILES {
        return Err(tr(lang, Key::TooManyFiles).to_string());
    }

    let temp_dir = std::env::temp_dir().join(format!(
        "driftlet-pkg-{}-{}",
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
        let mut entry = archive.by_index(i)
            .map_err(|e| trf(lang, Key::ReadPackageFailed, &[&e.to_string()]))?;
        // enclosed_name 拒绝绝对路径与 ".."，防 zip slip
        let Some(rel) = entry.enclosed_name() else {
            continue; // 跳过不安全路径
        };
        if entry.is_dir() {
            continue;
        }
        let out_path = temp_dir.join(&rel);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = fs::File::create(&out_path).map_err(|e| e.to_string())?;
        // 不信任 zip 头声明的解压大小（可造假）：按实际写出字节数累计，
        // 并用 take 截断读取，确保落盘总量绝不越限（防 zip 炸弹）
        let remaining = MAX_TOTAL_BYTES - total;
        let mut limited = entry.by_ref().take(remaining + 1);
        let written = io::copy(&mut limited, &mut out).map_err(|e| e.to_string())?;
        total += written;
        if total > MAX_TOTAL_BYTES {
            return Err(tr(lang, Key::ExtractedTooLarge).to_string());
        }
    }
    Ok(guard)
}

/// 定位包内皮肤根目录：根目录有 skin.json 则用之；否则要求恰好一个
/// 一级子目录包含 skin.json（常见的“zip 里套一层文件夹”情况）。
fn find_skin_root(extract_dir: &Path, lang: &str) -> Result<PathBuf, String> {
    if extract_dir.join("skin.json").exists() {
        return Ok(extract_dir.to_path_buf());
    }
    let entries: Vec<PathBuf> = fs::read_dir(extract_dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("skin.json").exists())
        .collect();
    match entries.len() {
        1 => Ok(entries.into_iter().next().unwrap()),
        _ => Err(tr(lang, Key::NoSkinJsonInPackage).to_string()),
    }
}

/// 读取并解析包内 skin.json（容忍 UTF-8 BOM）
fn read_manifest(base: &Path, lang: &str) -> Result<SkinManifest, String> {
    let path = base.join("skin.json");
    // 与 loader 同一体积上限：包内清单同样视为小文件，超限即异常（解压
    // 总量上限管不住「一个超大 skin.json + 少量小文件」的畸形包）
    let size = fs::metadata(&path)
        .map_err(|e| trf(lang, Key::ReadSkinJsonFailed, &[&e.to_string()]))?
        .len();
    if size > loader::MAX_MANIFEST_BYTES {
        return Err(format!(
            "skin.json too large ({} bytes, limit {} bytes)",
            size, loader::MAX_MANIFEST_BYTES
        ));
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| trf(lang, Key::ReadSkinJsonFailed, &[&e.to_string()]))?;
    serde_json::from_str(content.trim_start_matches('\u{feff}'))
        .map_err(|e| trf(lang, Key::SkinJsonParseFailed, &[&e.to_string()]))
}

/// 打包分发的皮肤必须声明合法 id —— 它是更新时保留用户数据的关键
fn require_package_id(manifest: &SkinManifest, lang: &str) -> Result<String, String> {
    let id = manifest.id.as_deref()
        .ok_or_else(|| tr(lang, Key::PackageMissingId).to_string())?;
    validate_skin_id(id, lang)?;
    Ok(id.to_string())
}

fn check_entry_exists(base: &Path, manifest: &SkinManifest, lang: &str) -> Result<(), String> {
    // 与 loader 同一套 entry 名校验：含 "../\\:" 的 entry 即使此刻在解压
    // 目录里找得到，装上后也会被 loader 拒载——皮肤「装完即消失」，必须
    // 在安装前拦下
    if !loader::is_valid_entry_name(&manifest.entry) {
        return Err(format!("Invalid entry file name '{}'", manifest.entry));
    }
    if base.join(&manifest.entry).exists() {
        Ok(())
    } else {
        Err(trf(lang, Key::EntryFileMissing, &[manifest.entry.as_str()]))
    }
}

/// 比较版本号。按点分数字段逐段比较（"1.2.0" > "1.10"？否，按数值 10 > 2）。
/// 无法解析时退化为字符串比较：相同 = Same，不同 = Newer（视为更新）。
pub fn compare_versions(a: Option<&str>, b: Option<&str>) -> VersionRelation {
    fn parts(v: &str) -> Option<Vec<u64>> {
        v.trim_start_matches('v')
            .split('.')
            .map(|p| p.parse::<u64>().ok())
            .collect()
    }
    match (a, b) {
        (Some(a), Some(b)) => match (parts(a), parts(b)) {
            (Some(pa), Some(pb)) => {
                for i in 0..pa.len().max(pb.len()) {
                    let x = pa.get(i).copied().unwrap_or(0);
                    let y = pb.get(i).copied().unwrap_or(0);
                    if x != y {
                        return if x > y { VersionRelation::Newer } else { VersionRelation::Older };
                    }
                }
                VersionRelation::Same
            }
            _ => {
                if a == b { VersionRelation::Same } else { VersionRelation::Newer }
            }
        },
        (Some(_), None) => VersionRelation::Newer,
        (None, Some(_)) => VersionRelation::Older,
        (None, None) => VersionRelation::Same,
    }
}

/// 目录递归复制限深：防恶意构造的超深嵌套耗尽路径/栈
const MAX_COPY_DEPTH: u32 = 32;

pub(crate) fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    copy_dir_recursive_inner(src, dst, 0)
}

fn copy_dir_recursive_inner(src: &Path, dst: &Path, depth: u32) -> io::Result<()> {
    if depth > MAX_COPY_DEPTH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
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

/// RAII 守卫：临时解压目录在离开作用域时清理
struct TempDirGuard(PathBuf);
impl TempDirGuard {
    fn path(&self) -> &Path { &self.0 }
}
impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "driftlet-pkgtest-{}-{}-{}",
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

    /// 用 zip writer 造一个皮肤包（Stored 压缩，不依赖 deflate 特性）
    fn write_package(dir: &Path, skin_json: &str, wrap_folder: bool) -> PathBuf {
        write_package_named(dir, "test.dskin", skin_json, wrap_folder)
    }

    fn write_package_named(dir: &Path, filename: &str, skin_json: &str, wrap_folder: bool) -> PathBuf {
        let pkg = dir.join(filename);
        let file = fs::File::create(&pkg).unwrap();
        let mut zw = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let prefix = if wrap_folder { "myskin/" } else { "" };
        zw.start_file(format!("{}skin.json", prefix), opts).unwrap();
        io::Write::write_all(&mut zw, skin_json.as_bytes()).unwrap();
        zw.start_file(format!("{}index.html", prefix), opts).unwrap();
        io::Write::write_all(&mut zw, b"<html></html>").unwrap();
        zw.finish().unwrap();
        pkg
    }

    #[test]
    fn inspect_valid_package_at_root() {
        let dir = unique_dir("root");
        let pkg = write_package(&dir, r#"{"id":"my-skin","name":"My Skin","version":"1.0.0"}"#, false);
        let skins = unique_dir("skins");
        let info = inspect_package(&pkg, &skins, "zh-CN").unwrap();
        assert_eq!(info.id, "my-skin");
        assert_eq!(info.status, "new");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&skins);
    }

    #[test]
    fn inspect_deflated_package() {
        // 真实世界 zip 多用 deflate 压缩 —— 确保 deflate 解码路径可用
        let dir = unique_dir("deflate");
        let pkg = dir.join("deflated.dskin");
        {
            let file = fs::File::create(&pkg).unwrap();
            let mut zw = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zw.start_file("skin.json", opts).unwrap();
            io::Write::write_all(&mut zw, br#"{"id":"deflated-skin","name":"D"}"#).unwrap();
            zw.start_file("index.html", opts).unwrap();
            io::Write::write_all(&mut zw, b"<html></html>").unwrap();
            zw.finish().unwrap();
        }
        let skins = unique_dir("skins");
        let info = inspect_package(&pkg, &skins, "zh-CN").unwrap();
        assert_eq!(info.id, "deflated-skin");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&skins);
    }

    #[test]
    fn inspect_valid_package_wrapped_in_folder() {
        let dir = unique_dir("wrap");
        let pkg = write_package(&dir, r#"{"id":"my-skin","name":"My Skin"}"#, true);
        let skins = unique_dir("skins");
        let info = inspect_package(&pkg, &skins, "zh-CN").unwrap();
        assert_eq!(info.id, "my-skin");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&skins);
    }

    #[test]
    fn rejects_package_without_id() {
        let dir = unique_dir("noid");
        let pkg = write_package(&dir, r#"{"name":"No Id"}"#, false);
        let skins = unique_dir("skins");
        let err = inspect_package(&pkg, &skins, "zh-CN").unwrap_err();
        assert!(err.contains("id"), "unexpected error: {}", err);
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&skins);
    }

    #[test]
    fn rejects_non_zip() {
        let dir = unique_dir("notzip");
        let pkg = dir.join("fake.dskin");
        fs::write(&pkg, b"not a zip at all").unwrap();
        let skins = unique_dir("skins");
        assert!(inspect_package(&pkg, &skins, "zh-CN").is_err());
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&skins);
    }

    #[test]
    fn install_then_update_detects_status() {
        let dir = unique_dir("upd");
        let skins = unique_dir("skins");
        let pkg1 = write_package_named(&dir, "v1.dskin", r#"{"id":"my-skin","name":"My Skin","version":"1.0.0"}"#, false);
        let skin = install_package(&pkg1, &skins, "zh-CN").unwrap();
        assert_eq!(skin.id, "my-skin");
        assert!(skins.join("my-skin").join("index.html").exists());

        // 更新版本 → "update"
        let pkg2 = write_package_named(&dir, "v2.dskin", r#"{"id":"my-skin","name":"My Skin","version":"1.1.0"}"#, false);
        let info = inspect_package(&pkg2, &skins, "zh-CN").unwrap();
        assert_eq!(info.status, "update");
        assert_eq!(info.installed_version.as_deref(), Some("1.0.0"));

        // 更新后旧文件被替换
        install_package(&pkg2, &skins, "zh-CN").unwrap();
        let manifest = loader::load_skin_manifest(&skins.join("my-skin")).unwrap();
        assert_eq!(manifest.version.as_deref(), Some("1.1.0"));

        // 同版本 → "reinstall"，旧版本 → "downgrade"
        let info = inspect_package(&pkg2, &skins, "zh-CN").unwrap();
        assert_eq!(info.status, "reinstall");
        let info = inspect_package(&pkg1, &skins, "zh-CN").unwrap();
        assert_eq!(info.status, "downgrade");

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&skins);
    }

    #[test]
    fn install_update_preserves_settings_json() {
        let dir = unique_dir("pres");
        let skins = unique_dir("skins");
        let pkg1 = write_package_named(&dir, "v1.dskin", r#"{"id":"my-skin","name":"My Skin","version":"1.0.0"}"#, false);
        install_package(&pkg1, &skins, "zh-CN").unwrap();

        // 模拟用户在「皮肤设置」页改过的值
        fs::write(
            skins.join("my-skin").join("settings.json"),
            r##"{"accent":"#00ff00"}"##,
        )
        .unwrap();

        let pkg2 = write_package_named(&dir, "v2.dskin", r#"{"id":"my-skin","name":"My Skin","version":"1.1.0"}"#, false);
        install_package(&pkg2, &skins, "zh-CN").unwrap();

        let content = fs::read_to_string(skins.join("my-skin").join("settings.json")).unwrap();
        assert_eq!(content, r##"{"accent":"#00ff00"}"##, "settings.json must survive update");

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&skins);
    }

    #[test]
    fn install_cleans_staging_dirs_and_leaves_none() {
        let dir = unique_dir("stg");
        let skins = unique_dir("skins");
        let pkg = write_package(&dir, r#"{"id":"my-skin","name":"My Skin","version":"1.0.0"}"#, false);

        // 上次安装失败留下的暂存目录：安装前必须被清理，不干扰本次安装
        fs::create_dir_all(skins.join(".staging-my-skin")).unwrap();
        fs::write(skins.join(".staging-my-skin").join("junk.txt"), "junk").unwrap();
        fs::create_dir_all(skins.join(".my-skin.old")).unwrap();

        install_package(&pkg, &skins, "zh-CN").unwrap();

        assert!(skins.join("my-skin").join("index.html").exists());
        assert!(!skins.join(".staging-my-skin").exists(), "staging must be gone after install");
        assert!(!skins.join(".my-skin.old").exists(), "old dir must be gone after install");
        assert!(!skins.join("my-skin").join("junk.txt").exists(), "staging junk must not leak into dest");

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&skins);
    }

    /// 旧目录被占用（文件句柄未关）时 rename 失败 → 安装报错但原皮肤完好。
    /// 仅 Windows 成立（Linux 允许 rename 被占用的目录）。
    #[cfg(windows)]
    #[test]
    fn failed_replace_keeps_existing_skin_intact() {
        let dir = unique_dir("lock");
        let skins = unique_dir("skins");
        let pkg1 = write_package_named(&dir, "v1.dskin", r#"{"id":"my-skin","name":"My Skin","version":"1.0.0"}"#, false);
        install_package(&pkg1, &skins, "zh-CN").unwrap();

        // 持有已安装皮肤里的文件句柄 → Windows 下 rename 旧目录必失败
        let _open = fs::File::open(skins.join("my-skin").join("index.html")).unwrap();

        let pkg2 = write_package_named(&dir, "v2.dskin", r#"{"id":"my-skin","name":"My Skin","version":"9.9.9"}"#, false);
        assert!(install_package(&pkg2, &skins, "zh-CN").is_err(), "rename of in-use dir must fail");

        // 原皮肤未被替换，暂存目录已清理
        let manifest = loader::load_skin_manifest(&skins.join("my-skin")).unwrap();
        assert_eq!(manifest.version.as_deref(), Some("1.0.0"), "existing skin must stay intact");
        assert!(!skins.join(".staging-my-skin").exists());
        assert!(!skins.join(".my-skin.old").exists());

        drop(_open);
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&skins);
    }

    #[test]
    fn inspect_reports_manifest_permissions() {
        let dir = unique_dir("perms");
        let pkg = write_package(
            &dir,
            r#"{"id":"my-skin","name":"My Skin","permissions":["registry","shell"]}"#,
            false,
        );
        let skins = unique_dir("skins");
        let info = inspect_package(&pkg, &skins, "zh-CN").unwrap();
        assert_eq!(info.permissions, vec!["registry".to_string(), "shell".to_string()]);
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&skins);
    }

    #[test]
    fn rejects_zip_bomb_by_actual_bytes() {
        // zip 头声明大小可造假：这里用 deflate 压缩 256MB+1 的零字节
        //（压缩包本身只有几百 KB），按实际写出量必须触发上限
        let dir = unique_dir("bomb");
        let pkg = dir.join("bomb.dskin");
        {
            let file = fs::File::create(&pkg).unwrap();
            let mut zw = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zw.start_file("skin.json", opts).unwrap();
            io::Write::write_all(&mut zw, br#"{"id":"bomb-skin","name":"B"}"#).unwrap();
            zw.start_file("index.html", opts).unwrap();
            io::Write::write_all(&mut zw, b"<html></html>").unwrap();
            zw.start_file("payload.bin", opts).unwrap();
            let zeros = vec![0u8; 1024 * 1024];
            for _ in 0..(MAX_TOTAL_BYTES / 1024 / 1024 + 1) {
                io::Write::write_all(&mut zw, &zeros).unwrap();
            }
            zw.finish().unwrap();
        }
        let skins = unique_dir("skins");
        let err = inspect_package(&pkg, &skins, "zh-CN").unwrap_err();
        assert_eq!(err, "皮肤包解压后过大", "zip bomb must hit the extracted-size limit");

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&skins);
    }

    #[test]
    fn version_comparison() {
        use VersionRelation::*;
        assert_eq!(compare_versions(Some("1.2.0"), Some("1.10.0")), Older);
        assert_eq!(compare_versions(Some("2.0"), Some("1.9.9")), Newer);
        assert_eq!(compare_versions(Some("1.0.0"), Some("1.0")), Same);
        assert_eq!(compare_versions(Some("abc"), Some("abc")), Same);
        assert_eq!(compare_versions(Some("abc"), Some("def")), Newer);
        assert_eq!(compare_versions(None, None), Same);
    }
}
