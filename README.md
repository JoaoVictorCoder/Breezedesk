# Plume

Lightweight desktop widgets for Windows, in the spirit of Rainmeter, written in Rust and built to use as little CPU and RAM as possible.

![preview](preview.png)

## Why it is light
- Talks to the Win32 API directly (`windows-sys`): no UI framework, no webview, no script runtime.
- Each widget is a *layered* window (`UpdateLayeredWindow`) with per-pixel transparency.
- Drawing is done in software with `tiny-skia`, and text with `fontdue` plus a glyph cache.
- System metrics are sampled at most once per second and shared by every widget.
- The process only wakes up on each widget's `update_ms`. The rest of the time it sleeps in `GetMessage`.
- Hidden widgets are not redrawn at all.

## Build and run (on Windows)
```
cargo build --release
target\release\plume.exe
```
On first run it creates a `widgets\` folder next to the `.exe` with two examples.
You can also point it at another folder: `plume.exe C:\my\widgets`.

- **Drag:** click and drag a widget. Its position is saved in `widgets\positions.state`.
- **Tray icon:** left click shows/hides the widgets; right click opens the menu.
- **Menu** (tray icon or right click on a widget): *Show/Hide widgets*, *Reload widgets*, *Open widgets folder*, *Start with Windows* and *Exit*.
- **PNG preview** (works on any OS): `plume --preview widgets`

## Widget format (`widgets/*.toml`)
```toml
name = "my-widget"      # unique
x = 100
y = 100
width = 250
height = 100
update_ms = 1000
z = "bottom"            # bottom (on the desktop) | normal | top (always on top)
opacity = 255
draggable = true
click_through = false
font = "C:/Windows/Fonts/segoeui.ttf"

[background]
color = "#101018C0"     # #RRGGBB or #RRGGBBAA
radius = 12
border = "#FFFFFF30"

[[element]]
type = "text"           # text | rect | bar | graph
x = 10
y = 10
size = 20
align = "left"
color = "#FFFFFF"
text = "CPU {cpu}% - {time:%H:%M:%S}"
```

| Element | Fields |
|---|---|
| `text`  | x, y, text, size, color, align (left/center/right), font |
| `rect`  | x, y, w, h, color, radius |
| `bar`   | x, y, w, h, source (`cpu`/`ram`), color, background, radius |
| `graph` | x, y, w, h, source, color, fill, line_width |

Text variables: `{cpu}` `{ram}` `{ram_used}` `{ram_total}` `{net_down}` `{net_up}` `{uptime}` `{time:FORMAT}` ([chrono format](https://docs.rs/chrono/latest/chrono/format/strftime/)).

## Project layout
- `src/config.rs`: TOML format
- `src/metrics.rs`: CPU, RAM, network, time and variables
- `src/render.rs`: drawing (cross-platform)
- `src/win.rs`: windows, dragging, menu and timers (Win32)
- `src/tray.rs`: tray icon and start with Windows

## Roadmap
Auto-reload when a `.toml` changes, images (PNG), disk/GPU/battery, click actions and plugins.
