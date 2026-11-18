//! Draws a widget into an RGBA Pixmap (platform independent).

use crate::config::{Align, ColorSpec, Element, Expr, Kind, Rgba};
use crate::metrics::Metrics;
use crate::web::Web;
use crate::widget::{Ctx, ElState, Widget};
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
    /// (font, bold, italic) → (font file, synthesize bold)
    styles: HashMap<(String, bool, bool), (String, bool)>,
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

    /// Main font for a style: the bold/italic file next to the regular one
    /// (segoeuib.ttf, arialbd.ttf, DejaVuSans-Bold.ttf...) when it exists.
    /// The flag says the bold must be synthesized.
    pub fn styled(&mut self, path: &str, bold: bool, italic: bool) -> Option<(String, bool)> {
        let main = self.main(path)?;
        if !bold && !italic {
            return Some((main, false));
        }
        let key = (main.clone(), bold, italic);
        if let Some(v) = self.styles.get(&key) {
            return Some(v.clone());
        }
        let p = std::path::Path::new(&main);
        let (stem, ext) = (
            p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            p.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "ttf".into()),
        );
        let suffixes: &[&str] = match (bold, italic) {
            (true, true) => &["z", "bi", "-BoldItalic", "-BoldOblique", "BoldItalic"],
            (true, false) => &["b", "bd", "-Bold", "Bold"],
            _ => &["i", "-Italic", "-Oblique", "Italic"],
        };
        let found = suffixes
            .iter()
            .map(|suf| p.with_file_name(format!("{stem}{suf}.{ext}")).to_string_lossy().into_owned())
            .find(|c| self.load(c).is_some());
        let v = match found {
            Some(f) => (f, false),
            None => (main, bold),
        };
        self.styles.insert(key, v.clone());
        Some(v)
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

/// Which part of a widget to draw. Fast-changing elements (the audio
/// visualizer) are drawn on top of a cached "base" image, so a 30 fps
/// spectrum doesn't redraw the whole widget every frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layer {
    All,
    /// Everything except live elements (clears the image first).
    Base,
    /// Only live elements, drawn over what is already there.
    Live,
}

pub fn is_live(el: &Element) -> bool {
    matches!(el.kind, Kind::Spectrum { .. })
}

/// Everything a draw call needs besides the element itself.
struct Painter<'a> {
    ctx: &'a Ctx<'a>,
    wd: &'a Widget,
    m: &'a Metrics,
    /// Logical → device pixels (monitor DPI × widget scale).
    s: f32,
    base: Transform,
    now: std::time::Instant,
}

/// Draws a widget at `scale` (the pixmap is `width*scale` × `height*scale`).
/// Returns true while an animation is running (more frames are needed).
#[allow(clippy::too_many_arguments)]
pub fn render(
    w: &mut Widget,
    pm: &mut Pixmap,
    m: &Metrics,
    web: &Web,
    fonts: &mut Fonts,
    images: &mut Images,
    scale: f32,
    layer: Layer,
) -> bool {
    let s = if scale > 0.0 { scale } else { 1.0 };
    if layer != Layer::Live {
        pm.fill(tiny_skia::Color::TRANSPARENT);
    }
    let mut states = std::mem::take(&mut w.states);
    let mut animating = false;
    {
        let ctx = w.ctx(m, web);
        let p = Painter { ctx: &ctx, wd: w, m, s, base: Transform::from_scale(s, s), now: std::time::Instant::now() };
        if layer != Layer::Live {
            if let Some(bg) = &w.cfg.background {
                let (ww, wh) = (w.cfg.width as f32, w.cfg.height as f32);
                let inset = if bg.border.is_some() { bg.border_width / 2.0 } else { 0.0 };
                if let Some(path) = rounded_rect(inset, inset, ww - 2.0 * inset, wh - 2.0 * inset, bg.radius) {
                    fill(pm, &path, bg.color.resolve(&ctx), p.base);
                    if let Some(bc) = &bg.border {
                        stroke(pm, &path, bc.resolve(&ctx), bg.border_width, LineCap::Butt, p.base);
                    }
                }
            }
        }
        for (i, el) in w.cfg.elements.iter().enumerate() {
            let live = is_live(el);
            if (layer == Layer::Base && live) || (layer == Layer::Live && !live) {
                continue;
            }
            let st = &mut states[i];
            st.bounds = None;
            if el.visible.as_ref().is_some_and(|v| !v.eval(&ctx).truthy()) {
                continue;
            }
            let hover = w.hovered == Some(i);
            let (b, anim) = p.element(el, hover, pm, fonts, images, st);
            st.bounds = b;
            animating |= anim;
        }
    }
    w.states = states;
    animating
}

