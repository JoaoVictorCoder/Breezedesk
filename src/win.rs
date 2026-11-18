//! Windows-specific part: one "layered" window per widget.
//!
//! Each widget is a borderless WS_POPUP + WS_EX_LAYERED window. Its content is
//! drawn into an in-memory buffer and pushed with `UpdateLayeredWindow`, which
//! supports per-pixel transparency: the same trick Rainmeter uses, but without
//! plugins loaded for nothing and without GDI+.

use crate::actions::{self, Action, MediaKey};
use crate::config::{self, Background, Blur, ZOrder};
use crate::metrics::Metrics;
use crate::render::{self, Fonts, Images};
use crate::tray::{self, Tray, WM_APP_TRAY};
use crate::update;
use crate::web::Web;
use crate::widget::{Mouse, Widget};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;
use tiny_skia::Pixmap;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const TIMER_UPDATE: usize = 1;
const TIMER_WATCH: usize = 2;
const TIMER_ANIM: usize = 3;
const WM_APP_RELOAD: u32 = WM_APP + 1;
const WM_APP_ACTIONS: u32 = WM_APP + 3;
const WM_MOUSELEAVE: u32 = 0x02A3;
const MENU_RELOAD: usize = 1;
const MENU_EDIT: usize = 2;
const MENU_EXIT: usize = 3;
const MENU_TOGGLE: usize = 4;
const MENU_AUTOSTART: usize = 5;
const MENU_EXAMPLES: usize = 6;
const MENU_LOG: usize = 7;
const MENU_CHECK_UPDATE: usize = 8;
const MENU_UPDATE: usize = 9;
const MENU_AUTO_UPDATE: usize = 10;
const MENU_LOCK: usize = 11;
const MENU_EDITOR: usize = 12;
/// Menu ids for the "Widgets" submenu: MENU_WIDGET + widget index.
const MENU_WIDGET: usize = 1000;
const WM_DPICHANGED: u32 = 0x02E0;
/// Distance (logical pixels) at which a dragged widget snaps to an edge.
const SNAP: i32 = 12;
/// How often the cached base image of a widget with a visualizer is redrawn.
const BASE_REFRESH_MS: u128 = 250;
const WM_APP_UPDATE_FOUND: u32 = WM_APP + 4;
const WM_APP_UPDATE_NONE: u32 = WM_APP + 5;
const WM_APP_UPDATE_READY: u32 = WM_APP + 6;
const WM_APP_UPDATE_FAILED: u32 = WM_APP + 7;
/// Hours between automatic update checks.
const UPDATE_CHECK_HOURS: u64 = 6;

/// "TaskbarCreated": Explorer restarted and the tray icon must be re-added.
static WM_TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Signed 16-bit x/y packed in an LPARAM.
fn point_of(lp: LPARAM) -> (i32, i32) {
    ((lp & 0xFFFF) as u16 as i16 as i32, ((lp >> 16) & 0xFFFF) as u16 as i16 as i32)
}

/// In-memory buffer (DIB) where pixels are copied before being sent to the window.
struct Surface {
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
}

impl Surface {
    fn new(w: u32, h: u32) -> Option<Surface> {
        unsafe {
            let dc = CreateCompatibleDC(null_mut());
            let mut bi: BITMAPINFO = std::mem::zeroed();
            bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bi.bmiHeader.biWidth = w as i32;
            bi.bmiHeader.biHeight = -(h as i32); // top-down
            bi.bmiHeader.biPlanes = 1;
            bi.bmiHeader.biBitCount = 32;
            bi.bmiHeader.biCompression = BI_RGB;
            let mut bits = null_mut();
            let bmp = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if bmp.is_null() || bits.is_null() {
                DeleteDC(dc);
                return None;
            }
            let old = SelectObject(dc, bmp);
            Some(Surface { dc, bmp, old, bits: bits as *mut u8 })
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bmp);
            DeleteDC(self.dc);
        }
    }
}

struct WinWidget {
    w: Widget,
    hwnd: HWND,
    /// Logical → device pixels (monitor DPI × widget `scale`).
    scale: f32,
    pixmap: Pixmap,
    surface: Surface,
    shown: bool,
    /// Widgets with a visualizer: everything else, drawn less often.
    base: Option<Pixmap>,
    base_at: Option<std::time::Instant>,
    /// Hash of the last frame sent to the window (identical frames are skipped).
    last_frame: u64,
    animating: bool,
}

impl WinWidget {
    fn device_size(&self) -> (u32, u32) {
        device_size(&self.w.cfg, self.scale)
    }

    /// (Re)creates the drawing buffers for the current scale.
    fn resize_buffers(&mut self) -> bool {
        let (w, h) = self.device_size();
        match (Pixmap::new(w, h), Surface::new(w, h)) {
            (Some(p), Some(sf)) => {
                self.pixmap = p;
                self.surface = sf;
                self.base = None;
                self.last_frame = 0;
                true
            }
            _ => false,
        }
    }
}

fn device_size(cfg: &config::WidgetConfig, scale: f32) -> (u32, u32) {
    (((cfg.width as f32 * scale).round() as u32).max(1), ((cfg.height as f32 * scale).round() as u32).max(1))
}

/// Monitor scaling of a window (1.0 = 100%, 1.5 = 150%...).
fn dpi_scale(hwnd: HWND) -> f32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 }
}

impl Drop for WinWidget {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.hwnd) };
    }
}

struct App {
    dir: PathBuf,
    widgets: Vec<WinWidget>,
    metrics: Metrics,
    fonts: Fonts,
    images: Images,
    web: Web,
    state: SavedState,
    hidden: bool,
    /// Widgets cannot be dragged.
    locked: bool,
    /// The desktop window last seen in front (Win+D raises one).
    desktop_window: HWND,
    /// While dragging: cursor position relative to the widget's corner.
    drag_offset: Option<(i32, i32)>,
    /// A newer release found on GitHub.
    update: Option<update::Release>,
    /// Download/installation in progress.
    updating: bool,
    tray: Option<Tray>,
    watch: Vec<(PathBuf, Option<SystemTime>, u64)>,
    /// "Show desktop" (Win+D) is active: the desktop window is in front of
    /// everything, so desktop-level widgets are raised above it meanwhile.
    desktop_shown: bool,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    /// (widget name, action) waiting to run outside the window procedure.
    static QUEUE: RefCell<VecDeque<(String, String)>> = const { RefCell::new(VecDeque::new()) };
}

/// Accesses the global state without panicking if it is already borrowed
/// (some Win32 calls re-enter the wndproc).
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

fn queue_actions(widget: &str, list: Vec<String>) {
    if list.is_empty() {
        return;
    }
    QUEUE.with(|q| q.borrow_mut().extend(list.into_iter().map(|a| (widget.to_string(), a))));
    unsafe { PostThreadMessageW(MAIN_THREAD.load(Ordering::Relaxed), WM_APP_ACTIONS, 0, 0) };
}

