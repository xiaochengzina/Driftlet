//! Sandboxed file access for skins (the `files` permission).
//!
//! Every path a skin touches resolves inside its own install directory:
//! absolute paths and `..` components are rejected up front, then the final
//! path is canonicalized and must still sit under the canonicalized skin
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
/// For writes the target may not exist yet: its parent directories are
/// created first so canonicalization still works.
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
            Component::Normal(_) => {}
            // Prefix (C:), RootDir (\), ParentDir (..) — all escapes
            _ => return Err(trf(lang, Key::InvalidPath, &[rel])),
        }
    }
    let joined = base.join(p);

    // canonicalize needs the path to exist.  For a write target that does
    // not exist yet, canonicalize the (created) parent and re-attach the
    // file name — the escape check below still applies to the result.
    let canon = match joined.canonicalize() {
        Ok(c) => c,
        Err(_) if for_write => {
            let parent = joined
                .parent()
                .ok_or_else(|| trf(lang, Key::InvalidPath, &[rel]))?;
            std::fs::create_dir_all(parent)
                .map_err(|_| trf(lang, Key::InvalidPath, &[rel]))?;
            let name = joined
                .file_name()
                .ok_or_else(|| trf(lang, Key::InvalidPath, &[rel]))?;
            parent
                .canonicalize()
                .map_err(|_| trf(lang, Key::InvalidPath, &[rel]))?
                .join(name)
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
        return Err(trf(lang, Key::ProtectedFile, &[rel]));
    }
    Ok(p)
}

pub fn read_file(base: &Path, rel: &str, binary: bool, lang: &str) -> Result<String, String> {
    let p = resolve(base, rel, false, lang)?;
    let meta = std::fs::metadata(&p).map_err(|_| trf(lang, Key::InvalidPath, &[rel]))?;
    if !meta.is_file() || meta.len() > MAX_READ_BYTES {
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
            .map_err(|_| trf(lang, Key::InvalidPath, &["<base64>"]))?
    } else {
        data.as_bytes().to_vec()
    };
    if bytes.len() > MAX_WRITE_BYTES {
        return Err(tr(lang, Key::FileTooLarge).to_string());
    }
    let p = resolve_protected(base, rel, true, lang)?;
    std::fs::write(&p, bytes).map_err(|_| trf(lang, Key::InvalidPath, &[rel]))
}

pub fn list_dir(base: &Path, rel: &str, lang: &str) -> Result<Vec<DirEntry>, String> {
    let p = resolve(base, rel, false, lang)?;
    let mut out = Vec::new();
    let rd = std::fs::read_dir(&p).map_err(|_| trf(lang, Key::InvalidPath, &[rel]))?;
    for entry in rd.flatten() {
        let meta = entry.metadata().ok();
        out.push(DirEntry {
            name: entry.file_name().to_string_lossy().to_string(),
            is_dir: meta.as_ref().map(|m| m.is_dir()).unwrap_or(false),
            size: meta.map(|m| m.len()).unwrap_or(0),
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
    std::fs::remove_file(&p).map_err(|_| trf(lang, Key::InvalidPath, &[rel]))
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
        // ...but reading is fine
        assert_eq!(read_file(&base, "skin.json", false, "zh-CN").unwrap(), "{}");
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
