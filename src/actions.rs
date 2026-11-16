//! Click actions.
//!
//! ```text
//! https://example.com           open a URL, file, folder or program
//! "C:\app.exe" --some-arg       program with arguments (quote the path)
//! !reload  !exit                reload widgets / quit Plume
//! !hide  !show  !toggle         all widgets
//! !toggle:clock                 show/hide one widget by name
//! !refresh                      update this widget now
//! !set:name=expression          set a variable (e.g. !set:page=page+1)
//! !lua:function                 call a function of the widget's Lua script
//! !media:playpause|next|prev|stop
//! !volume:up|down|mute
//! ```

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Open { target: String, args: Option<String> },
    Reload,
    Exit,
    Hide,
    Show,
    Toggle,
    ToggleWidget(String),
    Refresh,
    Set(String, String),
    Lua(String),
    Key(MediaKey),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MediaKey {
    PlayPause,
    Next,
    Prev,
    Stop,
    VolumeUp,
    VolumeDown,
    Mute,
}

pub fn parse(s: &str) -> Result<Action, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty action".into());
    }
    if let Some(cmd) = s.strip_prefix('!') {
        let (name, arg) = cmd.split_once(':').map_or((cmd, ""), |(a, b)| (a, b.trim()));
        return Ok(match (name.trim().to_lowercase().as_str(), arg) {
            ("reload", _) => Action::Reload,
            ("exit" | "quit", _) => Action::Exit,
            ("hide", _) => Action::Hide,
            ("show", _) => Action::Show,
            ("toggle", "") => Action::Toggle,
            ("toggle", w) => Action::ToggleWidget(w.to_string()),
            ("refresh", _) => Action::Refresh,
            ("lua", f) if !f.is_empty() => Action::Lua(f.to_string()),
            ("set", a) => {
                let (k, v) = a.split_once('=').ok_or("use !set:name=expression")?;
                Action::Set(k.trim().to_string(), v.trim().to_string())
            }
            ("media", m) => Action::Key(match m {
                "playpause" | "play" | "pause" => MediaKey::PlayPause,
                "next" => MediaKey::Next,
                "prev" | "previous" => MediaKey::Prev,
                "stop" => MediaKey::Stop,
                _ => return Err(format!("unknown media command {m:?}")),
            }),
            ("volume", v) => Action::Key(match v {
                "up" => MediaKey::VolumeUp,
                "down" => MediaKey::VolumeDown,
                "mute" => MediaKey::Mute,
                _ => return Err(format!("unknown volume command {v:?}")),
            }),
            _ => return Err(format!("unknown action {s:?}")),
        });
    }
    if let Some(rest) = s.strip_prefix('"') {
        let (target, args) = rest.split_once('"').ok_or("missing closing quote")?;
        let args = args.trim();
        return Ok(Action::Open {
            target: target.to_string(),
            args: (!args.is_empty()).then(|| args.to_string()),
        });
    }
    Ok(Action::Open { target: s.to_string(), args: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing() {
        assert_eq!(parse("!reload").unwrap(), Action::Reload);
        assert_eq!(parse("!toggle:clock").unwrap(), Action::ToggleWidget("clock".into()));
        assert_eq!(parse("!set:page = page + 1").unwrap(), Action::Set("page".into(), "page + 1".into()));
        assert_eq!(parse("!media:next").unwrap(), Action::Key(MediaKey::Next));
        assert_eq!(parse("!lua:refresh_all").unwrap(), Action::Lua("refresh_all".into()));
        assert_eq!(
            parse(r#""C:\Program Files\App\app.exe" --x 1"#).unwrap(),
            Action::Open { target: r"C:\Program Files\App\app.exe".into(), args: Some("--x 1".into()) }
        );
        assert_eq!(parse("https://a.b").unwrap(), Action::Open { target: "https://a.b".into(), args: None });
        assert!(parse("!nope").is_err());
        assert!(parse("!media:dance").is_err());
    }
}
