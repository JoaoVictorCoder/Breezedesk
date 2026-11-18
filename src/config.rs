//! Widget file format (TOML).

use crate::expr::{self, Ast, Template};
use serde::{Deserialize, Deserializer};
use std::path::{Path, PathBuf};

/// RGBA color parsed from "#RRGGBB" or "#RRGGBBAA" strings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const WHITE: Rgba = Rgba { r: 255, g: 255, b: 255, a: 255 };

    pub fn parse(s: &str) -> Option<Rgba> {
        let h = s.trim().strip_prefix('#')?;
        let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        match h.len() {
            6 => Some(Rgba { r: byte(0)?, g: byte(2)?, b: byte(4)?, a: 255 }),
            8 => Some(Rgba { r: byte(0)?, g: byte(2)?, b: byte(4)?, a: byte(6)? }),
            _ => None,
        }
    }

    pub fn to_skia(self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba8(self.r, self.g, self.b, self.a)
    }
}

/// A color that is either fixed (`"#FF8800"`) or computed each frame
/// (`"cpu > 80 ? '#FF3B30' : accent"` or `"{accent}"`).
#[derive(Clone, Debug)]
pub enum ColorSpec {
    Static(Rgba),
    Dynamic(Template),
}

impl ColorSpec {
    pub fn fixed(c: Rgba) -> ColorSpec {
        ColorSpec::Static(c)
    }
    pub fn resolve(&self, env: &dyn expr::Env) -> Rgba {
        match self {
            ColorSpec::Static(c) => *c,
            ColorSpec::Dynamic(t) => Rgba::parse(&t.render(env)).unwrap_or(Rgba::WHITE),
        }
    }
    pub fn names(&self, f: &mut dyn FnMut(&str)) {
        if let ColorSpec::Dynamic(t) = self {
            t.names(f);
        }
    }
}

impl<'de> Deserialize<'de> for ColorSpec {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        if let Some(c) = Rgba::parse(&s) {
            return Ok(ColorSpec::Static(c));
        }
        let src = if s.contains('{') { s.clone() } else { format!("{{{s}}}") };
        Template::parse(&src)
            .map(ColorSpec::Dynamic)
            .map_err(|e| serde::de::Error::custom(format!("invalid color {s:?}: {e}")))
    }
}

/// An expression given as a number, a boolean or a string (`"cpu * 2"`).
#[derive(Clone, Debug)]
pub struct Expr(pub Ast);

impl Expr {
    pub fn num(v: f64) -> Expr {
        Expr(Ast::Lit(expr::Value::int(v)))
    }
    pub fn eval(&self, env: &dyn expr::Env) -> expr::Value {
        self.0.eval(env)
    }
}

impl<'de> Deserialize<'de> for Expr {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            N(f64),
            B(bool),
            S(String),
        }
        match Raw::deserialize(d)? {
            Raw::N(v) => Ok(Expr(Ast::Lit(expr::Value::Num(v, 0)))),
            Raw::B(b) => Ok(Expr(Ast::Lit(expr::Value::bool(b)))),
            Raw::S(s) => expr::parse(&s).map(Expr).map_err(serde::de::Error::custom),
        }
    }
}

/// Text with `{expression}` parts.
#[derive(Clone, Debug, Default)]
pub struct Text(pub Template);

impl<'de> Deserialize<'de> for Text {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Template::parse(&s).map(Text).map_err(serde::de::Error::custom)
    }
}

/// One action or a list of actions (each one is a template).
#[derive(Clone, Debug, Default)]
pub struct Actions(pub Vec<Template>);

