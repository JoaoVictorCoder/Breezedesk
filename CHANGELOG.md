# Changelog

Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this project follows [Semantic Versioning](https://semver.org/).

## [0.4.0] - 2026-11-16
### Added
- **Expressions** everywhere: `{cpu * 2}`, conditions, ternaries, functions (`round`, `default`, `trunc`, `fmt_time`...), dynamic colors, `visible` conditions.
- **`[variables]`** per widget, `!set:name=expression` to change them.
- **LuaJIT scripts** (`script = "file.lua"`), sandboxed, with `update()`, `init()` and the `plume.*` host API.
- **Click actions**: left, right, middle click and scroll. Actions open URLs, files and programs, control media and volume, set variables, call Lua, and reload, hide or toggle widgets.
- **Hover effects**: `hover_color`, `hover_path` for images, `hover_opacity` for the whole widget. The cursor becomes a hand over clickable elements.
- New elements: **image** (PNG/JPG/BMP/ICO, rounded corners), **ring** (circular gauge), **spectrum** (audio visualizer).
- **Rotation** for any element (`rotate`, `rotate_center`), including text and images.
- New data: **disk**, **battery**, **GPU** usage and VRAM, **CPU/GPU temperatures**, time parts (`hour`, `minute`...).
- **Now playing**: title, artist, album, position, cover art.
- **Internet data** (`[[web]]`): JSON APIs and RSS/Atom feeds, read with dotted paths.
- **Blur and acrylic** backgrounds.
- **Auto-reload** when a widget file changes.
- Errors are shown as tray notifications and written to `plume.log`. *Open log* and *Install example widgets* were added to the menu.
- `enabled = false` turns a widget off.
- Characters missing from a font fall back to Segoe UI Symbol / Emoji.
- New examples: analog clock, gauges, media, weather (with Lua), news, launcher.

### Changed
- `source = "cpu"` is now `value = "cpu"` (any expression). `source` still works.
- Only the data some widget uses is measured. Background threads start on demand.

## [0.3.0] - 2026-11-16
### Changed
- The whole project is now in English: code, comments, UI text, examples and docs.
- Renamed to **Plume** (formerly Pluma).
- Example widgets renamed to `clock.toml` and `system.toml`; the date now uses ISO format (`YYYY-MM-DD`).

## [0.2.0] - 2026-11-16
### Added
- System tray icon: left click shows/hides the widgets, right click opens the menu.
- *Show/Hide widgets* and *Start with Windows* menu entries.
- The tray icon comes back automatically if Explorer restarts.

### Changed
- Renamed to Pluma (formerly RustMeter).
- Hidden widgets are not redrawn (saves CPU).

## [0.1.0] - 2026-11-16
### Added
- Widgets in transparent windows (UpdateLayeredWindow), configured with TOML files.
- `text`, `rect`, `bar` and `graph` elements; variables for CPU, RAM, network, uptime and time.
- Drag widgets with the mouse; positions are saved.
- Right-click menu: Reload, Open folder and Exit.
- `--preview` mode that renders the widgets to PNG.
