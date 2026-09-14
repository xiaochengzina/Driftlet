//! .dskin / .zip 皮肤包的校验与安装。
//!
//! 皮肤包是一个 zip 压缩包，skin.json 位于根目录或唯一的一级子目录中。
//! 打包分发时 skin.json 必须声明合法的 `id` 字段 —— 它决定安装文件夹名
//! 和用户数据的归属键，保证更新时用户数据能保留下来。

use serde::Serialize;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::i18n::{Key, tr, trf};
use crate::skin::loader::{self, validate_skin_id};
use crate::skin::types::{Skin, SkinManifest};

/// 安全上限：防恶意/损坏包耗尽磁盘
/// （tools/pack-skin/src/main.rs 手工镜像了这些上限与清单体积上限——
/// 改动必须同步并重建 pack-skin.exe）
const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024; // 压缩包 256 MB
const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024; // 解压后合计 1 GB
const MAX_FILES: usize = 10000;

/// 创作者自带预览图的尺寸上限（像素）。解码内存 = 宽×高×4B 驻留管理器
/// 渲染进程（列表里每张可见预览各一份），112px 高的卡片用不到巨型原图。
/// 与 capture.rs 的截图降采样（640px）口径不同是刻意的：那边服务列表
/// 缩略，这里允许创作者保留更精细的原始预览。
/// （tools/pack-skin/src/main.rs 手工镜像了该上限——改动必须同步并重建）
const MAX_PREVIEW_DIMENSION: u32 = 1280;

/// loader.rs 认的预览文件名（三者只生效其一，查找顺序即此顺序）
const PREVIEW_FILE_NAMES: [&str; 3] = ["preview.png", "preview.jpg", "preview.jpeg"];

