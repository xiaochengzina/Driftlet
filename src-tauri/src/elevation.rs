//! 提权启动的自动降级（Windows，任务计划交互令牌方案）。
//!
//! Driftlet 的全部能力（音量/媒体/注册表读/run_command 普通权限/电源五条/
//! WebView2/托盘）都不需要管理员——清单也无 requestedExecutionLevel
//! （asInvoker）。提权只会发生在用户从已提权的终端/启动器拉起本程序时
//! （子进程继承父进程完整性级别）。提权运行的害处是实打实的：皮肤窗口
//! 随之高完整性，run_command 的子进程全部提权（皮肤拿到的「普通权限」
//! 承诺失效），Explorer 拖 .dskin 进管理器还会被 UIPI 拦截。
//!
//! 方案 = **任务计划代起**：注册一次性任务（`/it` 交互令牌、不传 /ru
//! 免密码）→ `schtasks /run` 立即触发 → 父进程退出；子进程由任务计划
//! 服务从交互会话令牌创建，天然中完整性（本地探针实证：子进程
//! S-1-16-8192 Medium IL）。子进程启动时自清任务（`--demote-cleanup`
//! 参数携带任务名）。
//!
//! 已实测堵死的路线、勿改回：CreateProcessAsUserW 要的 SeAssignPrimaryToken
//! 默认只发给 SYSTEM/服务账户（提权管理员令牌根本没有）；CreateProcess
//! WithTokenW 只要 SeImpersonate 仍报 ERROR_ACCESS_DENIED(5)；explorer COM
//! 链（…→IShellDispatch2.ShellExecute）实测机 GetItemObject 拿不到
//! IShellFolderViewDual（E_NOINTERFACE，shdocvw 对象模型版本差异）；
//! `runas /trustlevel:0x40000` 在非提权环境实测即失败（exit 1——runas
//! 的密码提示走 WriteConsole 直写，无控制台时直接死，静默自动化不可用）。

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
use windows::Win32::Security::{
    AdjustTokenPrivileges, GetTokenInformation, LookupPrivilegeValueW, TokenElevation,
    LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_ELEVATION,
    TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// 降级代起用的一次性任务名（固定名：每次 /f 覆盖，不会堆积）。
const TASK_NAME: &str = "Driftlet-DeElevate";
/// 子进程命令行里的自清标记：值 = 要删除的任务名。
const CLEANUP_ARG_PREFIX: &str = "--demote-cleanup=";

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
/// 管理员的普通启动 TokenIsElevated=0，不触发降级）。
fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false; // 查不到按不提权处理：宁多跑不提权的，不误降级
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

/// 是否应该执行降级。两道闸：
/// - `DRIFTLET_ALLOW_ELEVATED`：明确放行（测试/用户有意提权运行）；
/// - debug 构建默认不降级——`npm run tauri dev` 若从提权终端起，降级会把
///   子进程从终端/构建链上撕下来，dev loop 直接断；要在 debug 下测本路径
///   设 `DRIFTLET_FORCE_DEMOTE=1`。
fn should_demote() -> bool {
    if std::env::var_os("DRIFTLET_ALLOW_ELEVATED").is_some() {
        return false;
    }
    if cfg!(debug_assertions) && std::env::var_os("DRIFTLET_FORCE_DEMOTE").is_none() {
        return false;
    }
    is_elevated()
}

/// CommandLineToArgvW 兼容的引号包裹：始终加双引号，内部 `"` 转义为 `\"`，
/// 紧邻引号的连续反斜杠翻倍（防 `\"` 被解析成字面引号、结尾 `\` 吃掉收尾
/// 引号）。
fn quote_arg(s: &std::ffi::OsStr) -> String {
    let s = s.to_string_lossy();
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    let mut backslashes = 0usize;
    for ch in s.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(ch);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

/// 经一次性任务计划代起普通权限副本（参数原样透传，.dskin 关联参数不丢）。
/// `/it`（交互令牌）+ 不传 `/ru` = 以当前交互用户运行、免密码；
/// `/st 00:00` 已过期仅触发一条无害警告，`/run` 强制立即执行。
/// 子进程经 `--demote-cleanup=` 拿到任务名，启动后自清（见
/// cleanup_handoff_task）。
fn relaunch_via_task() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // /tr 是「程序+参数」一整段；exe 与每个参数各自引号包裹
    let mut tr = quote_arg(exe.as_os_str());
    tr.push(' ');
    tr.push_str(&format!("{CLEANUP_ARG_PREFIX}{TASK_NAME}"));
    for a in std::env::args_os().skip(1) {
        tr.push(' ');
        tr.push_str(&quote_arg(&a));
    }

    let create = std::process::Command::new("schtasks.exe")
        .args([
            "/create", "/tn", TASK_NAME, "/tr", &tr,
            "/sc", "once", "/st", "00:00", "/it", "/f",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    if !create.status.success() {
        return Err(format!(
            "schtasks /create: {}", String::from_utf8_lossy(&create.stderr).trim()
        ));
    }
    let run = std::process::Command::new("schtasks.exe")
        .args(["/run", "/tn", TASK_NAME])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    if !run.status.success() {
        return Err(format!(
            "schtasks /run: {}", String::from_utf8_lossy(&run.stderr).trim()
        ));
    }
    Ok(())
}

/// 子进程侧：若由降级任务拉起（命令行含 `--demote-cleanup=<任务名>`），
/// 起后台线程删掉那个一次性任务。失败忽略（任务残留无害——同名任务
/// 下次 /f 覆盖）。该参数不被 dskin_arg 识别，不干扰包安装通道。
pub fn cleanup_handoff_task() {
    let Some(name) = std::env::args()
        .find_map(|a| a.strip_prefix(CLEANUP_ARG_PREFIX).map(str::to_string))
    else {
        return;
    };
    std::thread::spawn(move || {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let _ = std::process::Command::new("schtasks.exe")
            .args(["/delete", "/tn", &name, "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    });
}

/// 启动降级入口（run() 最前、Builder/single-instance 初始化之前调用）：
/// 提权运行 → 任务计划代起普通权限副本并**退出本进程**；**降级失败 =
/// 弹原生消息框提示 + 硬退出**（用户明确要求提权不可用——提权继续跑
/// 意味着皮肤与 run_command 子进程全部高完整性，不能用「继续运行」
/// 兜底；`DRIFTLET_ALLOW_ELEVATED=1` 仍是有意提权运行的放行闸）。
pub fn startup_demote() {
    cleanup_handoff_task();
    if !should_demote() {
        return;
    }
    match relaunch_via_task() {
        Ok(()) => {
            log::info!(
                "elevated launch detected — relaunched via scheduled task (interactive \
                 token, medium integrity), exiting the elevated process"
            );
            std::process::exit(0);
        }
        Err(e) => {
            log::error!("auto de-elevation failed ({}); refusing to run elevated", e);
            show_demote_failed(&e);
            std::process::exit(1);
        }
    }
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

/// 降级失败的原生提示框（此刻窗口系统尚未建立，用 MessageBoxW——与
/// lib.rs 的 fatal_startup_error 同一手法）。
fn show_demote_failed(detail: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};
    let lang = early_language();
    let text = windows::core::HSTRING::from(crate::i18n::trf(
        &lang,
        crate::i18n::Key::DemoteFailed,
        &[detail],
    ));
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            windows::core::w!("Driftlet"),
            MB_OK | MB_ICONWARNING,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::quote_arg;
    use std::ffi::OsStr;

    #[test]
    fn quote_arg_plain() {
        assert_eq!(quote_arg(OsStr::new("C:\\app\\driftlet.exe")), "\"C:\\app\\driftlet.exe\"");
    }

    #[test]
    fn quote_arg_with_space() {
        assert_eq!(
            quote_arg(OsStr::new("D:\\my skins\\a b.dskin")),
            "\"D:\\my skins\\a b.dskin\""
        );
    }

    #[test]
    fn quote_arg_escapes_quotes_and_trailing_backslashes() {
        // 内部引号：\"；引号前的反斜杠翻倍防吃掉引号
        assert_eq!(quote_arg(OsStr::new("a\"b")), "\"a\\\"b\"");
        assert_eq!(quote_arg(OsStr::new("C:\\dir\\")), "\"C:\\dir\\\\\"");
    }
}
