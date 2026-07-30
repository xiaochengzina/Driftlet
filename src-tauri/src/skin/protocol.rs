use std::borrow::Cow;
use std::path::Path;
use tauri::http;
use tauri::Manager;

const SKIN_SCHEME: &str = "skin";

/// Handle requests for the `skin://` custom protocol.
///
/// URL format: `skin://localhost/{skin-id}/{relative-file-path}?opacity={f64}&locked={0|1}`
///
/// - The path maps directly under the app's skins directory.
/// - HTML entry files are injected with the Tauri bridge (drag-region styles,
///   opacity, position-lock state, and the `__DESK_PP__` JS helper).
/// - All other files are served as-is with a guessed MIME type.
/// - 不限制请求来源窗口：管理器窗口也经 skin://localhost/<id>/preview.png
///   加载预览图（skin:// 本就全皮肤同源，跨窗口读取不构成新暴露面）。
pub fn handle_skin_request<R: tauri::Runtime>(
    ctx: tauri::UriSchemeContext<'_, R>,
    request: http::Request<Vec<u8>>,
) -> http::Response<Cow<'static, [u8]>> {
    let state = match ctx.app_handle().try_state::<crate::AppState>() {
        Some(state) => state,
        None => {
            log::error!("skin:// protocol: AppState not available");
            return internal_server_error("AppState not available");
        }
    };
    let skins_dir = state.skins_dir.clone();

    let uri = request.uri();
    let path = uri.path();
    let relative_path = path.strip_prefix('/').unwrap_or(path);

    // Security: reject empty paths and anything that escapes skins_dir.
    if relative_path.is_empty() || relative_path.contains("..") {
        return not_found();
    }

    // 拒绝任何含冒号的路径段：冒号只可能意味着 ADS（settings.json:$DATA）
    // 或盘符路径，都不是合法的皮肤内文件
    if relative_path.split('/').any(|seg| seg.contains(':')) {
        return not_found();
    }

    // settings.json（含 .bak/.tmp 备份/临时文件）存的是用户设置值——拦截，
    // 防止 A 皮肤经 skin:// fetch B 皮肤的设置（该暴露面由值文件引入）。
    if is_settings_file_name(relative_path.rsplit('/').next().unwrap_or("")) {
        return not_found();
    }

    let file_path = skins_dir.join(relative_path);
    let Ok(canonical_skins_dir) = skins_dir.canonicalize() else {
        return not_found();
    };
    let Ok(canonical_file_path) = file_path.canonicalize() else {
        return not_found();
    };
    if !canonical_file_path.starts_with(&canonical_skins_dir) {
        return not_found();
    }

    // 规范化后按真实文件名再拦截一次 settings.json：Windows 8.3 短名
    //（SETTIN~1.JSO）能绕过规范化前的字符串判断，canonicalize 会还原成长名
    if is_settings_file_name(
        canonical_file_path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
    ) {
        return not_found();
    }

    let bytes = match std::fs::read(&canonical_file_path) {
        Ok(b) => b,
        Err(e) => {
            log::warn!(
                "skin:// protocol: failed to read {}: {}",
                canonical_file_path.display(),
                e
            );
            return not_found();
        }
    };

    let (body, mime) = if is_html_entry(&canonical_file_path, relative_path) {
        let html = String::from_utf8_lossy(&bytes).into_owned();
        let opacity = parse_opacity(uri.query());
        let locked = parse_locked(uri.query());
        let resizable = parse_resizable(uri.query());
        let settings_json = baked_settings_json(&skins_dir, relative_path);
        let language = state.lang();
        let injected = inject_bridge(html, opacity, locked, resizable, &settings_json, &language);
        (Cow::Owned(injected.into_bytes()), "text/html")
    } else {
        let mime = guess_mime(&canonical_file_path);
        (Cow::Owned(bytes), mime)
    };

    http::Response::builder()
        .status(http::StatusCode::OK)
        .header(http::header::CONTENT_TYPE, mime)
        .header(http::header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(body)
        .unwrap_or_else(|e| {
            log::error!("skin:// protocol: failed to build response: {}", e);
            internal_server_error("failed to build response")
        })
}

/// settings.json 及其衍生文件（.bak/.tmp 等）的文件名判断，大小写不敏感。
/// 规范化前后各用一次（见 handle_skin_request）。
fn is_settings_file_name(file_name: &str) -> bool {
    let name = file_name.to_ascii_lowercase();
    name == crate::skin::settings::SETTINGS_FILENAME
        || name.starts_with(&format!("{}.", crate::skin::settings::SETTINGS_FILENAME))
}

