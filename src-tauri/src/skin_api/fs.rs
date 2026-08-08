//! Sandboxed file access for skins (the `files` permission).
//!
//! Every path a skin touches resolves inside its own install directory:
//! absolute paths and `..` components are rejected up front, then the path
//! (for a not-yet-existing write target: its deepest existing ancestor) is
//! canonicalized and must still sit under the canonicalized skin
//! directory — which also defeats symlink escapes.  App-managed files
//! (skin.json / settings.json*) stay readable but can never be written or
//! deleted by the skin.

use std::path::{Component, Path, PathBuf};
use serde::Serialize;
use base64::Engine;
use crate::i18n::{tr, trf, Key};

/// Files managed by the app — a skin may read but never write/delete them.
const PROTECTED: [&str; 4] = [
    "skin.json",
    "settings.json",
    "settings.json.bak",
    "settings.json.tmp",
];

pub const MAX_READ_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_WRITE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

fn is_protected(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| PROTECTED.iter().any(|p| p.eq_ignore_ascii_case(n)))
        .unwrap_or(false)
}

/// Resolve `rel` inside `base` ("" / "." = the skin directory itself).
/// For writes the target may not exist yet: nothing is created here — the
/// deepest EXISTING ancestor is canonicalized and containment-checked (a
/// symlinked component escapes at that point), and write_file creates the
/// missing parent directories only after the check passes.  Creating first
/// and checking later would let a sandboxed symlink plant empty directories
/// outside the skin folder.
pub fn resolve(base: &Path, rel: &str, for_write: bool, lang: &str) -> Result<PathBuf, String> {
    let canon_base = base
        .canonicalize()
        .map_err(|_| trf(lang, Key::InvalidPath, &[rel]))?;

    let rel = rel.trim();
    if rel.is_empty() || rel == "." {
        return Ok(canon_base);
    }

    let p = Path::new(rel);
    for c in p.components() {
        match c {
            Component::CurDir => {}
            // NTFS 备用数据流（file:stream，如 skin.json::$DATA）：流路径经
            // canonicalize 后 file_name 不再是 "skin.json"，会绕过下方的受保护
            // 文件名单匹配而改写 manifest —— 分量含冒号一律拒绝。
            Component::Normal(s) if s.to_str().map(|s| s.contains(':')).unwrap_or(true) => {
                return Err(trf(lang, Key::InvalidPath, &[rel]));
            }
            // Windows 文件 API 会剥掉分量尾部的点与空格（"settings.json."
            // 落盘成 "settings.json"）——尾点/尾空格让受保护名单的
            // file_name 匹配失效（目标尚不存在、走下方结构路径分支时），
            // 一律拒绝；Linux 下同名拒绝无害（跨平台行为一致）
            Component::Normal(s)
                if s.to_str()
                    .map(|s| s.ends_with('.') || s.ends_with(' '))
                    .unwrap_or(true) =>
            {
                return Err(trf(lang, Key::InvalidPath, &[rel]));
            }
            Component::Normal(_) => {}
            // Prefix (C:), RootDir (\), ParentDir (..) — all escapes
            _ => return Err(trf(lang, Key::InvalidPath, &[rel])),
        }
    }
    let joined = base.join(p);

    // canonicalize needs the path to exist.  A write target may not exist
    // yet: walk UP to the deepest existing ancestor, canonicalize THAT and
    // apply the containment check to it — a symlinked component inside the
    // sandbox escapes at this point, BEFORE anything is created on disk.
    // The returned path then stays the un-canonicalized join (structurally
    // under base: rel was already stripped of prefix/root/`..`), and
    // write_file creates the missing parents.
    let canon = match joined.canonicalize() {
        Ok(c) => c,
        Err(_) if for_write => {
            let mut ancestor = joined.parent();
            let canon_ancestor = loop {
                let a = ancestor.ok_or_else(|| trf(lang, Key::InvalidPath, &[rel]))?;
                match a.canonicalize() {
                    Ok(c) => break c,
                    Err(_) => ancestor = a.parent(),
                }
            };
            // base 本身已存在且已 canonicalize，上溯必然到此为止
            if canon_ancestor != canon_base && !canon_ancestor.starts_with(&canon_base) {
                return Err(trf(lang, Key::InvalidPath, &[rel]));
            }
            return Ok(joined);
        }
        Err(_) => return Err(trf(lang, Key::InvalidPath, &[rel])),
    };
    if canon != canon_base && !canon.starts_with(&canon_base) {
        return Err(trf(lang, Key::InvalidPath, &[rel]));
    }
    Ok(canon)
}

