//! Plume Editor: edit widgets visually instead of writing TOML by hand.
//!
//! It edits the same `.toml` files Plume reads, keeping their comments and
//! layout (toml_edit). Plume reloads a widget as soon as its file is saved, so
//! with "Auto-save" on, changes show up on the desktop while you drag values.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[path = "../../src/package.rs"]
mod package;

use eframe::egui::{self, Color32, RichText};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
use toml_edit::{value, ArrayOfTables, DocumentMut, Item, Table, Value};

// ------------------------------------------------------------------ schema

const WIDGET_KEYS: &[&str] = &[
    "name", "enabled", "x", "y", "width", "height", "update_ms", "z", "opacity", "hover_opacity", "scale", "draggable",
    "click_through", "font", "script",
];
const BACKGROUND_KEYS: &[&str] = &["color", "radius", "border", "border_width", "blur"];
const WEB_KEYS: &[&str] = &["name", "url", "interval", "format"];
const COMMON_KEYS: &[&str] = &[
    "visible", "on_click", "on_right_click", "on_middle_click", "on_scroll_up", "on_scroll_down", "hover_color", "rotate",
    "rotate_center", "animate",
];
const TYPES: &[&str] = &["text", "rect", "bar", "graph", "ring", "image", "spectrum", "calendar"];

fn type_keys(t: &str) -> &'static [&'static str] {
    match t {
        "text" => &["x", "y", "text", "size", "color", "align", "font", "bold", "italic", "wrap", "max_lines"],
        "rect" => &["x", "y", "w", "h", "color", "radius", "border", "border_width"],
        "bar" => &["x", "y", "w", "h", "value", "min", "max", "color", "background", "radius", "vertical"],
        "graph" => &["x", "y", "w", "h", "value", "min", "max", "color", "fill", "line_width", "step"],
        "ring" => &["x", "y", "radius", "thickness", "value", "min", "max", "color", "background", "start", "sweep", "round_caps"],
        "image" => &["x", "y", "w", "h", "path", "hover_path", "radius", "opacity"],
        "spectrum" => &["x", "y", "w", "h", "bars", "color", "gap", "radius"],
        "calendar" => &[
            "x", "y", "w", "h", "size", "color", "dim_color", "today_color", "today_text", "header", "monday_first", "offset",
        ],
        _ => &[],
    }
}

/// Fixed choices for some keys.
fn choices(key: &str) -> Option<&'static [&'static str]> {
    Some(match key {
        "z" => &["bottom", "normal", "top"],
        "align" => &["left", "center", "right"],
        "blur" => &["none", "blur", "acrylic"],
        "format" => &["json", "rss", "text"],
        _ => return None,
    })
}

/// Value for a newly added key.
fn default_for(key: &str) -> Value {
    match key {
        "enabled" | "draggable" | "header" | "round_caps" => true.into(),
        "click_through" | "bold" | "italic" | "vertical" | "monday_first" => false.into(),
        "opacity" | "hover_opacity" => 255i64.into(),
        "scale" => 1.0.into(),
        "update_ms" => 1000i64.into(),
        "size" => 14i64.into(),
        "max" => 100i64.into(),
        "radius" | "border_width" | "line_width" | "thickness" => 4i64.into(),
        "interval" => 600i64.into(),
        "animate" => 300i64.into(),
        "bars" => 32i64.into(),
        "width" | "height" | "w" | "h" | "wrap" => 100i64.into(),
        "max_lines" => 1i64.into(),
        "sweep" => 360i64.into(),
        "start" => (-90i64).into(),
        "rotate_center" => {
            let mut a = toml_edit::Array::new();
            a.push(0i64);
            a.push(0i64);
            Value::Array(a)
        }
        k if is_color_key(k) => "#FFFFFF".into(),
        "text" => "Text".into(),
        "value" => "cpu".into(),
        "visible" => "true".into(),
        "z" => "bottom".into(),
        "align" => "left".into(),
        "blur" => "none".into(),
        "format" => "json".into(),
        "font" => "C:/Windows/Fonts/segoeui.ttf".into(),
        k if k.starts_with("on_") => "https://".into(),
        "url" => "https://".into(),
        "name" => "data".into(),
        "path" | "hover_path" => "assets/image.png".into(),
        "script" => "script.lua".into(),
        _ => 0i64.into(),
    }
}

