//! WebView2 运行时版本地板（默认皮肤的渲染基线）。
//!
//! 屿族官方皮肤的缩放模型整体建立在容器查询（container-type: size + cqw/cqh，
//! Chromium 105+）与 color-mix()（Chromium 111+）之上——旧运行时上这些声明
//! 整体失效：卡件布局塌陷、主题混色丢失（实机反馈「旧版本 WebView2 中差别
//! 很大」）。安装/更新端已由 bundle.windows.nsis.minimumWebview2Version 升旧
//! （bundler 模板逻辑：已装但低于地板时调 EdgeUpdate 在线升级）；本模块是
//! 运行侧兜底——启动时读注册表的运行时版本，低于地板即原生消息框告知并
//! 引导更新（每次启动都提醒：渲染是真坏，不是警告噪音；「是」顺手打开
//! 微软一键更新页）。

#[cfg(target_os = "windows")]
use crate::i18n::{Key, trf};

/// 运行时地板：Chromium 111（color-mix 基线；容器查询 105 已含在内）。
/// 与 tauri.conf.json 的 bundle.windows.nsis.minimumWebview2Version 同值——
/// 改地板必须两侧同步。
const FLOOR: [u64; 4] = [111, 0, 0, 0];

/// WebView2 常青运行时在 EdgeUpdate 下的客户端 GUID（安装器模板同值）。
const CLIENT_GUID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

/// 读注册表里的运行时版本（与安装器同一取值顺序：HKLM 64 位视图 → HKLM →
/// HKCU；Clients\\<guid> 的 pv 值）。读不到 = 未安装或不可判定——「未安装」由
/// 安装器兜底（缺失时引导下载），运行侧不误报。
#[cfg(target_os = "windows")]
fn installed_runtime_version() -> Option<[u64; 4]> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let tail = format!("Microsoft\\EdgeUpdate\\Clients\\{CLIENT_GUID}");
    let candidates = [
        (HKEY_LOCAL_MACHINE, format!("SOFTWARE\\WOW6432Node\\{tail}")),
        (HKEY_LOCAL_MACHINE, format!("SOFTWARE\\{tail}")),
        (HKEY_CURRENT_USER, format!("SOFTWARE\\{tail}")),
    ];
    for (hive, path) in candidates {
        if let Ok(ver) = RegKey::predef(hive)
            .open_subkey(&path)
            .and_then(|k| k.get_value::<String, _>("pv"))
        {
            if let Some(v) = parse_4seg(&ver) {
                return Some(v);
            }
        }
    }
    None
}

/// "138.0.3351.121" → [138, 0, 3351, 121]；缺段补 0，出现非数字段视为不可判定
/// （None → 不告警，宁漏勿误）。
fn parse_4seg(v: &str) -> Option<[u64; 4]> {
    let mut out = [0u64; 4];
    for (i, seg) in v.split('.').take(4).enumerate() {
        out[i] = seg.trim().parse().ok()?;
    }
    Some(out)
}

/// 版本比较 = 数组的字典序（主版本先行，天然正确）。
fn below_floor(v: [u64; 4]) -> bool {
    v < FLOOR
}

/// 启动时检查：运行时低于地板则原生消息框告知（此刻窗口系统尚未建立，与
/// elevation.rs 提权提醒同一手法）——「是」打开微软一键更新页后继续，
/// 「否」直接继续。每启动必提醒（渲染是真坏，提醒即修复入口）。
#[cfg(target_os = "windows")]
pub fn runtime_floor_notice() {
    use windows::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONWARNING, MB_YESNO, MessageBoxW};
    use windows::core::PCWSTR;

    let Some(v) = installed_runtime_version() else {
        return;
    };
    if !below_floor(v) {
        return;
    }
    let cur = v.map(|n| n.to_string()).join(".");
    let floor = FLOOR.map(|n| n.to_string()).join(".");
    let lang = crate::elevation::early_language();
    let text = windows::core::HSTRING::from(trf(&lang, Key::Webview2Outdated, &[&cur, &floor]));
    log::warn!("WebView2 runtime {cur} is below the rendering floor {floor} (user notified)");
    let yes = unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            windows::core::w!("Driftlet"),
            MB_YESNO | MB_ICONWARNING,
        ) == IDYES
    };
    if yes {
        // 微软官方常青一键更新（与安装器下载同一引导程序）
        let _ = crate::skin_api::open_target_impl(
            "https://go.microsoft.com/fwlink/p/?LinkId=2124703",
            &lang,
        );
    }
}

#[cfg(not(target_os = "windows"))]
pub fn runtime_floor_notice() {}

#[cfg(test)]
mod tests {
    use super::{below_floor, parse_4seg};

    #[test]
    fn parses_runtime_versions() {
        assert_eq!(parse_4seg("138.0.3351.121"), Some([138, 0, 3351, 121]));
        assert_eq!(parse_4seg("111.0.0.0"), Some([111, 0, 0, 0]));
        assert_eq!(parse_4seg("120.1"), Some([120, 1, 0, 0])); // 缺段补 0
        assert_eq!(parse_4seg("abc"), None); // 非数字 = 不可判定
        assert_eq!(parse_4seg(""), None);
    }

    #[test]
    fn floor_comparison_is_numeric() {
        assert!(below_floor([110, 9, 9, 9])); // 主版本低
        assert!(!below_floor([111, 0, 0, 0])); // 等于地板放行
        assert!(!below_floor([138, 0, 3351, 121])); // 高于地板放行
        assert!(!below_floor([1000, 0, 0, 0])); // 字典序不得按字符串比（"1000" < "111"）
    }
}
