/// Capture a skin window's WebView2 content to a PNG file via
/// ICoreWebView2::CapturePreview. Windows-only.
///
/// 取代旧的 GDI PrintWindow 路径：直接取 webview 渲染结果，alpha 正确，
/// 不受窗口遮挡 / 贴桌面 / 最小化影响。

#[cfg(target_os = "windows")]
pub fn capture_webview_to_png(
    window: &tauri::WebviewWindow,
    output_path: &std::path::Path,
    lang: &'static str,
) -> Result<(), String> {
    use std::sync::mpsc;
    use std::time::Duration;
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::Com::{
        IStream, STATSTG, STATFLAG_DEFAULT, STREAM_SEEK_SET,
        StructuredStorage::CreateStreamOnHGlobal,
    };
    use webview2_com::{
        CapturePreviewCompletedHandler,
        Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
    };
    use crate::i18n::{tr, trf, Key};

    /// 读出 IStream 全部内容并写进文件（在截图完成回调里调用）。
    fn drain_stream_to_file(stream: &IStream, path: &std::path::Path, lang: &str) -> Result<(), String> {
        unsafe {
            let mut stat = STATSTG::default();
            stream
                .Stat(&mut stat, STATFLAG_DEFAULT)
                .map_err(|e| format!("IStream::Stat failed: {e:?}"))?;
            let size = stat.cbSize as usize;
            stream
                .Seek(0, STREAM_SEEK_SET, None)
                .map_err(|e| format!("IStream::Seek failed: {e:?}"))?;

            let mut buf = vec![0u8; size];
            let mut filled = 0usize;
            while filled < size {
                let mut got = 0u32;
                let hr = stream.Read(
                    buf[filled..].as_mut_ptr() as *mut _,
                    (size - filled) as u32,
                    Some(&mut got),
                );
                if hr.is_err() {
                    return Err(format!("IStream::Read failed: {hr:?}"));
                }
                if got == 0 {
                    break;
                }
                filled += got as usize;
            }
            buf.truncate(filled);
            std::fs::write(path, &buf).map_err(|e| trf(lang, Key::WritePreviewFailed, &[&e.to_string()]))?;
        }
        Ok(())
    }

    // 截图在 WebView2 的完成回调里才真正结束 —— 结果经 channel 传回当前（命令）线程。
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    let path = output_path.to_path_buf();

    window
        .with_webview(move |webview| unsafe {
            let send_err = {
                let tx = tx.clone();
                move |msg: String| {
                    let _ = tx.send(Err(msg));
                }
            };

            let core = match webview.controller().CoreWebView2() {
                Ok(c) => c,
                Err(e) => return send_err(trf(lang, Key::WebViewNotReady, &[&format!("{e:?}")])),
            };
            let stream = match CreateStreamOnHGlobal(HGLOBAL::default(), true) {
                Ok(s) => s,
                Err(e) => return send_err(format!("CreateStreamOnHGlobal failed: {e:?}")),
            };

            let cb_stream = stream.clone();
            let handler = CapturePreviewCompletedHandler::create(Box::new(move |result| {
                let outcome = result
                    .map_err(|e| format!("CapturePreview failed: {e:?}"))
                    .and_then(|_| drain_stream_to_file(&cb_stream, &path, lang));
                let _ = tx.send(outcome);
                Ok(())
            }));

            if let Err(e) = core.CapturePreview(
                COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                &stream,
                &handler,
            ) {
                send_err(trf(lang, Key::CapturePreviewCallFailed, &[&format!("{e:?}")]));
            }
        })
        .map_err(|e| trf(lang, Key::AccessWebViewFailed, &[&e.to_string()]))?;

    // with_webview 从工作线程调用只是把闭包投递到事件循环，
    // 超时兜底窗口已销毁 / webview 无响应的情况。
    rx.recv_timeout(Duration::from_secs(10))
        .map_err(|_| tr(lang, Key::CaptureTimeout).to_string())?
}
