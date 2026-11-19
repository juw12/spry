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

/// Binding the port is also the lock that keeps Spry to one running copy.
pub fn bind() -> Option<Arc<Server>> {
    Server::http(("127.0.0.1", port())).ok().map(Arc::new)
}

pub fn url(token: &str) -> String {
    format!("http://127.0.0.1:{}/?t={token}", port())
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

    let (code, kind, body): (u16, &str, String) = match path {
        _ if !authed => (403, "text/plain", "Open Spry from its tray icon, or run: spry open".into()),
        "/" => (200, "text/html; charset=utf-8", DASHBOARD.replace("__TOKEN__", &shared.token)),
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

    let mut response = Response::from_string(body).with_status_code(code);
    for (name, value) in [("Content-Type", kind), ("Cache-Control", "no-store"), ("X-Content-Type-Options", "nosniff")]
    {
        if let Ok(header) = Header::from_bytes(name, value) {
            response.add_header(header);
        }
    }
    let _ = request.respond(response);
}
