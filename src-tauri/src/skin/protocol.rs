use std::borrow::Cow;
use std::path::{Path, PathBuf};
use tauri::http;
use tauri::Manager;

const SKIN_SCHEME: &str = "skin";

/// Handle requests for the `skin://` custom protocol.
///
/// URL format: `skin://localhost/{skin-folder}/{relative-file-path}?opacity={f64}&locked={0|1}&resizable={0|1}`
///
/// - 首段是皮肤在 skins 目录下的**磁盘文件夹名**（不一定是皮肤 id——文件夹
///   直装时 id 可能是 slugify 派生值），路径直接映射到 skins 目录之下。
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
    let relative_path = decode_uri_path(uri.path());

    // 保留端点 __fs__：任意绝对路径文件直出（file_system 权限门）——
    // 皮肤 id 只允许小写字母/数字/中划线，永不与 __fs__ 撞名。
    if relative_path == "__fs__" {
        return handle_fs_reference(&ctx, uri.query());
    }

    let Some(canonical_file_path) = resolve_skin_file(&skins_dir, &relative_path) else {
        return not_found();
    };

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

    let (body, mime) = if is_html_entry(&canonical_file_path, &relative_path) {
        let html = String::from_utf8_lossy(&bytes).into_owned();
        let opacity = parse_opacity(uri.query());
        let locked = parse_locked(uri.query());
        let resizable = parse_resizable(uri.query());
        let settings_json = baked_settings_json(&skins_dir, &relative_path);
        let language = state.lang();
        let theme = crate::commands::current_theme(&state);
        let injected = inject_bridge(html, opacity, locked, resizable, &settings_json, &language, &theme);
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

/// URI 路径 → 皮肤相对路径：percent 解码 + 去前导 `/`。
/// wry 递交的 URI 保留百分号编码（tauri 自家 asset 协议同样先解码再处理）——
/// 中文 entry/资源名解码后才能命中；`..`/冒号判定也须落在解码后的语义上
///（%2e%2e 解码前不像逃逸）。
fn decode_uri_path(uri_path: &str) -> String {
    let decoded = percent_encoding::percent_decode_str(uri_path).decode_utf8_lossy();
    let path = decoded.as_ref();
    path.strip_prefix('/').unwrap_or(path).to_string()
}

/// 把皮肤相对路径解析为 skins_dir 内的真实文件路径——全部防护集中在
/// 这里：空路径、`..`、冒号路径段、settings.json 变体（规范化前后各拦一次）、
/// canonicalize 包含性校验。任一步不过返回 None（调用方统一 404，不区分
/// 原因）。抽成纯函数是让逃逸拦截这个安全不变量可被测试钉住。
fn resolve_skin_file(skins_dir: &Path, relative_path: &str) -> Option<PathBuf> {
    // Security: reject empty paths and anything that escapes skins_dir.
    if relative_path.is_empty() || relative_path.contains("..") {
        return None;
    }

    // 拒绝任何含冒号的路径段：冒号只可能意味着 ADS（settings.json:$DATA）
    // 或盘符路径，都不是合法的皮肤内文件
    if relative_path.split('/').any(|seg| seg.contains(':')) {
        return None;
    }

    // settings.json（含 .bak/.tmp 备份/临时文件）存的是用户设置值——拦截，
    // 防止 A 皮肤经 skin:// fetch B 皮肤的设置（该暴露面由值文件引入）。
    if is_settings_file_name(relative_path.rsplit('/').next().unwrap_or("")) {
        return None;
    }

    let file_path = skins_dir.join(relative_path);
    let canonical_skins_dir = skins_dir.canonicalize().ok()?;
    let canonical_file_path = file_path.canonicalize().ok()?;
    if !canonical_file_path.starts_with(&canonical_skins_dir) {
        return None;
    }

    // 规范化后按真实文件名再拦截一次 settings.json：Windows 8.3 短名
    //（SETTIN~1.JSO）能绕过规范化前的字符串判断，canonicalize 会还原成长名
    if is_settings_file_name(
        canonical_file_path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
    ) {
        return None;
    }

    Some(canonical_file_path)
}

/// settings.json 及其衍生文件（.bak/.tmp 等）的文件名判断，大小写不敏感。
/// 规范化前后各用一次（见 resolve_skin_file）。
fn is_settings_file_name(file_name: &str) -> bool {
    let name = file_name.to_ascii_lowercase();
    name == crate::skin::settings::SETTINGS_FILENAME
        || name.starts_with(&format!("{}.", crate::skin::settings::SETTINGS_FILENAME))
}

// ─── __fs__ 端点（file_system 权限门的任意路径直出）───
//
// 让页面用 URL 直接引用皮肤目录外的文件（<img src>、CSS url()、<video>）：
// `http://skin.localhost/__fs__?path=<percent 编码的绝对路径>`。命令通道
// （skin_read_any_file）走 JS 内存收发 base64，展示型引用（尤其媒体）用
// URL 直出才不经过 JS、可由浏览器流式加载与缓存。
// 安全门：身份取自 UriSchemeContext 的 webview label（可信 IPC 通道——
// Referer 可被页面伪造，绝不用）；必须是声明了 file_system 的皮肤窗口；
// 目标须为绝对路径且是存在的普通文件。此端点不碰 resolve_skin_file 的
// 沙箱逻辑——它的边界就是「声明了 file_system 的皮肤可读整盘」，与命令
// 通道 skin_read_any_file 完全同义。

fn handle_fs_reference<R: tauri::Runtime>(
    ctx: &tauri::UriSchemeContext<'_, R>,
    query: Option<&str>,
) -> http::Response<Cow<'static, [u8]>> {
    let state = match ctx.app_handle().try_state::<crate::AppState>() {
        Some(state) => state,
        None => return not_found(),
    };
    let Some(skin_id) = ctx.webview_label().strip_prefix("skin-") else {
        return not_found();
    };
    // 与 require_perm 同语义：重扫目录按 manifest 校验权限声明
    let granted = crate::skin::loader::scan_skins_directory(&state.skins_dir)
        .into_iter()
        .find(|s| s.id == skin_id)
        .map(|s| {
            s.manifest
                .permissions
                .iter()
                .any(|p| p == crate::skin_api::PERM_FILE_SYSTEM)
        })
        .unwrap_or(false);
    if !granted {
        return not_found();
    }

    let Some(file_path) = parse_fs_query(query) else {
        return not_found();
    };
    let bytes = match std::fs::read(&file_path) {
        Ok(b) => b,
        Err(_) => return not_found(),
    };
    let mime = guess_mime(&file_path);
    http::Response::builder()
        .status(http::StatusCode::OK)
        .header(http::header::CONTENT_TYPE, mime)
        .header(http::header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Cow::Owned(bytes))
        .unwrap_or_else(|_| internal_server_error("failed to build response"))
}

