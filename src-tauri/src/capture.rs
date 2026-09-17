/// Capture a skin window's WebView2 content to a PNG file via
/// ICoreWebView2::CapturePreview. Windows-only.
///
/// 取代旧的 GDI PrintWindow 路径：直接取 webview 渲染结果，alpha 正确，
/// 不受窗口遮挡 / 贴桌面 / 最小化影响。
///
/// 落盘前降采样到 PREVIEW_MAX_DIMENSION：管理器皮肤卡片的显示高度只有
/// 96px（style.css .skin-preview），而 CapturePreview 按窗口原始分辨率
/// 出图——大屏皮肤可达 1920×1080+，解码后 ~8MB RGBA 整张驻留管理器渲染
/// 进程（列表里每张预览都如此）。压到 640px（显示尺寸的数倍，含高分屏
/// 余量）后单张解码内存 ~0.6MB。降采样失败不阻断截图：原图落盘，大不了
/// 偏大，不能没有。
#[cfg(target_os = "windows")]
pub fn capture_webview_to_png(
    window: &tauri::WebviewWindow,
    output_path: &std::path::Path,
    lang: &'static str,
) -> Result<(), String> {
    use crate::i18n::{Key, tr, trf};
    use std::sync::mpsc;
    use std::time::Duration;
    use webview2_com::{
        CapturePreviewCompletedHandler,
        Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
    };
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::Com::{
        IStream, STATFLAG_DEFAULT, STATSTG, STREAM_SEEK_SET,
        StructuredStorage::CreateStreamOnHGlobal,
    };

    /// 预览图降采样上限（像素）。管理器卡片显示高度 96px，640 已含
    /// 高分屏与宽预览余量；创作者自带的 preview.png 上限见 package.rs
    /// 的安装校验（1280），两边口径不同是刻意的：截图服务的是列表缩略，
    /// 创作者预览允许更精细的原图。
    const PREVIEW_MAX_DIMENSION: u32 = 640;

    /// PNG 解码 → 等比压边 → 重编码。任何一步失败都回退原字节：
    /// 预览图「偏大」只是内存取舍，「缺失」会直接打断加载流程的
    /// 「装完即见预览」体验，所以这里永不以报错收场。
    fn downscale_preview_png(buf: Vec<u8>) -> Vec<u8> {
        let Ok(img) = image::load_from_memory_with_format(&buf, image::ImageFormat::Png) else {
            return buf;
        };
        let (w, h) = (img.width(), img.height());
        let longest = w.max(h);
        if longest <= PREVIEW_MAX_DIMENSION {
            return buf;
        }
        let scale = PREVIEW_MAX_DIMENSION as f32 / longest as f32;
        let nw = ((w as f32 * scale).round() as u32).max(1);
        let nh = ((h as f32 * scale).round() as u32).max(1);
        let resized = image::imageops::resize(
            &img.to_rgba8(),
            nw,
            nh,
            image::imageops::FilterType::Lanczos3,
        );
        let mut out = std::io::Cursor::new(Vec::new());
        match image::write_buffer_with_format(
            &mut out,
            resized.as_raw(),
            nw,
            nh,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        ) {
            Ok(()) => out.into_inner(),
            Err(_) => buf,
        }
    }

    /// 读出 IStream 全部内容并写进文件（在截图完成回调里调用）。
    fn drain_stream_to_file(
        stream: &IStream,
        path: &std::path::Path,
        lang: &str,
    ) -> Result<(), String> {
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
            // 零字节截图不得当成功落盘（Stat/Read 得 0 时静默写空文件，
            // 失败截图被当成功——预览图变成空白文件还当最新）
            if filled == 0 {
                return Err(format!(
                    "capture produced 0 bytes (source: {} bytes declared)",
                    size
                ));
            }
            // 降采样后再落盘：列表缩略用不到全尺寸位图（见文件头注释）
            let buf = downscale_preview_png(buf);
            // 落盘原子写（tmp + rename）：连点两路截图/进程崩溃的半截写
            // 不会留下坏图（2026-09 审查 F4）
            let tmp = path.with_extension("tmp");
            std::fs::write(&tmp, &buf)
                .and_then(|_| std::fs::rename(&tmp, path))
                .map_err(|e| trf(lang, Key::WritePreviewFailed, &[&e.to_string()]))?;
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
                send_err(trf(
                    lang,
                    Key::CapturePreviewCallFailed,
                    &[&format!("{e:?}")],
                ));
            }
        })
        .map_err(|e| trf(lang, Key::AccessWebViewFailed, &[&e.to_string()]))?;

    // with_webview 从工作线程调用只是把闭包投递到事件循环，
    // 超时兜底窗口已销毁 / webview 无响应的情况。
    rx.recv_timeout(Duration::from_secs(10))
        .map_err(|_| tr(lang, Key::CaptureTimeout).to_string())?
}
