# Driftlet

> [中文版](README.md) | English

A Windows desktop skin manager built with Tauri 2 + Vite / vanilla JavaScript. It presents web pages as desktop widgets, offering transparent windows, frameless windows, always on top, and pin to desktop.

---

## Features

- Install, uninstall, load, and reload skins
- Install/update skins from `.dskin` skin packages (zip format); updates preserve user settings data
- Double-click a `.dskin` file to bring up the install wizard directly (the installer registers the file association)
- Skin permission model: sensitive capabilities must be declared in `permissions` in `skin.json` (5 kinds: registry / Shell / system control / clipboard / microphone); the install wizard lists them one by one and flags them with a two-tier high/medium risk grading (see "Security Model")
- Custom skin settings: declare config items in `skin.json` (20 control types + groups + descriptions); the config panel is generated automatically
- Adjust a skin's opacity, position, size, and zoom (the "Window" tab can enable resize-by-dragging: shows a frame hint, drag edges/corners to resize directly; zoom scales the whole window and its content from 50%–200%)
- Always on top / pin to desktop (mutually exclusive, pin to desktop by default), disable dragging
- Click-through (per-skin toggle on the "Window" tab, off by default): clicks and scrolls pass through to the window or desktop below — combined with pin-to-desktop the skin becomes a pure display widget
- Capture preview images for skins
- Tray icon management; closing the main window hides it to the tray
- Autostart, dark/light theme switching
- Right-click a skin window to open the skin menu (open config / refresh / unload)
- A global hotkey hides/shows all loaded skins with one keystroke (default Ctrl+Shift+Alt+D, changeable or disabled in Settings), with a synced checked item in the tray menu; Alt+F4 on a skin window only hides it — call it back via the hotkey or the tray
- Browser refresh/navigation shortcuts like F5 are blocked in both the manager and skin windows — pages cannot be refreshed by keystroke; the window lifecycle belongs entirely to the manager
- Layout backup: export/import all settings and skins as a single zip from the Settings page (for migration or sharing)
- Startup update check (on by default, can be turned off in Settings): prompts when a new GitHub release is available, with a one-click jump to the download page or an option to stop reminding

---

## Requirements

- Windows 10/11 (some features rely on the Win32 API)
- Node.js
- Rust / Cargo (required by Tauri 2)
- WebView2 Runtime (bundled with Win11)

---

## Quick Start

```bash
# Clone the repository
git clone <repo-url>
cd Driftlet

# Install dependencies
npm install

# Development mode
npm run tauri dev

# Build the production bundle
npm run tauri build
```

---

## Project Structure

```
├── src/                  # Frontend source
│   ├── js/               # Vanilla JS (app.js entry)
│   └── css/              # Styles
├── src-tauri/src/        # Rust backend
│   ├── commands.rs       # Tauri IPC commands (manager commands uniformly guarded by require_manager)
│   ├── lib.rs            # App startup, state, auto-loading
│   ├── desktop.rs        # Windows "pin to desktop" implementation
│   ├── window/factory.rs # Skin window creation / frameless subclassing
│   ├── window/snap.rs    # Edge snapping (rewrites coordinates in place during WM_MOVING)
│   ├── skin/             # Skin scanning, loading, config, .dskin package installation
│   └── skin_api/         # System info and sensitive-capability commands callable by skins (require_perm authorization)
├── src-tauri/capabilities/ # Window permissions: default.json (main window) / skin.json (skin windows, empty permissions)
├── examples/             # Example skin sources (reference; shipped as standalone .dskin, not bundled)
│   ├── controls-demo/        # Demo of all settings controls (bilingual; UI language follows the manager)
│   ├── sys-monitor/          # System monitor (the read-only system-info API set)
│   ├── media-hub/            # Media console (volume / media / spectrum / notifications)
│   ├── toolbox/              # Local toolbox (clipboard / files / registry / commands / settings read-write)
│   ├── deepseek-balance/     # DeepSeek balance auto-query (networked-skin reference; low-balance alert + notification + top-up)
│   ├── driftlet.js           # Optional wrapper: named command functions + event helpers (copy into a skin folder)
│   └── driftlet.d.ts         # Type definitions for the bridge and all commands (editor autocomplete)
├── tools/
│   ├── pack-skin.exe     # Skin packaging tool (standalone, generates .dskin)
│   ├── pack-skin/        # Packaging tool source (Rust)
│   └── win32-probes/     # Windows window probing scripts (for debugging)
├── CHANGELOG.md          # Version change log
└── docs/                 # Development docs
    ├── 皮肤开发指南.md    # Interface docs and specs for skin creators
    ├── 关键机制.md        # Window / desktop layer implementation details (do not regress)
    ├── 设计系统.md        # Manager UI visual system and frontend contract
    └── 已知问题.md        # Known issues and future directions
```

