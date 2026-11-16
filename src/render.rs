//! Draws a widget into an RGBA Pixmap (platform independent).

use crate::config::{Align, Element, Rgba, WidgetConfig};
use crate::metrics::Metrics;
use fontdue::{Font, FontSettings, Metrics as GlyphMetrics};
use std::collections::HashMap;
use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform};

/// Loaded font + cache of already rasterized glyphs.
pub struct FontEntry {
    font: Font,
    glyphs: HashMap<(char, u32), (GlyphMetrics, Vec<u8>)>,
}

#[derive(Default)]
pub struct Fonts {
    map: HashMap<String, Option<FontEntry>>,
}

impl Fonts {
    pub fn get(&mut self, path: &str) -> Option<&mut FontEntry> {
        self.map
            .entry(path.to_string())
            .or_insert_with(|| {
                const FALLBACKS: &[&str] = &[
                    "C:/Windows/Fonts/segoeui.ttf",
                    "C:/Windows/Fonts/arial.ttf",
                    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                ];
                let bytes = std::iter::once(path)
                    .chain(FALLBACKS.iter().copied())
                    .find_map(|p| std::fs::read(p).ok())?;
                let font = Font::from_bytes(bytes, FontSettings::default()).ok()?;
                Some(FontEntry { font, glyphs: HashMap::new() })
            })
            .as_mut()
    }
}

impl FontEntry {
    fn glyph(&mut self, c: char, px: f32) -> &(GlyphMetrics, Vec<u8>) {
        let font = &self.font;
        self.glyphs.entry((c, px.to_bits())).or_insert_with(|| font.rasterize(c, px))
    }

    fn line_width(&mut self, line: &str, px: f32) -> f32 {
        line.chars().map(|c| self.glyph(c, px).0.advance_width).sum()
    }
}

pub fn render(cfg: &WidgetConfig, pm: &mut Pixmap, m: &Metrics, fonts: &mut Fonts) {
    pm.fill(tiny_skia::Color::TRANSPARENT);
    let (w, h) = (cfg.width as f32, cfg.height as f32);

    if let Some(bg) = &cfg.background {
        let inset = if bg.border.is_some() { bg.border_width / 2.0 } else { 0.0 };
        if let Some(p) = rounded_rect(inset, inset, w - 2.0 * inset, h - 2.0 * inset, bg.radius) {
            fill(pm, &p, bg.color);
            if let Some(bc) = bg.border {
                stroke(pm, &p, bc, bg.border_width);
            }
        }
    }

    for el in &cfg.elements {
        match el {
            Element::Rect { x, y, w, h, color, radius } => {
                if let Some(p) = rounded_rect(*x, *y, *w, *h, *radius) {
                    fill(pm, &p, *color);
                }
            }
            Element::Bar { x, y, w, h, source, color, background, radius } => {
                if let Some(p) = rounded_rect(*x, *y, *w, *h, *radius) {
                    fill(pm, &p, *background);
                }
                let frac = (m.value(*source) / 100.0).clamp(0.0, 1.0);
                if frac > 0.0 {
                    if let Some(p) = rounded_rect(*x, *y, w * frac, *h, *radius) {
                        fill(pm, &p, *color);
                    }
                }
            }
            Element::Graph { x, y, w, h, source, color, fill: fill_color, line_width } => {
                draw_graph(pm, m, *source, (*x, *y, *w, *h), *color, *fill_color, *line_width);
            }
            Element::Text { x, y, text, size, color, align, font } => {
                let path = font.as_deref().unwrap_or(&cfg.font);
                if let Some(f) = fonts.get(path) {
                    draw_text(pm, f, &m.expand(text), *x, *y, *size, *color, *align);
                }
            }
        }
    }
}