fn is_color_key(k: &str) -> bool {
    k == "color" || k.ends_with("_color") || matches!(k, "border" | "background" | "fill" | "today_text")
}

fn new_element(t: &str) -> Table {
    let mut tb = Table::new();
    tb["type"] = value(t);
    for k in type_keys(t) {
        let needed = match t {
            "text" => ["x", "y", "text", "size", "color"].contains(k),
            "ring" => ["x", "y", "radius", "thickness", "value", "color"].contains(k),
            "image" => ["x", "y", "w", "h", "path"].contains(k),
            "calendar" => ["x", "y", "w", "h", "size"].contains(k),
            _ => ["x", "y", "w", "h", "color", "value"].contains(k),
        };
        if needed {
            tb[*k] = Item::Value(match *k {
                "x" | "y" if t == "ring" => 50i64.into(),
                "x" | "y" => 10i64.into(),
                "radius" if t == "ring" => 30i64.into(),
                _ => default_for(k),
            });
        }
    }
    tb
}

fn parse_color(s: &str) -> Option<Color32> {
    let h = s.trim().strip_prefix('#')?;
    let b = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    match h.len() {
        6 => Some(Color32::from_rgb(b(0)?, b(2)?, b(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(b(0)?, b(2)?, b(4)?, b(6)?)),
        _ => None,
    }
}

fn color_hex(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 255 { format!("#{r:02X}{g:02X}{b:02X}") } else { format!("#{r:02X}{g:02X}{b:02X}{a:02X}") }
}

/// Replaces a value but keeps its comments/spacing.
fn set_value(slot: &mut Value, mut new: Value) {
    *new.decor_mut() = slot.decor().clone();
    *slot = new;
}

// ------------------------------------------------------------------ files

struct WidgetFile {
    path: PathBuf,
    doc: Option<DocumentMut>,
    error: Option<String>,
    dirty: bool,
    modified: Option<SystemTime>,
}

impl WidgetFile {
    fn load(path: PathBuf) -> WidgetFile {
        let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|s| s.parse::<DocumentMut>().map_err(|e| e.to_string())) {
            Ok(doc) => WidgetFile { path, doc: Some(doc), error: None, dirty: false, modified },
            Err(e) => WidgetFile { path, doc: None, error: Some(e), dirty: false, modified },
        }
    }

    fn name(&self) -> String {
        self.doc
            .as_ref()
            .and_then(|d| d.get("name").and_then(|n| n.as_str()).map(str::to_string))
            .unwrap_or_else(|| self.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())
    }

    fn enabled(&self) -> bool {
        self.doc.as_ref().and_then(|d| d.get("enabled").and_then(|e| e.as_bool())).unwrap_or(true)
    }

    fn save(&mut self) -> Result<(), String> {
        if let Some(doc) = &self.doc {
            std::fs::write(&self.path, doc.to_string()).map_err(|e| e.to_string())?;
            self.dirty = false;
            self.modified = std::fs::metadata(&self.path).and_then(|m| m.modified()).ok();
        }
        Ok(())
    }
}

fn list_widgets(dir: &Path) -> Vec<WidgetFile> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "toml")).collect())
        .unwrap_or_default();
    paths.sort();
    paths.into_iter().map(WidgetFile::load).collect()
}

/// Same folder Plume uses (see Plume's main.rs).
fn default_dir() -> PathBuf {
    if let Some(arg) = std::env::args().nth(1) {
        return PathBuf::from(arg);
    }
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default();
    let portable = exe_dir.join("widgets");
    if portable.exists() {
        return portable;
    }
    std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("Plume").join("widgets")).unwrap_or(portable)
}