---

## Skin Development

> For the full interface documentation and specs, see [`docs/skin-development-guide.md`](docs/skin-development-guide.md); this section is a quick start.

In development builds (`npm run tauri dev`), a loaded skin can reload automatically when its files are saved (300 ms debounce) — no manual right-click refresh needed; hot reload is off by default and takes effect once enabled on the Settings page.

A skin is a standalone folder containing at least:

```
my-skin/
├── skin.json        # Skin metadata / window defaults
├── index.html       # Entry page
└── ...              # Images, css, js, and other resources (all inside the skin folder)
```

### skin.json Example

```json
{
  "id": "my-skin",
  "name": "My Skin",
  "name_en": "My Skin",
  "version": "1.0.0",
  "author": "You",
  "description": "A simple desktop widget",
  "entry": "index.html",
  "window": {
    "width": 300,
    "height": 200,
    "transparent": true,
    "always_on_top": false,
    "on_desktop": true,
    "resizable": false,
    "zoom": 1.0,
    "opacity": 0.95
  }
}
```

### Draggable Region

Add the `.drag-region` class to an element in the HTML to make it drag the skin window:

```html
<div class="drag-region">
  <!-- Content here can drag the window -->
</div>
```

### Adaptive Layout

The skin window size can be changed by the user at any time (via values in the manager panel, or by dragging the frame when `resizable` is enabled), so the skin layout must be adaptive: elements must not overflow the window's visible area, and no window-level scrollbars may appear. For the spec and the two paradigms (scale-to-fit / fill + internal scrolling), see `docs/skin-development-guide.md` §3.3; the example skin has been adapted accordingly.

### Calling Backend Commands

Inside a skin, backend commands are called through the injected bridge (recommended entry `window.driftlet`; `window.__DESK_PP__` is the same object under its legacy name, kept forever):

```js
if (window.driftlet?.invoke) {
  const [cpu] = await window.driftlet.invoke('get_cpu_info');
  console.log(cpu.usage); // total usage %; rate readings return 0 on the first call (baseline) — poll once per second
}
```

The optional wrapper `examples/driftlet.js` turns commands into named functions like `Driftlet.getCpuInfo()` (with `driftlet.d.ts` for editor autocomplete); the full command list and contracts are in `docs/skin-development-guide.md` chapter 5.

### Custom Settings

A skin can declare config items with the `settings` array in `skin.json`; the manager's config panel will show a dedicated "Skin Settings" tab, automatically generating the corresponding controls from the declarations, with values persisted in the global config:

```json
"settings": [
  { "key": "title",        "type": "text",        "label": "Title",       "default": "Hello" },
  { "key": "notes",        "type": "longtext",    "label": "Notes",       "default": "" },
  { "key": "alarm_time",   "type": "time",        "label": "Alarm Time",  "default": "07:30" },
  { "key": "start_date",   "type": "date",        "label": "Start Date",  "default": "2026-01-01" },
  { "key": "show_seconds", "type": "boolean",     "label": "Show Seconds","default": true },
  { "key": "features",     "type": "multiselect", "label": "Enabled Features", "default": ["a"],
    "options": [ { "value": "a", "label": "Feature A" }, { "value": "b", "label": "Feature B" } ] },
  { "key": "mode",         "type": "radio",       "label": "Mode",        "default": "auto",
    "options": [ { "value": "day", "label": "Day" }, { "value": "night", "label": "Night" }, { "value": "auto" } ] },
  { "key": "accent_color", "type": "palette",     "label": "Accent Color","default": "#ff3333",
    "options": [ { "value": "#ff3333" }, { "value": "#4da3ff" } ] },
  { "key": "active_range", "type": "timerange",   "label": "Active Range",
    "default": { "start": "2026-07-20 12:00:00", "end": "2026-08-20 00:00:00" } },
  { "key": "level",        "type": "slider",      "label": "Intensity",   "default": 60, "min": 0, "max": 100, "step": 1 },
  { "key": "refresh_ms",   "type": "number",      "label": "Refresh Interval", "default": 1000, "min": 100, "max": 10000 },
  { "key": "tasks",        "type": "tasklist",    "label": "Task List",   "default": ["Sample task"] }
]
```

