# Driftlet Skin Development Guide

> [中文版](皮肤开发指南.md) | English

> The complete API documentation and specification for skin creators. After reading this document you can develop, debug, package, and publish a Driftlet skin on your own.
> This document covers Driftlet 1.0. For internal implementation details (mechanisms that must not regress), see `docs/critical-mechanisms.md`.

---

## Contents

1. [What Is a Skin](#1-what-is-a-skin)
2. [skin.json Reference](#2-skinjson-reference)
3. [Page Development Conventions](#3-page-development-conventions)
4. [Custom Settings (settings)](#4-custom-settings-settings)
5. [Bridge API (window.driftlet)](#5-bridge-api-windowdriftlet)
6. [Security Model](#6-security-model)
7. [Development Workflow and Debugging](#7-development-workflow-and-debugging)
8. [Packaging and Distribution (.dskin)](#8-packaging-and-distribution-dskin)
9. [Release Checklist](#9-release-checklist)
10. [Example Skins](#10-example-skins)

---

## 1. What Is a Skin

A skin = a standalone folder containing a small web page (HTML/CSS/JS), displayed by Driftlet as a transparent, frameless desktop window. A skin window can be always on top or pinned to the desktop layer, and supports dragging, edge snapping, whole-window zooming, a right-click menu, preview images, and custom settings.

### 1.1 Minimal Structure

```
my-skin/
├── skin.json        # required — skin metadata / window config / custom settings declarations
├── index.html       # required — entry page
└── ...              # images, css, js and other assets (must live inside the skin folder)
```

Optional files:

- `preview.png` / `preview.jpg` / `preview.jpeg` — the preview image in the manager's list; can also be generated with "Capture Preview" in the manager's config panel.

### 1.2 How It Works (30-Second Version)

- The manager creates a WebView2 window (transparent, frameless) for each loaded skin; the page is loaded from the skin folder via the `skin://` custom protocol.
- **Before** the page loads, the manager injects the bridge script `window.__DESK_PP__` into the HTML (current setting values, drag takeover, right-click menu takeover, command channel) — the bridge is already ready when skin scripts run; no waiting needed.
- All of a skin's "system capabilities" (system info, file read/write, notifications, etc.) are obtained by calling backend commands through `__DESK_PP__.invoke`; sensitive capabilities require permission declarations in `skin.json` (§2.3).
- The window lifecycle (load, refresh, unload, position, size, opacity, always on top / pin to desktop) is entirely the manager's job; a skin only cares about its page — it never creates, moves, or closes its own window.
- An Alt+F4 close request is intercepted and downgraded to hiding the window — bring it back with the global hotkey (default Ctrl+Shift+Alt+D) or the tray checkbox; programmatic closes (unload / reload / quit) are unaffected.
- The manager and all skin windows uniformly disable WebView2 browser accelerator keys: F5 / Ctrl+R / Ctrl+F5 refresh, Ctrl+P print, Alt+Home, F12 and the like are all dead (editing keys like Ctrl+C/V are unaffected) — pages cannot be refreshed or navigated by keystroke. The single exception is Developer mode: with Settings → Advanced → Developer mode on, F12 / Ctrl+Shift+I inside a skin window opens DevTools (§7.1).

### 1.3 Five-Minute Quickstart

A complete, runnable minimal skin — two files, copy and go:

`hello/skin.json`:

```json
{
  "id": "hello",
  "name": "Hello",
  "version": "1.0.0",
  "window": { "width": 260, "height": 120, "on_desktop": true },
  "settings": [
    { "key": "text", "type": "text", "label": "Text", "default": "Hello, Driftlet!" }
  ]
}
```

`hello/index.html`:

```html
<!DOCTYPE html>
<html>
<head>
  <meta charset="UTF-8" />
  <style>
    html, body { height: 100%; margin: 0; }
    body { background: transparent; overflow: hidden; }
    .card {
      width: 100%; height: 100%; box-sizing: border-box;
      display: flex; align-items: center; justify-content: center;
      background: rgba(20, 24, 32, 0.85); color: #fff;
      border-radius: 12px; font: 14px/1.4 sans-serif; user-select: none;
    }
  </style>
</head>
<body>
  <div class="card drag-region" id="msg"></div>
  <script>
    'use strict';
    const msg = document.getElementById('msg');
    function render() {
      // The bridge is injected before the page loads; ?. guards the
      // bridge-less case (opening the page in a plain browser)
      msg.textContent = (window.driftlet?.settings || {}).text || 'Hello, Driftlet!';
    }
    render();
    // Apply setting changes from the manager live (no reload needed)
    document.addEventListener('desk-setting-changed', render);
  </script>
</body>
</html>
```

Copy the `hello` folder into `<install dir>\skins\`, click "Refresh" in the manager, then "Load" — the skin appears on your desktop: drag the card to move it; change "Text" on the manager's "Skin Settings" page and the card updates instantly.

Next steps: more control types → chapter 4; system capabilities → chapter 5 (see the command cheat sheet at the top of the chapter).

> **Entry name**: the examples here use the recommended entry `window.driftlet`; `window.__DESK_PP__` found in older skins is the same object under its legacy name, kept forever — no changes needed. An optional wrapper `examples/driftlet.js` (named functions like `Driftlet.getCpuInfo()`) plus `examples/driftlet.d.ts` (editor autocomplete) can be copied into your skin folder.

---

## 2. skin.json Reference

Full example:

```json
{
  "id": "my-skin",
  "name": "My Skin",
  "version": "1.0.0",
  "author": "You",
  "description": "A simple desktop widget",
  "bilingual": false,
  "entry": "index.html",
  "window": {
    "width": 300,
    "height": 200,
    "transparent": true,
    "always_on_top": false,
    "on_desktop": true,
    "resizable": false,
    "zoom": 1.0,
    "opacity": 1.0
  },
  "permissions": [],
  "settings": []
}
```

The `skin.json` file itself must not exceed **1 MB** (larger files are refused at load time).

### 2.1 Top-Level Fields

| Field | Required | Description |
|------|------|------|
| `id` | Required for packaging/distribution | Unique skin ID; rules below |
| `name` | Yes | Skin name, shown in the manager's list and config panel |
| `name_en` | No | English skin name (preferred in the English UI when `bilingual: true`; falls back to `name` if empty), see §4.5 |
| `entry` | No | Entry HTML file name, default `index.html`; must be a **plain file name** — `..`, `/`, `\`, `:` are not allowed |
| `author` | No | Author, shown on the card and the config panel |
| `version` | No | Version number, e.g. `"1.0.0"`; update packages use it to decide upgrade/downgrade — strongly recommended to always set it |
| `min_host_version` | No | Minimum host version, e.g. `"1.0.6"`; if the host is older, the install wizard warns that "some features may not work" (installation is NOT blocked). Runtime feature detection: see `hostVersion` in §5.1 |
| `description` | No | One-line description |
| `description_en` | No | English description (same selection rules as `name_en`) |
| `bilingual` | No | Chinese/English bilingual declaration, default `false`; when `true`, the various `*_en` English strings take effect, see §4.5 |
| `window` | No | Window defaults, see §2.2 |
| `permissions` | No | Sensitive capability declarations (`registry` / `shell` / `system` / `clipboard` / `mic`), see §2.3 |
| `settings` | No | Custom settings declarations, see Chapter 4 |

A skin's unique identifier (skin id) = the **`id` field** of `skin.json`:

- Rules: lowercase letters, digits, dashes; must start with a letter or digit; ≤64 characters (e.g. `controls-demo`); **Windows reserved device names are not allowed** (`con`, `prn`, `aux`, `nul`, `com1`–`com9`, `lpt1`–`lpt9`, including base names with extensions like `con.txt`).
- It is the ownership key for user data — **the same id is treated as the same skin**; an update package (same id, higher version) replaces the files while preserving the user's settings data (window config is stored per id in the global config.json; user values from the "Skin Settings" tab live in `settings.json` in the install folder and are preserved automatically on update — see §8.3).
- **Required** when packaged as `.dskin` for distribution; may be omitted during folder-based development/installation, in which case the system derives one from the folder name.

### 2.2 The window Field

| Field | Type | Default | Description |
|------|------|------|------|
| `width` / `height` | number | 300 / 200 | Initial size in **logical pixels** (DPI scaling is handled by the system) |
| `transparent` | boolean | true | Transparent window background (skin development generally keeps this true) |
| `always_on_top` | boolean | false | Initially always on top. Mutually exclusive with `on_desktop` |
| `on_desktop` | boolean | true | Initially pinned to the desktop layer (still visible when Win+D shows the desktop). Mutually exclusive with `always_on_top` |
| `resizable` | boolean | false | Whether the window edges/corners can initially be dragged to resize (minimum 60×40). While enabled, an animated yellow-and-black striped border hint is shown, and the outer 6px edges are the resize hot zone, taking precedence over `.drag-region` dragging. Users can toggle it anytime on the manager's "Window" tab; before enabling it, the skin should have a responsive layout per §3.3, otherwise content gets clipped as the window shrinks |
| `zoom` | number | 1.0 | Default zoom (0.5 – 2.0): actual window = `width × zoom` × `height × zoom`; content is scaled by the same factor via WebView2 ZoomFactor — **lay the skin out at its base size and the platform handles the overall scaling, no adaptation needed**. Users can adjust it anytime on the manager's "Window" tab |
| `opacity` | number | 1.0 | Initial opacity, 0.1 – 1.0 |

Except for `transparent`, which is fixed by the author, all of the above are **initial defaults** — later changes made by the user in the manager override them and are persisted.

### 2.3 Permission Declarations (permissions)

Before a skin can call "sensitive capability" backend commands (§5.3), it must declare the corresponding permission at the top level of `skin.json`, otherwise the call is rejected:

```json
"permissions": ["registry", "shell"]
```

| Permission | Risk | Commands unlocked |
|------|------|-----------|
| `registry` | Medium risk | `read_registry_value` (read-only) |
| `shell` | High risk | `run_command` (normal privileges, hidden window) |
| `system` | High risk | `set_volume` / `set_mute` / `media_control` / `open_external` / `show_notification` (change system state: adjust volume, control media playback, open external links/files, send system notifications) |
| `clipboard` | Medium risk | `read_clipboard_text` / `write_clipboard_text` (reading may expose sensitive content the user just copied) |
| `mic` | Medium risk | `get_mic_spectrum` (microphone input — unlike system loopback, this is real audio capture and privacy-sensitive) |

"Risk" is the two-tier grading shown on the install wizard (see below) — display-only; backend enforcement remains a binary "declared / undeclared" check regardless of tier.

Rules:

- **Declare only the permissions you actually use**; unknown names are ignored.
- Read-only system info commands (§5.2) and settings read/write commands (§5.4) need no declaration; neither does file read/write inside the skin folder (`skin_read_file` / `skin_write_file` / `skin_list_dir` / `skin_delete_file`) — the fs sandbox already confines every operation to the skin's own install folder (absolute paths and `..` rejected, canonicalize containment check, symlink-escape protection, `skin.json`/`settings.json*` read-only protection). The old `files` permission has been removed entirely; a leftover `"files"` declaration in an old skin is treated as an unknown name and ignored — harmless.
- **Permission declarations are visible to users**: when installing/updating a skin, the install wizard lists every declared permission one by one, flagged in two risk tiers — `shell` and `system` as "High risk" (red warning badge), `registry`, `clipboard`, and `mic` as "Medium risk" (yellow warning badge); the removed `files` declaration is silently skipped and not shown. Declaring permissions you don't use lowers users' willingness to install.
- From the user's perspective: a skin declaring `shell` is equivalent to being able to run local programs — state honestly on the release page which permissions the skin uses and what for.

---

## 3. Page Development Conventions

### 3.1 Required Rules

- **Keep all assets inside the skin folder** and reference them with relative paths (`<img src="bg.png">`). Skins are loaded via the `skin://` custom protocol and cannot access paths outside the folder.
- Never write `skin://` asset references directly — on Windows, WebView2 does not support subresource loading over non-standard protocols and fails with `ERR_UNKNOWN_URL_SCHEME` (at runtime `skin://localhost/...` is rewritten into the `http://skin.localhost/...` form). The absolute form of a reference is `http://skin.localhost/<skin id>/<path>`, but it hardcodes the skin id into your code and renaming the folder means 404 — **always prefer relative paths**.
- For transparency, set `body { background: transparent }` and paint the background on your own container.
- **The layout must adapt to the window size**: no element may exceed the window's visible area — when the window shrinks, content must scale/reflow with it; neither overflow clipping nor window-level scrollbars are allowed. See §3.3 for how.
- **Do not use `-webkit-app-region: drag`**. Use the `.drag-region` class for draggable areas instead (see §3.2).
- Right-click anywhere on the page is taken over by the app by default (it shows the native "Open Skin Settings / Reload Skin / Unload Skin" menu, and WebView2's default menu is disabled). If your skin needs to handle right-click on a specific element (e.g. right-click-to-edit on a card): listen for `contextmenu` and call `preventDefault()` — the bridge checks `defaultPrevented` at the end of the window bubble path, and clicks the skin consumed will not open the host menu. (Hosts on 1.0.1 or earlier lack this contract; fall back to a window capture-phase listener plus `stopPropagation()`.)
- **Prefer `textContent` for dynamic content**; never splice unescaped strings such as user input or task text into `innerHTML` (reason in §6.3).

### 3.2 Draggable Areas

Add the `.drag-region` class to an element, and the user can drag the window by holding the left mouse button on it:

```html
<div class="drag-region">
  <!-- this empty area drags the window -->
</div>
```

Convention: make the whole shell a drag-region; interactive elements nested inside are unaffected — a press landing on `button` / `input` / `select` / `textarea` / `a` / `label` / `[contenteditable]` automatically skips dragging. Other elements that need exclusive `pointerdown` (e.g. list drag-sorting) should call `e.stopPropagation()` on that element to stop the bubbling.

Note: with border drag-resize (`resizable`) enabled, the outermost 6px of the window is the resize hot zone and takes priority over dragging — pressing an element flush against the edge triggers resizing instead of moving (§2.2).

Also: users can enable "Edge snapping" on the manager's "Window" tab — when a window is dragged near a screen edge or another skin window's edge it aligns automatically (screen edges win; the snap gap is customizable; after snapping, release and drag again to move away freely within 1 second, and the window won't leave the screen). Snapping is done at the app layer; skins need no adaptation.

### 3.3 Size, DPI, and Responsive Layout

- Window size and position in the config are always **logical pixels**; CSS pixels inside the skin match them — no DPI conversion to worry about.
- **The layout must adapt to any window size** — users can change width/height anytime on the manager's "Window" tab, enable `resizable` for free border dragging, or use "Zoom" to scale the whole skin to 50%–200% (done by the platform via WebView2 ZoomFactor; lay the skin out at its base size, no special adaptation needed); a skin must not depend on the default size in `skin.json`. Rules:
  - The shell fills the window: `html, body { height: 100% }`, root container `width/height: 100%`;
  - No element may be hardcoded to a fixed size exceeding `window.width × window.height`;
  - `body { overflow: hidden }` — no window-level scrollbars;
  - Overly tall content must not stretch out of body — either scale it down, or tuck it into its own container with internal scrolling (see below).
- Two compliant paradigms (pick one according to the content's nature):
  1. **Proportional scaling** (clocks, graphics): use `clamp(min, min(Xvw, Yvh), max)` for font sizes/spacing — `min(Xvw, Yvh)` guarantees the content shrinks proportionally whichever of width or height gets smaller, and at the default window the computed value equals the original design value.
  2. **Fill + internal scroll** (panels, lists): panel `height: 100%; overflow: hidden auto`; overly tall content scrolls inside the panel instead of stretching past the window. Reference: `examples/controls-demo`.
- With many fixed blocks, use media queries as a fallback: hide decorative elements in short windows so the core functionality stays intact.
- Self-check: in the manager, shrink the window to half and then double it — content should scale/reflow completely: no clipping, no overflow, no window-level scrollbars.

### 3.4 Network and Security

- A skin window is a full browser (WebView2): `<img>`, `<script src>`, `fetch`, `WebSocket` can all reach external networks; the manager imposes no restrictions.
- The page's origin is `http://skin.localhost` (the rewritten form of the `skin://` protocol on Windows). `fetch`/`XHR` requests to external sites are cross-origin, so **the target server must return CORS allow headers**; tag-based loading (`<img>`/`<script>`) and WebSocket are not subject to CORS.
- Skin pages have no CSP constraints — freedom, but it also means that layer of protection is absent: only load resources and scripts you trust yourself.
- All skins share the same origin (`http://skin.localhost`); the same-origin policy does **not** isolate skins from each other's ordinary resource files. Don't store private data as plain files in the skin folder — user private data should use `password`-type setting items (§4.3), which the platform guarantees will never appear in any page.
- Code in a skin package is **local code that can access the network**. Consider reminding users on your release page: installing a third-party skin is equivalent to installing a small desktop app.

---

## 4. Custom Settings (settings)

Declare a `settings` array in `skin.json` and the manager's "Skin Settings" tab auto-generates controls from the declarations; values are persisted and readable in real time while the skin runs.

```json
"settings": [
  { "key": "title", "type": "text", "label": "Title", "group": "Text",
    "description": "Title shown at the top", "default": "Hello" },
  { "key": "accent_color", "type": "palette", "label": "Accent color", "group": "Appearance",
    "default": "#ff3333" }
]
```

### 4.1 Common Setting Item Fields

| Field | Required | Description |
|------|------|------|
| `key` | Yes | Unique identifier (unique within the skin; it is the persistence key) |
| `type` | Yes | Control type, see §4.2 |
| `label` | No | Control title; `key` is shown if omitted |
| `label_en` | No | English control title (bilingual skins only, see §4.5) |
| `description` | No | Help text shown below the control |
| `description_en` | No | English help text (bilingual skins only) |
| `group` | No | Group name; controls of the same group are collected into one card; groups are ordered by first appearance; unspecified ones go into an untitled card |
| `group_en` | No | English group name (bilingual skins only) |
| `default` | No | Default value; if omitted, a per-type fallback applies (false / 0 / "" / first option) |

### 4.2 Control Types (20 Total)

| type | Control | Value format | Type-specific fields / notes |
|------|------|--------|------------------|
| `text` | Short text input | `"string"` | ≤256 characters |
| `longtext` | Long text input | `"string"` | Multiline, ≤4000 characters |
| `number` | Number input | `number` | `min` / `max` / `step` |
| `slider` | Slider | `number` | `min` / `max` / `step`, defaults 0/100/1 |
| `stepper` | Number stepper | `number` | `min` / `max` / `step`, unbounded/1 by default; −/+ buttons step the value, the button is disabled at its bound |
| `boolean` | Toggle switch | `true / false` | |
| `select` | Dropdown | `"a"` | Requires `options` |
| `radio` | Mutually-exclusive switch group | `"a"` | Requires `options`; only one per group |
| `multiselect` | Multi-select switch group | `["a","b"]` | Requires `options`; value is a subset |
| `weekdays` | Weekday picker | `["mon","wed"]` | Fixed Monday–Sunday multi-select |
| `time` | Time picker (24h) | `"HH:MM"` or `"HH:MM:SS"` | Second precision |
| `date` | Date picker | `"YYYY-MM-DD"` | |
| `datetime` | Date-time picker | `"YYYY-MM-DD HH:MM:SS"` | Second precision; empty string = unset; for countdown-target-style scenarios |
| `password` | Password input | `"string"` | Masked display (with show/hide toggle), ≤256 characters, good for API keys. **The value is never injected into pages**; see §4.3 for how to read it |
| `timerange` | Time range | `{ "start": "YYYY-MM-DD HH:MM:SS", "end": "..." }` | Second precision; empty string = unset |
| `palette` | Color palette | `"#rrggbb"` or `"#rrggbbaa"` | Preset colors + custom color picking (the color panel has a built-in screen eyedropper) + opacity slider; `options` can customize the preset colors |
| `font` | Font picker | `"Microsoft YaHei UI"` | Enumerates installed system fonts; empty string = default |
| `tasklist` | Task list | `["item 1","item 2"]` | Users freely add/remove/edit |
| `todolist` | To-do task list | `[{ "text": "...", "done": true }]` | Task rows with checkboxes; max 500 items, 200 characters per item (silently truncated beyond) |
| `datetasklist` | Dated task list | `[{ "time": "YYYY-MM-DD HH:MM:SS", "text": "..." }]` | Each item carries a date-time; time may be empty |

`options` structure: `[{ "value": "a", "label": "Display name" }]`; `label` may be omitted (falls back to showing `value`); bilingual skins may additionally set `label_en` (see §4.5).

### 4.3 Reading Settings in a Skin

Initial values are injected into `window.__DESK_PP__.settings` before the page loads (a key → value object):

```js
const settings = window.__DESK_PP__?.settings || {};
document.body.style.color = settings.accent_color || '#ff3333';
```

When the user changes settings in the manager, a `desk-setting-changed` event is dispatched in real time (no skin reload needed):

```js
document.addEventListener('desk-setting-changed', (e) => {
  const { key, value } = e.detail;
  // __DESK_PP__.settings[key] is already synced to the new value — just re-apply
  applySettings();
});
```

**The `password`-type special case (important)**: all skins share the same page origin, so values baked into the HTML in `__DESK_PP__.settings` could theoretically be read by other skins (§6.1). Therefore **password-type keys in the `settings` object are always empty strings**; the real value can only be fetched per window identity via the `skin_get_setting` command (§5.4):

```js
let apiKey = '';
// read once at startup
if (window.__DESK_PP__?.invoke) {
  apiKey = await window.__DESK_PP__.invoke('skin_get_setting', { key: 'api_key' }) || '';
}
// after the user edits in the manager, desk-setting-changed still carries the new value (the event is only sent to this skin's own window)
document.addEventListener('desk-setting-changed', (e) => {
  if (e.detail.key === 'api_key') apiKey = e.detail.value;
});
```

Recommended practice:

- Read `settings` once at startup and apply it; then listen to the event for incremental updates. Without listening you would still get new values on the next reload, but the experience is worse.
- Defend against missing values (`??` defaults): configs saved before a new setting item was added may have no value for it.
- A value's type always matches the `type` you declared (the backend validates and rejects invalid values), but **after you change a type, old persisted values are discarded and fall back to the default**.

### 4.4 Writing Settings in a Skin (skin_set_setting)

A skin can also actively write its own setting values — it reads/writes **the same file** as the manager's "Skin Settings" tab (`settings.json` in the skin folder), which suits data that "both the skin and the manager can edit", like task lists and countdown targets:

```js
await window.__DESK_PP__.invoke('skin_set_setting', {
  key: 'tasks',
  value: [{ text: 'Write the weekly report', done: false }, { text: 'Water the plants', done: true }],
});
```

Contract:

- Only keys **declared in your own skin.json `settings`** can be written; undeclared keys are rejected (this prevents abusing the settings file as arbitrary storage).
- Values go through exactly the same validation and normalization as in the manager, per the declared `type` (overlong values truncated, invalid values error out).
- No `permissions` declaration needed: the caller identity comes from the window itself — a skin cannot impersonate another skin, nor touch other skins' settings files.
- After a successful write: if the manager's config panel is open, the corresponding control refreshes in place; `__DESK_PP__.settings[key]` is silently synced to the new value, but `desk-setting-changed` is **not** dispatched back to you (you just wrote the value yourself — echoing the event would easily loop). Changes made on the manager side are still pushed to you via `desk-setting-changed`.
- When the skin and the manager edit the same key at the same time, last write wins (single-value granularity); different keys don't interfere.

### 4.5 Chinese/English Bilingual (bilingual)

The manager UI supports switching between Chinese and English, but the strings in the settings schema (group names, control titles, descriptions, option display names) are provided by the skin author — the manager does not machine-translate them. To make the settings page follow the manager's language, declare at the top level of `skin.json`:

```json
{
  "bilingual": true,
  "name": "我的皮肤",
  "name_en": "My Skin",
  "settings": [
    { "key": "title", "type": "text",
      "label": "标题", "label_en": "Title",
      "group": "文本", "group_en": "Text",
      "description": "显示在顶部的标题", "description_en": "Title shown at the top",
      "default": "Hello" }
  ]
}
```

Rules:

- `bilingual` is an **author declaration** (telling the manager "this skin provides English strings"), not a user option; it defaults to `false`, in which case all `*_en` fields are ignored.
- When the manager language is **English** and `bilingual: true`, `name_en` / `description_en` and setting items' `label_en` / `description_en` / `group_en` / options' `label_en` are preferred; **if a field is left empty, that spot falls back to the default string** — translating only part of them is fine.
- When the manager language is Chinese, the default strings are always shown (`name` / `description` / `label` / `group` / option `label`).
- Bilingual-capable fields: the top-level skin name and description (`name_en` / `description_en`), plus the four kinds in the settings schema (control title, control description, group name, option display name); **values** like `default` do not participate in bilingual (values are data, passed to the skin as-is).
- Group cards are merged by the display string of the current language: if a group's `group` has a `group_en`, set it for **all** controls in that group, otherwise the English UI splits them into two cards.
- Whether to go bilingual is up to you: if not, don't write `bilingual` or any `*_en` field, and the skin shows the default strings in every language.

Note: `bilingual` and the `*_en` fields only control the strings of the **manager's config panel** — the language of the skin's **own page** is up to the skin. To make the page follow the manager's language: read `__DESK_PP__.language` initially (baked in before the page loads, no race) and listen for the `desk-language-changed` event to re-render:

```js
function currentLang() {
  return window.__DESK_PP__?.language === 'en' ? 'en' : 'zh-CN';
}
function render() { /* render UI strings per currentLang() */ }
render();
document.addEventListener('desk-language-changed', () => {
  render();  // __DESK_PP__.language is already synced (same as e.detail.language)
});
```

Not listening is harmless — the bridge bakes the new language on the next reload anyway. See `examples/controls-demo` (§10) for a complete reference implementation.

---

## 5. Bridge API (window.driftlet)

Injected by the app before the page loads; ready to use when skin scripts run. The recommended entry is `window.driftlet`; `window.__DESK_PP__` is the same object under its legacy name, kept forever — older skins need no changes.

**Command cheat sheet** (full contracts in §5.2–§5.4; the optional wrapper `examples/driftlet.js` turns commands into named functions like `Driftlet.getCpuInfo()`, with `driftlet.d.ts` for editor autocomplete):

| Command | Permission | Purpose |
|------|------|------|
| `get_cpu_info` / `get_gpu_info` / `get_memory_info` | — | CPU / GPU / memory |
| `get_disks_info` / `get_disk_space` | — | Disk list / space of a volume |
| `get_network_info` | — | Adapter rates and local IPs |
| `get_audio_spectrum` | — | System loopback spectrum |
| `get_os_info` / `get_processes` | — | OS info / process list |
| `get_volume` / `get_media_info` | — | Volume read / now playing |
| `get_battery_info` / `get_idle_time` | — | Battery / input idle time |
| `get_foreground_window_info` / `get_monitors` | — | Foreground window / monitors |
| `skin_read_file` / `skin_write_file` / `skin_list_dir` / `skin_delete_file` | — | Skin-folder file I/O (sandboxed) |
| `skin_get_setting` / `skin_set_setting` | — | Read / write your own declared settings |
| `skin_log` | — | Send an explicit host-log message (console output is forwarded automatically, §5.4) |
| `read_registry_value` | `registry` | Registry read-only |
| `run_command` | `shell` | Run a command (normal privileges, hidden window) |
| `set_volume` / `set_mute` / `media_control` / `open_external` / `show_notification` | `system` | Set volume / mute / media transport / open external target / toast notification |
| `read_clipboard_text` / `write_clipboard_text` | `clipboard` | Clipboard read / write |
| `get_mic_spectrum` | `mic` | Microphone spectrum |

### 5.1 Bridge Members

| Member | Type | Description |
|------|------|------|
| `settings` | object | Currently effective setting values (key → value); `password`-type keys are always empty strings, see §4.3 |
| `invoke(cmd, args?)` | function | Calls a backend command; returns a Promise (rejects on failure with a readable error message) |
| `language` | string | The manager UI language (`"zh-CN"` / `"en"`); updated in real time when the manager switches language, with a `desk-language-changed` event dispatched (`e.detail.language`) — usage in §4.5 |
| `hostVersion` | string | Host version (e.g. `"1.0.6"`); for feature detection — when you rely on commands/controls introduced in a newer version, compare numeric segments and degrade gracefully |
| `positionLocked` | boolean | Position lock state (used internally by the bridge; read-only reference) |
| `resizable` | boolean | Border resize switch state (used internally by the bridge; read-only reference) |
| `setOpacity(v)` / `setResizable(on)` | function | Used internally by the bridge; skins must not call them directly |

**Defensive style**: when debugging the page in a plain browser the bridge doesn't exist (§7.2), so all `__DESK_PP__` access should use optional chaining:

```js
const settings = window.__DESK_PP__?.settings || {};
const cpu = window.__DESK_PP__?.invoke ? await window.__DESK_PP__.invoke('get_cpu_info') : null;
```

The public commands a skin can call fall into three groups: §5.2 system info (read-only, no permission needed), §5.3 sensitive capabilities (require `permissions` declarations), §5.4 settings read/write. All other commands are unreachable (§5.5).

### 5.2 System Info (Read-Only, No Permission Needed)

**Rate-type readings (CPU/GPU usage, disk/network speed) = the delta between two samples; the first call returns 0 (baseline)** — poll once per second; don't call just once.

#### `get_cpu_info`

```js
const [cpu] = await window.__DESK_PP__.invoke('get_cpu_info');
// {
//   name: "Intel(R) Core(TM) i7-12700",
//   physical_cores: 12,        // physical core count
//   logical_cores: 20,         // logical thread count
//   frequency_mhz: 4800,       // current frequency (MHz, same reading as Task Manager's "Speed"; may exceed base clock under turbo)
//   usage: 23.5,               // total usage %
//   usage_per_core: [10.1, ...] // per-thread usage %
// }
```

Returns an array (reserved for multi-socket CPUs; always 1 entry on ordinary machines).

#### `get_gpu_info`

```js
const gpus = await window.__DESK_PP__.invoke('get_gpu_info');
// [{
//   name: "NVIDIA GeForce RTX 3060",
//   gpu_type: "discrete",          // adapter type: "discrete" | "integrated"
//   usage: 12.0,               // usage % (summed across engines, capped at 100)
//   vram_total: 12884901888,   // VRAM total (bytes): discrete = dedicated, integrated = shared system memory
//   vram_used: 4294967296,
//   vram_usage_pct: 33.3
// }]
```

Multiple GPUs return multiple entries; for integrated GPUs (unified memory) VRAM total/used are reported as shared system memory — "dedicated + shared" can exceed physical RAM, so it is not used as the accounting basis.

#### `get_memory_info`

```js
const mem = await window.__DESK_PP__.invoke('get_memory_info');
// {
//   ram:    { total, used, free, usage_pct, free_pct },  // physical memory
//   swap:   { total, used, free, usage_pct, free_pct },  // page file (paged pool)
//   commit: { total, used, free, usage_pct, free_pct }   // virtual memory (committed)
// }
// all sizes in bytes; percentages 0–100; commit is Windows-only (null elsewhere)
```

For virtual memory usage, read `commit`: Task Manager's "Committed xx/yy GB" — used = committed bytes, total = commit limit (RAM + page file − system reserve). `swap` is only the spill written to the page file and often stays 0 when RAM is plentiful.

#### `get_disks_info`

```js
const disks = await window.__DESK_PP__.invoke('get_disks_info');
// [{
//   name: "System", mount_point: "C:\\", fs: "NTFS",
//   total: 512110190592, used: 256060604416, free: 256049586176,
//   usage_pct: 50.0,
//   read_bps: 1048576, write_bps: 0   // current read/write speed (bytes/sec)
// }]
```

One entry per mount point; speeds are only provided for drive-letter mount points (`C:` etc.) — other mount kinds report 0.

#### `get_disk_space`

Space of a given drive letter or path (the volume containing that path):

```js
const c = await window.__DESK_PP__.invoke('get_disk_space', { path: 'C:' });
// { total, used, free, usage_pct, free_pct }  // bytes + percentages
// path can also be a path like "D:\\data"; rejects when no matching volume is found
```

#### `get_network_info`

```js
const net = await window.__DESK_PP__.invoke('get_network_info');
// {
//   adapters: [{
//     name: "Ethernet",
//     ips: ["192.168.1.10"],   // IPs of this adapter (loopback and IPv6 link-local excluded)
//     mac: "AA:BB:CC:DD:EE:FF",
//     upload_bps: 10240, download_bps: 204800  // bytes/sec (0 on first call)
//   }],
//   local_ips: ["192.168.1.10", ...]      // all IPs of this machine (deduplicated, same filtering)
// }
```

#### `get_audio_spectrum`

Real-time spectrum of the sound the system is playing (WASAPI loopback capture, no microphone needed). The first call starts the capture thread automatically; after polling stops for about 30 seconds the audio device is released, and polling again restarts it:

```js
const { bands, peak } = await window.__DESK_PP__.invoke('get_audio_spectrum', { bands: 32 });
// bands: [0.12, 0.85, ...]  // energy per band 0–1, logarithmically distributed 30Hz–16kHz; the bands param defaults to 32, clamped to 1–64
// peak: 0.42                // instantaneous peak volume 0–1
```

Rejects when there is no audio device or capture fails (recovers automatically afterwards). For a volume bar, poll with a 30–50ms timer or `requestAnimationFrame`.

#### `get_os_info`

```js
const os = await window.__DESK_PP__.invoke('get_os_info');
// {
//   os_name: "Windows 11 Pro", os_version: "11 (22631)", build: 22631,
//   is_windows_11: true, host_name: "DESKTOP-ABC", user_name: "you",
//   uptime_secs: 86400
// }
```

`os_name` is the product name (registry ProductName); `os_version` follows the `"{major} ({build})"` format.

#### `get_processes`

```js
const top = await window.__DESK_PP__.invoke('get_processes', { sort: 'cpu', limit: 10 });
// {
//   total: 217,                          // total process count
//   processes: [{ pid: 1234, name: "chrome.exe", cpu: 12.3, memory_bytes: 524288000 }]
// }
```

`sort`: `"cpu"` (default) / `"memory"` (any other value falls back to `"cpu"`); `limit` defaults to 10, clamped to 1–100. `cpu` is a whole-machine percentage 0–100 (100 = all cores fully busy, same accounting as Task Manager), **the first call returns 0 (baseline)**.

#### `get_volume`

```js
const v = await window.__DESK_PP__.invoke('get_volume');
// { volume_pct: 100.0, muted: false }   // system master volume and mute state
```

#### `get_media_info`

The media currently playing (SMTC — most players such as NetEase Cloud Music, QQ Music, Spotify, and browser videos integrate with it). **Returns `null` when there is no playback session (not an error)**:

```js
const m = await window.__DESK_PP__.invoke('get_media_info');
// null, or:
// {
//   title: "Song title", artist: "Artist", album: "Album",
//   status: "playing",                  // playing | paused | stopped
//   position_secs: 42.5, duration_secs: 231.0,
//   cover_base64: "<image base64>",     // cover art; null when the source app doesn't provide one
//   cover_mime: "image/jpeg"            // cover format (sniffed from content); null when unrecognized
// }
```

Fields the player didn't report are empty strings; progress may likewise be 0. `position_secs` is a snapshot of the player's last report and does not advance on its own during playback (reporting cadence varies by player — interpolate yourself if you need a smooth progress bar).

#### `get_battery_info`

```js
const b = await window.__DESK_PP__.invoke('get_battery_info');
// {
//   has_battery: true,      // false on desktops; the other fields are meaningless then
//   ac_online: true,        // AC power connected
//   charging: false,
//   percent: 85,            // null when unknown
//   secs_remaining: 7200    // null when unknown (common while charging)
// }
```

#### `get_idle_time`

Milliseconds since the user's last keyboard/mouse input:

```js
const idleMs = await window.__DESK_PP__.invoke('get_idle_time'); // 45230 = 45.2 seconds idle
```

#### `get_foreground_window_info`

The current foreground window (what the user is using); returns `null` in the rare cases where there is no foreground window:

```js
const w = await window.__DESK_PP__.invoke('get_foreground_window_info');
// { title: "Document - Word", pid: 12345, process_name: "WINWORD.EXE" }
```

`title` is capped at 512 characters (truncated beyond that); `process_name` is an empty string when it can't be determined (e.g. protected processes without sufficient privileges).

#### `get_monitors`

```js
const ms = await window.__DESK_PP__.invoke('get_monitors');
// [{
//   name: "\\\\.\\DISPLAY1",
//   rect:      { x: 0, y: 0, width: 1920, height: 1080 },  // physical pixels
//   work_area: { x: 0, y: 0, width: 1920, height: 1040 },  // excluding the taskbar
//   is_primary: true,
//   scale_factor: 1.25        // DPI scaling (1.25 = 125%)
// }]
```

Note that rect/work_area are **physical pixels**, differing from the logical pixels used in window config by a scale_factor. A secondary monitor's `rect.x/y` can be negative (virtual coordinates left of / above the primary monitor).

### 5.3 Sensitive Capabilities (Require permissions in skin.json)

See §2.3 for how to declare. Undeclared calls reject with an error like `Skin 'my-skin' has not declared the 'shell' permission`.

#### File Read/Write (No Permission Needed) — Limited to the Skin's Own Folder

These four commands used to require the `files` permission; the sandbox already confines their operations to the skin's own folder (see the limits below), so the declaration has been dropped.

```js
// write text (subdirectories are created automatically)
await window.__DESK_PP__.invoke('skin_write_file', { path: 'data/cache.json', data: JSON.stringify(obj) });
// read text (must be valid UTF-8, otherwise rejects)
const text = await window.__DESK_PP__.invoke('skin_read_file', { path: 'data/cache.json' });
// read binary → base64; write binary ← base64
const b64 = await window.__DESK_PP__.invoke('skin_read_file', { path: 'img.bin', binary: true });
await window.__DESK_PP__.invoke('skin_write_file', { path: 'img.bin', data: b64, binary: true });
// list a directory (path defaults to the skin root)
const entries = await window.__DESK_PP__.invoke('skin_list_dir', { path: 'data' });
// [{ name: "cache.json", is_dir: false, size: 123 }]
// delete a file (files only; cannot delete directories)
await window.__DESK_PP__.invoke('skin_delete_file', { path: 'data/cache.json' });
```

Limits:

- `path` is always relative to the skin folder: absolute paths, `..`, path segments containing colons, and symlink escapes are rejected;
- read ≤ 32MB, write ≤ 16MB;
- `skin.json` and `settings.json` (including `.bak` / `.tmp`) are managed by the app — readable, but writing and deleting are forbidden (only these four files directly under the skin root; same-named files in subdirectories are unrestricted);
- good for storing the skin's own cache/data; the skin folder is deleted along with the skin — don't use it as a persistent store.

#### Registry Read-Only (Permission `registry`)

```js
const v = await window.__DESK_PP__.invoke('read_registry_value', {
  root: 'HKCU', path: 'Environment', name: 'TEMP'
});
// { kind: "expand_string", value: "%USERPROFILE%\\AppData\\Local\\Temp" }
```

- `root`: `HKCU` / `HKLM` / `HKCR` / `HKU` (full names like `HKEY_CURRENT_USER` are also accepted);
- `kind`: `string` / `expand_string` / `multi_string` (value is an array) / `dword` / `qword` / `binary` (value is base64); `qword` is returned as a JSON number and loses precision beyond 2^53 (rare in practice);
- read-only interface, no writing. Rejects when the key or value doesn't exist.

#### Running Commands (Permission `shell`)

Executes with normal privileges (inheriting the app's own privileges, no elevation) and a hidden window, waits for completion, and returns the output:

```js
const r = await window.__DESK_PP__.invoke('run_command', {
  command: 'cmd', args: ['/c', 'ipconfig'], timeoutMs: 10000
});
// { code: 0, stdout: "...", stderr: "" }
```

- `timeoutMs`: default 30000, clamped to 100–120000 (milliseconds, decimals are rounded); on timeout the process is killed and the call rejects;
- `stdout` / `stderr` are each truncated at 1MB (truncation does not stop the command from finishing normally); GBK output on Chinese Windows is transcoded automatically;
- `code` is the process exit code (it can be negative for abnormal exits, e.g. -1073741819 = 0xC0000005);
- launch failures (command not found, etc.) reject;
- suited for one-shot query commands; processes needing interaction don't work; grandchild processes of long-running processes may not be cleaned up by the timeout, and output can be incomplete while a grandchild holds the output pipes — use with care.

#### System Volume & Media Control (Permission `system`)

```js
await window.__DESK_PP__.invoke('set_volume', { volumePct: 60 });  // 0–100, out-of-range values are clamped
await window.__DESK_PP__.invoke('set_mute', { muted: true });

const ok = await window.__DESK_PP__.invoke('media_control', { action: 'play_pause' });
// action: play | pause | play_pause | next | previous
// returns a boolean: whether the target player accepted the action; rejects when there is no playback session
```

#### Clipboard (Permission `clipboard`)

```js
const text = await window.__DESK_PP__.invoke('read_clipboard_text');
await window.__DESK_PP__.invoke('write_clipboard_text', { text: 'hello' });
```

Text only. What you read is the user's current clipboard — it may contain sensitive content they just copied, so only call it when you genuinely need it; that's also why it requires its own permission.

#### Opening Links/Files (Permission `system`)

Opens with the system default program (browser / file association):

```js
await window.__DESK_PP__.invoke('open_external', { target: 'https://example.com' });
await window.__DESK_PP__.invoke('open_external', { target: 'D:\\docs\\report.pdf' });
```

Allowed targets: `http(s)://`, `mailto:`, local absolute paths (files or folders). Explicitly rejected:

- **Executables / types the system resolves as code or remote references** (`.exe` `.bat` `.cmd` `.ps1` `.vbs` `.vbe` `.js` `.jse` `.wsf` `.wsh` `.msi` `.msp` `.msc` `.scr` `.com` `.pif` `.cpl` `.lnk` `.hta` `.reg` `.dll` `.jar` `.url` `.search-ms` `.library-ms` `.application` `.appref-ms` `.diagcab` `.website`) — to run programs, use `run_command` with the `shell` permission, so users get a correct expectation of capabilities; the last six (Explorer search/library files, ClickOnce, diagnostic packages, etc.) can indirectly point at remote shares — same NTLM-leak surface as UNC paths, so they are rejected too;
- **UNC paths** (`\\host\share` form; accessing one triggers an SMB connection);
- relative paths and schemes like `file:` / `javascript:`;
- nonexistent targets and open failures return the same error, indistinguishable.

#### System Notifications (Permission `system`)

Pops a Windows toast notification (visible in the Action Center, shown as coming from Driftlet):

```js
await window.__DESK_PP__.invoke('show_notification', { title: 'Reminder', body: 'Time to drink some water' });
```

- `title` ≤ 64 characters, `body` ≤ 256 characters (truncated beyond); `body` may be omitted;
- no buttons/callbacks or other interaction — it's just a "reminder";
- the first call registers a Driftlet shortcut in the Start Menu (a system requirement for non-packaged apps to send notifications) — this is normal;
- keep the frequency low — users who get spammed will simply turn off notifications for the whole app.

#### Microphone Spectrum (Permission `mic`)

Same pipeline and same return structure as `get_audio_spectrum` (system loopback, no permission needed), but captures **microphone input**:

```js
const { bands, peak } = await window.__DESK_PP__.invoke('get_mic_spectrum', { bands: 32 });
```

Rejects when there is no microphone device (or desktop-app microphone access is disabled in the system's privacy settings); lazy start, auto-release after 30 idle seconds — same as loopback.

### 5.4 Settings Read/Write Commands (No Permission Needed)

#### `skin_get_setting` — Read Your Own Setting Value by Key

```js
const value = await window.__DESK_PP__.invoke('skin_get_setting', { key: 'api_key' });
```

- Only keys **declared in your own skin.json `settings`** can be read; undeclared keys are rejected;
- returns the effective value of the key (the user's saved value, or the schema default if unsaved);
- any type can be read — it is **the only channel for reading `password`-type values** (§4.3); for normal types `__DESK_PP__.settings` is more convenient, no need for this command;
- identity comes from the window itself; a skin cannot read other skins' settings.

#### `skin_set_setting` — Write Your Own Setting Value

Writes the value of a custom setting item into `settings.json` in its own folder, sharing the same data with the manager's "Skin Settings" tab. Usage and contract: see §4.4.

#### Console Output — Forwarded to the Host Log Automatically (Zero Integration)

F12 DevTools is disabled by the platform in skin windows, so the injected bridge automatically forwards the page's console output to the host log — no code needed:

- `console.log/info/debug` are recorded as info, `console.warn` as warning, `console.error` as error;
- uncaught script exceptions (with file and line), unhandled Promise rejections, resource load failures (img/script/link), and CSP violations also land in the log as errors;
- view them in **Manager → Settings → Advanced → Logs → Open Log Window** — the source filter narrows down to a single skin;
- forwarding has built-in flood protection (consecutive duplicates collapse into `(xN)`, batched reporting every 250 ms, overflow dropped with a notice) — still, avoid logging from hot paths (e.g. a `requestAnimationFrame` callback);
- automatic forwarding needs no permission declaration and no integration — existing skins benefit as-is.

#### `skin_log` — Send a Message to the Host Log Window Explicitly

```js
await window.__DESK_PP__.invoke('skin_log', { level: 'warn', message: 'API response empty, using cached data' });
```

- The explicit channel beyond automatic console forwarding: for business events that aren't errors but are worth recording ("fell back to cached data");
- `level` only accepts `'warn'` / `'error'`; anything else (or omitted) is recorded as info (operation event);
- the message goes into the host's in-memory ring buffer (cap 1000 entries, each message truncated to 1000 chars), visible in the log window; the source label automatically carries your skin id (`skin:<id>`) — no need to add one, and it cannot be forged;
- while the log window is closed, messages stay in the backend only — zero frontend overhead;
- no permission declaration needed (messages never leave the local log buffer); the backend only identifies the caller via its window identity.

### 5.5 Call Boundary (Commands Skins Can't Reach)

`__DESK_PP__.invoke` is technically a straight pass-through, but **the backend gates every command by the calling window's identity**:

- Manager-only commands (install/uninstall/load skins, modify window config, modify global settings, autostart, preview capture, etc.) **can only be called from the manager window** — skin calls are rejected: `This command can only be called from the manager window`. Don't try to call them, and don't try to forge an identity (identity comes from the window itself and cannot be forged).
- Bridge-internal commands `start_skin_drag` / `start_skin_resize` / `show_skin_context_menu` are used by the injected script on drag/resize press and right-click; skins must not call them directly.
- The complete list of commands actually available to skins = everything listed in §5.2–§5.4 of this document. This chapter will be updated in sync when future versions add capabilities.

### 5.6 Error-Handling Conventions

- When a command fails, the rejection value is a **human-readable message** (its language follows the manager UI) — fine for displaying in place or logging, but never branch your program logic on the message text.
- "No data" is expressed through return values, not errors: queries return `null` or a flag (`get_media_info` resolves `null` with no playback session, `get_battery_info` uses `has_battery`, `get_foreground_window_info` is `null` in rare cases); only actions reject on failure (`media_control` with no session, `set_volume` failures, and so on).
- To branch on host capabilities, read `__DESK_PP__.hostVersion` and compare numeric version segments — don't call-then-catch to probe whether a feature exists.

---

## 6. Security Model

Driftlet's design premise: **a skin is third-party, network-capable local code**. The platform draws a clear line between you (the author) and the user — this section explains what the platform enforces, what it doesn't, and where your responsibility lies.

### 6.1 Platform-Enforced Boundaries

| Boundary | Description |
|------|------|
| Manager command isolation | Any manager command called from a skin window is rejected by the backend (§5.5); a skin cannot install/uninstall/tamper with other skins or the global config |
| Permission declarations | Sensitive capabilities (§5.3) must be declared in skin.json and are **shown to the user one by one at install time** (flagged in two risk tiers: `shell`/`system` high risk, `registry`/`clipboard`/`mic` medium risk); undeclared means rejected |
| File sandbox | File read/write is confined to the skin's own folder (`..`, absolute paths, colon path segments, and symlink escapes rejected; 32MB/16MB read/write caps); `skin.json` and the user's `settings.json` are read-only to every skin — the sandbox itself is the boundary, so file read/write needs no permission declaration |
| Settings isolation | `settings.json` is never served over `skin://` (including 8.3 short names, NTFS streams, and other variants); a skin's setting values are unreachable by other skins |
| Password protection | `password`-type setting values are never injected into any page (always empty strings in `__DESK_PP__.settings`); only the owning skin window can read them via `skin_get_setting` |
| Package install hardening | `.dskin` extraction guards against path traversal (zip-slip), caps the total by actual extracted bytes (anti zip-bomb), limits file count, and rolls back on staging failure |

### 6.2 What Skins Can Do (Unrestricted)

- Full browser capabilities: DOM, Canvas, WebAudio, `fetch`/`WebSocket` to any external network (cross-origin subject to the target server's CORS; tag-based loading unrestricted);
- **Same origin** with other skins (`http://skin.localhost`): can request other skins' ordinary resource files (e.g. images) — this is inherent to the protocol design; private data is protected by the "settings isolation + password protection" pair (§6.1), so always use `password` setting items for private data, never plain files in the skin folder;
- with declared permissions, the corresponding local capabilities (§5.3).

### 6.3 The Author's Security Responsibilities

The platform can't see inside skin pages; the following are on you:

- **Prevent XSS**: assign dynamic content (user input, task text, network responses) via `textContent`, or escape it before putting it into `innerHTML` — skin setting values and task lists are all user-editable; splicing them straight into HTML is an injection. The example skin `controls-demo` renders everything via `textContent` and demonstrates this correctly.
- **Only load scripts and resources you trust**: skin pages have no CSP protection — one hijacked CDN script gets your full page capabilities (including the command channel of declared permissions). Bundle resources locally where possible.
- **Minimize permissions**: declare only what you actually use — the install page shows users the declaration list; over-declaring directly hurts install willingness and user safety.
- **Declare purposes honestly**: explain what each permission is for on the release page; `shell` and `system` are flagged high risk, `registry`, `clipboard`, and `mic` medium risk.

---

## 7. Development Workflow and Debugging

### 7.1 Three Ways to Install

1. **During development (recommended)**: copy the skin folder directly into `<install dir>\skins\`, click "Refresh" in the manager and it appears; click "Load" to run it. After editing code, right-click the skin → "Reload Skin" for instant effect.
2. **Install via the manager**: click "+ Add Skin" and pick a `.dskin` file; the install wizard pops up (showing skin info and permission declarations); confirm to install.
3. **Double-click install**: the installed app registers the `.dskin` file association; double-clicking a skin package launches Driftlet and enters the same install wizard.

During development you can also test the full double-click install chain with a command-line argument: `npm run tauri dev -- -- "D:\path\x.dskin"`.

Additionally, Settings → Advanced → Developer mode (off by default) bundles two development aids: in debug builds (`npm run tauri dev`), a watcher recursively watches the whole skins directory and a loaded skin reloads automatically after its file changes settle behind a 300 ms debounce — no manual right-click reload needed; in any build, pressing F12 or Ctrl+Shift+I inside a skin window opens DevTools.

### 7.2 Debugging in a Plain Browser

A skin is essentially a web page — layout, styles, and most logic can be debugged by opening `index.html` directly in a browser (or via the frontend preview after `npm run dev`):

- Without the bridge, `window.__DESK_PP__` is `undefined` — use optional chaining `?.` with defaults for all access (§5), and the page renders fine in a bridge-less environment;
- to mock setting values, assign `window.__DESK_PP__ = { settings: { ... } }` manually in the browser console;
- parts depending on backend commands (system info, file read/write) can only be tested for real inside the manager.
- with the `examples/driftlet.js` wrapper, a missing bridge makes every command reject a clear `Error` — catch it and the page renders fine offline.

### 7.3 Troubleshooting

| Symptom | Check first |
|------|------|
| Skin is blank after loading | Whether the entry file name matches `entry`; whether asset references were written as `skin://` directly (use relative paths instead, §3.1) |
| Styles/scripts silently not loading | Same as above — `skin://` subresources can't resolve on Windows; check whether they were written in the absolute `skin://` form |
| Settings changes have no effect | Whether you listen to `desk-setting-changed`; whether `key` matches the declaration |
| password reads as an empty string | This is by design — use `skin_get_setting` instead (§4.3) |
| Command errors "has not declared the '...' permission" | Add the permission to `permissions` in `skin.json`, then **reload the skin** (permissions are read live on every call; no reinstall needed) |
| Command errors "can only be called from the manager window" | You called a manager-only command — see §5.5; switch to the skin commands listed in this document |
| Window position/size not as expected | Trust the "Position & Size" section of the config panel (logical pixels); check whether any element has a fixed size exceeding the window |
| File read/write errors "invalid path" | Paths must be relative to the skin folder; no `..`, drive letters, or colons |
| Want to see a skin's output while it runs in the manager | Open Manager → Settings → Advanced → Logs and filter by your skin — console output and errors land in the log automatically (§5.4); use `skin_log` for explicit business events |

---

## 8. Packaging and Distribution (.dskin)

A skin is ultimately distributed to users as a single `.dskin` file — it is simply a zip archive.

### 8.1 Generate with the pack-skin Tool (Recommended)

`tools/pack-skin.exe` is a **standalone, install-free tool** (~320 KB) — it doesn't need Driftlet, Node.js, or any environment installed; copy the exe anywhere and use it:

```
pack-skin.exe <skin folder> [output directory]
:: e.g. pack-skin.exe .\my-skin  →  produces my-skin-1.0.0.dskin in the current directory
```

Pre-pack validation uses **exactly the same rules** as the install side (strongly-typed skin.json parsing, valid `id`, entry file existence, legal setting item types); failures report the exact line and column — if it packs, it installs:

- Structural errors (e.g. `window.width` written as a string, a nonexistent setting item `type`) are rejected outright with line/column positions;
- a missing `version` prints a warning (not blocking, but update detection degrades — recommended to add it);
- a malformed `min_host_version` (must be a numeric-segment version like `"1.0.5"`) is rejected outright;
- auto-excluded: `settings.json*` (user data), `.git` / `.svn` / `node_modules` directories, existing `*.dskin` artifacts, `.DS_Store` / `Thumbs.db` / `desktop.ini`;
- over-limit is rejected outright: 64 MB archive / 256 MB extracted / 5000 files.

Source lives in `tools/pack-skin/` (Rust); rebuild with `cargo build --release`.

### 8.2 Manual Packaging

1. Make sure `skin.json` declares `id` and `version`.
2. Zip the skin files: `skin.json` must sit at the archive **root** (or under exactly one same-named folder); don't mix multiple skins into one package; don't include `settings.json`.
3. Rename the extension from `.zip` to `.dskin` (a plain rename; contents unchanged).

### 8.3 Install/Update Behavior on the User Side

- Both entry points (the manager's "+ Add Skin", double-clicking a `.dskin`) share the same install wizard: it first validates whether it's a legal skin package (can `skin.json` be found and parsed, does `id` exist, does the entry file exist), clearly stating the reason if invalid; the confirmation page shows the skin's name, version, author, description, and **all permission declarations** (flagged in two risk tiers: `shell`/`system` high risk, `registry`/`clipboard`/`mic` medium risk).
- When the same `id` is already installed, version numbers decide: higher version → "Update", same version → "Reinstall" (overwrite), lower version → "Downgrade" — all labeled on the confirmation page.
- **Update/reinstall/downgrade all keep the user's settings data**: window config is stored per `id` in the global config.json, decoupled from skin files; user values from the "Skin Settings" tab live in `settings.json` in the skin folder — taken out before install and written back after replacement (user values win over a same-named file in the package). New setting items use defaults; data of removed setting items lapses automatically.
- `settings.json` (including `.bak` / `.tmp`) is **user data and should never enter a distribution package** — `pack-skin.exe` excludes it automatically; don't include it in manual packaging either (even if it slips in, it gets overwritten by the user's existing values at install time).
- A running skin is unloaded first and **stays unloaded** after update/reinstall/downgrade — the user reloads it in the manager when needed (the install wizard's "Load Now" works too).

---

## 9. Release Checklist

Go through these one by one before packaging:

- [ ] `skin.json` declares a valid `id` (lowercase letters/digits/dashes, not a reserved device name) and `version`
- [ ] `skin.json` fields are complete (name / author / version / description); file < 1 MB
- [ ] All assets are inside the skin folder, referenced with **relative paths**; no external CDN dependencies (or graceful offline degradation confirmed acceptable)
- [ ] No opaque background covering the desktop under transparency
- [ ] Drag areas use `.drag-region`; no `-webkit-app-region: drag`
- [ ] Verified in the manager by shrinking the window to half and doubling it: content adapts fully — no clipping, no window-level scrollbars (§3.3)
- [ ] If `settings` is declared: `key`s all unique, types correct (one of the 20), `default`s match their types
- [ ] If `"bilingual": true` is declared: `group_en` is set for every control in a group (avoids split groups in the English UI), and every field that needs bilingual has its `*_en` (§4.5)
- [ ] Setting reads have fallbacks (`?.` + `??`), and `desk-setting-changed` is listened to for live application
- [ ] `password`-type values are read via `skin_get_setting`, not relying on values in `__DESK_PP__.settings` (always empty strings)
- [ ] Dynamic content is rendered via `textContent` or escaped; no user-editable values spliced raw into `innerHTML` (§6.3)
- [ ] If sensitive APIs are used: `permissions` declares only what's actually used (§2.3), and the declaration list shown in the install wizard is one you can stand behind
- [ ] Bridge-less scenarios (opened in a plain browser) don't throw (`?.` defense, §7.2)
- [ ] If you rely on commands/controls introduced in a newer version: declared `min_host_version` (install-time warning), or degrade at runtime via `__DESK_PP__.hostVersion`
- [ ] A `preview.png` is provided, or a preview was captured in the manager
- [ ] Packed with `pack-skin.exe` into a `.dskin`, and actually installed/updated once in the manager to verify (including the permission declaration display)

---

## 10. Example Skins

The repo ships four example skins. `controls-demo` is the reference implementation of the settings system and page conventions; `sys-monitor` / `media-hub` / `toolbox` together cover all backend commands callable by skins (§5.2–§5.4) and are the reference for API usage. Strongly recommended to read their code before starting:

| Skin | What it demonstrates |
|------|----------|
| `examples/controls-demo` | All 20 setting control types + groups + descriptions + Chinese/English bilingual (§4.5), HTML/CSS/JS split with relative-path references, light nautical-chart UI, the fill + internal scroll paradigm, the schema read via `fetch('skin.json')` on a relative path with control labels/groups/options rendered per the bridge language, `desk-language-changed` driving the UI language to follow the manager instantly (§4.5), setting values live-applied in the demo area (accent color / progress bar / status dot / font / day-night tint / panel density / stepper-driven ticker interval), `password`-type values read via `skin_get_setting`, and rendering entirely with DOM APIs / `textContent` |
| `examples/sys-monitor` | The full §5.2 read-only system-info set: CPU (total bar + per-thread mini bars) / GPU / memory / disks (incl. per-volume space) / network (rates + local IPs) / OS / top-5 processes (sortable by CPU or memory) / battery / idle time / foreground window / monitors; rate readings poll every 1s (first call is a zero baseline), static info reads once at startup, polling pauses while the page is hidden. **Zero permission declarations** |
| `examples/media-hub` | Volume read/set/mute, SMTC media info (cover / progress / status) and playback control (play_pause/next/previous), dual-source spectrum from system loopback and microphone (live canvas bars + peak line, paused while hidden, device auto-released ~30s after polling stops), toast notifications; permissions `system` + `mic` |
| `examples/toolbox` | Clipboard read/write, skin-directory file write/read/list/delete, read-only registry (preset + custom keys), command execution (preset `ver`/`ipconfig` + custom, showing code/stdout/stderr), opening links (including a rejected `.exe` target demo), `skin_get_setting` / `skin_set_setting` (the only read channel for `password` values, writing settings back, syncing manager-side edits via `desk-setting-changed`); permissions `registry` / `shell` / `clipboard` / `system` |

The four skins `controls-demo` / `sys-monitor` / `media-hub` / `toolbox` also follow: bilingual UI that follows the manager language, dynamic content rendered exclusively via `textContent` / DOM APIs, no crashes when the bridge is missing (plain-browser preview), and rejected-command error text displayed inline in the corresponding card.

`controls-demo` and `sys-monitor` declare no `permissions` — every capability they use is permission-free.