fn open_external(path: &Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(not(windows))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

// ------------------------------------------------------------------ app

struct Editor {
    dir: PathBuf,
    files: Vec<WidgetFile>,
    selected: usize,
    auto_save: bool,
    last_change: Option<Instant>,
    last_disk_check: Instant,
    status: String,
    confirm_delete: bool,
    new_var: String,
}

impl Editor {
    fn new(dir: PathBuf) -> Editor {
        let files = list_widgets(&dir);
        Editor {
            status: format!("{} widgets in {}", files.len(), dir.display()),
            dir,
            files,
            selected: 0,
            auto_save: true,
            last_change: None,
            last_disk_check: Instant::now(),
            confirm_delete: false,
            new_var: String::new(),
        }
    }

    fn reload_list(&mut self) {
        let current = self.files.get(self.selected).map(|f| f.path.clone());
        self.files = list_widgets(&self.dir);
        self.selected = current.and_then(|p| self.files.iter().position(|f| f.path == p)).unwrap_or(0);
    }

    fn save_all(&mut self) {
        let mut n = 0;
        for f in self.files.iter_mut().filter(|f| f.dirty) {
            match f.save() {
                Ok(()) => n += 1,
                Err(e) => self.status = format!("Could not save {}: {e}", f.path.display()),
            }
        }
        if n > 0 {
            self.status = format!("Saved {n} file(s). Plume reloads them automatically.");
        }
    }

    fn changed(&mut self) {
        if let Some(f) = self.files.get_mut(self.selected) {
            f.dirty = true;
        }
        self.last_change = Some(Instant::now());
    }

    fn new_widget(&mut self) {
        let mut n = 1;
        let path = loop {
            let p = self.dir.join(format!("widget-{n}.toml"));
            if !p.exists() {
                break p;
            }
            n += 1;
        };
        let body = format!(
            "# Created with Plume Editor.\nname = \"widget-{n}\"\nx = 200\ny = 200\nwidth = 240\nheight = 90\n\n\
             [background]\ncolor = \"#0E0F14B0\"\nradius = 14\n\n\
             [[element]]\ntype = \"text\"\nx = 16\ny = 14\nsize = 32\ntext = \"{{time:%H:%M}}\"\n"
        );
        match std::fs::write(&path, body) {
            Ok(()) => {
                self.reload_list();
                self.selected = self.files.iter().position(|f| f.path == path).unwrap_or(0);
                self.status = format!("Created {}", path.display());
            }
            Err(e) => self.status = format!("Could not create a widget: {e}"),
        }
    }

    fn install_package(&mut self) {
        let Some(file) = rfd::FileDialog::new().add_filter("Plume package", &[package::EXT]).pick_file() else { return };
        let pkg = match package::Package::open(&file) {
            Ok(p) => p,
            Err(e) => {
                self.status = format!("Could not open the package: {e}");
                return;
            }
        };
        let conflicts = pkg.conflicts(&self.dir);
        let mut text = format!("Install {}?", pkg.widgets().join(", "));
        if !conflicts.is_empty() {
            text.push_str(&format!("\n\nThese files will be replaced:\n{}", conflicts.join("\n")));
        }
        text.push_str("\n\nWidgets can open programs and websites when clicked. Only install packages you trust.");
        let ok = rfd::MessageDialog::new()
            .set_title("Install widget package")
            .set_description(text)
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        if ok != rfd::MessageDialogResult::Yes {
            return;
        }
        self.status = match pkg.install(&self.dir) {
            Ok(n) => format!("Installed {n} files."),
            Err(e) => format!("Installation failed: {e}"),
        };
        self.reload_list();
    }

    fn export_package(&mut self) {
        let Some(f) = self.files.get(self.selected) else { return };
        let Some(rel) = f.path.file_name().map(PathBuf::from) else { return };
        let Some(out) = rfd::FileDialog::new()
            .add_filter("Plume package", &[package::EXT])
            .set_file_name(format!("{}.{}", f.name(), package::EXT))
            .save_file()
        else {
            return;
        };
        self.status = match package::create(&self.dir, &[rel], &out) {
            Ok(n) => format!("Exported {n} files to {}", out.display()),
            Err(e) => format!("Export failed: {e}"),
        };
    }

    /// Picks up changes made to the files by other programs.
    fn check_disk(&mut self) {
        if self.last_disk_check.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_disk_check = Instant::now();
        let on_disk: Vec<PathBuf> = list_widgets_paths(&self.dir);
        if on_disk.len() != self.files.len() || on_disk.iter().zip(&self.files).any(|(p, f)| *p != f.path) {
            self.reload_list();
            return;
        }
        for f in self.files.iter_mut().filter(|f| !f.dirty) {
            let m = std::fs::metadata(&f.path).and_then(|m| m.modified()).ok();
            if m != f.modified {
                *f = WidgetFile::load(f.path.clone());
            }
        }
    }
}

fn list_widgets_paths(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "toml")).collect())
        .unwrap_or_default();
    paths.sort();
    paths
}

// ------------------------------------------------------------------ field editors

