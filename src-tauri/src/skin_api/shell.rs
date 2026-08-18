//! `run_command` implementation (the `shell` permission).
//!
//! The child runs with the app's own token (the app is never elevated, so
//! children never are either — no privilege escalation path), console window
//! hidden, stdout/stderr captured with a 1 MB cap each and a timeout.
//!
//! 超时只终止直接子进程：`cmd /c` 拉起的子孙进程不连带（杀的是 cmd
//! 本身，其子孙成孤儿继续跑）——要整树清理得走 Job Object，超出本模块
//! 的取舍范围。

use serde::Serialize;
use std::io::Read;
use std::time::{Duration, Instant};
use crate::i18n::{trf, Key};

#[derive(Debug, Clone, Serialize)]
pub struct CommandOutput {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;
pub const MAX_TIMEOUT_MS: u64 = 120_000;

pub fn run(
    command: &str,
    args: &[String],
    timeout_ms: Option<f64>,
    lang: &str,
) -> Result<CommandOutput, String> {
    // 参数 f64 收下 JSON 整数/小数（u64 遇小数会被 serde 拒成英文报错），
    // 这里取整并钳制：下限 100ms、上限 120s、默认 30s。
    // NaN 防线：NaN 穿透 clamp 后 as u64 = 0 → 立即超时——非有限值回默认
    let timeout = Duration::from_millis(
        match timeout_ms {
            Some(t) if t.is_finite() => t.clamp(100.0, MAX_TIMEOUT_MS as f64) as u64,
            _ => DEFAULT_TIMEOUT_MS,
        },
    );

    let mut cmd = std::process::Command::new(command);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    // Hidden window: no console flash for CLI children.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(windows::Win32::System::Threading::CREATE_NO_WINDOW.0);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| trf(lang, Key::CommandSpawnFailed, &[&e.to_string()]))?;

    // Both pipes are drained on helper threads: reading them sequentially
    // here would deadlock as soon as a chatty child fills the other pipe.
    // 结果经 channel 回传而不是 join 句柄：孙进程会继承管道写端（cmd /c
    // 拉起长驻进程时写端不随子进程关闭），join 会无限阻塞——超时分支与
    // 出错分支直接 detach（线程随孙进程退出自然结束），正常分支
    // recv_timeout 兜底。
    let mut out_pipe = child.stdout.take().unwrap();
    let mut err_pipe = child.stderr.take().unwrap();
    let (out_tx, out_rx) = std::sync::mpsc::channel();
    let (err_tx, err_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = out_tx.send(read_capped(&mut out_pipe));
    });
    std::thread::spawn(move || {
        let _ = err_tx.send(read_capped(&mut err_pipe));
    });

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    // as_secs_f64：亚秒超时（如 500ms）按 as_secs 会显示成
                    // 「0 秒」，毫秒级取整也丢精度
                    return Err(trf(
                        lang,
                        Key::CommandTimeout,
                        &[&timeout.as_secs_f64().to_string()],
                    ));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                // 与超时分支同套清理：try_wait 出错时子进程状态未知，杀掉
                // 再返回；reader 线程 detach 不 join（孙进程持有管道写端
                // 时 join 永不返回）
                let _ = child.kill();
                let _ = child.wait();
                return Err(trf(lang, Key::TaskFailed, &[&e.to_string()]));
            }
        }
    };

    // 子进程已退出：reader 正常立刻读到 EOF 回传；孙进程继承写端时读不到
    // EOF，2 秒宽限后按空输出放行——强过无限阻塞
    let stdout = out_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap_or_default();
    let stderr = err_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap_or_default();
    Ok(CommandOutput {
        code: status.code(),
        stdout,
        stderr,
    })
}