impl Actions {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'de> Deserialize<'de> for Actions {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            One(String),
            Many(Vec<String>),
        }
        let list = match Raw::deserialize(d)? {
            Raw::One(s) => vec![s],
            Raw::Many(v) => v,
        };
        list.iter()
            .map(|s| Template::parse(s))
            .collect::<Result<_, _>>()
            .map(Actions)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ZOrder {
    /// Stays "glued" to the desktop, behind other windows.
    #[default]
    Bottom,
    Normal,
    Top,
}

#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
#[allow(clippy::enum_variant_names)]
pub enum Blur {
    #[default]
    None,
    /// Classic blur behind the widget.
    Blur,
    /// Windows 10/11 acrylic (blur + noise), tinted with the background color.
    Acrylic,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Background {
    #[serde(default = "default_bg")]
    pub color: ColorSpec,
    #[serde(default)]
    pub radius: f32,
    pub border: Option<ColorSpec>,
    #[serde(default = "one")]
    pub border_width: f32,
    #[serde(default)]
    pub blur: Blur,
}

#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum WebFormat {
    #[default]
    Json,
    Rss,
    Text,
}

/// Data downloaded periodically: `[[web]]`.
#[derive(Deserialize, Clone, Debug)]
pub struct WebConfig {
    pub name: String,
    pub url: Text,
    /// Seconds between downloads.
    #[serde(default = "default_interval")]
    pub interval: u64,
    #[serde(default)]
    pub format: WebFormat,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Kind {
    Text {
        x: f32,
        y: f32,
        text: Text,
        #[serde(default = "default_size")]
        size: f32,
        #[serde(default = "white")]
        color: ColorSpec,
        #[serde(default)]
        align: Align,
        /// Font for this text only (otherwise the widget's font is used).
        font: Option<String>,
        /// Bold / italic: uses the font's bold/italic file when it exists
        /// (segoeuib.ttf, arialbd.ttf...), otherwise a synthetic bold.
        #[serde(default)]
        bold: bool,
        #[serde(default)]
        italic: bool,
        /// Wraps lines longer than this width (pixels).
        #[serde(alias = "max_width")]
        wrap: Option<f32>,
        /// At most this many lines; the last one ends with "…".
        max_lines: Option<usize>,
    },
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: ColorSpec,
        #[serde(default)]
        radius: f32,
        border: Option<ColorSpec>,
        #[serde(default = "one")]
        border_width: f32,
    },
    Bar {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        #[serde(alias = "source")]
        value: Expr,
        #[serde(default = "zero")]
        min: Expr,
        #[serde(default = "hundred")]
        max: Expr,
        #[serde(default = "white")]
        color: ColorSpec,
        #[serde(default = "default_track")]
        background: ColorSpec,
        #[serde(default)]
        radius: f32,
        #[serde(default)]
        vertical: bool,
    },
    Graph {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        #[serde(alias = "source")]
        value: Expr,
        #[serde(default = "zero")]
        min: Expr,
        #[serde(default = "hundred")]
        max: Expr,
        #[serde(default = "white")]
        color: ColorSpec,
        /// Fill below the line (optional).
        fill: Option<ColorSpec>,
        #[serde(default = "one")]
        line_width: f32,
        /// Pixels between samples.
        #[serde(default = "three")]
        step: f32,
    },
    /// Circular gauge. `x`/`y` are the center.
    Ring {
        x: f32,
        y: f32,
        radius: f32,
        #[serde(default = "six")]
        thickness: f32,
        #[serde(alias = "source")]
        value: Expr,
        #[serde(default = "zero")]
        min: Expr,
        #[serde(default = "hundred")]
        max: Expr,
        #[serde(default = "white")]
        color: ColorSpec,
        #[serde(default = "default_track")]
        background: ColorSpec,
        /// Start angle in degrees (0 = right, -90 = top).
        #[serde(default = "minus_ninety")]
        start: f32,
        /// Total angle of the gauge in degrees.
        #[serde(default = "full_turn")]
        sweep: f32,
        #[serde(default = "yes")]
        round_caps: bool,
    },
    Image {
        x: f32,
        y: f32,
        w: Option<f32>,
        h: Option<f32>,
        /// File path (relative to the widget file) or `{media_cover}`.
        path: Text,
        hover_path: Option<Text>,
        #[serde(default)]
        radius: f32,
        #[serde(default = "one")]
        opacity: f32,
    },
    /// Audio visualizer (what is playing on the speakers).
    Spectrum {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        #[serde(default = "default_bars")]
        bars: usize,
        #[serde(default = "white")]
        color: ColorSpec,
        #[serde(default = "two")]
        gap: f32,
        #[serde(default)]
        radius: f32,
    },
    /// Month calendar with today highlighted.
    Calendar {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        #[serde(default = "default_size")]
        size: f32,
        #[serde(default = "white")]
        color: ColorSpec,
        /// Color of the weekday names and of days from other months.
        #[serde(default = "dim")]
        dim_color: ColorSpec,
        #[serde(default = "default_today")]
        today_color: ColorSpec,
        #[serde(default = "white")]
        today_text: ColorSpec,
        /// Month and year above the grid.
        #[serde(default = "yes")]
        header: bool,
        /// Weeks start on Monday (default: Sunday).
        #[serde(default)]
        monday_first: bool,
        /// Months to move from the current one (an expression, e.g. a variable
        /// changed with !set to browse months).
        #[serde(default = "zero")]
        offset: Expr,
    },
}

#[derive(Deserialize, Clone, Debug)]
pub struct Element {
    #[serde(flatten)]
    pub kind: Kind,
    /// Only drawn while this is true.
    pub visible: Option<Expr>,
    #[serde(default)]
    pub on_click: Actions,
    #[serde(default)]
    pub on_right_click: Actions,
    #[serde(default)]
    pub on_middle_click: Actions,
    #[serde(default)]
    pub on_scroll_up: Actions,
    #[serde(default)]
    pub on_scroll_down: Actions,
    /// Color used while the mouse is over the element.
    pub hover_color: Option<ColorSpec>,
    /// Rotation in degrees (can be an expression, e.g. `"second * 6"`).
    pub rotate: Option<Expr>,
    /// Rotation pivot in widget coordinates (default: element center).
    pub rotate_center: Option<[f32; 2]>,
    /// Bars and rings slide to new values over this many milliseconds.
    #[serde(default)]
    pub animate: u32,
}

impl Element {
    pub fn interactive(&self) -> bool {
        !(self.on_click.is_empty()
            && self.on_right_click.is_empty()
            && self.on_middle_click.is_empty()
            && self.on_scroll_up.is_empty()
            && self.on_scroll_down.is_empty()
            && self.hover_color.is_none()
            && !matches!(self.kind, Kind::Image { hover_path: Some(_), .. }))
    }