/// Merge the skin's declared setting defaults with the persisted overrides
/// (皮肤文件夹的 settings.json) and serialize for embedding in a <script>
/// tag.  "</" is escaped so a "</script>" sequence inside a value cannot
/// close the tag early.  Missing manifest / schema errors degrade to an
/// empty object.
///
/// password 类型设置项的值恒替换为空串：skin:// 对所有皮肤同源，烘焙进
/// HTML 的值可被任意皮肤 fetch 到；password 值由 skin_get_setting 命令
/// 按窗口身份校验后单独下发（不经过本注入）。
fn baked_settings_json(
    skins_dir: &Path,
    relative_path: &str,
) -> String {
    let skin_id = relative_path.split('/').next().unwrap_or("");
    let skin_dir = skins_dir.join(skin_id);
    let values = match crate::skin::loader::load_skin_manifest(&skin_dir) {
        Ok(manifest) => {
            let overrides = crate::skin::settings::load_skin_settings(&skin_dir);
            let mut values =
                crate::skin::loader::effective_settings(&manifest, Some(&overrides));
            for def in &manifest.settings {
                if def.kind == crate::skin::types::SkinSettingKind::Password {
                    values.insert(def.key.clone(), serde_json::Value::from(""));
                }
            }
            values
        }
        Err(_) => serde_json::Map::new(),
    };
    serde_json::to_string(&values)
        .unwrap_or_else(|_| "{}".to_string())
        .replace("</", "<\\/")
}

fn is_html_entry(path: &Path, relative_path: &str) -> bool {
    // 大小写不敏感：PAGE.HTML 同样是 HTML 入口，必须注入桥
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    if matches!(ext.as_deref(), Some("html") | Some("htm")) {
        return true;
    }
    let rel = relative_path.to_ascii_lowercase();
    rel.ends_with(".html") || rel.ends_with(".htm")
}

fn parse_opacity(query: Option<&str>) -> f64 {
    let Some(q) = query else {
        return 1.0;
    };
    for pair in q.split('&') {
        let mut parts = pair.splitn(2, '=');
        if parts.next() == Some("opacity") {
            if let Some(value) = parts.next() {
                if let Ok(v) = value.parse::<f64>() {
                    // 下限与 set_skin_opacity 一致：0.0 会让窗口彻底隐形
                    return v.clamp(0.1, 1.0);
                }
            }
        }
    }
    1.0
}

fn parse_locked(query: Option<&str>) -> bool {
    parse_query_flag(query, "locked")
}

fn parse_resizable(query: Option<&str>) -> bool {
    parse_query_flag(query, "resizable")
}

fn parse_query_flag(query: Option<&str>, key: &str) -> bool {
    let Some(q) = query else {
        return false;
    };
    q.split('&').any(|pair| {
        let mut parts = pair.splitn(2, '=');
        parts.next() == Some(key) && matches!(parts.next(), Some("1") | Some("true"))
    })
}

