#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Off Windows only `--preview` exists, so the interactive parts are unused there.
#![cfg_attr(not(windows), allow(dead_code))]

mod actions;
mod audio;
mod config;
mod expr;
mod log;
mod media;
mod metrics;
mod probe;
mod render;
mod script;
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

/// Widgets folder:
/// 1. the first argument, if given;
/// 2. `widgets\` next to the executable, if it exists (portable copy, zip);
/// 3. `%APPDATA%\Plume\widgets` (installed copy; survives updates).
fn widgets_dir() -> PathBuf {
    if let Some(arg) = std::env::args().skip(1).find(|a| !a.starts_with("--")) {
        return PathBuf::from(arg);
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
        let Some(mut pm) = tiny_skia::Pixmap::new(w.cfg.width.max(1), w.cfg.height.max(1)) else { continue };
        render::render(&mut w, &mut pm, &m, &web, &mut fonts, &mut images);
        let out = format!("preview-{}.png", w.cfg.name);
        pm.save_png(&out).expect("failed to save png");
        println!("{out}");
    }
}

fn main() {
    let dir = widgets_dir();
    if std::env::args().any(|a| a == "--preview") {
        preview(&dir);
        return;
    }
    #[cfg(windows)]
    win::run(dir);
    #[cfg(not(windows))]
    eprintln!("Widgets only run on Windows. Use --preview to render PNGs.");
}
