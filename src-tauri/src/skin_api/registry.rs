//! Read-only registry access (the `registry` permission, Windows only).

use super::RegistryValue;
use crate::i18n::{trf, Key};

#[cfg(target_os = "windows")]
pub fn read(root: &str, path: &str, name: &str, lang: &str) -> Result<RegistryValue, String> {
    use base64::Engine;
    use winreg::enums::*;
    use winreg::RegKey;

    let hive = match root.to_ascii_uppercase().as_str() {
        "HKCU" | "HKEY_CURRENT_USER" => HKEY_CURRENT_USER,
        "HKLM" | "HKEY_LOCAL_MACHINE" => HKEY_LOCAL_MACHINE,
        "HKCR" | "HKEY_CLASSES_ROOT" => HKEY_CLASSES_ROOT,
        "HKU" | "HKEY_USERS" => HKEY_USERS,
        _ => return Err(trf(lang, Key::RegistryRootInvalid, &[root])),
    };
    let key = RegKey::predef(hive)
        .open_subkey(path)
        .map_err(|e| trf(lang, Key::RegistryReadFailed, &[&e.to_string()]))?;
    let raw = key
        .get_raw_value(name)
        .map_err(|e| trf(lang, Key::RegistryReadFailed, &[&e.to_string()]))?;

    let (kind, value) = match raw.vtype {
        REG_SZ => ("string", serde_json::json!(utf16z(&raw.bytes))),
        REG_EXPAND_SZ => ("expand_string", serde_json::json!(utf16z(&raw.bytes))),
        REG_MULTI_SZ => (
            "multi_string",
            serde_json::json!(
                utf16z(&raw.bytes)
                    .split('\0')
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            ),
        ),
        REG_DWORD if raw.bytes.len() == 4 => (
            "dword",
            serde_json::json!(u32::from_le_bytes(raw.bytes[..4].try_into().unwrap())),
        ),
        REG_QWORD if raw.bytes.len() == 8 => (
            "qword",
            serde_json::json!(u64::from_le_bytes(raw.bytes[..8].try_into().unwrap())),
        ),
        // REG_BINARY and everything else: hand back raw bytes as base64.
        _ => (
            "binary",
            serde_json::json!(base64::engine::general_purpose::STANDARD.encode(&raw.bytes)),
        ),
    };

    Ok(RegistryValue {
        kind: kind.to_string(),
        value,
    })
}

/// UTF-16 LE registry string bytes → String, trailing NUL stripped.
#[cfg(target_os = "windows")]
fn utf16z(bytes: &[u8]) -> String {
    let wide: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&wide)
        .trim_end_matches('\0')
        .to_string()
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn reads_a_string_value() {
        // Present on every Windows install, readable without elevation.
        let v = read(
            "HKLM",
            "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
            "ProductName",
            "zh-CN",
        )
        .unwrap();
        assert_eq!(v.kind, "string");
        assert!(v.value.as_str().unwrap().contains("Windows"));
    }

    #[test]
    fn rejects_bad_root_and_missing_value() {
        assert!(read("HKXX", "SOFTWARE", "x", "zh-CN").is_err());
        assert!(read("HKCU", "No\\Such\\Path\\Driftlet", "x", "zh-CN").is_err());
    }
}