/// 包检查结果，发给前端用于确认弹窗
#[derive(Debug, Clone, Serialize)]
pub struct PackageInfo {
    pub id: String,
    /// 中文皮肤名（manifest 的 name_zh——旧字段名 name 经 alias 解析进这里；
    /// 前端 dom.js dispName 按界面语言选取，缺失/为空回退 name_en）
    pub name_zh: String,
    /// 英文皮肤名（同 name_zh 的选取规则）
    pub name_en: Option<String>,
    pub author: Option<String>,
    pub version: Option<String>,
    /// 中文简介（manifest 的 description_zh，旧字段名 description 经 alias）
    pub description_zh: Option<String>,
    /// 英文简介（同 name_en 的选取规则）
    pub description_en: Option<String>,
    /// skin.json 声明的敏感能力（"registry" / "shell" / "system" /
    /// "clipboard" / "mic" / "file_system" / "control"，对应 skin_api 的
    /// PERM_* 常量），
    /// 安装向导展示给用户确认
    pub permissions: Vec<String>,
    /// "new" | "update" | "reinstall" | "downgrade"
    pub status: String,
    /// 已安装的版本（未安装为 None）
    pub installed_version: Option<String>,
    /// skin.json 声明的 min_host_version 高于当前宿主版本时为 Some（该值），
    /// 安装向导据此提示「部分功能可能不可用」；满足或未声明为 None。
    pub requires_host_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionRelation {
    Same,
    Newer,
    Older,
}

/// 检查一个皮肤包：解析并校验，返回包信息与安装状态。
/// 不是合法皮肤包时返回错误提示。
pub fn inspect_package(
    package_path: &Path,
    skins_dir: &Path,
    lang: &str,
) -> Result<PackageInfo, String> {
    let extracted = extract_package(package_path, lang)?;
    let result = (|| {
        let base = find_skin_root(extracted.path(), lang)?;
        let manifest = read_manifest(&base, lang)?;
        let id = require_package_id(&manifest, lang)?;
        check_entry_exists(&base, &manifest, lang)?;
        check_preview_limits(&base, lang)?;

        let installed = loader::load_skin_manifest(&skins_dir.join(&id)).ok();
        let (status, installed_version) = match &installed {
            None => ("new", None),
            Some(inst) => {
                let rel = compare_versions(manifest.version.as_deref(), inst.version.as_deref());
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
            permissions: manifest.permissions.clone(),
            name_zh: manifest.name_zh.clone().unwrap_or_default(),
            author: manifest.author,
            version: manifest.version,
            description_zh: manifest.description_zh,
            status: status.to_string(),
            installed_version,
            // 宿主版本不足时把要求值带给向导（提示不拦截；比较复用更新检测的
            // 数字段口径）
            requires_host_version: manifest
                .min_host_version
                .as_deref()
                .filter(|v| crate::update::is_newer(v, env!("CARGO_PKG_VERSION")))
                .map(str::to_string),
        })
    })();
    result
}

/// 安装（或更新）皮肤包。调用方负责：已加载的皮肤先卸载、安装后再加载。
/// 启动恢复 `skins/` 内的逐文件夹暂存残留（`.<folder>.old` / `.staging-*`）。
/// 三段式替换（包安装/选择性导入/从源同步共用）崩溃在「让位 → 拷入」之间
/// 会留下「目标缺失 + .old 唯一副本」——扫描器跳过点开头目录，皮肤就此
/// 消失（随后 prune 把它的 config 条目也抹掉，残留连备份导出都跳过）。
/// 恢复语义同 backup::rollback_interrupted_import（审查 A-H1，旧副本优先
/// = 安全方向）：.staging-* 一律删除（纯半成品）；.<folder>.old → 目标在
/// 也删目标（可能是半成品）后 rename 回 .old。返回恢复条数（日志用）。
pub fn recover_interrupted_folder_ops(skins_dir: &Path) -> usize {
    if !skins_dir.is_dir() {
        return 0;
    }
    let mut recovered = 0;
    let Ok(entries) = fs::read_dir(skins_dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        if name.starts_with(".staging-") {
            let _ = fs::remove_dir_all(&path);
            continue;
        }
        let Some(folder) = name
            .strip_prefix('.')
            .and_then(|n| n.strip_suffix(".old"))
            .filter(|f| !f.is_empty())
        else {
            continue;
        };
        let target = skins_dir.join(folder);
        if target.exists() {
            let _ = fs::remove_dir_all(&target);
        }
        match fs::rename(&path, &target) {
            Ok(()) => {
                recovered += 1;
                log::warn!(
                    "folder-op recovery: {:?} restored from interrupted replace",
                    target
                );
            }
            Err(e) => log::error!("folder-op recovery failed for {:?}: {}", path, e),
        }
    }
    recovered
}

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
    check_preview_limits(&base, lang)?;

    // 同 id 不同文件夹名遮蔽：扫描去重按文件夹名字典序保留先者，而包装载
    // 一律落在 skins/<id>——若已有「id 相同但文件夹名 ≠ id」的皮肤（如文件夹
    // 直装 "CoolWidget"），新包装上后会被静默遮蔽、永远加载旧版本。安装前
    // 拦下并引导先移除旧副本。
    if let Some(existing) = loader::scan_skins_directory(skins_dir)
        .into_iter()
        .find(|s| s.id == id)
    {
        let folder = existing
            .directory
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if folder != id {
            return Err(format!(
                "another copy of skin '{}' already exists at folder '{}' — remove it first, then install again",
                id, folder
            ));
        }
    }

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
                    id,
                    e,
                    old
                );
            }
        }
    }

    Ok(Skin {
        id,
        manifest,
        // 安装路径的皮肤不是副本（副本只由 duplicate_skin 产生并写入标记）
        origin: None,
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

    let mut archive =
        zip::ZipArchive::new(file).map_err(|_| tr(lang, Key::NotValidZip).to_string())?;
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
    fs::create_dir_all(&temp_dir)
        .map_err(|e| trf(lang, Key::CreateTempDirFailed, &[&e.to_string()]))?;

    let guard = TempDirGuard(temp_dir.clone());
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
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
            size,
            loader::MAX_MANIFEST_BYTES
        ));
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| trf(lang, Key::ReadSkinJsonFailed, &[&e.to_string()]))?;
    serde_json::from_str(content.trim_start_matches('\u{feff}'))
        .map_err(|e| trf(lang, Key::SkinJsonParseFailed, &[&e.to_string()]))
}

