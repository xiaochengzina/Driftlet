//! Skin development hot reload (debug builds only — `lib.rs` setup gates the
//! `start` call behind `cfg!(debug_assertions)`).
//!
//! One `notify` watcher recursively watches the whole skins directory (no
//! per-skin watch bookkeeping); a debounce loop reloads a loaded skin ~300ms
//! after its folder goes quiet.
//!
//! Self-write filtering is the load-bearing part: the app itself writes into
//! skin folders, and reacting to those would loop forever.  Two mechanisms:
//!
//!   * Name list below — fixed-name app-managed files; it MUST be extended
//!     whenever a new fixed-name self-write into skin folders appears:
//!       - `settings.json` (+ `.tmp` / `.bak`) — skin settings writers
//!         (`set_skin_custom_setting`, `skin_set_setting`, migration,
//!         corrupt-file recovery; see skin/settings.rs).
//!       - `preview.png` — `capture_skin_preview` (commands.rs).
//!       - `.staging-*` / `.*.old` — package install's staged replace.
//!         (backup.rs's `.import-old` cousins are NOT matched — they end in
//!         `-old`, not `.old` — and need no filtering: they live beside
//!         rather than inside skins/.)
//!   * Recent-write set — skins writing ARBITRARY names into their own
//!     folder via `skin_write_file` / `skin_delete_file` (a name list can't
//!     cover those: toolbox saving its todo.json would otherwise reload
//!     itself).  The write side registers the resolved path via
//!     `note_self_write`; the watcher skips events that hit a fresh entry.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Manager};

use crate::skin::loader;
use crate::AppState;

/// Quiet period after the last change before a skin is reloaded.  Long
/// enough to absorb editor atomic-save bursts (tmp + rename), short enough
/// to feel instant while iterating.
const DEBOUNCE_MS: u64 = 300;

/// File/dir names produced by the app itself — never a reason to reload.
const SELF_WRITE_NAMES: &[&str] = &[
    "settings.json",
    "settings.json.tmp",
    "settings.json.bak",
    "preview.png",
];

fn is_self_write(path: &Path) -> bool {
    path.components().any(|c| {
        let name = c.as_os_str().to_string_lossy();
        SELF_WRITE_NAMES.contains(&name.as_ref())
            || name.starts_with(".staging-")
            || name.ends_with(".old")
    })
}

// ─── Recent self-write set (arbitrary names written via the skin API) ───

/// 皮肤经 `skin_write_file` / `skin_delete_file` 写自身目录的登记：
/// 规范化绝对路径 → 登记时间。watcher 命中未过期登记即跳过并消耗之。
/// 过期未命中的登记在下一次登记/查询时顺手清掉，不堆积。
static RECENT_SELF_WRITES: LazyLock<Mutex<HashMap<PathBuf, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 自写登记有效期：覆盖 notify 事件到达 + 去抖窗口的正常延迟。
const SELF_WRITE_TTL: Duration = Duration::from_secs(5);

/// 登记一次皮肤 API 自写——skin_api/fs.rs 在写/删落盘成功后调用，
/// `path` 为 resolve 出的绝对路径。
pub fn note_self_write(path: &Path) {
    let mut map = RECENT_SELF_WRITES.lock().unwrap_or_else(|e| e.into_inner());
    map.retain(|_, t| t.elapsed() < SELF_WRITE_TTL);
    map.insert(normalize_watch_path(path), Instant::now());
}

/// 命中未过期登记则消耗之并返回 true（过期登记顺手清除，视为未命中）。
fn take_recent_self_write(path: &Path) -> bool {
    let mut map = RECENT_SELF_WRITES.lock().unwrap_or_else(|e| e.into_inner());
    match map.remove(&normalize_watch_path(path)) {
        Some(t) => t.elapsed() < SELF_WRITE_TTL,
        None => false,
    }
}

/// canonicalize 的产物带 `\\?\` 长路径前缀，notify 事件路径不带——
/// 登记与比对前统一剥掉。
fn normalize_watch_path(path: &Path) -> PathBuf {
    let s = path.as_os_str().to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path.to_path_buf(),
    }
}

/// The skin folder a changed path belongs to, i.e. `<skins_dir>/<folder>/...`.
fn skin_folder(skins_dir: &Path, path: &Path) -> Option<PathBuf> {
    let rel = path.strip_prefix(skins_dir).ok()?;
    let folder = rel.components().next()?;
    Some(skins_dir.join(folder))
}

pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let skins_dir = app.state::<AppState>().skins_dir.clone();

        let (tx, rx) = mpsc::channel();
        let mut watcher: RecommendedWatcher = match notify::recommended_watcher(
            move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = tx.send(event);
                }
            },
        ) {
            Ok(w) => w,
            Err(e) => {
                log::warn!("hotreload: failed to create watcher: {}", e);
                return;
            }
        };
        if let Err(e) = watcher.watch(&skins_dir, RecursiveMode::Recursive) {
            log::warn!("hotreload: failed to watch {:?}: {}", skins_dir, e);
            return;
        }
        log::info!("hotreload: watching {:?}", skins_dir);

        // Debounce: remember the last event time per skin folder; a folder
        // that has been quiet for DEBOUNCE_MS gets its skin reloaded once.
        let mut pending: HashMap<PathBuf, Instant> = HashMap::new();
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(event) => {
                    for path in event.paths {
                        // 先查自写登记（皮肤 API 自写，任意文件名），
                        // 再查固定名清单
                        if take_recent_self_write(&path) || is_self_write(&path) {
                            continue;
                        }
                        if let Some(folder) = skin_folder(&skins_dir, &path) {
                            pending.insert(folder, Instant::now());
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }

            // Master switch (settings panel, config.hot_reload): when off,
            // events are drained but never fire — the watcher keeps running
            // so toggling back on takes effect immediately.
            if !app
                .state::<AppState>()
                .hot_reload_enabled
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                pending.clear();
                continue;
            }

            let quiet = Duration::from_millis(DEBOUNCE_MS);
            let fired: Vec<PathBuf> = pending
                .iter()
                .filter(|(_, last)| last.elapsed() >= quiet)
                .map(|(folder, _)| folder.clone())
                .collect();
            for folder in fired {
                pending.remove(&folder);
                reload_skin_in_folder(&app, &folder);
            }
        }
    });
}

/// Map a skin folder to a skin id (ids don't have to equal folder names) and
/// reload it if loaded.  Reload goes through the manager's own path, spawned
/// — the old window's label is freed by the event loop, so destroy+create
/// must not run synchronously on one thread (see tray::reload_all_skins).
fn reload_skin_in_folder(app: &AppHandle, folder: &Path) {
    let skins = loader::scan_skins_directory(&app.state::<AppState>().skins_dir);
    let Some(skin) = skins.iter().find(|s| s.directory == folder) else {
        return; // 未安装/无 manifest 的文件夹改动与运行中的皮肤无关
    };
    if !app.state::<AppState>().registry.is_loaded(&skin.id) {
        return; // 只热重载已加载的皮肤
    }
    let id = skin.id.clone();
    log::info!("hotreload: reloading '{}'", id);
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::commands::reload_skin_impl(handle, id.clone()).await {
            log::warn!("hotreload: failed to reload '{}': {}", id, e);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_write_names_are_filtered() {
        let base = Path::new(r"C:\skins\clock");
        assert!(is_self_write(&base.join("settings.json")));
        assert!(is_self_write(&base.join("settings.json.tmp")));
        assert!(is_self_write(&base.join("settings.json.bak")));
        assert!(is_self_write(&base.join("preview.png")));
        assert!(is_self_write(&base.join(".staging-clock").join("index.html")));
        assert!(is_self_write(&base.join(".clock.old").join("index.html")));
        assert!(!is_self_write(&base.join("index.html")));
        assert!(!is_self_write(&base.join("js").join("main.js")));
    }

    #[test]
    fn recent_self_write_is_consumed_once() {
        let p = Path::new(r"C:\skins\clock\todo.json");
        assert!(!take_recent_self_write(p));
        note_self_write(p);
        assert!(take_recent_self_write(p));
        // 登记被消耗：同一路径第二次不再命中
        assert!(!take_recent_self_write(p));
        // canonicalize 的 \\?\ 前缀与 notify 事件路径等价命中
        note_self_write(Path::new(r"\\?\C:\skins\clock\data.json"));
        assert!(take_recent_self_write(Path::new(r"C:\skins\clock\data.json")));
    }

    #[test]
    fn skin_folder_takes_first_component() {
        let skins = Path::new(r"C:\app\skins");
        assert_eq!(
            skin_folder(skins, Path::new(r"C:\app\skins\clock\js\main.js")),
            Some(PathBuf::from(r"C:\app\skins\clock"))
        );
        assert_eq!(skin_folder(skins, Path::new(r"C:\else\clock\a.js")), None);
    }
}