    /// Every variable/function the element uses.
    pub fn names(&self, f: &mut dyn FnMut(&str)) {
        let e = |x: &Expr, f: &mut dyn FnMut(&str)| x.0.names(f);
        if let Some(v) = &self.visible {
            e(v, f);
        }
        if let Some(r) = &self.rotate {
            e(r, f);
        }
        if let Some(c) = &self.hover_color {
            c.names(f);
        }
        for a in [&self.on_click, &self.on_right_click, &self.on_middle_click, &self.on_scroll_up, &self.on_scroll_down] {
            a.0.iter().for_each(|t| t.names(f));
        }
        match &self.kind {
            Kind::Text { text, color, .. } => {
                text.0.names(f);
                color.names(f);
            }
            Kind::Rect { color, border, .. } => {
                color.names(f);
                if let Some(b) = border {
                    b.names(f);
                }
            }
            Kind::Bar { value, min, max, color, background, .. }
            | Kind::Ring { value, min, max, color, background, .. } => {
                e(value, f);
                e(min, f);
                e(max, f);
                color.names(f);
                background.names(f);
            }
            Kind::Graph { value, min, max, color, fill, .. } => {
                e(value, f);
                e(min, f);
                e(max, f);
                color.names(f);
                if let Some(c) = fill {
                    c.names(f);
                }
            }
            Kind::Image { path, hover_path, .. } => {
                path.0.names(f);
                if let Some(p) = hover_path {
                    p.0.names(f);
                }
            }
            Kind::Spectrum { color, .. } => {
                f("audio_level");
                color.names(f);
            }
            Kind::Calendar { color, dim_color, today_color, today_text, offset, .. } => {
                for c in [color, dim_color, today_color, today_text] {
                    c.names(f);
                }
                e(offset, f);
            }
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct WidgetConfig {
    /// Unique name, used to remember the position.
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default = "default_update")]
    pub update_ms: u32,
    #[serde(default = "yes")]
    pub draggable: bool,
    #[serde(default)]
    pub click_through: bool,
    #[serde(default)]
    pub z: ZOrder,
    /// Overall opacity, 0-255.
    #[serde(default = "full")]
    pub opacity: u8,
    /// Overall opacity while the mouse is over the widget.
    pub hover_opacity: Option<u8>,
    /// Size multiplier, on top of the monitor's scaling (125%, 150%...).
    #[serde(default = "one")]
    pub scale: f32,
    #[serde(default = "default_font")]
    pub font: String,
    /// Lua (LuaJIT) script, relative to the widget file.
    pub script: Option<String>,
    /// User variables: numbers, strings or `{expressions}`.
    #[serde(default)]
    pub variables: toml::Table,
    pub background: Option<Background>,
    #[serde(default, rename = "web")]
    pub web: Vec<WebConfig>,
    #[serde(default, rename = "element")]
    pub elements: Vec<Element>,
    /// Folder of the .toml file (paths are relative to it).
    #[serde(skip)]
    pub dir: PathBuf,
}

impl WidgetConfig {
    pub fn resolve(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() { p.to_path_buf() } else { self.dir.join(p) }
    }
}

fn default_bg() -> ColorSpec { ColorSpec::fixed(Rgba { r: 16, g: 16, b: 24, a: 192 }) }
fn default_track() -> ColorSpec { ColorSpec::fixed(Rgba { r: 255, g: 255, b: 255, a: 40 }) }
fn white() -> ColorSpec { ColorSpec::fixed(Rgba::WHITE) }
fn dim() -> ColorSpec { ColorSpec::fixed(Rgba { r: 255, g: 255, b: 255, a: 110 }) }
fn default_today() -> ColorSpec { ColorSpec::fixed(Rgba { r: 90, g: 200, b: 250, a: 255 }) }
fn default_size() -> f32 { 14.0 }
fn default_update() -> u32 { 1000 }
fn default_interval() -> u64 { 600 }
fn default_bars() -> usize { 32 }
fn default_font() -> String { "C:/Windows/Fonts/segoeui.ttf".into() }
fn zero() -> Expr { Expr::num(0.0) }
fn hundred() -> Expr { Expr::num(100.0) }
fn one() -> f32 { 1.0 }
fn two() -> f32 { 2.0 }
fn three() -> f32 { 3.0 }
fn six() -> f32 { 6.0 }
fn minus_ninety() -> f32 { -90.0 }
fn full_turn() -> f32 { 360.0 }
fn yes() -> bool { true }
fn full() -> u8 { 255 }

/// Loads every enabled `*.toml` in a folder. Returns valid widgets + readable errors.
pub fn load_dir(dir: &Path) -> (Vec<WidgetConfig>, Vec<String>) {
    let mut widgets = Vec::new();
    let mut errors = Vec::new();
    let mut paths: Vec<_> = match std::fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path())).collect(),
        Err(e) => return (widgets, vec![format!("{}: {e}", dir.display())]),
    };
    paths.sort();
    for p in paths.into_iter().filter(|p| p.extension().is_some_and(|e| e == "toml")) {
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| {
            toml::from_str::<WidgetConfig>(&s).map_err(|e| e.to_string())
        }) {
            Ok(mut w) if w.enabled => {
                w.dir = dir.to_path_buf();
                widgets.push(w);
            }
            Ok(_) => {}
            Err(e) => errors.push(format!("{}:\n{e}", p.display())),
        }
    }
    (widgets, errors)
}