/// Edits the plain values of a table (not sub-tables). Returns true if changed.
fn edit_values(ui: &mut egui::Ui, table: &mut Table, known: &[&str], id: &str) -> bool {
    let mut changed = false;
    let keys: Vec<String> = table.iter().filter(|(_, i)| i.is_value()).map(|(k, _)| k.to_string()).collect();
    let mut remove = None;
    egui::Grid::new(id).num_columns(3).spacing([10.0, 6.0]).striped(true).show(ui, |ui| {
        for key in &keys {
            let Some(v) = table.get_mut(key).and_then(|i| i.as_value_mut()) else { continue };
            ui.label(RichText::new(key).monospace());
            ui.push_id((id, key), |ui| {
                changed |= edit_value(ui, key, v);
            });
            if key != "type" && ui.small_button("x").on_hover_text("Remove this setting").clicked() {
                remove = Some(key.clone());
            }
            ui.end_row();
        }
    });
    if let Some(k) = remove {
        table.remove(&k);
        changed = true;
    }
    let missing: Vec<&&str> = known.iter().filter(|k| !table.contains_key(k)).collect();
    if !missing.is_empty() {
        let mut add: Option<&str> = None;
        egui::ComboBox::from_id_salt((id, "add")).selected_text("+ Add setting").show_ui(ui, |ui| {
            for k in missing {
                if ui.selectable_label(false, *k).clicked() {
                    add = Some(k);
                }
            }
        });
        if let Some(k) = add {
            table[k] = Item::Value(default_for(k));
            changed = true;
        }
    }
    changed
}

fn edit_value(ui: &mut egui::Ui, key: &str, v: &mut Value) -> bool {
    match v {
        Value::Integer(i) => {
            let mut n = *i.value();
            let speed = if matches!(key, "update_ms" | "interval") { 10.0 } else { 1.0 };
            let r = ui.add(egui::DragValue::new(&mut n).speed(speed));
            if r.changed() {
                set_value(v, n.into());
                return true;
            }
            false
        }
        Value::Float(f) => {
            let mut n = *f.value();
            if ui.add(egui::DragValue::new(&mut n).speed(0.1)).changed() {
                set_value(v, n.into());
                return true;
            }
            false
        }
        Value::Boolean(b) => {
            let mut x = *b.value();
            if ui.checkbox(&mut x, "").changed() {
                set_value(v, x.into());
                return true;
            }
            false
        }
        Value::String(s) => {
            let mut text = s.value().clone();
            if key == "type" {
                ui.label(RichText::new(text).strong());
                return false;
            }
            let mut changed = false;
            ui.horizontal(|ui| {
                if let Some(opts) = choices(key) {
                    egui::ComboBox::from_id_salt(key).selected_text(text.clone()).show_ui(ui, |ui| {
                        for o in opts {
                            changed |= ui.selectable_value(&mut text, o.to_string(), *o).changed();
                        }
                    });
                } else {
                    if let Some(mut c) = parse_color(&text) {
                        if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::OnlyBlend).changed() {
                            text = color_hex(c);
                            changed = true;
                        }
                    }
                    let edit = if key == "text" && text.contains('\n') {
                        egui::TextEdit::multiline(&mut text).desired_rows(2)
                    } else {
                        egui::TextEdit::singleline(&mut text)
                    };
                    changed |= ui.add(edit.desired_width(300.0).min_size(egui::vec2(300.0, 0.0))).changed();
                }
            });
            if changed {
                set_value(v, text.into());
            }
            changed
        }
        other => {
            // Arrays and inline tables: edited as TOML text.
            let mut text = other.to_string().trim().to_string();
            let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(300.0).min_size(egui::vec2(300.0, 0.0)));
            if r.changed() {
                if let Ok(doc) = format!("x = {text}").parse::<DocumentMut>() {
                    if let Some(nv) = doc.get("x").and_then(|i| i.as_value()).cloned() {
                        set_value(other, nv);
                        return true;
                    }
                }
            }
            false
        }
    }
}

fn summary(t: &Table) -> String {
    let ty = t.get("type").and_then(|v| v.as_str()).unwrap_or("?");
    let detail = ["text", "value", "path"].iter().find_map(|k| t.get(k).and_then(|v| v.as_str())).unwrap_or("");
    let detail: String = detail.chars().take(40).collect();
    format!("{ty}  {detail}")
}

