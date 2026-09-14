//! 电源与会话控制（Windows；`system` 权限）：锁屏 / 关显示器 / 睡眠 /
//! 关机·重启·注销 / 清空回收站。
//!
//! 这一组全部**无路径、无目标参数**——system 权限刻意没有「启动 exe」
//! 的通道（open_external 的可执行黑名单同理）：运行程序只属于 `shell`
//! 权限的 `run_command`，让用户对能力有正确预期。新增 system 命令时
//! 不得引入可执行目标参数（见 docs/关键机制.md）。

use windows::Win32::Foundation::{GetLastError, LPARAM, WPARAM};
use windows::Win32::System::Shutdown::{
    EWX_LOGOFF, EWX_REBOOT, EWX_SHUTDOWN, ExitWindowsEx, LockWorkStation,
};
use windows::Win32::UI::Shell::{SHEmptyRecycleBinW, SHQUERYRBINFO, SHQueryRecycleBinW};
use windows::Win32::UI::WindowsAndMessaging::{
    HWND_BROADCAST, PostMessageW, SC_MONITORPOWER, WM_SYSCOMMAND,
};
use windows::core::PCWSTR;

/// 关机 / 重启 / 注销（power_control 的 action 参数）。
#[derive(Debug, Clone, Copy)]
pub enum PowerAction {
    Shutdown,
    Restart,
    Logoff,
}

/// 睡眠/关机/注销要求调用进程持有 SE_SHUTDOWN_NAME（默认随令牌发放但处于
/// 禁用态，需显式启用）。实现收编在 crate::elevation（提权降级也要启用
/// 特权的同款需求——SeAssignPrimaryToken / SeIncreaseQuota），勿再抄一份。
fn enable_shutdown_privilege() -> Result<(), String> {
    crate::elevation::enable_privilege(windows::Win32::Security::SE_SHUTDOWN_NAME)
}

/// 锁定当前会话（等同 Win+L）。任何线程可调、立即返回。
pub fn lock() -> Result<(), String> {
    unsafe { LockWorkStation() }.map_err(|e| e.to_string())
}

/// 关闭显示器（SC_MONITORPOWER, lparam=2；任意输入即唤醒）。
/// 走 PostMessage 广播而非 SendMessage：广播会遇到挂死窗口，同步等待会被
/// 拖住（GetWindowTextW 阻塞同类教训——见 status.rs 的 SendMessageTimeoutW
/// 选择），投递语义下消息最终进各顶层窗口队列，资源管理器收到即灭屏。
pub fn monitor_off() -> Result<(), String> {
    unsafe {
        PostMessageW(
            Some(HWND_BROADCAST),
            WM_SYSCOMMAND,
            WPARAM(SC_MONITORPOWER as usize),
            LPARAM(2), // 2 = 关闭；-1 = 开，1 = 低功耗
        )
    }
    .map_err(|e| e.to_string())
}

/// 进入睡眠（SetSuspendState，不强制、不休眠）。机器禁用睡眠/休眠策略
/// 或令牌拿不到 SE_SHUTDOWN_NAME 时报错。
pub fn sleep() -> Result<(), String> {
    enable_shutdown_privilege()?;
    let ok = unsafe { windows::Win32::System::Power::SetSuspendState(false, false, false) };
    if ok {
        Ok(())
    } else {
        Err(format!("SetSuspendState failed: {:?}", unsafe {
            GetLastError()
        }))
    }
}

/// 关机 / 重启 / 注销（ExitWindowsEx，**不带 EWX_FORCE**——未保存数据的
/// 应用可阻止关机，用户会看到系统级「应用阻止关机」界面而不是静默丢数据；
/// 皮肤不应能绕过它）。
pub fn power(action: PowerAction) -> Result<(), String> {
    let flags = match action {
        PowerAction::Shutdown => EWX_SHUTDOWN,
        PowerAction::Restart => EWX_REBOOT,
        PowerAction::Logoff => EWX_LOGOFF, // 注销不需要 SE_SHUTDOWN_NAME，但启用无害
    };
    enable_shutdown_privilege()?;
    unsafe { ExitWindowsEx(flags, windows::Win32::System::Shutdown::SHUTDOWN_REASON(0)) }
        .map_err(|e| e.to_string())
}

/// 清空回收站（SHEmptyRecycleBinW，flags=0 = 与资源管理器一致的常规清空：
/// 系统确认框 + 进度 + 音效——破坏性操作把最终确认权留给用户，皮肤不能
/// 静默清空）。先 SHQueryRecycleBinW 查空：已空直接成功，不弹确认框。
pub fn empty_recycle_bin() -> Result<(), String> {
    unsafe {
        let mut info = SHQUERYRBINFO {
            cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
            ..Default::default()
        };
        SHQueryRecycleBinW(PCWSTR::null(), &mut info).map_err(|e| e.to_string())?;
        if info.i64NumItems == 0 {
            return Ok(());
        }
        // 确认框无属主（不传皮肤窗口 hwnd——它可能是壁纸层/穿透窗，
        // 模态对话框挂在穿透窗下反而点不到）；flags=0 = 资源管理器同款
        // 常规清空（确认框 + 进度 + 音效）
        SHEmptyRecycleBinW(None, PCWSTR::null(), 0).map_err(|e| e.to_string())
    }
}