fn read_capped<R: Read>(r: &mut R) -> String {
    // 读满上限后必须继续读到 EOF（丢弃后续字节）：就此停读会关闭管道
    // 读端，子进程再写吃 broken pipe——多数命令随即出错退出（皮肤拿到
    // 截断输出加一个与输出无关的退出码），个别忽略写错误的则阻塞到
    // 超时被杀；两种情况都不是文档承诺的「截断 1MB 后正常返回」
    let mut buf = Vec::new();
    let mut limited = r.take(MAX_OUTPUT_BYTES as u64);
    let _ = limited.read_to_end(&mut buf);
    if buf.len() >= MAX_OUTPUT_BYTES {
        let _ = std::io::copy(&mut limited.into_inner(), &mut std::io::sink());
        // 截断点可能切在多字节 UTF-8 字符中间：残尾会让 decode_console
        // 的 from_utf8 整体失败、整篇回退 OEM 误码。被切断的残尾 ≤3 字节
        // （UTF-8 字符最长 4 字节）且必然在末尾；从头就非法的是 GBK 等
        // 非 UTF-8 编码，原样交给 decode_console 的 OEM 回退
        if let Err(e) = std::str::from_utf8(&buf) {
            let up_to = e.valid_up_to();
            if up_to >= buf.len().saturating_sub(3) {
                buf.truncate(up_to);
            }
        }
    }
    decode_console(&buf)
}

/// Console children on a Chinese Windows speak the OEM codepage (GBK), not
/// UTF-8 — decoding as lossy UTF-8 would garble every non-ASCII line.  Use
/// UTF-8 when valid, otherwise the system OEM codepage (Windows only).
#[cfg(target_os = "windows")]
fn decode_console(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    use windows::Win32::Globalization::{MultiByteToWideChar, CP_OEMCP};
    unsafe {
        let len = MultiByteToWideChar(CP_OEMCP, Default::default(), bytes, None);
        if len <= 0 {
            return String::from_utf8_lossy(bytes).to_string();
        }
        let mut wide = vec![0u16; len as usize];
        let n = MultiByteToWideChar(CP_OEMCP, Default::default(), bytes, Some(&mut wide));
        if n <= 0 {
            return String::from_utf8_lossy(bytes).to_string();
        }
        wide.truncate(n as usize);
        String::from_utf16_lossy(&wide)
    }
}

#[cfg(not(target_os = "windows"))]
fn decode_console(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn captures_stdout_and_exit_code() {
        let out = run("cmd", &args(&["/c", "echo hello"]), Some(5000.0), "zh-CN").unwrap();
        assert_eq!(out.code, Some(0));
        assert!(out.stdout.contains("hello"));
    }

    #[test]
    fn times_out_and_kills() {
        let err = run(
            "cmd",
            &args(&["/c", "ping", "-n", "10", "127.0.0.1", ">nul"]),
            Some(500.0),
            "zh-CN",
        )
        .unwrap_err();
        assert!(err.contains("超时"));
    }

    #[test]
    fn spawn_failure_is_reported() {
        assert!(run("no-such-exe-driftlet", &[], Some(1000.0), "zh-CN").is_err());
    }

    #[test]
    fn oversized_output_is_truncated_not_killed() {
        // 输出 >1MB：读满上限后继续排空管道，子进程正常跑完（不再吃
        // broken pipe），返回截断的 1MB 与真实退出码
        let out = run(
            "cmd",
            &args(&["/c", "for /l %i in (1,1,40000) do @echo xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"]),
            Some(30000.0),
            "zh-CN",
        )
        .unwrap();
        assert_eq!(out.code, Some(0));
        assert_eq!(out.stdout.len(), MAX_OUTPUT_BYTES);
    }

    #[test]
    fn truncation_backs_off_to_utf8_boundary() {
        // 1MB 截断点切在多字节字符中间时回退到字符边界，合法 UTF-8 输出
        // 不会整篇被回退成 OEM 误码
        let unit = "界"; // 3 字节 UTF-8，1MB 不是 3 的倍数，必然切断
        let mut raw = unit.repeat(MAX_OUTPUT_BYTES / 3 + 1).into_bytes();
        raw.truncate(MAX_OUTPUT_BYTES + 10);
        let mut cursor = std::io::Cursor::new(raw);
        let s = read_capped(&mut cursor);
        assert!(std::str::from_utf8(s.as_bytes()).is_ok());
        assert!(s.len() <= MAX_OUTPUT_BYTES);
    }
}
