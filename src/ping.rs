//! ping('host'): round-trip time in milliseconds (None when there is no answer).

use std::net::{IpAddr, ToSocketAddrs};

fn resolve(host: &str) -> Option<std::net::Ipv4Addr> {
    (host, 0).to_socket_addrs().ok()?.find_map(|a| match a.ip() {
        IpAddr::V4(v4) => Some(v4),
        IpAddr::V6(_) => None,
    })
}

#[cfg(windows)]
pub fn ping(host: &str) -> Option<f64> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY};
    let ip = resolve(host)?;
    unsafe {
        let h = IcmpCreateFile();
        if h.is_null() || h as isize == -1 {
            return None;
        }
        let data = *b"plume-ping";
        let mut reply = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + data.len() + 16];
        let n = IcmpSendEcho(
            h,
            u32::from_ne_bytes(ip.octets()),
            data.as_ptr() as *const _,
            data.len() as u16,
            std::ptr::null(),
            reply.as_mut_ptr() as *mut _,
            reply.len() as u32,
            1000,
        );
        IcmpCloseHandle(h);
        if n == 0 {
            return None;
        }
        let r = &*(reply.as_ptr() as *const ICMP_ECHO_REPLY);
        (r.Status == 0).then_some(r.RoundTripTime as f64)
    }
}

#[cfg(not(windows))]
pub fn ping(host: &str) -> Option<f64> {
    let ip = resolve(host)?;
    let out = std::process::Command::new("ping").args(["-c", "1", "-W", "1", &ip.to_string()]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let t = text.split("time=").nth(1)?;
    t.split_whitespace().next()?.parse().ok()
}
