//! Ícone na bandeja do sistema (área de notificação) e "Iniciar com o Windows".

use std::ptr::{null, null_mut};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Transform};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// Mensagem que o Windows manda à nossa janela quando o ícone é clicado.
pub const WM_APP_TRAY: u32 = WM_APP + 2;
const TRAY_ID: u32 = 1;

pub struct Tray {
    hwnd: HWND,
    icon: HICON,
}

impl Tray {
    pub fn new(hwnd: HWND) -> Tray {
        let tray = Tray { hwnd, icon: make_icon() };
        tray.add();
        tray
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = self.hwnd;
        nid.uID = TRAY_ID;
        nid
    }

    /// Adiciona o ícone. Chamado de novo se o Explorer reiniciar.
    pub fn add(&self) {
        let mut nid = self.data();
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_APP_TRAY;
        nid.hIcon = self.icon;
        for (d, s) in nid.szTip.iter_mut().zip("Pluma".encode_utf16()) {
            *d = s;
        }
        unsafe { Shell_NotifyIconW(NIM_ADD, &nid) };
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let nid = self.data();
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &nid);
            DestroyIcon(self.icon);
        }
    }
}

/// Desenha o ícone em tempo de execução (sem arquivo .ico): um mini gráfico de barras.
fn make_icon() -> HICON {
    let s = unsafe { GetSystemMetrics(SM_CXSMICON) }.clamp(16, 64) as u32;
    let Some(mut pm) = Pixmap::new(s, s) else {
        return unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };
    };
    let f = s as f32 / 16.0; // escala a partir de um desenho em 16x16
    let mut paint = Paint { anti_alias: true, ..Paint::default() };

    paint.set_color(Color::from_rgba8(30, 34, 48, 255));
    if let Some(p) = rounded(0.5 * f, 0.5 * f, 15.0 * f, 15.0 * f, 3.5 * f) {
        pm.fill_path(&p, &paint, FillRule::Winding, Transform::identity(), None);
    }
    let bars = [(3.0, 9.0, (90, 200, 250)), (6.75, 5.0, (90, 200, 250)), (10.5, 7.0, (255, 159, 10))];
    for (x, top, (r, g, b)) in bars {
        paint.set_color(Color::from_rgba8(r, g, b, 255));
        if let Some(rect) = Rect::from_xywh(x * f, top * f, 2.5 * f, (13.0 - top) * f) {
            pm.fill_rect(rect, &paint, Transform::identity(), None);
        }
    }

    unsafe {
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = s as i32;
        bi.bmiHeader.biHeight = -(s as i32);
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB;
        let mut bits = null_mut();
        let color = CreateDIBSection(null_mut(), &bi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if color.is_null() || bits.is_null() {
            return LoadIconW(null_mut(), IDI_APPLICATION);
        }
        // Ícones usam alfa "direto" (não pré-multiplicado), em BGRA.
        let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (s * s * 4) as usize);
        for (d, p) in dst.chunks_exact_mut(4).zip(pm.pixels()) {
            let c = p.demultiply();
            d.copy_from_slice(&[c.blue(), c.green(), c.red(), c.alpha()]);
        }
        let mask = CreateBitmap(s as i32, s as i32, 1, 1, null());
        let info = ICONINFO { fIcon: TRUE, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info);
        DeleteObject(mask);
        DeleteObject(color);
        if icon.is_null() {
            LoadIconW(null_mut(), IDI_APPLICATION)
        } else {
            icon
        }
    }
}

fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let k = 0.552_284_8 * r;
    let (r2, b) = (x + w, y + h);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(r2 - r, y);
    pb.cubic_to(r2 - r + k, y, r2, y + r - k, r2, y + r);
    pb.line_to(r2, b - r);
    pb.cubic_to(r2, b - r + k, r2 - r + k, b, r2 - r, b);
    pb.line_to(x + r, b);
    pb.cubic_to(x + r - k, b, x, b - r + k, x, b - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

// ---------- Iniciar com o Windows (HKCU\...\Run) ----------

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "Pluma";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn open_run_key(access: u32) -> Option<HKEY> {
    let key = wide(RUN_KEY);
    let mut h: HKEY = null_mut();
    let ok = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, access, &mut h) };
    (ok == ERROR_SUCCESS).then_some(h)
}

pub fn autostart_enabled() -> bool {
    let Some(h) = open_run_key(KEY_QUERY_VALUE) else { return false };
    let name = wide(RUN_VALUE);
    let ok = unsafe { RegQueryValueExW(h, name.as_ptr(), null(), null_mut(), null_mut(), null_mut()) };
    unsafe { RegCloseKey(h) };
    ok == ERROR_SUCCESS
}

pub fn set_autostart(on: bool) {
    let Some(h) = open_run_key(KEY_SET_VALUE) else { return };
    let name = wide(RUN_VALUE);
    unsafe {
        if on {
            if let Ok(exe) = std::env::current_exe() {
                let cmd = wide(&format!("\"{}\"", exe.display()));
                RegSetValueExW(
                    h,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    cmd.as_ptr() as *const u8,
                    (cmd.len() * 2) as u32,
                );
            }
        } else {
            RegDeleteValueW(h, name.as_ptr());
        }
        RegCloseKey(h);
    }
}
