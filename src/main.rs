#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg_attr(not(windows), allow(dead_code))]
mod config;
mod metrics;
mod render;
#[cfg(windows)]
mod tray;
#[cfg(windows)]
mod win;

use std::path::PathBuf;

/// Widgets folder: first argument, or `widgets/` next to the executable.
fn widgets_dir() -> PathBuf {
    if let Some(arg) = std::env::args().skip(1).find(|a| !a.starts_with("--")) {
        return PathBuf::from(arg);
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = exe_dir.join("widgets");
    if !dir.exists() {
        // First run: write the examples.
        let _ = std::fs::create_dir_all(&dir);
        for (name, src) in config::EXAMPLES {
            let _ = std::fs::write(dir.join(name), src);
        }
    }
    dir
}

/// `plume --preview [folder]` renders each widget to a PNG (works on any OS).
fn preview(dir: &std::path::Path) {
    let (widgets, errors) = config::load_dir(dir);
    for e in errors {
        eprintln!("error: {e}");
    }
    let mut m = metrics::Metrics::new();
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(950));
        m.refresh();
    }
    let mut fonts = render::Fonts::default();
    for w in widgets {
        let mut pm = tiny_skia::Pixmap::new(w.width, w.height).expect("invalid size");
        render::render(&w, &mut pm, &m, &mut fonts);
        let out = format!("preview-{}.png", w.name);
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
