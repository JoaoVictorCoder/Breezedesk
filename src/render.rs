//! Draws a widget into an RGBA Pixmap (platform independent).

use crate::config::{Align, ColorSpec, Element, Kind, Rgba};
use crate::metrics::Metrics;
use crate::web::Web;
use crate::widget::{Ctx, Widget};
use fontdue::{Font, FontSettings, Metrics as GlyphMetrics};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tiny_skia::{
    FillRule, FilterQuality, LineCap, Mask, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform,
};

// ------------------------------------------------------------------ fonts

/// Fonts tried, in order, for characters the chosen font does not have
/// (symbols, emoji...), like Windows itself does.
const FALLBACKS: &[&str] = &[
    "C:/Windows/Fonts/segoeui.ttf",
    "C:/Windows/Fonts/seguisym.ttf",
    "C:/Windows/Fonts/seguiemj.ttf",
    "C:/Windows/Fonts/arial.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
    "/usr/share/fonts/truetype/freefont/FreeMono.ttf",
];

/// Loaded font + cache of already rasterized glyphs.
pub struct FontEntry {
    font: Font,
    glyphs: HashMap<(char, u32), (GlyphMetrics, Vec<u8>)>,
}

#[derive(Default)]
pub struct Fonts {
    map: HashMap<String, Option<FontEntry>>,
    /// (font, char) → font that actually has the char.
    pick: HashMap<(String, char), Option<String>>,
}

impl Fonts {
    fn load(&mut self, path: &str) -> Option<&mut FontEntry> {
        self.map
            .entry(path.to_string())
            .or_insert_with(|| {
                let bytes = std::fs::read(path).ok()?;
                let font = Font::from_bytes(bytes, FontSettings::default()).ok()?;
                Some(FontEntry { font, glyphs: HashMap::new() })
            })
            .as_mut()
    }

    /// The font to use as "main" font: the requested one, or the first fallback that exists.
    fn main(&mut self, path: &str) -> Option<String> {
        std::iter::once(path)
            .chain(FALLBACKS.iter().copied())
            .find(|p| self.load(p).is_some())
            .map(str::to_string)
    }

    fn font_for(&mut self, main: &str, ch: char) -> Option<String> {
        if let Some(p) = self.pick.get(&(main.to_string(), ch)) {
            return p.clone();
        }
        let has = |fonts: &mut Fonts, p: &str| fonts.load(p).is_some_and(|f| ch.is_whitespace() || f.font.lookup_glyph_index(ch) != 0);
        let found = std::iter::once(main)
            .chain(FALLBACKS.iter().copied())
            .find(|p| has(self, p))
            .map(str::to_string)
            .or_else(|| Some(main.to_string()));
        self.pick.insert((main.to_string(), ch), found.clone());
        found
    }

    fn glyph(&mut self, main: &str, ch: char, px: f32) -> Option<&(GlyphMetrics, Vec<u8>)> {
        let path = self.font_for(main, ch)?;
        let e = self.load(&path)?;
        let font = &e.font;
        Some(e.glyphs.entry((ch, px.to_bits())).or_insert_with(|| font.rasterize(ch, px)))
    }

    fn line_width(&mut self, main: &str, line: &str, px: f32) -> f32 {
        line.chars().map(|c| self.glyph(main, c, px).map_or(0.0, |g| g.0.advance_width)).sum()
    }

    /// (ascent, line height) of the main font.
    fn line_metrics(&mut self, main: &str, px: f32) -> (f32, f32) {
        let lm = self.load(main).and_then(|f| f.font.horizontal_line_metrics(px));
        (lm.map_or(px, |l| l.ascent), lm.map_or(px * 1.2, |l| l.new_line_size))
    }
}

// ----------------------------------------------------------------- images

/// Decoded images, plus copies already scaled (and rounded) to their final size.
#[derive(Default)]
pub struct Images {
    files: HashMap<PathBuf, Option<Arc<Pixmap>>>,
    scaled: HashMap<(String, u32, u32, u32), Arc<Pixmap>>,
}

impl Images {
    pub fn clear(&mut self) {
        self.files.clear();
        self.scaled.clear();
    }

    /// (cache key, image) for a path or `media:cover`.
    fn source(&mut self, path: &str, w: &Widget, m: &Metrics) -> Option<(String, Arc<Pixmap>)> {
        if path == "media:cover" {
            let info = m.media.info();
            return Some((format!("media:cover#{}", info.cover_id), info.cover?));
        }
        let full = w.cfg.resolve(path);
        let img = self
            .files
            .entry(full.clone())
            .or_insert_with(|| {
                let img = std::fs::read(&full).ok().and_then(|b| crate::media::decode_image(&b)).map(Arc::new);
                if img.is_none() {
                    crate::log::log(format!("[{}] cannot load image {}", w.cfg.name, full.display()));
                }
                img
            })
            .clone()?;
        Some((full.to_string_lossy().into_owned(), img))
    }