Supported `type` values and value formats:

| type | Control | Value Format | Notes |
|------|---------|--------------|-------|
| `text` | Short text input | `"string"` | ≤256 chars |
| `longtext` | Long text input | `"string"` | Multi-line, ≤4000 chars |
| `password` | Password input (masked) | `"string"` | ≤256 chars; the value is not injected into the page — see the note below the table for how to read it |
| `time` | Time picker (24h, second precision) | `"HH:MM"` or `"HH:MM:SS"` | |
| `date` | Date picker | `"YYYY-MM-DD"` | |
| `datetime` | Date-time picker | `"YYYY-MM-DD HH:MM:SS"` | Empty string = unset |
| `boolean` | Toggle switch | `true / false` | |
| `multiselect` | Multi-toggle group | `["a","b"]` | Requires `options`; the value is a subset of the selected items |
| `radio` | Exclusive toggle group | `"a"` | Requires `options`; only one per group |
| `weekdays` | Weekday picker | `["mon","wed"]` | Multi-select Mon–Sun, fixed options |
| `select` | Dropdown select | `"a"` | Requires `options` |
| `font` | Font picker | `"Microsoft YaHei UI"` | Enumerates installed system fonts; empty string = default |
| `palette` | Palette | `"#rrggbb"` or `"#rrggbbaa"` | `options` as preset colors (optional; includes a screen eyedropper and an opacity slider) |
| `number` | Number input | `number` | Optional `min` / `max` / `step` |
| `slider` | Slider | `number` | Optional `min` / `max` / `step`, defaults 0/100/1 |
| `timerange` | Time range (second precision) | `{ "start": "YYYY-MM-DD HH:MM:SS", "end": "..." }` | Empty string means unset |
| `tasklist` | Task list (add/delete/edit) | `["Item 1","Item 2"]` | |
| `todolist` | Todo list (checkable) | `[{ "text": "...", "done": true }]` | Skins can write back via `skin_set_setting` |
| `datetasklist` | Dated task list | `[{ "time": "YYYY-MM-DD HH:MM:SS", "text": "..." }]` | Each task carries a date-time; time may be empty |

Values of type `password` are **not baked into the page with the bridge's `settings`** (all skins share the same origin under skin://, so anything injected into the page could be scraped by other skins); instead, read them on demand inside the skin with the `skin_get_setting` command — `await driftlet.invoke('skin_get_setting', { key: 'my_key' })`. Identity is taken from the calling window, so a skin can only read its own values.

The `label` of each `options` entry may be omitted, falling back to displaying the `value`. Each setting can also carry a `"description"` note, shown below the control's label:

```json
{ "key": "level", "type": "slider", "label": "Intensity", "description": "0 to 100; affects the particle count", "default": 60 }
```

### Groups

Settings can specify a group name with `"group"`; the "Skin Settings" tab places controls of the same group into one card (consistent with the section style of the "Window" tab). Groups are ordered by first appearance; controls without a `group` go into the untitled card at the top:

```json
"settings": [
  { "key": "title", "type": "text", "label": "Title", "group": "Text", "default": "Hello" },
  { "key": "notes", "type": "longtext", "label": "Notes", "group": "Text", "default": "" },
  { "key": "accent_color", "type": "palette", "label": "Accent Color", "group": "Appearance", "default": "#ff3333" }
]
```

Reading and listening inside a skin:

```js
// Initial values: baked in by the injected bridge before the page loads
const settings = window.driftlet?.settings || {};
console.log(settings.accent_color);

// Runtime changes: pushed in real time when settings change in the manager, no reload needed
document.addEventListener('desk-setting-changed', (e) => {
  const { key, value } = e.detail;
  // Apply the new value...
});
```

Reference example: `examples/controls-demo` (demo of all 20 control types; the UI language follows the manager).

