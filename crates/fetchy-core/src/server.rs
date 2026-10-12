//! Tiny loopback-only HTTP API used by the browser extension.
//!
//! Threat model: any web page can fire requests at 127.0.0.1. So every call
//! that changes state needs a secret token (which a page can't know), the
//! `Origin` must be a browser-extension origin (or absent), and the `Host`
//! header must be loopback (defeats DNS rebinding).

use crate::{AddOptions, Manager};
use serde::Deserialize;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const MAX_REQ: usize = 256 * 1024;

pub fn load_or_create_token(dir: &Path) -> String {
    let p = dir.join("api_token");
    if let Ok(t) = std::fs::read_to_string(&p) {
        let t = t.trim().to_string();
        if t.len() >= 32 {
            return t;
        }
    }
    let t = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let _ = std::fs::write(&p, &t);
    t
}

#[derive(Deserialize)]
struct AddReq {
    url: String,
    referer: Option<String>,
    cookie: Option<String>,
    filename: Option<String>,
    user_agent: Option<String>,
    queue: Option<String>,
}

/// Binds the first free port in `port..port+10` and serves forever.
/// Returns the port actually bound.
pub async fn start(m: Manager, port: u16, token: String) -> std::io::Result<u16> {
    let mut last = None;
    for p in port..port.saturating_add(10) {
        match TcpListener::bind(("127.0.0.1", p)).await {
            Ok(l) => {
                let bound = l.local_addr()?.port();
                tokio::spawn(async move {
                    loop {
                        let Ok((s, _)) = l.accept().await else { return };
                        let (m, t) = (m.clone(), token.clone());
                        tokio::spawn(async move {
                            let _ = tokio::time::timeout(
                                std::time::Duration::from_secs(10),
                                handle(s, m, t),
                            )
                            .await;
                        });
                    }
                });
                return Ok(bound);
            }
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no port")))
}

struct Req {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Req {
    fn header(&self, k: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(h, _)| h == k)
            .map(|(_, v)| v.as_str())
    }
}

async fn read_req(s: &mut TcpStream) -> Option<Req> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_REQ {
            return None;
        }
        let n = s.read(&mut tmp).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let mut first = lines.next()?.split_whitespace();
    let (method, path) = (first.next()?.to_string(), first.next()?.to_string());
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let len: usize = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    if len > MAX_REQ {
        return None;
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = s.read(&mut tmp).await.ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(len);
    Some(Req {
        method,
        path,
        headers,
        body,
    })
}

fn extension_origin(o: &str) -> bool {
    [
        "chrome-extension://",
        "moz-extension://",
        "safari-web-extension://",
    ]
    .iter()
    .any(|p| o.starts_with(p))
}

fn loopback_host(h: &str) -> bool {
    let host = h.rsplit_once(':').map(|(a, _)| a).unwrap_or(h);
    matches!(host, "127.0.0.1" | "localhost" | "[::1]")
}

fn eq_const(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

async fn handle(mut s: TcpStream, m: Manager, token: String) {
    let Some(req) = read_req(&mut s).await else {
        return;
    };
    let origin = req.header("origin").map(str::to_owned);
    let cors = origin.as_deref().filter(|o| extension_origin(o)).map(|o| {
        format!(
            "Access-Control-Allow-Origin: {o}\r\nAccess-Control-Allow-Headers: content-type, x-fetchy-token\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nVary: Origin\r\n"
        )
    });
    let (code, body) = route(&req, &m, &token, origin.as_deref()).await;
    let reason = match code {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Error",
    };
    let resp = format!(
        "HTTP/1.1 {code} {reason}\r\n{}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        cors.unwrap_or_default(),
        body.len()
    );
    let _ = s.write_all(resp.as_bytes()).await;
    let _ = s.shutdown().await;
}

async fn route(req: &Req, m: &Manager, token: &str, origin: Option<&str>) -> (u16, String) {
    let err = |c: u16, msg: &str| (c, serde_json::json!({ "error": msg }).to_string());
    if !req.header("host").is_some_and(loopback_host) {
        return err(403, "bad host");
    }
    if origin.is_some_and(|o| !extension_origin(o)) {
        return err(403, "origin not allowed");
    }
    if req.method == "OPTIONS" {
        return (204, String::new());
    }
    if req.method == "GET" && req.path == "/ping" {
        return (
            200,
            serde_json::json!({ "app": "Fetchy", "version": env!("CARGO_PKG_VERSION") })
                .to_string(),
        );
    }
    if !req
        .header("x-fetchy-token")
        .is_some_and(|t| eq_const(t, token))
    {
        return err(401, "missing or wrong token");
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/status") => {
            let l = m.list();
            let active = l
                .iter()
                .filter(|i| i.status == crate::Status::Downloading)
                .count();
            (
                200,
                serde_json::json!({ "ok": true, "downloads": l.len(), "active": active })
                    .to_string(),
            )
        }
        ("POST", "/add") => {
            let Ok(a) = serde_json::from_slice::<AddReq>(&req.body) else {
                return err(400, "invalid json");
            };
            let opts = AddOptions {
                referer: a.referer,
                cookie: a.cookie,
                filename: a.filename.filter(|f| !f.trim().is_empty()),
                user_agent: a.user_agent,
                queue: a.queue,
                ..Default::default()
            };
            match m.add_smart(&a.url, opts).await {
                Ok(i) => (
                    200,
                    serde_json::json!({ "ok": true, "id": i.id }).to_string(),
                ),
                Err(e) => err(400, &e),
            }
        }
        _ => err(404, "not found"),
    }
}
