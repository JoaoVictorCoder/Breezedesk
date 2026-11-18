# Plume

Lightweight desktop widgets for Windows, in the spirit of Rainmeter, written in Rust and built to use as little CPU and RAM as possible.

**Website:** https://joaovictorcoder.github.io/Plume/

![preview](preview.png)

## Features
- Transparent widgets drawn on the desktop, always on top or in a normal window layer. You can drag them, make them click-through and set their opacity.
- Text (with wrapping, bold and italic), rectangles, bars, line graphs, circular gauges, images, a month calendar and an audio visualizer.
- **Plume Editor**: edit widgets visually, without touching TOML.
- **Widget packages** (`.plume`): share widgets as one file; double-click to install.
- Sharp on high-DPI screens (125%, 150%, 4K) and across monitors with different scaling.
- Variables, math and conditions in any text, color or value: `{cpu > 80 ? 'HOT' : 'ok'}`.
- CPU (total and per core), RAM, GPU, VRAM, disks, temperatures, network, ping, battery, top processes, uptime and date/time (in your language).
- Reads LibreHardwareMonitor sensors when it is running (CPU/GPU temperatures on any PC, fans, voltages...).
- Now playing (Spotify, browsers, players...) with cover art and media controls.
- Internet data: any JSON API, plus RSS/Atom feeds.
- **LuaJIT** scripts for anything the TOML can't express.
- Click, right-click, middle-click and scroll actions: open apps, URLs and folders, control media and volume, set variables, call Lua.
- Hover effects, smooth animations, rotation, blur and acrylic backgrounds.
- Snap to screen edges and to other widgets while dragging; lock positions; widgets left on a disconnected monitor come back.
- Widgets reload automatically when you save their files.
- A system tray icon, an option to start with Windows, and a log file.

## Why it is light
- Talks to the Win32 API directly: no UI framework, no webview.
- Each widget is a *layered* window (`UpdateLayeredWindow`) with per-pixel transparency.
- Drawing is done in software (`tiny-skia`), and text with `fontdue` plus a glyph cache.
- **Only what is used gets measured.** If no widget shows the GPU, the GPU is never queried. The audio, media and internet background threads only start when a widget needs them, and audio capture stops when nothing reads it.
- System metrics are sampled at most once per second and shared by every widget.
- Hidden widgets are not redrawn at all.
- Downloads use WinHTTP, which is built into Windows, so no TLS library is bundled.