/// 打包分发的皮肤必须声明合法 id —— 它是更新时保留用户数据的关键
fn require_package_id(manifest: &SkinManifest, lang: &str) -> Result<String, String> {
    let id = manifest
        .id
        .as_deref()
        .ok_or_else(|| tr(lang, Key::PackageMissingId).to_string())?;
    validate_skin_id(id, lang)?;
    Ok(id.to_string())
}

fn check_entry_exists(base: &Path, manifest: &SkinManifest, lang: &str) -> Result<(), String> {
    // 例外：http(s) URL 入口 = 网页皮肤，无本地文件可查
    if crate::skin::types::is_url_entry(&manifest.entry) {
        return Ok(());
    }
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

/// 预览图尺寸校验（不解码，只读文件头取尺寸）：超上限拦截。头解析失败
/// （损坏/格式不明）放行——那种图浏览器同样解不出来，没有内存风险，
/// 「读不懂」不能当「超大」拦。
fn check_preview_limits(base: &Path, lang: &str) -> Result<(), String> {
    for name in PREVIEW_FILE_NAMES {
        let path = base.join(name);
        if !path.is_file() {
            continue;
        }
        return match image_dimensions(&path) {
            Some((w, h)) if w.max(h) > MAX_PREVIEW_DIMENSION => Err(trf(
                lang,
                Key::PreviewTooLarge,
                &[
                    name,
                    &w.to_string(),
                    &h.to_string(),
                    &MAX_PREVIEW_DIMENSION.to_string(),
                ],
            )),
            Some(_) => Ok(()),
            None => {
                log::warn!("package: 预览图尺寸读取失败，跳过上限检查: {:?}", path);
                Ok(())
            }
        };
    }
    Ok(())
}

/// 只读图片头取尺寸（PNG 看 IHDR，JPEG 扫 SOF 段），不解码像素。
/// tools/pack-skin/src/main.rs 手工镜像本函数——改动必须同步并重建。
fn image_dimensions(path: &Path) -> Option<(u32, u32)> {
    // 前 512KB 探测窗：PNG 的 IHDR 恒在前 24 字节；JPEG 的 SOF 在帧头
    // 段链里，EXIF 再大 512KB 也兜得住。截断读防单文件 probing 放大
    // （此时解压总量上限还没兜住「单个巨型预览」的情形）
    const PROBE_BYTES: u64 = 512 * 1024;
    let mut head = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(PROBE_BYTES)
        .read_to_end(&mut head)
        .ok()?;
    png_dimensions(&head).or_else(|| jpeg_dimensions(&head))
}

fn png_dimensions(head: &[u8]) -> Option<(u32, u32)> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if head.len() < 24 || head[..8] != SIG {
        return None;
    }
    // IHDR 恒为首个 chunk：长度(4) + 类型(4) + 宽(4) + 高(4)
    if &head[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes(head[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(head[20..24].try_into().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

fn jpeg_dimensions(head: &[u8]) -> Option<(u32, u32)> {
    if head.len() < 4 || head[0] != 0xff || head[1] != 0xd8 {
        return None;
    }
    let mut i = 2;
    while i + 9 < head.len() {
        if head[i] != 0xff {
            i += 1;
            continue;
        }
        let marker = head[i + 1];
        i += 2;
        // 无长度段的独立标记：SOI/EOI/RSTn/TEM，跳过一个字节继续找
        if marker == 0xd8 || marker == 0xd9 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        // SOS 之后才是扫描数据，SOF 必然在其前——走到这说明头被截断或损坏
        if marker == 0xda {
            return None;
        }
        if i + 2 > head.len() {
            return None;
        }
        let seg_len = u16::from_be_bytes([head[i], head[i + 1]]) as usize;
        if seg_len < 2 {
            return None;
        }
        // SOF0–SOF15（C0–CF）的载荷是帧头；C4(DHT)/C8(JPG)/CC(DAC) 同名异义
        if (0xc0..=0xcf).contains(&marker) && marker != 0xc4 && marker != 0xc8 && marker != 0xcc {
            if i + 7 > head.len() {
                return None;
            }
            // 段布局：长度(2) + 精度(1) + 高(2) + 宽(2) + 分量数(1)…
            let h = u16::from_be_bytes([head[i + 3], head[i + 4]]) as u32;
            let w = u16::from_be_bytes([head[i + 5], head[i + 6]]) as u32;
            return (w > 0 && h > 0).then_some((w, h));
        }
        i += seg_len;
    }
    None
}

/// 把皮肤文件夹打成 .dskin 分发包，返回打包的文件数。
/// 收集规则 / skip 清单 / zip 约定（Deflated + 正斜杠路径）与
/// tools/pack-skin/src/main.rs 手工镜像——改动必须两边同步（镜像对拍脚本
/// 已覆盖 skip 清单与体积上限）。
/// 先过装载校验（manifest 合法 + entry 存在——打出去的包必须能装回来），
/// 再经 .tmp 原子就位（中断不留半截包）。
pub fn create_package(skin_dir: &Path, out_path: &Path, lang: &str) -> Result<usize, String> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    loader::load_skin_manifest(skin_dir)?;
    // 创作者自带预览图的尺寸上限与安装侧同口径：打包即拦，不等到装回
    check_preview_limits(skin_dir, lang)?;

    // settings.json* 是用户的设置值数据（运行时生成），不打进分发包；
    // *.dskin 是旧打包产物防滚进新包。比较大小写不敏感（全小写常量）
    const SKIP_FILES: [&str; 6] = [
        ".ds_store",
        "thumbs.db",
        "desktop.ini",
        "settings.json",
        "settings.json.bak",
        "settings.json.tmp",
    ];
    // 版本控制与依赖目录不属于皮肤资源
    const SKIP_DIRS: [&str; 3] = [".git", ".svn", "node_modules"];

    fn collect(dir: &Path, base: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let p = entry.path();
            if p.is_dir() {
                if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    if SKIP_DIRS.contains(&name) {
                        continue;
                    }
                }
                collect(&p, base, out)?;
            } else if p.is_file() {
                if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    let lower = name.to_ascii_lowercase();
                    if SKIP_FILES.contains(&lower.as_str()) || lower.ends_with(".dskin") {
                        // 子目录里出现排除条目多半是误放（pack-skin 会提示）——
                        // 应用侧无终端，记日志
                        if dir != base {
                            log::info!(
                                "package: skipping excluded entry in subdir {}",
                                p.display()
                            );
                        }
                        continue;
                    }
                }
                if let Ok(rel) = p.strip_prefix(base) {
                    out.push(rel.to_path_buf());
                }
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    collect(skin_dir, skin_dir, &mut files)
        .map_err(|e| trf(lang, Key::PackageCreateFailed, &[&e]))?;

    // 与 pack-skin 同口径：解压后合计上限（防误打包巨型目录）
    let total_bytes: u64 = files
        .iter()
        .map(|r| fs::metadata(skin_dir.join(r)).map(|m| m.len()).unwrap_or(0))
        .sum();
    if total_bytes > MAX_TOTAL_BYTES {
        return Err(trf(
            lang,
            Key::PackageCreateFailed,
            &[&format!(
                "文件总体积超过安装上限（{} MB）",
                MAX_TOTAL_BYTES / 1024 / 1024
            )],
        ));
    }

    let tmp = out_path.with_extension("dskin.tmp");
    let result: Result<(), String> = (|| -> Result<(), String> {
        let file = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut zw = zip::ZipWriter::new(file);
        let opts =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for rel in &files {
            // zip 内路径统一用正斜杠；非 ASCII 文件名 zip crate 自动置 UTF-8 标志
            let rel_unix = rel.to_string_lossy().replace('\\', "/");
            zw.start_file(&rel_unix, opts).map_err(|e| e.to_string())?;
            let data = fs::read(skin_dir.join(rel)).map_err(|e| e.to_string())?;
            zw.write_all(&data).map_err(|e| e.to_string())?;
        }
        zw.finish().map_err(|e| e.to_string())?;
        Ok(())
    })()
    .map_err(|e| trf(lang, Key::PackageCreateFailed, &[&e]));
    match result {
        Ok(()) => {
            // 产物必须装得回来：压缩后体积同样过上限（与安装侧同口径——
            // 不可压缩内容（已压缩的图/视频）多时可能超，不查会产出安装侧
            // 必拒的包）
            let size = fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
            if size > MAX_PACKAGE_BYTES {
                let _ = fs::remove_file(&tmp);
                return Err(tr(lang, Key::PackageTooLarge).to_string());
            }
            fs::rename(&tmp, out_path)
                .map(|_| files.len())
                .map_err(|e| {
                    let _ = fs::remove_file(&tmp);
                    trf(lang, Key::PackageCreateFailed, &[&e.to_string()])
                })
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
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
                        return if x > y {
                            VersionRelation::Newer
                        } else {
                            VersionRelation::Older
                        };
                    }
                }
                VersionRelation::Same
            }
            _ => {
                if a == b {
                    VersionRelation::Same
                } else {
                    VersionRelation::Newer
                }
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
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::fs::MetadataExt;
            // junction 在 Windows 上不被 is_symlink 标记，须按属性位判
            //（reparse point 成环可穿越出暂存树）
            if meta.file_attributes() & 0x400 != 0 {
                log::warn!(
                    "Skipping junction/reparse point during skin copy: {:?}",
                    src_path
                );
                continue;
            }
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
        assert!(
            err.to_string().contains("nesting"),
            "unexpected error: {}",
            err
        );

        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);
    }

    /// 用 zip writer 造一个皮肤包（Stored 压缩，不依赖 deflate 特性）
    fn write_package(dir: &Path, skin_json: &str, wrap_folder: bool) -> PathBuf {
        write_package_named(dir, "test.dskin", skin_json, wrap_folder)
    }

    fn write_package_named(
        dir: &Path,
        filename: &str,
        skin_json: &str,
        wrap_folder: bool,
    ) -> PathBuf {
        let pkg = dir.join(filename);
        let file = fs::File::create(&pkg).unwrap();
        let mut zw = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let prefix = if wrap_folder { "myskin/" } else { "" };
        zw.start_file(format!("{}skin.json", prefix), opts).unwrap();
        io::Write::write_all(&mut zw, skin_json.as_bytes()).unwrap();
        zw.start_file(format!("{}index.html", prefix), opts)
            .unwrap();
        io::Write::write_all(&mut zw, b"<html></html>").unwrap();
        zw.finish().unwrap();
        pkg
    }

    #[test]
    fn extract_skips_zip_slip_entries() {
        let dir = unique_dir("zipslip");
        let pkg = dir.join("slip.dskin");
        let file = fs::File::create(&pkg).unwrap();
        let mut zw = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zw.start_file("skin.json", opts).unwrap();
        io::Write::write_all(&mut zw, br#"{"name":"T","id":"slip-skin"}"#).unwrap();
        // 逃逸条目：解压目标的上一级（系统临时目录）——enclosed_name 必须拦下
        let marker = format!("driftlet-zipslip-{}-marker.txt", std::process::id());
        zw.start_file(format!("../{}", marker), opts).unwrap();
        io::Write::write_all(&mut zw, b"evil").unwrap();
        zw.finish().unwrap();

        let extracted = extract_package(&pkg, "zh-CN").unwrap();
        assert!(
            !std::env::temp_dir().join(&marker).exists(),
            "zip-slip 条目落盘了：enclosed_name 防护失效"
        );
        assert!(extracted.path().join("skin.json").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn inspect_valid_package_at_root() {
        let dir = unique_dir("root");
        let pkg = write_package(
            &dir,
            r#"{"id":"my-skin","name":"My Skin","version":"1.0.0"}"#,
            false,
        );
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
        let pkg1 = write_package_named(
            &dir,
            "v1.dskin",
            r#"{"id":"my-skin","name":"My Skin","version":"1.0.0"}"#,
            false,
        );
        let skin = install_package(&pkg1, &skins, "zh-CN").unwrap();
        assert_eq!(skin.id, "my-skin");
        assert!(skins.join("my-skin").join("index.html").exists());

        // 更新版本 → "update"
        let pkg2 = write_package_named(
            &dir,
            "v2.dskin",
            r#"{"id":"my-skin","name":"My Skin","version":"1.1.0"}"#,
            false,
        );
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
        let pkg1 = write_package_named(
            &dir,
            "v1.dskin",
            r#"{"id":"my-skin","name":"My Skin","version":"1.0.0"}"#,
            false,
        );
        install_package(&pkg1, &skins, "zh-CN").unwrap();

        // 模拟用户在「皮肤设置」页改过的值
        fs::write(
            skins.join("my-skin").join("settings.json"),
            r##"{"accent":"#00ff00"}"##,
        )
        .unwrap();

        let pkg2 = write_package_named(
            &dir,
            "v2.dskin",
            r#"{"id":"my-skin","name":"My Skin","version":"1.1.0"}"#,
            false,
        );
        install_package(&pkg2, &skins, "zh-CN").unwrap();

        let content = fs::read_to_string(skins.join("my-skin").join("settings.json")).unwrap();
        assert_eq!(
            content, r##"{"accent":"#00ff00"}"##,
            "settings.json must survive update"
        );

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&skins);
    }

    #[test]
    fn install_cleans_staging_dirs_and_leaves_none() {
        let dir = unique_dir("stg");
        let skins = unique_dir("skins");
        let pkg = write_package(
            &dir,
            r#"{"id":"my-skin","name":"My Skin","version":"1.0.0"}"#,
            false,
        );

        // 上次安装失败留下的暂存目录：安装前必须被清理，不干扰本次安装
        fs::create_dir_all(skins.join(".staging-my-skin")).unwrap();
        fs::write(skins.join(".staging-my-skin").join("junk.txt"), "junk").unwrap();
        fs::create_dir_all(skins.join(".my-skin.old")).unwrap();

        install_package(&pkg, &skins, "zh-CN").unwrap();

        assert!(skins.join("my-skin").join("index.html").exists());
        assert!(
            !skins.join(".staging-my-skin").exists(),
            "staging must be gone after install"
        );
        assert!(
            !skins.join(".my-skin.old").exists(),
            "old dir must be gone after install"
        );
        assert!(
            !skins.join("my-skin").join("junk.txt").exists(),
            "staging junk must not leak into dest"
        );

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
        let pkg1 = write_package_named(
            &dir,
            "v1.dskin",
            r#"{"id":"my-skin","name":"My Skin","version":"1.0.0"}"#,
            false,
        );
        install_package(&pkg1, &skins, "zh-CN").unwrap();

        // 持有已安装皮肤里的文件句柄 → Windows 下 rename 旧目录必失败
        let _open = fs::File::open(skins.join("my-skin").join("index.html")).unwrap();

        let pkg2 = write_package_named(
            &dir,
            "v2.dskin",
            r#"{"id":"my-skin","name":"My Skin","version":"9.9.9"}"#,
            false,
        );
        assert!(
            install_package(&pkg2, &skins, "zh-CN").is_err(),
            "rename of in-use dir must fail"
        );

        // 原皮肤未被替换，暂存目录已清理
        let manifest = loader::load_skin_manifest(&skins.join("my-skin")).unwrap();
        assert_eq!(
            manifest.version.as_deref(),
            Some("1.0.0"),
            "existing skin must stay intact"
        );
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
        assert_eq!(
            info.permissions,
            vec!["registry".to_string(), "shell".to_string()]
        );
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&skins);
    }

    #[test]
    fn rejects_zip_bomb_by_actual_bytes() {
        // zip 头声明大小可造假：这里用 deflate 压缩「上限+1MB」的零字节
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
        assert_eq!(
            err, "皮肤包解压后过大",
            "zip bomb must hit the extracted-size limit"
        );

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

    /// 预览图尺寸校验：只读文件头（不解码像素），PNG 看 IHDR / JPEG 扫 SOF。
    /// 头级构造即可——解析器根本不碰像素数据。
    fn png_header(w: u32, h: u32) -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        v.extend_from_slice(&13u32.to_be_bytes()); // IHDR 长度
        v.extend_from_slice(b"IHDR");
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&h.to_be_bytes());
        v
    }

    fn jpeg_header(w: u16, h: u16) -> Vec<u8> {
        // SOI + SOF0（段布局：长度(2) 精度(1) 高(2) 宽(2) 分量数(1)）
        vec![
            0xff,
            0xd8,
            0xff,
            0xc0,
            0x00,
            0x11,
            0x08,
            (h >> 8) as u8,
            (h & 0xff) as u8,
            (w >> 8) as u8,
            (w & 0xff) as u8,
            0x03,
        ]
    }

    #[test]
    fn preview_dimensions_parse_from_headers() {
        assert_eq!(png_dimensions(&png_header(4000, 3000)), Some((4000, 3000)));
        assert_eq!(png_dimensions(&png_header(100, 60)), Some((100, 60)));
        assert_eq!(
            jpeg_dimensions(&jpeg_header(1920, 1080)),
            Some((1920, 1080))
        );
        assert_eq!(jpeg_dimensions(&jpeg_header(320, 200)), Some((320, 200)));
        // 非图片/截断输入一律 None（放行路径，由浏览器解码兜底）
        assert_eq!(png_dimensions(b"not an image at all"), None);
        assert_eq!(jpeg_dimensions(b"\xff\xd8\xff"), None);
        assert_eq!(
            image_dimensions(&std::env::temp_dir().join("definitely-missing.png")),
            None
        );
    }

    #[test]
    fn rejects_oversized_preview_png() {
        let dir = unique_dir("bigprev");
        let pkg = write_package(&dir, r#"{"id":"my-skin","name":"My Skin"}"#, false);
        // 往解压包里注入超大尺寸的 PNG 头
        let file = fs::File::create(&pkg).unwrap();
        let mut zw = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zw.start_file("skin.json", opts).unwrap();
        io::Write::write_all(&mut zw, br#"{"id":"my-skin","name":"My Skin"}"#).unwrap();
        zw.start_file("index.html", opts).unwrap();
        io::Write::write_all(&mut zw, b"<html></html>").unwrap();
        zw.start_file("preview.png", opts).unwrap();
        io::Write::write_all(&mut zw, &png_header(4000, 3000)).unwrap();
        zw.finish().unwrap();

        let skins = unique_dir("skins");
        let err = inspect_package(&pkg, &skins, "zh-CN").unwrap_err();
        assert!(err.contains("preview.png"), "unexpected error: {}", err);
        assert!(err.contains("4000"), "unexpected error: {}", err);
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&skins);
    }

    #[test]
    fn accepts_preview_within_limit() {
        let dir = unique_dir("okprev");
        let pkg = dir.join("ok.dskin");
        let file = fs::File::create(&pkg).unwrap();
        let mut zw = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zw.start_file("skin.json", opts).unwrap();
        io::Write::write_all(&mut zw, br#"{"id":"my-skin","name":"My Skin"}"#).unwrap();
        zw.start_file("index.html", opts).unwrap();
        io::Write::write_all(&mut zw, b"<html></html>").unwrap();
        zw.start_file("preview.jpg", opts).unwrap();
        io::Write::write_all(&mut zw, &jpeg_header(1280, 720)).unwrap();
        zw.finish().unwrap();

        let skins = unique_dir("skins");
        let info = inspect_package(&pkg, &skins, "zh-CN").unwrap();
        assert_eq!(info.id, "my-skin");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&skins);
    }

    #[test]
    fn unreadable_preview_header_passes_with_warning() {
        // 头解析不出的预览不放行拦截（浏览器同样解不出，无内存风险）
        let dir = unique_dir("badprev");
        let pkg = dir.join("bad.dskin");
        let file = fs::File::create(&pkg).unwrap();
        let mut zw = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zw.start_file("skin.json", opts).unwrap();
        io::Write::write_all(&mut zw, br#"{"id":"my-skin","name":"My Skin"}"#).unwrap();
        zw.start_file("index.html", opts).unwrap();
        io::Write::write_all(&mut zw, b"<html></html>").unwrap();
        zw.start_file("preview.png", opts).unwrap();
        io::Write::write_all(&mut zw, b"garbage-bytes").unwrap();
        zw.finish().unwrap();

        let skins = unique_dir("skins");
        let info = inspect_package(&pkg, &skins, "zh-CN").unwrap();
        assert_eq!(info.id, "my-skin");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&skins);
    }

    #[test]
    fn create_package_rejects_oversized_preview() {
        // 创作者直装目录打包：自带超大预览应在打包侧被拦（与安装侧同口径）
        let skin = unique_dir("pkgskin");
        fs::write(
            skin.join("skin.json"),
            r#"{"id":"my-skin","name":"My Skin","entry":"index.html"}"#,
        )
        .unwrap();
        fs::write(skin.join("index.html"), "<html></html>").unwrap();
        fs::write(skin.join("preview.png"), png_header(5000, 5000)).unwrap();
        let out = unique_dir("pkgout").join("out.dskin");
        let err = create_package(&skin, &out, "zh-CN").unwrap_err();
        assert!(err.contains("preview.png"), "unexpected error: {}", err);
    }

    /// 审查高危修复的启动恢复：.<folder>.old 旧副本还原（目标缺失/半成品
    /// 形态都还原——A-H1 旧副本优先），.staging-* 纯垃圾清理，正常目录不动
    #[test]
    fn recover_interrupted_folder_ops_restores_old_copy() {
        let skins = unique_dir("skins-recover");
        // 形态 1：目标缺失 + .old 唯一副本（让位后、拷入前崩）
        fs::create_dir_all(skins.join(".clock.old")).unwrap();
        fs::write(
            skins.join(".clock.old").join("skin.json"),
            r#"{"id":"clock"}"#,
        )
        .unwrap();
        fs::write(
            skins.join(".clock.old").join("settings.json"),
            r#"{"city":"tokyo"}"#,
        )
        .unwrap();
        // 形态 2：半成品目标 + 完好 .old（拷入中途崩——旧副本优先还原）
        fs::create_dir_all(skins.join("dock")).unwrap(); // 半成品（无 skin.json）
        fs::create_dir_all(skins.join(".dock.old")).unwrap();
        fs::write(
            skins.join(".dock.old").join("skin.json"),
            r#"{"id":"dock"}"#,
        )
        .unwrap();
        // 纯垃圾：.staging-*；正常目录：不得触碰
        fs::create_dir_all(skins.join(".staging-junk")).unwrap();
        fs::write(skins.join(".staging-junk").join("x"), "x").unwrap();
        fs::create_dir_all(skins.join("fine-skin")).unwrap();
        fs::write(
            skins.join("fine-skin").join("skin.json"),
            r#"{"id":"fine"}"#,
        )
        .unwrap();

        assert_eq!(recover_interrupted_folder_ops(&skins), 2);
        assert!(
            skins.join("clock").join("skin.json").is_file(),
            "缺失目标必须还原"
        );
        assert!(
            skins.join("clock").join("settings.json").is_file(),
            "用户设置必须随还原保留"
        );
        assert!(
            skins.join("dock").join("skin.json").is_file(),
            "半成品必须被旧副本覆盖"
        );
        assert!(
            !skins.join(".clock.old").exists() && !skins.join(".dock.old").exists(),
            "还原后暂存必须消失"
        );
        assert!(
            !skins.join(".staging-junk").exists(),
            "staging 垃圾必须清理"
        );
        assert!(
            skins.join("fine-skin").join("skin.json").is_file(),
            "正常目录不得被碰"
        );

        let _ = fs::remove_dir_all(&skins);
    }
}
