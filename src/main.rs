#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Off Windows only `--preview` exists, so the interactive parts are unused there.
#![cfg_attr(not(windows), allow(dead_code))]

mod actions;
mod audio;
mod config;
mod expr;
mod locale;
mod lhm;
mod log;
mod media;
mod metrics;
mod package;
mod ping;
mod probe;
mod render;
mod script;
mod snap;
#[cfg(windows)]
mod tray;
mod update;
mod web;
mod widget;
#[cfg(windows)]
mod win;

use std::path::{Path, PathBuf};

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Command line: `plume [widgets-folder] [--preview] [--install-package file.plume]`.
/// A `.plume` file given alone (double-click, drag onto the exe) is installed.
struct Args {
    dir: Option<PathBuf>,
    preview: bool,
    package: Option<PathBuf>,
}

fn args() -> Args {
    let mut a = Args { dir: None, preview: false, package: None };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--preview" => a.preview = true,
            "--install-package" => a.package = it.next().map(PathBuf::from),
            _ if arg.starts_with("--") => {}
            _ if Path::new(&arg).extension().is_some_and(|e| e.eq_ignore_ascii_case(package::EXT)) => {
                a.package = Some(PathBuf::from(arg));
            }
            _ => a.dir = Some(PathBuf::from(arg)),
        }
    }
    a
}

/// Widgets folder:
/// 1. the folder given on the command line;
/// 2. `widgets\` next to the executable, if it exists (portable copy, zip);
/// 3. `%APPDATA%\Plume\widgets` (installed copy; survives updates).
fn widgets_dir(arg: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = arg {
        return dir;
    }
    let portable = exe_dir().join("widgets");
    if portable.exists() {
        return portable;
    }
    let dir = std::env::var_os("APPDATA")
        .map(|p| PathBuf::from(p).join("Plume").join("widgets"))
        .unwrap_or(portable);
    if !dir.exists() {
        // First run: write the examples.
        install_examples(&dir);
    }
    dir
}

/// Writes the example widgets that are missing (never overwrites). Returns how many.
pub fn install_examples(dir: &Path) -> usize {
    let mut n = 0;
    for (name, bytes) in config::EXAMPLES {
        let path = dir.join(name);
        if path.exists() {
            continue;
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::write(&path, bytes).is_ok() {
            n += 1;
        }
    }
    n
}

/// `plume --preview [folder]` renders each widget to a PNG (works on any OS).
fn preview(dir: &Path) {
    let (cfgs, errors) = config::load_dir(dir);
    for e in errors {
        eprintln!("error: {e}");
    }
    let mut m = metrics::Metrics::new();
    let web = web::Web::default();
    let mut widgets: Vec<widget::Widget> = cfgs
        .into_iter()
        .map(|c| {
            let (w, errors) = widget::Widget::new(c, &m);
            errors.iter().for_each(|e| eprintln!("error: {e}"));
            w
        })
        .collect();
    for w in &mut widgets {
        for job in w.web_jobs(&m, &web) {
            web.fetch_now(&job);
        }
    }
    let secs: u32 = std::env::var("PLUME_PREVIEW_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(5);
    m.refresh();
    for _ in 0..secs {
        std::thread::sleep(std::time::Duration::from_millis(950));
        m.refresh();
        for w in &mut widgets {
            w.update(&m, &web);
        }
    }
    let (mut fonts, mut images) = (render::Fonts::default(), render::Images::default());
    for mut w in widgets {
        let scale: f32 = std::env::var("PLUME_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(1.0) * w.cfg.scale;
        let (pw, ph) = ((w.cfg.width as f32 * scale).round() as u32, (w.cfg.height as f32 * scale).round() as u32);
        let Some(mut pm) = tiny_skia::Pixmap::new(pw.max(1), ph.max(1)) else { continue };
        render::render(&mut w, &mut pm, &m, &web, &mut fonts, &mut images, scale, render::Layer::All);
        let out = format!("preview-{}.png", w.cfg.name);
        pm.save_png(&out).expect("failed to save png");
        println!("{out}");
    }
}

/// Installs a `.plume` package after asking. Returns true if Plume should
/// start afterwards (it was not running yet).
fn install_package(file: &Path, dir: &Path) -> bool {
    let pkg = match package::Package::open(file) {
        Ok(p) => p,
        Err(e) => {
            message(&format!("Could not open {}:\n\n{e}", file.display()), false);
            return false;
        }
    };
    let names = pkg.widgets().join(", ");
    let conflicts = pkg.conflicts(dir);
    let n = pkg.files.len();
    let mut text = format!("Install these widgets?\n\n{names}\n\n{n} file{} will go to {}", if n == 1 { "" } else { "s" }, dir.display());
    if !conflicts.is_empty() {
        text.push_str(&format!("\n\nThese files will be replaced:\n{}", conflicts.join("\n")));
    }
    text.push_str("\n\nWidgets can open programs and websites when you click them, and run Lua scripts (without file access). Only install packages you trust.");
    if !message(&text, true) {
        return false;
    }
    match pkg.install(dir) {
        Ok(_) => !plume_running(),
        Err(e) => {
            message(&format!("Installation failed:\n\n{e}"), false);
            false
        }
    }
}

/// Shows a message; with `ask`, Yes/No (returns true for Yes).
#[cfg(windows)]
fn message(text: &str, ask: bool) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let w = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (t, c) = (w(text), w("Plume"));
    let flags = if ask { MB_YESNO | MB_ICONQUESTION } else { MB_OK | MB_ICONWARNING };
    unsafe { MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), flags) == IDYES }
}

#[cfg(not(windows))]
fn message(text: &str, ask: bool) -> bool {
    eprintln!("{text}");
    ask
}

/// True if Plume is already running (it will reload the widgets by itself).
#[cfg(windows)]
fn plume_running() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW;
    let w = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (class, title) = (w("PlumeWidget"), w("PlumeTray"));
    !unsafe { FindWindowW(class.as_ptr(), title.as_ptr()) }.is_null()
}

#[cfg(not(windows))]
fn plume_running() -> bool {
    true
}

fn main() {
    let a = args();
    let dir = widgets_dir(a.dir);
    if a.preview {
        preview(&dir);
        return;
    }
    if let Some(file) = a.package {
        if !install_package(&file, &dir) {
            return;
        }
    }
    #[cfg(windows)]
    win::run(dir);
    #[cfg(not(windows))]
    eprintln!("Widgets only run on Windows. Use --preview to render PNGs.");
}