/// Resolve a write/delete target and reject app-managed files.
fn resolve_protected(base: &Path, rel: &str, for_write: bool, lang: &str) -> Result<PathBuf, String> {
    let p = resolve(base, rel, for_write, lang)?;
    if is_protected(&p) {
        // 受保护文件只认皮肤根目录直下（skin.json、settings.json*）；子目录
        // 里的同名文件是皮肤自己的数据，放行。父目录统一 canonicalize 再比
        // ——p 对已有文件是 canon 路径、对新文件是 base.join 的结构路径
        let canon_base = base
            .canonicalize()
            .map_err(|_| trf(lang, Key::InvalidPath, &[rel]))?;
        let under_root = p
            .parent()
            .and_then(|par| par.canonicalize().ok())
            .map(|c| c == canon_base)
            .unwrap_or(false);
        if under_root {
            return Err(trf(lang, Key::ProtectedFile, &[rel]));
        }
    }
    Ok(p)
}

pub fn read_file(base: &Path, rel: &str, binary: bool, lang: &str) -> Result<String, String> {
    let p = resolve(base, rel, false, lang)?;
    let meta = std::fs::metadata(&p).map_err(|_| trf(lang, Key::InvalidPath, &[rel]))?;
    // 非普通文件（目录等）与超大文件分开报错：前者是路径不对，后者才是超限
    if !meta.is_file() {
        return Err(trf(lang, Key::InvalidPath, &[rel]));
    }
    if meta.len() > MAX_READ_BYTES {
        return Err(tr(lang, Key::FileTooLarge).to_string());
    }
    let bytes = std::fs::read(&p).map_err(|_| trf(lang, Key::InvalidPath, &[rel]))?;
    if binary {
        Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
    } else {
        String::from_utf8(bytes).map_err(|_| tr(lang, Key::NotTextFile).to_string())
    }
}

pub fn write_file(base: &Path, rel: &str, data: &str, binary: bool, lang: &str) -> Result<(), String> {
    let bytes = if binary {
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| tr(lang, Key::InvalidBase64).to_string())?
    } else {
        data.as_bytes().to_vec()
    };
    if bytes.len() > MAX_WRITE_BYTES {
        return Err(tr(lang, Key::FileTooLarge).to_string());
    }
    let p = resolve_protected(base, rel, true, lang)?;
    // resolve 只校验不建目录（先建后查会顺着沙箱内的符号链接把空目录建到
    // 沙箱外）；包含性校验通过后，缺失的父目录在这里建。
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|_| trf(lang, Key::InvalidPath, &[rel]))?;
    }
    std::fs::write(&p, bytes).map_err(|_| trf(lang, Key::InvalidPath, &[rel]))?;
    // 登记自写：hotreload 不把皮肤自己的保存当成外部改动而触发热重载
    // （hotreload 模块仅 debug 构建存在，release 需同步 cfg 掉）
    #[cfg(debug_assertions)]
    crate::hotreload::note_self_write(&p);
    Ok(())
}

