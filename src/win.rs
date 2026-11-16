//! Windows-specific part: one "layered" window per widget.
//!
//! Each widget is a borderless WS_POPUP + WS_EX_LAYERED window. Its content is
//! drawn into an in-memory buffer and pushed with `UpdateLayeredWindow`, which
//! supports per-pixel transparency: the same trick Rainmeter uses, but without
//! Lua, without plugins loaded for nothing and without GDI+.

use crate::config::{self, WidgetConfig, ZOrder};
use crate::metrics::Metrics;
use crate::render::{self, Fonts};
use crate::tray::{self, Tray, WM_APP_TRAY};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::ptr::{null, null_mut};
use tiny_skia::Pixmap;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const TIMER_ID: usize = 1;
const WM_APP_RELOAD: u32 = WM_APP + 1;
const MENU_RELOAD: usize = 1;
const MENU_EDIT: usize = 2;
const MENU_EXIT: usize = 3;
const MENU_TOGGLE: usize = 4;
const MENU_AUTOSTART: usize = 5;

/// "TaskbarCreated": Explorer restarted and the tray icon must be re-added.
static WM_TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
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

struct Widget {
    cfg: WidgetConfig,
    hwnd: HWND,
    pixmap: Pixmap,
    surface: Surface,
}

impl Drop for Widget {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.hwnd) };
    }
}

struct App {
    dir: PathBuf,
    widgets: Vec<Widget>,
    metrics: Metrics,
    fonts: Fonts,
    positions: HashMap<String, (i32, i32)>,
    hidden: bool,
    tray: Option<Tray>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Accesses the global state without panicking if it is already borrowed
/// (some Win32 calls re-enter the wndproc).
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
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

fn message_box(text: &str) {
    let (t, c) = (wide(text), wide("Plume"));
    unsafe { MessageBoxW(null_mut(), t.as_ptr(), c.as_ptr(), MB_ICONWARNING | MB_OK) };
}

impl App {
    fn load(&mut self) {
        self.widgets.clear(); // Drop destroys the old windows
        let (cfgs, errors) = config::load_dir(&self.dir);
        if !errors.is_empty() {
            message_box(&format!("Errors in widget files:\n\n{}", errors.join("\n\n")));
        }
        for cfg in cfgs {
            match create_widget(cfg, &self.positions) {
                Some(w) => self.widgets.push(w),
                None => message_box("Failed to create a widget window."),
            }
        }
        for i in 0..self.widgets.len() {
            self.paint(i);
        }
        if self.hidden {
            for w in &self.widgets {
                unsafe { ShowWindow(w.hwnd, SW_HIDE) };
            }
        }
    }

    fn set_hidden(&mut self, hidden: bool) {
        self.hidden = hidden;
        for w in &self.widgets {
            unsafe {
                if hidden {
                    ShowWindow(w.hwnd, SW_HIDE);
                } else {
                    ShowWindow(w.hwnd, SW_SHOWNOACTIVATE);
                    if w.cfg.z == ZOrder::Bottom {
                        SetWindowPos(w.hwnd, HWND_BOTTOM, 0, 0, 0, 0,
                            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                    }
                }
            }
        }
        if !hidden {
            for i in 0..self.widgets.len() {
                self.paint(i);
            }
        }
    }

    fn index_of(&self, hwnd: HWND) -> Option<usize> {
        self.widgets.iter().position(|w| w.hwnd == hwnd)
    }

    fn paint(&mut self, i: usize) {
        if self.hidden {
            return; // no CPU spent on invisible widgets
        }
        self.metrics.refresh();
        let w = &mut self.widgets[i];
        render::render(&w.cfg, &mut w.pixmap, &self.metrics, &mut self.fonts);

        // tiny-skia uses premultiplied RGBA; Windows wants premultiplied BGRA.
        let src = w.pixmap.data();
        let dst = unsafe { std::slice::from_raw_parts_mut(w.surface.bits, src.len()) };
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }

        let size = SIZE { cx: w.cfg.width as i32, cy: w.cfg.height as i32 };
        let origin = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: w.cfg.opacity,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            UpdateLayeredWindow(
                w.hwnd,
                null_mut(),
                null(), // keep the current position
                &size,
                w.surface.dc,
                &origin,
                0,
                &blend,
                ULW_ALPHA,
            );
        }
    }
}

