//! Sensors from LibreHardwareMonitor (or OpenHardwareMonitor), when it is
//! running. It reads the hardware directly (it needs admin rights once, at its
//! own start), so it gives CPU temperatures on PCs where Windows exposes none,
//! AMD GPU temperatures, fan speeds, voltages, power...
//!
//! It publishes every sensor through WMI (`root\LibreHardwareMonitor`).

/// (name, identifier like "/amdcpu/0/temperature/2", value)
pub type Sensor = (String, String, f32);

#[derive(Default)]
pub struct Lhm {
    #[cfg(windows)]
    imp: imp::State,
}

impl Lhm {
    /// Current sensor values (empty when no monitor is running).
    pub fn read(&mut self) -> Vec<Sensor> {
        #[cfg(windows)]
        {
            self.imp.read()
        }
        #[cfg(not(windows))]
        {
            Vec::new()
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::Sensor;
    use std::time::{Duration, Instant};
    use windows::core::{BSTR, VARIANT};
    use windows::Win32::System::Com::*;
    use windows::Win32::System::Wmi::*;

    const NAMESPACES: [&str; 2] = [r"ROOT\LibreHardwareMonitor", r"ROOT\OpenHardwareMonitor"];

    #[derive(Default)]
    pub struct State {
        svc: Option<IWbemServices>,
        com_ready: bool,
        last_try: Option<Instant>,
    }

    impl State {
        pub fn read(&mut self) -> Vec<Sensor> {
            if !self.com_ready {
                unsafe {
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                }
                self.com_ready = true;
            }
            if self.svc.is_none() {
                // Not running: try again every 30 s.
                if self.last_try.is_some_and(|t| t.elapsed() < Duration::from_secs(30)) {
                    return Vec::new();
                }
                self.last_try = Some(Instant::now());
                self.svc = connect();
            }
            let Some(svc) = &self.svc else { return Vec::new() };
            match query(svc) {
                Ok(list) => list,
                Err(_) => {
                    self.svc = None; // closed meanwhile
                    Vec::new()
                }
            }
        }
    }

    fn connect() -> Option<IWbemServices> {
        unsafe {
            let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).ok()?;
            for ns in NAMESPACES {
                let Ok(svc) = locator.ConnectServer(&BSTR::from(ns), &BSTR::new(), &BSTR::new(), &BSTR::new(), 0, &BSTR::new(), None)
                else {
                    continue;
                };
                // The namespace exists only while the monitor runs, but stays
                // registered afterwards: check it really has sensors.
                if query(&svc).is_ok_and(|l| !l.is_empty()) {
                    return Some(svc);
                }
            }
            None
        }
    }

    fn query(svc: &IWbemServices) -> windows::core::Result<Vec<Sensor>> {
        unsafe {
            let rows = svc.ExecQuery(
                &BSTR::from("WQL"),
                &BSTR::from("SELECT Name, Identifier, Value FROM Sensor"),
                WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                None,
            )?;
            let mut out = Vec::new();
            loop {
                let mut row = [None; 1];
                let mut got = 0u32;
                let _ = rows.Next(WBEM_INFINITE, &mut row, &mut got);
                let Some(obj) = row[0].take().filter(|_| got == 1) else { break };
                let get = |name: &str| -> Option<VARIANT> {
                    let mut v = VARIANT::default();
                    obj.Get(&windows::core::HSTRING::from(name), 0, &mut v, None, None).ok()?;
                    Some(v)
                };
                let (Some(n), Some(id), Some(val)) = (get("Name"), get("Identifier"), get("Value")) else { continue };
                let (Ok(n), Ok(id), Ok(val)) = (BSTR::try_from(&n), BSTR::try_from(&id), f64::try_from(&val)) else {
                    continue;
                };
                out.push((n.to_string(), id.to_string(), val as f32));
            }
            Ok(out)
        }
    }
}
