# Changelog

Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this project follows [Semantic Versioning](https://semver.org/).

## [0.7.0] - 2026-11-18
### Changed
- **Plume is now called Breezedesk** (another project already used the name Plume).
  - Program: `breezedesk.exe` and `breezedesk-editor.exe`, installed in `%LOCALAPPDATA%\Programs\Breezedesk`.
  - Widgets: `%APPDATA%\Breezedesk\widgets`. Lua: `breezedesk.get(...)` etc.
  - Widget packages use the `.breeze` extension.
- Automatic migration from Plume: installed copies update themselves to Breezedesk and the installer moves the widgets, settings, "Start with Windows" and saved positions, then removes the old program, shortcuts and uninstall entry. Portable copies move `%APPDATA%\Plume` on first start.
- Still compatible: `.plume` packages install, Lua scripts using `plume.*` work, `plume.state` is read.

## [0.6.2] - 2026-11-18
### Fixed
- Clicking the desktop no longer brings the widgets above open windows. "Show desktop" (Win+D) is now detected by checking that the desktop really is in front of every window, not just that it is the active window.
- Widgets no longer disappear behind the desktop (e.g. after clicking the taskbar): "bottom" widgets are kept right above the desktop window instead of at the very bottom of the window stack.

## [0.6.1] - 2026-11-18
### Fixed
- Dragging widgets: they always follow the mouse now. Snapping only considers the screen edges and neighbouring widgets, so a widget no longer gets stuck or moves along one axis only.
- Widgets can be dragged from elements that only react to scrolling, right-click or hover (the calendar, news list, music cover...); only elements with a left/middle click action block dragging.

## [0.6.0] - 2026-11-18
### Added
- **Plume Editor** (`plume-editor.exe`): edit widgets visually (fields, colors, elements, variables, data sources), turn widgets on/off, create and delete widgets, with auto-save. Comments in the files are kept. *Edit widgets...* in the tray menu opens it.
- **Widget packages** (`.plume`): double-click to install (with a confirmation that lists the widgets); export from the editor. Only widget file types are accepted and paths cannot leave the widgets folder.
- **High-DPI support**: widgets follow each monitor's scaling (125%, 150%, 4K...) and stay sharp; `scale` per widget.
- **Text**: word wrapping (`wrap`), `max_lines` with "…", `bold` and `italic`.
- **Dates in your language** (day and month names), or any language with `time(fmt, 'pt_BR')`.
- **Calendar** element and example.
- **Animations**: `animate = 300` makes bars and rings slide to new values.
- **More data**: per-core CPU (`cpu_core(n)`, `cpu_cores`), top processes (`top_name(i)`, `top_cpu(i)`, `top_mem(i)`, `topmem_*`), `ping(host)`, LibreHardwareMonitor sensors (`sensor(name)`, and better `cpu_temp`/`gpu_temp`).
- Tray menu: show/hide each widget (remembered), *Lock positions*.
- Dragged widgets snap to screen edges and to other widgets (Shift = free).
- Widgets left on a disconnected monitor are moved back on screen.
- New example: `processes.toml`.

### Changed
- Positions and hidden widgets are saved in `plume.state` (read from `positions.state` the first time).
- Fewer false antivirus alerts:
  - `plume.exe` now carries its icon, version information and an application manifest (runs as the normal user, DPI aware).
  - Releases are built with the Microsoft toolchain (MSVC) on Windows.
  - The installer and uninstaller ask Plume to close instead of killing the process.
- Identical frames are not sent to Windows again; widgets with a visualizer redraw the rest of the widget only 4 times per second.

## [0.5.0] - 2026-11-17
### Added
- **Installer** (`Plume-Setup.exe`): installs for the current user only (no admin rights), with a Start menu shortcut, an uninstaller and an option to start with Windows.
- **Automatic updates** from GitHub Releases: download, SHA-256 check, silent install and restart. *Check for updates* and *Automatic updates* are in the tray menu.
- The tray menu shows the running version.
- **Releases are automated**: every push to `main` publishes a release. The `dev` branch gets CI builds for testing.

### Changed
- Installed copies keep widgets in `%APPDATA%\Plume\widgets`. A `widgets` folder next to the `.exe` still works (portable mode).

## [0.4.1] - 2026-11-17
### Fixed
- Widgets no longer disappear when "Show desktop" (Win+D) is used: while the desktop is in front, desktop-level widgets are raised above it, then put back.
- Smoother updates: sensors (CPU, disks, temperatures, GPU...) are now read in a background thread, so slow readings no longer freeze drawing every second.
- Widgets that update every second (or slower) are lined up with the clock, so clocks never skip or repeat a second.
- The media example updates at ~32 fps for a smoother visualizer.

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
