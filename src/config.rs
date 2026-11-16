//! Widget file format (TOML).

use serde::{Deserialize, Deserializer};
use std::path::Path;

/// RGBA color parsed from "#RRGGBB" or "#RRGGBBAA" strings.
#[derive(Clone, Copy, Debug)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const WHITE: Rgba = Rgba { r: 255, g: 255, b: 255, a: 255 };

    pub fn parse(s: &str) -> Option<Rgba> {
        let h = s.trim().trim_start_matches('#');
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

impl<'de> Deserialize<'de> for Rgba {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Rgba::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("invalid color: {s:?}")))
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

#[derive(Deserialize, Clone, Copy, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Cpu,
    Ram,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Background {
    #[serde(default = "default_bg")]
    pub color: Rgba,
    #[serde(default)]
    pub radius: f32,
    pub border: Option<Rgba>,
    #[serde(default = "one")]
    pub border_width: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Element {
    Text {
        x: f32,
        y: f32,
        text: String,
        #[serde(default = "default_size")]
        size: f32,
        #[serde(default = "white")]
        color: Rgba,
        #[serde(default)]
        align: Align,
        /// Font for this text only (otherwise the widget's font is used).
        font: Option<String>,
    },
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Rgba,
        #[serde(default)]
        radius: f32,
    },
    Bar {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        source: Source,
        #[serde(default = "white")]
        color: Rgba,
        #[serde(default = "default_track")]
        background: Rgba,
        #[serde(default)]
        radius: f32,
    },
    Graph {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        source: Source,
        #[serde(default = "white")]
        color: Rgba,
        /// Fill below the line (optional).
        fill: Option<Rgba>,
        #[serde(default = "one")]
        line_width: f32,
    },
}

#[derive(Deserialize, Clone, Debug)]
pub struct WidgetConfig {
    /// Unique name, used to remember the position.
    pub name: String,
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
    #[serde(default = "default_font")]
    pub font: String,
    pub background: Option<Background>,
    #[serde(default, rename = "element")]
    pub elements: Vec<Element>,
}

fn default_bg() -> Rgba { Rgba { r: 16, g: 16, b: 24, a: 192 } }
fn default_track() -> Rgba { Rgba { r: 255, g: 255, b: 255, a: 40 } }
fn white() -> Rgba { Rgba::WHITE }
fn default_size() -> f32 { 14.0 }
fn default_update() -> u32 { 1000 }
fn default_font() -> String { "C:/Windows/Fonts/segoeui.ttf".into() }
fn one() -> f32 { 1.0 }
fn yes() -> bool { true }
fn full() -> u8 { 255 }

/// Loads every `*.toml` in a folder. Returns valid widgets + readable errors.
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
            Ok(w) => widgets.push(w),
            Err(e) => errors.push(format!("{}:\n{e}", p.display())),
        }
    }
    (widgets, errors)
}

/// Examples written on first run.
pub const EXAMPLES: &[(&str, &str)] = &[
    ("clock.toml", include_str!("../widgets/clock.toml")),
    ("system.toml", include_str!("../widgets/system.toml")),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn examples_parse() {
        for (name, src) in EXAMPLES {
            toml::from_str::<WidgetConfig>(src).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    #[test]
    fn colors() {
        let c = Rgba::parse("#FF800040").unwrap();
        assert_eq!((c.r, c.g, c.b, c.a), (255, 128, 0, 64));
        assert!(Rgba::parse("#123").is_none());
    }
}
