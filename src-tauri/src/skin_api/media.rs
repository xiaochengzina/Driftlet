//! Now-playing information and transport control via Windows SMTC
//! (GlobalSystemMediaTransportControlsSessionManager).
//!
//! WinRT async operations are awaited with the blocking `.get()`, so both
//! entry points must run on a worker thread (the commands wrap them in
//! `spawn_blocking`) — never on the main/UI thread.
//!
//! Degradation contract: "no current session" is not an error (returns
//! None); cover-art failure only blanks the cover field; a broken SMTC
//! service is the only hard error.

use base64::Engine;
use super::{MediaAction, MediaInfo};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
};

/// 当前时刻的 WinRT DateTime 基准值（1601-01-01 UTC 起的 100ns 计数）——
/// 与 timeline.LastUpdatedTime 同一基准，两者相减即「距上次上报的时长」
#[cfg(target_os = "windows")]
fn windows_now_100ns() -> i64 {
    // Unix epoch(1970) → WinRT epoch(1601) 的 100ns 差值
    const EPOCH_DIFF_100NS: i64 = 116_444_736_000_000_000;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    now.as_nanos() as i64 / 100 + EPOCH_DIFF_100NS
}

/// 会话择优：GetCurrentSession 是「最近交互过」的会话而非「正在播放」的——
/// 网易云音乐等场景里系统当前会话可能根本不是正主。枚举全部会话按
/// 「正在播放 > 有进度 > 有元数据」打分挑最像的。
fn pick_session(mgr: &Manager) -> Option<Session> {
    let sessions = mgr.GetSessions().ok()?;
    let mut best: Option<(Session, i32)> = None;
    for i in 0..sessions.Size().unwrap_or(0) {
        let Ok(session) = sessions.GetAt(i) else { continue };
        let mut score = 0i32;
        if let Ok(pb) = session.GetPlaybackInfo() {
            if let Ok(PlaybackStatus::Playing) = pb.PlaybackStatus() {
                score += 100;
            }
            // 支持寻址的会话几乎必有进度（寻址控件与时间线同族能力）
            if let Ok(controls) = pb.Controls() {
                if let Ok(true) = controls.IsPlaybackPositionEnabled() {
                    score += 10;
                }
            }
        }
        if let Ok(tl) = session.GetTimelineProperties() {
            if let Ok(d) = tl.EndTime() {
                if d.Duration > 0 {
                    score += 5;
                }
            }
        }
        if session
            .TryGetMediaPropertiesAsync()
            .and_then(|op| op.get())
            .ok()
            .and_then(|p| p.Title().ok())
            .map(|t| !t.to_string().is_empty())
            .unwrap_or(false)
        {
            score += 1;
        }
        if score > 0 && best.as_ref().map(|(_, s)| score > *s).unwrap_or(true) {
            best = Some((session, score));
        }
    }
    best.map(|(s, _)| s)
}

