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
use crate::web::Web;
use crate::widget::{Mouse, Widget};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::SystemTime;
use tiny_skia::Pixmap;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows_sys::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const TIMER_UPDATE: usize = 1;
const TIMER_WATCH: usize = 2;
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
    pixmap: Pixmap,
    surface: Surface,
    shown: bool,
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
    positions: HashMap<String, (i32, i32)>,
    hidden: bool,
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

fn positions_path(dir: &Path) -> PathBuf {
    dir.join("positions.state") // not .toml, so it is not loaded as a widget
}

fn load_positions(dir: &Path) -> HashMap<String, (i32, i32)> {
    std::fs::read_to_string(positions_path(dir))
        .ok()
        .and_then(|s| toml::from_str::<HashMap<String, [i32; 2]>>(&s).ok())
        .map(|m| m.into_iter().map(|(k, [x, y])| (k, (x, y))).collect())
        .unwrap_or_default()
}

fn save_positions(dir: &Path, pos: &HashMap<String, (i32, i32)>) {
    let mut out = String::from("# Generated automatically when widgets are dragged.\n");
    let mut keys: Vec<_> = pos.keys().collect();
    keys.sort();
    for k in keys {
        let (x, y) = pos[k];
        out.push_str(&format!("{k:?} = [{x}, {y}]\n"));
    }
    let _ = std::fs::write(positions_path(dir), out);
}

/// Files that trigger an automatic reload when they change.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Option<SystemTime>, u64)> {
    fn walk(dir: &Path, depth: u32, out: &mut Vec<(PathBuf, Option<SystemTime>, u64)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_lowercase();
            if name.starts_with('.') || name == "positions.state" || name == "plume.log" {
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
            match create_window(w, &self.positions) {
                Some(ww) => self.widgets.push(ww),
                None => errors.push("Failed to create a widget window.".into()),
            }
        }
        self.web.set_jobs(jobs);
        for e in &errors {
            crate::log::log(e);
        }
        if let Some(first) = errors.first() {
            let more = if errors.len() > 1 { format!("\n(+{} more, see plume.log)", errors.len() - 1) } else { String::new() };
            self.notify("Plume: widget error", &format!("{first}{more}"), true);
        }
        self.watch = snapshot(&self.dir);
        self.metrics.refresh();
        for i in 0..self.widgets.len() {
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
                        SetWindowPos(w.hwnd, HWND_BOTTOM, 0, 0, 0, 0, flags);
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
                SetWindowPos(w.hwnd, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
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
            self.show(i, shown && !self.hidden);
            if shown {
                self.paint(i, true);
            }
        }
    }

    /// `update` = recompute data (timer); otherwise only redraw (hover).
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
        render::render(&mut ww.w, &mut ww.pixmap, &self.metrics, &self.web, &mut self.fonts, &mut self.images);

        // tiny-skia uses premultiplied RGBA; Windows wants premultiplied BGRA.
        let src = ww.pixmap.data();
        let dst = unsafe { std::slice::from_raw_parts_mut(ww.surface.bits, src.len()) };
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
        let cfg = &ww.w.cfg;
        let opacity = if ww.w.mouse_over { cfg.hover_opacity.unwrap_or(cfg.opacity) } else { cfg.opacity };
        let size = SIZE { cx: cfg.width as i32, cy: cfg.height as i32 };
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

    /// Mouse position (widget coordinates) → hover state; redraws if it changed.
    fn hover(&mut self, i: usize, pos: Option<(i32, i32)>) {
        let w = &mut self.widgets[i].w;
        if !w.has_hover_effects() {
            return;
        }
        let (over, el) = match pos {
            Some((x, y)) => (true, w.element_at(x as f32, y as f32)),
            None => (false, None),
        };
        if over != w.mouse_over || el != w.hovered {
            w.mouse_over = over;
            w.hovered = el;
            self.paint(i, false);
        }
    }

    fn mouse(&mut self, i: usize, x: i32, y: i32, button: Mouse) -> bool {
        let w = &self.widgets[i].w;
        let Some(el) = w.element_at(x as f32, y as f32) else { return false };
        let list = w.actions(el, button, &self.metrics, &self.web);
        if list.is_empty() {
            return false;
        }
        queue_actions(&w.cfg.name.clone(), list);
        true
    }
}

