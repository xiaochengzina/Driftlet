//! UI language dictionaries for backend-produced, user-facing strings
//! (tray menu, skin context menu, command error messages).
//!
//! Two languages: zh-CN (default) and en.  `AppState.language` holds the
//! current choice; commands read it and pass `&str` down to helpers.  Any
//! missing/unknown language falls back to Chinese.

/// Fallback language when the configured value is missing or unknown.
pub const DEFAULT_LANG: &str = "zh-CN";

/// Normalize a configured language tag to a supported one.
/// Anything other than "en" resolves to the zh-CN default.
pub fn normalize(lang: &str) -> &'static str {
    match lang {
        "en" => "en",
        _ => DEFAULT_LANG,
    }
}

/// Message keys for every user-facing backend string.
/// Templates may contain `{}` placeholders, substituted in order by `trf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    // Tray menu + tooltip
    TrayShowManager,
    TrayReloadAll,
    TrayToggleSkins,
    TrayQuit,
    TrayTooltip,
    // hotkey.rs
    HotkeyInvalid,
    HotkeyRegisterFailed,
    // Skin window right-click menu
    MenuOpenConfig,
    MenuReload,
    MenuUnload,
    // commands.rs
    InvalidResizeDirection,
    InvalidPlacement,
    // Only used in the non-Windows fallback arm.
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    ResizeWindowsOnly,
    PreviewNeedsLoadedSkin,
    SkinNotFound,
    // Only used in the non-Windows fallback arm.
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    PreviewWindowsOnly,
    SkinAlreadyLoaded,
    ConfigSaveFailed,
    TaskFailed,
    SkinHasNoSetting,
    SettingNeedsBool,
    SettingNeedsNumber,
    SettingNeedsWhat,
    WhatText,
    WhatTime,
    WhatDate,
    WhatColor,
    WhatOption,
    WhatFont,
    SettingNeedsTime,
    SettingNeedsDate,
    SettingNeedsColor,
    SettingValueNotAllowed,
    SettingNeedsArray,
    SettingNeedsStringArray,
    SettingNeedsTimeRange,
    SettingMissingStart,
    SettingMissingEnd,
    SettingNeedsDateTime,
    EntryNeedsObject,
    EntryMissingTime,
    EntryMissingText,
    EntryTimeFormat,
    InvalidWeekday,
    SkinNotLoaded,
    DialogError,
    DskinFilterName,
    PickFilterName,
    NotASkinWindow,
    ManagerOnly,
    LogWindowOnly,
    UnloadBeforeRemove,
    RemoveSkinFailed,
    SkinFolderNotFound,
    OpenFailed,
    // skin/loader.rs
    InvalidSkinId,
    // skin/package.rs
    ReplaceOldDirFailed,
    InstallSkinFailed,
    RestoreSettingsFailed,
    OpenPackageFailed,
    PackageTooLarge,
    NotValidZip,
    TooManyFiles,
    CreateTempDirFailed,
    ReadPackageFailed,
    ExtractedTooLarge,
    NoSkinJsonInPackage,
    ReadSkinJsonFailed,
    SkinJsonParseFailed,
    PackageMissingId,
    EntryFileMissing,
    // backup.rs
    BackupFilterName,
    BackupNotZip,
    BackupTooLarge,
    BackupTooManyFiles,
    BackupExtractedTooLarge,
    InvalidBackup,
    BackupFormatUnsupported,
    ReadBackupFailed,
    ExportBackupFailed,
    ImportBackupFailed,
    ImportPartialUnloaded,
    // capture.rs
    WritePreviewFailed,
    WebViewNotReady,
    CapturePreviewCallFailed,
    AccessWebViewFailed,
    CaptureTimeout,
    // skin_api
    PermissionDenied,
    InvalidPath,
    FileTooLarge,
    ProtectedFile,
    NotTextFile,
    InvalidBase64,
    RegistryReadFailed,
    RegistryRootInvalid,
    CommandTimeout,
    CommandSpawnFailed,
    AudioUnavailable,
    VolumeFailed,
    MediaControlFailed,
    InvalidMediaAction,
    ClipboardFailed,
    InvalidTarget,
    NotificationFailed,
    // Only used in the non-Windows fallback arms.
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    WindowsOnly,
}