pub fn info() -> Result<Option<MediaInfo>, String> {
    let mgr = Manager::RequestAsync()
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    // No session = nothing controllable playing right now → None, not error.
    let session = match pick_session(&mgr) {
        Some(s) => s,
        None => return Ok(None),
    };

    let props = session
        .TryGetMediaPropertiesAsync()
        .and_then(|op| op.get())
        .ok();

    let playback = session.GetPlaybackInfo().map_err(|e| e.to_string())?;
    let status = match playback.PlaybackStatus().map_err(|e| e.to_string())? {
        PlaybackStatus::Playing => "playing",
        PlaybackStatus::Paused => "paused",
        _ => "stopped",
    };
    // 源播放器报「支持寻址」才可拖进度条（媒体中心式播控都可能关寻址）
    let seekable = playback
        .Controls()
        .and_then(|c| c.IsPlaybackPositionEnabled())
        .unwrap_or(false);

    let (mut position_secs, mut duration_secs) = (0.0, 0.0);
    if let Ok(timeline) = session.GetTimelineProperties() {
        if let Ok(p) = timeline.Position() {
            position_secs = p.Duration as f64 / 10_000_000.0;
        }
        if let Ok(d) = timeline.EndTime() {
            duration_secs = d.Duration as f64 / 10_000_000.0;
        }
        // 实时进度：SMTC 的 Position 是「上报时刻」的快照，不随播放推进——
        // 播放中用 LastUpdatedTime + 播放速率推算当前位置，暂停/停止用快照原值
        if status == "playing" {
            if let Ok(last_updated) = timeline.LastUpdatedTime() {
                // WinRT DateTime = 1601 纪元起的 100ns；本地当前时刻同基准
                let rate = playback
                    .PlaybackRate()
                    .ok()
                    .and_then(|r| r.Value().ok())
                    .unwrap_or(1.0);
                let elapsed_secs = ((windows_now_100ns() - last_updated.UniversalTime) as f64 / 10_000_000.0).max(0.0);
                position_secs += elapsed_secs * rate;
            }
        }
        // 钳到 [0, duration]（推算可能略超）
        if duration_secs > 0.0 {
            position_secs = position_secs.clamp(0.0, duration_secs);
        }
    }

    let cover = props
        .as_ref()
        .and_then(|p| p.Thumbnail().ok())
        .and_then(read_thumbnail);

    Ok(Some(MediaInfo {
        title: props.as_ref().and_then(|p| p.Title().ok()).map(|h| h.to_string()).unwrap_or_default(),
        artist: props.as_ref().and_then(|p| p.Artist().ok()).map(|h| h.to_string()).unwrap_or_default(),
        album: props.as_ref().and_then(|p| p.AlbumTitle().ok()).map(|h| h.to_string()).unwrap_or_default(),
        status: status.to_string(),
        position_secs,
        duration_secs,
        seekable,
        cover_base64: cover.as_ref().map(|(b, _)| b.clone()),
        cover_mime: cover.and_then(|(_, m)| m.map(str::to_string)),
    }))
}

pub fn control(action: MediaAction) -> Result<bool, String> {
    let mgr = Manager::RequestAsync()
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    // 控制走择优选出的同一会话（与 info 同一路径——控制与展示不打架）
    let session = pick_session(&mgr)
        .ok_or_else(|| "no media session (nothing playing)".to_string())?;
    let op = match action {
        MediaAction::Play => session.TryPlayAsync(),
        MediaAction::Pause => session.TryPauseAsync(),
        MediaAction::PlayPause => session.TryTogglePlayPauseAsync(),
        MediaAction::Next => session.TrySkipNextAsync(),
        MediaAction::Previous => session.TrySkipPreviousAsync(),
    };
    op.map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())
}

/// 拖动进度条寻址：position_secs 为绝对秒数。源不支持寻址时返回 Ok(false)
///（不是错误——皮肤据此把进度条锁成只读）。
pub fn seek(position_secs: f64) -> Result<bool, String> {
    // 入参钳制（审查 L4）：非有限值归 0、范围钳 0–24h——负值/巨值不直达
    // SMTC（Rust 的 as i64 本就饱和转换无 UB，这里钳的是语义边界）
    let position_secs = if position_secs.is_finite() {
        position_secs.clamp(0.0, 86_400.0)
    } else {
        0.0
    };
    let mgr = Manager::RequestAsync()
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    let session = pick_session(&mgr)
        .ok_or_else(|| "no media session (nothing playing)".to_string())?;
    let supported = session
        .GetPlaybackInfo()
        .and_then(|pb| pb.Controls())
        .and_then(|c| c.IsPlaybackPositionEnabled())
        .unwrap_or(false);
    if !supported {
        return Ok(false);
    }
    session
        .TryChangePlaybackPositionAsync((position_secs * 10_000_000.0) as i64)
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())
}

