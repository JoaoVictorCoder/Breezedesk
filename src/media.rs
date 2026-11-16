//! "Now playing" from any app that talks to Windows media controls
//! (Spotify, browsers, media players...). Runs in a background thread that is
//! only started when a widget uses a `media_*` variable.

use std::sync::{Arc, Mutex};
use std::time::Instant;
use tiny_skia::Pixmap;

#[derive(Clone, Default)]
pub struct MediaInfo {
    pub active: bool,
    pub playing: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Seconds.
    pub position: f64,
    pub duration: f64,
    pub cover: Option<Arc<Pixmap>>,
    /// Changes every time the cover changes (used as a cache key).
    pub cover_id: u64,
}

#[derive(Default)]
struct Shared {
    info: MediaInfo,
    sampled: Option<Instant>,
}

#[derive(Default)]
pub struct Media {
    shared: Arc<Mutex<Shared>>,
    started: bool,
}

impl Media {
    pub fn ensure_started(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        #[cfg(windows)]
        {
            let shared = self.shared.clone();
            std::thread::spawn(move || imp::run(shared));
        }
    }

    /// Snapshot, with the position extrapolated while playing.
    pub fn info(&self) -> MediaInfo {
        let s = self.shared.lock().unwrap();
        let mut info = s.info.clone();
        if info.playing {
            if let Some(t) = s.sampled {
                info.position += t.elapsed().as_secs_f64();
            }
        }
        if info.duration > 0.0 {
            info.position = info.position.min(info.duration);
        }
        info
    }
}

pub fn decode_image(bytes: &[u8]) -> Option<Pixmap> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let mut pm = Pixmap::new(w, h)?;
    for (dst, src) in pm.pixels_mut().iter_mut().zip(img.pixels()) {
        let [r, g, b, a] = src.0;
        *dst = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
    }
    Some(pm)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };
    use windows::Storage::Streams::DataReader;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    pub fn run(shared: Arc<Mutex<Shared>>) {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let mut last_track = String::new();
        let mut cover_id = 0u64;
        loop {
            let manager = Manager::RequestAsync().and_then(|op| op.get());
            let Ok(manager) = manager else {
                crate::log::log("media: Windows media controls are not available");
                std::thread::sleep(Duration::from_secs(30));
                continue;
            };
            loop {
                match manager.GetCurrentSession() {
                    Ok(session) => {
                        if let Err(e) = poll(&session, &shared, &mut last_track, &mut cover_id) {
                            crate::log::log(format!("media: {e}"));
                        }
                    }
                    Err(_) => {
                        let mut s = shared.lock().unwrap();
                        s.info = MediaInfo::default();
                        last_track.clear();
                    }
                }
                std::thread::sleep(Duration::from_millis(1000));
            }
        }
    }

    fn poll(
        session: &Session,
        shared: &Arc<Mutex<Shared>>,
        last_track: &mut String,
        cover_id: &mut u64,
    ) -> windows::core::Result<()> {
        let props = session.TryGetMediaPropertiesAsync()?.get()?;
        let title = props.Title()?.to_string_lossy();
        let artist = props.Artist()?.to_string_lossy();
        let album = props.AlbumTitle()?.to_string_lossy();
        let playing = session.GetPlaybackInfo()?.PlaybackStatus()? == Status::Playing;

        let tl = session.GetTimelineProperties()?;
        let ticks = |t: i64| t as f64 / 10_000_000.0;
        let start = ticks(tl.StartTime()?.Duration);
        let end = ticks(tl.EndTime()?.Duration);
        let mut position = ticks(tl.Position()?.Duration) - start;
        if playing {
            // Apps update the position only now and then: extrapolate from that moment.
            let updated = tl.LastUpdatedTime()?.UniversalTime;
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as i64 / 100
                + 116_444_736_000_000_000;
            if updated > 0 && now > updated {
                position += ticks(now - updated);
            }
        }

        let track = format!("{title}\u{1}{artist}\u{1}{album}");
        let mut cover = None;
        let changed = track != *last_track;
        if changed {
            *last_track = track;
            *cover_id += 1;
            cover = read_cover(&props).ok().flatten().map(Arc::new);
        }

        let mut s = shared.lock().unwrap();
        s.info.active = true;
        s.info.playing = playing;
        s.info.title = title;
        s.info.artist = artist;
        s.info.album = album;
        s.info.position = position.max(0.0);
        s.info.duration = (end - start).max(0.0);
        if changed {
            s.info.cover = cover;
            s.info.cover_id = *cover_id;
        }
        s.sampled = Some(Instant::now());
        Ok(())
    }

    fn read_cover(
        props: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties,
    ) -> windows::core::Result<Option<Pixmap>> {
        let Ok(thumb) = props.Thumbnail() else { return Ok(None) };
        let stream = thumb.OpenReadAsync()?.get()?;
        let size = stream.Size()? as u32;
        if size == 0 {
            return Ok(None);
        }
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
        reader.LoadAsync(size)?.get()?;
        let mut buf = vec![0u8; size as usize];
        reader.ReadBytes(&mut buf)?;
        Ok(decode_image(&buf))
    }
}