pub fn list_dir(base: &Path, rel: &str, lang: &str) -> Result<Vec<DirEntry>, String> {
    let p = resolve(base, rel, false, lang)?;
    let mut out = Vec::new();
    let rd = std::fs::read_dir(&p).map_err(|_| trf(lang, Key::InvalidPath, &[rel]))?;
    for entry in rd.flatten() {
        // metadata 拿不到的项（并发删除/权限异常）跳过——编造 is_dir/size
        // 会把目录呈现成 0 字节文件
        let Some(meta) = entry.metadata().ok() else {
            continue;
        };
        out.push(DirEntry {
            name: entry.file_name().to_string_lossy().to_string(),
            is_dir: meta.is_dir(),
            size: meta.len(),
        });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Ok(out)
}

pub fn delete_file(base: &Path, rel: &str, lang: &str) -> Result<(), String> {
    let p = resolve_protected(base, rel, false, lang)?;
    if !p.is_file() {
        return Err(trf(lang, Key::InvalidPath, &[rel]));
    }
    std::fs::remove_file(&p).map_err(|_| trf(lang, Key::InvalidPath, &[rel]))?;
    // 登记自写：hotreload 不把皮肤自己的删除当成外部改动而触发热重载
    // （hotreload 模块仅 debug 构建存在，release 需同步 cfg 掉）
    #[cfg(debug_assertions)]
    crate::hotreload::note_self_write(&p);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_base(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("driftlet-fs-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn rejects_escapes_and_absolute_paths() {
        let base = temp_base("esc");
        assert!(resolve(&base, "..", false, "zh-CN").is_err());
        assert!(resolve(&base, "../other/x.txt", false, "zh-CN").is_err());
        assert!(resolve(&base, "a/../../b.txt", false, "zh-CN").is_err());
        assert!(resolve(&base, "/etc/passwd", false, "zh-CN").is_err());
        #[cfg(target_os = "windows")]
        assert!(resolve(&base, "C:\\Windows\\x.dll", false, "zh-CN").is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn roundtrip_and_subdirectory_creation() {
        let base = temp_base("rw");
        write_file(&base, "data/notes/todo.txt", "你好", false, "zh-CN").unwrap();
        assert_eq!(read_file(&base, "data/notes/todo.txt", false, "zh-CN").unwrap(), "你好");

        // binary roundtrip (0xFF/0xFE/0xFD is not valid UTF-8)
        write_file(&base, "bin.dat", "/v79", true, "zh-CN").unwrap();
        assert_eq!(read_file(&base, "bin.dat", true, "zh-CN").unwrap(), "/v79");
        assert!(read_file(&base, "bin.dat", false, "zh-CN").is_err());

        let entries = list_dir(&base, "data", "zh-CN").unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_dir && entries[0].name == "notes");

        delete_file(&base, "data/notes/todo.txt", "zh-CN").unwrap();
        assert!(read_file(&base, "data/notes/todo.txt", false, "zh-CN").is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn protected_files_cannot_be_written_or_deleted() {
        let base = temp_base("prot");
        std::fs::write(base.join("skin.json"), "{}").unwrap();
        assert!(write_file(&base, "skin.json", "x", false, "zh-CN").is_err());
        assert!(write_file(&base, "settings.json", "x", false, "zh-CN").is_err());
        assert!(write_file(&base, "settings.json.tmp", "x", false, "zh-CN").is_err());
        assert!(delete_file(&base, "skin.json", "zh-CN").is_err());
        // 子目录里的同名文件是皮肤自己的数据，放行
        write_file(&base, "data/skin.json", "x", false, "zh-CN").unwrap();
        // ...but reading is fine
        assert_eq!(read_file(&base, "skin.json", false, "zh-CN").unwrap(), "{}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn rejects_trailing_dot_and_space_components() {
        let base = temp_base("trail");
        std::fs::write(base.join("skin.json"), "{}").unwrap();
        // Windows 文件 API 会剥掉分量尾部的点/空格：不拒的话 "skin.json."
        // 落盘成 skin.json，受保护名单的 file_name 匹配被绕过
        assert!(resolve(&base, "skin.json.", true, "zh-CN").is_err());
        assert!(resolve(&base, "sub/skin.json.", true, "zh-CN").is_err());
        assert!(resolve(&base, "sub /skin.json", true, "zh-CN").is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn root_listing_via_empty_and_dot() {
        let base = temp_base("root");
        std::fs::write(base.join("a.txt"), "1").unwrap();
        assert_eq!(list_dir(&base, "", "zh-CN").unwrap().len(), 1);
        assert_eq!(list_dir(&base, ".", "zh-CN").unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn rejects_ads_stream_paths() {
        let base = temp_base("ads");
        std::fs::write(base.join("skin.json"), "{}").unwrap();
        // 写/删受保护文件的 ADS 形式必须被拒（否则会改写 skin.json 主数据流）
        assert!(write_file(&base, "skin.json::$DATA", "x", false, "zh-CN").is_err());
        assert!(delete_file(&base, "skin.json::$DATA", "zh-CN").is_err());
        // 读/列路径同样不放行流语法
        assert!(resolve(&base, "a.txt:stream", false, "zh-CN").is_err());
        let _ = std::fs::remove_dir_all(&base);
    }
}
