//! System data (CPU, RAM, network, disks, temperatures, GPU, battery, media,
//! audio, time). Only the categories some widget actually uses are measured.

use crate::expr::Value;
use crate::probe::{Battery, Probe};
use crate::{audio::Audio, media::Media};
use std::cell::Cell;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use sysinfo::{Components, Disks, Networks, System};

const GB: f64 = 1024.0 * 1024.0 * 1024.0;

pub mod cat {
    pub const CPU: u32 = 1;
    pub const MEM: u32 = 2;
    pub const NET: u32 = 4;
    pub const DISK: u32 = 8;
    pub const TEMP: u32 = 16;
    pub const GPU: u32 = 32;
    pub const GPU_TEMP: u32 = 64;
    pub const BATTERY: u32 = 128;
    pub const MEDIA: u32 = 256;
    pub const AUDIO: u32 = 512;
}

/// Which category a variable/function name belongs to.
pub fn category(name: &str) -> u32 {
    match name {
        "cpu" => cat::CPU,
        "ram" | "ram_used" | "ram_total" | "ram_free" => cat::MEM,
        "cpu_temp" => cat::TEMP,
        "gpu" | "gpu_mem" | "gpu_mem_used" | "gpu_mem_total" => cat::GPU,
        "gpu_temp" => cat::GPU_TEMP,
        "has_battery" => cat::BATTERY,
        "audio_level" => cat::AUDIO,
        n if n.starts_with("net_") => cat::NET,
        n if n.starts_with("disk") => cat::DISK,
        n if n.starts_with("battery") => cat::BATTERY,
        n if n.starts_with("media_") => cat::MEDIA,
        _ => 0,
    }
}

/// Latest measurements, written by the sampler thread.
#[derive(Clone)]
struct Snapshot {
    cpu: f32,
    ram_used: u64,
    ram_total: u64,
    net_down: f64,
    net_up: f64,
    cpu_temp: Option<f32>,
    /// (mount point in lowercase without trailing slash, total, available)
    disks: Vec<(String, u64, u64)>,
    battery: Option<Battery>,
    gpu: Option<f32>,
    gpu_mem_used: Option<u64>,
    gpu_mem_total: Option<u64>,
    gpu_temp: Option<f32>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Snapshot {
            cpu: 0.0,
            ram_used: 0,
            ram_total: 1,
            net_down: 0.0,
            net_up: 0.0,
            cpu_temp: None,
            disks: Vec::new(),
            battery: None,
            gpu: None,
            gpu_mem_used: None,
            gpu_mem_total: None,
            gpu_temp: None,
        }
    }
}

#[derive(Default)]
struct Shared {
    snap: Mutex<Snapshot>,
    version: AtomicU64,
    wants: AtomicU32,
}

/// System data. The slow part (reading sensors, WMI, PDH...) runs in a
/// background thread, so drawing never waits for it.
pub struct Metrics {
    shared: Arc<Shared>,
    snap: Snapshot,
    seen_version: u64,
    started: bool,
    pub media: Media,
    pub audio: Audio,
    /// Categories used by the widgets.
    wants: Cell<u32>,
}

impl Metrics {
    pub fn new() -> Self {
        Metrics {
            shared: Arc::default(),
            snap: Snapshot::default(),
            seen_version: 0,
            started: false,
            media: Media::default(),
            audio: Audio::default(),
            wants: Cell::new(0),
        }
    }

    /// Forgets what was requested (after a reload).
    pub fn reset_wants(&self) {
        self.wants.set(0);
        self.shared.wants.store(0, Ordering::Relaxed);
    }

    /// Marks a name as used (so its category gets measured).
    pub fn mark(&self, name: &str) {
        self.wants.set(self.wants.get() | category(name));
    }

    /// Takes the latest measurements (cheap: never waits for the sensors).
    pub fn refresh(&mut self) {
        let w = self.wants.get();
        self.shared.wants.fetch_or(w, Ordering::Relaxed);
        if !self.started {
            self.started = true;
            let shared = self.shared.clone();
            std::thread::spawn(move || sampler(shared));
        }
        let v = self.shared.version.load(Ordering::Acquire);
        if v != self.seen_version {
            self.seen_version = v;
            self.snap = self.shared.snap.lock().unwrap().clone();
        }
        if w & cat::MEDIA != 0 {
            self.media.ensure_started();
        }
        if w & cat::AUDIO != 0 {
            self.audio.ensure_started();
        }
    }

    pub fn ram_percent(&self) -> f32 {
        self.snap.ram_used as f32 * 100.0 / self.snap.ram_total as f32
    }