## Install
Download **`Plume-Setup.exe`** from the [latest release](https://github.com/JoaoVictorCoder/Plume/releases/latest) and run it.
- It installs for your user only, so it needs no administrator rights. The program goes to `%LOCALAPPDATA%\Programs\Plume`.
- It adds a Start menu shortcut and an entry in *Apps & features* to uninstall it.
- **Your widgets** live in `%APPDATA%\Plume\widgets`. Installs and updates never touch them, and the uninstaller asks before deleting them.
- Windows may show a SmartScreen warning because the program is not signed: click *More info* → *Run anyway*.
- Some antivirus programs (e.g. Windows Defender's `Wacatac!ml`) can flag new, unsigned programs by mistake. The `!ml` means a machine-learning guess, not a known virus. The full source code is in this repository, and false positives can be reported to Microsoft at https://www.microsoft.com/en-us/wdsi/filesubmission.

**Portable:** download `Plume-X.Y.Z-portable.zip` instead, extract it anywhere and run `plume.exe`. A `widgets` folder next to the `.exe` makes Plume portable: it keeps everything in that folder.

## Updates
Installed copies check GitHub for a new release 20 seconds after starting and then every 6 hours. A new version is downloaded, checked against its published SHA-256, installed silently, and Plume restarts. You get a notification when this happens.
- To turn this off, uncheck *Automatic updates* in the tray menu. Plume then only tells you that an update exists.
- *Check for updates* in the tray menu checks right away.
- Portable copies are never changed: they only tell you that an update exists.

## Usage
- **Tray icon:** left click shows/hides all widgets; right click opens the menu.
- **Menu** (tray icon or right click on a widget): *Show/Hide widgets*, *Widgets* (show/hide each one), *Lock positions*, *Edit widgets...*, *Reload widgets*, *Open widgets folder*, *Install example widgets*, *Open log*, *Start with Windows*, *Check for updates*, *Automatic updates* and *Exit*.
- **Drag:** drag any part of a widget that is not clickable. It snaps to screen edges and other widgets (hold **Shift** to place it freely). Positions and hidden widgets are saved in `widgets\plume.state`.
- **Edit visually:** *Edit widgets...* opens Plume Editor (also in the Start menu). With *Auto-save* on, changes appear on the desktop while you edit.
- **Packages:** double-click a `.plume` file to install its widgets (you are asked first). In the editor, *Export as package...* creates one from a widget, with its script and images.
- **Errors** in widget files show up as a notification and are written to `widgets\plume.log`.
- **Other folder:** `plume.exe C:\my\widgets` uses that folder instead.
- **PNG preview** (works on any OS): `plume --preview widgets`

## Examples
| File | Shows |
|---|---|
| `clock.toml` | Digital clock with a blurred background |
| `system.toml` | CPU graph with a conditional color, RAM, GPU, disk, temperatures, network, battery |
| `analog-clock.toml` | Rotation driven by expressions, hover opacity |
| `gauges.toml` | Circular gauges, colors from `[variables]`, acrylic background |
| `media.toml` | Now playing, cover art, media buttons, volume on scroll, audio visualizer |
| `weather.toml` + `weather.lua` | JSON API (open-meteo, no key) + LuaJIT script |
| `news.toml` | RSS reader with clickable headlines and paging (`!set`) |
| `launcher.toml` | Image, app buttons with hover effects |
| `calendar.toml` | Month calendar in your language, browse months with clicks/scroll |
| `processes.toml` | Top processes, per-core CPU bars (animated), ping |

Set `enabled = false` in a file to turn a widget off.

## Widget format (`widgets/*.toml`)
```toml
name = "my-widget"      # unique
enabled = true
x = 100
y = 100
width = 250
height = 100
update_ms = 1000        # how often data is refreshed (16 = 60 fps)
z = "bottom"            # bottom (on the desktop) | normal | top (always on top)
opacity = 255           # 0-255
hover_opacity = 255     # opacity while the mouse is over the widget (optional)
scale = 1.0             # size multiplier, on top of the monitor's scaling
draggable = true
click_through = false
font = "C:/Windows/Fonts/segoeui.ttf"
script = "my-widget.lua"   # optional, LuaJIT

[variables]             # your own values; strings can contain {expressions}
accent = "#5AC8FA"
limit = 80
label = "CPU {cpu}%"

[background]
color = "#101018C0"     # #RRGGBB or #RRGGBBAA, or an expression
radius = 12
border = "#FFFFFF30"
border_width = 1
blur = "none"           # none | blur | acrylic

[[web]]                 # data from the internet (optional, any number)
name = "weather"
url = "https://api.example.com/data?city={city}"
interval = 600          # seconds
format = "json"         # json | rss | text

[[element]]
type = "text"
x = 10
y = 10
text = "{label}"
```

### Elements
| `type` | Fields |
|---|---|
| `text` | x, y, text, size, color, align (`left`/`center`/`right`), font, bold, italic, wrap (width in pixels), max_lines |
| `rect` | x, y, w, h, color, radius, border, border_width |
| `bar` | x, y, w, h, value, min (0), max (100), color, background, radius, vertical |
| `graph` | x, y, w, h, value, min, max, color, fill, line_width, step |
| `ring` | x, y (center), radius, thickness, value, min, max, color, background, start (-90), sweep (360), round_caps |
| `image` | x, y, w, h, path, hover_path, radius, opacity (0-1). PNG, JPG, BMP or ICO. |
| `spectrum` | x, y, w, h, bars, color, gap, radius. Shows what the speakers are playing. |
| `calendar` | x, y, w, h, size, color, dim_color, today_color, today_text, header, monday_first, offset (months, an expression) |

Fields every element has:

| Field | |
|---|---|
| `visible` | Expression; the element is only drawn while it is true. |
| `on_click`, `on_right_click`, `on_middle_click`, `on_scroll_up`, `on_scroll_down` | One action or a list of actions. |
| `hover_color` | Color used while the mouse is over the element. |
| `rotate` | Degrees; can be an expression (`"second * 6"`). |
| `rotate_center` | `[x, y]` pivot, in widget coordinates. Defaults to the element's center. |
| `animate` | Bars and rings slide to new values over this many milliseconds (e.g. `300`). |

Colors can be fixed (`"#FF8800"`) or expressions (`"cpu > 80 ? '#FF3B30' : accent"`).

### Expressions
Everything inside `{...}` in text, and the `value`, `visible`, `rotate` and color fields, is an expression:

```
cpu * 2                     + - * / %
cpu > 80 ? "HOT" : "ok"     comparisons, && || ! (also and/or/not), ternary
if(battery < 20, "low", "") functions
weather.current.temp        dotted paths into web data and Lua tables
"CPU " + cpu + "%"          joins text
```
Numbers keep a sensible number of decimals: `cpu` shows none, `ram_used` shows one. Use `round(x, n)` to choose.

**Functions:** `round(x, n)`, `floor`, `ceil`, `abs`, `min`, `max`, `clamp(x, lo, hi)`, `if(c, a, b)`, `default(x, fallback)`, `is_null(x)`, `num`, `str`, `upper`, `lower`, `len`, `trunc(text, n)`, `pad(x, width)`, `fmt_bytes(n)`, `fmt_time(seconds)`, `time(format, locale)`, `cpu_core(n)`, `top_name(i)`, `top_cpu(i)`, `top_mem(i)`, `topmem_name(i)`, `topmem_cpu(i)`, `topmem_mem(i)`, `ping(host)`, `sensor(name)`, `disk(drive)`, `disk_used(drive)`, `disk_free(drive)`, `disk_total(drive)`, `get('dynamic.path')`, `web_ready(name)`, `lua(function, args...)`.

**Variables:**

| | |
|---|---|
| CPU / memory | `cpu`, `cpu_cores`, `cpu_core(n)` (n from 0), `ram`, `ram_used`, `ram_total`, `ram_free` (GB) |
| Processes | `top_name(i)`, `top_cpu(i)` (%), `top_mem(i)` (MB): i-th program using the most CPU (from 0). `topmem_*` sorts by memory. |
| Network latency | `ping('1.1.1.1')` in ms (`null` without an answer), measured every 5 s |
| GPU | `gpu` (%), `gpu_mem` (%), `gpu_mem_used`, `gpu_mem_total` (GB) |
| Temperatures | `cpu_temp`, `gpu_temp` (°C); `sensor('CPU Package')` reads any LibreHardwareMonitor sensor by name or identifier |
| Disk | `disk`, `disk_used`, `disk_free`, `disk_total` (drive C:, or use the functions) |
| Network | `net_down`, `net_up` (text, e.g. "1.2 MB/s"), `net_down_bytes`, `net_up_bytes` |
| Battery | `has_battery`, `battery`, `battery_charging`, `battery_plugged`, `battery_time` |
| Media | `media_title`, `media_artist`, `media_album`, `media_playing`, `media_active`, `media_status`, `media_position`, `media_duration` (s), `media_progress` (%), `media_cover` (use as an image `path`) |
| Audio | `audio_level` (0-100) |
| Time | `hour`, `hour12`, `minute`, `second`, `day`, `month`, `year`, `weekday`, `uptime`, `uptime_s`, `time('%H:%M')` ([chrono format](https://docs.rs/chrono/latest/chrono/format/strftime/)). Day and month names follow the Windows language; `time('%A', 'en_US')` picks another. |

A sensor that is not available gives `null`, which shows as empty text. Use `default(gpu_temp, '-')` to show something else.

### Actions
```
https://example.com            open a URL, file, folder or program
"C:\Path\app.exe" --arg        program with arguments (quote the path)
assets/readme.txt              paths are relative to the widgets folder
!reload  !exit                 reload widgets / quit Plume
!hide  !show  !toggle          all widgets
!toggle:clock                  show/hide one widget by name
!refresh                       update this widget now
!set:name=expression           set a variable (e.g. !set:page = page + 1)
!lua:function                  call a function of the widget's script
!media:playpause | next | prev | stop
!volume:up | down | mute
```
Actions are templates too: `on_click = "{news.items.0.link}"`.

### Lua scripts (LuaJIT)
```lua
-- update() runs on every widget update; the table it returns becomes variables.
function update()
  local cpu = plume.get("cpu")
  return { status = cpu > 80 and "Busy" or "Idle", info = { cores = 8 } }  -- {status}, {info.cores}
end

-- Any function can be used in expressions ({lua('double', cpu)}) or actions (!lua:clicked).
function double(x) return x * 2 end
function clicked() plume.set("count", (plume.get("count") or 0) + 1) end
```
- **Host API:** `plume.get(name)`, `plume.eval("expression")`, `plume.set(name, value)` (persistent variable), `plume.run("action")`, `plume.log(...)`, `plume.widget`.
- **Optional `init()`:** called once after the script loads.
- **Sandbox:** scripts get `string`, `table`, `math`, `bit`, `jit` and `os.time/date/clock`. File access (`io`), `require` and running programs are not available, so a downloaded widget cannot touch your files.

## Notes and limits
- **Temperatures:** with [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor) running, CPU and GPU temperatures (any brand) come from it. Without it, the CPU temperature uses Windows' ACPI sensor, which many PCs do not expose, and the GPU temperature works with NVIDIA drivers. A missing sensor shows as `null`.
- **Blur/acrylic** uses the same Windows API as Rainmeter's blur plugins. On some Windows 10 builds acrylic makes dragging feel slow; use `blur` there.
- **Media** works with any app that shows up in the Windows media overlay.
- Symbols missing from the chosen font (♪ ☀ ⏯ ...) are taken automatically from Segoe UI Symbol or Segoe UI Emoji.

## Project layout
| File | Role |
|---|---|
| `src/config.rs` | TOML format |
| `src/expr.rs` | Expression language and templates |
| `src/widget.rs` | Widget runtime: variables, script and state |
| `src/render.rs` | Drawing (cross-platform) |
| `src/metrics.rs`, `src/probe.rs` | System data, battery, GPU, temperatures |
| `src/media.rs`, `src/audio.rs` | Now playing, audio loopback + FFT |
| `src/web.rs` | Downloads, JSON and RSS |
| `src/script.rs` | LuaJIT |
| `src/actions.rs` | Click actions |
| `src/update.rs` | Automatic updates from GitHub Releases |
| `src/package.rs` | `.plume` widget packages (shared with the editor) |
| `src/locale.rs`, `src/lhm.rs`, `src/ping.rs` | Date language, LibreHardwareMonitor sensors, ping |
| `editor/` | Plume Editor (egui), a separate program: `plume-editor.exe` |
| `src/win.rs`, `src/tray.rs` | Windows, mouse, timers, auto-reload, blur, tray, settings |
| `installer/plume.nsi` | Windows installer (NSIS) |
| `scripts/` | Version picking and packaging (used by the workflows) |
| `.github/workflows/` | CI on `dev`, releases on `main` |

## Development
### Branches and releases
| Branch | What happens on push |
|---|---|
| `dev` | **CI** builds and tests. The installer and the portable zip are attached to the workflow run as an artifact (*Actions* → the run → *Artifacts*). Users are not affected, and these dev builds never update themselves. |
| `main` | **Release**: builds, tests and publishes a GitHub release. Installed copies update to it automatically. |

Day-to-day flow:
1. Work and push on `dev`, then test the build from the CI artifact.
2. When it is good, open a pull request `dev` → `main`. CI runs on the PR, and merging it publishes the release.

To protect `main` from direct pushes, go to *Settings* → *Branches* → add a rule for `main` with *Require a pull request before merging*.

### Version numbers
The release workflow uses the version in `Cargo.toml` if no release has it yet. Otherwise it releases the latest version with the last number + 1. Bugfix releases need no change. For new features or breaking changes, bump `version` in `Cargo.toml` (e.g. `0.5.3` → `0.6.0`) in the pull request.

### Building
On Windows: `cargo build --release` (needs the *Desktop development with C++* tools).

Releases are built on Windows with the Microsoft toolchain (MSVC), then packaged on Linux. To build and package everything on Linux instead (cross-compiling):
```bash
sudo apt-get install gcc-mingw-w64-x86-64 nsis zip
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
scripts/package.sh 0.5.0        # → dist/ (installer, checksum, portable zip)
```

## License
[MIT](LICENSE)
