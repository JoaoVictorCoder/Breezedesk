//! `.plume` widget packages: a zip with widget files (TOML, Lua, images,
//! fonts). Double-clicking one installs it into the widgets folder.
//!
//! Only widget file types are accepted (never programs), and every path must
//! stay inside the widgets folder.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

pub const EXT: &str = "plume";
const ALLOWED: &[&str] = &["toml", "lua", "png", "jpg", "jpeg", "bmp", "ico", "ttf", "otf", "txt", "md", "json"];
const MAX_FILES: usize = 300;
const MAX_BYTES: u64 = 50 * 1024 * 1024;

pub struct Package {
    /// (path relative to the widgets folder, contents)
    pub files: Vec<(PathBuf, Vec<u8>)>,
}

fn allowed(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| ALLOWED.contains(&e.to_lowercase().as_str()))
}

/// A path that stays inside the widgets folder (no "..", no drive, no root).
fn safe_relative(p: &Path) -> bool {
    p.components().all(|c| matches!(c, Component::Normal(_))) && p.components().next().is_some()
}

impl Package {
    pub fn open(path: &Path) -> Result<Package, String> {
        let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut zip = zip::ZipArchive::new(f).map_err(|e| format!("not a valid .plume package ({e})"))?;
        let mut files = Vec::new();
        let mut total = 0u64;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            if entry.is_dir() {
                continue;
            }
            let raw = entry.name().map(|n| n.to_string()).unwrap_or_default();
            let absolute = raw.starts_with(['/', '\\']) || raw.contains(':');
            let Some(rel) = entry.enclosed_name().filter(|p| safe_relative(p) && !absolute) else {
                return Err(format!("unsafe path in package: {}", entry.name().map(|n| n.to_string()).unwrap_or_default()));
            };
            if !allowed(&rel) {
                return Err(format!("file type not allowed in a widget package: {}", rel.display()));
            }
            total += entry.size();
            if files.len() >= MAX_FILES || total > MAX_BYTES {
                return Err("package is too large".into());
            }
            let mut buf = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            files.push((rel, buf));
        }
        if !files.iter().any(|(p, _)| is_widget(p)) {
            return Err("the package contains no widget (.toml file)".into());
        }
        Ok(Package { files })
    }

    /// Names of the widgets inside.
    pub fn widgets(&self) -> Vec<String> {
        self.files
            .iter()
            .filter(|(p, _)| is_widget(p))
            .map(|(p, data)| {
                std::str::from_utf8(data)
                    .ok()
                    .and_then(|s| s.parse::<toml::Table>().ok())
                    .and_then(|t| t.get("name").and_then(|n| n.as_str()).map(str::to_string))
                    .unwrap_or_else(|| p.display().to_string())
            })
            .collect()
    }

    /// Files that already exist in `dir` with different contents.
    pub fn conflicts(&self, dir: &Path) -> Vec<String> {
        self.files
            .iter()
            .filter(|(p, data)| std::fs::read(dir.join(p)).is_ok_and(|old| old != *data))
            .map(|(p, _)| p.display().to_string())
            .collect()
    }

    pub fn install(&self, dir: &Path) -> Result<usize, String> {
        for (rel, data) in &self.files {
            let dest = dir.join(rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(&dest, data).map_err(|e| format!("{}: {e}", dest.display()))?;
        }
        Ok(self.files.len())
    }
}

/// Widget files sit at the top of the package (like in the widgets folder).
fn is_widget(p: &Path) -> bool {
    p.components().count() == 1 && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("toml"))
}

/// Files a widget uses: its script, images and fonts with fixed relative paths.
#[allow(dead_code)] // used by the editor (which includes this file)
pub fn references(dir: &Path, widget: &Path) -> Vec<PathBuf> {
    fn walk(v: &toml::Value, dir: &Path, out: &mut BTreeSet<PathBuf>) {
        match v {
            toml::Value::Table(t) => {
                for (k, v) in t {
                    if let (true, Some(s)) = (["script", "path", "hover_path", "font"].contains(&k.as_str()), v.as_str()) {
                        let p = Path::new(s);
                        if !s.contains('{') && p.is_relative() && safe_relative(p) && allowed(p) && dir.join(p).is_file() {
                            out.insert(p.to_path_buf());
                        }
                    }
                    walk(v, dir, out);
                }
            }
            toml::Value::Array(a) => a.iter().for_each(|v| walk(v, dir, out)),
            _ => {}
        }
    }
    let mut out = BTreeSet::new();
    if let Some(v) = std::fs::read_to_string(dir.join(widget)).ok().and_then(|s| s.parse::<toml::Table>().ok()) {
        walk(&toml::Value::Table(v), dir, &mut out);
    }
    out.into_iter().collect()
}

/// Writes a package with the given widget files (paths relative to `dir`) and
/// everything they reference. Returns the number of files.
#[allow(dead_code)] // used by the editor
pub fn create(dir: &Path, widgets: &[PathBuf], out: &Path) -> Result<usize, String> {
    let mut files: BTreeSet<PathBuf> = BTreeSet::new();
    for w in widgets {
        files.insert(w.clone());
        files.extend(references(dir, w));
    }
    let f = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut zip = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for rel in &files {
        let data = std::fs::read(dir.join(rel)).map_err(|e| format!("{}: {e}", rel.display()))?;
        let name = rel.to_string_lossy().replace('\\', "/");
        zip.start_file(name, opts).map_err(|e| e.to_string())?;
        zip.write_all(&data).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(files.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("plume-pkg-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn roundtrip_and_safety() {
        let src = tmp("src");
        std::fs::create_dir_all(src.join("assets")).unwrap();
        std::fs::write(
            src.join("w.toml"),
            "name = \"demo\"\nwidth = 1\nheight = 1\nscript = \"w.lua\"\n[[element]]\ntype = \"image\"\nx = 0\ny = 0\npath = \"assets/a.png\"\n",
        )
        .unwrap();
        std::fs::write(src.join("w.lua"), "-- lua").unwrap();
        std::fs::write(src.join("assets/a.png"), [1, 2, 3]).unwrap();
        std::fs::write(src.join("other.toml"), "name = \"other\"").unwrap();

        assert_eq!(references(&src, Path::new("w.toml")), vec![PathBuf::from("assets/a.png"), PathBuf::from("w.lua")]);
        let out = src.join("demo.plume");
        assert_eq!(create(&src, &[PathBuf::from("w.toml")], &out).unwrap(), 3);

        let pkg = Package::open(&out).unwrap();
        assert_eq!(pkg.widgets(), vec!["demo".to_string()]);
        let dst = tmp("dst");
        assert!(pkg.conflicts(&dst).is_empty());
        assert_eq!(pkg.install(&dst).unwrap(), 3);
        assert!(dst.join("assets/a.png").is_file());
        std::fs::write(dst.join("w.lua"), "changed").unwrap();
        assert_eq!(pkg.conflicts(&dst), vec!["w.lua".to_string()]);

        // Malicious packages are refused.
        let bad = |name: &str, entry: &str| {
            let p = src.join(name);
            let mut z = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
            z.start_file("ok.toml", zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(b"name='x'").unwrap();
            z.start_file(entry, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(b"x").unwrap();
            z.finish().unwrap();
            Package::open(&p)
        };
        assert!(bad("a.plume", "../evil.toml").is_err());
        assert!(bad("b.plume", "virus.exe").is_err());
        assert!(bad("c.plume", "/etc/x.toml").is_err());
    }
}
