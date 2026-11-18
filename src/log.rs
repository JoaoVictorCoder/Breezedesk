//! Tiny log file (`breezedesk.log` in the widgets folder). Repeated messages are written once.

use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

struct State {
    path: Option<PathBuf>,
    seen: HashSet<String>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

pub fn init(path: PathBuf) {
    let _ = std::fs::write(&path, "");
    *STATE.lock().unwrap() = Some(State { path: Some(path), seen: HashSet::new() });
}

pub fn path() -> Option<PathBuf> {
    STATE.lock().ok()?.as_ref()?.path.clone()
}

/// Forget repeated messages (called on reload so errors show again).
pub fn reset_seen() {
    if let Some(s) = STATE.lock().unwrap().as_mut() {
        s.seen.clear();
    }
}

pub fn log(msg: impl AsRef<str>) {
    let msg = msg.as_ref();
    #[cfg(not(windows))]
    eprintln!("{msg}");
    let mut guard = STATE.lock().unwrap();
    let Some(state) = guard.as_mut() else { return };
    if state.seen.len() > 1000 || !state.seen.insert(msg.to_string()) {
        return;
    }
    if let Some(p) = &state.path {
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open(p) {
            let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let _ = writeln!(f, "[{now}] {msg}");
        }
    }
}
