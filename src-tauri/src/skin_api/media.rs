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
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
};

pub fn info() -> Result<Option<MediaInfo>, String> {
    let mgr = Manager::RequestAsync()
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    // No session = nothing controllable playing right now → None, not error.
    let session = match mgr.GetCurrentSession() {
        Ok(s) => s,
        Err(_) => return Ok(None),
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

    let (mut position_secs, mut duration_secs) = (0.0, 0.0);
    if let Ok(timeline) = session.GetTimelineProperties() {
        if let Ok(p) = timeline.Position() {
            position_secs = p.Duration as f64 / 10_000_000.0;
        }
        if let Ok(d) = timeline.EndTime() {
            duration_secs = d.Duration as f64 / 10_000_000.0;
        }
    }

    let cover_base64 = props
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
        cover_base64,
    }))
}

pub fn control(action: MediaAction) -> Result<bool, String> {
    let mgr = Manager::RequestAsync()
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    let session = mgr.GetCurrentSession().map_err(|e| e.to_string())?;
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

/// Artwork is optional and often small (≤ a few hundred KB); any failure
/// just means "no cover". 2 MB cap guards against pathological streams.
fn read_thumbnail(
    reference: windows::Storage::Streams::IRandomAccessStreamReference,
) -> Option<String> {
    let stream = reference.OpenReadAsync().ok()?.get().ok()?;
    let size = stream.Size().ok()?;
    if size == 0 || size > 2 * 1024 * 1024 {
        return None;
    }
    let reader = windows::Storage::Streams::DataReader::CreateDataReader(&stream).ok()?;
    let loaded = reader.LoadAsync(size as u32).ok()?.get().ok()?;
    let mut buf = vec![0u8; loaded as usize];
    reader.ReadBytes(&mut buf).ok()?;
    Some(base64::engine::general_purpose::STANDARD.encode(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "hardware probe — run manually with --nocapture"]
    fn probe_media_info() {
        match info() {
            Ok(Some(m)) => println!(
                "playing: {} - {} [{}] {}/{}s cover={}",
                m.artist,
                m.title,
                m.status,
                m.position_secs,
                m.duration_secs,
                m.cover_base64.as_ref().map(|c| c.len()).unwrap_or(0)
            ),
            Ok(None) => println!("no media session (nothing playing)"),
            Err(e) => panic!("SMTC broken: {}", e),
        }
    }
}