/// Value shown by an animated bar/ring: slides from the old to the new value
/// over `ms` milliseconds (ease-out). Returns (value, still animating).
fn animated(st: &mut ElState, ms: u32, target: f32, now: std::time::Instant) -> (f32, bool) {
    if ms == 0 || st.anim_start.is_none() {
        // No animation, or first frame: show the value directly.
        st.anim_from = target;
        st.anim_to = target;
        st.anim_start = now.checked_sub(std::time::Duration::from_secs(60)).or(Some(now));
        return (target, false);
    }
    let current = |st: &ElState| -> (f32, bool) {
        let start = st.anim_start.unwrap_or(now);
        let t = now.saturating_duration_since(start).as_secs_f32() * 1000.0 / ms as f32;
        if t >= 1.0 {
            (st.anim_to, false)
        } else {
            let e = 1.0 - (1.0 - t).powi(3);
            (st.anim_from + (st.anim_to - st.anim_from) * e, true)
        }
    };
    if (target - st.anim_to).abs() > 1e-4 {
        st.anim_from = current(st).0;
        st.anim_to = target;
        st.anim_start = Some(now);
    }
    current(st)
}

impl Painter<'_> {
    fn rotation(&self, el: &Element, b: Rect) -> Transform {
        let deg = el.rotate.as_ref().map_or(0.0, |r| r.eval(self.ctx).num() as f32);
        if deg == 0.0 {
            return self.base;
        }
        let [cx, cy] = el.rotate_center.unwrap_or([b.x() + b.width() / 2.0, b.y() + b.height() / 2.0]);
        self.base.pre_concat(Transform::from_rotate_at(deg, cx, cy))
    }

    fn frac(&self, v: &Expr, min: &Expr, max: &Expr) -> f32 {
        let (v, lo, hi) = (v.eval(self.ctx).num(), min.eval(self.ctx).num(), max.eval(self.ctx).num());
        if hi > lo { ((v - lo) / (hi - lo)).clamp(0.0, 1.0) as f32 } else { 0.0 }
    }

    #[allow(clippy::too_many_arguments)]
    fn element(
        &self,
        el: &Element,
        hover: bool,
        pm: &mut Pixmap,
        fonts: &mut Fonts,
        images: &mut Images,
        st: &mut ElState,
    ) -> (Option<Rect>, bool) {
        let ctx = self.ctx;
        let s = self.s;
        let color = |c: &ColorSpec| -> Rgba {
            match (&el.hover_color, hover) {
                (Some(h), true) => h.resolve(ctx),
                _ => c.resolve(ctx),
            }
        };
        let mut animating = false;
        let bounds = (|| -> Option<Rect> {
            match &el.kind {
                Kind::Rect { x, y, w, h, color: c, radius, border, border_width } => {
                    let b = Rect::from_xywh(*x, *y, *w, *h)?;
                    let t = self.rotation(el, b);
                    let path = rounded_rect(*x, *y, *w, *h, *radius)?;
                    fill(pm, &path, color(c), t);
                    if let Some(bc) = border {
                        stroke(pm, &path, bc.resolve(ctx), *border_width, LineCap::Butt, t);
                    }
                    Some(b)
                }
                Kind::Bar { x, y, w, h, value, min, max, color: c, background, radius, vertical } => {
                    let b = Rect::from_xywh(*x, *y, *w, *h)?;
                    let t = self.rotation(el, b);
                    if let Some(path) = rounded_rect(*x, *y, *w, *h, *radius) {
                        fill(pm, &path, background.resolve(ctx), t);
                    }
                    let (f, anim) = animated(st, el.animate, self.frac(value, min, max), self.now);
                    animating = anim;
                    let path = if *vertical {
                        rounded_rect(*x, y + h * (1.0 - f), *w, h * f, *radius)
                    } else {
                        rounded_rect(*x, *y, w * f, *h, *radius)
                    };
                    if let Some(path) = path {
                        fill(pm, &path, color(c), t);
                    }
                    Some(b)
                }
                Kind::Graph { x, y, w, h, min, max, color: c, fill: fill_color, line_width, step, .. } => {
                    let b = Rect::from_xywh(*x, *y, *w, *h)?;
                    let t = self.rotation(el, b);
                    let (lo, hi) = (min.eval(ctx).num() as f32, max.eval(ctx).num() as f32);
                    let hist = &st.history;
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
                            if let Some(path) = pb.finish() {
                                fill(pm, &path, fc.resolve(ctx), t);
                            }
                        }
                        let mut pb = PathBuilder::new();
                        pb.move_to(pts[0].0, pts[0].1);
                        pts[1..].iter().for_each(|&(px, py)| pb.line_to(px, py));
                        if let Some(path) = pb.finish() {
                            stroke(pm, &path, color(c), *line_width, LineCap::Round, t);
                        }
                    }
                    Some(b)
                }
                Kind::Ring { x, y, radius, thickness, value, min, max, color: c, background, start, sweep, round_caps } => {
                    let r = radius + thickness / 2.0;
                    let b = Rect::from_xywh(x - r, y - r, 2.0 * r, 2.0 * r)?;
                    let t = self.rotation(el, b);
                    let cap = if *round_caps { LineCap::Round } else { LineCap::Butt };
                    if let Some(path) = arc(*x, *y, *radius, *start, *sweep) {
                        stroke(pm, &path, background.resolve(ctx), *thickness, cap, t);
                    }
                    let (f, anim) = animated(st, el.animate, self.frac(value, min, max), self.now);
                    animating = anim;
                    if f > 0.0 {
                        if let Some(path) = arc(*x, *y, *radius, *start, sweep * f) {
                            stroke(pm, &path, color(c), *thickness, cap, t);
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
                    let (key, src) = images.source(&p, self.wd, self.m)?;
                    let (sw, sh) = (src.width() as f32, src.height() as f32);
                    let (dw, dh) = match (iw, ih) {
                        (Some(a), Some(b)) => (*a, *b),
                        (Some(a), None) => (*a, a * sh / sw),
                        (None, Some(b)) => (b * sw / sh, *b),
                        (None, None) => (sw, sh),
                    };
                    let b = Rect::from_xywh(*x, *y, dw, dh)?;
                    let rotated = el.rotate.as_ref().is_some_and(|r| r.eval(ctx).num() != 0.0);
                    let t = self.rotation(el, b).pre_translate(*x, *y).pre_scale(1.0 / s, 1.0 / s);
                    let img = images.scaled(key, &src, (dw * s).round() as u32, (dh * s).round() as u32, radius * s)?;
                    let quality = if rotated { FilterQuality::Bicubic } else { FilterQuality::Nearest };
                    let paint = PixmapPaint { opacity: opacity.clamp(0.0, 1.0), quality, ..Default::default() };
                    pm.draw_pixmap(0, 0, img.as_ref().as_ref(), &paint, t, None);
                    Some(b)
                }
                Kind::Spectrum { x, y, w, h, bars, color: c, gap, radius } => {
                    let b = Rect::from_xywh(*x, *y, *w, *h)?;
                    let t = self.rotation(el, b);
                    let n = (*bars).clamp(1, 256);
                    let bands = self.m.audio.bands(n);
                    let bw = ((w - gap * (n - 1) as f32) / n as f32).max(1.0 / s);
                    let col = color(c);
                    for (i, v) in bands.iter().enumerate() {
                        let bh = (v * h).max(1.0 / s);
                        if let Some(path) = rounded_rect(x + i as f32 * (bw + gap), y + h - bh, bw, bh, radius.min(bh / 2.0)) {
                            fill(pm, &path, col, t);
                        }
                    }
                    Some(b)
                }
                Kind::Calendar { .. } => self.calendar(el, pm, fonts, color),
                Kind::Text { x, y, text, size, color: c, align, font, bold, italic, wrap, max_lines } => {
                    let raw = text.0.render(ctx);
                    let (f, fake_bold) = fonts.styled(font.as_deref().unwrap_or(&self.wd.cfg.font), *bold, *italic)?;
                    let px = size * s;
                    let (_, line_h) = fonts.line_metrics(&f, px);
                    let lines = layout(fonts, &f, &raw, px, wrap.map(|w| w * s), *max_lines);
                    let widths: Vec<f32> = lines.iter().map(|l| fonts.line_width(&f, l, px)).collect();
                    let tw = widths.iter().copied().fold(0.0, f32::max) / s;
                    let th = line_h * lines.len().max(1) as f32 / s;
                    let left = match align {
                        Align::Left => *x,
                        Align::Center => x - tw / 2.0,
                        Align::Right => x - tw,
                    };
                    let b = Rect::from_xywh(left, *y, tw.max(1.0), th.max(1.0))?;
                    let col = color(c);
                    let bold_px = if fake_bold { (px / 28.0).max(0.6) } else { 0.0 };
                    let rotated = el.rotate.as_ref().is_some_and(|r| r.eval(ctx).num() != 0.0);
                    if !rotated {
                        draw_text(pm, fonts, &f, &lines, x * s, y * s, px, col, *align, bold_px);
                    } else {
                        // Rotated text: draw into a small image, then draw that rotated.
                        let mut tmp = Pixmap::new((tw * s).ceil() as u32 + 4, (th * s).ceil() as u32 + 4)?;
                        let ax = 2.0 + match align {
                            Align::Left => 0.0,
                            Align::Center => tw * s / 2.0,
                            Align::Right => tw * s,
                        };
                        draw_text(&mut tmp, fonts, &f, &lines, ax, 2.0, px, col, *align, bold_px);
                        let t = self.rotation(el, b).pre_translate(left, *y).pre_scale(1.0 / s, 1.0 / s).pre_translate(-2.0, -2.0);
                        let paint = PixmapPaint { quality: FilterQuality::Bicubic, ..Default::default() };
                        pm.draw_pixmap(0, 0, tmp.as_ref(), &paint, t, None);
                    }
                    Some(b)
                }
            }
        })();
        (bounds, animating)
    }

    fn calendar(&self, el: &Element, pm: &mut Pixmap, fonts: &mut Fonts, color: impl Fn(&ColorSpec) -> Rgba) -> Option<Rect> {
        use chrono::{Datelike, Duration, NaiveDate};
        let Kind::Calendar { x, y, w, h, size, color: c, dim_color, today_color, today_text, header, monday_first, offset } =
            &el.kind
        else {
            return None;
        };
        let b = Rect::from_xywh(*x, *y, *w, *h)?;
        let (s, ctx) = (self.s, self.ctx);
        let loc = crate::locale::system();
        let today = chrono::Local::now().date_naive();
        let months = today.year() * 12 + today.month0() as i32 + offset.eval(ctx).num() as i32;
        let first = NaiveDate::from_ymd_opt(months.div_euclid(12), months.rem_euclid(12) as u32 + 1, 1)?;
        let f = fonts.styled(&self.wd.cfg.font, false, false)?.0;
        let px = size * s;
        let (_, line_h) = fonts.line_metrics(&f, px);
        let rows = 7 + *header as usize;
        let (cw, ch) = (w / 7.0, h / rows as f32);
        let (col, dim, tcol, ttext) = (color(c), dim_color.resolve(ctx), today_color.resolve(ctx), today_text.resolve(ctx));
        let mut text_at = |pm: &mut Pixmap, text: &str, cx: f32, row: usize, c: Rgba| {
            let ty = y + row as f32 * ch + (ch - line_h / s) / 2.0;
            draw_text(pm, fonts, &f, &[text.to_string()], cx * s, ty * s, px, c, Align::Center, 0.0);
        };
        let mut row = 0;
        if *header {
            let title = first.format_localized("%B %Y", loc).to_string();
            let mut title_chars = title.chars();
            let title: String = title_chars.next().map(|c| c.to_uppercase().chain(title_chars).collect()).unwrap_or_default();
            text_at(pm, &title, x + w / 2.0, 0, col);
            row = 1;
        }
        // Weekday names, from a known Sunday (2023-01-01).
        let sunday = NaiveDate::from_ymd_opt(2023, 1, 1)?;
        for i in 0..7 {
            let d = sunday + Duration::days(i + *monday_first as i64);
            let name: String = d.format_localized("%a", loc).to_string().chars().take(3).collect();
            text_at(pm, &name, x + (i as f32 + 0.5) * cw, row, dim);
        }
        let lead = if *monday_first { first.weekday().num_days_from_monday() } else { first.weekday().num_days_from_sunday() };
        let start = first - Duration::days(lead as i64);
        for k in 0..42 {
            let d = start + Duration::days(k);
            let (cx, r) = (x + ((k % 7) as f32 + 0.5) * cw, row + 1 + (k / 7) as usize);
            let in_month = d.month() == first.month();
            let mut c = if in_month { col } else { dim };
            if d == today {
                let rad = cw.min(ch) * 0.46;
                let cy = y + r as f32 * ch + ch / 2.0;
                if let Some(path) = PathBuilder::from_circle(cx, cy, rad) {
                    fill(pm, &path, tcol, self.base);
                }
                c = ttext;
            }
            text_at(pm, &d.day().to_string(), cx, r, c);
        }
        Some(b)
    }
}

