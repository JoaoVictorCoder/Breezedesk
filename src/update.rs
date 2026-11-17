//! Automatic updates from the project's GitHub Releases.
//!
//! The latest release is read from the GitHub API. If its version is newer,
//! the installer (`Plume-Setup.exe`) is downloaded, checked against the
//! published SHA-256 (`Plume-Setup.exe.sha256`) and run silently; it replaces
//! the program and starts it again.

use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// GitHub repository the releases come from.
pub const REPO: &str = "JoaoVictorCoder/Plume";
pub const INSTALLER: &str = "Plume-Setup.exe";

/// Builds made from the `dev` branch never update themselves.
pub const DEV_BUILD: bool = option_env!("PLUME_DEV").is_some();

pub type Version = (u64, u64, u64);

#[derive(Clone, Debug)]
pub struct Release {
    pub version: Version,
    pub page: String,
    pub installer_url: Option<String>,
    pub checksum_url: Option<String>,
}

pub fn parse_version(s: &str) -> Option<Version> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next().flatten().unwrap_or(0), it.next().flatten().unwrap_or(0)))
}

pub fn current() -> Version {
    parse_version(env!("CARGO_PKG_VERSION")).unwrap_or((0, 0, 0))
}

pub fn fmt(v: Version) -> String {
    format!("{}.{}.{}", v.0, v.1, v.2)
}

/// API address (overridable with `PLUME_UPDATE_URL` for testing).
fn api_url() -> String {
    std::env::var("PLUME_UPDATE_URL")
        .unwrap_or_else(|_| format!("https://api.github.com/repos/{REPO}/releases/latest"))
}

pub fn parse_release(json: &str) -> Result<Release, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("bad release data: {e}"))?;
    let tag = v["tag_name"].as_str().ok_or("release has no tag")?.to_string();
    let version = parse_version(&tag).ok_or_else(|| format!("unknown version {tag:?}"))?;
    let asset = |name: &str| {
        v["assets"].as_array()?.iter().find(|a| a["name"] == name)?["browser_download_url"]
            .as_str()
            .map(str::to_string)
    };
    Ok(Release {
        version,
        page: v["html_url"].as_str().unwrap_or("").to_string(),
        installer_url: asset(INSTALLER),
        checksum_url: asset(&format!("{INSTALLER}.sha256")),
    })
}

/// The latest release, if it is newer than this build.
pub fn check() -> Result<Option<Release>, String> {
    let rel = parse_release(&crate::web::get(&api_url())?)?;
    Ok((rel.version > current()).then_some(rel))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Downloads the installer to the temp folder and verifies its checksum.
pub fn download(rel: &Release) -> Result<PathBuf, String> {
    let url = rel.installer_url.as_ref().ok_or("the release has no installer")?;
    let sum_url = rel.checksum_url.as_ref().ok_or("the release has no checksum")?;
    let expected = crate::web::get(sum_url)?
        .split_whitespace()
        .next()
        .ok_or("empty checksum file")?
        .to_lowercase();
    let bytes = crate::web::get_bytes(url)?;
    let actual = sha256_hex(&bytes);
    if actual != expected {
        return Err(format!("checksum mismatch (expected {expected}, got {actual})"));
    }
    let dir = std::env::temp_dir().join("plume-update");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("Plume-Setup-{}.exe", fmt(rel.version)));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.5"), Some((0, 5, 0)));
        assert_eq!(parse_version("2.0.1-beta+7"), Some((2, 0, 1)));
        assert_eq!(parse_version("nope"), None);
        assert!((0, 10, 0) > (0, 9, 9));
        assert!(current() >= (0, 5, 0));
    }

    #[test]
    fn release_json_and_checksum() {
        let json = r#"{"tag_name":"v9.1.0","html_url":"https://github.com/x/y/releases/tag/v9.1.0",
            "assets":[{"name":"Plume-Setup.exe","browser_download_url":"https://dl/setup.exe"},
                      {"name":"Plume-Setup.exe.sha256","browser_download_url":"https://dl/setup.sha256"}]}"#;
        let r = parse_release(json).unwrap();
        assert_eq!(r.version, (9, 1, 0));
        assert_eq!(r.installer_url.as_deref(), Some("https://dl/setup.exe"));
        assert_eq!(r.checksum_url.as_deref(), Some("https://dl/setup.sha256"));
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
