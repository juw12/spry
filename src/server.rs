//! The dashboard: one page and a small JSON API, on this computer only.
//! Every request must come to 127.0.0.1 and carry the secret token.

use crate::config::{Config, port};
use crate::engine::Shared;
use crate::{bench, clean, install};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tiny_http::{Header, Method, Request, Response, Server};

const DASHBOARD: &str = include_str!("dashboard.html");
const JSON: &str = "application/json";
const FONT: &str = "font/woff2";
/// The page's typefaces travel inside Spry, so it looks the same offline and asks nobody else.
const FONTS: [(&str, &[u8]); 3] = [
    ("/fonts/geist.woff2", include_bytes!("../assets/fonts/geist.woff2")),
    ("/fonts/geist-mono.woff2", include_bytes!("../assets/fonts/geist-mono.woff2")),
    ("/fonts/instrument-serif.woff2", include_bytes!("../assets/fonts/instrument-serif.woff2")),
];

/// Binding the port is also the lock that keeps Spry to one running copy.
pub fn bind() -> Option<Arc<Server>> {
    Server::http(("127.0.0.1", port())).ok().map(Arc::new)
}

pub fn url(token: &str) -> String {
    format!("http://127.0.0.1:{}/?t={token}", port())
}

/// One request to the Spry that is running on this computer, from another `spry` process.
/// None when it is not running, or does not know the path.
pub fn send(method: &str, path: &str) -> Option<String> {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port())).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let request = format!(
        "{method} {path}?t={} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        crate::config::token(),
        port()
    );
    stream.write_all(request.as_bytes()).ok()?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply).ok()?;
    let (head, body) = reply.split_once("\r\n\r\n")?;
    head.starts_with("HTTP/1.1 200").then(|| body.to_string())
}

pub fn serve(server: Arc<Server>, shared: Arc<Shared>) {
    // Two threads, so a storage scan does not hold up the live numbers.
    for _ in 0..2 {
        let (server, shared) = (server.clone(), shared.clone());
        std::thread::spawn(move || {
            while let Ok(request) = server.recv() {
                handle(request, &shared);
            }
        });
    }
}

fn handle(mut request: Request, shared: &Shared) {
    let url = request.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let param = |key: &str| {
        query.split('&').filter_map(|kv| kv.split_once('=')).find(|(k, _)| *k == key).map(|(_, v)| v.to_string())
    };
    let hosts = [format!("127.0.0.1:{}", port()), format!("localhost:{}", port())];
    let host_ok =
        request.headers().iter().any(|h| h.field.equiv("Host") && hosts.iter().any(|x| h.value == x.as_str()));
    let authed = host_ok && param("t").as_deref() == Some(shared.token.as_str());
    let post = *request.method() == Method::Post;
    let ok = || (200, JSON, r#"{"ok":true}"#.to_string());
    let font = FONTS.iter().find(|(name, _)| *name == path).map(|(_, bytes)| *bytes);

    let (code, kind, body): (u16, &str, String) = match path {
        _ if !authed => (403, "text/plain", "Open Spry from its tray icon, or run: spry open".into()),
        _ if font.is_some() => (200, FONT, String::new()),
        "/" => (200, "text/html; charset=utf-8", DASHBOARD.replace("__TOKEN__", &shared.token)),
        // Asked by a second start and by `spry open`: bring the window to the front. The answer says
        // whether this Spry has a window; without one the asker opens the page in the browser.
        "/api/show" if post => {
            let window = shared.window.load(Ordering::Relaxed);
            if window {
                shared.show.store(true, Ordering::Relaxed);
            }
            (200, JSON, format!(r#"{{"ok":true,"window":{window}}}"#))
        }
        "/api/status" => {
            *shared.watched.lock().unwrap() = Some(Instant::now());
            (200, JSON, shared.status.lock().unwrap().clone())
        }
        "/api/config" if post => {
            let mut text = String::new();
            let _ = request.as_reader().read_to_string(&mut text);
            match serde_json::from_str::<Config>(&text) {
                Ok(cfg) => {
                    let cfg = cfg.sanitize();
                    cfg.save();
                    *shared.cfg.lock().unwrap() = cfg;
                    ok()
                }
                Err(e) => (400, "text/plain", e.to_string()),
            }
        }
        "/api/pause" if post => {
            let secs: u64 = param("secs").and_then(|s| s.parse().ok()).unwrap_or(0).min(24 * 3600);
            *shared.pause_until.lock().unwrap() = (secs > 0).then(|| Instant::now() + Duration::from_secs(secs));
            ok()
        }
        "/api/quit" if post => {
            shared.quit.store(true, Ordering::Relaxed);
            ok()
        }
        "/api/autostart" if post => {
            let on = param("on").as_deref() == Some("1");
            let done = if on { install::install().map(|_| ()) } else { install::uninstall() };
            shared.autostart.store(install::is_installed(), Ordering::Relaxed);
            match done {
                Ok(()) => ok(),
                Err(e) => (500, "text/plain", e.to_string()),
            }
        }
        "/api/storage" => {
            let list = if post {
                let ids: Vec<String> = param("ids").unwrap_or_default().split(',').map(str::to_string).collect();
                clean::clean(&ids)
            } else {
                clean::scan()
            };
            (200, JSON, serde_json::to_string(&list).unwrap_or_default())
        }
        "/api/bench" if post => match bench::run(4.0, bench::default_burners()) {
            Ok(report) => (200, JSON, serde_json::to_string(&report).unwrap_or_default()),
            Err(e) => (500, "text/plain", e.to_string()),
        },
        _ => (404, "text/plain", "not found".into()),
    };

    // A typeface never changes within a version, so the page need not fetch it on every visit.
    let (bytes, cache) = match font {
        Some(bytes) if code == 200 => (bytes.to_vec(), "private, max-age=86400"),
        _ => (body.into_bytes(), "no-store"),
    };
    let mut response = Response::from_data(bytes).with_status_code(code);
    for (name, value) in [("Content-Type", kind), ("Cache-Control", cache), ("X-Content-Type-Options", "nosniff")] {
        if let Ok(header) = Header::from_bytes(name, value) {
            response.add_header(header);
        }
    }
    let _ = request.respond(response);
}
