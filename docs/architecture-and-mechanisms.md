# Driftlet Architecture & Mechanisms Overview (Developer Guide)

> [中文版](架构与机制总览.md) | English

> This document is for developers taking over or contributing to Driftlet, and aims to be a complete mental model of the project in one read: architecture, runtime mechanisms, design decisions, and incident lessons — nothing held back. **It merges the full content of "Critical Mechanisms (do-not-regress)"** (reorganized by theme into the chapters, each entry keeping its incident rationale and do-not-regress markers); the original `docs/critical-mechanisms.md` remains the authoritative dual-version do-not-regress list — both documents stay in force, and mechanism changes must update both. A coverage map at the end proves nothing was dropped.
>
> Prerequisites: working Rust + vanilla JS; Windows desktop concepts (HWND, subclassing, DPI) are explained in place.
>
> Internal documents referenced here — `docs/设计规范.md` (design spec), `docs/实机测试清单.md` (real-machine test checklist), `docs/公开发布流程.md` (public release process), `docs/已知问题.md` (known issues), `docs/proposals/`, and the `tools/win32-probes/` probe scripts — are **dev-repo only** (never synced to the public repo).

---

## Table of Contents

1. What the project is (positioning / tech stack / data layout / two-repo split)
2. Process & window overview (host process / window inventory / capabilities / single instance)
3. Frontend architecture (no-framework decision / module table / shared widgets / i18n / theming)
4. Backend architecture (module table / AppState / lock model / command policy table / vendored patches)
5. The skin model (skin.json / validation & normalization / identity vs directory / bundled seeds)
6. The skin runtime (skin:// protocol / injected bridge / event surface / network reality)
7. Window mechanics (frameless / pin-to-desktop / drag / resize / snapping / click-through / DPI / capture / icons)
8. Skin backend API & security model (permissions / gates / command families / limits master table / SSRF)
9. Skin packages & the install pipeline (.dskin / staging / double-click / NSIS customizations / uninstall cleanup)
10. Layout backup (export / import / merge mode / crash rollback)
11. The update channel (check / racing download / hash trust / install)
12. Global features (tray / hotkeys / Focus Mode / logging / hot reload / elevation notice)
13. Testing, real-machine, and release (cargo test / checklist / public release process / signing)
14. Repository discipline (AGENTS.md hard rules / commit & CHANGELOG conventions)
15. Incident-pattern summary + critical-mechanisms coverage map

---

## 1. What the project is

**Driftlet is a Windows desktop-widget ("skin") platform**: web pages presented as desktop widgets — transparent, frameless windows, always-on-top or pinned to the desktop — loaded, configured, and arranged by a manager. Third-party creators write skins in plain HTML/CSS/JS and call native capabilities (system info, media, clipboard, files, …) through an injected bridge.

- **Tech stack**: Tauri v2 (Rust backend) + WebView2 (system runtime, not bundled) + Vite + vanilla-JS frontend (**deliberately no frontend framework** — the manager is a single-page utility UI; a framework's runtime cost and build complexity don't pay off. Shared widgets live in `src/js/dom.js` — don't create a third copy).
- **License**: GPL v3; free and open-source, resale forbidden (the ten-clause user agreement ships on the installer license page / About panel).
- **Current version**: 1.2.4. **Version numbers agree in four places** (hard rule): `package.json` / `package-lock.json` (two spots) / `src-tauri/Cargo.toml` / `src-tauri/tauri.conf.json`, plus the finalized CHANGELOG section heading (dev-repo only — the public repo's change record lives on the Releases page: the version's section lifted at release time + an English translation written then); the public repo's Release workflow has a version-gate script.
- **Data layout = portable mode**: everything lives next to the install directory — skins `<install dir>\skins\`, global config `<install dir>\config\config.json`, update downloads `<install dir>\update\`, bundled skin resources `<install dir>\bundled-skins\`; when the install dir is not writable (e.g. Program Files) everything falls back to `%APPDATA%\com.driftlet.app\` (legacy %APPDATA% configs migrate on first launch). WebView2 user data lives in `%LOCALAPPDATA%\com.driftlet.app\`.
- **Two-repo split**: the dev repo (this one) keeps everything; the public repo (xiaochengzina/Driftlet) syncs by an exclusion list (`docs/` is whitelist-based: only the skin development guide, critical mechanisms, and this overview — in their dual versions — cross over; design spec / real-machine checklist / release process / proposals / known issues stay internal). Process: `docs/公开发布流程.md` (dev-repo only).

## 2. Process & window overview

**Process model**: one Rust host process + one WebView2 renderer process tree per window. Closing the manager window destroys it (reclaiming ~80 MB of renderer; with no skins loaded the whole WebView2 process tree exits, leaving only the Rust host), and summoning it from the tray rebuilds it (page reload ~1 s) — the hide-and-suspend route (WebView2 TrySuspend) was disproven on real machines; do not invest again (see docs/已知问题.md, dev-repo only).

**Window inventory**:

| Window | label | Created by | Notes |
|---|---|---|---|
| Manager | `main` | at startup; after destroy, rebuilt by tray / skin-menu "Open skin config" / .dskin double-click | 960×640 (min 640×460), native frame without title bar |
| Skin window | `skin-<id>` | `window/factory.rs` on load/recall | frameless + Win32 subclass; except web skins (entry = URL) |
| Log window | `log` | `open_log_window` from Settings → Advanced | separate page log.html; theme/language baked into the URL query |

**Capabilities split per window** (`src-tauri/capabilities/`, three files): `default.json` attaches only to `"main"` (core:default + the minimal window-control set actually used by the manager frontend: minimize/toggle-maximize/internal-toggle-maximize/start-dragging/hide/close/show/unminimize/set-focus); `log.json` covers the log window; `skin.json` attaches to `"skin-*"` with **empty permissions** — skin pages are third-party web pages, granted no Tauri core/plugin permissions, and can only reach backend commands through the injected bridge (custom commands are not subject to capabilities ACL; the Rust-side gates guard them). The tauri.conf CSP (`default-src 'self'` etc.) applies only to app-protocol pages; skin-page responses carry no CSP (the shared-origin reality, §6).

**Single instance**: `tauri-plugin-single-instance` must stay registered **first** in the plugin list — a second instance forwards the .dskin path / recall request to the first (hot path); a cold-start double-click goes through the command line (§9).

**Tray**: left-click = show/hide the manager, right-click = the menu; if tray creation fails, closing the window degrades to a real exit (no windowless zombie process — decided by `AppState.tray_ok`).

## 3. Frontend architecture (src/)

**Decision: framework-free vanilla JS**. The manager is a multi-page Vite build (`index.html` for the manager + `log.html` for the log window — `rollupOptions.input` in `vite.config.ts`; dev port 1420 strictPort). CSS is centralized in `src/css/style.css` (design tokens + components; the spec is docs/设计规范.md, dev-repo only).

**`src/js/` module responsibilities**:

| Module | Responsibility |
|---|---|
| `app.js` | Main entry: startup sequence, nav rail/sidebar skeleton, routing (skin library / editor / panels) |
| `api.js` | Tauri IPC wrapper (thin invoke layer + event listen helpers) |
| `dom.js` | Shared DOM widgets: `esc`/`escAttr` (HTML escaping), `confirmDialog` (Esc/mask close, initial focus on Cancel, danger red vs primary blue), `bindEsc`, `closeOnMaskClick`, `bindHotkeyCapture`, `dispName` |
| `i18n.js` | UI language zh-CN/en: flat dotted-key dictionaries (372 keys per language, strictly symmetric), `t(key, params)` placeholder substitution (**no escaping** — callers `esc()` first in HTML contexts), `initI18n` reads the backend at startup, `applyLang` persists and repaints |
| `skin-list.js` | Skin library list (group folding, three badge states hidden/loaded/unloaded, previews, placeholder icons) |
| `skin-editor.js` | Skin editor: "Window" tab + "Skin Settings" tab (23 control types rendered by `renderCustomSettings`/`bindCustomSettings`) + actions area; all real-time sync listeners converge here |
| `layouts.js` | Layout-preset panel (save/apply/overwrite/rename/delete; overwrite has a confirm dialog) |
| `settings.js` | Settings panel (General/Advanced tabs: theme, language, autostart, hot reload, update settings, log entry, …) |
| `install-wizard.js` | .dskin install wizard (four states: checking → confirm → installing → done/failed; a `_gen` generation counter obsoletes stale async results) |
| `perms.js` | The single source for permission-declaration rendering (12 permissions, three tier colors; shared by wizard / editor / import review) |
| `about.js` | About panel (version / public repository / full user agreement / manual update check) |
| `update-check.js` | Startup update check and the update dialog (progress bar, channel indicator, install-now / go-to-download) |
| `focus.js` | Focus Mode panel (status card, action tier, fullscreen auto, whitelist) |
| `toast.js` | Global singleton toast (a new one replaces the old, fades out upward — don't appendChild your own) |
| `log.js` | Log-window page (listen before pull, seq-based incremental merge, per-skin filtering) |

**i18n discipline**: all UI copy goes through the `t()` dictionaries (same keys in both languages); skin schema copy (label/description/group/options) is author-provided and bypasses this module. **Key counts must stay symmetric** (currently 372 = 372).

**Theming**: CSS variables hang on `:root` (light is the default) and `:root[data-theme="dark"]`; theme option buttons carry their own `data-theme` attribute for JS.

## 4. Backend architecture (src-tauri/src/)

**Module tree (16)**:

| Module | Responsibility |
|---|---|
| `lib.rs` | Startup sequence, AppState, the 5-second maintenance timer, tray assembly, manager-window lifecycle (destroy-on-close / rebuild), cold-start args |
| `commands.rs` | Manager IPC commands (uniform first-line `require_manager`) + the `set_skin_*_impl` window-property convergence point + a large unit-test suite |
| `policy.rs` | **Command policy table**: 126 commands registered across eight gate tiers + three completeness tests (below) |
| `window/factory.rs` | Skin-window creation, the frameless subclass (`skin_subclass_proc`), HWND registries for drag/resize/snap/click-through |
| `window/registry.rs` | Loaded-skin registry (`RwLock<HashMap<id, WebviewWindow>>`) |
| `window/snap.rs` | Edge snapping (in-place RECT rewrite in WM_MOVING; threshold/candidates/raw-trajectory) |
| `desktop.rs` | Pin-to-desktop pinner (250 ms z-order adjacency watchdog — deliberately no helper window, no state machine) |
| `skin/types.rs` | Strong types: SkinManifest / SkinSettingKind (23 controls) / SkinRuntimeConfig / AppConfig |
| `skin/loader.rs` | Skin-directory scan, skin.json validation, window-defaults normalization |
| `skin/config.rs` | config.json IO (load-time normalization, atomic save_config, v1→v2 migration, orphan pruning) |
| `skin/settings.rs` | Per-skin settings.json (user overrides) IO |
| `skin/package.rs` | .dskin validation & staging install, safety limits, copy_dir_recursive protections |
| `skin/protocol.rs` | The skin:// protocol handler + the injected bridge script (`inject_bridge`) |
| `skin_api/` | Skin-callable command families: mod (bus / http_request / open_external / broadcast / window config), fs, registry, shell, status, system, gpu, volume, media, notify, audio, power, pdh |
| `tray.rs` | Tray menu (visibility submenu / layouts submenu / Focus Mode check / reload-all / quit), graceful_exit |
| `hotkey.rs` | Global hotkey (Focus Mode toggle) + per-skin visibility hotkeys (persistent registry) + the visibility-change funnel sync_tray_toggle_item |
| `focus.rs` | Focus Mode state machine and fullscreen detection |
| `update.rs` | Update check, multi-channel racing segmented download, hash verification, install handoff |
| `backup.rs` | Layout-backup export/import (incl. selective merge) |
| `app_log.rs` | In-memory log ring buffer + custom log-crate logger |
| `capture.rs` | Skin preview capture (ICoreWebView2::CapturePreview + 640 px downscale) |
| `hotreload.rs` | Skin hot-reload watcher (debug builds only) |
| `elevation.rs` | Elevated-launch notice (no auto-demotion) |
| `i18n.rs` | Backend copy dictionaries (errors/tray/dialogs, zh/en) and language normalization |

**Key AppState fields** (`lib.rs`): `registry` (skin windows), `config: Mutex<AppConfig>`, `config_dir`/`skins_dir`, `exiting`/`tray_ok: AtomicBool`, `pinner`, `main_thread_id` (Win32 subclassing must run on the window-owning thread), `pending_package` (double-clicked .dskin awaiting the frontend's idempotent pull), `pending_open_config` (skin-menu "Open config" stashed while the manager is destroyed), `hotkey_error` (registration failure awaiting a toast), `language` mirror, `toggle_item`/`skin_vis_items` (tray check-item handles), and the three locks.

**Lock model & ordering** (do not regress): `lifecycle_lock` (tokio; serializes load/unload/reload/reset and the skin_api lifecycle commands — "check-exists → register" is not atomic) → `install_lock` (tokio; serializes package install/uninstall, backup import/export, reload-all) → `settings_lock` (std; the two writers of settings.json vs directory replacement). Guards must be Send to cross .await; no reverse paths, no ABBA. Also: **never hold a lock guard across a call that re-takes the same lock** (std Mutex is non-reentrant — holding the config guard while calling rebuild_tray_menu once deadlocked); **all Mutex poisoning is recovered** via `.lock().unwrap_or_else(|e| e.into_inner())` (service continues after a panic on a locked path).

**Command registration & the policy table (policy.rs — the single source of truth for the whole attack surface)**: all **126** IPC commands register a gate tier in `COMMAND_POLICIES` — eight tiers: **ManagerOnly (64)** / LogWindow (2) / Perm (38) / AnyPerm (open_external only, 1) / ControlTarget (7) / CallerSkin (8) / SkinLabel (4) / Ungated (2). The table doesn't change runtime behavior (the gates live in each command body); its value is three completeness tests: `policies_complete_and_marked` (table ↔ the lib.rs generate_handler! list are the same set in both directions, and each command body contains the gate-marker text of its registered tier), `perm_const_names_resolve` (permission const names anchor back to the real constants), and `gate_distribution_snapshot` (per-tier counts pinned). **New-command discipline: gate on the first line of the body + one row in the table, or cargo test goes red**; a new command module must register in `policy::tests::module_source`.

**Vendored patches** (`src-tauri/vendor/`, pointed to by `[patch.crates-io]`; preserve across dependency upgrades, never overwrite): ① `tauri-runtime-wry` — the `Destroyed` patch for dead window handles (killing a process never posts WM_DESTROY), marked NOTE(driftlet); ② `tray-icon` — the 0.24 garbage-mask HICON fix + the quiet `NIM_DELETE` variant on the `TaskbarCreated` path (after an explorer restart the old icon is already dead, so deletion inevitably fails — don't error). Plus the comctl32 v6 manifest scheme in `build.rs` (the cargo-test exe once crashed at startup with `0xc0000139` due to the embedded manifest — the fix must not be taken apart).

## 5. The skin model

**A skin = a self-contained folder**: at minimum `skin.json` (metadata / window defaults / settings schema) + `index.html` + assets. Packaged form = `.dskin` (a zip with the extension renamed; skin.json at the root or inside the single first-level subfolder).

**skin.json schema** (strongly typed in `skin/types.rs`; the full reference is the skin development guide §2/§4):

- Top level: `id` (kebab-case; required for packaged distribution; folder-drop installs fall back to a slugified folder name), `name_zh`/`name_en`, `version`, `author`, `description_zh/en`, `entry` (default index.html; rejects `../\\/:`; an http(s) URL = a web skin), `permissions` (12 kinds, §8), `min_host_version` (install-time warning, never blocks), `refresh_seconds` (web skins only, ≤24 h).
- `window` defaults: width/height (300/200, clamped 1..10000), transparent (true), always_on_top/on_desktop (false/true), resizable, zoom (1.0∈[0.5,2.0]), opacity (1.0∈[0.1,1.0]), edge_snap/snap_gap (a skin may declare factory snapping; ≤200 px normalized).
- `settings[]`: 23 control types (boolean/number/stepper/text/longtext/time/date/palette/select/multiselect/radio/weekdays/font/slider/timerange/tasklist/todolist/datetime/password/datetasklist/file/directory/gpu_adapter) + group + description. **A new control type must be synced to four places**: `types.rs` (SkinSettingKind), the loader validation, `skin-editor.js` render/bind, and the pack-skin mirror (plus docs and controls-demo).
- **Symmetric copy-field naming + legacy-key aliases (do not regress)**: `name_zh/name_en`, `label_zh/label_en`, `group_zh/group_en` come in pairs; legacy unsuffixed keys (name/label/group/description) remain accepted via serde aliases. Display fallback = UI language first, the other language when missing (no bilingual switch — removed).

**Load-time validation & normalization** (loader.rs): skin.json ≤1 MB; id blacklists Windows reserved device names (con/prn/aux/nul/com1-9/lpt1-9, judged by base name); **window defaults are normalized at load** (width/height clamp, opacity fallback, zoom clamp, refresh ≤24 h — `opacity:0 + always-on-top + a giant size` makes an invisible topmost click-eating full-screen window, and the wizard never shows window defaults, so load must enforce it).

**Skin identity = id, but id ≠ folder name** (do not regress): locating a skin's directory by id must resolve through a scan (`find_skin_dir`) — never `skins_dir.join(id)`; the URL's first segment is the on-disk folder name as well. The scan dedupes by id (later duplicates skipped with a warning); a same-id-different-folder shadowing case is blocked before install.

**Runtime config** (`SkinRuntimeConfig`, stored as `skin_settings[id]` in config.json — owned by id, decoupled from skin files): geometry/placement/toggles. **The Option-follow pattern**: resizable/zoom/edge_snap/snap_gap are `Option` — None = follow the manifest default (upgraded old configs automatically follow the author's new declarations), Some = the user's explicit choice (explicit always wins); **values must be resolved to effective values in get_skin_detail before going down to the manager panel** (the panel-display incident: the isles skins' manifests declare on/20px while a bare None rendered as off/0). New fields in persisted structures must carry `#[serde(default)]` (except `Option`) — forgetting means every old config.json fails to parse wholesale and is reset as a "corrupt" file, with all skin data in the blast radius (only .bak as the backstop). Persisted entries for skins gone from disk are pruned at startup (`prune_stale_entries`).

**User setting values** live in the skin folder's `settings.json` (schema defaults + overrides merged; missing → empty, BOM tolerated, corrupt → renamed .bak, atomic writes; "Reset" = delete the file; the app never rewrites skin.json; overrides are adopted only when their type matches the schema). **password-type values are never baked into the page** (skin:// is same-origin across all skins — injected values would be readable by any skin): `baked_settings_json` always substitutes an empty string, and skins read them via `skin_get_setting` dispensed per window identity. **settings.json (incl. .bak/.tmp) is never served over skin://** (double interception: by URL filename, and again by the canonicalized real name — defeating 8.3 short names like `SETTIN~1.JSO` and ADS `::$DATA` bypasses); assetProtocol was deleted wholesale (its scope allows by prefix and can't fence individual files — it once exposed settings.json to every window). **settings.json never ships in packages** (pack-skin excludes it).

**The six bundled isles skins** (isles-countdown/calendar/clock/monitor/weather/timer): sources live in `examples/`, packed into the installer via bundle.resources and landing in `bundled-skins/`; a **one-time seed** (`AppConfig.bundled_skins_seeded`) — copies in only what's missing (deleted skins never resurrect, user-updated ones never overwritten), and old-version upgrades seed once as well. During development `copy_example_skins` syncs only examples/ (demos/ sync was stopped).

## 6. The skin runtime: the skin:// protocol & the injected bridge

Skin pages and assets load through the `skin://` custom protocol (`skin/protocol.rs`). Its job is not to limit the network but to provide four control points:

1. **Bridge injection**: serving HTML bakes `__DESK_PP__` (settings/opacity/positionLocked/resizable/language/theme/hostVersion + right-click takeover + resize hot zones + the console hook), with the recommended alias `window.driftlet` (same object, permanently compatible). **Window state must arrive atomically via the entry URL query (opacity/locked/resizable)** — never restore it with a post-creation eval (racing page load; once caused flicker on always-on-top ↔ pin-to-desktop switches); runtime toggles do use eval (the page is loaded, no race). Settings JSON serialized into a `<script>` must escape `</` as `<\/` (or a `</script>` inside a value closes the tag early). The injected script stays pure ASCII (the page's charset is not ours to control).
2. **User-data isolation**: settings.json is always 404 (double interception, §5).
3. **Path sandbox**: canonicalize + prefix check — skins can only read inside skins_dir; dev/prod path differences are masked.
4. **The `__fs__` reserved endpoint**: `http://skin.localhost/__fs__?path=<percent-encoded absolute path>` — a skin declaring `file_system` references external files by URL (images/video without touching JS memory; the base64 channel is for data processing). Undeclared → 404; UNC always rejected (no SMB/NTLM egress).

**URL rewriting (Windows)**: WebView2 can't load subresources over non-standard protocols, so wry rewrites the navigation `skin://localhost/...` into `http://skin.localhost/...` and the request side maps it back by host. Docs and skins always use relative paths (the absolute form's first segment = the on-disk folder name).

**Network reality**: a skin page is a full Chromium — only `http://skin.*` is intercepted; external http/https goes straight to the real network; skin-page responses carry no CSP. The design premise: **a skin is local code that can access the network** — treat it under that trust model (the install wizard lists every permission).

**Boundary (known exposure)**: all skins share the single origin `http://skin.localhost` — the same-origin policy does not isolate skins (A can request B's ordinary asset files; only settings.json is fenced). password values staying out of pages exists precisely for this. If skin networking is ever to be restricted, the protocol layer is the place.

**Web skins (entry = http(s) URL)**: the window loads the site directly via `WebviewUrl::External` — no skin://, no bridge injection, so no drag regions / context menu / command channel (tauri's remote-origin guard blocks commands from remote origins); site login state persists naturally in the WebView2 cookie jar. `refresh_seconds` timed refresh applies only to this form.

**Bridge event surface (skin-side DOM events)**: `desk-setting-changed` (setting value), `desk-language-changed`/`desk-theme-changed` (UI language/theme), `desk-window-config-changed` (window property, detail={key,value}), `desk-skin-message` (inter-skin broadcast, detail={channel,from,payload}), `desk-skin-menu-item` (custom menu-item clicks). **Manager-direction events**: `skin-moved`/`skin-resized` (the drag debounce channel), `window-config-changed` (the window-property broadcast, §7.11), `skin-setting-changed` (skin-wrote → manager), `skins-visibility-changed` (the visibility funnel).

**Console flood protection (bridge side — IPC flooding is the real cost)**: console.log/info/debug/warn/error are wrapped and forwarded, plus error/unhandledrejection/securitypolicyviolation capture; the queue flushes once per 250 ms in a single invoke, adjacent duplicates merge as (xN), ≤30 entries per flush, queue hard cap 300, each message pre-truncated to 1200 chars, overflow synthesizes one flood-guard warn; exceptions inside the hook are silently swallowed (never call console from inside the hook — recursion). The batch channel `skin_console_log` (SkinLabel identity, ≤60 per batch) deliberately skips `caller_skin` (it rescans the disk every call — a sustained high-frequency channel can't rescan per batch).

## 7. Window mechanics (the Win32 layer)

### 7.1 The frameless mechanism (four lines of defense, do not regress)

The root causes and defenses behind the "classic title bar sometimes appears" bug: ① `SetWindowSubclass` **must be called on the window-owning thread** (when load/reload runs on a spawn_blocking worker, the cross-thread call fails silently — go through `run_on_main_thread`); ② **never `DwmExtendFrameIntoClientArea(-1)`** (glass extension draws the DWM frame across the whole client area); ③ send `SWP_FRAMECHANGED` only when the style is genuinely dirty (sending unconditionally keeps giving the frame chances to reappear); ④ tao's `to_window_styles()` always includes `WS_CAPTION` for top-level windows and every set_window_flags routes through it — the vendored patch plus our own subclass (`force_frameless`, the WM_STYLECHANGING/CHANGED strippers) keep it off.

### 7.2 Pinning to the desktop (the desktop.rs pinner)

- **What Win+D really does**: skins **are not minimized** (the frameless style strips WS_MINIMIZEBOX, and minimize-all only targets minimizable windows); a skin's "disappearance" is **z-order occlusion**.
- **Current scheme = continuous z-order re-attachment**: a 250 ms watchdog checks every pinned skin is in place (the test: **immediately below it sits the icon host or another pinned skin** — with multiple skins, strict "directly above the host" can't hold for all of them at once); anything off gets repaired. **Deliberately no Show-Desktop state machine and no helper window** — re-verifying every tick self-heals (the old state machine once stuck after a restore; it also heals explorer.exe restarts).
- Icon-host lookup uses **no version heuristics** (both forms are accepted: Progman directly containing DefView, and a top-level WorkerW containing DefView).
- The z-order must sit **immediately above** the icon window — below it, the desktop ListView (SysListView32) swallows every click.
- When pinned, set `WS_EX_TOOLWINDOW` and clear `WS_EX_APPWINDOW` (invisible to taskbar/Alt+Tab, double insurance); when unpinning **clear both bits** (once only one was cleared). Fall back to HWND_TOP when topping z-order; verify the window's process ownership before touching it (never act on a recycled hwnd).
- "Always on top" and "pin to desktop" are mutually exclusive, exactly one always on (pin by default); config load normalizes both-on/both-off to pin (`normalize_mode_flags`, including the wallpaper_layer migration).

### 7.3 Position lock

The toggle is the bridged `__DESK_PP__.positionLocked`; baked at creation via the URL query `locked=1` (never a post-creation eval); runtime toggles go through `set_skin_position_locked`'s eval; the lock CSS uses the uniform `#desk-lock-style` id.

### 7.4 Skin dragging & the context menu

- No more `-webkit-app-region: drag` (Windows treats it as a title bar and pops the system menu on right-click). `.drag-region` gets a pointerdown binding from the injected script → `start_skin_drag` enters the system modal move loop. **A press landing on an interactive element (`button/input/select/textarea/a/label/[contenteditable="true"]`) must skip the drag** — the modal loop captures the mouse at pointerdown, the matching pointerup is eaten, and the DOM click never fires (in 1.0 controls-demo had no in-window buttons, so the bug stayed latent until a skin with buttons appeared).
- Right-click = a self-drawn native menu (open config / refresh / **the window-behavior quick-toggle section** (place-on-top / lock position / click-through / resize-by-dragging / edge snapping; MF_CHECKED states are a snapshot taken when the menu opens, and clicks dispatch to the matching `set_skin_*_impl`) / the custom-items section (registered by the skin via `skin_set_menu_items`: ≤8 items, id ≤32, label ≤40, checkable, bilingual labels, cleared on unload) / hide / unload). WebView2's default menu is disabled via `SetAreDefaultContextMenusEnabled(false)` — the first call at creation always fails (WebView2 isn't ready); a retry thread (~6 s) plus the 5 s maintenance timer cover it — **do not go back to a single call at creation**.
- The right-click event is checked for `defaultPrevented` at the end of bubbling — a skin handling `oncontextmenu` itself on an element (stopPropagation + preventDefault) suppresses the host menu there.

### 7.5 Border drag-resize (resizable, tri-state)

- **The hot zone must live in the JS bridge layer, not the subclass's WM_NCHITTEST** — the WebView2 child window fills the whole skin window, so the parent (the subclassed tao window) never receives hit tests (implemented that way once: even the frameless cursor couldn't drag; reverted).
- The chain: effective value → baked at creation as `resizable=1` → the bridge script binds 6 px edge hot zones (cursor shapes + `start_skin_resize` synthesized messages, same mechanism as dragging) → the modal resize loop. Runtime toggles = `set_skin_resizable` evals `__DESK_PP__.setResizable(on)` + the frame hint layer (`#desk-resize-frame`, four 4 px yellow-black diagonal-stripe edges).
- **Size/position persistence stays in the backend Moved/Resized events** (never move it back to the frontend — that listener is only registered while the config page is open, which once caused "drags aren't saved while the panel is closed"): physical ÷ **the creation-time snapshot scale** (`SCALE_SNAPSHOTS`: physical client width ÷ logical width measured at creation — the old layout scale (current physical ÷ stored logical) made the conversion equal the stored value on every event during a resize, so border-drag sizes were never persisted); in-memory config updates immediately + 500 ms debounced disk write (`debounced_config_flush`) + `emit_to("main", "skin-moved"/"skin-resized")` refreshes the panel inputs (panel loop-back events carry the same value and are skipped, not written).
- The subclass's `WM_GETMINMAXINFO` sets a 60×40 minimum (DPI-scaled) and must stay — the modal resize loop reads it no matter who triggered it.
- Config values are **always logical pixels** (creation/set/readback follow one convention); any path feeding a config value into PhysicalSize/PhysicalPosition diverges the actual size from the panel at DPI ≠100% and "restores" after reload.

### 7.6 Edge snapping (snap.rs)

- **The hook is the subclass's `WM_MOVING`** (not Moved + set_position — that loops back and the trajectory is jagged). Each step of the system modal move loop passes the proposed screen RECT in, and it is rewritten in place.
- **Snap state is registered per HWND** (`SNAP_WINDOWS` — only the HWND is available inside the subclass callback): upsert at creation, unregister at destroy (HWNDs get recycled by the system — a leftover entry would misread unrelated windows as snap candidates).
- **Snapping = pure distance**: per axis, screen candidates first — |delta| ≤ threshold (`SNAP_THRESHOLD` = 10 logical px, converted to physical at the window's DPI) wins (the screen beats a window even if the window is closer); otherwise the smallest-|delta| window candidate (four-edge alignment + adjacent abutting, requiring overlap or ≤threshold spacing in the perpendicular direction to prevent phantom snaps; oversized windows are excluded from screen candidates).
- **Slow-drag detachment = the raw drag trajectory**: the WM_MOVING proposed rect is a delta relative to "the last written-back value" — after snapping, a slow drag keeps proposing inside the threshold zone and the window gets "stuck". Snap evaluation now tracks the raw trajectory accumulated from mouse deltas: dragging slowly out of the threshold zone detaches smoothly (verified with injected on-machine trajectories, incl. the regression test `slow_drag_detaches_once_raw_leaves_threshold_zone`). **The escape window (1 s of snap immunity on re-grab) was removed along with the fix — do not add it back**; fine adjustment now uses the coordinate inputs on the "Window" tab.
- The gap `snap_gap` (logical px, clamp 0–200 = `MAX_SNAP_GAP`) applies to both screen edges and window abutting (alignment candidates don't include the gap); the Option-follow pattern is in §5.

### 7.7 Click-through (restored — the old root cause was our own subclass stripping the bits, not the style bits being ineffective)

- Mechanism: tao's `set_ignore_cursor_events(true)` sets `WS_EX_TRANSPARENT|WS_EX_LAYERED`. **The real root cause of the two failed attempts back then**: our frameless subclass's three stripping sites (force_frameless / WM_STYLECHANGING / WM_STYLECHANGED) unconditionally removed those two bits, with the 5 s self-heal timer re-stripping as a backstop.
- The crux: click-through windows are **registered per HWND** (`PASSTHROUGH_HWNDS`), and the three stripping sites use a preserving mask for registered windows; **ordering matters: register first, then call set_ignore_cursor_events** (tao lands the SetWindowLongPtr via execute_in_thread, which fires the style messages — an unregistered window gets stripped right back); the destroy path must unregister (same HWND-recycling discipline as snapping).
- Residual risk: "LAYERED breaks rendering" can't be disproven once for all environments (old WebView2 runtimes, exotic GPU paths) — the per-skin toggle, default off, is the circuit breaker. Past probe conclusions (tools/win32-probes/ctprobe.ps1, dev-repo only — don't retry): a top-level non-layered TRANSPARENT is ignored by hit-testing; WorkerW sibling transparent hit-skipping only works for same-thread windows; subclass-returned HTTRANSPARENT is a dead end.
- `click_through` defaults off; `set_skin_click_through` flips in place when loaded, persists-only when not.

### 7.8 DPI, virtual displays, and the wallpaper layer

- **The black-background root cause: WebView2 visual-tree misplacement on inconsistent-DPI virtual displays** (proven on a GameViewer virtual screen + Win10 21H2 @125%): remote-desktop software's virtual displays (GameViewer/Sunlogin and other IddCx screens) report contradictory DPIs, and WebView2's rasterization scale misaligns. Platform-side detection/handling lives in the code; **skin-author side**: sizes are always logical pixels (§7.5).
- **The wallpaper layer is removed** (SetParent-ing the skin into WorkerW/Progman as a child window = below desktop icons, above the wallpaper, physically mouse-immune): it depended on a full set of undocumented internals (the Progman/WorkerW/DefView class names, the 0x052C message) — too fragile, abandoned wholesale, with its need now served by pin-to-desktop + click-through. Old `wallpaper_layer:true` configs migrate to pin-to-desktop via normalize_mode_flags (the field is deprecated, kept only for migration); leftover black holes in the wallpaper surface get a one-time forced repaint (`repaint_wallpaper_surfaces_once`). **Kept**: the `Destroyed` patch for dead window handles (killed processes never post WM_DESTROY) in vendored tauri-runtime-wry.

### 7.9 Preview capture (capture.rs)

- `capture_skin_preview` **must stay an async fn** — non-async commands run on the main thread in Tauri 2, and capture blocks in `recv_timeout` waiting for the WebView2 CapturePreview completion callback (a sync command would freeze the main thread; the same constraint covers run_command, the three SMTC commands, open_log_window, …).
- Capture goes through `ICoreWebView2::CapturePreview` — **never go back to PrintWindow + GDI** (undefined alpha, black backgrounds on transparent skins, and total failure when covered/pinned/minimized).
- **Downscale to a longest side ≤640 px before hitting disk**: the manager card's display area is only 226×96 CSS px, while a full-size PNG from a big skin decodes to ~8 MB of RGBA resident in the manager renderer per visible preview. Any failure falls back to writing the original (oversized is a memory tradeoff; missing breaks "installed → preview visible"). Creator-shipped preview.png is covered by a second line: install/packaging validate a longest side ≤1280 px (header-only parse — PNG IHDR / JPEG SOF; unparseable headers pass), mirrored by hand in pack-skin (changing either side requires syncing and rebuilding the exe).

### 7.10 Window icons

Tao 0.35.x's `RgbaIcon::into_windows_icon()` is the same buggy code as the tray-icon garbage-mask issue, and tauri-codegen bakes icons/icon.ico as the default_window_icon applied to every window — Task Manager/Alt+Tab would pick up exactly that garbage-mask HICON. So `run()` explicitly calls `set_default_window_icon(None)`; window icons are instead set by `apply_window_icon()` — `CreateIconFromResourceEx` on the multi-size icon.ico + `WM_SETICON` (called for both the manager and the log window): the exe-resource fallback does **not** cover the taskbar button / hover preview / Alt+Tab (observed the generic default icon there), so it cannot be relied on. **Do not** call `.icon()/set_icon()` on windows or remove the suppression line. Tray icons are separate per-size PNGs (tray.rs embeds 7 sizes, 16–48, rendered per-size by tools/make-tray-icon.py from grid-aligned parameterized geometry — never a composed ico, never downscaled from a big image).

### 7.11 Data flow & off-screen rescue

- **Dragged-position saving stays in the backend Moved event** (§7.5). Window-property changes broadcast through the single funnel `emit_window_config_changed` (**do not bypass**): every path mutating a window property (manager panel / right-click quick toggles / skin self-control `skin_set_window_config`) lands in a `set_skin_*_impl` and thereby passes through it — it **dispatches twice**: `emit_to("main", "window-config-changed", { skinId, key, value })` (regardless of loaded state — an open editor syncs in place via skin-editor.js; an input being edited is protected by focus and never clobbered) +, when the skin is loaded, an eval dispatch of `desk-window-config-changed`. Incident behind this: the event used to go only to the skin — toggling edge snapping from the right-click menu left the "Window" tab's checkbox stale. Completeness is pinned by the source-scan test `window_config_setters_all_broadcast` (all 10 impls broadcast; a miss goes red). Drag/border-resize position/size changes take the Moved/Resized debounce channel, not this one.
- **Rescuing fully off-screen skins** (after unplugging an external monitor / a DPI topology change, a skin can land outside every display — invisible and undraggable): the check is `factory::offscreen_target` — a skin counts as off-screen only when its rectangle intersects **no** monitor work area (physical pixels); **partially off-screen is never touched** (multi-monitor seams and deliberate half-hiding are legitimate). Off-screen skins are moved to the primary monitor's work area (top-left +24 px; persisted through the Moved debounce). Two hook points: `rescue_offscreen_skins` in lib.rs's 5-second maintenance timer (topology changes self-heal within 5 s, and the first tick covers the startup auto-load — no WM_DISPLAYCHANGE hook needed) + the config page's "Bring on screen" 「复位到屏幕内」 button via `bring_skin_onscreen`.

### 7.12 Manager/log window frames & frontend behavioral constraints

- **Native frame without a title bar**: after a frameless build (`decorations(false)`), `apply_native_frame` does three things — strips WS_CAPTION and adds back WS_THICKFRAME|WS_MINIMIZEBOX etc.; the title-bar area is self-drawn (dragging goes through tauri's injected drag.js with `data-tauri-drag-region="deep"` + `plugin:window|start_dragging` — default.json/log.json must grant `core:window:allow-start-dragging`, the main window additionally allow-internal-toggle-maximize; a long-missing grant once caused "the title bar can't be dragged"; skin windows use start_skin_drag, no ACL surface); corner rounding follows the system (DWM auto-rounds on Win11, square on Win10 — a product decision; the "transparent window + CSS border-radius" route was tried and abandoned). Probes: focus-frame-probe.ps1 / manager-focus-frame.ps1 in tools/win32-probes/ (dev-repo only).
- **The `[hidden]` attribute vs author display (relapsed four times — prevent the fifth)**: the UA stylesheet gives `[hidden]` only display:none; any author rule with display:block/flex/... wins — hide elements with a dedicated class or inline style; never trust hidden.
- After hide→reshow, buttons retain :hover styles (WebView2 gets no mouseleave while hidden and recomputes hover from the last known cursor position) — synthesized events can't clear it (not trusted input); only a real pointer move refreshes it.
- **Palette screen-picking = NOT provided; do not invest again (both paths proven dead)**: the eyedropper inside the native `<input type="color">` panel fails in WebView2 (field report one); a standalone EyeDropper-API button AbortError-ed instantly on runtime 152 (field report two). Both paths dead; wait for the evergreen runtime to fix itself.
- Alt+F4 on a skin window = hide, not close (WM_CLOSE → CloseRequested → prevent_close + hide + tray-check sync); **programmatic closes must register `INTENTIONAL_CLOSES`** (uninstall/reload/exit converge in `close_skin_window_nowait`, registering the label before close; the event handler consumes the registration and lets it through; a failed close revokes the registration; creation defensively clears leftovers for the same label) — in vendored tauri-runtime-wry, `close()` and a user's Alt+F4 both go through on_close_requested, and CloseRequested carries no reason.
- **`close()` returning ≠ the label is freed**: after `window.close()` in destroy_skin_window the webview takes a few more ms to actually die, and immediately re-creating with the same label reports "already exists" (right-click refresh reliably reproduced "can't come back after unload") — after close, poll/await confirmation before rebuilding.

## 8. The skin backend API (skin_api) & the security model

### 8.1 The gate model (the bridge is a passthrough — no command whitelist)

`__DESK_PP__.invoke` forwards straight to `__TAURI_INTERNALS__.invoke` — any command registered in invoke_handler is callable by skins. Therefore **sensitive commands must carry their own permission checks**, and identity is judged purely by window label:

- `require_manager` — manager commands (64): calls from skin windows always fail with "该命令仅允许管理器窗口调用" ("this command may only be called by the manager window");
- `require_perm(PERM_X)` — the skin must declare the permission in skin.json (the manifest is re-scanned on every call, so edits take effect immediately);
- `caller_skin` — identity for the permission-free baseline (sandboxed operations: own-folder files / own-schema settings / logging / broadcast);
- SkinLabel — harmless self-targeted commands (identity reverse-looked-up from the label: show_skin_context_menu / skin_set_menu_items / open_skin_devtools / skin_console_log);
- ControlTarget — the control family: self-target is permission-free (omitted/empty/own skinId = self, converged in `resolve_control_target` — a skin never learns its own id through the bridge), targeting others goes through the control gate (skin_list_skins is the exception, always control — it is inherently "looking at others");
- Ungated — only start_skin_drag / start_skin_resize (they act only on the caller's own window; not even an identity check is needed);
- web skins are blocked from commands wholesale by tauri's remote-origin guard.

### 8.2 The permission model (12 kinds; the three tiers are display-only)

High risk (red): `shell` / `system` / `file_system`; medium (yellow): `registry` / `clipboard` / `mic` / `control`; low (blue): `media` / `notify` / `sys_info` / `network` / `open_link`. **Backend enforcement stays a binary declared-or-not check** — the tiers are just wizard/editor display grading. The install wizard lists declarations one by one with tier flags. **Unknown names are ignored** (a legacy `"files"` declaration is harmless; the name `files` must never be resurrected — old skins declaring it would silently gain the new semantics; arbitrary-path access got the fresh name `file_system` for exactly that reason; `media_info` likewise merged into `media`).

### 8.3 Command-family tour (the external contract is the skin development guide §5)

- **sys_info (low, 13 read-only)**: get_cpu_info/get_gpu_info/get_memory_info/get_disks_info/get_disk_space/get_network_info/get_os_info/get_battery_info/get_monitors/get_system_theme/get_processes/get_idle_time/get_foreground_window_info. Rate-type readings = the **sampling-baseline pattern** (a static Mutex holds the sampler; the first call returns 0 as the baseline, and skins poll once per second); sysinfo 0.32 has no disk-IO stats (go through PDH `\LogicalDisk(*)\Disk Read/Write Bytes/sec`, joining by drive letter — never `\PhysicalDisk`); the GPU assembles two sources (DXGI enumeration for name/LUID/total VRAM + PDH for usage); the foreground-window title caps at 511 chars (SendMessageTimeoutW at 300 ms so a hung window can't freeze the host).
- **media (low, 7)**: get_volume/set_volume/set_mute/get_media_info/media_control/media_seek/get_audio_spectrum (system-output loopback spectrum; the microphone spectrum is separately `mic`, medium). SMTC session picking = enumerate + prefer (playing > has progress > has metadata), never GetCurrentSession; position_secs is advanced by the backend during playback (snapshot + rate × elapsed); no session returns null — not an error; the three SMTC commands must be async + spawn_blocking. Volume's COM convention: after `CoInitializeEx(MTA)`, **both S_OK and S_FALSE pair with CoUninitialize** (checking only S_OK misses one uninit); **the main thread's COM apartment belongs to tao** (never CoInitializeEx(MTA) on the main thread — tao's OleInitialize wants STA).
- **registry (medium)**: read-only; root-key whitelist HKCU/HKLM/HKCR/HKU; six value kinds (qword loses precision beyond 2^53; binary returns base64).
- **shell (high)**: `run_command` — normal privileges, never elevated; CREATE_NO_WINDOW hidden; 120 s timeout kills the process; output truncated at 1 MB; stdout/stderr read on two threads; GBK transcoded via the OEM fallback (truncation backs off to a UTF-8 char boundary); exit codes may be negative. Must be async + spawn_blocking.
- **clipboard (medium)**: read/write text via tauri-plugin-clipboard-manager (Rust-side app.clipboard()).
- **notify (low)**: `show_notification` — the Toast three-piece set, all required (startup `SetCurrentProcessExplicitAppUserModelID("Driftlet")` + the Start-menu Driftlet.lnk self-healing its PKEY_AppUserModel_ID + `CreateToastNotifierWithId` with the same ID); title/body truncated to 64/256 chars; XML's five chars escaped; the AUMID wide-char buffer is **deliberately leaked** (the property bag may read lazily — freeing is use-after-free); after CoInitializeEx inside `create_shortcut` there is never a CoUninitialize (propsys's shortcut handler heap-corrupts when the MTA apartment is destroyed — 0xc0000374 observed). The AUMID is also the taskbar icon's resolution key (the shortcut must carry SetIconLocation).
- **file_system (high, 5)**: skin_read_any_file/skin_write_any_file/skin_list_any_dir/skin_create_any_dir/skin_delete_any_path — arbitrary absolute paths, the whole disk reachable (**four no-write roots**: skins_dir / config_dir / the update dir / the exe program dir — otherwise a skin rewrites its own skin.json to silently self-elevate, or swaps the installer in the update dir to escalate "Install now" into code execution); UNC rejected in both forms plus prefix-level (the SMB/NTLM egress surface); mutation targets reject `..` (Windows resolves `..` component-wise along symlinks, not pure-lexically — `ensure_mutable_any_path` rejects `..` components first, then resolve_location canonicalizes the deepest existing ancestor); deleting a directory tree requires explicit `recursive:true`; failures pass through the raw system error; the text channel is strictly UTF-8. Display references go through the `__fs__` endpoint (§6).
- **control (medium, 8)**: skin_list_skins (always control) + skin_get_window_config/skin_set_window_config + three lifecycle + two visibility (self-target all permission-free). Window-config patches update per key; an unknown key or wrong-typed value rejects the whole patch; opacity/x,y/width,height/position_locked/resizable require the target loaded (runtime operations need a window), the rest persist-only when unloaded; zoom applies before width/height in the same batch; one-sided x/y or width/height merge the current value; **config mutations dispatch per-key into the manager commands' `set_skin_*_impl` in-process implementations** (same path — never copy a second one in skin_api); the three lifecycle commands are fire-and-forget when self-targeted (the calling webview dies immediately — the return value is not dependable).
- **network (low)**: `http_request` — read any public URL's response beyond CORS, custom methods/headers (six-method whitelist GET/POST/PUT/PATCH/DELETE/HEAD), timeout default 15 s clamped 1–60, response body truncated at 4 MB, base64 both ways with `binary:true`, HTTP 4xx/5xx return the status code without rejecting (only transport failures reject). **The SSRF line**: localhost/loopback/link-local/private/unspecified/broadcast all rejected (incl. inet_aton numeric literal forms, restored by `parse_inet_aton` and checked by the same rules — WHATWG parsing normalizes them first; this function is defense-in-depth); **redirects re-checked per hop** (`redirects(0)` + manual following, re-validating scheme and private-host per hop, capped at 3 — never go back to ureq auto-follow; a 302 springboard once bypassed the first-hop validation wholesale, audit H1); request headers filtered per hop by `headers_for_hop` (cross-origin strips authorization/cookie/proxy-authorization; a GET-converted hop also strips content-length/content-type — otherwise a skin's token leaks to third-party hosts with the 302, re-review D-A).
- **open_link (low) / system (high)**: `open_external` uses `ShellExecuteW` (the shell plugin's open is deprecated — don't go back); URI whitelist http(s)/mailto/ms-settings — http(s) passes either open_link or system (`require_any_perm`, denial reports the first permission name, low-risk first), mailto/ms-settings require system; local paths always rejected (the local-path surface was cut). Tier decision = the pure function `open_external_required_perms` (pinned by tests). system additionally covers the five power commands (`power.rs`, deliberately no path/target parameters): lock_workstation / monitor_off (PostMessage broadcasts SC_MONITORPOWER — never SendMessage) / sleep (SetSuspendState, no force) / power_control (shutdown/restart/logoff, no EWX_FORCE) / empty_recycle_bin (the system confirmation dialog; already-empty succeeds directly).
- **The permission-free baseline**: the four sandboxed skin-folder file commands (skin_read_file/skin_write_file/skin_list_dir/skin_delete_file — read ≤32 MB / write ≤16 MB, paths relative to the skin folder, rejecting absolute/`..`/colons/symlinks/DOS device names (CON once hung the main thread); root-level skin.json/settings.json* are read-only; writes auto-create parents; the text channel is UTF-8 no-BOM with strict decoding) + own-schema settings read/write (skin_get_setting/skin_set_setting — only declared keys, values pass the same `validate_custom_setting_zh` validation; after writing, `emit_to("main", "skin-setting-changed")` syncs the manager directionally) + skin_log/skin_console_log + skin_broadcast (channel 1–64 bytes, payload ≤16 KB, you receive your own broadcasts, a skin being unloaded won't receive, permission-free = unauthenticated — channel names are public to every loaded skin; never trust broadcast content).

### 8.4 Error-handling conventions

A failed command rejects with a **human-readable message** (almost always in the manager's UI language; a few parameter-validation messages — skin_broadcast / skin_set_menu_items / http_request — are hardcoded English); "no data" is expressed by return values (null or flags), only action failures reject; branch on host capabilities via `__DESK_PP__.hostVersion` numeric-segment comparison — never probe feature existence by catching errors.

### 8.5 Stability conventions (do not regress)

- **All Mutexes recover from poisoning**: `.lock().unwrap_or_else(|e| e.into_inner())`.
- **Never hold a lock guard across a call that re-takes the same lock** (non-reentrant; the lock order is in §4).
- **FFI callbacks must `catch_unwind`** (skin_subclass_proc, EnumDisplayMonitors, … — a panic must not cross the FFI boundary; the process-handle leak was fixed alongside; WM_GETMINMAXINFO defaults first, then overrides).
- **Hotkey bookkeeping always uses the `Shortcut` Display normalized string** (lowercase modifiers + keyboard-type main key) — user input, stored config, and registry keys compare on the same string, or duplicate detection breaks.
- Per-skin visibility hotkeys = a persistent registry + independent dispatch (hit takes priority over the global one): stored in `skin_settings[id].hotkey`, and `SKIN_HOTKEYS` is fully rebuilt from config at startup/after imports; a press while the skin is unloaded silently no-ops.

## 9. Skin packages & the install pipeline

**.dskin = zip** (skin.json at the root or inside the single first-level subfolder). Creators pack with `tools/pack-skin.exe` (no install, no Node/Rust needed): **it is a hand-mirrored copy of the install side's SkinManifest strong-typing validation + safety limits (256 MB / 1 GB / 10000 files)** (`tools/pack-skin/src/main.rs` ↔ `src-tauri/src/skin/types.rs`, `loader.rs` validation, `package.rs` limits) — **changes must be synced and the exe rebuilt over `tools/pack-skin.exe`** (the exe is committed so creators don't need a toolchain; the repo ships check-pack-skin-mirror.py to help). Packaging excludes: `settings.json*` (user data), .git/.svn/node_modules, existing *.dskin artifacts, .DS_Store/Thumbs.db/desktop.ini; a missing version prints a warning without blocking (update detection degrades); a non-numeric-dotted min_host_version only prints a notice; structural errors reject with line/column positions.

**Install-side validation & protection** (`skin/package.rs`): enclosed_name against zip-slip; sizes measured by **actually extracted bytes** (accumulating io::copy's return — zip headers can lie, and a zip bomb is exactly "claims small, extracts huge"); the install/update staging directory copy `copy_dir_recursive` skips symlinks and junctions (junctions aren't marked by is_symlink on Windows — detected via the reparse attribute bit) and caps depth at 32 (MAX_COPY_DEPTH); the actually-installed id must match the id parsed at the confirm page (TOCTOU closure — "confirm A's permissions, install B's content" rolls back on mismatch).

**Install = a four-step staging rollback** (`install_package`): ① copy everything into `skins/.staging-<id>`; ② rename an existing `<id>` to `.<id>.old`; ③ rename staging into place (on failure, rename .old back); ④ restore settings.json from .old, then delete .old. No half-swapped directory survives any failure. The skin scan skips dot-prefixed directories (staging/.old are never listed). Updating a running skin: unload first, and it **stays unloaded** after install (product decision: updates/reinstalls/rollbacks never auto-resume). The file picker filters .dskin only.

**Double-click install (file association)**: `bundle.fileAssociations` in `tauri.conf.json`, written to the registry by the NSIS installer — only the installed build has the double-click entry (portable exe/dev don't, but the install logic is fully testable via a CLI arg: `npm run tauri dev -- -- "D:\path\x.dskin"`). **The cold/hot entry paths cannot be merged**: cold start = `dskin_arg(std::env::args_os().map(to_string_lossy))` (`args()` panics outright on non-UTF-8 args — a release build has no console, so that's a silent crash; skips argv[0], case-insensitive extension, the file must exist) → stored in `AppState.pending_package`, the main window shows immediately, and the frontend **pulls** once ready via `take_pending_package_install` (Mutex::take consume-style, pops only once; **you cannot emit an event here** — the frontend listener isn't registered yet; emitting loses it); hot start = the single-instance callback (plugin registered first) → `tray::show_manager_window` + emit. Known window: a second double-click within the few hundred ms before the frontend is ready may be lost (the first package's wizard shows) — acceptable. **Install serialization = install_lock** (repeated double-clicks would otherwise race remove_dir_all vs copy on the same skin dir). The wizard `install-wizard.js` has four states + a `_gen` generation counter obsoleting stale async flows. The .dskin icon = `src-tauri/icons/dskin.ico` (10 frames 16–256 scaled from icon/dskin-icon.png, packed by tools/make-ico.cjs).

**NSIS installer customizations** (`src-tauri/windows/installer.nsi` — a self-maintained template taken from the tauri-cli v2.11.4 tag; **upgrading @tauri-apps/cli requires re-syncing this template**): ① one package, two languages — `nsis.languages = ["English", "SimpChinese"]` — runtime auto-matches the system UI language, falling back to the array's **first** entry (English must come first); a hook writes `$LANGUAGE` to the registry for the uninstaller's `MUI_UNGETLANGUAGE` (without it, the two-language build pops a language picker at uninstall). ② The finish page's "Run at startup" checkbox (a third slot beyond MUI2's native RUN/SHOWREADME, added at custom page coordinates). ③ The welcome text replaced with a custom LangString (the stock "close all other applications first" claim is irrelevant here). ④ The license page resolves per install language (Chinese installers get installer-license-zh.txt in pure Chinese, everyone else installer-license-en.txt — **both files MUST be UTF-8 with BOM**: without it makensis compiles silently but the runtime decodes as ANSI, garbling all Chinese; the template hardwires `MUI_PAGE_LICENSE` straight at the repo files, a relative path whose depth is constant but coupled to the cli's bundler layout); the About panel's "User Agreement → View" shares the same pair of files (`get_user_agreement` picks by UI language, guarded by a dual-version sync test in commands.rs). ⑤ The reinstall page's options are swapped ("Reinstall on top (keeps existing data)" promoted to first/default; "Uninstall the existing version first (data will be wiped)" demoted to second; the upgrade/downgrade/same-version branches' recommendation copy all replaced). ⑥ The footer BrandingText changed to `${PRODUCTNAME} ${VERSION}` (the stock `${COPYRIGHT}` was always empty at the time — no copyright config — silently falling back to NSIS's default string; the copyright field was configured later). ⑦ The WebView2 runtime floor `minimumWebview2Version` = 111 (the container-queries/color-mix baseline — the isles skins' scaling model is built entirely on them; on older runtimes the declarations fail wholesale and cards collapse): an installed-but-older runtime gets upgraded — the standard package goes online via EdgeUpdate, **the offline package upgrades it in place with the embedded offline runtime** (the upstream template only has the online path there, which always fails without network; fixed as customization ⑤ after a field report; update mode included); a runtime-side startup check backstops it (`webview2.rs`; the floor value is identical on both sides — never change just one).

**Uninstall data cleanup = the POSTUNINSTALL hook deletes everything, no checkbox**: app data lives next to the exe (portable), while the stock template only removes installed files and does a non-recursive RMDir on `$INSTDIR` (any residue = failure = the whole install dir stays) — so the hook, on non-update uninstalls, unconditionally adds `RMDir /r /REBOOTOK "$INSTDIR"` for the whole directory (incident behind this: it used to delete only config/skins plus a non-recursive root retry — a leftover downloaded installer under update/ made the root deletion fail and the whole install directory survived; hence the "install dir is always wiped" rule, user-dropped files included, /REBOOTOK covering locked files) + the %APPDATA%/%LOCALAPPDATA% data + the registry key. **The `${If} $UpdateMode <> 1` guard must never be removed** — an update install runs the old uninstaller first; without it every update would wipe user configs and skins (and in update mode the running installer itself sits in update/, so a recursive wipe would delete the running self). The stock "delete app data" checkbox was removed from the template (the original creates it unconditionally with no config switch — only a self-maintained template can do it).

## 10. Layout backup (export / import)

One-click export/import of all config + skins from the Settings page (a single zip; for machine migration and layout sharing).

- **Zip layout**: the `driftlet-backup.json` manifest (format/app/app_version/created_at) + `config/` + `skins/`.
- **Import extracts to a temp dir first and only touches the live dirs after every check passes** (same size/entry/zip-slip lines as skin packages: 5000 entries / 256 MB; config/config.json must be present; format only accepts the current value).
- **All loaded skins must be unloaded before replacement** (WebView2 locks directories; rename inevitably fails on Windows), holding install_lock against installs.
- **Crash-window startup rollback**: an existing `.import-old` means the last import was interrupted mid-flight (the success path deletes them) — the target dir may be missing/half-copied/intact, so the old copy is always moved back.
- **The post-import runtime rebuild checklist (missing one item = state drift)**: re-read config + prune_stale_entries + write back the in-memory mirrors, the language mirror + rebuild the tray menu, sync the autolaunch plugin per config.autostart, rebuild hotkeys wholesale (incl. per-skin), re-verify the Focus Mode state, and repaint the whole UI.
- **Export-side defenses**: the target must not sit inside config/ or skins/ (otherwise the zip packs the very file being written — self-containment, size explosion; judged by canonicalizing the parent dir); write a temp file first, then rename into place.
- **Selective import = merge mode**: checking skins in the review list → `import_config(skin_ids)` — each checked skin gets the three-step replacement (`.<folder>.old` yields → copy into place → old settings values retrieved), config unions in, everything else untouched; the review page shows the packaged skins' permission declarations (same conventions as the install wizard).

## 11. The update channel (update.rs)

- **Startup check** (on by default, can be turned off entirely in Settings; once per process): `check_update` queries the public repo's GitHub releases API (UA = `Driftlet/<version>`, nothing but the UA), numeric-segment version comparison; network failures stay silent and never interrupt startup. **A new version pops the dialog immediately** (no waiting for the download); during the download a progress bar + channel indicator shows; when done the primary button becomes "Install now"; only if every channel fails does it degrade to "Go to download page".
- **Trust model (do not regress)**: the version number and the official SHA-256 come **only from api.github.com** — the hash is parsed from the release notes' `SHA256: <hex>` line (mandated by the release process); download links are pinned to the public repo's release-download domain by prefix check (only link shapes returned by the GitHub API assets are accepted); mirror channels are enabled only when the release notes carry the official hash; a mismatching on-disk hash is discarded immediately.
- **Multi-channel racing + segmented parallelism**: direct GitHub and acceleration mirrors race — first finisher wins; each source splits into 8 Range segments in parallel (a single connection to GitHub is often throttled to a dozen KB/s domestically — segmentation buys about an order of magnitude); the first request's probe Range doubles as total-size discovery via content-range (206 → preallocate + offset writes); progress throttled to 500 ms; layered timeouts 60 s/10 s/15 s.
- **No re-download**: once the same version's installer finished downloading — or "Later" was clicked — restarts don't re-download; "Install now" is offered directly (a version marker + `.part` segment bookkeeping).
- **Install**: `install_update` (require_manager) launches the installer from the fixed directory (only the one we just downloaded — arbitrary paths refused) — before executing it re-verifies the marker's integrity + the version is newer + the file hash matches, refusing on any miss (the user's confirmed "install the official update" must never execute a third-party-rewritten installer); then `tray::graceful_exit` exits the whole app (NSIS's CheckIfAppRunning takes over).
- **Three gates against junk piling up**: fixed filename overwrite + the `.s<source>.part` segments wiped at start/failure + startup cleanup (current version ≥ marker version = installed → delete the installer and the marker).
- **The update directory is one of file_system's four no-write roots** (audit H2).

## 12. Global features

### 12.1 The tray (tray.rs)

Menu = Show manager / Reload all / **the Focus Mode check** / the Skin Visibility submenu (one checkable row per loaded skin, click toggles) / the Layouts submenu (click applies) / Quit. The visibility submenu is maintained in two layers: the menu rebuilds only when the load set changes; pure visibility flips just set_checked by real visibility (hammering the hotkey doesn't jitter the menu). **All visibility changes funnel through `hotkey::sync_tray_toggle_item`** (the global hotkey / tray checks / Alt+F4 degraded-hiding / editor buttons / load/reload wrap-ups all pass through) — which also emits `skins-visibility-changed` to the manager, so the "Hidden" 「已隐藏」 badge reflects the real window state, not hotkey bookkeeping. Tray icons are per-size PNGs (§7.10).

### 12.2 Hotkeys (hotkey.rs)

- **The global hotkey = the Focus Mode toggle** (default Ctrl+Shift+Alt+D, changeable or disabled in the Focus Mode panel; upgraded in 1.2.4 from "hide/show all skins" — with the default action tier "Hide" the behavior matches the old one, and whitelisted skins are no longer affected). A registration failure (combo occupied) is stored in `hotkey_error`; the frontend pulls it once at init for a toast (never silent).
- **Per-skin visibility hotkeys**: each skin can record a combo in the editor (saving is refused on duplicates with the global hotkey / another skin's combo / a system-occupied one); a persistent registry dispatches independently, with hits taking priority over the global; bookkeeping always uses the Shortcut Display normalized string.

### 12.3 Focus Mode (focus.rs)

One click (hotkey / tray / panel) — or fullscreen auto — enters a do-not-disturb desktop: every non-whitelisted skin is closed per the action tier, and exit restores from the entry snapshot:

- **Action tiers**: Hide (default; instant restore, state kept) / Unload (reclaims all memory; restoring reloads); changing the tier mid-mode doesn't affect the current restore (it follows the entry tier).
- **Whitelist**: per-skin checkboxes in the panel exempt skins from both tiers.
- **Fullscreen auto-enter** (default off): a 2.5 s poll detects fullscreen apps (games / fullscreen video, incl. exclusive fullscreen; tolerance ≤4 px), and auto-exits with a 5 s debounce after fullscreen ends; manually exiting during fullscreen suppresses auto-enter for that fullscreen period.
- **Crash safety**: mode state + entry tier + snapshot persist in `config.focus_mode` — if the app exits/crashes while the mode is active, the next startup restores per the snapshot; the load set is never lost.
- Entry affordance: the nav-rail "Focus Mode" 「专注模式」 item carries a steady beacon while the mode is active. The settled design: `docs/proposals/专注模式方案-2026-09.md` (dev-repo only).

### 12.4 Logging (app_log.rs + the log window)

The backend buffer always records (a `VecDeque<LogEntry>` ring buffer, 1000 entries evicting the oldest, each message truncated at 1000 chars, each carrying a monotonic seq); the frontend pulls on demand. Capture scope = a custom log-crate logger (only records whose target starts with `driftlet_lib` — wry/tao and other third-party noise stays out) + the skin-side console-hook forwarding (flood parameters in §6) + explicit `skin_log` (level accepts only warn/error; everything else maps to info). Operation-event hook points (Info level, stderr only — never reaches the window): manager started / destroy-on-close and no-tray exit / app exit / hotkey triggered (**never move it into `focus::toggle`** — the tray menu's toggle calls the same function and would be misrecorded) / skin loaded-unloaded (the impl convergence points) / setting changes (one line on each side — **key only, never the value**: password-type values must not appear even on stderr). The log window: created by `open_log_window` from Settings → Advanced (require_manager; re-opening an open window just shows/focuses it) — **it must be async + spawn_blocking window creation** (a sync command runs in the main thread's WebView2 IPC callback context, and building a window there deadlocks the main thread — reproduced on a real machine); theme/language are baked into the URL query (the log window can't pass get_app_config's require_manager); the frontend **listens first, then invokes get_app_log** (the reverse order loses entries inside the window), dedupes by seq, then goes purely incremental; `get_app_log`/`clear_app_log` are gated by `require_log_window` on the label (the buffer contains internal paths and other skins' messages — skins must not read it).

### 12.5 Development mode: skin hot reload (hotreload.rs)

**Debug builds only** (gated in setup behind `cfg!(debug_assertions)`) + a Settings toggle (`config.hot_reload`, default off). A single notify watcher recursively watches the whole skins dir, debounces 300 ms per skin folder, and reuses `reload_skin_impl`; **filtering out our own writes is the crux**: settings.json (and .tmp/.bak), preview.png, .staging-*/*.old never trigger a reload, or it loops forever — **any new app-self write into a skin folder must be registered in the filter table**. Reloads must be spawned (the old window's label frees only after the event loop processes close — a synchronous destroy+create on the same thread hits "already exists").

### 12.6 The elevated-launch notice (elevation.rs)

The app needs no administrator rights (manifest asInvoker; every capability works as a standard user). At the very top of `run()` (before Builder/single-instance) it checks `TokenElevation` (not "is the user an admin" — a normal launch by an admin under UAC doesn't trigger); when elevated it only **notifies once** (MessageBoxW — the windowing system isn't up yet; language from an early read of config.json) explaining the consequences (shell-permissioned skins can silently run commands with full admin rights; Explorer → manager .dskin drags are blocked by UIPI) — "Yes" writes `allow_elevated` as a persistent opt-out, "No" exits. **No demotion is attempted at all** — both routes were tried in the field and retired for good, do not resurrect: ① scheduled-task relaunch (trips behavioral AV detection Behavior:Win32/Execution.A!ml, and is provably useless for genuine Administrator accounts); ② SAFER restricted token + CreateProcessWithTokenW/AsUserW (a standard user elevated via UAC has neither of the required admin privileges in their token — both arms observed failing on a real machine). Leftover tasks and the `--demote-cleanup` argument from the 1.1.2 era were removed together with the demotion machinery. `DRIFTLET_ALLOW_ELEVATED=1` and the `allow_elevated` field are both opt-outs that skip the notice. `elevation::enable_privilege` stays as the shared privilege-enabling helper (an Ok from AdjustTokenPrivileges still requires checking GetLastError ≠ ERROR_NOT_ALL_ASSIGNED) — skin_api's sleep/power_control enable SE_SHUTDOWN_NAME through it; never copy a third implementation.

### 12.7 The manager "destroy on close"

The manager renderer's ~80 MB is the biggest resident block besides skins; the suspend route (TrySuspend) was disproven on real machines (see docs/已知问题.md, dev-repo only). Closing (the close button / Alt+F4 — the frontend's `win.close()` uniformly routes to the CloseRequested branch) = `prevent_close` + `destroy()`; summoning rebuilds (tray / skin-menu "Open config" / .dskin double-click — the latter two rebuild first when the manager is destroyed, then navigate, via the `pending_open_config`/`pending_package` stash-and-pull pattern). The startup update check runs once per process (a rebuild doesn't re-prompt).

## 13. Testing, real-machine, and release

- **Full backend tests**: `cargo test --manifest-path src-tauri/Cargo.toml` (mandatory after changes; currently 160 items — the policy table's three completeness tests, the window-broadcast completeness scan, snapping regressions, package safety, backup rollback, update verification, …; hardware probes are ignored and run manually).
- **Frontend build**: `npx vite build` (multi-page: index + log).
- **The real-machine test checklist** (`docs/实机测试清单.md`, dev-repo only): **before every formal release the maintainer personally walks the whole list with the release-candidate installer** (new/changed features must add "steps + expectation" entries; ⭑ marks historical-incident regression points that every version must pass). No pass, no release.
- **The public release process** (`docs/公开发布流程.md`, dev-repo only): finalize the release commit in the dev repo → sync to the public repo per the exclusion list (docs/ is whitelist-based) → verify → push → the Release workflow (version gate → frontend build → cargo test → NSIS packaging → SignPath signing (two conditions: public repo + vars.SIGNPATH_ENABLED; unsigned degrades to a plain CI build) → the signed package's SHA-256 written into the run summary) → create the GitHub Release (notes carry the SHA256 line + the signing statement — the in-app updater downloads and verifies exactly that file). Two artifacts ship: the standard installer + an offline variant (a `tauri.offline.conf.json` override embeds the full WebView2 runtime, +~205 MB (210 MB measured), for machines with no network; the update channel only ever uses the standard one). Build the offline variant locally with `npm run build:offline` (orchestrated by tools/build-offline.mjs: preserve any existing standard build → build → rename to the <-offline> suffix → restore — the bundler always writes the standard name first, and "produce-then-rename" would clobber a previously built standard package (learned in the field); the naming rule's single source is tools/rename-offline-installer.mjs, shared with CI). The README dual versions carry the Code signing policy section; `PRIVACY.md` is the bilingual privacy policy (honestly covering the update check's four kinds of network activity + the third-party skin trust boundary).

## 14. Repository discipline (the AGENTS.md hard rules)

1. **Dual-version doc sync**: three pairs must be updated together — `docs/皮肤开发指南.md` ↔ `skin-development-guide.md`, `docs/关键机制.md` ↔ `critical-mechanisms.md`, and `docs/架构与机制总览.md` ↔ this document; CHANGELOG.md is Chinese-only.
2. **The pack-skin mirror stays in sync**: the mirror of types.rs/loader.rs/package.rs lives in tools/pack-skin/src/main.rs — changes must be synced and the exe rebuilt over the committed copy.
3. **Version numbers agree in four places** (§1).
4. **Command gates**: new sensitive commands in skin_api must pass require_perm; manager commands must lead with require_manager; log-buffer reads must pass require_log_window; **any new command must register a tier in policy.rs's COMMAND_POLICIES** (the completeness tests cross-check both directions — a missing registration goes red).
5. **Vendored patches**: the NOTE(driftlet) patches inside src-tauri/vendor/ (tauri-runtime-wry / tray-icon) must survive dependency upgrades.
6. **Line endings**: .gitattributes sets `* text=auto eol=lf` (*.ps1 stays CRLF); when git status shows contentless dirty files, check line endings first.
7. **Generated artifacts**: dist/, node_modules/, target/ are never committed; `src-tauri/gen/schemas/` is committed knowingly (editor completion for capabilities) — regenerate and commit along with builds after plugin/capability changes.
8. **The real-machine checklist** (§13) must pass before releases, and new features must add entries.

**Commit conventions**: detailed Chinese conventional style (feat:/fix:/refactor:/chore:/docs:); changes touching critical mechanisms record the incident basis in the commit message or docs/关键机制.md. **CHANGELOG voice** (Chinese-only, not synced to the public repo; readers = the Releases page — the section is lifted at release time with an English translation written then): no dev-repo-only content (internal doc references / toolchain names / process narration / bookkeeping numbers); technical details follow the public code and public docs; features torn down during development that never shipped in any public release are not listed, and in-cycle iterations on unreleased features get no separate notes either (only the final shipped shape is written). Release notes stay concise — user-perceivable changes, not implementation-detail lists.

## 15. Incident-pattern summary + the critical-mechanisms coverage map

### 15.1 Recurring incident patterns (abstracted lessons)

1. **"Restore state with a post-creation eval" always loses** — it races page load. The right answer is always atomic baking via the creation URL query (opacity/locked/resizable share one path).
2. **Cross-thread Win32 calls fail silently** — SetWindowSubclass and friends must run on the window-owning thread (run_on_main_thread); FFI callbacks must catch_unwind.
3. **Tauri 2 runs non-async commands on the main thread** — anything blocking (subprocesses/COM/window creation/capture) must be async + spawn_blocking, or the whole app freezes.
4. **HWNDs/handles get recycled by the system** — every HWND-keyed registry (snapping/click-through) must unregister on destroy.
5. **"The event goes only to one side" causes display drift** — for any state change, think through persistence, the skin-side event, and the manager-side broadcast (a single funnel + a completeness test is the standard solution).
6. **Option-follow vs explicit choice** — fields with "unset = follow manifest" semantics must be resolved to effective values before reaching the panel; a bare None piped to the frontend is guaranteed display drift.
7. **Numbers/names written into docs drift** — command counts, pixel sizes, and function names become real errors once code evolves past them (this repo's 2026-09 full-doc audit fixed 33 such drifts in one pass). When changing code, grep the docs for the function names/numbers.
8. **Safety limits trust measurement, not declarations** — zip sizes are measured by actually extracted bytes; paths by canonicalization; redirects re-checked per hop.
9. **Anything maintained in two places must be committed in pairs** — the pack-skin mirror, the dual-version docs, the self-maintained NSIS template; each has its own one-time incident lesson.
10. **Real machines beat reasoning** — edge-snap feel, TrySuspend suspension, the eyedropper, and demotion routes were all "plausible in theory, disproven on hardware"; new mechanisms pass the real-machine checklist before settling.

### 15.2 The critical-mechanisms coverage map (proof nothing was dropped)

All 25 sections of `docs/critical-mechanisms.md` are merged into this document per the map below (when this document is the one revised, the dual-version original is written back in sync):

| Critical-mechanisms section | Merged into |
|---|---|
| The frameless mechanism | §7.1 |
| Pinning to the desktop | §7.2 |
| Position lock | §7.3 |
| Skin dragging & context menu | §7.4 |
| Border drag-resize (resizable) | §7.5 |
| Edge snapping | §7.6 |
| Click-through | §7.7 |
| Virtual-display DPI & rasterization scale | §7.8 |
| The wallpaper layer (removed) | §7.8 |
| Preview capture | §7.9 |
| Data flow | §7.11 + §5 (Option resolution) |
| Custom skin settings | §5 + §6 |
| The skin backend API (skin_api) | §8 entirely |
| Stability conventions (Mutex / FFI callbacks) | §8.5 + §4 (lock model) |
| Skin packages (.dskin) & skin identity | §9 + §5 (identity vs directory) |
| Double-click install (.dskin association) | §9 |
| Layout backup (export/import) | §10 |
| Update check & auto-download | §11 |
| Development mode (skin hot reload) | §12.5 |
| Bundled-skins first-install seed | §5, last part |
| The skin:// protocol | §6 |
| Tray icons | §7.10 + §4 (vendored patches) |
| The log window & app_log | §12.4 |
| The comctl32 v6 manifest for cargo test (build.rs) | §4, last part |
| Behavioral constraints | §7.12 + §12.7 |

---

> This document is maintained as the code evolves; when the docs and the implementation disagree, the code is right and the docs get fixed (dual-version docs in pairs). Maintenance entry points: AGENTS.md (discipline) → this document (the full picture) → docs/critical-mechanisms.md (the do-not-regress list) → docs/skin-development-guide.md (the external contract).


