//! Audio visualizer: captures what the speakers are playing (WASAPI loopback)
//! and turns it into a spectrum with an FFT. The capture only runs while a
//! widget is reading it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub const FFT_SIZE: usize = 2048;

#[derive(Default)]
struct Shared {
    /// Smoothed magnitudes of the FFT bins, 0..1.
    mags: Vec<f32>,
    /// RMS level, 0..100.
    level: f32,
    rate: f32,
}

pub struct Audio {
    shared: Arc<Mutex<Shared>>,
    /// Milliseconds since `epoch` of the last read (the thread idles if old).
    last_read: Arc<AtomicU64>,
    epoch: Instant,
    started: bool,
}

impl Default for Audio {
    fn default() -> Self {
        Audio {
            shared: Arc::default(),
            last_read: Arc::new(AtomicU64::new(0)),
            epoch: Instant::now(),
            started: false,
        }
    }
}

impl Audio {
    pub fn ensure_started(&mut self) {
        self.touch();
        if self.started {
            return;
        }
        self.started = true;
        #[cfg(windows)]
        {
            let (shared, last, epoch) = (self.shared.clone(), self.last_read.clone(), self.epoch);
            std::thread::spawn(move || imp::run(shared, last, epoch));
        }
    }

    fn touch(&self) {
        self.last_read.store(self.epoch.elapsed().as_millis() as u64, Ordering::Relaxed);
    }

    pub fn level(&self) -> f32 {
        self.touch();
        self.shared.lock().unwrap().level
    }

    /// `n` bars, log-spaced between 40 Hz and 16 kHz, each 0..1.
    pub fn bands(&self, n: usize) -> Vec<f32> {
        self.touch();
        let s = self.shared.lock().unwrap();
        bands_from(&s.mags, s.rate, n)
    }
}

fn bands_from(mags: &[f32], rate: f32, n: usize) -> Vec<f32> {
    if mags.is_empty() || rate <= 0.0 || n == 0 {
        return vec![0.0; n];
    }
    let (fmin, fmax) = (40.0f32, 16_000.0f32.min(rate / 2.0));
    let bin = |f: f32| ((f / rate * FFT_SIZE as f32) as usize).min(mags.len() - 1);
    (0..n)
        .map(|i| {
            let f0 = fmin * (fmax / fmin).powf(i as f32 / n as f32);
            let f1 = fmin * (fmax / fmin).powf((i + 1) as f32 / n as f32);
            let (a, b) = (bin(f0), bin(f1).max(bin(f0) + 1).min(mags.len()));
            mags[a..b].iter().copied().fold(0.0, f32::max)
        })
        .collect()
}

/// In-place radix-2 FFT (re, im).
pub fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f32::consts::PI / len as f32;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (ang * k as f32).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

/// Turns the latest samples into smoothed 0..1 magnitudes + level.
#[cfg_attr(not(windows), allow(dead_code))]
fn analyze(samples: &[f32], prev: &mut Vec<f32>) -> f32 {
    let n = samples.len();
    let mut re: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(i, s)| s * (0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos()))
        .collect();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    prev.resize(n / 2, 0.0);
    for k in 0..n / 2 {
        let mag = (re[k] * re[k] + im[k] * im[k]).sqrt() / (n as f32 / 4.0);
        let db = 20.0 * mag.max(1e-9).log10();
        let v = ((db + 70.0) / 60.0).clamp(0.0, 1.0);
        // fast attack, slow decay
        prev[k] = if v > prev[k] { v } else { prev[k] * 0.82 + v * 0.18 };
    }
    let rms = (samples.iter().map(|s| s * s).sum::<f32>() / n as f32).sqrt();
    (rms * 2.0 * 100.0).min(100.0)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::time::Duration;
    use windows::Win32::Media::Audio::*;
    use windows::Win32::System::Com::*;

    pub fn run(shared: Arc<Mutex<Shared>>, last_read: Arc<AtomicU64>, epoch: Instant) {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let wanted = || epoch.elapsed().as_millis() as u64 - last_read.load(Ordering::Relaxed) < 3000;
        loop {
            if !wanted() {
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
            if let Err(e) = unsafe { capture(&shared, &wanted) } {
                crate::log::log(format!("audio: {e}"));
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }

    unsafe fn capture(shared: &Arc<Mutex<Shared>>, wanted: &dyn Fn() -> bool) -> windows::core::Result<()> {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
        let fmt = client.GetMixFormat()?;
        let wf = *fmt;
        let channels = wf.nChannels.max(1) as usize;
        let bits = wf.wBitsPerSample;
        let is_float = wf.wFormatTag == 3 || (wf.wFormatTag == 0xFFFE && bits == 32);
        let rate = wf.nSamplesPerSec as f32;
        let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 2_000_000, 0, fmt, None);
        CoTaskMemFree(Some(fmt as *const _));
        init?;
        let cap: IAudioCaptureClient = client.GetService()?;
        client.Start()?;

        let mut ring = vec![0f32; FFT_SIZE];
        let mut pos = 0usize;
        let mut mags = Vec::new();
        let mut window = vec![0f32; FFT_SIZE];
        while wanted() {
            std::thread::sleep(Duration::from_millis(25));
            let mut got = false;
            loop {
                if cap.GetNextPacketSize()? == 0 {
                    break;
                }
                let mut data = std::ptr::null_mut();
                let (mut frames, mut flags) = (0u32, 0u32);
                cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                for f in 0..frames as usize {
                    let mut sum = 0.0;
                    if !silent && !data.is_null() {
                        for c in 0..channels {
                            let i = f * channels + c;
                            sum += if is_float {
                                *(data as *const f32).add(i)
                            } else if bits == 16 {
                                *(data as *const i16).add(i) as f32 / 32768.0
                            } else {
                                0.0
                            };
                        }
                    }
                    ring[pos] = sum / channels as f32;
                    pos = (pos + 1) % FFT_SIZE;
                }
                cap.ReleaseBuffer(frames)?;
                got = true;
            }
            if !got {
                // Nothing is playing: loopback delivers no data, so fade out.
                for _ in 0..(rate as usize / 40).min(FFT_SIZE) {
                    ring[pos] = 0.0;
                    pos = (pos + 1) % FFT_SIZE;
                }
            }
            for (i, w) in window.iter_mut().enumerate() {
                *w = ring[(pos + i) % FFT_SIZE];
            }
            let level = analyze(&window, &mut mags);
            let mut s = shared.lock().unwrap();
            s.mags.clone_from(&mags);
            s.level = level;
            s.rate = rate;
        }
        client.Stop()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_peaks_in_right_band() {
        let rate = 48_000.0;
        let samples: Vec<f32> =
            (0..FFT_SIZE).map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / rate).sin() * 0.5).collect();
        let mut mags = Vec::new();
        let level = analyze(&samples, &mut mags);
        assert!(level > 10.0);
        let peak = mags.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        let freq = peak as f32 * rate / FFT_SIZE as f32;
        assert!((freq - 1000.0).abs() < 50.0, "{freq}");
        let bands = bands_from(&mags, rate, 16);
        assert_eq!(bands.len(), 16);
        assert!(bands.iter().cloned().fold(0.0, f32::max) > 0.8);
    }
}