fn draw_graph(
    pm: &mut Pixmap,
    m: &Metrics,
    src: crate::config::Source,
    (x, y, w, h): (f32, f32, f32, f32),
    color: Rgba,
    fill_color: Option<Rgba>,
    lw: f32,
) {
    let hist = m.history(src);
    if hist.len() < 2 {
        return;
    }
    const STEP: f32 = 3.0; // pixels between samples
    let n = ((w / STEP) as usize + 1).min(hist.len());
    let pts: Vec<(f32, f32)> = hist
        .iter()
        .skip(hist.len() - n)
        .enumerate()
        .map(|(i, v)| {
            let px = x + w - (n - 1 - i) as f32 * STEP;
            let py = y + h - (v / 100.0).clamp(0.0, 1.0) * h;
            (px, py)
        })
        .collect();

    if let Some(fc) = fill_color {
        let mut pb = PathBuilder::new();
        pb.move_to(pts[0].0, y + h);
        for &(px, py) in &pts {
            pb.line_to(px, py);
        }
        pb.line_to(pts[pts.len() - 1].0, y + h);
        pb.close();
        if let Some(p) = pb.finish() {
            fill(pm, &p, fc);
        }
    }
    let mut pb = PathBuilder::new();
    pb.move_to(pts[0].0, pts[0].1);
    for &(px, py) in &pts[1..] {
        pb.line_to(px, py);
    }
    if let Some(p) = pb.finish() {
        stroke(pm, &p, color, lw);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_text(pm: &mut Pixmap, f: &mut FontEntry, text: &str, x: f32, y: f32, px: f32, c: Rgba, align: Align) {
    let lm = f.font.horizontal_line_metrics(px);
    let ascent = lm.map_or(px, |l| l.ascent);
    let line_h = lm.map_or(px * 1.2, |l| l.new_line_size);

    for (i, line) in text.lines().enumerate() {
        let lw = f.line_width(line, px);
        let mut pen = match align {
            Align::Left => x,
            Align::Center => x - lw / 2.0,
            Align::Right => x - lw,
        };
        let baseline = y + ascent + i as f32 * line_h;
        for ch in line.chars() {
            let (gm, cov) = f.glyph(ch, px);
            let gx = (pen + gm.xmin as f32).round() as i32;
            let gy = (baseline - gm.ymin as f32 - gm.height as f32).round() as i32;
            blit_coverage(pm, gx, gy, gm.width, gm.height, cov, c);
            pen += gm.advance_width;
        }
    }
}

/// Blends a coverage bitmap (glyph) into the pixmap, with premultiplied alpha.
fn blit_coverage(pm: &mut Pixmap, gx: i32, gy: i32, w: usize, h: usize, cov: &[u8], c: Rgba) {
    let (pw, ph) = (pm.width() as i32, pm.height() as i32);
    let data = pm.data_mut();
    for row in 0..h {
        let py = gy + row as i32;
        if !(0..ph).contains(&py) {
            continue;
        }
        for col in 0..w {
            let px = gx + col as i32;
            if !(0..pw).contains(&px) {
                continue;
            }
            let a = cov[row * w + col] as u32 * c.a as u32 / 255;
            if a == 0 {
                continue;
            }
            let i = ((py * pw + px) * 4) as usize;
            let inv = 255 - a;
            data[i] = ((c.r as u32 * a + data[i] as u32 * inv) / 255) as u8;
            data[i + 1] = ((c.g as u32 * a + data[i + 1] as u32 * inv) / 255) as u8;
            data[i + 2] = ((c.b as u32 * a + data[i + 2] as u32 * inv) / 255) as u8;
            data[i + 3] = (a + data[i + 3] as u32 * inv / 255) as u8;
        }
    }
}

fn paint(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c.to_skia());
    p.anti_alias = true;
    p
}

fn fill(pm: &mut Pixmap, path: &Path, c: Rgba) {
    pm.fill_path(path, &paint(c), FillRule::Winding, Transform::identity(), None);
}

fn stroke(pm: &mut Pixmap, path: &Path, c: Rgba, width: f32) {
    let s = Stroke { width, ..Stroke::default() };
    pm.stroke_path(path, &paint(c), &s, Transform::identity(), None);
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    if r == 0.0 {
        return Some(PathBuilder::from_rect(Rect::from_xywh(x, y, w, h)?));
    }
    let k = 0.552_284_8 * r; // Bezier approximation of an arc
    let (r2, b) = (x + w, y + h);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(r2 - r, y);
    pb.cubic_to(r2 - r + k, y, r2, y + r - k, r2, y + r);
    pb.line_to(r2, b - r);
    pb.cubic_to(r2, b - r + k, r2 - r + k, b, r2 - r, b);
    pb.line_to(x + r, b);
    pb.cubic_to(x + r - k, b, x, b - r + k, x, b - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}
