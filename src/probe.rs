//! Windows-only sensors: battery, GPU usage/memory (PDH + DXGI) and NVIDIA GPU
//! temperature (NVML, loaded dynamically if the driver is installed).

#[derive(Clone, Copy, Debug)]
pub struct Battery {
    pub percent: f32,
    pub charging: bool,
    pub plugged: bool,
    pub seconds_left: Option<u32>,
}

#[derive(Default)]
pub struct Probe {
    pub battery: Option<Battery>,
    pub gpu: Option<f32>,
    pub gpu_mem_used: Option<u64>,
    pub gpu_mem_total: Option<u64>,
    pub gpu_temp: Option<f32>,
    #[cfg(windows)]
    imp: imp::State,
}

#[cfg(not(windows))]
impl Probe {
    pub fn refresh(&mut self, _wants: u32) {}
}

#[cfg(windows)]
impl Probe {
    pub fn refresh(&mut self, wants: u32) {
        use crate::metrics::cat;
        if wants & cat::BATTERY != 0 {
            self.battery = imp::battery();
        }
        if wants & cat::GPU != 0 {
            let (util, mem) = self.imp.gpu();
            self.gpu = util;
            self.gpu_mem_used = mem;
            if self.gpu_mem_total.is_none() {
                self.gpu_mem_total = imp::vram_total();
            }
        }
        if wants & cat::GPU_TEMP != 0 {
            self.gpu_temp = self.imp.gpu_temp();
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::Battery;
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows_sys::Win32::System::Performance::*;
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn battery() -> Option<Battery> {
        let mut s: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
        if unsafe { GetSystemPowerStatus(&mut s) } == 0 || s.BatteryFlag & 128 != 0 || s.BatteryLifePercent == 255 {
            return None;
        }
        Some(Battery {
            percent: s.BatteryLifePercent as f32,
            charging: s.BatteryFlag & 8 != 0,
            plugged: s.ACLineStatus == 1,
            seconds_left: (s.BatteryLifeTime != u32::MAX).then_some(s.BatteryLifeTime),
        })
    }

    pub fn vram_total() -> Option<u64> {
        use windows::Win32::Graphics::Dxgi::*;
        unsafe {
            let f: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
            let mut best = 0u64;
            let mut i = 0;
            while let Ok(a) = f.EnumAdapters1(i) {
                let mut d = DXGI_ADAPTER_DESC1::default();
                if a.GetDesc1(&mut d).is_ok() && d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0 {
                    best = best.max(d.DedicatedVideoMemory as u64);
                }
                i += 1;
            }
            (best > 0).then_some(best)
        }
    }

    type NvmlInit = unsafe extern "C" fn() -> i32;
    type NvmlHandle = unsafe extern "C" fn(u32, *mut *mut c_void) -> i32;
    type NvmlTemp = unsafe extern "C" fn(*mut c_void, u32, *mut u32) -> i32;

    #[derive(Default)]
    pub struct State {
        pdh: Option<Pdh>,
        pdh_failed: bool,
        nvml: Option<(NvmlTemp, *mut c_void)>,
        nvml_failed: bool,
    }

    struct Pdh {
        query: isize,
        util: isize,
        mem: isize,
    }

    impl State {
        /// (3D engine utilization %, dedicated memory in use)
        pub fn gpu(&mut self) -> (Option<f32>, Option<u64>) {
            if self.pdh.is_none() && !self.pdh_failed {
                self.pdh = Pdh::open();
                self.pdh_failed = self.pdh.is_none();
            }
            let Some(p) = &self.pdh else { return (None, None) };
            unsafe { PdhCollectQueryData(p.query) };
            let util = sum(p.util).map(|v| v.clamp(0.0, 100.0) as f32);
            let mem = sum(p.mem).map(|v| v as u64);
            (util, mem)
        }

        pub fn gpu_temp(&mut self) -> Option<f32> {
            if self.nvml.is_none() && !self.nvml_failed {
                self.nvml = unsafe { load_nvml() };
                self.nvml_failed = self.nvml.is_none();
            }
            let (get_temp, dev) = self.nvml?;
            let mut t = 0u32;
            (unsafe { get_temp(dev, 0, &mut t) } == 0).then_some(t as f32)
        }
    }

    impl Pdh {
        fn open() -> Option<Pdh> {
            unsafe {
                let mut query = std::mem::zeroed();
                if PdhOpenQueryW(null(), 0, &mut query) != 0 {
                    return None;
                }
                let add = |path: &str| {
                    let w = wide(path);
                    let mut c = std::mem::zeroed();
                    (PdhAddEnglishCounterW(query, w.as_ptr(), 0, &mut c) == 0).then_some(c)
                };
                let util = add(r"\GPU Engine(*engtype_3D)\Utilization Percentage");
                let mem = add(r"\GPU Adapter Memory(*)\Dedicated Usage");
                match (util, mem) {
                    (Some(util), Some(mem)) => {
                        PdhCollectQueryData(query);
                        Some(Pdh { query, util, mem })
                    }
                    _ => {
                        PdhCloseQuery(query);
                        None
                    }
                }
            }
        }
    }

    /// Sum of all instances of a wildcard counter.
    fn sum(counter: isize) -> Option<f64> {
        unsafe {
            let (mut size, mut count) = (0u32, 0u32);
            let r = PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, null_mut());
            if r != PDH_MORE_DATA || size == 0 {
                return None;
            }
            let items = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            let mut buf: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> = Vec::with_capacity(size as usize / items + 1);
            let r = PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, buf.as_mut_ptr());
            if r != 0 {
                return None;
            }
            buf.set_len(count as usize);
            Some(
                buf.iter()
                    .filter(|i| i.FmtValue.CStatus == 0 || i.FmtValue.CStatus == 1)
                    .map(|i| i.FmtValue.Anonymous.doubleValue)
                    .sum(),
            )
        }
    }

    unsafe fn load_nvml() -> Option<(NvmlTemp, *mut c_void)> {
        let mut lib = LoadLibraryW(wide("nvml.dll").as_ptr());
        if lib.is_null() {
            lib = LoadLibraryW(wide(r"C:\Program Files\NVIDIA Corporation\NVSMI\nvml.dll").as_ptr());
        }
        if lib.is_null() {
            return None;
        }
        let init: NvmlInit = std::mem::transmute(GetProcAddress(lib, c"nvmlInit_v2".as_ptr().cast())?);
        let handle: NvmlHandle = std::mem::transmute(GetProcAddress(lib, c"nvmlDeviceGetHandleByIndex_v2".as_ptr().cast())?);
        let temp: NvmlTemp = std::mem::transmute(GetProcAddress(lib, c"nvmlDeviceGetTemperature".as_ptr().cast())?);
        if init() != 0 {
            return None;
        }
        let mut dev = null_mut();
        (handle(0, &mut dev) == 0).then_some((temp, dev))
    }
}