fn create_window(w: Widget, positions: &HashMap<String, (i32, i32)>) -> Option<WinWidget> {
    let cfg = &w.cfg;
    let (x, y) = positions.get(&cfg.name).copied().unwrap_or((cfg.x, cfg.y));
    let mut ex = WS_EX_LAYERED | WS_EX_TOOLWINDOW; // TOOLWINDOW = no taskbar button
    if cfg.z == ZOrder::Top {
        ex |= WS_EX_TOPMOST;
    }
    if cfg.click_through {
        ex |= WS_EX_TRANSPARENT;
    }
    let class = wide("PlumeWidget");
    let title = wide(&cfg.name);
    let (width, height) = (cfg.width.max(1), cfg.height.max(1));
    let hwnd = unsafe {
        CreateWindowExW(
            ex,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            x,
            y,
            width as i32,
            height as i32,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        )
    };
    if hwnd.is_null() {
        return None;
    }
    let pixmap = Pixmap::new(width, height)?;
    let surface = Surface::new(width, height)?;
    unsafe {
        if let Some(bg) = &cfg.background {
            if bg.blur != Blur::None {
                apply_blur(hwnd, bg, width, height);
            }
        }
        let insert_after = if cfg.z == ZOrder::Bottom { HWND_BOTTOM } else { null_mut() };
        let flags = SWP_NOMOVE
            | SWP_NOSIZE
            | SWP_NOACTIVATE
            | SWP_SHOWWINDOW
            | if cfg.z == ZOrder::Bottom { 0 } else { SWP_NOZORDER };
        SetWindowPos(hwnd, insert_after, 0, 0, 0, 0, flags);
        arm_timer(hwnd, cfg.update_ms);
    }
    Some(WinWidget { w, hwnd, pixmap, surface, shown: true })
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
unsafe fn apply_blur(hwnd: HWND, bg: &Background, w: u32, h: u32) {
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
        let d = (bg.radius * 2.0) as i32;
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

fn show_menu(hwnd: HWND) {
    unsafe {
        let hidden = with_app(|a| a.hidden).unwrap_or(false);
        let autostart = tray::autostart_enabled();
        let menu = CreatePopupMenu();
        let items: [(usize, &str, u32); 10] = [
            (MENU_TOGGLE, if hidden { "Show widgets" } else { "Hide widgets" }, MF_STRING),
            (0, "", MF_SEPARATOR),
            (MENU_RELOAD, "Reload widgets", MF_STRING),
            (MENU_EDIT, "Open widgets folder", MF_STRING),
            (MENU_EXAMPLES, "Install example widgets", MF_STRING),
            (MENU_LOG, "Open log", MF_STRING),
            (0, "", MF_SEPARATOR),
            (MENU_AUTOSTART, "Start with Windows", MF_STRING | if autostart { MF_CHECKED } else { MF_UNCHECKED }),
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
        DestroyMenu(menu);
        // Microsoft-recommended workaround for menus opened from the tray.
        PostMessageW(hwnd, WM_NULL, 0, 0);
        match cmd {
            MENU_TOGGLE => {
                with_app(|a| a.set_hidden(!a.hidden));
            }
            MENU_AUTOSTART => tray::set_autostart(!autostart),
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
                    with_app(|a| a.notify("Plume", &msg, false));
                    PostThreadMessageW(GetCurrentThreadId(), WM_APP_RELOAD, 0, 0);
                }
            }
            MENU_EXIT => PostQuitMessage(0),
            _ => {}
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

/// True for the desktop window that "show desktop" (Win+D) brings to the front.
fn is_desktop_window(hwnd: HWND) -> bool {
    let mut buf = [0u16; 32];
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    let class = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
    class == "WorkerW" || class == "Progman"
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

/// Updates the "show desktop" state from the window now in the foreground.
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
    let desktop = is_desktop_window(hwnd);
    with_app(|a| a.set_desktop_shown(desktop));
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
                let w = &a.widgets[i].w;
                Some(if w.element_at(x as f32, y as f32).is_some() || !w.cfg.draggable { HTCLIENT } else { HTCAPTION })
            });
            hit.flatten().unwrap_or(HTCLIENT) as LRESULT
        }
        WM_SETCURSOR => {
            let hand = with_app(|a| {
                let mut pt = POINT { x: 0, y: 0 };
                GetCursorPos(&mut pt);
                let (i, x, y) = locate(a, hwnd, pt.x, pt.y)?;
                let w = &a.widgets[i].w;
                let el = w.element_at(x as f32, y as f32)?;
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
                let cfg = &a.widgets[i].w.cfg;
                let inside = x >= 0 && y >= 0 && x < cfg.width as i32 && y < cfg.height as i32;
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
            if bottom.flatten().unwrap_or(false) {
                let wpos = &mut *(lp as *mut WINDOWPOS);
                wpos.hwndInsertAfter = HWND_BOTTOM;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        // Drag finished: save the position.
        WM_EXITSIZEMOVE => {
            let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            GetWindowRect(hwnd, &mut r);
            with_app(|a| {
                if let Some(i) = a.index_of(hwnd) {
                    let name = a.widgets[i].w.cfg.name.clone();
                    a.positions.insert(name, (r.left, r.top));
                    save_positions(&a.dir, &a.positions);
                }
            });
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

pub fn run(dir: PathBuf) {
    crate::log::init(dir.join("plume.log"));
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        MAIN_THREAD.store(GetCurrentThreadId(), Ordering::Relaxed);

        let class = wide("PlumeWidget");
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
        let class = wide("PlumeWidget");
        let title = wide("PlumeTray");
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

    let positions = load_positions(&dir);
    APP.with(|a| {
        *a.borrow_mut() = Some(App {
            dir,
            widgets: Vec::new(),
            metrics: Metrics::new(),
            fonts: Fonts::default(),
            images: Images::default(),
            web: Web::default(),
            positions,
            hidden: false,
            tray: (!tray_hwnd.is_null()).then(|| Tray::new(tray_hwnd)),
            watch: Vec::new(),
            desktop_shown: false,
        })
    });
    with_app(App::load);
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