/// Element list with move/remove buttons. Returns true if changed.
fn edit_elements(ui: &mut egui::Ui, doc: &mut DocumentMut) -> bool {
    let mut changed = false;
    let mut action: Option<(usize, i32)> = None; // (index, -1 up / +1 down / 0 delete)
    if let Some(aot) = doc.get_mut("element").and_then(|i| i.as_array_of_tables_mut()) {
        let n = aot.len();
        for (i, t) in aot.iter_mut().enumerate() {
            let ty = t.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
            egui::CollapsingHeader::new(format!("#{}  {}", i + 1, summary(t))).id_salt(("el", i)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.add_enabled(i > 0, egui::Button::new("Up")).clicked() {
                        action = Some((i, -1));
                    }
                    if ui.add_enabled(i + 1 < n, egui::Button::new("Down")).clicked() {
                        action = Some((i, 1));
                    }
                    if ui.button("Delete").clicked() {
                        action = Some((i, 0));
                    }
                });
                let mut known: Vec<&str> = type_keys(&ty).to_vec();
                known.extend_from_slice(COMMON_KEYS);
                changed |= edit_values(ui, t, &known, &format!("element-{i}"));
            });
        }
    }
    if let Some((i, dir)) = action {
        if let Some(aot) = doc.get_mut("element").and_then(|i| i.as_array_of_tables_mut()) {
            let mut tables: Vec<Table> = aot.iter().cloned().collect();
            if dir == 0 {
                tables.remove(i);
            } else {
                let j = (i as i32 + dir) as usize;
                tables.swap(i, j);
            }
            aot.clear();
            for t in tables {
                aot.push(t);
            }
            changed = true;
        }
    }
    let mut add: Option<&str> = None;
    egui::ComboBox::from_id_salt("add-element").selected_text("+ Add element").show_ui(ui, |ui| {
        for t in TYPES {
            if ui.selectable_label(false, *t).clicked() {
                add = Some(t);
            }
        }
    });
    if let Some(t) = add {
        if !doc.contains_key("element") {
            doc["element"] = Item::ArrayOfTables(ArrayOfTables::new());
        }
        if let Some(aot) = doc.get_mut("element").and_then(|i| i.as_array_of_tables_mut()) {
            aot.push(new_element(t));
            changed = true;
        }
    }
    changed
}

