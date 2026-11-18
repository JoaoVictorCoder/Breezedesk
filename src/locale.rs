//! Language for day and month names: the Windows display language (or
//! `LANG` elsewhere), e.g. "pt-BR" → `pt_BR`. Falls back to English.

use chrono::Locale;
use std::sync::OnceLock;

fn parse(name: &str) -> Option<Locale> {
    // "pt-BR", "pt_BR.UTF-8", "pt" ...
    let base = name.split(['.', '@']).next()?.replace('-', "_");
    if base.is_empty() {
        return None;
    }
    Locale::try_from(base.as_str()).ok().or_else(|| {
        // Language only ("pt") → first locale of that language.
        let lang = base.split('_').next()?;
        let guesses = [format!("{lang}_{}", lang.to_uppercase())];
        guesses.iter().find_map(|g| Locale::try_from(g.as_str()).ok())
    })
}

#[cfg(windows)]
fn system_name() -> Option<String> {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
    let mut buf = [0u16; 85];
    let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    (n > 1).then(|| String::from_utf16_lossy(&buf[..n as usize - 1]))
}

#[cfg(not(windows))]
fn system_name() -> Option<String> {
    ["LC_ALL", "LC_TIME", "LANG"].iter().find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty() && s != "C"))
}

/// The user's locale (computed once).
pub fn system() -> Locale {
    static L: OnceLock<Locale> = OnceLock::new();
    *L.get_or_init(|| system_name().and_then(|n| parse(&n)).unwrap_or(Locale::en_US))
}

/// A locale by name ("pt_BR", "de-DE"...), or the user's locale.
pub fn get(name: &str) -> Locale {
    if name.is_empty() { system() } else { parse(name).unwrap_or_else(system) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(parse("pt-BR"), Some(Locale::pt_BR));
        assert_eq!(parse("de_DE.UTF-8"), Some(Locale::de_DE));
        assert_eq!(parse("fr"), Some(Locale::fr_FR));
        assert_eq!(parse(""), None);
        let d = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
        assert_eq!(d.format_localized("%A, %B", Locale::pt_BR).to_string(), "segunda, novembro");
    }
}