### Installing Skins

1. In the manager, click "+ Add Skin" and choose a `.dskin` skin package (user settings data is preserved on update).
2. With the installed build, you can also **double-click a `.dskin` file** directly: it launches Driftlet and pops up the install wizard; confirm to install (the file association is registered by the installer; the portable exe has no such entry).
3. During development, you can copy the skin folder directly into `<install dir>\skins\`.

A `.dskin` is just the skin folder zipped up (with `skin.json` at the root declaring `id`/`version`) and renamed; skin authors can use the standalone packaging tool `tools/pack-skin.exe` (no installation, no Node.js / Rust environment needed) to generate and validate one in a single step:

```
tools\pack-skin.exe <skin folder> [output dir]
```

See `docs/skin-development-guide.md` §8 for details.

---

## Security Model

A third-party skin is **networked local code** (a full Chromium web page + backend command calls) — treat it with that trust model. Driftlet's lines of defense:

- **Permission declaration**: 5 kinds of sensitive capabilities — registry, Shell, system control (volume / media / open external / notifications), clipboard, microphone — must be declared in `permissions` in `skin.json` before they can be called, and the backend enforces per-command checks; the install wizard lists each declaration with a two-tier grading (high risk `shell` / `system` in red, medium risk `registry` / `clipboard` / `mic` in yellow).
- **Manager commands are callable only from the manager window**: all management commands (load / unload / settings, etc.) verify the caller window's identity; calls from skin windows are always rejected (except three harmless commands: dragging, frame resizing, and the right-click menu).
- **Zero grants for skin windows**: skin windows have empty capabilities — no Tauri core/plugin permissions at all; they can only reach backend commands through the injected bridge `__DESK_PP__.invoke`.
- **File sandbox**: a skin's file reads/writes are confined to its own folder (absolute paths and `..` escapes rejected); `skin.json` / `settings.json` are write- and delete-protected.
- **Cross-skin isolation of settings values**: `settings.json` is intercepted by the skin:// protocol (including 8.3 short-name, ADS, and other bypass tricks), so skin A cannot read skin B's settings; `password` values never land on the page and are dispensed by `skin_get_setting` based on window identity.
- **`.dskin` install hardening**: extraction guards against zip-slip and zip bombs (metered by actual decompressed bytes); size/file-count limits 64MB / 256MB / 5000; staged, rollback-style installation that leaves the old version intact on failure.

---

## Runtime Data Locations

All data lives alongside the install directory (portable mode):

- Skins directory: `<install dir>\skins\`
- Global config: `<install dir>\config\config.json` (Window-tab data and global settings)
- Skin settings values: `<install dir>\skins\<skin id>\settings.json` (user values from the "Skin Settings" tab; travels with the skin folder and is preserved when the skin is updated)

Note: configs stored by older versions under `%APPDATA%\com.driftlet.app\` are migrated automatically on first launch; if the install directory is not writable (e.g., a protected Program Files location), the app falls back to `%APPDATA%\com.driftlet.app\`.

---

## Notes

- "Always on top" and "pin to desktop" are mutually exclusive — one of them is always active, defaulting to "pin to desktop".
- Skin windows stay frameless via a custom Win32 subclass; read `docs/关键机制.md` (Chinese) before touching any window / desktop layer code.
- Skin resources are all loaded through the custom `skin://` protocol; put any external file references inside the skin folder.

---

## Dev / Build Commands

```bash
npm run dev           # Start the Vite frontend only
npm run build         # Build the frontend to dist/
npm run tauri dev     # Development mode (frontend + Tauri)
npm run tauri build   # Production installer build
```

Installer artifacts (NSIS only, `bundle.targets = ["nsis"]`; the installer is Chinese-English bilingual and automatically follows the system UI language):

- NSIS: `src-tauri/target/release/bundle/nsis/Driftlet_<version>_x64-setup.exe`

Note: `nsis.languages = ["English", "SimpChinese"]` — at runtime the installer matches the system language automatically, falling back to the **first** entry in the array when there is no match, so English must come first (Chinese systems → Simplified Chinese, everything else → English). An MSI used to be produced as well; it is no longer generated.

Backend-only checks:

```bash
cd src-tauri
cargo check
cargo build --release
```
