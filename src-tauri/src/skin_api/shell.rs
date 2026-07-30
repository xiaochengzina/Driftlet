//! `run_command` implementation (the `shell` permission).
//!
//! The child runs with the app's own token (the app is never elevated, so
//! children never are either — no privilege escalation path), console window
//! hidden, stdout/stderr captured with a 1 MB cap each and a timeout.

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
    timeout_ms: Option<u64>,
    lang: &str,
) -> Result<CommandOutput, String> {
    let timeout = Duration::from_millis(
        timeout_ms
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .clamp(100, MAX_TIMEOUT_MS),
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
    let mut out_pipe = child.stdout.take().unwrap();
    let mut err_pipe = child.stderr.take().unwrap();
    let out_thr = std::thread::spawn(move || read_capped(&mut out_pipe));
    let err_thr = std::thread::spawn(move || read_capped(&mut err_pipe));

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = out_thr.join();
                    let _ = err_thr.join();
                    return Err(trf(
                        lang,
                        Key::CommandTimeout,
                        &[&timeout.as_secs().to_string()],
                    ));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(trf(lang, Key::CommandSpawnFailed, &[&e.to_string()])),
        }
    };

    let stdout = out_thr.join().unwrap_or_default();
    let stderr = err_thr.join().unwrap_or_default();
    Ok(CommandOutput {
        code: status.code(),
        stdout,
        stderr,
    })
}

fn read_capped<R: Read>(r: &mut R) -> String {
    let mut buf = Vec::new();
    let _ = r.take((MAX_OUTPUT_BYTES + 1) as u64).read_to_end(&mut buf);
    buf.truncate(MAX_OUTPUT_BYTES);
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
        let out = run("cmd", &args(&["/c", "echo hello"]), Some(5000), "zh-CN").unwrap();
        assert_eq!(out.code, Some(0));
        assert!(out.stdout.contains("hello"));
    }

    #[test]
    fn times_out_and_kills() {
        let err = run(
            "cmd",
            &args(&["/c", "ping", "-n", "10", "127.0.0.1", ">nul"]),
            Some(500),
            "zh-CN",
        )
        .unwrap_err();
        assert!(err.contains("超时"));
    }

    #[test]
    fn spawn_failure_is_reported() {
        assert!(run("no-such-exe-driftlet", &[], Some(1000), "zh-CN").is_err());
    }
}