/// 解析 __fs__ 端点的查询串：path 参数（percent 编码）必须是存在的普通
/// 文件的绝对路径。抽成纯函数以便测试钉住这条安全不变量。
fn parse_fs_query(query: Option<&str>) -> Option<PathBuf> {
    let query = query?;
    for pair in query.split('&') {
        if let Some(v) = pair.strip_prefix("path=") {
            let decoded = percent_encoding::percent_decode_str(v)
                .decode_utf8_lossy()
                .into_owned();
            let p = PathBuf::from(&decoded);
            if p.is_absolute() && p.is_file() {
                return p.canonicalize().ok();
            }
        }
    }
    None
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

fn inject_bridge(html: String, opacity: f64, locked: bool, resizable: bool, settings_json: &str, language: &str, theme: &str) -> String {
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
    // 当前生效主题烘焙进桥（auto 已在后端折算成具体 light/dark）：皮肤据此
    // 跟随管理器昼夜配色。运行时切换由 set_theme 命令 eval 更新并派发
    // desk-theme-changed 事件。
    let theme_json = serde_json::to_string(theme).unwrap_or_else(|_| "\"light\"".into());
    // 宿主版本烘焙进桥：皮肤据此做能力探测（新控件/新命令在老宿主上自行降级），
    // 与 min_host_version 的安装期提示互补。版本号不随运行期变化，无需事件同步。
    let host_version_json =
        serde_json::to_string(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| "\"unknown\"".into());
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
  theme: {theme_json},
  hostVersion: {host_version_json},
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
// Recommended alias (same object); __DESK_PP__ is the legacy name, kept for
// compatibility.  New skins should use window.driftlet.
window.driftlet=window.__DESK_PP__;
(function(){{
  // Right-click: suppress WebView2's default menu (also disabled via
  // ICoreWebView2Settings) and open the native skin menu instead —
  // unless the page consumed the click itself: a skin that calls
  // preventDefault() on contextmenu (e.g. an in-card edit) opts out of
  // the host menu for that click.  Listening on window (last stop of
  // the bubble path) makes defaultPrevented reflect every page handler,
  // regardless of where it was registered.
  window.addEventListener('contextmenu', function(e){{
    if (e.defaultPrevented) return;
    e.preventDefault();
    window.__DESK_PP__.invoke('show_skin_context_menu');
  }});
  function onPointerDown(e){{
    if (e.button !== 0) return;
    if (window.__DESK_PP__.positionLocked) return;
    // Interactive elements keep their clicks: start_skin_drag enters the
    // system modal move loop on pointerdown, which captures the mouse and
    // eats the matching pointerup — the DOM 'click' never fires.
    var t = e.target;
    if (t && t.closest && t.closest('button,input,select,textarea,a,label,[contenteditable="true"]')) return;
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
(function(){{
  // Console forwarding to the host log: DevTools is disabled in skin
  // windows, so console output and uncaught errors would vanish otherwise.
  // Batched (one invoke per flush) + capped — a console.log in a tight
  // loop must not drown the IPC channel.  ASCII-only on purpose: the
  // bridge is injected into pages whose charset we don't control.
  var FLUSH_MS = 250, FLUSH_CAP = 30, QUEUE_CAP = 300, MSG_CAP = 1200;
  var queue = [], dropped = 0, timer = null;
  function ser(v){{
    try {{
      if (typeof v === 'string') return v;
      if (v instanceof Error) return String(v.stack || v);
      if (v === undefined) return 'undefined';
      if (typeof v === 'function') return String(v);
      var seen = [];
      return JSON.stringify(v, function(k, x){{
        if (x && typeof x === 'object') {{
          if (seen.indexOf(x) !== -1) return '[Circular]';
          seen.push(x);
        }}
        return x;
      }});
    }} catch (e) {{ return String(v); }}
  }}
  function enqueue(level, message){{
    if (message.length > MSG_CAP) message = message.slice(0, MSG_CAP) + '...';
    var last = queue[queue.length - 1];
    if (last && last.level === level && last.message === message) {{
      last.n++;
      return;
    }}
    // Hard in-queue cap: a tight loop logging DISTINCT messages would
    // otherwise grow the queue unboundedly within one flush window.
    if (queue.length >= QUEUE_CAP) {{ dropped++; return; }}
    queue.push({{level: level, message: message, n: 1}});
    if (!timer) timer = setTimeout(flush, FLUSH_MS);
  }}
  function flush(){{
    timer = null;
    var batch = queue.splice(0, FLUSH_CAP);
    if (queue.length) {{ dropped += queue.length; queue.length = 0; }}
    var entries = [];
    for (var i = 0; i < batch.length; i++) {{
      var e = batch[i];
      entries.push({{level: e.level, message: e.n > 1 ? e.message + ' (x' + e.n + ')' : e.message}});
    }}
    if (dropped) {{
      entries.push({{level: 'warn', message: '[host] dropped ' + dropped + ' console messages (flood guard)'}});
      dropped = 0;
    }}
    if (!entries.length) return;
    // Never log from inside the forwarder (recursion): send failures are
    // swallowed silently.
    try {{ window.__DESK_PP__.invoke('skin_console_log', {{entries: entries}}).catch(function(){{}}); }} catch (e) {{}}
  }}
  var LEVELS = {{log: 'info', info: 'info', debug: 'info', warn: 'warn', error: 'error'}};
  ['log','info','debug','warn','error'].forEach(function(name){{
    var orig = console[name];
    if (typeof orig !== 'function') return;
    console[name] = function(){{
      try {{ enqueue(LEVELS[name], Array.prototype.map.call(arguments, ser).join(' ')); }} catch (e) {{}}
      return orig.apply(console, arguments);
    }};
  }});
  // Capture phase: resource-error events (img/script/link) don't bubble.
  window.addEventListener('error', function(e){{
    try {{
      if (e && e.message) {{
        enqueue('error', e.message + (e.filename ? ' @ ' + e.filename + ':' + e.lineno : ''));
      }} else if (e && e.target && e.target !== window) {{
        var src = e.target.src || e.target.href;
        if (src) enqueue('error', 'resource failed: ' + src);
      }}
    }} catch (_) {{}}
  }}, true);
  window.addEventListener('unhandledrejection', function(e){{
    try {{ enqueue('error', 'unhandled rejection: ' + ser(e.reason)); }} catch (_) {{}}
  }});
  document.addEventListener('securitypolicyviolation', function(e){{
    try {{ enqueue('error', 'CSP blocked: ' + (e.blockedURI || '') + ' (' + e.violatedDirective + ')'); }} catch (_) {{}}
  }});
}})();
(function(){{
  // Dev mode (设置页「高级」开关): F12 / Ctrl+Shift+I opens DevTools for
  // this skin window.  Browser accelerator keys stay disabled (F5/Ctrl+R
  // et al.) — with them off these keys reach the page as DOM events, which
  // we forward; the backend command is the authority and no-ops while dev
  // mode is off, so this listener can live here unconditionally.  Capture
  // phase so the page cannot swallow the keys.
  window.addEventListener('keydown', function(e){{
    var hit = e.key === 'F12'
      || (e.ctrlKey && e.shiftKey && (e.key === 'I' || e.key === 'i' || e.code === 'KeyI'));
    if (!hit) return;
    e.preventDefault();
    try {{ window.__DESK_PP__.invoke('open_skin_devtools').catch(function(){{}}); }} catch (_) {{}}
  }}, true);
}})();
</script>"#
    );

    // 大小写敏感定位修复：HTML 标签可大写（</HEAD>）。不 to_lowercase()
    //（非 ASCII 字符小写化会改变字节长度，索引会漂——insert_str 落在非
    // 字符边界即 panic）；按 ASCII 大小写不敏感窗口在原文上定位（多字节
    // UTF-8 的字节全 ≥0x80，窗口起点必落在 ASCII 字节上，索引安全）。
    let find_ci = |needle: &str| {
        let h = html.as_bytes();
        let n = needle.as_bytes();
        h.windows(n.len()).position(|w| w.eq_ignore_ascii_case(n))
    };
    if let Some(pos) = find_ci("</head>") {
        let mut out = html;
        out.insert_str(pos, &bridge);
        out
    } else if let Some(pos) = find_ci("<body>") {
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
    fn resolve_skin_file_blocks_escapes_and_settings() {
        let skins_dir = std::env::temp_dir().join(format!(
            "driftlet-resolve-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let skin_dir = skins_dir.join("my-skin");
        std::fs::create_dir_all(&skin_dir).unwrap();
        std::fs::write(skin_dir.join("index.html"), "<html></html>").unwrap();
        std::fs::write(skin_dir.join("settings.json"), "{}").unwrap();
        // skins_dir 外的目标（逃逸终点）
        std::fs::write(skins_dir.parent().unwrap().join("outside.txt"), "x").unwrap();

        // 正常文件可解析
        assert!(resolve_skin_file(&skins_dir, "my-skin/index.html").is_some());
        // 空路径 / 不存在
        assert!(resolve_skin_file(&skins_dir, "").is_none());
        assert!(resolve_skin_file(&skins_dir, "my-skin/nope.png").is_none());
        // 逃逸：.. 与编码后的 %2e%2e（先经 decode_uri_path 解码）
        assert!(resolve_skin_file(&skins_dir, "../outside.txt").is_none());
        assert!(resolve_skin_file(&skins_dir, &decode_uri_path("/%2e%2e/outside.txt")).is_none());
        assert!(resolve_skin_file(&skins_dir, "my-skin/../../outside.txt").is_none());
        // 冒号路径段（ADS / 盘符）
        assert!(resolve_skin_file(&skins_dir, "my-skin/settings.json:$DATA").is_none());
        // settings.json 及其衍生（大小写不敏感）
        assert!(resolve_skin_file(&skins_dir, "my-skin/settings.json").is_none());
        assert!(resolve_skin_file(&skins_dir, "my-skin/SETTINGS.JSON").is_none());
        assert!(resolve_skin_file(&skins_dir, "my-skin/settings.json.bak").is_none());
        // 普通同名文件在子目录不受限
        std::fs::create_dir_all(skin_dir.join("sub")).unwrap();
        std::fs::write(skin_dir.join("sub/settings.json"), "{}").unwrap();
        // 注意：文件名拦截只看末段——子目录里的 settings.json 同样被拦（与
        // handle_skin_request 现行口径一致：任何位置的 settings.json 都不出协议）
        assert!(resolve_skin_file(&skins_dir, "my-skin/sub/settings.json").is_none());

        let _ = std::fs::remove_dir_all(&skins_dir);
        let _ = std::fs::remove_file(skins_dir.parent().unwrap().join("outside.txt"));
    }

    #[test]
    fn fs_query_requires_existing_absolute_file() {
        // 存在的绝对路径文件放行
        let dir = std::env::temp_dir().join(format!("driftlet-fsref-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("pic.png");
        std::fs::write(&file, b"x").unwrap();
        let abs = file.to_string_lossy().replace('\\', "/");
        let q = format!("path={}", percent_encoding::utf8_percent_encode(&abs, percent_encoding::NON_ALPHANUMERIC));
        assert!(parse_fs_query(Some(&q)).is_some(), "存在的绝对路径文件应放行");
        // 非绝对路径 / 不存在 / 目录 / 缺参数一律拒
        assert!(parse_fs_query(Some("path=notes%2Ftodo.txt")).is_none());
        assert!(parse_fs_query(Some("path=D%3A%2Fno-such-driftlet.xyz")).is_none());
        let dir_q = format!("path={}", percent_encoding::utf8_percent_encode(&dir.to_string_lossy().replace('\\', "/"), percent_encoding::NON_ALPHANUMERIC));
        assert!(parse_fs_query(Some(&dir_q)).is_none(), "目录不放行");
        assert!(parse_fs_query(None).is_none());
        assert!(parse_fs_query(Some("other=x")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bridge_bakes_language() {
        let out = inject_bridge("<html></html>".to_string(), 1.0, false, false, "{}", "zh-CN", "light");
        assert!(out.contains(r#"language: "zh-CN""#), "桥必须烘焙管理器语言");
        let out = inject_bridge("<html></html>".to_string(), 1.0, false, false, "{}", "en", "dark");
        assert!(out.contains(r#"language: "en""#));
    }

    #[test]
    fn bridge_bakes_theme() {
        let out = inject_bridge("<html></html>".to_string(), 1.0, false, false, "{}", "zh-CN", "dark");
        assert!(out.contains(r#"theme: "dark""#), "桥必须烘焙当前生效主题");
        let out = inject_bridge("<html></html>".to_string(), 1.0, false, false, "{}", "zh-CN", "light");
        assert!(out.contains(r#"theme: "light""#));
    }

    #[test]
    fn bridge_hooks_console_forwarding() {
        let out = inject_bridge("<html></html>".to_string(), 1.0, false, false, "{}", "zh-CN", "light");
        assert!(out.contains("skin_console_log"), "桥必须注入 console 转发 hook");
        assert!(out.contains("unhandledrejection"), "桥必须捕获未处理 rejection");
        assert!(out.contains("open_skin_devtools"), "桥必须注入 DevTools 快捷键 hook");
        assert!(
            out.contains(&format!("hostVersion: \"{}\"", env!("CARGO_PKG_VERSION"))),
            "桥必须烘焙宿主版本号"
        );
        assert!(
            out.contains("window.driftlet=window.__DESK_PP__"),
            "桥必须挂 driftlet 别名"
        );
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