fn inject_bridge(html: String, opacity: f64, locked: bool, resizable: bool, settings_json: &str, language: &str) -> String {
    // Lock state is baked into the injected bridge at serve time: window
    // recreation (reload / on-desktop toggle) used to restore only the
    // cursor CSS via a racy eval and lose __DESK_PP__.positionLocked,
    // silently unlocking the skin.  Serving it in the bridge makes the
    // flag correct before any user interaction.  The lock CSS uses the
    // same #desk-lock-style id as set_skin_position_locked's eval so the
    // unlock path can remove it on a live window.
    let lock_style = if locked {
        "<style id=\"desk-lock-style\">.drag-region{-webkit-app-region:no-drag!important;cursor:default!important}</style>"
    } else {
        ""
    };
    // Opacity is baked ONLY on <html>: runtime changes set
    // documentElement's inline style, which overrides this rule.  If body
    // carried the baked value too, it would survive the inline override on
    // html and keep the page dim (e.g. baked 0.5, then runtime set to 1.0
    // stayed at 0.5 until the next reload).
    // 管理器界面语言烘焙进桥：皮肤可据此让自己的界面跟随管理器语言（与锁定
    // 态同理，必须在 serve 时烘焙而非创建后 eval，防竞态）。运行时切换由
    // set_language 命令 eval 更新并派发 desk-language-changed 事件。
    let language_json = serde_json::to_string(language).unwrap_or_else(|_| "\"zh-CN\"".into());
    let bridge = format!(
        r#"<style>
.drag-region {{
  -webkit-app-region: no-drag;
  app-region: no-drag;
  cursor: grab;
}}
html {{ opacity: {opacity}; }}
</style>
{lock_style}
<script>
window.__DESK_PP__={{
  setOpacity:function(v){{document.documentElement.style.opacity=v;}},
  positionLocked: {locked},
  resizable: {resizable},
  language: {language_json},
  // 约定：password 类型设置项的值在此恒为空串——skin:// 对所有皮肤同源，
  // 烘焙值可被任意皮肤读取；password 值需经 skin_get_setting 命令获取
  //（后端按窗口身份校验后单独下发）。
  settings: {settings_json},
  invoke:function(cmd,args){{return window.__TAURI_INTERNALS__.invoke(cmd,args||{{}});}},
  // Runtime toggle for border-resize (「窗口」页开关 -> set_skin_resizable
  // evals this).  Also shows/hides the animated hazard-stripe frame marking
  // the grab area — transparent skins have no visible window edge otherwise.
  setResizable:function(on){{
    window.__DESK_PP__.resizable=!!on;
    var f=document.getElementById('desk-resize-frame');
    if(on&&!f){{
      var st=document.getElementById('desk-resize-frame-style');
      if(!st){{
        st=document.createElement('style');
        st.id='desk-resize-frame-style';
        st.textContent='#desk-resize-frame{{position:fixed;inset:0;z-index:2147483647;pointer-events:none;}}'
          +'#desk-resize-frame i{{position:absolute;display:block;overflow:hidden;}}'
          +'#desk-resize-frame b{{position:absolute;display:block;will-change:transform;}}'
          +'#desk-resize-frame .t,#desk-resize-frame .b{{left:0;right:0;height:4px;}}'
          +'#desk-resize-frame .t{{top:0;}}'
          +'#desk-resize-frame .b{{bottom:0;}}'
          +'#desk-resize-frame .l,#desk-resize-frame .r{{top:4px;bottom:4px;width:4px;}}'
          +'#desk-resize-frame .l{{left:0;}}'
          +'#desk-resize-frame .r{{right:0;}}'
          +'#desk-resize-frame .t b,#desk-resize-frame .b b{{top:0;bottom:0;left:-34px;right:-34px;background:repeating-linear-gradient(45deg,#ffd400 0 12px,#161616 12px 24px);}}'
          +'#desk-resize-frame .l b,#desk-resize-frame .r b{{left:0;right:0;top:-34px;bottom:-34px;background:repeating-linear-gradient(-45deg,#ffd400 0 12px,#161616 12px 24px);}}'
          +'#desk-resize-frame .t b{{animation:deskStripeT .9s linear infinite;}}'
          +'#desk-resize-frame .b b{{animation:deskStripeB .9s linear infinite;}}'
          +'#desk-resize-frame .l b{{animation:deskStripeL .9s linear infinite;}}'
          +'#desk-resize-frame .r b{{animation:deskStripeR .9s linear infinite;}}'
          +'@keyframes deskStripeT{{to{{transform:translateX(33.94px);}}}}'
          +'@keyframes deskStripeB{{to{{transform:translateX(-33.94px);}}}}'
          +'@keyframes deskStripeL{{to{{transform:translateY(-33.94px);}}}}'
          +'@keyframes deskStripeR{{to{{transform:translateY(33.94px);}}}}'
          +'@media (prefers-reduced-motion: reduce){{#desk-resize-frame b{{animation:none;}}}}';
        (document.head||document.documentElement).appendChild(st);
      }}
      f=document.createElement('div');
      f.id='desk-resize-frame';
      f.innerHTML='<i class="t"><b></b></i><i class="r"><b></b></i><i class="b"><b></b></i><i class="l"><b></b></i>';
      (document.body||document.documentElement).appendChild(f);
    }}
    if(!on){{
      if(f)f.remove();
      var st=document.getElementById('desk-resize-frame-style');
      if(st)st.remove();
      var st2=document.getElementById('desk-resize-style');
      if(st2)st2.remove();
    }}
  }}
}};
(function(){{
  // Right-click: suppress WebView2's default menu (also disabled via
  // ICoreWebView2Settings) and open the native skin menu instead.
  document.addEventListener('contextmenu', function(e){{
    e.preventDefault();
    window.__DESK_PP__.invoke('show_skin_context_menu');
  }});
  function onPointerDown(e){{
    if (e.button !== 0) return;
    if (window.__DESK_PP__.positionLocked) return;
    window.__DESK_PP__.invoke('start_skin_drag');
  }}
  function attach(){{
    document.querySelectorAll('.drag-region').forEach(function(el){{
      el.removeEventListener('pointerdown', onPointerDown);
      el.addEventListener('pointerdown', onPointerDown);
    }});
  }}
  function setup(){{
    attach();
    if (typeof MutationObserver !== 'undefined' && document.body) {{
      new MutationObserver(attach).observe(document.body, {{childList:true, subtree:true}});
    }}
  }}
  if (document.readyState === 'loading') {{
    document.addEventListener('DOMContentLoaded', setup);
  }} else {{
    setup();
  }}
}})();
(function(){{
  // Border-resize hot zones (skin.json window.resizable / 「窗口」页开关).
  // The WebView2 child window covers the entire skin window, so native
  // WM_NCHITTEST on the parent never fires — the bridge detects edge
  // proximity itself, mirrors the resize cursor via #desk-resize-style,
  // and has the backend synthesize WM_NCLBUTTONDOWN(HT*) to start the
  // system size loop (same mechanism as start_skin_drag for moves).
  // Listeners are always registered; the flag is checked per event so the
  // panel toggle (setResizable) takes effect without a reload.
  var BORDER = 6;
  var CURSORS = {{n:'ns-resize', s:'ns-resize', w:'ew-resize', e:'ew-resize',
    nw:'nwse-resize', se:'nwse-resize', ne:'nesw-resize', sw:'nesw-resize'}};
  function zoneAt(x, y) {{
    var w = window.innerWidth, h = window.innerHeight;
    var l = x < BORDER, r = x >= w - BORDER, t = y < BORDER, b = y >= h - BORDER;
    if (l && t) return 'nw'; if (r && t) return 'ne';
    if (l && b) return 'sw'; if (r && b) return 'se';
    if (l) return 'w'; if (r) return 'e'; if (t) return 'n'; if (b) return 's';
    return '';
  }}
  function setZoneCursor(zone) {{
    var style = document.getElementById('desk-resize-style');
    if (zone) {{
      if (!style) {{
        style = document.createElement('style');
        style.id = 'desk-resize-style';
        (document.head || document.documentElement).appendChild(style);
      }}
      var css = '*{{cursor:' + CURSORS[zone] + '!important}}';
      if (style.textContent !== css) style.textContent = css;
    }} else if (style) {{
      style.remove();
    }}
  }}
  document.addEventListener('pointermove', function(e){{
    if (!window.__DESK_PP__.resizable) {{ setZoneCursor(''); return; }}
    setZoneCursor(zoneAt(e.clientX, e.clientY));
  }}, true);
  document.addEventListener('pointerleave', function(){{ setZoneCursor(''); }}, true);
  document.addEventListener('pointerdown', function(e){{
    if (e.button !== 0) return;
    if (!window.__DESK_PP__.resizable) return;
    var z = zoneAt(e.clientX, e.clientY);
    if (!z) return;
    e.preventDefault();
    e.stopPropagation();
    window.__DESK_PP__.invoke('start_skin_resize', {{direction: z}});
  }}, true);
  // Sync the frame hint once the body exists (initial state comes baked
  // from the URL; runtime toggles go through setResizable directly).
  function syncFrame(){{ window.__DESK_PP__.setResizable(window.__DESK_PP__.resizable); }}
  if (document.readyState === 'loading') {{
    document.addEventListener('DOMContentLoaded', syncFrame);
  }} else {{
    syncFrame();
  }}
}})();
</script>"#
    );

    if let Some(pos) = html.find("</head>") {
        let mut out = html;
        out.insert_str(pos, &bridge);
        out
    } else if let Some(pos) = html.find("<body>") {
        let mut out = html;
        out.insert_str(pos + "<body>".len(), &format!("<head>{}</head>", bridge));
        out
    } else {
        format!(
            "<!DOCTYPE html><html><head>{}</head><body>{}</body></html>",
            bridge,
            html
        )
    }
}

