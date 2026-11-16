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

/// Pasta dos widgets: 1º argumento, ou `widgets/` ao lado do executável.
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
        // Primeira execução: grava os exemplos.
        let _ = std::fs::create_dir_all(&dir);
        for (name, src) in config::EXAMPLES {
            let _ = std::fs::write(dir.join(name), src);
        }
    }
    dir
}

/// `rustmeter --preview [pasta]` renderiza cada widget em PNG (funciona em qualquer SO).
fn preview(dir: &std::path::Path) {
    let (widgets, errors) = config::load_dir(dir);
    for e in errors {
        eprintln!("erro: {e}");
    }
    let mut m = metrics::Metrics::new();
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(950));
        m.refresh();
    }
    let mut fonts = render::Fonts::default();
    for w in widgets {
        let mut pm = tiny_skia::Pixmap::new(w.width, w.height).expect("tamanho inválido");
        render::render(&w, &mut pm, &m, &mut fonts);
        let out = format!("preview-{}.png", w.name);
        pm.save_png(&out).expect("falha ao salvar png");
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
    eprintln!("Os widgets só rodam no Windows. Use --preview para gerar PNGs.");
}