/// What Breezedesk remembers between runs (in `breezedesk.state`, next to the widgets):
/// where each widget was dragged and which ones were hidden from the menu.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct SavedState {
    #[serde(default)]
    positions: std::collections::BTreeMap<String, [i32; 2]>,
    #[serde(default)]
    hidden: std::collections::BTreeSet<String>,
}

fn state_path(dir: &Path) -> PathBuf {
    dir.join("breezedesk.state") // not .toml, so it is not loaded as a widget
}

fn load_state(dir: &Path) -> SavedState {
    if let Some(st) = std::fs::read_to_string(state_path(dir)).ok().and_then(|s| toml::from_str(&s).ok()) {
        return st;
    }
    // Before the rename (Plume) the state was in plume.state.
    if let Some(st) = std::fs::read_to_string(dir.join("plume.state")).ok().and_then(|s| toml::from_str(&s).ok()) {
        return st;
    }
    // Older versions saved only positions, in positions.state.
    let positions = std::fs::read_to_string(dir.join("positions.state"))
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default();
    SavedState { positions, ..Default::default() }
}

fn save_state(dir: &Path, st: &SavedState) {
    let body = toml::to_string(st).unwrap_or_default();
    let _ = std::fs::write(state_path(dir), format!("# Generated by Breezedesk: widget positions and hidden widgets.\n{body}"));
}