    fn scaled(&mut self, key: String, src: &Pixmap, w: u32, h: u32, radius: f32) -> Option<Arc<Pixmap>> {
        if self.scaled.len() > 64 {
            self.scaled.clear();
        }
        let k = (key, w, h, radius.to_bits());
        if let Some(p) = self.scaled.get(&k) {
            return Some(p.clone());
        }
        let mut out = Pixmap::new(w.max(1), h.max(1))?;
        let paint = PixmapPaint { quality: FilterQuality::Bicubic, ..Default::default() };
        let t = Transform::from_scale(w as f32 / src.width() as f32, h as f32 / src.height() as f32);
        out.draw_pixmap(0, 0, src.as_ref(), &paint, t, None);
        if radius > 0.0 {
            if let (Some(mut mask), Some(path)) = (Mask::new(w, h), rounded_rect(0.0, 0.0, w as f32, h as f32, radius)) {
                mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
                out.apply_mask(&mask);
            }
        }
        let out = Arc::new(out);
        self.scaled.insert(k, out.clone());
        Some(out)
    }
}

// ----------------------------------------------------------------- render

pub fn render(w: &mut Widget, pm: &mut Pixmap, m: &Metrics, web: &Web, fonts: &mut Fonts, images: &mut Images) {
    pm.fill(tiny_skia::Color::TRANSPARENT);
    let (ww, wh) = (w.cfg.width as f32, w.cfg.height as f32);
    let mut bounds = vec![None; w.cfg.elements.len()];
    {
        let ctx = w.ctx(m, web);
        if let Some(bg) = &w.cfg.background {
            let inset = if bg.border.is_some() { bg.border_width / 2.0 } else { 0.0 };
            if let Some(p) = rounded_rect(inset, inset, ww - 2.0 * inset, wh - 2.0 * inset, bg.radius) {
                fill(pm, &p, bg.color.resolve(&ctx), Transform::identity());
                if let Some(bc) = &bg.border {
                    stroke(pm, &p, bc.resolve(&ctx), bg.border_width, LineCap::Butt, Transform::identity());
                }
            }
        }
        for (i, el) in w.cfg.elements.iter().enumerate() {
            if el.visible.as_ref().is_some_and(|v| !v.eval(&ctx).truthy()) {
                continue;
            }
            let hover = w.hovered == Some(i);
            bounds[i] = draw_element(el, hover, pm, &ctx, w, m, fonts, images, i);
        }
    }
    for (st, b) in w.states.iter_mut().zip(bounds) {
        st.bounds = b;
    }
}

fn rotation(el: &Element, ctx: &Ctx, b: Rect) -> Transform {
    let deg = el.rotate.as_ref().map_or(0.0, |r| r.eval(ctx).num() as f32);
    if deg == 0.0 {
        return Transform::identity();
    }
    let [cx, cy] = el.rotate_center.unwrap_or([b.x() + b.width() / 2.0, b.y() + b.height() / 2.0]);
    Transform::from_rotate_at(deg, cx, cy)
}

