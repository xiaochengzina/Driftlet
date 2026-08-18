//! Master volume control (Windows): IAudioEndpointVolume on the default
//! render endpoint.  COM is initialized per call — Tauri commands land on
//! threads whose apartment state we don't own, so RPC_E_CHANGED_MODE
//! (already initialized differently) just means "proceed, don't uninit".

use super::VolumeInfo;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
};

fn with_endpoint<T>(f: impl FnOnce(&IAudioEndpointVolume) -> Result<T, String>) -> Result<T, String> {
    unsafe {
        // 每次成功的 CoInitializeEx 都要配对一次 CoUninitialize，而 S_FALSE
        // （本线程已按同一模型初始化过）同样是成功 HRESULT——只认 S_OK 会
        // 漏配一次 uninit，引用计数不平衡。RPC_E_CHANGED_MODE（已按别的
        // 套间模型初始化）是失败码，is_ok() 为 false，正好不配对。
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        let initialized_here = hr.is_ok();
        let result = (|| {
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| e.to_string())?;
            let device = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| e.to_string())?;
            let endpoint: IAudioEndpointVolume = device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| e.to_string())?;
            f(&endpoint)
        })();
        if initialized_here {
            CoUninitialize();
        }
        result
    }
}

pub fn get_volume() -> Result<VolumeInfo, String> {
    with_endpoint(|ep| unsafe {
        let scalar = ep.GetMasterVolumeLevelScalar().map_err(|e| e.to_string())?;
        let muted = ep.GetMute().map_err(|e| e.to_string())?.as_bool();
        Ok(VolumeInfo {
            volume_pct: scalar * 100.0,
            muted,
        })
    })
}

pub fn set_volume(volume_pct: f32) -> Result<(), String> {
    // NaN 防线：NaN 穿透 clamp 直达 COM——非有限值按 0 处理
    let scalar = if volume_pct.is_finite() { (volume_pct / 100.0).clamp(0.0, 1.0) } else { 0.0 };
    with_endpoint(|ep| unsafe {
        ep.SetMasterVolumeLevelScalar(scalar, std::ptr::null())
            .map_err(|e| e.to_string())
    })
}

pub fn set_mute(muted: bool) -> Result<(), String> {
    with_endpoint(|ep| unsafe {
        ep.SetMute(muted, std::ptr::null()).map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "hardware probe — run manually with --nocapture"]
    fn probe_volume() {
        let v = get_volume().unwrap();
        println!("volume: {:?}", v);
        assert!((0.0..=100.0).contains(&v.volume_pct));
        // No-op write: set to the value we just read (must not fail).
        set_volume(v.volume_pct).unwrap();
        set_mute(v.muted).unwrap();
    }
}
