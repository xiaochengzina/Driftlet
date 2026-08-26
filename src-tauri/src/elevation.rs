//! 提权启动的提醒（Windows；不自动降级——全部降级机械已移除）。
//!
//! Driftlet 的全部能力（音量/媒体/注册表读/run_command 普通权限/电源五条/
//! WebView2/托盘）都不需要管理员——清单也无 requestedExecutionLevel
//! （asInvoker）。提权只会发生在用户从已提权的终端/启动器拉起本程序时
//! （子进程继承父进程完整性级别），或账户本身一切进程天然提权（内置
//! Administrator / UAC 关闭）。
//!
//! 历史上的自动降级方案已全部实测并废弃，**勿改回**：
//! - 任务计划代起（schtasks /create + /run）：程序化建任务代起会触发
//!   安全软件行为检测（Behavior:Win32/Execution.A!ml，真机实测命中）；
//!   对真 Administrator 还原理上无效（交互令牌本身提权）；
//! - SAFER 受限令牌 + CreateProcessWithTokenW / CreateProcessAsUserW：
//!   使用令牌需要 SeImpersonatePrivilege / SeAssignPrimaryTokenPrivilege
//!   ——标准用户被 UAC 提权拉起时令牌里这些管理员特权根本不存在
//!   （真机实测 PRIVILEGE_NOT_HELD / ACCESS_DENIED 双路全挂）；
//! - explorer COM 链（GetItemObject 拿不到 IShellFolderViewDual）、
//!   `runas /trustlevel:0x40000`（密码提示走 WriteConsole、无控制台即死）。
//!
//! 现行为 = **检测到提权即提醒一次**（MessageBoxW，此刻窗口系统尚未建立）：
//! 「是」= 把 `allow_elevated` 写进 config.json 持久放行并继续（以后不再
//! 提示）；「否」= 退出。`DRIFTLET_ALLOW_ELEVATED=1` 与该标记同样跳过提醒。
//! debug 构建默认不提醒（dev loop 免打扰），`DRIFTLET_FORCE_DEMOTE=1` 可测。

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
use windows::Win32::Security::{
    AdjustTokenPrivileges, GetTokenInformation, LookupPrivilegeValueW, TokenElevation,
    LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_ELEVATION,
    TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// 启用本进程令牌上的一个特权（AdjustTokenPrivileges）。提权令牌里
/// SeShutdown 等默认「持有但禁用」，SetSuspendState / ExitWindowsEx 等
/// API 要求启用态（缺了报 ERROR_PRIVILEGE_NOT_HELD 1314——本函数就是为
/// 这个坑加的）。AdjustTokenPrivileges 返回 Ok 不代表特权真的启用成功
/// ——须查 GetLastError == ERROR_NOT_ALL_ASSIGNED 的「部分未分配」。
/// pub(crate)：skin_api::power 的睡眠/关机走这里启用 SE_SHUTDOWN_NAME。
pub(crate) fn enable_privilege(name: PCWSTR) -> Result<(), String> {
    use windows::Win32::Foundation::{ERROR_NOT_ALL_ASSIGNED, LUID};
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
        .map_err(|e| e.to_string())?;
        let result = (|| {
            let mut luid = LUID::default();
            LookupPrivilegeValueW(PCWSTR::null(), name, &mut luid)
                .map_err(|e| e.to_string())?;
            let tp = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: luid,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };
            AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None)
                .map_err(|e| e.to_string())?;
            if GetLastError() == ERROR_NOT_ALL_ASSIGNED {
                return Err("privilege not held by this token".to_string());
            }
            Ok(())
        })();
        let _ = CloseHandle(token);
        result
    }
}

/// 当前进程是否提权（TokenElevation，而非「用户是不是管理员」——UAC 下
/// 管理员的普通启动 TokenIsElevated=0，不触发提醒）。
fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false; // 查不到按不提权处理
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut ret_len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

/// 是否应该弹出提权提醒。三道闸：
/// - `DRIFTLET_ALLOW_ELEVATED`：明确放行（测试/用户有意提权运行）；
/// - config.json 的 `allow_elevated`：用户在提醒框选「继续」后写入的
///   持久放行标记；
/// - debug 构建默认不提醒（dev loop 免打扰）；要在 debug 下看本路径设
///   `DRIFTLET_FORCE_DEMOTE=1`（沿用旧环境变量名，不另起新名）。
fn should_warn() -> bool {
    if std::env::var_os("DRIFTLET_ALLOW_ELEVATED").is_some() {
        return false;
    }
    if persisted_allow_elevated() {
        return false;
    }
    if cfg!(debug_assertions) && std::env::var_os("DRIFTLET_FORCE_DEMOTE").is_none() {
        return false;
    }
    is_elevated()
}