fn create_widget(cfg: WidgetConfig, positions: &HashMap<String, (i32, i32)>) -> Option<Widget> {
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
    let hwnd = unsafe {
        CreateWindowExW(
            ex,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            x,
            y,
            cfg.width as i32,
            cfg.height as i32,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        )
    };
    if hwnd.is_null() {
        return None;
    }
    let pixmap = Pixmap::new(cfg.width.max(1), cfg.height.max(1))?;
    let surface = Surface::new(cfg.width.max(1), cfg.height.max(1))?;
    unsafe {
        let insert_after = if cfg.z == ZOrder::Bottom { HWND_BOTTOM } else { null_mut() };
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW
            | if cfg.z == ZOrder::Bottom { 0 } else { SWP_NOZORDER };
        SetWindowPos(hwnd, insert_after, 0, 0, 0, 0, flags);
        SetTimer(hwnd, TIMER_ID, cfg.update_ms.max(16), None);
    }
    Some(Widget { cfg, hwnd, pixmap, surface })
}

fn show_menu(hwnd: HWND) {
    unsafe {
        let hidden = with_app(|a| a.hidden).unwrap_or(false);
        let autostart = tray::autostart_enabled();
        let menu = CreatePopupMenu();
        let items: [(usize, &str, u32); 7] = [
            (MENU_TOGGLE, if hidden { "Show widgets" } else { "Hide widgets" }, MF_STRING),
            (0, "", MF_SEPARATOR),
            (MENU_RELOAD, "Reload widgets", MF_STRING),
            (MENU_EDIT, "Open widgets folder", MF_STRING),
            (MENU_AUTOSTART, "Start with Windows",
                MF_STRING | if autostart { MF_CHECKED } else { MF_UNCHECKED }),
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
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            0,
            hwnd,
            null(),
        ) as usize;
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
                    let _ = std::process::Command::new("explorer").arg(dir).spawn();
                }
            }
            MENU_EXIT => PostQuitMessage(0),
            _ => {}
        }
    }
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
            with_app(|a| {
                if let Some(t) = &a.tray {
                    t.add();
                }
            });
            0
        }
        WM_TIMER if wp == TIMER_ID => {
            with_app(|a| {
                if let Some(i) = a.index_of(hwnd) {
                    a.paint(i);
                }
            });
            0
        }
        // Clicking anywhere = "title bar", so Windows handles the dragging.
        WM_NCHITTEST => {
            let drag = with_app(|a| a.index_of(hwnd).map(|i| a.widgets[i].cfg.draggable));
            if drag.flatten().unwrap_or(false) {
                HTCAPTION as LRESULT
            } else {
                HTCLIENT as LRESULT
            }
        }
        WM_NCRBUTTONUP | WM_RBUTTONUP => {
            show_menu(hwnd);
            0
        }
        // "bottom" widgets never rise in the window stack.
        WM_WINDOWPOSCHANGING => {
            let bottom = with_app(|a| {
                a.index_of(hwnd).map(|i| a.widgets[i].cfg.z == ZOrder::Bottom)
            });
            if bottom.flatten().unwrap_or(false) {
                let wp = &mut *(lp as *mut WINDOWPOS);
                wp.hwndInsertAfter = HWND_BOTTOM;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        // Drag finished: save the position.
        WM_EXITSIZEMOVE => {
            let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            GetWindowRect(hwnd, &mut r);
            with_app(|a| {
                if let Some(i) = a.index_of(hwnd) {
                    let name = a.widgets[i].cfg.name.clone();
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
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

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

    // Invisible window that only exists to receive tray events.
    let tray_hwnd = unsafe {
        let tb = wide("TaskbarCreated");
        WM_TASKBAR_CREATED.store(RegisterWindowMessageW(tb.as_ptr()), Ordering::Relaxed);
        let class = wide("PlumeWidget");
        let title = wide("PlumeTray");
        CreateWindowExW(0, class.as_ptr(), title.as_ptr(), WS_POPUP, 0, 0, 0, 0,
            null_mut(), null_mut(), GetModuleHandleW(null()), null())
    };

    let positions = load_positions(&dir);
    APP.with(|a| {
        *a.borrow_mut() = Some(App {
            dir,
            widgets: Vec::new(),
            metrics: Metrics::new(),
            fonts: Fonts::default(),
            positions,
            hidden: false,
            tray: (!tray_hwnd.is_null()).then(|| Tray::new(tray_hwnd)),
        })
    });
    with_app(App::load);

    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if msg.hwnd.is_null() && msg.message == WM_APP_RELOAD {
                with_app(App::load);
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    // Destroy the windows before exiting.
    APP.with(|a| a.borrow_mut().take());
}
