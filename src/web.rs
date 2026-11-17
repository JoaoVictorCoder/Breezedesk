//! Data downloaded from the internet (`[[web]]`): JSON APIs, RSS/Atom feeds or
//! plain text. A single background thread does the downloads; widgets read the
//! latest result with dotted paths like `weather.current.temperature_2m`.
//!
//! On Windows the download uses WinHTTP (built into Windows: no TLS library in
//! the executable). Elsewhere (only used by `--preview`) it shells out to curl.

use crate::config::WebFormat;
use serde_json::Value as Json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Job {
    /// "widget/name"
    pub key: String,
    pub url: String,
    pub interval: Duration,
    pub format: WebFormat,
}

#[derive(Default)]
struct Shared {
    jobs: Vec<(Job, Option<Instant>)>,
    data: HashMap<String, Json>,
}

#[derive(Default, Clone)]
pub struct Web {
    shared: Arc<Mutex<Shared>>,
    started: Arc<std::sync::atomic::AtomicBool>,
}

impl Web {
    /// Replaces the job list (after a reload). Data already downloaded is kept.
    pub fn set_jobs(&self, jobs: Vec<Job>) {
        let mut s = self.shared.lock().unwrap();
        let old = std::mem::take(&mut s.jobs);
        s.jobs = jobs
            .into_iter()
            .map(|j| {
                let last = old.iter().find(|(o, _)| o.key == j.key && o.url == j.url).and_then(|(_, t)| *t);
                (j, last)
            })
            .collect();
        let has_jobs = !s.jobs.is_empty();
        drop(s);
        if has_jobs && !self.started.swap(true, std::sync::atomic::Ordering::SeqCst) {
            let web = self.clone();
            std::thread::spawn(move || web.run());
        }
    }