/// config.json 里的「允许提权运行」持久放行标记。此时 AppState 尚未建立，
/// 直接读文件（与 early_language 同一约定：便携布局 <exe>/config）。
fn config_json_path() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join("config").join("config.json")))
}

fn persisted_allow_elevated() -> bool {
    config_json_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|v| v.get("allow_elevated")?.as_bool())
        .unwrap_or(false)
}

/// 把 allow_elevated 写进 config.json。打底必须是**完整形状的 AppConfig**：
/// 文件缺失（首装从未落盘）时若写极简 JSON（{"allow_elevated": true}），
/// load_config 会因 version/loaded_skins/skin_settings 三个无默认值必填
/// 字段缺失而判「损坏重置」——标记随 .bak 一起被丢，下次启动照弹
///（真机实测：选「是」后每次启动都弹窗 + 日志 Config corrupt backed up）。
fn persist_allow_elevated() {
    let Some(path) = config_json_path() else { return };
    let existing = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    let mut v = config_base_for_flag(existing);
    v["allow_elevated"] = serde_json::Value::Bool(true);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap_or_default());
}

/// persist 的打底选择（纯函数，测试钉住）：已有配置形状完整（必填三键
/// 在）才按原值保留，否则按 load_config 同款「损坏重置」语义用默认配置
/// 打底——绝不能写形状不全的极简 JSON。
fn config_base_for_flag(existing: Option<serde_json::Value>) -> serde_json::Value {
    existing
        .filter(|v| {
            v.get("version").is_some()
                && v.get("loaded_skins").is_some()
                && v.get("skin_settings").is_some()
        })
        .unwrap_or_else(|| {
            serde_json::to_value(crate::skin::types::AppConfig::default())
                .unwrap_or_else(|_| serde_json::json!({}))
        })
}

#[cfg(test)]
mod tests {
    use super::config_base_for_flag;

    /// 打底产物必须能被 AppConfig 解析（之前写极简 JSON 被 load_config 判
    /// 损坏重置、标记全丢的事故）——文件缺失、形状不全、形状完整三种形态
    #[test]
    fn config_base_is_always_full_shaped() {
        for (name, existing) in [
            ("缺失", None),
            ("极简 JSON（事故形态）", Some(serde_json::json!({"allow_elevated": true}))),
            ("形状不完整", Some(serde_json::json!({"version": 2}))),
            ("形状完整", Some(serde_json::json!({
                "version": 2, "loaded_skins": ["clock"], "skin_settings": {},
                "theme": "dark",
            }))),
        ] {
            let base = config_base_for_flag(existing.clone());
            let parsed: Result<crate::skin::types::AppConfig, _> = serde_json::from_value(base.clone());
            assert!(parsed.is_ok(), "{name}：打底必须能被 AppConfig 解析");
            // 形状完整的原值必须保留（只补标记，不重置用户配置）
            if name == "形状完整" {
                assert_eq!(base["theme"], "dark");
                assert_eq!(base["loaded_skins"], serde_json::json!(["clock"]));
            }
        }
    }
}

/// 启动提权提醒入口（run() 最前、Builder/single-instance 初始化之前调用）：
/// 检测到提权运行 → 弹原生消息框说明影响，「是」= 写 allow_elevated 持久
/// 放行并继续（以后不再提示）；「否」= 退出。不做任何降级尝试（任务计划 /
/// 令牌 API 全部实测不可靠或不可用，见模块头注）。
pub fn startup_elevation_notice() {
    if !should_warn() {
        return;
    }
    if show_elevated_notice() {
        log::warn!("running elevated by user choice (notified once)");
        return;
    }
    std::process::exit(1);
}

/// AppState 建立前的轻量语言判定：<exe>/config/config.json 的 language
/// 字段，读不到回退 OS UI 语言（与 AppConfig 首启默认同函数）。
fn early_language() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join("config").join("config.json")))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|v| v.get("language")?.as_str().map(str::to_string))
        .unwrap_or_else(crate::skin::types::default_language)
}

/// 提权提醒框（一次）：「是」= 写持久放行标记并继续；「否」= 退出。
/// 此刻窗口系统尚未建立，用 MessageBoxW（与 lib.rs 的 fatal_startup_error
/// 同一手法）。返回是否继续。
fn show_elevated_notice() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_ICONWARNING, MB_YESNO};
    let lang = early_language();
    let text = windows::core::HSTRING::from(crate::i18n::tr(&lang, crate::i18n::Key::ElevatedNotice));
    let yes = unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            windows::core::w!("Driftlet"),
            MB_YESNO | MB_ICONWARNING,
        ) == IDYES
    };
    if yes {
        persist_allow_elevated();
    }
    yes
}
