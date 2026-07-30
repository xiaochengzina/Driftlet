//! Windows toast notifications (WinRT `Windows.UI.Notifications`).
//!
//! Unpackaged-app recipe — REQUIRED on Win10 (dev machine runs 19044):
//!  1. The process has an AppUserModelID (lib.rs calls
//!     `SetCurrentProcessExplicitAppUserModelID("Driftlet")` at startup);
//!  2. A Start Menu shortcut carrying the SAME AUMID must exist —
//!     `ensure_shortcut` creates it on first use.  Without the shortcut,
//!     `CreateToastNotifierWithId` succeeds but the toast never shows.
//!
//! Step 2 is the IShellLink + IPropertyStore(PKEY_AppUserModel_ID) dance
//! from Microsoft's DesktopToastsSample; the shortcut is created once at
//! %APPDATA%\Microsoft\Windows\Start Menu\Programs\Driftlet.lnk.

use windows::core::{HSTRING, PCWSTR};
use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

const AUMID: &str = "Driftlet";
const MAX_TITLE: usize = 64;
const MAX_BODY: usize = 256;

/// ensure_aumid_shortcut 是 COM + 文件 IO，且本身是幂等自检——进程内
/// 成功一次即可，后续调用直接跳过；失败不置位，下次调用重试。
static SHORTCUT_CHECKED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn show(title: &str, body: &str) -> Result<(), String> {
    if !SHORTCUT_CHECKED.load(std::sync::atomic::Ordering::Relaxed) {
        ensure_aumid_shortcut()?;
        SHORTCUT_CHECKED.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    let title = truncate(title, MAX_TITLE);
    let body = truncate(body, MAX_BODY);
    let xml = format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        xml_escape(&title),
        xml_escape(&body),
    );

    let doc = XmlDocument::new().map_err(|e| e.to_string())?;
    doc.LoadXml(&HSTRING::from(&xml)).map_err(|e| e.to_string())?;
    let toast = ToastNotification::CreateToastNotification(&doc).map_err(|e| e.to_string())?;
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))
        .map_err(|e| e.to_string())?;
    notifier.Show(&toast).map_err(|e| e.to_string())
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// ─── Start Menu shortcut with AUMID ───

fn shortcut_path() -> Option<std::path::PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        std::path::PathBuf::from(appdata)
            .join(r"Microsoft\Windows\Start Menu\Programs\Driftlet.lnk"),
    )
}

pub fn ensure_aumid_shortcut() -> Result<(), String> {
    let Some(lnk) = shortcut_path() else { return Ok(()) };
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // Cargo test binaries (target/**/deps/driftlet_lib-*.exe) must never own
    // the shortcut: they have no icon and would poison the taskbar/Start
    // Menu entry the real app maintains.  Skipping here means probes don't
    // clobber the app's registration.
    let is_test_binary = exe
        .file_stem()
        .map(|s| s.to_string_lossy().starts_with("driftlet_lib"))
        .unwrap_or(false);
    if is_test_binary {
        return Ok(());
    }
    if lnk.exists() {
        // Only skip when the shortcut still points at THIS exe.  A stale
        // target (dev/test binary, uninstalled path) gets the shortcut
        // rewritten — it would otherwise rot in the Start Menu forever
        // (the AUMID property survives either way, but the entry itself
        // would launch a dead path).
        match shortcut_target(&lnk) {
            Ok(target) if target == exe => return Ok(()),
            Ok(_) | Err(_) => {} // fall through and rewrite
        }
    }
    create_shortcut(&exe, &lnk)
}

/// Read the shortcut's target path (IPersistFile::Load + IShellLink::GetPath).
#[cfg(target_os = "windows")]
fn shortcut_target(lnk: &std::path::Path) -> Result<std::path::PathBuf, String> {
    use windows::core::Interface;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_ALL, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    let lnk_w: Vec<u16> = {
        use std::os::windows::ffi::OsStrExt;
        lnk.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    };
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED); // see create_shortcut: never uninit
        let link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        let persist: IPersistFile = link.cast().map_err(|e| e.to_string())?;
        persist
            .Load(
                windows::core::PCWSTR(lnk_w.as_ptr()),
                windows::Win32::System::Com::STGM_READ,
            )
            .map_err(|e| e.to_string())?;
        let mut buf = [0u16; 1024];
        let mut find_data = windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW::default();
        link.GetPath(&mut buf, &mut find_data, 0)
            .map_err(|e| e.to_string())?;
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Ok(std::path::PathBuf::from(
            String::from_utf16_lossy(&buf[..end]),
        ))
    }
}

#[cfg(target_os = "windows")]
fn create_shortcut(exe: &std::path::Path, lnk: &std::path::Path) -> Result<(), String> {
    use windows::core::Interface;
    use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
    use windows::Win32::System::Com::StructuredStorage::{
        PROPVARIANT, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_ALL, COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Variant::VT_LPWSTR;
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    let to_wide = |s: &std::ffi::OsStr| -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        s.encode_wide().chain(std::iter::once(0)).collect()
    };
    let exe_w = to_wide(exe.as_os_str());
    let lnk_w = to_wide(lnk.as_os_str());
    // Deliberately leaked: the shell-link property bag may lazily READ the
    // string via our pointer after SetValue/Commit return (freeing it would
    // be a use-after-free — seen as heap corruption at teardown).  20 bytes
    // once per process is a fair price for certainty.
    let aumid_w: &'static [u16] =
        AUMID.encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>().leak();

    unsafe {
        // CoInitializeEx is required for CoCreateInstance; we deliberately
        // NEVER CoUninitialize — propsys' inproc shell-link handler does not
        // survive MTA apartment teardown (heap corruption on uninit), and
        // one leaked apartment per process is harmless.  S_FALSE /
        // RPC_E_CHANGED_MODE are both fine here.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        link.SetPath(PCWSTR(exe_w.as_ptr()))
            .map_err(|e| e.to_string())?;
        // 显式图标：任务栏按 AUMID 匹配到本快捷方式时用它（而不是隐式
        // 解析目标 exe 的图标——目标可能是无图标二进制的场景下保险）。
        link.SetIconLocation(PCWSTR(exe_w.as_ptr()), 0)
            .map_err(|e| e.to_string())?;

        // Stamp the AUMID property — this is what registers the ID for toasts.
        let store: IPropertyStore = link.cast().map_err(|e| e.to_string())?;
        let mut pv = PROPVARIANT::default();
        pv.Anonymous.Anonymous = std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
            vt: VT_LPWSTR,
            wReserved1: 0,
            wReserved2: 0,
            wReserved3: 0,
            Anonymous: PROPVARIANT_0_0_0 {
                pwszVal: windows::core::PWSTR(aumid_w.as_ptr() as *mut u16),
            },
        });
        store
            .SetValue(&PKEY_AppUserModel_ID, &pv)
            .map_err(|e| e.to_string())?;
        store.Commit().map_err(|e| e.to_string())?;

        let persist: IPersistFile = link.cast().map_err(|e| e.to_string())?;
        persist
            .Save(PCWSTR(lnk_w.as_ptr()), true)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_escapes_and_truncates() {
        assert_eq!(xml_escape("<a&\"b\">"), "&lt;a&amp;&quot;b&quot;&gt;");
        assert_eq!(truncate("abcdef", 3), "abc");
    }

    /// Shows a REAL toast on this machine — watch the screen.
    #[test]
    #[ignore = "hardware probe — shows a real notification"]
    fn probe_show_toast() {
        show("Driftlet 通知测试", "如果你在屏幕上看到这条通知，接口工作正常。").unwrap();
    }}