/// Look up `key` in the dictionary for `lang` (unknown → zh-CN).
pub fn tr(lang: &str, key: Key) -> &'static str {
    match normalize(lang) {
        "en" => en(key),
        _ => zh(key),
    }
}

/// Translate `key` and substitute `{}` placeholders with `args`, in order.
/// 单次扫描：遇到 `{}` 消费下一个参数，参数值本身含 `{}` 也不会被二次
/// 替换（旧的顺序 replacen 会把已插入参数里的 `{}` 当占位符再替换，错序）。
pub fn trf(lang: &str, key: Key, args: &[&str]) -> String {
    let template = tr(lang, key);
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    for arg in args {
        match rest.find("{}") {
            Some(pos) => {
                out.push_str(&rest[..pos]);
                out.push_str(arg);
                rest = &rest[pos + 2..];
            }
            None => break, // 占位符用完：多余参数忽略（同旧行为）
        }
    }
    out.push_str(rest);
    out
}

fn zh(key: Key) -> &'static str {
    match key {
        Key::TrayShowManager => "打开桌面皮肤管理器",
        Key::TrayReloadAll => "重新加载所有皮肤",
        Key::TrayToggleSkins => "隐藏已加载的皮肤",
        Key::TrayQuit => "退出",
        Key::TrayTooltip => "桌面皮肤管理器",
        Key::HotkeyInvalid => "无效的快捷键：需要修饰键 + 普通键（如 Ctrl+Alt+D）",
        Key::HotkeyRegisterFailed => "快捷键注册失败：{}（可能被其他程序占用）",
        Key::MenuOpenConfig => "打开皮肤配置",
        Key::MenuReload => "刷新皮肤",
        Key::MenuUnload => "卸载皮肤",
        Key::InvalidResizeDirection => "无效的缩放方向: {}",
        Key::InvalidPlacement => "未知放置模式: {}",
        Key::ResizeWindowsOnly => "边框缩放仅支持 Windows",
        Key::PreviewNeedsLoadedSkin => "皮肤未加载 — 请先加载皮肤再截取预览",
        Key::SkinNotFound => "皮肤 '{}' 未找到",
        Key::PreviewWindowsOnly => "预览截取仅支持 Windows",
        Key::SkinAlreadyLoaded => "皮肤已加载",
        Key::ConfigSaveFailed => "配置保存失败: {}",
        Key::TaskFailed => "任务失败: {}",
        Key::SkinHasNoSetting => "皮肤 '{}' 没有设置项 '{}'",
        Key::SettingNeedsBool => "设置项 '{}' 需要布尔值",
        Key::SettingNeedsNumber => "设置项 '{}' 需要数字",
        Key::SettingNeedsWhat => "设置项 '{}' 需要{}",
        Key::WhatText => "文本",
        Key::WhatTime => "时间",
        Key::WhatDate => "日期",
        Key::WhatColor => "颜色值",
        Key::WhatOption => "选项值",
        Key::WhatFont => "字体名",
        Key::SettingNeedsTime => "设置项 '{}' 需要 HH:MM 或 HH:MM:SS 时间",
        Key::SettingNeedsDate => "设置项 '{}' 需要 YYYY-MM-DD 日期",
        Key::SettingNeedsColor => "设置项 '{}' 需要 #rrggbb 或 #rrggbbaa 颜色值",
        Key::SettingValueNotAllowed => "设置项 '{}' 的取值 '{}' 不在可选项内",
        Key::SettingNeedsArray => "设置项 '{}' 需要数组",
        Key::SettingNeedsStringArray => "设置项 '{}' 需要字符串数组",
        Key::SettingNeedsTimeRange => "设置项 '{}' 需要时间范围对象",
        Key::SettingMissingStart => "设置项 '{}' 缺少 start",
        Key::SettingMissingEnd => "设置项 '{}' 缺少 end",
        Key::SettingNeedsDateTime => "设置项 '{}' 需要 YYYY-MM-DD HH:MM:SS 格式",
        Key::EntryNeedsObject => "设置项 '{}' 的条目需要对象",
        Key::EntryMissingTime => "设置项 '{}' 的条目缺少 time",
        Key::EntryMissingText => "设置项 '{}' 的条目缺少 text",
        Key::EntryTimeFormat => "设置项 '{}' 的条目时间需要 YYYY-MM-DD HH:MM:SS 格式",
        Key::InvalidWeekday => "设置项 '{}' 的取值 '{}' 不是有效的星期",
        Key::SkinNotLoaded => "皮肤未加载",
        Key::DialogError => "对话框错误: {}",
        Key::DskinFilterName => "Driftlet 皮肤包",
        Key::PickFilterName => "允许的类型",
        Key::NotASkinWindow => "不是皮肤窗口",
        Key::ManagerOnly => "该命令仅允许管理器窗口调用",
        Key::LogWindowOnly => "该命令仅允许日志窗口调用",
        Key::UnloadBeforeRemove => "请先卸载皮肤再删除",
        Key::RemoveSkinFailed => "删除皮肤失败: {}",
        Key::SkinFolderNotFound => "皮肤文件夹 '{}' 未找到",
        Key::OpenFailed => "打开失败: {}",
        Key::InvalidSkinId => "皮肤 id '{}' 不合法：仅允许小写字母、数字、中划线，且必须以字母或数字开头",
        Key::ReplaceOldDirFailed => "无法替换旧版本皮肤目录: {}",
        Key::InstallSkinFailed => "安装皮肤失败: {}",
        Key::RestoreSettingsFailed => "无法恢复用户设置: {}",
        Key::OpenPackageFailed => "无法打开皮肤包: {}",
        Key::PackageTooLarge => "皮肤包过大（超过 64 MB）",
        Key::NotValidZip => "不是有效的皮肤包（无法作为 zip 打开）",
        Key::TooManyFiles => "皮肤包文件过多",
        Key::CreateTempDirFailed => "无法创建临时目录: {}",
        Key::ReadPackageFailed => "读取皮肤包失败: {}",
        Key::ExtractedTooLarge => "皮肤包解压后过大",
        Key::NoSkinJsonInPackage => "不是有效的皮肤包：找不到 skin.json",
        Key::ReadSkinJsonFailed => "无法读取 skin.json: {}",
        Key::SkinJsonParseFailed => "skin.json 解析失败: {}",
        Key::PackageMissingId => "不是有效的皮肤包：skin.json 缺少 id 字段",
        Key::EntryFileMissing => "不是有效的皮肤包：入口文件 '{}' 不存在",
        Key::BackupFilterName => "Driftlet 备份",
        Key::BackupNotZip => "不是有效的备份文件（无法作为 zip 打开）",
        Key::BackupTooLarge => "备份文件过大（超过 64 MB）",
        Key::BackupTooManyFiles => "备份文件条目过多",
        Key::BackupExtractedTooLarge => "备份解压后过大",
        Key::InvalidBackup => "不是有效的 Driftlet 备份（缺少 config/config.json）",
        Key::BackupFormatUnsupported => "备份由更新版本的 Driftlet 创建（格式 {}），无法导入",
        Key::ReadBackupFailed => "读取备份文件失败：{}",
        Key::ExportBackupFailed => "导出备份失败：{}",
        Key::ImportBackupFailed => "导入备份失败：{}",
        Key::ImportPartialUnloaded => "导入已中止；以下皮肤已被卸载，重新加载即可恢复：{}",
        Key::WritePreviewFailed => "写入预览图失败: {}",
        Key::WebViewNotReady => "WebView2 尚未就绪: {}",
        Key::CapturePreviewCallFailed => "CapturePreview 调用失败: {}",
        Key::AccessWebViewFailed => "无法访问 WebView: {}",
        Key::CaptureTimeout => "截图超时（10 秒无响应）",
        Key::PermissionDenied => "皮肤 '{}' 未声明权限 '{}'",
        Key::InvalidPath => "无效的路径 '{}'",
        Key::FileTooLarge => "文件超出大小限制",
        Key::ProtectedFile => "该文件由应用管理，禁止修改：{}",
        Key::NotTextFile => "不是文本文件，请用二进制模式读取",
        Key::InvalidBase64 => "无效的二进制数据（base64 解码失败）",
        Key::RegistryReadFailed => "注册表读取失败：{}",
        Key::RegistryRootInvalid => "无效的注册表根键 '{}'",
        Key::CommandTimeout => "命令执行超时（{} 秒）",
        Key::CommandSpawnFailed => "命令启动失败：{}",
        Key::AudioUnavailable => "音频捕获不可用：{}",
        Key::VolumeFailed => "音量操作失败：{}",
        Key::MediaControlFailed => "媒体控制失败：{}",
        Key::InvalidMediaAction => "无效的媒体动作 '{}'",
        Key::ClipboardFailed => "剪贴板操作失败：{}",
        Key::InvalidTarget => "不允许打开的链接或路径：{}",
        Key::NotificationFailed => "通知发送失败：{}",
        Key::WindowsOnly => "该功能仅支持 Windows",
    }
}

