//! In-memory app log ring buffer + custom `log` crate logger.
//!
//! 后端日志全部留在内存环形缓冲（上限 [`MAX_ENTRIES`]，淘汰最旧），只有
//! label 为 `"log"` 的日志窗口存在时才把新条目 emit 过去——窗口不开，
//! 前端零开销。日志窗口打开时先 listen 再调 `get_app_log` 拉快照，按
//! `seq` 去重合并。
//!
//! 捕获口径：自定义 logger 挂在 `log` crate 上，只收 `driftlet_lib` 目标
//!（wry/tao 等第三方 crate 的打点不进门），全级别 eprintln 到 stderr 顶替
//! 原 env_logger 的开发期可见性；但**只有 Warn 及以上进缓冲**——Info 级
//! 操作流水（加载/卸载/设置修改/启停/快捷键）是用户刚做完就知道的事，
//! 实测无价值，不进日志窗口。皮肤消息（`skin_log`/`skin_console_log` 命令）
//! 直接 `push` 不受该闸门限制，source 带皮肤 id。

use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager};

/// 环形缓冲上限：满后每进一条淘汰最旧一条（先进先出式淘汰，保留最新）。
pub const MAX_ENTRIES: usize = 1000;
/// 单条消息字符上限：截断防爆量（皮肤可能发来整段 JSON/堆栈）。
pub const MAX_MESSAGE_CHARS: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

impl LogLevel {
    /// skin_log 的 level 参数解析：只认 "warn"/"error"，其余一律 info。
    pub fn from_level_str(s: Option<&str>) -> Self {
        match s {
            Some("warn") => LogLevel::Warn,
            Some("error") => LogLevel::Error,
            _ => LogLevel::Info,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    /// 单调递增序号：前端 listen 与快照合并时按它去重排序。
    pub seq: u64,
    /// Unix epoch 毫秒（前端转本地 HH:MM:SS 显示）。
    pub ts_ms: u64,
    pub level: LogLevel,
    /// "backend" 或 "skin:<id>"。
    pub source: String,
    pub message: String,
}

static BUF: LazyLock<Mutex<VecDeque<LogEntry>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));
static SEQ: AtomicU64 = AtomicU64::new(0);
/// setup 时注入的 AppHandle：push 时向日志窗口（若开着）定向 emit。
static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 追加一条日志。任何线程可调；锁只持极短（一次入队）。
/// 事件合批：皮肤高频调 skin_log 时逐条 emit 会把日志窗口打成 IPC 事件
/// 风暴——100ms 窗口内合并为一批发出（事件名不变，payload 由单条改为
/// 数组；前端兼容两种形状）。
pub fn push(level: LogLevel, source: impl Into<String>, message: impl Into<String>) {
    let mut msg: String = message.into();
    if msg.chars().count() > MAX_MESSAGE_CHARS {
        msg = msg.chars().take(MAX_MESSAGE_CHARS).collect();
    }
    let entry = LogEntry {
        seq: SEQ.fetch_add(1, Ordering::Relaxed),
        ts_ms: now_ms(),
        level,
        source: source.into(),
        message: msg,
    };
    {
        let mut buf = BUF.lock().unwrap_or_else(|e| e.into_inner());
        while buf.len() >= MAX_ENTRIES {
            buf.pop_front();
        }
        buf.push_back(entry.clone());
    }
    // 窗口不存在时 get_webview_window 返回 None，直接不 emit ——
    // 「不开窗口不推前端」的内存约定就落在这一行。
    if APP.get().is_none() {
        return;
    }
    let spawn_flush = {
        let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
        pending.0.push(entry);
        if pending.1 {
            false
        } else {
            pending.1 = true;
            true
        }
    };
    if spawn_flush {
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let batch = {
                let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
                pending.1 = false;
                std::mem::take(&mut pending.0)
            };
            if batch.is_empty() {
                return;
            }
            if let Some(app) = APP.get() {
                if app.get_webview_window("log").is_some() {
                    let _ = app.emit_to("log", "app-log-added", &batch);
                }
            }
        });
    }
}

/// 事件合批缓冲：(待发条目, flush 线程已排期)。约 100ms 一批。
static PENDING: std::sync::LazyLock<Mutex<(Vec<LogEntry>, bool)>> =
    std::sync::LazyLock::new(|| Mutex::new((Vec::new(), false)));

pub fn entries() -> Vec<LogEntry> {
    BUF.lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .cloned()
        .collect()
}

pub fn clear() {
    BUF.lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

// ─── log crate 对接 ───

struct DriftletLogger;

impl log::Log for DriftletLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
            && metadata.target().starts_with("driftlet_lib")
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let msg = format!("{}", record.args());
        // 原 env_logger 的开发期 stderr 可见性全级别保留（release GUI 无控制台，无害）。
        eprintln!("[{}][{}] {}", record.level(), record.target(), msg);
        // 日志窗口只收警告/报错：Info 级操作流水（加载/卸载/设置修改…）实测
        // 对用户无价值——刚做完就知道的事，没有可行动信息。
        if record.level() <= log::Level::Warn {
            let level = match record.level() {
                log::Level::Error => LogLevel::Error,
                _ => LogLevel::Warn,
            };
            push(level, "backend", msg);
        }
    }

    fn flush(&self) {}
}

/// 替换 env_logger：全局只初始化一次（run() 开头调用）。
pub fn init_logger() {
    if log::set_boxed_logger(Box::new(DriftletLogger)).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_evicts_oldest_and_truncates() {
        // 单函数顺序断言：BUF 是全局静态，拆多个 #[test] 会并行互相干扰。
        clear();

        // 淘汰最旧、保留最新
        for i in 0..(MAX_ENTRIES + 10) {
            push(LogLevel::Info, "backend", format!("msg-{}", i));
        }
        let list = entries();
        assert_eq!(list.len(), MAX_ENTRIES);
        assert_eq!(list.first().unwrap().message, "msg-10");
        assert_eq!(list.last().unwrap().message, format!("msg-{}", MAX_ENTRIES + 9));
        // seq 单调递增
        assert!(list.windows(2).all(|w| w[0].seq < w[1].seq));

        // clear
        clear();
        assert!(entries().is_empty());

        // 消息截断
        let long = "x".repeat(MAX_MESSAGE_CHARS + 500);
        push(LogLevel::Warn, "backend", long);
        assert_eq!(
            entries().last().unwrap().message.chars().count(),
            MAX_MESSAGE_CHARS
        );

        // level 解析
        assert_eq!(LogLevel::from_level_str(Some("warn")), LogLevel::Warn);
        assert_eq!(LogLevel::from_level_str(Some("error")), LogLevel::Error);
        assert_eq!(LogLevel::from_level_str(Some("info")), LogLevel::Info);
        assert_eq!(LogLevel::from_level_str(Some("debug")), LogLevel::Info);
        assert_eq!(LogLevel::from_level_str(None), LogLevel::Info);

        clear();
    }
}