    pub fn var(&self, name: &str) -> Option<Value> {
        self.mark(name);
        let num = |v: f64, d: u8| Some(Value::Num(v, d));
        let opt = |v: Option<f32>| Some(v.map_or(Value::Null, |v| Value::Num(v as f64, 0)));
        let now = chrono::Local::now();
        use chrono::{Datelike, Timelike};
        match name {
            "cpu" => num(self.snap.cpu as f64, 0),
            "ram" => num(self.ram_percent() as f64, 0),
            "ram_used" => num(self.snap.ram_used as f64 / GB, 1),
            "ram_total" => num(self.snap.ram_total as f64 / GB, 1),
            "ram_free" => num(self.snap.ram_total.saturating_sub(self.snap.ram_used) as f64 / GB, 1),
            "net_down" => Some(Value::Str(rate(self.snap.net_down))),
            "net_up" => Some(Value::Str(rate(self.snap.net_up))),
            "net_down_bytes" => num(self.snap.net_down, 0),
            "net_up_bytes" => num(self.snap.net_up, 0),
            "disk" | "disk_used" | "disk_free" | "disk_total" => self.call(name, &[]),
            "cpu_temp" => opt(self.snap.cpu_temp),
            "gpu" => opt(self.snap.gpu),
            "gpu_mem_used" => Some(self.snap.gpu_mem_used.map_or(Value::Null, |b| Value::Num(b as f64 / GB, 1))),
            "gpu_mem_total" => Some(self.snap.gpu_mem_total.map_or(Value::Null, |b| Value::Num(b as f64 / GB, 1))),
            "gpu_mem" => Some(match (self.snap.gpu_mem_used, self.snap.gpu_mem_total) {
                (Some(u), Some(t)) if t > 0 => Value::Num(u as f64 * 100.0 / t as f64, 0),
                _ => Value::Null,
            }),
            "gpu_temp" => opt(self.snap.gpu_temp),
            "has_battery" => Some(Value::bool(self.snap.battery.is_some())),
            "battery" => opt(self.snap.battery.map(|b| b.percent)),
            "battery_charging" => Some(Value::bool(self.snap.battery.is_some_and(|b| b.charging))),
            "battery_plugged" => Some(Value::bool(self.snap.battery.is_some_and(|b| b.plugged))),
            "battery_time" => Some(Value::Str(
                self.snap.battery.and_then(|b| b.seconds_left).map_or(String::new(), |s| {
                    format!("{}h {:02}m", s / 3600, (s % 3600) / 60)
                }),
            )),
            "uptime" => {
                let s = System::uptime();
                Some(Value::Str(format!("{}h {:02}m", s / 3600, (s % 3600) / 60)))
            }
            "uptime_s" => num(System::uptime() as f64, 0),
            "hour" => num(now.hour() as f64, 0),
            "hour12" => num(((now.hour() + 11) % 12 + 1) as f64, 0),
            "minute" => num(now.minute() as f64, 0),
            "second" => num(now.second() as f64, 0),
            "day" => num(now.day() as f64, 0),
            "month" => num(now.month() as f64, 0),
            "year" => num(now.year() as f64, 0),
            "weekday" => num(now.weekday().num_days_from_sunday() as f64, 0),
            "audio_level" => num(self.audio.level() as f64, 0),
            n if n.starts_with("media_") => self.media_var(n),
            _ => None,
        }
    }

    fn media_var(&self, name: &str) -> Option<Value> {
        let m = self.media.info();
        Some(match name {
            "media_title" => Value::Str(m.title),
            "media_artist" => Value::Str(m.artist),
            "media_album" => Value::Str(m.album),
            "media_playing" => Value::bool(m.playing),
            "media_active" => Value::bool(m.active),
            "media_status" => Value::Str(
                if !m.active { "stopped" } else if m.playing { "playing" } else { "paused" }.into(),
            ),
            "media_position" => Value::Num(m.position, 0),
            "media_duration" => Value::Num(m.duration, 0),
            "media_progress" => Value::Num(
                if m.duration > 0.0 { (m.position / m.duration * 100.0).clamp(0.0, 100.0) } else { 0.0 },
                0,
            ),
            "media_cover" => Value::Str(if m.cover.is_some() { "media:cover".into() } else { String::new() }),
            _ => return None,
        })
    }

    /// Functions that read the system: `time(fmt)`, `disk(drive)`, ...
    pub fn call(&self, name: &str, args: &[Value]) -> Option<Value> {
        self.mark(name);
        let arg = |i: usize| args.get(i).map(|v| v.to_string()).unwrap_or_default();
        match name {
            "time" | "date" => {
                let fmt = arg(0);
                let fmt = if fmt.is_empty() { "%H:%M" } else { fmt.as_str() };
                let mut out = String::new();
                use std::fmt::Write;
                // Invalid formats must not panic.
                if write!(out, "{}", chrono::Local::now().format(fmt)).is_err() {
                    out = "?".into();
                }
                Some(Value::Str(out))
            }
            "disk" | "disk_used" | "disk_free" | "disk_total" => {
                let drive = arg(0);
                let d = self.find_disk(if drive.is_empty() { DEFAULT_DRIVE } else { &drive });
                let Some((total, free)) = d else { return Some(Value::Null) };
                Some(match name {
                    "disk" => Value::Num((total - free) as f64 * 100.0 / total.max(1) as f64, 0),
                    "disk_used" => Value::Num((total - free) as f64 / GB, 1),
                    "disk_free" => Value::Num(free as f64 / GB, 1),
                    _ => Value::Num(total as f64 / GB, 1),
                })
            }
            _ => None,
        }
    }