/// Files that trigger an automatic reload when they change.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Option<SystemTime>, u64)> {
    fn walk(dir: &Path, depth: u32, out: &mut Vec<(PathBuf, Option<SystemTime>, u64)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_lowercase();
            if name.starts_with('.') || name.ends_with(".state") || name.ends_with(".log") {
                continue;
            }
            let Ok(md) = e.metadata() else { continue };
            if md.is_dir() {
                if depth < 2 {
                    walk(&p, depth + 1, out);
                }
            } else {
                out.push((p, md.modified().ok(), md.len()));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, 0, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

impl App {
    fn notify(&self, title: &str, text: &str, warning: bool) {
        if let Some(t) = &self.tray {
            t.notify(title, text, warning);
        }
    }

    fn load(&mut self) {
        self.widgets.clear(); // Drop destroys the old windows
        self.images.clear();
        self.metrics.reset_wants();
        crate::log::reset_seen();
        let (cfgs, mut errors) = config::load_dir(&self.dir);
        let mut jobs = Vec::new();
        for cfg in cfgs {
            let (mut w, errs) = Widget::new(cfg, &self.metrics);
            errors.extend(errs);
            jobs.extend(w.web_jobs(&self.metrics, &self.web));
            let shown = !self.state.hidden.contains(&w.cfg.name);
            match create_window(w, &self.state.positions, shown) {
                Some(ww) => self.widgets.push(ww),
                None => errors.push("Failed to create a widget window.".into()),
            }
        }
        self.web.set_jobs(jobs);
        for e in &errors {
            crate::log::log(e);
        }
        if let Some(first) = errors.first() {
            let more = if errors.len() > 1 { format!("\n(+{} more, see breezedesk.log)", errors.len() - 1) } else { String::new() };
            self.notify("Breezedesk: widget error", &format!("{first}{more}"), true);
        }
        self.watch = snapshot(&self.dir);
        self.metrics.refresh();
        for i in 0..self.widgets.len() {
            ensure_visible(self.widgets[i].hwnd);
            self.paint(i, true);
        }
        if self.hidden {
            for w in &self.widgets {
                unsafe { ShowWindow(w.hwnd, SW_HIDE) };
            }
        }
        // New windows start at the bottom: re-apply "show desktop" if it is active.
        if std::mem::take(&mut self.desktop_shown) {
            self.set_desktop_shown(true);
        }
    }

    /// Raises desktop/normal widgets above the desktop window while "show
    /// desktop" is active, and puts them back afterwards (like Rainmeter does).
    fn set_desktop_shown(&mut self, shown: bool) {
        if shown == self.desktop_shown {
            return;
        }
        self.desktop_shown = shown;
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        for w in &self.widgets {
            let z = w.w.cfg.z;
            if z == ZOrder::Top {
                continue;
            }
            unsafe {
                if shown {
                    SetWindowPos(w.hwnd, HWND_TOPMOST, 0, 0, 0, 0, flags);
                } else {
                    SetWindowPos(w.hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, flags);
                    if z == ZOrder::Bottom {
                        place_above_desktop(w.hwnd);
                    }
                }
            }
        }
        if cfg!(debug_assertions) {
            crate::log::log(format!("show desktop: {shown}"));
            crate::log::reset_seen();
        }
    }

    fn index_of(&self, hwnd: HWND) -> Option<usize> {
        self.widgets.iter().position(|w| w.hwnd == hwnd)
    }

    fn index_by_name(&self, name: &str) -> Option<usize> {
        self.widgets.iter().position(|w| w.w.cfg.name == name)
    }

    fn show(&self, i: usize, show: bool) {
        let w = &self.widgets[i];
        unsafe {
            if !show {
                ShowWindow(w.hwnd, SW_HIDE);
                return;
            }
            ShowWindow(w.hwnd, SW_SHOWNOACTIVATE);
            if self.desktop_shown && w.w.cfg.z != ZOrder::Top {
                SetWindowPos(w.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            } else if w.w.cfg.z == ZOrder::Bottom {
                place_above_desktop(w.hwnd);
            }
        }
    }

    fn set_hidden(&mut self, hidden: bool) {
        self.hidden = hidden;
        for i in 0..self.widgets.len() {
            let shown = self.widgets[i].shown;
            self.show(i, !hidden && shown);
            if !hidden && shown {
                self.paint(i, true);
            }
        }
    }

    fn toggle_widget(&mut self, name: &str) {
        if let Some(i) = self.index_by_name(name) {
            let shown = !self.widgets[i].shown;
            self.widgets[i].shown = shown;
            if shown {
                self.state.hidden.remove(name);
            } else {
                self.state.hidden.insert(name.to_string());
            }
            save_state(&self.dir, &self.state);
            self.show(i, shown && !self.hidden);
            if shown {
                self.paint(i, true);
            }
        }
    }

    /// `update` = recompute data (timer); otherwise only redraw (hover, animation).
    fn paint(&mut self, i: usize, update: bool) {
        if self.hidden || !self.widgets[i].shown {
            return; // no CPU spent on invisible widgets
        }
        let ww = &mut self.widgets[i];
        if update {
            self.metrics.refresh();
            ww.w.update(&self.metrics, &self.web);
            let acts = std::mem::take(&mut ww.w.pending_actions);
            queue_actions(&ww.w.cfg.name, acts);
        }
        let (m, web, fonts, images, s) = (&self.metrics, &self.web, &mut self.fonts, &mut self.images, ww.scale);
        let has_live = ww.w.cfg.elements.iter().any(render::is_live);
        let animating = if has_live {
            // Redraw the slow part only now and then; the visualizer on top every frame.
            let stale = ww.base_at.is_none_or(|t| t.elapsed().as_millis() >= BASE_REFRESH_MS) || !update;
            let mut anim = ww.animating;
            if stale || ww.base.is_none() {
                let mut base = ww.base.take().unwrap_or_else(|| ww.pixmap.clone());
                anim = render::render(&mut ww.w, &mut base, m, web, fonts, images, s, render::Layer::Base);
                ww.base = Some(base);
                ww.base_at = Some(std::time::Instant::now());
            }
            if let Some(base) = &ww.base {
                ww.pixmap.data_mut().copy_from_slice(base.data());
            }
            render::render(&mut ww.w, &mut ww.pixmap, m, web, fonts, images, s, render::Layer::Live);
            anim
        } else {
            render::render(&mut ww.w, &mut ww.pixmap, m, web, fonts, images, s, render::Layer::All)
        };
        if animating != ww.animating {
            ww.animating = animating;
            unsafe {
                if animating {
                    SetTimer(ww.hwnd, TIMER_ANIM, 16, None);
                } else {
                    KillTimer(ww.hwnd, TIMER_ANIM);
                }
            }
        }

        let cfg = &ww.w.cfg;
        let opacity = if ww.w.mouse_over { cfg.hover_opacity.unwrap_or(cfg.opacity) } else { cfg.opacity };
        // Nothing changed since the last frame (most seconds for most widgets): skip.
        let frame = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            ww.pixmap.data().hash(&mut h);
            opacity.hash(&mut h);
            h.finish()
        };
        if frame == ww.last_frame {
            return;
        }
        ww.last_frame = frame;

        // tiny-skia uses premultiplied RGBA; Windows wants premultiplied BGRA.
        let src = ww.pixmap.data();
        let dst = unsafe { std::slice::from_raw_parts_mut(ww.surface.bits, src.len()) };
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
        let (dw, dh) = ww.device_size();
        let size = SIZE { cx: dw as i32, cy: dh as i32 };
        let origin = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: opacity,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            UpdateLayeredWindow(ww.hwnd, null_mut(), null(), &size, ww.surface.dc, &origin, 0, &blend, ULW_ALPHA);
        }
    }

    /// The monitor scaling of a widget changed (moved to another monitor, or
    /// the Windows scaling setting changed).
    fn rescale(&mut self, i: usize, suggested: Option<RECT>) {
        let ww = &mut self.widgets[i];
        let scale = dpi_scale(ww.hwnd) * ww.w.cfg.scale.max(0.1);
        if (scale - ww.scale).abs() < 1e-3 && suggested.is_none() {
            return;
        }
        ww.scale = scale;
        if !ww.resize_buffers() {
            return;
        }
        let (w, h) = ww.device_size();
        unsafe {
            match suggested {
                Some(r) => SetWindowPos(ww.hwnd, null_mut(), r.left, r.top, w as i32, h as i32, SWP_NOZORDER | SWP_NOACTIVATE),
                None => SetWindowPos(ww.hwnd, null_mut(), 0, 0, w as i32, h as i32, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE),
            };
            if let Some(bg) = &ww.w.cfg.background {
                if bg.blur != Blur::None {
                    apply_blur(ww.hwnd, bg, w, h, scale);
                }
            }
        }
        self.paint(i, false);
    }

    /// Mouse position (widget coordinates) → hover state; redraws if it changed.
    fn hover(&mut self, i: usize, pos: Option<(i32, i32)>) {
        let s = self.widgets[i].scale;
        let w = &mut self.widgets[i].w;
        if !w.has_hover_effects() {
            return;
        }
        let (over, el) = match pos {
            Some((x, y)) => (true, w.element_at(x as f32 / s, y as f32 / s)),
            None => (false, None),
        };
        if over != w.mouse_over || el != w.hovered {
            w.mouse_over = over;
            w.hovered = el;
            self.paint(i, false);
        }
    }

    fn mouse(&mut self, i: usize, x: i32, y: i32, button: Mouse) -> bool {
        let s = self.widgets[i].scale;
        let w = &self.widgets[i].w;
        let (lx, ly) = (x as f32 / s, y as f32 / s);
        let found = match button {
            Mouse::Left | Mouse::Middle => w.clickable_at(lx, ly),
            _ => w.element_at(lx, ly),
        };
        let Some(el) = found else { return false };
        let list = w.actions(el, button, &self.metrics, &self.web);
        if list.is_empty() {
            return false;
        }
        queue_actions(&w.cfg.name.clone(), list);
        true
    }
}

fn create_window(w: Widget, positions: &std::collections::BTreeMap<String, [i32; 2]>, shown: bool) -> Option<WinWidget> {
    let cfg = &w.cfg;
    let [x, y] = positions.get(&cfg.name).copied().unwrap_or([cfg.x, cfg.y]);
    let mut ex = WS_EX_LAYERED | WS_EX_TOOLWINDOW; // TOOLWINDOW = no taskbar button
    if cfg.z == ZOrder::Top {
        ex |= WS_EX_TOPMOST;
    }
    if cfg.click_through {
        ex |= WS_EX_TRANSPARENT;
    }
    let class = wide("BreezedeskWidget");
    let title = wide(&cfg.name);
    let hwnd = unsafe {
        CreateWindowExW(
            ex,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            x,
            y,
            cfg.width.max(1) as i32,
            cfg.height.max(1) as i32,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        )
    };
    if hwnd.is_null() {
        return None;
    }
    // Scale for the monitor the widget is on (125%, 150%... in Windows settings).
    let scale = dpi_scale(hwnd) * cfg.scale.max(0.1);
    let (width, height) = device_size(cfg, scale);
    let pixmap = Pixmap::new(width, height)?;
    let surface = Surface::new(width, height)?;
    unsafe {
        if let Some(bg) = &cfg.background {
            if bg.blur != Blur::None {
                apply_blur(hwnd, bg, width, height, scale);
            }
        }
        let flags = SWP_NOMOVE | SWP_NOACTIVATE | SWP_NOZORDER | if shown { SWP_SHOWWINDOW } else { 0 };
        SetWindowPos(hwnd, null_mut(), 0, 0, width as i32, height as i32, flags);
        if cfg.z == ZOrder::Bottom {
            place_above_desktop(hwnd);
        }
        arm_timer(hwnd, cfg.update_ms);
    }
    Some(WinWidget {
        w,
        hwnd,
        scale,
        pixmap,
        surface,
        shown,
        base: None,
        base_at: None,
        last_frame: 0,
        animating: false,
    })
}

/// Moves a widget back onto a screen if it is (almost) entirely outside of
/// every monitor, e.g. after a monitor was disconnected. The saved position is
/// kept, so it returns when that monitor comes back.
fn ensure_visible(hwnd: HWND) {
    unsafe {
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetWindowRect(hwnd, &mut r);
        let mon = MonitorFromRect(&r, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(mon, &mut mi) == 0 {
            return;
        }
        let work = mi.rcWork;
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let ix = (r.right.min(work.right) - r.left.max(work.left)).max(0);
        let iy = (r.bottom.min(work.bottom) - r.top.max(work.top)).max(0);
        // Visible enough to grab (a quarter of it, or 40 px each way)?
        if ix * iy * 4 >= w * h || (ix >= 40 && iy >= 40) {
            return;
        }
        let x = r.left.clamp(work.left, (work.right - w).max(work.left));
        let y = r.top.clamp(work.top, (work.bottom - h).max(work.top));
        SetWindowPos(hwnd, null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

/// While dragging: where the widget goes. The position always comes from the
/// mouse (cursor minus where the widget was grabbed), then snaps to the screen
/// edges and neighbouring widgets (see snap.rs). Hold Shift to place it freely.
fn drag_rect(a: &App, hwnd: HWND, proposed: &RECT) -> RECT {
    let (w, h) = (proposed.right - proposed.left, proposed.bottom - proposed.top);
    let (mut x, mut y) = (proposed.left, proposed.top);
    if let Some((ox, oy)) = a.drag_offset {
        let mut pt = POINT { x: 0, y: 0 };
        unsafe { GetCursorPos(&mut pt) };
        (x, y) = (pt.x - ox, pt.y - oy);
    }
    let shift = unsafe { GetKeyState(VK_SHIFT as i32) } < 0;
    if let (false, Some(i)) = (shift, a.index_of(hwnd)) {
        let s = a.widgets[i].scale;
        let target = RECT { left: x, top: y, right: x + w, bottom: y + h };
        let mut mi: MONITORINFO = unsafe { std::mem::zeroed() };
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        let mon = unsafe { MonitorFromRect(&target, MONITOR_DEFAULTTONEAREST) };
        if unsafe { GetMonitorInfoW(mon, &mut mi) } != 0 {
            let wk = mi.rcWork;
            let work = crate::snap::Rect { x: wk.left, y: wk.top, w: wk.right - wk.left, h: wk.bottom - wk.top };
            let others: Vec<crate::snap::Rect> = a
                .widgets
                .iter()
                .filter(|o| o.hwnd != hwnd && o.shown && !a.hidden)
                .map(|o| {
                    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                    unsafe { GetWindowRect(o.hwnd, &mut r) };
                    crate::snap::Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
                })
                .collect();
            let me = crate::snap::Rect { x, y, w, h };
            (x, y) = crate::snap::snap(me, work, &others, (SNAP as f32 * s) as i32, (4.0 * s) as i32);
        }
    }
    RECT { left: x, top: y, right: x + w, bottom: y + h }
}

/// Schedules the next update. Slow widgets (>= 0.5 s) are lined up with the
/// clock (just after each second turns), so a clock never skips or repeats a
/// second; fast ones just repeat.
fn arm_timer(hwnd: HWND, update_ms: u32) {
    let ms = update_ms.max(16);
    let delay = if ms >= 500 {
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        (ms as u64 - now % ms as u64 + 15) as u32
    } else {
        ms
    };
    unsafe { SetTimer(hwnd, TIMER_UPDATE, delay, None) };
}

/// Blur/acrylic behind the widget (undocumented but stable API, also used by
/// Rainmeter plugins), clipped to the rounded background.
unsafe fn apply_blur(hwnd: HWND, bg: &Background, w: u32, h: u32, scale: f32) {
    #[repr(C)]
    struct AccentPolicy {
        state: u32,
        flags: u32,
        gradient: u32,
        animation: u32,
    }
    #[repr(C)]
    struct CompositionData {
        attribute: u32,
        data: *mut std::ffi::c_void,
        size: usize,
    }
    type SetWca = unsafe extern "system" fn(HWND, *mut CompositionData) -> BOOL;
    let user32 = GetModuleHandleW(wide("user32.dll").as_ptr());
    let Some(f) = GetProcAddress(user32, c"SetWindowCompositionAttribute".as_ptr().cast()) else { return };
    let set: SetWca = std::mem::transmute(f);
    // Acrylic needs a tint; the widget draws its own background on top, so keep it faint.
    let tint = match &bg.color {
        config::ColorSpec::Static(c) => (c.b as u32) << 16 | (c.g as u32) << 8 | c.r as u32,
        _ => 0,
    };
    let mut accent = AccentPolicy {
        state: if bg.blur == Blur::Acrylic { 4 } else { 3 },
        flags: 2,
        gradient: 0x0100_0000 | tint,
        animation: 0,
    };
    let mut data = CompositionData {
        attribute: 19, // WCA_ACCENT_POLICY
        data: &mut accent as *mut _ as *mut _,
        size: std::mem::size_of::<AccentPolicy>(),
    };
    set(hwnd, &mut data);
    if bg.radius > 0.0 {
        let d = (bg.radius * 2.0 * scale) as i32;
        let rgn = CreateRoundRectRgn(0, 0, w as i32 + 1, h as i32 + 1, d, d);
        SetWindowRgn(hwnd, rgn, TRUE); // the window owns the region now
    }
}

fn shell_open(target: &str, args: Option<&str>) {
    let (verb, t) = (wide("open"), wide(target));
    let a = args.map(wide);
    let r = unsafe {
        ShellExecuteW(
            null_mut(),
            verb.as_ptr(),
            t.as_ptr(),
            a.as_ref().map_or(null(), |a| a.as_ptr()),
            null(),
            SW_SHOWNORMAL,
        )
    };
    if (r as isize) <= 32 {
        crate::log::log(format!("could not open {target:?} (error {})", r as isize));
    }
}

fn press_key(key: MediaKey) {
    let vk = match key {
        MediaKey::PlayPause => VK_MEDIA_PLAY_PAUSE,
        MediaKey::Next => VK_MEDIA_NEXT_TRACK,
        MediaKey::Prev => VK_MEDIA_PREV_TRACK,
        MediaKey::Stop => VK_MEDIA_STOP,
        MediaKey::VolumeUp => VK_VOLUME_UP,
        MediaKey::VolumeDown => VK_VOLUME_DOWN,
        MediaKey::Mute => VK_VOLUME_MUTE,
    };
    unsafe {
        let mut inputs: [INPUT; 2] = std::mem::zeroed();
        for (i, input) in inputs.iter_mut().enumerate() {
            input.r#type = INPUT_KEYBOARD;
            input.Anonymous.ki.wVk = vk;
            input.Anonymous.ki.dwFlags = KEYEVENTF_EXTENDEDKEY | if i == 1 { KEYEVENTF_KEYUP } else { 0 };
        }
        SendInput(2, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

/// Runs queued actions (outside the window procedure, so they may reload, etc).
fn process_actions() {
    for _ in 0..256 {
        let Some((widget, text)) = QUEUE.with(|q| q.borrow_mut().pop_front()) else { return };
        let action = match actions::parse(&text) {
            Ok(a) => a,
            Err(e) => {
                crate::log::log(format!("[{widget}] {e}"));
                continue;
            }
        };
        match action {
            Action::Open { target, args } => {
                // Paths relative to the widgets folder work too.
                let target = with_app(|a| {
                    let p = a.dir.join(&target);
                    if !target.contains("://") && Path::new(&target).is_relative() && p.exists() {
                        p.to_string_lossy().into_owned()
                    } else {
                        target.clone()
                    }
                })
                .unwrap_or(target);
                shell_open(&target, args.as_deref());
            }
            Action::Key(k) => press_key(k),
            Action::Exit => unsafe { PostQuitMessage(0) },
            Action::Reload => {
                with_app(App::load);
            }
            Action::Hide => {
                with_app(|a| a.set_hidden(true));
            }
            Action::Show => {
                with_app(|a| a.set_hidden(false));
            }
            Action::Toggle => {
                with_app(|a| a.set_hidden(!a.hidden));
            }
            Action::ToggleWidget(name) => {
                with_app(|a| a.toggle_widget(&name));
            }
            Action::Refresh => {
                with_app(|a| a.index_by_name(&widget).map(|i| a.paint(i, true)));
            }
            Action::Set(name, src) => {
                with_app(|a| {
                    let i = a.index_by_name(&widget)?;
                    if let Err(e) = a.widgets[i].w.set_var(&name, &src, &a.metrics, &a.web) {
                        crate::log::log(format!("[{widget}] !set: {e}"));
                    }
                    a.paint(i, true);
                    Some(())
                });
            }
            Action::Lua(f) => {
                with_app(|a| {
                    let i = a.index_by_name(&widget)?;
                    if let Err(e) = a.widgets[i].w.call_lua(&f, &a.metrics, &a.web) {
                        crate::log::log(format!("[{widget}] !lua:{f}: {e}"));
                    }
                    let acts = std::mem::take(&mut a.widgets[i].w.pending_actions);
                    queue_actions(&widget, acts);
                    a.paint(i, true);
                    Some(())
                });
            }
        }
    }
}

// ------------------------------------------------------------------ updates

/// Results handed from the update threads to the window thread.
static UPDATE_FOUND: Mutex<Option<update::Release>> = Mutex::new(None);
static UPDATE_FILE: Mutex<Option<PathBuf>> = Mutex::new(None);
static UPDATE_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn post(msg: u32, wparam: usize) {
    unsafe { PostThreadMessageW(MAIN_THREAD.load(Ordering::Relaxed), msg, wparam, 0) };
}

/// True when running from the installer's folder (%LOCALAPPDATA%\Programs\Breezedesk).
fn is_installed() -> bool {
    let (Ok(exe), Some(local)) = (std::env::current_exe(), std::env::var_os("LOCALAPPDATA")) else { return false };
    let dir = PathBuf::from(local).join("Programs").join("Breezedesk");
    exe.to_string_lossy().to_lowercase().starts_with(&dir.to_string_lossy().to_lowercase())
}

/// Asks GitHub for a newer release in the background.
fn check_for_update(manual: bool) {
    std::thread::spawn(move || match update::check() {
        Ok(Some(rel)) => {
            *UPDATE_FOUND.lock().unwrap() = Some(rel);
            post(WM_APP_UPDATE_FOUND, manual as usize);
        }
        Ok(None) => {
            if manual {
                post(WM_APP_UPDATE_NONE, 0);
            }
        }
        Err(e) => {
            crate::log::log(format!("update check: {e}"));
            if manual {
                *UPDATE_ERROR.lock().unwrap() = Some(e);
                post(WM_APP_UPDATE_FAILED, 0);
            }
        }
    });
}

fn download_update(rel: update::Release) {
    std::thread::spawn(move || match update::download(&rel) {
        Ok(path) => {
            *UPDATE_FILE.lock().unwrap() = Some(path);
            post(WM_APP_UPDATE_READY, 0);
        }
        Err(e) => {
            crate::log::log(format!("update download: {e}"));
            *UPDATE_ERROR.lock().unwrap() = Some(e);
            post(WM_APP_UPDATE_FAILED, 0);
        }
    });
}

impl App {
    /// Installed copies download and install; portable/dev copies just tell the user.
    fn start_update(&mut self, rel: update::Release) {
        if is_installed() && !update::DEV_BUILD {
            if !self.updating {
                self.updating = true;
                self.notify("Breezedesk", &format!("Downloading Breezedesk {}...", update::fmt(rel.version)), false);
                download_update(rel);
            }
        } else {
            shell_open(&rel.page, None);
        }
    }

    fn on_update_message(&mut self, msg: u32, wparam: usize) {
        match msg {
            WM_APP_UPDATE_FOUND => {
                let Some(rel) = UPDATE_FOUND.lock().unwrap().take() else { return };
                let manual = wparam != 0;
                let v = update::fmt(rel.version);
                self.update = Some(rel.clone());
                if is_installed() && !update::DEV_BUILD && (manual || tray::auto_update_enabled()) {
                    self.start_update(rel);
                } else {
                    let how = if is_installed() { "right-click the tray icon and choose Update" } else { "download it from GitHub (tray menu > Update)" };
                    self.notify("Breezedesk update available", &format!("Breezedesk {v} is available: {how}."), false);
                }
            }
            WM_APP_UPDATE_NONE => {
                let v = update::fmt(update::current());
                self.notify("Breezedesk", &format!("Breezedesk is up to date ({v})."), false);
            }
            WM_APP_UPDATE_READY => {
                let Some(path) = UPDATE_FILE.lock().unwrap().take() else { return };
                self.notify("Breezedesk", "Installing the update. Breezedesk will restart in a moment.", false);
                let (verb, file, args) = (wide("open"), wide(&path.to_string_lossy()), wide("/S /RELAUNCH"));
                let r = unsafe {
                    ShellExecuteW(null_mut(), verb.as_ptr(), file.as_ptr(), args.as_ptr(), null(), SW_HIDE)
                };
                if (r as isize) > 32 {
                    unsafe { PostQuitMessage(0) }; // the installer replaces and restarts us
                } else {
                    self.updating = false;
                    self.notify("Breezedesk update failed", "Could not start the installer.", true);
                }
            }
            WM_APP_UPDATE_FAILED => {
                self.updating = false;
                let e = UPDATE_ERROR.lock().unwrap().take().unwrap_or_default();
                self.notify("Breezedesk update failed", &e, true);
            }
            _ => {}
        }
    }
}

fn show_menu(hwnd: HWND) {
    unsafe {
        let hidden = with_app(|a| a.hidden).unwrap_or(false);
        let autostart = tray::autostart_enabled();
        let auto_update = tray::auto_update_enabled();
        let available = with_app(|a| a.update.clone()).flatten();
        let title = format!(
            "Breezedesk {}{}",
            update::fmt(update::current()),
            if update::DEV_BUILD { " (dev build)" } else { "" }
        );
        let update_label = match &available {
            Some(r) => format!("Update to {}", update::fmt(r.version)),
            None => "Check for updates".to_string(),
        };
        let locked = with_app(|a| a.locked).unwrap_or(false);
        // "Widgets" submenu: one checkbox per widget.
        let names: Vec<(String, bool)> =
            with_app(|a| a.widgets.iter().map(|w| (w.w.cfg.name.clone(), w.shown)).collect()).unwrap_or_default();
        let sub = CreatePopupMenu();
        for (i, (name, shown)) in names.iter().enumerate() {
            let l = wide(name);
            AppendMenuW(sub, MF_STRING | if *shown { MF_CHECKED } else { MF_UNCHECKED }, MENU_WIDGET + i, l.as_ptr());
        }
        if names.is_empty() {
            let l = wide("(no widgets)");
            AppendMenuW(sub, MF_STRING | MF_GRAYED, 0, l.as_ptr());
        }
        let menu = CreatePopupMenu();
        let items: Vec<(usize, &str, u32)> = vec![
            (0, &title, MF_STRING | MF_GRAYED),
            (0, "", MF_SEPARATOR),
            (MENU_TOGGLE, if hidden { "Show widgets" } else { "Hide widgets" }, MF_STRING),
            (sub as usize, "Widgets", MF_POPUP),
            (MENU_LOCK, "Lock positions", MF_STRING | if locked { MF_CHECKED } else { MF_UNCHECKED }),
            (0, "", MF_SEPARATOR),
            (MENU_EDITOR, "Edit widgets...", MF_STRING),
            (MENU_RELOAD, "Reload widgets", MF_STRING),
            (MENU_EDIT, "Open widgets folder", MF_STRING),
            (MENU_EXAMPLES, "Install example widgets", MF_STRING),
            (MENU_LOG, "Open log", MF_STRING),
            (0, "", MF_SEPARATOR),
            (MENU_AUTOSTART, "Start with Windows", MF_STRING | if autostart { MF_CHECKED } else { MF_UNCHECKED }),
            (0, "", MF_SEPARATOR),
            (if available.is_some() { MENU_UPDATE } else { MENU_CHECK_UPDATE }, &update_label, MF_STRING),
            (
                MENU_AUTO_UPDATE,
                "Automatic updates",
                MF_STRING | if auto_update { MF_CHECKED } else { MF_UNCHECKED } | if update::DEV_BUILD { MF_GRAYED } else { 0 },
            ),
            (0, "", MF_SEPARATOR),
            (MENU_EXIT, "Exit", MF_STRING),
        ];
        for (id, label, flags) in items {
            let l = wide(label);
            AppendMenuW(menu, flags, id, l.as_ptr());
        }
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, null())
            as usize;
        DestroyMenu(menu); // also destroys the submenu
        // Microsoft-recommended workaround for menus opened from the tray.
        PostMessageW(hwnd, WM_NULL, 0, 0);
        match cmd {
            id if (MENU_WIDGET..MENU_WIDGET + names.len()).contains(&id) => {
                let name = names[id - MENU_WIDGET].0.clone();
                with_app(|a| a.toggle_widget(&name));
            }
            MENU_LOCK => {
                tray::set_positions_locked(!locked);
                with_app(|a| a.locked = !locked);
            }
            MENU_EDITOR => {
                if let Some(dir) = with_app(|a| a.dir.clone()) {
                    open_editor(&dir);
                }
            }
            MENU_TOGGLE => {
                with_app(|a| a.set_hidden(!a.hidden));
            }
            MENU_AUTOSTART => tray::set_autostart(!autostart),
            MENU_CHECK_UPDATE => check_for_update(true),
            MENU_UPDATE => {
                if let Some(rel) = available {
                    with_app(|a| a.start_update(rel));
                }
            }
            MENU_AUTO_UPDATE => tray::set_auto_update(!auto_update),
            // Reloading destroys this window, so it is done outside the wndproc.
            MENU_RELOAD => {
                PostThreadMessageW(GetCurrentThreadId(), WM_APP_RELOAD, 0, 0);
            }
            MENU_EDIT => {
                if let Some(dir) = with_app(|a| a.dir.clone()) {
                    shell_open(&dir.to_string_lossy(), None);
                }
            }
            MENU_LOG => {
                if let Some(p) = crate::log::path() {
                    shell_open(&p.to_string_lossy(), None);
                }
            }
            MENU_EXAMPLES => {
                if let Some(dir) = with_app(|a| a.dir.clone()) {
                    let n = crate::install_examples(&dir);
                    let msg = if n == 0 { "All examples are already installed.".to_string() } else { format!("{n} example files added.") };
                    with_app(|a| a.notify("Breezedesk", &msg, false));
                    PostThreadMessageW(GetCurrentThreadId(), WM_APP_RELOAD, 0, 0);
                }
            }
            MENU_EXIT => PostQuitMessage(0),
            _ => {}
        }
    }
}

/// Starts the visual editor (breezedesk-editor.exe, next to breezedesk.exe).
fn open_editor(dir: &Path) {
    let editor = std::env::current_exe().ok().and_then(|p| Some(p.parent()?.join("breezedesk-editor.exe")));
    match editor.filter(|p| p.exists()) {
        Some(p) => {
            let arg = format!("\"{}\"", dir.display());
            shell_open(&p.to_string_lossy(), Some(&arg));
        }
        None => {
            with_app(|a| a.notify("Breezedesk", "The editor (breezedesk-editor.exe) was not found next to breezedesk.exe.", true));
        }
    }
}

/// Screen point → (widget index, widget coordinates).
fn locate(a: &App, hwnd: HWND, sx: i32, sy: i32) -> Option<(usize, i32, i32)> {
    let i = a.index_of(hwnd)?;
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetWindowRect(hwnd, &mut r) };
    Some((i, sx - r.left, sy - r.top))
}

fn track_leave(hwnd: HWND, nonclient: bool) {
    let mut t = TRACKMOUSEEVENT {
        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE | if nonclient { TME_NONCLIENT } else { 0 },
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    unsafe { TrackMouseEvent(&mut t) };
}

/// True for the desktop windows (the icons host, or the one Win+D raises).
fn is_desktop_window(hwnd: HWND) -> bool {
    let mut buf = [0u16; 32];
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    let class = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
    class == "WorkerW" || class == "Progman"
}

/// The window that shows the desktop icons: Progman, or a WorkerW on some setups.
fn desktop_host() -> HWND {
    unsafe {
        let defview = wide("SHELLDLL_DefView");
        let has_icons = |w: HWND| !w.is_null() && !FindWindowExW(w, null_mut(), defview.as_ptr(), null()).is_null();
        let progman = FindWindowW(wide("Progman").as_ptr(), null());
        if has_icons(progman) {
            return progman;
        }
        let worker = wide("WorkerW");
        let mut w = null_mut();
        loop {
            w = FindWindowExW(null_mut(), w, worker.as_ptr(), null());
            if w.is_null() {
                return progman;
            }
            if has_icons(w) {
                return w;
            }
        }
    }
}

/// Where a "bottom" widget goes in the window stack: right above the desktop.
/// (Not at the very bottom: the desktop window itself could then cover it.)
/// `None` when the widget is already there.
fn above_desktop(hwnd: HWND) -> Option<HWND> {
    unsafe {
        let host = desktop_host();
        if host.is_null() {
            return Some(HWND_BOTTOM);
        }
        let prev = GetWindow(host, GW_HWNDPREV);
        if prev == hwnd {
            return None;
        }
        // The window above the desktop is "always on top" (the desktop is raised):
        // never join that group.
        if prev.is_null() || GetWindowLongW(prev, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0 {
            return Some(HWND_BOTTOM);
        }
        Some(prev)
    }
}

fn place_above_desktop(hwnd: HWND) {
    if let Some(after) = above_desktop(hwnd) {
        unsafe { SetWindowPos(hwnd, after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
    }
}

/// A normal application window the user can see (not ours, not minimized,
/// not "always on top" like the taskbar, not a hidden/cloaked app).
fn is_visible_app_window(w: HWND) -> bool {
    unsafe {
        if IsWindowVisible(w) == 0 || IsIconic(w) != 0 || !GetWindow(w, GW_OWNER).is_null() {
            return false;
        }
        let ex = GetWindowLongW(w, GWL_EXSTYLE) as u32;
        if ex & (WS_EX_TOPMOST | WS_EX_TOOLWINDOW) != 0 {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(w, &mut pid);
        if pid == std::process::id() {
            return false;
        }
        let mut cloaked = 0u32;
        let r = DwmGetWindowAttribute(w, DWMWA_CLOAKED as u32, &mut cloaked as *mut u32 as *mut _, 4);
        if r == 0 && cloaked != 0 {
            return false;
        }
        let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetWindowRect(w, &mut rc);
        rc.right > rc.left && rc.bottom > rc.top
    }
}

/// "Show desktop" is active when the desktop window is above every visible
/// application window. (A plain click on the desktop also makes it the active
/// window, but it stays below the open windows.)
fn desktop_raised(desktop: HWND) -> bool {
    unsafe {
        if desktop.is_null() || IsWindowVisible(desktop) == 0 {
            return false;
        }
        let mut w = GetTopWindow(null_mut());
        while !w.is_null() {
            if w == desktop {
                return true;
            }
            if is_visible_app_window(w) {
                return false;
            }
            w = GetWindow(w, GW_HWNDNEXT);
        }
        false
    }
}

unsafe extern "system" fn on_foreground(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    check_foreground(hwnd);
}

/// Re-checks "show desktop" when the active window changes (and every second).
/// Our own windows (a clicked widget, the tray menu) leave the state as it is.
fn check_foreground(hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == std::process::id() {
        return;
    }
    with_app(|a| {
        if is_desktop_window(hwnd) {
            a.desktop_window = hwnd;
        }
        let raised = desktop_raised(a.desktop_window);
        a.set_desktop_shown(raised);
    });
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        // Tray icon: left click shows/hides, right click opens the menu.
        WM_APP_TRAY => {
            match lp as u32 {
                WM_LBUTTONUP => {
                    with_app(|a| a.set_hidden(!a.hidden));
                }
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                _ => {}
            }
            0
        }
        m if m != 0 && m == WM_TASKBAR_CREATED.load(Ordering::Relaxed) => {
            with_app(|a| a.tray.as_ref().map(|t| t.add()));
            0
        }
        WM_TIMER if wp == TIMER_UPDATE => {
            with_app(|a| {
                let i = a.index_of(hwnd)?;
                let ms = a.widgets[i].w.cfg.update_ms;
                if ms >= 500 {
                    arm_timer(hwnd, ms);
                }
                a.paint(i, true);
                Some(())
            });
            0
        }
        WM_TIMER if wp == TIMER_ANIM => {
            with_app(|a| a.index_of(hwnd).map(|i| a.paint(i, false)));
            0
        }
        // Auto-reload when a widget file changes.
        WM_TIMER if wp == TIMER_WATCH => {
            with_app(|a| {
                if snapshot(&a.dir) != a.watch {
                    a.load();
                }
            });
            // Backup for the foreground hook, in case it is not delivered.
            check_foreground(GetForegroundWindow());
            0
        }
        // Over a clickable element: normal window area. Elsewhere: "title bar",
        // so Windows handles the dragging.
        WM_NCHITTEST => {
            let (sx, sy) = point_of(lp);
            let hit = with_app(|a| {
                let (i, x, y) = locate(a, hwnd, sx, sy)?;
                let s = a.widgets[i].scale;
                let w = &a.widgets[i].w;
                let on_element = w.clickable_at(x as f32 / s, y as f32 / s).is_some();
                Some(if on_element || !w.cfg.draggable || a.locked { HTCLIENT } else { HTCAPTION })
            });
            hit.flatten().unwrap_or(HTCLIENT) as LRESULT
        }
        WM_SETCURSOR => {
            let hand = with_app(|a| {
                let mut pt = POINT { x: 0, y: 0 };
                GetCursorPos(&mut pt);
                let (i, x, y) = locate(a, hwnd, pt.x, pt.y)?;
                let s = a.widgets[i].scale;
                let w = &a.widgets[i].w;
                let el = w.clickable_at(x as f32 / s, y as f32 / s)?;
                Some(!w.cfg.elements[el].on_click.is_empty())
            });
            if hand.flatten().unwrap_or(false) {
                SetCursor(LoadCursorW(null_mut(), IDC_HAND));
                return 1;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_MOUSEMOVE | WM_NCMOUSEMOVE => {
            let nc = msg == WM_NCMOUSEMOVE;
            let mut pt = POINT { x: 0, y: 0 };
            GetCursorPos(&mut pt);
            with_app(|a| {
                let (i, x, y) = locate(a, hwnd, pt.x, pt.y)?;
                a.hover(i, Some((x, y)));
                Some(())
            });
            track_leave(hwnd, nc);
            if nc { DefWindowProcW(hwnd, msg, wp, lp) } else { 0 }
        }
        WM_MOUSELEAVE | WM_NCMOUSELEAVE => {
            let mut pt = POINT { x: 0, y: 0 };
            GetCursorPos(&mut pt);
            with_app(|a| {
                let (i, x, y) = locate(a, hwnd, pt.x, pt.y)?;
                let (dw, dh) = a.widgets[i].device_size();
                let inside = x >= 0 && y >= 0 && x < dw as i32 && y < dh as i32;
                // Moving between the "title bar" part and a clickable element also
                // sends a leave message: only really leave if the cursor is outside.
                a.hover(i, inside.then_some((x, y)));
                Some(())
            });
            0
        }
        WM_LBUTTONUP | WM_MBUTTONUP => {
            let (x, y) = point_of(lp);
            let button = if msg == WM_LBUTTONUP { Mouse::Left } else { Mouse::Middle };
            with_app(|a| a.index_of(hwnd).map(|i| a.mouse(i, x, y, button)));
            0
        }
        WM_RBUTTONUP | WM_NCRBUTTONUP => {
            let mut pt = POINT { x: 0, y: 0 };
            GetCursorPos(&mut pt);
            let handled = with_app(|a| {
                let (i, x, y) = locate(a, hwnd, pt.x, pt.y)?;
                Some(a.mouse(i, x, y, Mouse::Right))
            });
            if !handled.flatten().unwrap_or(false) {
                show_menu(hwnd);
            }
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp >> 16) & 0xFFFF) as u16 as i16;
            let (sx, sy) = point_of(lp);
            let dir = if delta > 0 { Mouse::ScrollUp } else { Mouse::ScrollDown };
            with_app(|a| {
                let (i, x, y) = locate(a, hwnd, sx, sy)?;
                Some(a.mouse(i, x, y, dir))
            });
            0
        }
        // "bottom" widgets never rise in the window stack.
        WM_WINDOWPOSCHANGING => {
            let bottom = with_app(|a| {
                a.index_of(hwnd).map(|i| a.widgets[i].w.cfg.z == ZOrder::Bottom && !a.desktop_shown)
            });
            let wpos = &mut *(lp as *mut WINDOWPOS);
            if bottom.flatten().unwrap_or(false) && wpos.flags & SWP_NOZORDER == 0 {
                match above_desktop(hwnd) {
                    Some(after) => wpos.hwndInsertAfter = after,
                    None => wpos.flags |= SWP_NOZORDER, // already right above the desktop
                }
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        // The installer/uninstaller asks Breezedesk to quit by closing the hidden
        // tray window. Widget windows ignore close requests (e.g. Alt+F4).
        WM_CLOSE => {
            let is_widget = with_app(|a| a.index_of(hwnd).is_some()).unwrap_or(true);
            if !is_widget {
                PostQuitMessage(0);
            }
            0
        }
        // Drag finished: save the position.
        WM_EXITSIZEMOVE => {
            let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            GetWindowRect(hwnd, &mut r);
            with_app(|a| {
                a.drag_offset = None;
                if let Some(i) = a.index_of(hwnd) {
                    let name = a.widgets[i].w.cfg.name.clone();
                    a.state.positions.insert(name, [r.left, r.top]);
                    save_state(&a.dir, &a.state);
                }
            });
            0
        }
        // Being dragged: snap to edges.
        WM_ENTERSIZEMOVE => {
            // Remember where the widget was grabbed, to follow the mouse exactly.
            let (mut pt, mut r) = (POINT { x: 0, y: 0 }, RECT { left: 0, top: 0, right: 0, bottom: 0 });
            GetCursorPos(&mut pt);
            GetWindowRect(hwnd, &mut r);
            with_app(|a| a.drag_offset = Some((pt.x - r.left, pt.y - r.top)));
            0
        }
        WM_MOVING => {
            let r = &mut *(lp as *mut RECT);
            if let Some(new) = with_app(|a| drag_rect(a, hwnd, r)) {
                *r = new;
            }
            1
        }
        // Moved to a monitor with another scaling, or the scaling changed.
        WM_DPICHANGED => {
            let r = *(lp as *const RECT);
            with_app(|a| a.index_of(hwnd).map(|i| a.rescale(i, Some(r))));
            0
        }
        // Monitors were added/removed/rearranged: bring lost widgets back.
        WM_DISPLAYCHANGE => {
            with_app(|a| {
                for i in 0..a.widgets.len() {
                    a.rescale(i, None);
                    ensure_visible(a.widgets[i].hwnd);
                }
            });
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

pub fn run(dir: PathBuf) {
    crate::log::init(dir.join("breezedesk.log"));
    tray::migrate_from_plume();
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        MAIN_THREAD.store(GetCurrentThreadId(), Ordering::Relaxed);

        let class = wide("BreezedeskWidget");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: GetModuleHandleW(null()),
            hIcon: null_mut(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class.as_ptr(),
            hIconSm: null_mut(),
        };
        RegisterClassExW(&wc);
    }

    // Invisible window that only exists to receive tray events and the file watch timer.
    let tray_hwnd = unsafe {
        let tb = wide("TaskbarCreated");
        WM_TASKBAR_CREATED.store(RegisterWindowMessageW(tb.as_ptr()), Ordering::Relaxed);
        let class = wide("BreezedeskWidget");
        let title = wide("BreezedeskTray");
        let h = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        );
        if !h.is_null() {
            SetTimer(h, TIMER_WATCH, 1000, None);
        }
        h
    };

    let state = load_state(&dir);
    APP.with(|a| {
        *a.borrow_mut() = Some(App {
            dir,
            widgets: Vec::new(),
            metrics: Metrics::new(),
            fonts: Fonts::default(),
            images: Images::default(),
            web: Web::default(),
            state,
            hidden: false,
            locked: tray::positions_locked(),
            drag_offset: None,
            desktop_window: null_mut(),
            tray: (!tray_hwnd.is_null()).then(|| Tray::new(tray_hwnd)),
            watch: Vec::new(),
            desktop_shown: false,
            update: None,
            updating: false,
        })
    });
    with_app(App::load);
    // Automatic update checks: shortly after start, then every few hours.
    if !update::DEV_BUILD {
        std::thread::spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_secs(20));
            check_for_update(false);
            std::thread::sleep(std::time::Duration::from_secs(UPDATE_CHECK_HOURS * 3600));
        });
    }
    // Get told when the foreground window changes (no polling).
    let hook = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            null_mut(),
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };

    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if msg.hwnd.is_null() {
                match msg.message {
                    WM_APP_RELOAD => {
                        with_app(App::load);
                        continue;
                    }
                    WM_APP_UPDATE_FOUND | WM_APP_UPDATE_NONE | WM_APP_UPDATE_READY | WM_APP_UPDATE_FAILED => {
                        let (m, w) = (msg.message, msg.wParam);
                        with_app(|a| a.on_update_message(m, w));
                        continue;
                    }
                    WM_APP_ACTIONS => {
                        process_actions();
                        continue;
                    }
                    _ => {}
                }
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    if !hook.is_null() {
        unsafe { UnhookWinEvent(hook) };
    }
    // Destroy the windows before exiting.
    APP.with(|a| a.borrow_mut().take());
}