    fn run(&self) {
        loop {
            let due: Vec<Job> = {
                let mut s = self.shared.lock().unwrap();
                s.jobs
                    .iter_mut()
                    .filter(|(j, last)| last.is_none_or(|t| t.elapsed() >= j.interval))
                    .map(|(j, last)| {
                        *last = Some(Instant::now());
                        j.clone()
                    })
                    .collect()
            };
            for job in due {
                self.fetch_now(&job);
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    /// Downloads and stores one job (blocking).
    pub fn fetch_now(&self, job: &Job) {
        match get(&job.url).and_then(|body| parse(&body, job.format)) {
            Ok(v) => {
                self.shared.lock().unwrap().data.insert(job.key.clone(), v);
            }
            Err(e) => crate::log::log(format!("web {}: {e}", job.key)),
        }
    }

    /// Value at `path` inside the data of `key` (`None` if missing).
    pub fn lookup(&self, key: &str, path: &[&str]) -> Option<Json> {
        let s = self.shared.lock().unwrap();
        json_path(s.data.get(key)?, path).cloned()
    }

    pub fn has(&self, key: &str) -> bool {
        self.shared.lock().unwrap().data.contains_key(key)
    }
}

pub fn json_path<'a>(mut v: &'a Json, path: &[&str]) -> Option<&'a Json> {
    for p in path {
        v = match v {
            Json::Object(m) => m.get(*p)?,
            Json::Array(a) => a.get(p.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(v)
}

pub fn parse(body: &str, format: WebFormat) -> Result<Json, String> {
    match format {
        WebFormat::Json => serde_json::from_str(body).map_err(|e| format!("invalid JSON: {e}")),
        WebFormat::Text => Ok(Json::String(body.to_string())),
        WebFormat::Rss => parse_feed(body),
    }
}

/// RSS 2.0 or Atom → `{ title, items: [{ title, link, description, date }] }`.
pub fn parse_feed(body: &str) -> Result<Json, String> {
    let doc = roxmltree::Document::parse(body).map_err(|e| format!("invalid XML: {e}"))?;
    let root = doc.root_element();
    let child_text = |n: roxmltree::Node, name: &str| -> String {
        n.children()
            .find(|c| c.tag_name().name() == name)
            .map(|c| c.descendants().filter(|d| d.is_text()).filter_map(|d| d.text()).collect::<String>())
            .map(|s| strip_html(s.trim()))
            .unwrap_or_default()
    };
    let (channel, item_tag) = if root.tag_name().name() == "feed" {
        (root, "entry")
    } else {
        let ch = root.descendants().find(|n| n.tag_name().name() == "channel").unwrap_or(root);
        (ch, "item")
    };
    let items: Vec<Json> = root
        .descendants()
        .filter(|n| n.tag_name().name() == item_tag)
        .map(|it| {
            let link = it
                .children()
                .find(|c| c.tag_name().name() == "link")
                .map(|l| l.attribute("href").map(str::to_string).unwrap_or_else(|| l.text().unwrap_or("").trim().to_string()))
                .unwrap_or_default();
            let mut description = child_text(it, "description");
            if description.is_empty() {
                description = child_text(it, "summary");
            }
            let mut date = child_text(it, "pubDate");
            if date.is_empty() {
                date = child_text(it, "updated");
            }
            serde_json::json!({
                "title": child_text(it, "title"),
                "link": link,
                "description": description,
                "date": date,
            })
        })
        .collect();
    Ok(serde_json::json!({ "title": child_text(channel, "title"), "count": items.len(), "items": items }))
}

fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">")
}

#[cfg(not(windows))]
fn get(url: &str) -> Result<String, String> {
    let out = std::process::Command::new("curl")
        .args(["-sSL", "--max-time", "20", url])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(windows)]
fn get(url: &str) -> Result<String, String> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Networking::WinHttp::*;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    struct H(*mut std::ffi::c_void);
    impl Drop for H {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { WinHttpCloseHandle(self.0) };
            }
        }
    }
    let err = |what: &str| format!("{what} failed ({})", std::io::Error::last_os_error());

    unsafe {
        let wurl = wide(url);
        let mut host = vec![0u16; 256];
        let mut path = vec![0u16; 4096];
        let mut uc: URL_COMPONENTS = std::mem::zeroed();
        uc.dwStructSize = std::mem::size_of::<URL_COMPONENTS>() as u32;
        uc.lpszHostName = host.as_mut_ptr();
        uc.dwHostNameLength = host.len() as u32;
        uc.lpszUrlPath = path.as_mut_ptr();
        uc.dwUrlPathLength = path.len() as u32;
        let mut extra = vec![0u16; 4096];
        uc.lpszExtraInfo = extra.as_mut_ptr();
        uc.dwExtraInfoLength = extra.len() as u32;
        if WinHttpCrackUrl(wurl.as_ptr(), 0, 0, &mut uc) == 0 {
            return Err(err("invalid URL"));
        }
        let mut full_path: Vec<u16> = path[..uc.dwUrlPathLength as usize].to_vec();
        full_path.extend_from_slice(&extra[..uc.dwExtraInfoLength as usize]);
        full_path.push(0);
        host.truncate(uc.dwHostNameLength as usize);
        host.push(0);

        let agent = wide("Plume/0.4.1");
        let session = H(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, null(), null(), 0));
        if session.0.is_null() {
            return Err(err("WinHttpOpen"));
        }
        WinHttpSetTimeouts(session.0, 10_000, 10_000, 15_000, 20_000);
        let conn = H(WinHttpConnect(session.0, host.as_ptr(), uc.nPort, 0));
        if conn.0.is_null() {
            return Err(err("connect"));
        }
        let secure = if uc.nScheme == WINHTTP_INTERNET_SCHEME_HTTPS { WINHTTP_FLAG_SECURE } else { 0 };
        let verb = wide("GET");
        let req = H(WinHttpOpenRequest(conn.0, verb.as_ptr(), full_path.as_ptr(), null(), null(), null(), secure));
        if req.0.is_null() {
            return Err(err("open request"));
        }
        if WinHttpSendRequest(req.0, null(), 0, null(), 0, 0, 0) == 0 || WinHttpReceiveResponse(req.0, null_mut()) == 0 {
            return Err(err("request"));
        }
        let mut status = 0u32;
        let mut len = 4u32;
        WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(),
            &mut status as *mut u32 as *mut _,
            &mut len,
            null_mut(),
        );
        let mut body = Vec::new();
        loop {
            let mut avail = 0u32;
            if WinHttpQueryDataAvailable(req.0, &mut avail) == 0 {
                return Err(err("read"));
            }
            if avail == 0 {
                break;
            }
            let start = body.len();
            body.resize(start + avail as usize, 0);
            let mut read = 0u32;
            if WinHttpReadData(req.0, body[start..].as_mut_ptr() as *mut _, avail, &mut read) == 0 {
                return Err(err("read"));
            }
            body.truncate(start + read as usize);
            if body.len() > 8 * 1024 * 1024 {
                return Err("response too large".into());
            }
        }
        if !(200..300).contains(&status) {
            return Err(format!("HTTP {status}"));
        }
        Ok(String::from_utf8_lossy(&body).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rss_and_atom() {
        let rss = r#"<?xml version="1.0"?><rss><channel><title>News</title>
            <item><title>First &amp; best</title><link>https://a.example/1</link>
            <description><![CDATA[<p>Hello <b>world</b></p>]]></description></item>
            <item><title>Second</title><link>https://a.example/2</link></item></channel></rss>"#;
        let v = parse_feed(rss).unwrap();
        assert_eq!(v["title"], "News");
        assert_eq!(v["items"][0]["title"], "First & best");
        assert_eq!(v["items"][0]["description"], "Hello world");
        assert_eq!(v["items"][1]["link"], "https://a.example/2");
        assert_eq!(v["count"], 2);

        let atom = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Blog</title>
            <entry><title>Post</title><link href="https://b.example/p"/><summary>Sum</summary></entry></feed>"#;
        let v = parse_feed(atom).unwrap();
        assert_eq!(v["items"][0]["link"], "https://b.example/p");
        assert_eq!(v["items"][0]["description"], "Sum");
    }

    #[test]
    fn paths() {
        let v: Json = serde_json::json!({"a": {"b": [10, {"c": "x"}]}});
        assert_eq!(json_path(&v, &["a", "b", "1", "c"]), Some(&Json::String("x".into())));
        assert_eq!(json_path(&v, &["a", "z"]), None);
    }
}
