# Changelog

Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this project follows [Semantic Versioning](https://semver.org/).

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