/// Artwork is optional and often small (≤ a few hundred KB); any failure
/// just means "no cover". 2 MB cap guards against pathological streams.
/// 返回 (base64, mime)：SMTC 不给图片格式，按 magic bytes 嗅探。
fn read_thumbnail(
    reference: windows::Storage::Streams::IRandomAccessStreamReference,
) -> Option<(String, Option<&'static str>)> {
    let stream = reference.OpenReadAsync().ok()?.get().ok()?;
    let size = stream.Size().ok()?;
    if size == 0 || size > 2 * 1024 * 1024 {
        return None;
    }
    let reader = windows::Storage::Streams::DataReader::CreateDataReader(&stream).ok()?;
    let loaded = reader.LoadAsync(size as u32).ok()?.get().ok()?;
    let mut buf = vec![0u8; loaded as usize];
    reader.ReadBytes(&mut buf).ok()?;
    let mime = sniff_image_mime(&buf);
    Some((base64::engine::general_purpose::STANDARD.encode(buf), mime))
}

/// 常见图片格式的 magic bytes；认不出来返回 None（皮肤自行决定回退格式）。
fn sniff_image_mime(buf: &[u8]) -> Option<&'static str> {
    if buf.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if buf.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        Some("image/png")
    } else if buf.starts_with(b"GIF8") {
        Some("image/gif")
    } else if buf.starts_with(b"BM") {
        Some("image/bmp")
    } else if buf.len() >= 12 && buf.starts_with(b"RIFF") && &buf[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "hardware probe — run manually with --nocapture"]
    fn probe_media_info() {
        match info() {
            Ok(Some(m)) => println!(
                "playing: {} - {} [{}] {}/{}s seekable={} cover={}",
                m.artist,
                m.title,
                m.status,
                m.position_secs,
                m.duration_secs,
                m.seekable,
                m.cover_base64.as_ref().map(|c| c.len()).unwrap_or(0)
            ),
            Ok(None) => println!("no media session (nothing playing)"),
            Err(e) => panic!("SMTC broken: {}", e),
        }
    }

    /// 全会话探针：把每个 SMTC 会话的来源/状态/进度/寻址能力全 dump 出来——
    /// 诊断「某播放器拿不到进度」时跑这个，看它的会话到底报了什么。
    /// 用法：cargo test probe_all_sessions -- --ignored --nocapture
    #[test]
    #[ignore = "hardware probe — run manually with --nocapture"]
    fn probe_all_sessions() {
        let mgr = Manager::RequestAsync().unwrap().get().unwrap();
        let sessions = mgr.GetSessions().unwrap();
        let n = sessions.Size().unwrap_or(0);
        println!("=== {} session(s) ===", n);
        for i in 0..n {
            let Ok(s) = sessions.GetAt(i) else { continue };
            let app = s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
            let status = s.GetPlaybackInfo().and_then(|p| p.PlaybackStatus()).map(|s| format!("{:?}", s)).unwrap_or_else(|_| "?".into());
            let seek = s.GetPlaybackInfo().and_then(|p| p.Controls()).and_then(|c| c.IsPlaybackPositionEnabled()).unwrap_or(false);
            let title = s.TryGetMediaPropertiesAsync().and_then(|op| op.get()).ok().and_then(|p| p.Title().ok()).map(|t| t.to_string()).unwrap_or_default();
            let (pos, dur) = match s.GetTimelineProperties() {
                Ok(tl) => {
                    let p = tl.Position().map(|t| t.Duration as f64 / 1e7).unwrap_or(0.0);
                    let d = tl.EndTime().map(|t| t.Duration as f64 / 1e7).unwrap_or(0.0);
                    (p, d)
                }
                Err(_) => (0.0, 0.0),
            };
            println!("[{i}] app={app} status={status} seekable={seek} title={title} pos={pos:.1}/{dur:.1}s", i = i, app = app, status = status, seek = seek, title = title, pos = pos, dur = dur);
        }
    }
}