fn guess_mime(path: &Path) -> &'static str {
    // 大小写不敏感（PAGE.HTML / ICON.ICO 等）
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("html") | Some("htm") => "text/html",
        Some("css") => "text/css",
        Some("js") | Some("mjs") => "application/javascript",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("ico") => "image/x-icon",
        Some("json") => "application/json",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("ogg") => "audio/ogg",
        Some("flac") => "audio/flac",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        _ => "application/octet-stream",
    }
}

fn not_found() -> http::Response<Cow<'static, [u8]>> {
    http::Response::builder()
        .status(http::StatusCode::NOT_FOUND)
        .header(http::header::CONTENT_TYPE, "text/plain")
        .header(http::header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Cow::Borrowed(b"Not found" as &[u8]))
        .unwrap()
}

fn internal_server_error(msg: &str) -> http::Response<Cow<'static, [u8]>> {
    http::Response::builder()
        .status(http::StatusCode::INTERNAL_SERVER_ERROR)
        .header(http::header::CONTENT_TYPE, "text/plain")
        .header(http::header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Cow::Owned(msg.as_bytes().to_vec()))
        .unwrap()
}

/// Re-export the scheme name for consumers.
pub fn scheme() -> &'static str {
    SKIN_SCHEME
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opacity_clamps_like_the_setter() {
        // 下限与 set_skin_opacity 统一为 0.1：0.0 会让窗口彻底隐形
        assert_eq!(parse_opacity(Some("opacity=0")), 0.1);
        assert_eq!(parse_opacity(Some("opacity=0.05")), 0.1);
        assert_eq!(parse_opacity(Some("opacity=0.5")), 0.5);
        assert_eq!(parse_opacity(Some("opacity=2")), 1.0);
        assert_eq!(parse_opacity(Some("locked=1")), 1.0);
        assert_eq!(parse_opacity(None), 1.0);
    }

    #[test]
    fn html_entry_detection_is_case_insensitive() {
        assert!(is_html_entry(Path::new("skins/a/index.html"), "a/index.html"));
        assert!(is_html_entry(Path::new("skins/a/PAGE.HTML"), "a/PAGE.HTML"));
        assert!(is_html_entry(Path::new("skins/a/Page.Htm"), "a/Page.Htm"));
        assert!(!is_html_entry(Path::new("skins/a/style.css"), "a/style.css"));
    }

    #[test]
    fn settings_file_name_matching() {
        for name in ["settings.json", "SETTINGS.JSON", "settings.json.bak", "settings.json.tmp"] {
            assert!(is_settings_file_name(name), "{} must be intercepted", name);
        }
        for name in ["skin.json", "settings.jsonx", "my-settings.json", ""] {
            assert!(!is_settings_file_name(name), "{} must pass through", name);
        }
    }

    #[test]
    fn guess_mime_covers_common_media_and_case() {
        assert_eq!(guess_mime(Path::new("a/ICON.ICO")), "image/x-icon");
        assert_eq!(guess_mime(Path::new("a/pic.avif")), "image/avif");
        assert_eq!(guess_mime(Path::new("a/song.MP3")), "audio/mpeg");
        assert_eq!(guess_mime(Path::new("a/sound.wav")), "audio/wav");
        assert_eq!(guess_mime(Path::new("a/sound.ogg")), "audio/ogg");
        assert_eq!(guess_mime(Path::new("a/song.flac")), "audio/flac");
        assert_eq!(guess_mime(Path::new("a/clip.mp4")), "video/mp4");
        assert_eq!(guess_mime(Path::new("a/clip.WEBM")), "video/webm");
        assert_eq!(guess_mime(Path::new("a/data.bin")), "application/octet-stream");
    }

    #[test]
    fn bridge_bakes_language() {
        let out = inject_bridge("<html></html>".to_string(), 1.0, false, false, "{}", "zh-CN");
        assert!(out.contains(r#"language: "zh-CN""#), "桥必须烘焙管理器语言");
        let out = inject_bridge("<html></html>".to_string(), 1.0, false, false, "{}", "en");
        assert!(out.contains(r#"language: "en""#));
    }

    #[test]
    fn baked_settings_strips_password_values() {
        let skins_dir = std::env::temp_dir().join(format!(
            "driftlet-protocol-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let skin_dir = skins_dir.join("my-skin");
        std::fs::create_dir_all(&skin_dir).unwrap();
        std::fs::write(skin_dir.join("index.html"), "<html></html>").unwrap();
        std::fs::write(
            skin_dir.join("skin.json"),
            r##"{"name":"T","settings":[
                {"key":"accent","type":"palette","default":"#ff3333"},
                {"key":"token","type":"password"}
            ]}"##,
        )
        .unwrap();
        // 用户已保存的明文 password 值
        std::fs::write(
            skin_dir.join("settings.json"),
            r##"{"accent":"#00ff00","token":"s3cret"}"##,
        )
        .unwrap();

        let json = baked_settings_json(&skins_dir, "my-skin/index.html");
        let values: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(values["accent"], "#00ff00", "非 password 值照常烘焙");
        assert_eq!(values["token"], "", "password 值必须被替换为空串");
        assert!(!json.contains("s3cret"), "明文 password 不得出现在注入内容里");

        let _ = std::fs::remove_dir_all(&skins_dir);
    }
}