/// Example files written on first run (and by "Install example widgets").
pub const EXAMPLES: &[(&str, &[u8])] = &[
    ("clock.toml", include_bytes!("../widgets/clock.toml")),
    ("system.toml", include_bytes!("../widgets/system.toml")),
    ("analog-clock.toml", include_bytes!("../widgets/analog-clock.toml")),
    ("gauges.toml", include_bytes!("../widgets/gauges.toml")),
    ("media.toml", include_bytes!("../widgets/media.toml")),
    ("weather.toml", include_bytes!("../widgets/weather.toml")),
    ("weather.lua", include_bytes!("../widgets/weather.lua")),
    ("news.toml", include_bytes!("../widgets/news.toml")),
    ("launcher.toml", include_bytes!("../widgets/launcher.toml")),
    ("calendar.toml", include_bytes!("../widgets/calendar.toml")),
    ("processes.toml", include_bytes!("../widgets/processes.toml")),
    ("assets/plume.png", include_bytes!("../widgets/assets/plume.png")),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn examples_parse() {
        for (name, src) in EXAMPLES.iter().filter(|(n, _)| n.ends_with(".toml")) {
            let s = std::str::from_utf8(src).unwrap();
            toml::from_str::<WidgetConfig>(s).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    #[test]
    fn colors() {
        let c = Rgba::parse("#FF800040").unwrap();
        assert_eq!((c.r, c.g, c.b, c.a), (255, 128, 0, 64));
        assert!(Rgba::parse("#123").is_none());
    }

    #[test]
    fn element_fields() {
        let w: WidgetConfig = toml::from_str(
            r##"
            name = "t"
            width = 10
            height = 10
            [variables]
            accent = "#FF0000"
            [[element]]
            type = "bar"
            x = 0
            y = 0
            w = 5
            h = 5
            source = "cpu"
            color = "cpu > 50 ? '#FF0000' : accent"
            on_click = ["!reload", "https://example.com"]
            rotate = 45
            "##,
        )
        .unwrap();
        let el = &w.elements[0];
        assert!(el.interactive());
        assert_eq!(el.on_click.0.len(), 2);
        assert!(matches!(&el.kind, Kind::Bar { color: ColorSpec::Dynamic(_), .. }));
    }
}