/// Splits text into lines: explicit line breaks, then word wrapping at
/// `wrap` pixels, then at most `max_lines` (the last one ends with "…").
fn layout(fonts: &mut Fonts, f: &str, text: &str, px: f32, wrap: Option<f32>, max_lines: Option<usize>) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for para in text.split('\n') {
        let Some(width) = wrap.filter(|w| *w > 0.0) else {
            lines.push(para.to_string());
            continue;
        };
        let mut line = String::new();
        for word in para.split(' ') {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if fonts.line_width(f, &candidate, px) <= width || line.is_empty() && fonts.line_width(f, word, px) <= width {
                line = candidate;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            // A word longer than the line: break it by characters.
            let mut piece = String::new();
            for ch in word.chars() {
                piece.push(ch);
                if fonts.line_width(f, &piece, px) > width && piece.chars().count() > 1 {
                    piece.pop();
                    lines.push(std::mem::take(&mut piece));
                    piece.push(ch);
                }
            }
            line = piece;
        }
        lines.push(line);
    }
    if let Some(max) = max_lines.filter(|m| *m > 0) {
        if lines.len() > max {
            lines.truncate(max);
            let last = lines.last_mut().unwrap();
            let limit = wrap.unwrap_or(f32::MAX);
            last.push('…');
            while last.chars().count() > 1 && fonts.line_width(f, last, px) > limit {
                last.pop();
                last.pop();
                last.push('…');
            }
        }
    }
    lines
}

/// Draws lines of text. Coordinates and size are in device pixels.
#[allow(clippy::too_many_arguments)]
fn draw_text(pm: &mut Pixmap, fonts: &mut Fonts, f: &str, lines: &[String], x: f32, y: f32, px: f32, c: Rgba, align: Align, bold: f32) {
    let (ascent, line_h) = fonts.line_metrics(f, px);
    for (i, line) in lines.iter().enumerate() {
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
            if bold > 0.0 {
                // Synthetic bold: the same glyph again, slightly to the right.
                blit_coverage(pm, gx + bold.ceil() as i32, gy, gm.width, gm.height, cov, c);
            }
            pen += gm.advance_width + bold;
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