#[allow(clippy::too_many_arguments)]
fn draw_element(
    el: &Element,
    hover: bool,
    pm: &mut Pixmap,
    ctx: &Ctx,
    wd: &Widget,
    m: &Metrics,
    fonts: &mut Fonts,
    images: &mut Images,
    idx: usize,
) -> Option<Rect> {
    let color = |c: &ColorSpec| -> Rgba {
        match (&el.hover_color, hover) {
            (Some(h), true) => h.resolve(ctx),
            _ => c.resolve(ctx),
        }
    };
    let frac = |v: &crate::config::Expr, min: &crate::config::Expr, max: &crate::config::Expr| {
        let (v, lo, hi) = (v.eval(ctx).num(), min.eval(ctx).num(), max.eval(ctx).num());
        if hi > lo { ((v - lo) / (hi - lo)).clamp(0.0, 1.0) as f32 } else { 0.0 }
    };

    match &el.kind {
        Kind::Rect { x, y, w, h, color: c, radius, border, border_width } => {
            let b = Rect::from_xywh(*x, *y, *w, *h)?;
            let t = rotation(el, ctx, b);
            let p = rounded_rect(*x, *y, *w, *h, *radius)?;
            fill(pm, &p, color(c), t);
            if let Some(bc) = border {
                stroke(pm, &p, bc.resolve(ctx), *border_width, LineCap::Butt, t);
            }
            Some(b)
        }
        Kind::Bar { x, y, w, h, value, min, max, color: c, background, radius, vertical } => {
            let b = Rect::from_xywh(*x, *y, *w, *h)?;
            let t = rotation(el, ctx, b);
            if let Some(p) = rounded_rect(*x, *y, *w, *h, *radius) {
                fill(pm, &p, background.resolve(ctx), t);
            }
            let f = frac(value, min, max);
            let p = if *vertical {
                rounded_rect(*x, y + h * (1.0 - f), *w, h * f, *radius)
            } else {
                rounded_rect(*x, *y, w * f, *h, *radius)
            };
            if let Some(p) = p {
                fill(pm, &p, color(c), t);
            }
            Some(b)
        }
        Kind::Graph { x, y, w, h, min, max, color: c, fill: fill_color, line_width, step, .. } => {
            let b = Rect::from_xywh(*x, *y, *w, *h)?;
            let t = rotation(el, ctx, b);
            let (lo, hi) = (min.eval(ctx).num() as f32, max.eval(ctx).num() as f32);
            let hist = &wd.states[idx].history;
            if hist.len() >= 2 && hi > lo {
                let n = hist.len();
                let pts: Vec<(f32, f32)> = hist
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let px = x + w - (n - 1 - i) as f32 * step;
                        (px.max(*x), y + h - ((v - lo) / (hi - lo)).clamp(0.0, 1.0) * h)
                    })
                    .collect();
                if let Some(fc) = fill_color {
                    let mut pb = PathBuilder::new();
                    pb.move_to(pts[0].0, y + h);
                    pts.iter().for_each(|&(px, py)| pb.line_to(px, py));
                    pb.line_to(pts[n - 1].0, y + h);
                    pb.close();
                    if let Some(p) = pb.finish() {
                        fill(pm, &p, fc.resolve(ctx), t);
                    }
                }
                let mut pb = PathBuilder::new();
                pb.move_to(pts[0].0, pts[0].1);
                pts[1..].iter().for_each(|&(px, py)| pb.line_to(px, py));
                if let Some(p) = pb.finish() {
                    stroke(pm, &p, color(c), *line_width, LineCap::Round, t);
                }
            }
            Some(b)
        }
        Kind::Ring { x, y, radius, thickness, value, min, max, color: c, background, start, sweep, round_caps } => {
            let r = radius + thickness / 2.0;
            let b = Rect::from_xywh(x - r, y - r, 2.0 * r, 2.0 * r)?;
            let t = rotation(el, ctx, b);
            let cap = if *round_caps { LineCap::Round } else { LineCap::Butt };
            if let Some(p) = arc(*x, *y, *radius, *start, *sweep) {
                stroke(pm, &p, background.resolve(ctx), *thickness, cap, t);
            }
            let f = frac(value, min, max);
            if f > 0.0 {
                if let Some(p) = arc(*x, *y, *radius, *start, sweep * f) {
                    stroke(pm, &p, color(c), *thickness, cap, t);
                }
            }
            Some(b)
        }
        Kind::Image { x, y, w: iw, h: ih, path, hover_path, radius, opacity } => {
            let tpl = match (hover, hover_path) {
                (true, Some(hp)) => hp,
                _ => path,
            };
            let p = tpl.0.render(ctx);
            if p.is_empty() {
                return None;
            }
            let (key, src) = images.source(&p, wd, m)?;
            let (sw, sh) = (src.width() as f32, src.height() as f32);
            let (dw, dh) = match (iw, ih) {
                (Some(a), Some(b)) => (*a, *b),
                (Some(a), None) => (*a, a * sh / sw),
                (None, Some(b)) => (b * sw / sh, *b),
                (None, None) => (sw, sh),
            };
            let b = Rect::from_xywh(*x, *y, dw, dh)?;
            let t = rotation(el, ctx, b);
            let img = images.scaled(key, &src, dw.round() as u32, dh.round() as u32, *radius)?;
            let quality = if t.is_identity() { FilterQuality::Nearest } else { FilterQuality::Bicubic };
            let paint = PixmapPaint { opacity: opacity.clamp(0.0, 1.0), quality, ..Default::default() };
            pm.draw_pixmap(0, 0, img.as_ref().as_ref(), &paint, t.pre_translate(x.round(), y.round()), None);
            Some(b)
        }
        Kind::Spectrum { x, y, w, h, bars, color: c, gap, radius } => {
            let b = Rect::from_xywh(*x, *y, *w, *h)?;
            let t = rotation(el, ctx, b);
            let n = (*bars).clamp(1, 256);
            let bands = m.audio.bands(n);
            let bw = ((w - gap * (n - 1) as f32) / n as f32).max(1.0);
            let col = color(c);
            for (i, v) in bands.iter().enumerate() {
                let bh = (v * h).max(1.0);
                if let Some(p) = rounded_rect(x + i as f32 * (bw + gap), y + h - bh, bw, bh, radius.min(bh / 2.0)) {
                    fill(pm, &p, col, t);
                }
            }
            Some(b)
        }
        Kind::Text { x, y, text, size, color: c, align, font } => {
            let s = text.0.render(ctx);
            let f = fonts.main(font.as_deref().unwrap_or(&wd.cfg.font))?;
            let (_, line_h) = fonts.line_metrics(&f, *size);
            let widths: Vec<f32> = s.lines().map(|l| fonts.line_width(&f, l, *size)).collect();
            let tw = widths.iter().copied().fold(0.0, f32::max);
            let th = line_h * s.lines().count().max(1) as f32;
            let left = match align {
                Align::Left => *x,
                Align::Center => x - tw / 2.0,
                Align::Right => x - tw,
            };
            let b = Rect::from_xywh(left, *y, tw.max(1.0), th)?;
            let t = rotation(el, ctx, b);
            let col = color(c);
            if t.is_identity() {
                draw_text(pm, fonts, &f, &s, *x, *y, *size, col, *align);
            } else {
                // Rotated text: draw into a small image, then draw that rotated.
                let mut tmp = Pixmap::new(tw.ceil() as u32 + 4, th.ceil() as u32 + 4)?;
                let ax = match align {
                    Align::Left => 2.0,
                    Align::Center => 2.0 + tw / 2.0,
                    Align::Right => 2.0 + tw,
                };
                draw_text(&mut tmp, fonts, &f, &s, ax, 2.0, *size, col, *align);
                let paint = PixmapPaint { quality: FilterQuality::Bicubic, ..Default::default() };
                pm.draw_pixmap(0, 0, tmp.as_ref(), &paint, t.pre_translate(left - 2.0, y - 2.0), None);
            }
            Some(b)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_text(pm: &mut Pixmap, fonts: &mut Fonts, f: &str, text: &str, x: f32, y: f32, px: f32, c: Rgba, align: Align) {
    let (ascent, line_h) = fonts.line_metrics(f, px);
    for (i, line) in text.lines().enumerate() {
        let lw = fonts.line_width(f, line, px);
        let mut pen = match align {
            Align::Left => x,
            Align::Center => x - lw / 2.0,
            Align::Right => x - lw,
        };
        let baseline = y + ascent + i as f32 * line_h;
        for ch in line.chars() {
            let Some((gm, cov)) = fonts.glyph(f, ch, px) else { continue };
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

fn fill(pm: &mut Pixmap, path: &Path, c: Rgba, t: Transform) {
    pm.fill_path(path, &paint(c), FillRule::Winding, t, None);
}

fn stroke(pm: &mut Pixmap, path: &Path, c: Rgba, width: f32, cap: LineCap, t: Transform) {
    let s = Stroke { width, line_cap: cap, ..Stroke::default() };
    pm.stroke_path(path, &paint(c), &s, t, None);
}

/// Circular arc (degrees, 0 = right, clockwise), built from Bezier segments.
fn arc(cx: f32, cy: f32, r: f32, start: f32, sweep: f32) -> Option<Path> {
    if sweep.abs() < 0.01 || r <= 0.0 {
        return None;
    }
    let segs = (sweep.abs() / 90.0).ceil().max(1.0) as usize;
    let step = sweep.to_radians() / segs as f32;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let mut a = start.to_radians();
    let mut pb = PathBuilder::new();
    pb.move_to(cx + r * a.cos(), cy + r * a.sin());
    for _ in 0..segs {
        let (s0, c0) = a.sin_cos();
        let (s1, c1) = (a + step).sin_cos();
        pb.cubic_to(
            cx + r * (c0 - k * s0),
            cy + r * (s0 + k * c0),
            cx + r * (c1 + k * s1),
            cy + r * (s1 - k * c1),
            cx + r * c1,
            cy + r * s1,
        );
        a += step;
    }
    pb.finish()
}

pub fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
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