    /// (total, available) bytes of a drive: "C", "C:", "C:\" or a mount point.
    fn find_disk(&self, drive: &str) -> Option<(u64, u64)> {
        let want = drive.trim_end_matches(['\\', '/']).to_lowercase();
        let want = if want.len() == 1 { format!("{want}:") } else { want };
        self.snap
            .disks
            .iter()
            .find(|(mp, _, _)| *mp == want || (mp.is_empty() && want == "/") || (want.is_empty() && mp.is_empty()))
            .map(|(_, t, a)| (*t, *a))
    }
}

/// Background thread: measures what the widgets use, once per second.
fn sampler(shared: Arc<Shared>) {
    let mut sys = System::new();
    let mut nets: Option<Networks> = None;
    let mut disks: Option<Disks> = None;
    let mut comps: Option<Components> = None;
    let mut probe = Probe::default();
    let mut snap = Snapshot::default();
    let mut last = Instant::now();
    sys.refresh_memory();
    loop {
        let started = Instant::now();
        let elapsed = last.elapsed().as_secs_f64().max(0.1);
        last = started;
        let w = shared.wants.load(Ordering::Relaxed);

        if w & cat::CPU != 0 {
            sys.refresh_cpu_usage();
            snap.cpu = sys.global_cpu_usage();
        }
        sys.refresh_memory();
        snap.ram_used = sys.used_memory();
        snap.ram_total = sys.total_memory().max(1);
        if w & cat::NET != 0 {
            match &mut nets {
                Some(n) => {
                    n.refresh(true);
                    let (rx, tx) =
                        n.values().fold((0u64, 0u64), |(r, t), d| (r + d.received(), t + d.transmitted()));
                    snap.net_down = rx as f64 / elapsed;
                    snap.net_up = tx as f64 / elapsed;
                }
                None => nets = Some(Networks::new_with_refreshed_list()),
            }
        }
        if w & cat::DISK != 0 {
            let d = disks.get_or_insert_with(Disks::new_with_refreshed_list);
            d.refresh(true);
            snap.disks = d
                .list()
                .iter()
                .map(|d| {
                    let mp = d.mount_point().to_string_lossy().to_lowercase();
                    let mp = mp.trim_end_matches(['\\', '/']).to_string();
                    (mp, d.total_space(), d.available_space())
                })
                .collect();
        }
        if w & cat::TEMP != 0 {
            let c = comps.get_or_insert_with(Components::new_with_refreshed_list);
            c.refresh(true);
            snap.cpu_temp = cpu_temperature(c);
        }
        probe.refresh(w);
        snap.battery = probe.battery;
        snap.gpu = probe.gpu;
        snap.gpu_mem_used = probe.gpu_mem_used;
        snap.gpu_mem_total = probe.gpu_mem_total;
        snap.gpu_temp = probe.gpu_temp;

        *shared.snap.lock().unwrap() = snap.clone();
        shared.version.fetch_add(1, Ordering::Release);
        // Faster first rounds so values show up quickly after start.
        let period = if shared.version.load(Ordering::Relaxed) < 3 { 300 } else { 1000 };
        std::thread::sleep(Duration::from_millis(period).saturating_sub(started.elapsed()));
    }
}

#[cfg(windows)]
const DEFAULT_DRIVE: &str = "C:";
#[cfg(not(windows))]
const DEFAULT_DRIVE: &str = "/";

fn cpu_temperature(comps: &Components) -> Option<f32> {
    let temps: Vec<(String, f32)> = comps
        .iter()
        .filter_map(|c| Some((c.label().to_lowercase(), c.temperature()?)))
        .filter(|(_, t)| t.is_finite() && *t > 0.0)
        .collect();
    let cpu_like = ["package", "tctl", "tdie", "cpu", "core", "k10temp", "coretemp"];
    temps
        .iter()
        .filter(|(l, _)| cpu_like.iter().any(|k| l.contains(k)))
        .map(|(_, t)| *t)
        .reduce(f32::max)
        .or_else(|| temps.iter().map(|(_, t)| *t).reduce(f32::max))
}

fn rate(bps: f64) -> String {
    format!("{}/s", crate::expr::fmt_bytes(bps))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vars_and_wants() {
        let m = Metrics::new();
        assert!(m.var("cpu").is_some());
        assert!(m.var("hour").is_some());
        assert!(m.var("nope").is_none());
        assert_eq!(m.wants.get() & cat::CPU, cat::CPU);
        assert!(matches!(m.call("time", &[Value::Str("%Y".into())]), Some(Value::Str(s)) if s.len() == 4));
        assert!(matches!(m.call("time", &[Value::Str("%Q%".into())]), Some(Value::Str(_))));
    }
}