impl eframe::App for Editor {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        self.check_disk();
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
            self.save_all();
        }
        if self.auto_save && self.last_change.is_some_and(|t| t.elapsed() > Duration::from_millis(400)) {
            self.last_change = None;
            self.save_all();
        }
        ctx.request_repaint_after(Duration::from_millis(300));

        egui::Panel::top("toolbar").show(root, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Plume Editor");
                ui.separator();
                let dirty = self.files.iter().any(|f| f.dirty);
                if ui.add_enabled(dirty, egui::Button::new("Save")).on_hover_text("Ctrl+S").clicked() {
                    self.save_all();
                }
                ui.checkbox(&mut self.auto_save, "Auto-save").on_hover_text("Save while you edit: changes appear on the desktop right away");
                ui.separator();
                if ui.button("Open folder").clicked() {
                    open_external(&self.dir);
                }
                if ui.button("Install package...").clicked() {
                    self.install_package();
                }
            });
        });
        egui::Panel::bottom("status").show(root, |ui| {
            ui.label(&self.status);
        });

        egui::Panel::left("files").resizable(true).default_size(220.0).show(root, |ui| {
            ui.heading("Widgets");
            ui.label(RichText::new("Checkbox = on/off").small().weak());
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut toggled = None;
                for (i, f) in self.files.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let mut on = f.enabled();
                        if f.doc.is_some() && ui.checkbox(&mut on, "").changed() {
                            toggled = Some((i, on));
                        }
                        let label = format!("{}{}", f.name(), if f.dirty { " •" } else { "" });
                        let text = if f.error.is_some() { RichText::new(label).color(Color32::LIGHT_RED) } else { RichText::new(label) };
                        if ui.selectable_label(i == self.selected, text).clicked() {
                            self.selected = i;
                            self.confirm_delete = false;
                        }
                    });
                }
                if let Some((i, on)) = toggled {
                    self.selected = i;
                    if let Some(doc) = self.files[i].doc.as_mut() {
                        doc["enabled"] = value(on);
                    }
                    self.changed();
                }
            });
            ui.separator();
            if ui.button("+ New widget").clicked() {
                self.new_widget();
            }
            if ui.button("Export as package...").clicked() {
                self.export_package();
            }
            if !self.confirm_delete {
                if ui.button("Delete widget...").clicked() {
                    self.confirm_delete = true;
                }
            } else {
                ui.label("Delete this widget file?");
                ui.horizontal(|ui| {
                    if ui.button("Delete").clicked() {
                        if let Some(f) = self.files.get(self.selected) {
                            let _ = std::fs::remove_file(&f.path);
                            self.status = format!("Deleted {}", f.path.display());
                        }
                        self.confirm_delete = false;
                        self.reload_list();
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_delete = false;
                    }
                });
            }
        });

        egui::CentralPanel::default().show(root, |ui| {
            let Some(file) = self.files.get_mut(self.selected) else {
                ui.label("No widgets in this folder yet. Click \"New widget\".");
                return;
            };
            ui.horizontal(|ui| {
                ui.heading(file.name());
                if ui.small_button("Open in text editor").clicked() {
                    open_external(&file.path);
                }
            });
            ui.label(RichText::new(file.path.display().to_string()).small().weak());
            if let Some(e) = &file.error {
                ui.colored_label(Color32::LIGHT_RED, format!("This file has an error, fix it in a text editor:\n{e}"));
                return;
            }
            let Some(doc) = file.doc.as_mut() else { return };
            let mut changed = false;
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                egui::CollapsingHeader::new(RichText::new("Widget").strong()).default_open(true).show(ui, |ui| {
                    changed |= edit_values(ui, doc.as_table_mut(), WIDGET_KEYS, "widget");
                });
                egui::CollapsingHeader::new(RichText::new("Background").strong()).default_open(true).show(ui, |ui| {
                    match doc.get_mut("background").and_then(|i| i.as_table_mut()) {
                        Some(bg) => changed |= edit_values(ui, bg, BACKGROUND_KEYS, "background"),
                        None => {
                            if ui.button("+ Add background").clicked() {
                                let mut t = Table::new();
                                t["color"] = value("#0E0F14B0");
                                t["radius"] = value(12i64);
                                doc["background"] = Item::Table(t);
                                changed = true;
                            }
                        }
                    }
                });
                egui::CollapsingHeader::new(RichText::new("Elements").strong()).default_open(true).show(ui, |ui| {
                    changed |= edit_elements(ui, doc);
                });
                egui::CollapsingHeader::new(RichText::new("Variables").strong()).show(ui, |ui| {
                    if !doc.contains_key("variables") {
                        doc["variables"] = Item::Table(Table::new());
                    }
                    if let Some(vars) = doc.get_mut("variables").and_then(|i| i.as_table_mut()) {
                        changed |= edit_values(ui, vars, &[], "variables");
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut self.new_var).hint_text("new variable").desired_width(140.0));
                            let name = self.new_var.trim().to_string();
                            if ui.add_enabled(!name.is_empty() && !vars.contains_key(&name), egui::Button::new("+ Add")).clicked() {
                                vars[&name] = value("");
                                self.new_var.clear();
                                changed = true;
                            }
                        });
                    }
                });
                egui::CollapsingHeader::new(RichText::new("Internet data").strong()).show(ui, |ui| {
                    if let Some(aot) = doc.get_mut("web").and_then(|i| i.as_array_of_tables_mut()) {
                        for (i, t) in aot.iter_mut().enumerate() {
                            ui.push_id(("web", i), |ui| {
                                changed |= edit_values(ui, t, WEB_KEYS, &format!("web-{i}"));
                            });
                            ui.separator();
                        }
                    }
                    if ui.button("+ Add data source").clicked() {
                        if !doc.contains_key("web") {
                            doc["web"] = Item::ArrayOfTables(ArrayOfTables::new());
                        }
                        if let Some(aot) = doc.get_mut("web").and_then(|i| i.as_array_of_tables_mut()) {
                            let mut t = Table::new();
                            t["name"] = value("data");
                            t["url"] = value("https://");
                            t["interval"] = value(600i64);
                            t["format"] = value("json");
                            aot.push(t);
                            changed = true;
                        }
                    }
                });
            });
            if changed {
                file.dirty = true;
                self.last_change = Some(Instant::now());
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let dir = default_dir();
    let _ = std::fs::create_dir_all(&dir);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1000.0, 700.0]).with_title("Plume Editor"),
        ..Default::default()
    };
    eframe::run_native("Plume Editor", options, Box::new(move |_cc| Ok(Box::new(Editor::new(dir)))))
}