fn en(key: Key) -> &'static str {
    match key {
        Key::TrayShowManager => "Open Desktop Skin Manager",
        Key::TrayReloadAll => "Reload All Skins",
        Key::TrayToggleSkins => "Hide Loaded Skins",
        Key::TrayQuit => "Quit",
        Key::TrayTooltip => "Desktop Skin Manager",
        Key::HotkeyInvalid => "Invalid hotkey: it needs a modifier plus a regular key (e.g. Ctrl+Alt+D)",
        Key::HotkeyRegisterFailed => "Failed to register hotkey: {} (it may be taken by another app)",
        Key::MenuOpenConfig => "Open Skin Settings",
        Key::MenuReload => "Reload Skin",
        Key::MenuUnload => "Unload Skin",
        Key::InvalidResizeDirection => "Invalid resize direction: {}",
        Key::InvalidPlacement => "Unknown placement mode: {}",
        Key::ResizeWindowsOnly => "Border resize is only supported on Windows",
        Key::PreviewNeedsLoadedSkin => "Skin is not loaded — load it before capturing a preview",
        Key::SkinNotFound => "Skin '{}' not found",
        Key::PreviewWindowsOnly => "Preview capture is only supported on Windows",
        Key::SkinAlreadyLoaded => "Skin is already loaded",
        Key::ConfigSaveFailed => "Failed to save config: {}",
        Key::TaskFailed => "Task failed: {}",
        Key::SkinHasNoSetting => "Skin '{}' has no setting '{}'",
        Key::SettingNeedsBool => "Setting '{}' requires a boolean value",
        Key::SettingNeedsNumber => "Setting '{}' requires a number",
        Key::SettingNeedsWhat => "Setting '{}' requires {}",
        Key::WhatText => "a text value",
        Key::WhatTime => "a time value",
        Key::WhatDate => "a date value",
        Key::WhatColor => "a color value",
        Key::WhatOption => "an option value",
        Key::WhatFont => "a font name",
        Key::SettingNeedsTime => "Setting '{}' requires a time in HH:MM or HH:MM:SS format",
        Key::SettingNeedsDate => "Setting '{}' requires a date in YYYY-MM-DD format",
        Key::SettingNeedsColor => "Setting '{}' requires a color in #rrggbb or #rrggbbaa format",
        Key::SettingValueNotAllowed => "Setting '{}': value '{}' is not one of the allowed options",
        Key::SettingNeedsArray => "Setting '{}' requires an array",
        Key::SettingNeedsStringArray => "Setting '{}' requires an array of strings",
        Key::SettingNeedsTimeRange => "Setting '{}' requires a time-range object",
        Key::SettingMissingStart => "Setting '{}' is missing 'start'",
        Key::SettingMissingEnd => "Setting '{}' is missing 'end'",
        Key::SettingNeedsDateTime => "Setting '{}' requires YYYY-MM-DD HH:MM:SS format",
        Key::EntryNeedsObject => "Setting '{}': each entry must be an object",
        Key::EntryMissingTime => "Setting '{}': entry is missing 'time'",
        Key::EntryMissingText => "Setting '{}': entry is missing 'text'",
        Key::EntryTimeFormat => "Setting '{}': entry time must be in YYYY-MM-DD HH:MM:SS format",
        Key::InvalidWeekday => "Setting '{}': value '{}' is not a valid weekday",
        Key::SkinNotLoaded => "Skin is not loaded",
        Key::DialogError => "Dialog error: {}",
        Key::DskinFilterName => "Driftlet Skin Package",
        Key::PickFilterName => "Allowed types",
        Key::NotASkinWindow => "Not a skin window",
        Key::ManagerOnly => "This command can only be called from the manager window",
        Key::LogWindowOnly => "This command can only be called from the log window",
        Key::UnloadBeforeRemove => "Unload the skin before removing it",
        Key::RemoveSkinFailed => "Failed to remove skin: {}",
        Key::SkinFolderNotFound => "Skin folder '{}' not found",
        Key::OpenFailed => "Failed to open: {}",
        Key::InvalidSkinId => "Invalid skin id '{}': only lowercase letters, digits and dashes are allowed, and it must start with a letter or digit",
        Key::ReplaceOldDirFailed => "Failed to replace the existing skin directory: {}",
        Key::InstallSkinFailed => "Failed to install skin: {}",
        Key::RestoreSettingsFailed => "Failed to restore user settings: {}",
        Key::OpenPackageFailed => "Failed to open skin package: {}",
        Key::PackageTooLarge => "Skin package is too large (over 64 MB)",
        Key::NotValidZip => "Not a valid skin package (cannot be opened as a zip)",
        Key::TooManyFiles => "Skin package contains too many files",
        Key::CreateTempDirFailed => "Failed to create temp directory: {}",
        Key::ReadPackageFailed => "Failed to read skin package: {}",
        Key::ExtractedTooLarge => "Skin package is too large once extracted",
        Key::NoSkinJsonInPackage => "Not a valid skin package: skin.json not found",
        Key::ReadSkinJsonFailed => "Failed to read skin.json: {}",
        Key::SkinJsonParseFailed => "Failed to parse skin.json: {}",
        Key::PackageMissingId => "Not a valid skin package: skin.json is missing the id field",
        Key::EntryFileMissing => "Not a valid skin package: entry file '{}' does not exist",
        Key::BackupFilterName => "Driftlet Backup",
        Key::BackupNotZip => "Not a valid backup (cannot be opened as a zip)",
        Key::BackupTooLarge => "Backup file is too large (over 64 MB)",
        Key::BackupTooManyFiles => "Backup contains too many files",
        Key::BackupExtractedTooLarge => "Backup is too large once extracted",
        Key::InvalidBackup => "Not a valid Driftlet backup (missing config/config.json)",
        Key::BackupFormatUnsupported => "Backup was created by a newer Driftlet (format {}) and cannot be imported",
        Key::ReadBackupFailed => "Failed to read backup: {}",
        Key::ExportBackupFailed => "Failed to export backup: {}",
        Key::ImportBackupFailed => "Failed to import backup: {}",
        Key::ImportPartialUnloaded => "Import aborted; the following skins were unloaded — reload them to restore: {}",
        Key::WritePreviewFailed => "Failed to write preview image: {}",
        Key::WebViewNotReady => "WebView2 is not ready: {}",
        Key::CapturePreviewCallFailed => "CapturePreview call failed: {}",
        Key::AccessWebViewFailed => "Cannot access WebView: {}",
        Key::CaptureTimeout => "Capture timed out (no response for 10 seconds)",
        Key::PermissionDenied => "Skin '{}' has not declared the '{}' permission",
        Key::InvalidPath => "Invalid path '{}'",
        Key::FileTooLarge => "File exceeds the size limit",
        Key::ProtectedFile => "This file is managed by the app and cannot be modified: {}",
        Key::NotTextFile => "Not a text file; read it in binary mode",
        Key::InvalidBase64 => "Invalid binary data (base64 decode failed)",
        Key::RegistryReadFailed => "Failed to read registry: {}",
        Key::RegistryRootInvalid => "Invalid registry root '{}'",
        Key::CommandTimeout => "Command timed out ({} seconds)",
        Key::CommandSpawnFailed => "Failed to start command: {}",
        Key::AudioUnavailable => "Audio capture unavailable: {}",
        Key::VolumeFailed => "Volume operation failed: {}",
        Key::MediaControlFailed => "Media control failed: {}",
        Key::InvalidMediaAction => "Invalid media action '{}'",
        Key::ClipboardFailed => "Clipboard operation failed: {}",
        Key::InvalidTarget => "Opening this link or path is not allowed: {}",
        Key::NotificationFailed => "Failed to show notification: {}",
        Key::WindowsOnly => "This feature is only supported on Windows",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_language_falls_back_to_chinese() {
        assert_eq!(tr("fr", Key::TrayQuit), "退出");
        assert_eq!(tr("en", Key::TrayQuit), "Quit");
        assert_eq!(tr(DEFAULT_LANG, Key::TrayQuit), "退出");
    }

    #[test]
    fn placeholders_substitute_in_order() {
        assert_eq!(
            trf("en", Key::SkinNotFound, &["clock"]),
            "Skin 'clock' not found"
        );
        assert_eq!(
            trf("zh-CN", Key::SkinHasNoSetting, &["clock", "color"]),
            "皮肤 'clock' 没有设置项 'color'"
        );
        // Same arity in both languages
        assert_eq!(
            trf("en", Key::SettingValueNotAllowed, &["k", "v"]),
            "Setting 'k': value 'v' is not one of the allowed options"
        );
    }

    #[test]
    fn arg_containing_placeholder_is_not_rescanned() {
        // 参数值本身含 "{}"：单扫描实现按插入顺序消费占位符，
        // 已插入的 "{}" 不会被当成后续参数的占位符
        assert_eq!(
            trf("en", Key::SettingValueNotAllowed, &["{}", "v"]),
            "Setting '{}': value 'v' is not one of the allowed options"
        );
        // 多余参数忽略、占位符不足时原样保留（与旧行为一致）
        assert_eq!(
            trf("en", Key::SkinNotFound, &["a", "b"]),
            "Skin 'a' not found"
        );
        assert_eq!(
            trf("en", Key::SkinHasNoSetting, &["only-one"]),
            "Skin 'only-one' has no setting '{}'"
        );
    }

    #[test]
    fn placeholder_counts_match_between_languages() {
        fn count(s: &str) -> usize {
            s.matches("{}").count()
        }
        for key in [
            Key::InvalidResizeDirection,
            Key::InvalidPlacement,
            Key::HotkeyRegisterFailed,
            Key::SkinNotFound,
            Key::ConfigSaveFailed,
            Key::TaskFailed,
            Key::SkinHasNoSetting,
            Key::SettingNeedsBool,
            Key::SettingNeedsNumber,
            Key::SettingNeedsWhat,
            Key::SettingNeedsTime,
            Key::SettingNeedsDate,
            Key::SettingNeedsColor,
            Key::SettingValueNotAllowed,
            Key::SettingNeedsArray,
            Key::SettingNeedsStringArray,
            Key::SettingNeedsTimeRange,
            Key::SettingMissingStart,
            Key::SettingMissingEnd,
            Key::SettingNeedsDateTime,
            Key::EntryNeedsObject,
            Key::EntryMissingTime,
            Key::EntryMissingText,
            Key::EntryTimeFormat,
            Key::InvalidWeekday,
            Key::DialogError,
            Key::RemoveSkinFailed,
            Key::SkinFolderNotFound,
            Key::OpenFailed,
            Key::InvalidSkinId,
            Key::ReplaceOldDirFailed,
            Key::InstallSkinFailed,
            Key::RestoreSettingsFailed,
            Key::OpenPackageFailed,
            Key::CreateTempDirFailed,
            Key::ReadPackageFailed,
            Key::ReadSkinJsonFailed,
            Key::SkinJsonParseFailed,
            Key::EntryFileMissing,
            Key::WritePreviewFailed,
            Key::WebViewNotReady,
            Key::CapturePreviewCallFailed,
            Key::AccessWebViewFailed,
            Key::PermissionDenied,
            Key::InvalidPath,
            Key::ProtectedFile,
            Key::RegistryReadFailed,
            Key::RegistryRootInvalid,
            Key::CommandTimeout,
            Key::CommandSpawnFailed,
            Key::AudioUnavailable,
            Key::VolumeFailed,
            Key::MediaControlFailed,
            Key::InvalidMediaAction,
            Key::ClipboardFailed,
            Key::InvalidTarget,
            Key::NotificationFailed,
            Key::ManagerOnly,
            Key::BackupFormatUnsupported,
            Key::ReadBackupFailed,
            Key::ExportBackupFailed,
            Key::ImportBackupFailed,
            Key::ImportPartialUnloaded,
        ] {
            assert_eq!(
                count(zh(key)),
                count(en(key)),
                "placeholder count mismatch for {:?}",
                key
            );
        }
    }
}
