//! System data collection (CPU, RAM, network, time) and variable substitution.

use crate::config::Source;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use sysinfo::{Networks, System};

const HISTORY: usize = 200;
const MIN_INTERVAL: Duration = Duration::from_millis(900);

pub struct Metrics {
    sys: System,
    nets: Networks,
    last: Instant,
    pub cpu: f32,
    pub ram_used: u64,
    pub ram_total: u64,
    pub net_down: f64,
    pub net_up: f64,
    cpu_hist: VecDeque<f32>,
    ram_hist: VecDeque<f32>,
}

impl Metrics {
    pub fn new() -> Self {
        let mut m = Metrics {
            sys: System::new(),
            nets: Networks::new_with_refreshed_list(),
            last: Instant::now(),
            cpu: 0.0,
            ram_used: 0,
            ram_total: 1,
            net_down: 0.0,
            net_up: 0.0,
            cpu_hist: VecDeque::with_capacity(HISTORY),
            ram_hist: VecDeque::with_capacity(HISTORY),
        };
        m.sys.refresh_cpu_usage();
        m.sys.refresh_memory();
        m.ram_used = m.sys.used_memory();
        m.ram_total = m.sys.total_memory().max(1);
        m
    }

    /// Refreshes at most ~once per second, even if several widgets ask.
    pub fn refresh(&mut self) {
        let elapsed = self.last.elapsed();
        if elapsed < MIN_INTERVAL {
            return;
        }
        self.last = Instant::now();

        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.nets.refresh(true);

        self.cpu = self.sys.global_cpu_usage();
        self.ram_used = self.sys.used_memory();
        self.ram_total = self.sys.total_memory().max(1);

        let secs = elapsed.as_secs_f64();
        let (rx, tx) = self
            .nets
            .values()
            .fold((0u64, 0u64), |(r, t), d| (r + d.received(), t + d.transmitted()));
        self.net_down = rx as f64 / secs;
        self.net_up = tx as f64 / secs;

        let ram = self.ram_percent();
        push(&mut self.cpu_hist, self.cpu);
        push(&mut self.ram_hist, ram);
    }

    pub fn ram_percent(&self) -> f32 {
        self.ram_used as f32 * 100.0 / self.ram_total as f32
    }

    pub fn value(&self, s: Source) -> f32 {
        match s {
            Source::Cpu => self.cpu,
            Source::Ram => self.ram_percent(),
        }
    }

    pub fn history(&self, s: Source) -> &VecDeque<f32> {
        match s {
            Source::Cpu => &self.cpu_hist,
            Source::Ram => &self.ram_hist,
        }
    }

    /// Replaces `{variable}` / `{variable:arg}` with the current values.
    pub fn expand(&self, template: &str) -> String {
        let mut out = String::with_capacity(template.len() + 16);
        let mut rest = template;
        while let Some(start) = rest.find('{') {
            out.push_str(&rest[..start]);
            let Some(end) = rest[start..].find('}') else {
                out.push_str(&rest[start..]);
                return out;
            };
            let inner = &rest[start + 1..start + end];
            let (name, arg) = inner.split_once(':').unwrap_or((inner, ""));
            match self.var(name, arg) {
                Some(v) => out.push_str(&v),
                None => out.push_str(&rest[start..=start + end]),
            }
            rest = &rest[start + end + 1..];
        }
        out.push_str(rest);
        out
    }

    fn var(&self, name: &str, arg: &str) -> Option<String> {
        const GB: f64 = 1024.0 * 1024.0 * 1024.0;
        Some(match name {
            "cpu" => format!("{:.0}", self.cpu),
            "ram" => format!("{:.0}", self.ram_percent()),
            "ram_used" => format!("{:.1}", self.ram_used as f64 / GB),
            "ram_total" => format!("{:.1}", self.ram_total as f64 / GB),
            "net_down" => human_rate(self.net_down),
            "net_up" => human_rate(self.net_up),
            "uptime" => {
                let s = System::uptime();
                format!("{}h {:02}m", s / 3600, (s % 3600) / 60)
            }
            "time" => {
                let fmt = if arg.is_empty() { "%H:%M" } else { arg };
                chrono::Local::now().format(fmt).to_string()
            }
            _ => return None,
        })
    }
}

fn push(h: &mut VecDeque<f32>, v: f32) {
    if h.len() == HISTORY {
        h.pop_front();
    }
    h.push_back(v);
}

fn human_rate(bps: f64) -> String {
    if bps >= 1024.0 * 1024.0 {
        format!("{:.1} MB/s", bps / 1024.0 / 1024.0)
    } else {
        format!("{:.0} KB/s", bps / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_vars() {
        let m = Metrics::new();
        let s = m.expand("CPU {cpu}% {unknown} {time:%Y}");
        assert!(s.starts_with("CPU "));
        assert!(s.contains("{unknown}"));
        assert!(!s.contains("{time"));
        assert_eq!(m.expand("no brace {"), "no brace {");
    }
}
