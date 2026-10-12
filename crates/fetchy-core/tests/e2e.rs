#![allow(clippy::field_reassign_with_default)] // tests tweak one or two fields of a default value
//! End-to-end tests against a local HTTP server that honours `Range` and
//! throttles each *connection* (like most real CDNs), so connection count matters.

use fetchy_core::{AddOptions, Manager, Status};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const SIZE: u64 = 24 * 1024 * 1024;

fn byte_at(i: u64) -> u8 {
    (i.wrapping_mul(31).wrapping_add(7) % 251) as u8
}

/// `per_conn_bps`: throttle for each connection. `ranges`: honour Range or not.
async fn serve(per_conn_bps: u64, ranges: bool) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut req = Vec::new();
                let mut b = [0u8; 1024];
                while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut b).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => req.extend_from_slice(&b[..n]),
                    }
                }
                let text = String::from_utf8_lossy(&req).to_string();
                let range = if ranges {
                    text.lines().find_map(|l| {
                        let l = l.to_ascii_lowercase();
                        let v = l.strip_prefix("range: bytes=")?.to_string();
                        let (a, e) = v.split_once('-')?;
                        let a: u64 = a.parse().ok()?;
                        let e: u64 = if e.is_empty() {
                            SIZE - 1
                        } else {
                            e.parse().ok()?
                        };
                        Some((a, e.min(SIZE - 1)))
                    })
                } else {
                    None
                };
                let (a, e, head) = match range {
                    Some((a, e)) => (
                        a,
                        e,
                        format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{e}/{SIZE}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\nContent-Disposition: attachment; filename=\"test file.bin\"\r\n\r\n",
                            e - a + 1
                        ),
                    ),
                    None => (
                        0,
                        SIZE - 1,
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {SIZE}\r\nConnection: close\r\n\r\n"
                        ),
                    ),
                };
                if s.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                let mut pos = a;
                let chunk = 32 * 1024u64;
                while pos <= e {
                    let n = chunk.min(e - pos + 1);
                    let data: Vec<u8> = (pos..pos + n).map(byte_at).collect();
                    if s.write_all(&data).await.is_err() {
                        return;
                    }
                    pos += n;
                    if per_conn_bps > 0 {
                        tokio::time::sleep(Duration::from_secs_f64(n as f64 / per_conn_bps as f64))
                            .await;
                    }
                }
                let _ = s.shutdown().await;
            });
        }
    });
    port
}

fn verify(path: &std::path::Path) {
    let data = std::fs::read(path).unwrap();
    assert_eq!(data.len() as u64, SIZE, "size mismatch");
    for (i, b) in data.iter().enumerate() {
        assert_eq!(*b, byte_at(i as u64), "corruption at byte {i}");
    }
}

async fn fetch(
    m: &Manager,
    url: &str,
    dir: &std::path::Path,
    conns: u32,
) -> (Duration, fetchy_core::DownloadInfo) {
    let t = Instant::now();
    let info = m
        .add(
            url,
            AddOptions {
                dir: Some(dir.to_string_lossy().into()),
                connections: Some(conns),
                ..Default::default()
            },
        )
        .unwrap();
    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let i = m.get(&info.id).unwrap();
        match i.status {
            Status::Complete => return (t.elapsed(), i),
            Status::Error => panic!("download failed: {:?}", i.error),
            _ => {}
        }
        assert!(t.elapsed() < Duration::from_secs(60), "timeout");
    }
}

fn tmp(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("fetchy-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[tokio::test]
async fn multi_connection_is_faster_and_correct() {
    let port = serve(4 * 1024 * 1024, true).await; // 4 MB/s per connection
    let url = format!("http://127.0.0.1:{port}/file");

    let d1 = tmp("single");
    let m1 = Manager::new(tmp("data1")).unwrap();
    let (t1, i1) = fetch(&m1, &url, &d1, 1).await;
    verify(&d1.join(&i1.filename));

    let d8 = tmp("multi");
    let m8 = Manager::new(tmp("data8")).unwrap();
    let (t8, i8) = fetch(&m8, &url, &d8, 8).await;
    verify(&d8.join(&i8.filename));

    println!(
        "1 conn: {t1:?}   8 conns: {t8:?}   speedup x{:.1}",
        t1.as_secs_f64() / t8.as_secs_f64()
    );
    assert_eq!(i8.filename, "test file.bin", "Content-Disposition filename");
    assert!(t8 * 3 < t1, "8 connections should be >3x faster than 1");
}

#[tokio::test]
async fn server_without_range_falls_back_to_single_stream() {
    let port = serve(0, false).await;
    let d = tmp("norange");
    let m = Manager::new(tmp("data-nr")).unwrap();
    let (_, i) = fetch(&m, &format!("http://127.0.0.1:{port}/f"), &d, 8).await;
    assert!(!i.resumable);
    verify(&d.join(&i.filename));
}

#[tokio::test]
async fn pause_and_resume_is_byte_exact() {
    let port = serve(2 * 1024 * 1024, true).await;
    let url = format!("http://127.0.0.1:{port}/file");
    let d = tmp("resume");
    let data = tmp("data-resume");
    let m = Manager::new(&data).unwrap();
    let info = m
        .add(
            &url,
            AddOptions {
                dir: Some(d.to_string_lossy().into()),
                connections: Some(6),
                ..Default::default()
            },
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    m.pause(&info.id);
    tokio::time::sleep(Duration::from_millis(400)).await;
    let mid = m.get(&info.id).unwrap();
    assert_eq!(mid.status, Status::Paused);
    assert!(
        mid.downloaded > 0 && mid.downloaded < SIZE,
        "paused mid-way: {}",
        mid.downloaded
    );
    // Simulate an app restart: a brand-new manager must pick the job up from disk.
    drop(m);
    let m2 = Manager::new(&data).unwrap();
    m2.resume(&info.id);
    let t = Instant::now();
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let i = m2.get(&info.id).unwrap();
        if i.status == Status::Complete {
            verify(&d.join(&i.filename));
            break;
        }
        assert_ne!(i.status, Status::Error, "{:?}", i.error);
        assert!(t.elapsed() < Duration::from_secs(60));
    }
}

async fn http(port: u16, raw: String) -> String {
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    s.write_all(raw.as_bytes()).await.unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    out
}

#[tokio::test]
async fn extension_api_requires_token_and_extension_origin() {
    let file_port = serve(0, true).await;
    let dir = tmp("api-dl");
    let m = Manager::new(tmp("data-api")).unwrap();
    let mut st = m.settings();
    st.default_dir = dir.to_string_lossy().into();
    m.set_settings(st);
    let port = fetchy_core::server::start(
        m.clone(),
        17800,
        "s3cret-token-s3cret-token-s3cret-token".into(),
    )
    .await
    .unwrap();
    let body = format!(r#"{{"url":"http://127.0.0.1:{file_port}/x","referer":"http://r/"}}"#);
    let post = |extra: &str, host: &str| {
        format!(
            "POST /add HTTP/1.1\r\nHost: {host}\r\n{extra}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    };
    let h = format!("127.0.0.1:{port}");
    let tok = "X-Fetchy-Token: s3cret-token-s3cret-token-s3cret-token\r\n";

    assert!(
        http(port, post("", &h)).await.starts_with("HTTP/1.1 401"),
        "no token"
    );
    assert!(
        http(port, post("X-Fetchy-Token: nope\r\n", &h))
            .await
            .starts_with("HTTP/1.1 401"),
        "wrong token"
    );
    assert!(
        http(
            port,
            post(&format!("{tok}Origin: https://evil.example\r\n"), &h)
        )
        .await
        .starts_with("HTTP/1.1 403"),
        "web origin"
    );
    assert!(
        http(port, post(tok, "evil.example"))
            .await
            .starts_with("HTTP/1.1 403"),
        "rebinding host"
    );
    assert_eq!(m.list().len(), 0, "rejected calls must not add downloads");

    let ok = http(
        port,
        post(&format!("{tok}Origin: chrome-extension://abc\r\n"), &h),
    )
    .await;
    assert!(
        ok.starts_with("HTTP/1.1 200")
            && ok.contains("Access-Control-Allow-Origin: chrome-extension://abc"),
        "{ok}"
    );
    assert_eq!(m.list().len(), 1);
    let ping = http(port, format!("GET /ping HTTP/1.1\r\nHost: {h}\r\n\r\n")).await;
    assert!(ping.contains("Fetchy"));
}

#[tokio::test]
async fn queue_outside_schedule_waits_and_per_queue_limit_applies() {
    use fetchy_core::{Queue, Schedule};
    let port = serve(1024 * 1024, true).await;
    let url = format!("http://127.0.0.1:{port}/f");
    let dir = tmp("queues");
    let m = Manager::new(tmp("data-queues")).unwrap();
    let mut st = m.settings();
    // A window that is never open (all days deselected) + a one-at-a-time queue.
    st.queues.push(Queue {
        schedule: Some(Schedule {
            enabled: true,
            start: "00:00".into(),
            stop: Some("23:59".into()),
            days: 0,
        }),
        ..Queue::new("night", "Night", 2)
    });
    st.queues.push(Queue::new("slow", "Slow", 1));
    m.set_settings(st);
    let opts = |q: &str| AddOptions {
        dir: Some(dir.to_string_lossy().into()),
        queue: Some(q.into()),
        ..Default::default()
    };

    let waiting = m.add(&url, opts("night")).unwrap();
    let a = m.add(&url, opts("slow")).unwrap();
    let b = m.add(&url, opts("slow")).unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(
        m.get(&waiting.id).unwrap().status,
        Status::Queued,
        "closed window must not start"
    );
    let (sa, sb) = (m.get(&a.id).unwrap().status, m.get(&b.id).unwrap().status);
    assert!(
        (sa == Status::Downloading) != (sb == Status::Downloading),
        "exactly one of two in a max_concurrent=1 queue runs: {sa:?} {sb:?}"
    );
    // Opening the window starts the waiting item.
    let mut st = m.settings();
    st.queues
        .iter_mut()
        .find(|q| q.id == "night")
        .unwrap()
        .schedule = None;
    m.set_settings(st);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(m.get(&waiting.id).unwrap().status, Status::Downloading);
    m.pause_all();
}

// ---- HLS ------------------------------------------------------------------

const SEGS: usize = 40;
const SEG_LEN: usize = 30_000;
const KEY: [u8; 16] = *b"0123456789abcdef";

fn seg_plain(variant: &str, n: usize) -> Vec<u8> {
    let salt = if variant == "hi" || variant == "enc" {
        3
    } else {
        9
    };
    (0..SEG_LEN)
        .map(|i| ((i * 7 + n * 13 + salt) % 251) as u8)
        .collect()
}

fn playlist(variant: &str) -> String {
    let mut s = String::from(
        "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:5\n",
    );
    if variant == "enc" {
        s.push_str("#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n"); // IV omitted: derived from sequence number
    }
    for n in 0..SEGS {
        s.push_str(&format!("#EXTINF:4.0,\nseg{n}.ts\n"));
    }
    s + "#EXT-X-ENDLIST\n"
}

async fn serve_hls(delay_ms: u64) -> u16 {
    use aes::Aes128;
    use cbc::cipher::{BlockModeEncrypt, KeyIvInit, block_padding::Pkcs7};
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut b = vec![0u8; 4096];
                let n = s.read(&mut b).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&b[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
                let (ct, body): (&str, Vec<u8>) = if path == "/hls/master.m3u8" {
                    ("application/vnd.apple.mpegurl", b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=500000\nlo/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=4000000\nhi/index.m3u8\n".to_vec())
                } else if let Some(v) = path
                    .strip_prefix("/hls/")
                    .and_then(|p| p.strip_suffix("/index.m3u8"))
                {
                    ("application/vnd.apple.mpegurl", playlist(v).into_bytes())
                } else if path == "/hls/enc/key.bin" {
                    ("application/octet-stream", KEY.to_vec())
                } else if let Some((v, n)) = path
                    .strip_prefix("/hls/")
                    .and_then(|p| p.strip_suffix(".ts"))
                    .and_then(|p| p.split_once("/seg"))
                {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    let n: usize = n.parse().unwrap();
                    let plain = seg_plain(v, n);
                    if v == "enc" {
                        let iv = ((5 + n) as u128).to_be_bytes();
                        let mut buf = plain.clone();
                        buf.resize(plain.len() + 16, 0);
                        let n = cbc::Encryptor::<Aes128>::new_from_slices(&KEY, &iv)
                            .unwrap()
                            .encrypt_padded::<Pkcs7>(&mut buf, plain.len())
                            .unwrap()
                            .len();
                        buf.truncate(n);
                        ("video/mp2t", buf)
                    } else {
                        ("video/mp2t", plain)
                    }
                } else {
                    let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                    return;
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(&body).await;
                let _ = s.shutdown().await;
            });
        }
    });
    port
}

fn expected(variant: &str) -> Vec<u8> {
    (0..SEGS).flat_map(|n| seg_plain(variant, n)).collect()
}

async fn wait_complete(m: &Manager, id: &str) -> fetchy_core::DownloadInfo {
    let t = Instant::now();
    loop {
        tokio::time::sleep(Duration::from_millis(60)).await;
        let i = m.get(id).unwrap();
        match i.status {
            Status::Complete => return i,
            Status::Error => panic!("failed: {:?}", i.error),
            _ => assert!(t.elapsed() < Duration::from_secs(40), "timeout"),
        }
    }
}

#[tokio::test]
async fn hls_master_picks_best_variant_and_output_is_in_order() {
    let port = serve_hls(5).await;
    let d = tmp("hls-plain");
    let m = Manager::new(tmp("data-hls1")).unwrap();
    let info = m
        .add(
            &format!("http://127.0.0.1:{port}/hls/master.m3u8"),
            AddOptions {
                dir: Some(d.to_string_lossy().into()),
                connections: Some(8),
                ..Default::default()
            },
        )
        .unwrap();
    let done = wait_complete(&m, &info.id).await;
    assert_eq!(done.filename, "hls.ts");
    assert_eq!(
        std::fs::read(d.join(&done.filename)).unwrap(),
        expected("hi"),
        "segments must be concatenated in order"
    );
    assert_eq!(done.size, Some((SEGS * SEG_LEN) as u64));
}

#[tokio::test]
async fn hls_aes128_with_implicit_iv_decrypts() {
    let port = serve_hls(5).await;
    let d = tmp("hls-enc");
    let m = Manager::new(tmp("data-hls2")).unwrap();
    let info = m
        .add(
            &format!("http://127.0.0.1:{port}/hls/enc/index.m3u8"),
            AddOptions {
                dir: Some(d.to_string_lossy().into()),
                connections: Some(6),
                ..Default::default()
            },
        )
        .unwrap();
    let done = wait_complete(&m, &info.id).await;
    assert_eq!(done.filename, "enc.ts");
    assert_eq!(
        std::fs::read(d.join(&done.filename)).unwrap(),
        expected("enc")
    );
}

#[tokio::test]
async fn hls_pause_resume_across_restart() {
    let port = serve_hls(60).await;
    let d = tmp("hls-resume");
    let data = tmp("data-hls3");
    let m = Manager::new(&data).unwrap();
    let info = m
        .add(
            &format!("http://127.0.0.1:{port}/hls/enc/index.m3u8"),
            AddOptions {
                dir: Some(d.to_string_lossy().into()),
                connections: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(900)).await;
    m.pause(&info.id);
    tokio::time::sleep(Duration::from_millis(400)).await;
    let mid = m.get(&info.id).unwrap();
    assert_eq!(mid.status, Status::Paused);
    assert!(
        mid.downloaded > 0 && mid.downloaded < (SEGS * SEG_LEN) as u64,
        "mid-way: {}",
        mid.downloaded
    );
    drop(m);
    let m2 = Manager::new(&data).unwrap();
    m2.resume(&info.id);
    let done = wait_complete(&m2, &info.id).await;
    assert_eq!(
        std::fs::read(d.join(&done.filename)).unwrap(),
        expected("enc")
    );
}

/// Regression: Tauri runs sync commands on a thread with no tokio runtime. Calling the
/// manager from such a thread used to panic ("there is no reactor running") and abort the app.
#[tokio::test]
async fn manager_is_usable_from_threads_without_a_runtime() {
    let port = serve(0, true).await;
    let d = tmp("no-rt");
    let m = Manager::new(tmp("data-no-rt")).unwrap();
    let m2 = m.clone();
    let url = format!("http://127.0.0.1:{port}/f");
    let dir = d.to_string_lossy().to_string();
    let id = std::thread::spawn(move || {
        let info = m2
            .add(
                &url,
                AddOptions {
                    dir: Some(dir),
                    ..Default::default()
                },
            )
            .unwrap();
        m2.pause(&info.id);
        m2.resume(&info.id);
        info.id
    })
    .join()
    .expect("manager calls must not panic off-runtime");
    let done = wait_complete(&m, &id).await;
    verify(&d.join(&done.filename));
}

// ---- organising, deleting, details, link inspection ------------------------

#[tokio::test]
async fn downloads_are_organised_into_category_folders() {
    let port = serve(0, true).await;
    let base = tmp("organise");
    let m = Manager::new(tmp("data-organise")).unwrap();
    let mut st = m.settings();
    st.default_dir = base.to_string_lossy().into();
    m.set_settings(st);
    let url = format!("http://127.0.0.1:{port}/x");
    let song = m
        .add(
            &url,
            AddOptions {
                filename: Some("song.mp3".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let zip = m
        .add(
            &url,
            AddOptions {
                filename: Some("bundle.zip".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let other = m.add(&url, AddOptions::default()).unwrap(); // server says "test file.bin"
    for (id, folder, name) in [
        (&song.id, "Music", "song.mp3"),
        (&zip.id, "Compressed", "bundle.zip"),
        (&other.id, "Other", "test file.bin"),
    ] {
        let d = wait_complete(&m, id).await;
        assert_eq!(
            std::path::Path::new(&d.dir),
            base.join(folder),
            "{name} folder"
        );
        verify(&base.join(folder).join(name));
    }
    // HLS goes to Videos
    let hp = serve_hls(1).await;
    let v = m
        .add(
            &format!("http://127.0.0.1:{hp}/hls/master.m3u8"),
            AddOptions::default(),
        )
        .unwrap();
    let d = wait_complete(&m, &v.id).await;
    assert_eq!(std::path::Path::new(&d.dir), base.join("Videos"));
    // turning organising off puts new files straight in the base folder
    let mut st = m.settings();
    st.organize = false;
    m.set_settings(st);
    let flat = m
        .add(
            &url,
            AddOptions {
                filename: Some("flat.mp3".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let d = wait_complete(&m, &flat.id).await;
    assert_eq!(std::path::Path::new(&d.dir), base);
}

#[tokio::test]
async fn deleting_removes_the_file_from_disk() {
    let port = serve(0, true).await;
    let dir = tmp("delete");
    let m = Manager::new(tmp("data-delete")).unwrap();
    let url = format!("http://127.0.0.1:{port}/x");
    let o = |n: &str| AddOptions {
        dir: Some(dir.to_string_lossy().into()),
        filename: Some(n.into()),
        ..Default::default()
    };
    let a = m.add(&url, o("gone.bin")).unwrap();
    let b = m.add(&url, o("kept.bin")).unwrap();
    wait_complete(&m, &a.id).await;
    wait_complete(&m, &b.id).await;
    m.remove(&a.id, true).await;
    m.remove(&b.id, false).await;
    assert!(!dir.join("gone.bin").exists(), "file must be deleted");
    assert!(
        dir.join("kept.bin").exists(),
        "list-only removal keeps the file"
    );
    assert_eq!(m.list().len(), 0);
}

#[tokio::test]
async fn details_expose_live_connections() {
    let port = serve(1024 * 1024, true).await;
    let dir = tmp("details");
    let m = Manager::new(tmp("data-details")).unwrap();
    let info = m
        .add(
            &format!("http://127.0.0.1:{port}/x"),
            AddOptions {
                dir: Some(dir.to_string_lossy().into()),
                connections: Some(8),
                ..Default::default()
            },
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let d = m.details(&info.id).unwrap();
    let active = d.segments.iter().filter(|s| s.active).count();
    assert!(
        active >= 4,
        "expected several live connections, got {active}"
    );
    assert!(
        d.segments
            .iter()
            .all(|s| s.pos >= s.start && s.pos <= s.end)
    );
    let now = m.get(&info.id).unwrap();
    assert!(
        now.speed > 0 && now.active_connections as usize == active,
        "speed {} conns {}",
        now.speed,
        now.active_connections
    );
    m.pause(&info.id);
}

#[tokio::test]
async fn inspect_accepts_files_and_rejects_web_pages() {
    let file_port = serve(0, true).await;
    let html = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let html_port = html.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = html.accept().await else {
                return;
            };
            let mut b = [0u8; 2048];
            let _ = s.read(&mut b).await;
            let body = "<html>hi</html>";
            let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
        }
    });
    let m = Manager::new(tmp("data-inspect")).unwrap();
    let f = m
        .inspect(&format!("http://127.0.0.1:{file_port}/dl"))
        .await
        .expect("a file link is downloadable");
    assert_eq!(f.filename, "test file.bin");
    assert_eq!(f.size, Some(SIZE));
    assert!(
        m.inspect(&format!("http://127.0.0.1:{html_port}/page"))
            .await
            .is_none(),
        "web pages are not offered"
    );
    assert!(m.inspect("not a url").await.is_none());
    assert!(m.inspect("ftp://x/y.zip").await.is_none());
    let hp = serve_hls(1).await;
    assert!(
        m.inspect(&format!("http://127.0.0.1:{hp}/hls/master.m3u8"))
            .await
            .unwrap()
            .stream
    );
}

/// Tiny FTP server: USER/PASS/TYPE/SIZE/REST/PASV/RETR/QUIT, one file `/pub/data.bin`.
async fn ftp_serve() -> u16 {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((s, _)) = l.accept().await else { return };
            tokio::spawn(async move {
                let mut rd = BufReader::new(s);
                rd.get_mut().write_all(b"220 test ftp\r\n").await.unwrap();
                let mut rest = 0u64;
                let mut pasv: Option<TcpListener> = None;
                loop {
                    let mut line = String::new();
                    if rd.read_line(&mut line).await.unwrap_or(0) == 0 {
                        return;
                    }
                    let (cmd, arg) = line.trim().split_once(' ').unwrap_or((line.trim(), ""));
                    let reply: String = match cmd.to_ascii_uppercase().as_str() {
                        "USER" => "331 need pass\r\n".into(),
                        "PASS" => "230 ok\r\n".into(),
                        "TYPE" => "200 binary\r\n".into(),
                        "SIZE" if arg == "/pub/data.bin" => format!("213 {SIZE}\r\n"),
                        "SIZE" => "550 nope\r\n".into(),
                        "REST" => {
                            rest = arg.parse().unwrap_or(0);
                            "350 restarting\r\n".into()
                        }
                        "PASV" => {
                            let dl = TcpListener::bind("127.0.0.1:0").await.unwrap();
                            let p = dl.local_addr().unwrap().port();
                            pasv = Some(dl);
                            format!(
                                "227 Entering Passive Mode (127,0,0,1,{},{})\r\n",
                                p / 256,
                                p % 256
                            )
                        }
                        "RETR" => {
                            let Some(dl) = pasv.take() else { continue };
                            rd.get_mut()
                                .write_all(b"150 here it comes\r\n")
                                .await
                                .unwrap();
                            let (mut d, _) = dl.accept().await.unwrap();
                            let chunk: Vec<u8> = (rest..SIZE).map(byte_at).collect();
                            let _ = d.write_all(&chunk).await;
                            drop(d);
                            "226 done\r\n".into()
                        }
                        "QUIT" => return,
                        _ => "502 no\r\n".into(),
                    };
                    if rd.get_mut().write_all(reply.as_bytes()).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    port
}

#[tokio::test]
async fn ftp_download_is_byte_exact_with_multiple_connections() {
    let port = ftp_serve().await;
    let d = tmp("ftp");
    let m = Manager::new(tmp("data-ftp")).unwrap();
    let (_, i) = fetch(
        &m,
        &format!("ftp://user:pw@127.0.0.1:{port}/pub/data.bin"),
        &d,
        4,
    )
    .await;
    assert_eq!(i.filename, "data.bin");
    assert!(i.resumable);
    verify(&d.join(&i.filename));
}

#[tokio::test]
async fn ftp_missing_file_is_reported() {
    let port = ftp_serve().await;
    let m = Manager::new(tmp("data-ftp2")).unwrap();
    let info = m
        .add(
            &format!("ftp://127.0.0.1:{port}/pub/missing.bin"),
            AddOptions {
                dir: Some(tmp("ftp2").to_string_lossy().into()),
                ..Default::default()
            },
        )
        .unwrap();
    let t = Instant::now();
    loop {
        tokio::time::sleep(Duration::from_millis(60)).await;
        let i = m.get(&info.id).unwrap();
        if i.status == Status::Error {
            assert!(i.error.unwrap_or_default().contains("not found"));
            return;
        }
        assert!(t.elapsed() < Duration::from_secs(10), "never failed");
    }
}

#[tokio::test]
async fn hourly_quota_stops_downloading_until_the_window_resets() {
    let port = serve(0, true).await;
    let m = Manager::new(tmp("data-quota")).unwrap();
    let mut s = m.settings();
    s.queues
        .iter_mut()
        .find(|q| q.id == "limits")
        .unwrap()
        .quota_per_hour = 2 * 1024 * 1024;
    m.set_settings(s);
    let info = m
        .add(
            &format!("http://127.0.0.1:{port}/f"),
            AddOptions {
                dir: Some(tmp("quota").to_string_lossy().into()),
                connections: Some(4),
                queue: Some("limits".into()),
                ..Default::default()
            },
        )
        .unwrap();
    tokio::time::sleep(Duration::from_secs(3)).await;
    let i = m.get(&info.id).unwrap();
    assert_ne!(i.status, Status::Complete, "quota ignored");
    // A little overshoot (in-flight chunks) is fine, the whole file is not.
    assert!(
        i.downloaded < 6 * 1024 * 1024,
        "downloaded {} despite 2 MiB/h quota",
        i.downloaded
    );
    m.pause(&info.id);
}

#[tokio::test]
async fn grabber_project_queues_new_files_once() {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut b = [0u8; 2048];
                let n = s.read(&mut b).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&b[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (ct, body): (&str, &[u8]) = if path == "/" {
                    ("text/html", br#"<a href="a.bin">a</a><a href="/dir/b.zip">b</a><a href="x.html">page</a>"#)
                } else if path == "/x.html" {
                    ("text/html", br#"<a href="c.bin">c</a>"#)
                } else {
                    ("application/octet-stream", b"hello")
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(body).await;
            });
        }
    });
    let m = Manager::new(tmp("data-proj")).unwrap();
    let base = tmp("proj-files");
    let mut st = m.settings();
    st.default_dir = base.to_string_lossy().into();
    m.set_settings(st);
    let mut p = fetchy_core::GrabProject::default();
    p.name = "My Pics".into();
    p.url = format!("http://127.0.0.1:{port}/");
    p.options.depth = 1;
    let p = m.save_grab_project(p);
    assert!(!p.id.is_empty());
    assert_eq!(
        m.run_grab_project(&p.id).await.unwrap(),
        3,
        "a.bin, b.zip and c.bin (one level deep)"
    );
    assert_eq!(m.list().len(), 3);
    assert!(
        m.list()
            .iter()
            .all(|i| i.project.as_deref() == Some("My Pics")),
        "tagged with the project"
    );
    assert!(
        m.list()
            .iter()
            .all(|i| std::path::Path::new(&i.dir) == base.join("My Pics")),
        "saved in the project folder"
    );
    assert_eq!(
        m.run_grab_project(&p.id).await.unwrap(),
        0,
        "nothing new the second time"
    );
    let saved = m.grab_projects();
    assert_eq!(saved[0].last_found, 3);
    assert!(saved[0].last_run.is_some());
    for i in m.list() {
        m.remove(&i.id, true).await;
    }
}

#[tokio::test]
async fn start_now_overrides_a_closed_schedule_and_stop_halts_it() {
    let port = serve(2 * 1024 * 1024, true).await;
    let m = Manager::new(tmp("data-startnow")).unwrap();
    let mut st = m.settings();
    st.queues.push(fetchy_core::Queue {
        schedule: Some(fetchy_core::Schedule {
            enabled: true,
            start: "00:00".into(),
            stop: Some("23:59".into()),
            days: 0,
        }),
        ..fetchy_core::Queue::new("night", "Night", 2)
    });
    m.set_settings(st);
    let dir = tmp("startnow");
    let info = m
        .add(
            &format!("http://127.0.0.1:{port}/f"),
            AddOptions {
                dir: Some(dir.to_string_lossy().into()),
                queue: Some("night".into()),
                ..Default::default()
            },
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(
        m.get(&info.id).unwrap().status,
        Status::Queued,
        "closed window must hold it"
    );
    m.start_queue("night");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(m.get(&info.id).unwrap().status, Status::Downloading);
    m.stop_queue("night");
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(m.get(&info.id).unwrap().status, Status::Paused);
}

#[tokio::test]
async fn items_can_be_reordered_within_their_queue() {
    let m = Manager::new(tmp("data-order")).unwrap();
    let mut st = m.settings();
    st.queues.push(fetchy_core::Queue {
        schedule: Some(fetchy_core::Schedule {
            enabled: true,
            start: "00:00".into(),
            stop: Some("23:59".into()),
            days: 0,
        }),
        ..fetchy_core::Queue::new("hold", "Hold", 1)
    });
    m.set_settings(st);
    let add = |n: &str| {
        m.add(
            &format!("http://127.0.0.1:9/{n}"),
            AddOptions {
                queue: Some("hold".into()),
                ..Default::default()
            },
        )
        .unwrap()
        .id
    };
    let (a, b, c) = (add("a"), add("b"), add("c"));
    // Engine order = oldest first: a, b, c. Moving c up puts it before b.
    m.move_in_queue(&c, -1);
    let order = |m: &Manager| -> Vec<String> {
        let mut v = m.list();
        v.reverse(); // list() is newest-first
        v.into_iter().map(|i| i.id).collect()
    };
    assert_eq!(order(&m), vec![a.clone(), c.clone(), b.clone()]);
    m.move_in_queue(&a, 1);
    assert_eq!(order(&m), vec![c, a, b]);
}

#[tokio::test]
async fn grabber_makes_an_offline_copy_with_filters() {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut b = [0u8; 2048];
                let n = s.read(&mut b).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&b[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (ct, body): (&str, &[u8]) = match path.as_str() {
                    "/" => ("text/html", br#"<link rel="stylesheet" href="/s/site.css"><img src="/img/p.png"><a href="a.bin">a</a><a href="more/other.html">o</a><a href="/logout.html">out</a>"#),
                    "/more/other.html" => ("text/html", br#"<a href="../img/p.png">back</a><a href="deep.html">d</a>"#),
                    "/more/deep.html" => ("text/html", br#"<img src="/img/deep.png">"#),
                    _ => ("application/octet-stream", b"data"),
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(body).await;
            });
        }
    });
    let root = tmp("offline");
    let m = Manager::new(tmp("data-offline")).unwrap();
    let mut o = fetchy_core::GrabOptions::default();
    o.offline_links = true;
    o.save_mode = "folder".into();
    o.save_dir = root.to_string_lossy().into();
    o.depth = 1; // start page + one level: deep.html is out of reach
    o.exclude_exts = vec!["bin".into()];
    let url = format!("http://127.0.0.1:{port}/");
    let files = m.grab(&url, &o).await.unwrap();
    let names: Vec<_> = files.iter().map(|f| f.filename.as_str()).collect();
    assert!(
        names.contains(&"p.png") && names.contains(&"site.css"),
        "{names:?}"
    );
    assert!(!names.contains(&"a.bin"), "excluded type");
    assert!(!names.contains(&"deep.png"), "beyond the link level limit");
    assert_eq!(
        files.iter().filter(|f| f.filename == "p.png").count(),
        1,
        "duplicates are hidden"
    );
    assert_eq!(m.grab_add(&url, &o, None, "main").await.unwrap(), 2);
    let index = std::fs::read_to_string(root.join("127.0.0.1").join("index.html")).unwrap();
    assert!(index.contains(r#"src="img/p.png""#), "{index}");
    assert!(index.contains(r#"href="s/site.css""#), "{index}");
    assert!(index.contains(r#"href="more/other.html""#), "{index}");
    assert!(
        index.contains("a.bin"),
        "excluded file link stays as it was"
    );
    let other =
        std::fs::read_to_string(root.join("127.0.0.1").join("more").join("other.html")).unwrap();
    assert!(other.contains(r#"href="../img/p.png""#), "{other}");
    let t = Instant::now();
    while !root.join("127.0.0.1").join("img").join("p.png").exists() {
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "file never arrived in the mirrored layout"
        );
    }
}

/// Server that drops the connection mid-body for the first `bad` requests, then serves normally.
async fn flaky(bad: usize) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            let n = seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tokio::spawn(async move {
                let mut b = [0u8; 2048];
                let _ = s.read(&mut b).await;
                if n < bad {
                    let _ = s
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n")
                        .await;
                    return; // hang up early
                }
                let body = vec![b'x'; 1000];
                let head = "HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n";
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(&body).await;
            });
        }
    });
    port
}

#[tokio::test]
async fn failed_downloads_retry_by_themselves() {
    let port = flaky(3).await; // the probe and the first attempt are dropped
    let m = Manager::new(tmp("data-retry")).unwrap();
    let mut st = m.settings();
    st.auto_retry = 3;
    st.queues
        .iter_mut()
        .find(|q| q.id == "main")
        .unwrap()
        .max_retries = 1; // give up quickly inside one run
    m.set_settings(st);
    let dir = tmp("retry");
    let info = m
        .add(
            &format!("http://127.0.0.1:{port}/f.bin"),
            AddOptions {
                dir: Some(dir.to_string_lossy().into()),
                connections: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
    let t = Instant::now();
    let mut saw_retry_note = false;
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let i = m.get(&info.id).unwrap();
        saw_retry_note |= i.error.as_deref().is_some_and(|e| e.contains("retrying"));
        if i.status == Status::Complete {
            break;
        }
        assert!(
            t.elapsed() < Duration::from_secs(60),
            "never recovered: {:?} {:?}",
            i.status,
            i.error
        );
    }
    assert!(saw_retry_note, "the user is told a retry is coming");
    assert_eq!(std::fs::metadata(dir.join("f.bin")).unwrap().len(), 1000);
}

#[tokio::test]
async fn post_download_hooks_run() {
    let port = serve(0, true).await;
    let m = Manager::new(tmp("data-hist")).unwrap();
    let marker = tmp("hist-marker").join("done.txt");
    std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
    let mut st = m.settings();
    #[cfg(windows)]
    {
        st.post_command = format!("echo {{name}} > \"{}\"", marker.display());
    }
    #[cfg(not(windows))]
    {
        st.post_command = format!("echo {{name}} > '{}'", marker.display());
    }
    m.set_settings(st);
    let d = tmp("hist");
    let sha = {
        // expected checksum of the generated pattern file
        let bytes: Vec<u8> = (0..SIZE).map(byte_at).collect();
        fetchy_core::extras::hash_file_bytes_for_test(&bytes)
    };
    let (id, _) = {
        let info = m
            .add(
                &format!("http://127.0.0.1:{port}/data.bin"),
                AddOptions {
                    dir: Some(d.to_string_lossy().into()),
                    checksum: Some(sha),
                    connections: Some(4),
                    ..Default::default()
                },
            )
            .unwrap();
        let i = wait_complete(&m, &info.id).await;
        (info.id, i)
    };
    let t = Instant::now();
    while !std::fs::read_to_string(&marker).is_ok_and(|c| c.contains(".bin"))
        || m.get(&id).unwrap().verified.is_none()
    {
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            t.elapsed() < Duration::from_secs(15),
            "post-download work never finished: verified={:?} scan={:?} marker={:?}",
            m.get(&id).unwrap().verified,
            m.get(&id).unwrap().scan_status,
            std::fs::read_to_string(&marker)
        );
    }
    assert_eq!(m.get(&id).unwrap().verified, Some(true), "checksum matched");
    let note = std::fs::read_to_string(&marker).unwrap_or_default();
    assert!(
        note.contains(".bin"),
        "user command ran with {{name}}: {note:?}"
    );
    // checksum helper API
    let r = m_verify(&d.join(&m.get(&id).unwrap().filename)).await;
    assert!(!r.ok, "wrong checksum is reported");
}

async fn m_verify(p: &std::path::Path) -> fetchy_core::extras::ChecksumResult {
    fetchy_core::extras::verify(p, &"0".repeat(64)).unwrap()
}

#[tokio::test]
async fn auto_sort_rules_match_names_and_sites() {
    let port = serve(0, true).await;
    let m = Manager::new(tmp("data-sort")).unwrap();
    let base = tmp("sort-files");
    let mut st = m.settings();
    st.default_dir = base.to_string_lossy().into();
    st.categories = vec![
        fetchy_core::CategoryRule {
            name: "Bills".into(),
            folder: "Bills".into(),
            exts: vec![],
            sites: vec![],
            names: vec!["invoice*.pdf".into()],
        },
        fetchy_core::CategoryRule {
            name: "Local".into(),
            folder: "FromLocalhost".into(),
            exts: vec![],
            sites: vec!["127.0.0.1".into()],
            names: vec![],
        },
    ];
    m.set_settings(st);
    let a = m
        .add(
            &format!("http://127.0.0.1:{port}/x1"),
            AddOptions {
                filename: Some("Invoice-77.pdf".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let b = m
        .add(
            &format!("http://127.0.0.1:{port}/x2"),
            AddOptions {
                filename: Some("holiday.jpg".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let a = wait_complete(&m, &a.id).await;
    let b = wait_complete(&m, &b.id).await;
    assert_eq!(
        std::path::Path::new(&a.dir),
        base.join("Bills"),
        "name pattern wins (first rule)"
    );
    assert_eq!(
        std::path::Path::new(&b.dir),
        base.join("FromLocalhost"),
        "site rule"
    );
}

/// A tiny route-table server: `f(path, nth_request_for_that_path)` -> (content-type, body).
async fn serve_fn<F>(f: F) -> u16
where
    F: Fn(&str, usize) -> Option<(&'static str, Vec<u8>)> + Send + Sync + 'static,
{
    use std::collections::HashMap;
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let f = std::sync::Arc::new(f);
    let counts = std::sync::Arc::new(std::sync::Mutex::new(HashMap::<String, usize>::new()));
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            let (f, counts) = (f.clone(), counts.clone());
            tokio::spawn(async move {
                let mut b = [0u8; 4096];
                let n = s.read(&mut b).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&b[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let nth = {
                    let mut c = counts.lock().unwrap();
                    let e = c.entry(path.clone()).or_insert(0);
                    *e += 1;
                    *e
                };
                let (head, body) = match f(&path, nth) {
                    Some((ct, body)) => (
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        ),
                        body,
                    ),
                    None => (
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_string(),
                        vec![],
                    ),
                };
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(&body).await;
            });
        }
    });
    port
}

fn seg_bytes(tag: u8, n: usize) -> Vec<u8> {
    vec![tag.wrapping_add(n as u8); 100]
}

#[tokio::test]
async fn live_streams_are_recorded_until_they_end() {
    let port = serve_fn(|path, nth| {
        const M3U8: &str = "application/vnd.apple.mpegurl";
        if path == "/live.m3u8" {
            // The window moves on with every reload and ends on the fourth.
            let (first, last, end) = match nth {
                1 => (0, 1, false),
                2 => (1, 3, false),
                3 => (2, 4, false),
                _ => (3, 6, true),
            };
            let mut p =
                format!("#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXT-X-MEDIA-SEQUENCE:{first}\n");
            for n in first..=last {
                p.push_str(&format!("#EXTINF:2,\ns{n}.ts\n"));
            }
            if end {
                p.push_str("#EXT-X-ENDLIST\n");
            }
            return Some((M3U8, p.into_bytes()));
        }
        let n: usize = path
            .trim_start_matches("/s")
            .trim_end_matches(".ts")
            .parse()
            .ok()?;
        Some(("video/mp2t", seg_bytes(10, n)))
    })
    .await;
    let m = Manager::new(tmp("data-live")).unwrap();
    let d = tmp("live");
    let info = m
        .add(
            &format!("http://127.0.0.1:{port}/live.m3u8"),
            AddOptions {
                dir: Some(d.to_string_lossy().into()),
                ..Default::default()
            },
        )
        .unwrap();
    let done = wait_complete(&m, &info.id).await;
    let mut want = vec![];
    for n in 0..=6 {
        want.extend(seg_bytes(10, n));
    }
    assert_eq!(
        std::fs::read(d.join(&done.filename)).unwrap(),
        want,
        "every segment once, in order, none repeated"
    );
}

#[tokio::test]
async fn stopping_a_live_recording_keeps_what_was_recorded() {
    let port = serve_fn(|path, _| {
        if path == "/forever.m3u8" {
            let p = "#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:2,\ns0.ts\n#EXTINF:2,\ns1.ts\n";
            return Some(("application/vnd.apple.mpegurl", p.as_bytes().to_vec()));
        }
        let n: usize = path.trim_start_matches("/s").trim_end_matches(".ts").parse().ok()?;
        Some(("video/mp2t", seg_bytes(50, n)))
    })
    .await;
    let m = Manager::new(tmp("data-live2")).unwrap();
    let d = tmp("live2");
    let info = m
        .add(
            &format!("http://127.0.0.1:{port}/forever.m3u8"),
            AddOptions {
                dir: Some(d.to_string_lossy().into()),
                ..Default::default()
            },
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(
        m.get(&info.id).unwrap().status,
        Status::Downloading,
        "still recording"
    );
    m.pause(&info.id); // Stop = finish the recording
    let done = wait_complete(&m, &info.id).await;
    let mut want = seg_bytes(50, 0);
    want.extend(seg_bytes(50, 1));
    assert_eq!(std::fs::read(d.join(&done.filename)).unwrap(), want);
}

#[tokio::test]
async fn qualities_are_listed_and_audio_is_merged_by_ffmpeg() {
    const M3U8: &str = "application/vnd.apple.mpegurl";
    let port = serve_fn(|path, _| {
        let media = |tag: &str| format!("#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXTINF:2,\n{tag}0.ts\n#EXTINF:2,\n{tag}1.ts\n#EXT-X-ENDLIST\n");
        match path {
            "/m/master.m3u8" => Some((M3U8, "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"en\",DEFAULT=YES,URI=\"audio.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=640x360,AUDIO=\"aud\"\nv360.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080,AUDIO=\"aud\"\nv1080.m3u8\n".as_bytes().to_vec())),
            "/m/v1080.m3u8" => Some((M3U8, media("hi").into_bytes())),
            "/m/v360.m3u8" => Some((M3U8, media("lo").into_bytes())),
            "/m/audio.m3u8" => Some((M3U8, media("au").into_bytes())),
            p if p.ends_with(".ts") => {
                let tag = if p.contains("/hi") { 1 } else if p.contains("/lo") { 2 } else { 3 };
                let n: usize = p.trim_end_matches(".ts").chars().last()?.to_digit(10)? as usize;
                Some(("video/mp2t", vec![tag * 10 + n as u8; 64]))
            }
            _ => None,
        }
    })
    .await;
    let m = Manager::new(tmp("data-mux")).unwrap();
    let url = format!("http://127.0.0.1:{port}/m/master.m3u8");

    let q = m.stream_variants(&url).await.unwrap();
    assert_eq!(q.len(), 2);
    assert_eq!(q[0].height, Some(1080), "best first");
    assert!(q[0].url.ends_with("/m/v1080.m3u8"));
    assert!(
        q[0].audio_url
            .as_deref()
            .is_some_and(|a| a.ends_with("/m/audio.m3u8")),
        "the audio track is found"
    );

    // A stand-in ffmpeg: copies the video input (6th argument) to the output (last argument).
    let tools = tmp("fake-ffmpeg");
    std::fs::create_dir_all(&tools).unwrap();
    #[cfg(windows)]
    let (script, body) = (
        tools.join("ffmpeg.cmd"),
        "@echo off\r\nfor %%a in (%*) do set \"last=%%~a\"\r\ncopy /b \"%~6\" \"%last%\" >nul\r\n",
    );
    #[cfg(not(windows))]
    let (script, body) = (
        tools.join("ffmpeg.sh"),
        "#!/bin/sh\nfor a; do last=\"$a\"; done\ncp \"$6\" \"$last\"\n",
    );
    std::fs::write(&script, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let d = tmp("mux");
    let mut st = m.settings();
    st.ffmpeg_path = script.to_string_lossy().into();
    m.set_settings(st);

    let info = m
        .add_smart(
            &url,
            AddOptions {
                dir: Some(d.to_string_lossy().into()),
                filename: Some("Holiday".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let t = Instant::now();
    loop {
        tokio::time::sleep(Duration::from_millis(150)).await;
        let list = m.list();
        if list.len() == 1
            && list[0].status == Status::Complete
            && list[0].filename.ends_with(".mp4")
        {
            break;
        }
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "never merged: {:?}",
            list.iter()
                .map(|i| (&i.filename, i.status, &i.description))
                .collect::<Vec<_>>()
        );
    }
    let done = m.get(&info.id).unwrap();
    assert_eq!(done.filename, "Holiday.mp4");
    assert_eq!(
        std::fs::read(d.join("Holiday.mp4")).unwrap().len(),
        128,
        "the merged file replaced the two parts"
    );
    assert!(
        !d.join("Holiday.ts").exists() && !d.join("Holiday (audio).ts").exists(),
        "parts are cleaned up"
    );
}

#[tokio::test]
async fn dash_on_demand_streams_download_and_merge() {
    let port = serve_fn(|path, _| {
        let mpd = r#"<?xml version="1.0"?>
<MPD type="static" mediaPresentationDuration="PT4S" xmlns="urn:mpeg:dash:schema:mpd:2011"><Period>
 <AdaptationSet mimeType="video/mp4"><SegmentTemplate timescale="1" duration="2" startNumber="1" initialization="v/init.mp4" media="v/$Number$.m4s"/>
  <Representation id="v1" bandwidth="1000000" height="480"/></AdaptationSet>
 <AdaptationSet mimeType="audio/mp4"><SegmentTemplate timescale="1" duration="2" startNumber="1" initialization="a/init.mp4" media="a/$Number$.m4s"/>
  <Representation id="a1" bandwidth="96000"/></AdaptationSet>
</Period></MPD>"#;
        match path {
            "/dash/manifest.mpd" => Some(("application/dash+xml", mpd.as_bytes().to_vec())),
            "/dash/v/init.mp4" => Some(("video/mp4", vec![b'I'; 10])),
            "/dash/a/init.mp4" => Some(("audio/mp4", vec![b'i'; 10])),
            p if p.starts_with("/dash/v/") => Some(("video/mp4", vec![b'V'; 100])),
            p if p.starts_with("/dash/a/") => Some(("audio/mp4", vec![b'A'; 100])),
            _ => None,
        }
    })
    .await;
    let m = Manager::new(tmp("data-dash")).unwrap();
    let url = format!("http://127.0.0.1:{port}/dash/manifest.mpd");
    let q = m.stream_variants(&url).await.unwrap();
    assert_eq!(q.len(), 1);
    assert_eq!(q[0].height, Some(480));
    let tools = tmp("fake-ffmpeg-dash");
    std::fs::create_dir_all(&tools).unwrap();
    #[cfg(windows)]
    let (script, body) = (
        tools.join("ffmpeg.cmd"),
        "@echo off\r\nfor %%a in (%*) do set \"last=%%~a\"\r\ncopy /b \"%~6\" \"%last%\" >nul\r\n",
    );
    #[cfg(not(windows))]
    let (script, body) = (
        tools.join("ffmpeg.sh"),
        "#!/bin/sh\nfor a; do last=\"$a\"; done\ncp \"$6\" \"$last\"\n",
    );
    std::fs::write(&script, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut st = m.settings();
    st.ffmpeg_path = script.to_string_lossy().into();
    m.set_settings(st);
    let d = tmp("dash");
    let info = m
        .add_smart(
            &url,
            AddOptions {
                dir: Some(d.to_string_lossy().into()),
                filename: Some("Clip".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let t = Instant::now();
    loop {
        tokio::time::sleep(Duration::from_millis(150)).await;
        let list = m.list();
        if list.len() == 1
            && list[0].status == Status::Complete
            && list[0].description.contains("merged")
        {
            break;
        }
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "never merged: {:?}",
            list.iter()
                .map(|i| (&i.filename, i.status, &i.error, &i.description))
                .collect::<Vec<_>>()
        );
    }
    let done = m.get(&info.id).unwrap();
    assert_eq!(done.filename, "Clip.mp4");
    let bytes = std::fs::read(d.join("Clip.mp4")).unwrap();
    assert_eq!(
        bytes.len(),
        10 + 2 * 100,
        "init segment + two media segments of the video track"
    );
    assert_eq!(&bytes[..10], &[b'I'; 10]);
}

#[tokio::test]
async fn downloads_can_go_through_a_proxy() {
    // A fake HTTP proxy: requests arrive in absolute form (GET http://host/path).
    let proxy = serve_fn(|path, _| {
        if path.starts_with("http://files.invalid/") {
            Some(("application/octet-stream", vec![7u8; 300]))
        } else {
            None
        }
    })
    .await;
    let m = Manager::new(tmp("data-proxy")).unwrap();
    let mut st = m.settings();
    st.proxy = fetchy_core::ProxySettings {
        kind: "http".into(),
        host: "127.0.0.1".into(),
        port: proxy,
        username: String::new(),
        password: String::new(),
    };
    m.set_settings(st);
    let d = tmp("proxy");
    // files.invalid cannot be resolved: it only works through the proxy
    let info = m
        .add(
            "http://files.invalid/pic.bin",
            AddOptions {
                dir: Some(d.to_string_lossy().into()),
                ..Default::default()
            },
        )
        .unwrap();
    let done = wait_complete(&m, &info.id).await;
    assert_eq!(
        std::fs::read(d.join(&done.filename)).unwrap(),
        vec![7u8; 300]
    );
}

#[tokio::test]
async fn site_profiles_add_login_and_cookie_automatically() {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let seen2 = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            let seen = seen2.clone();
            tokio::spawn(async move {
                let mut b = [0u8; 4096];
                let n = s.read(&mut b).await.unwrap_or(0);
                *seen.lock().unwrap() = String::from_utf8_lossy(&b[..n]).to_ascii_lowercase();
                let _ = s
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                    )
                    .await;
            });
        }
    });
    let m = Manager::new(tmp("data-profile")).unwrap();
    let mut st = m.settings();
    st.site_profiles = vec![fetchy_core::SiteProfile {
        host: "127.0.0.1".into(),
        username: "alice".into(),
        password: "pw".into(),
        cookie: "sid=abc".into(),
        referer: String::new(),
    }];
    m.set_settings(st);
    let d = tmp("profile");
    let info = m
        .add(
            &format!("http://127.0.0.1:{port}/secret.txt"),
            AddOptions {
                dir: Some(d.to_string_lossy().into()),
                ..Default::default()
            },
        )
        .unwrap();
    wait_complete(&m, &info.id).await;
    let req = seen.lock().unwrap().clone();
    // base64("alice:pw") = YWxpY2U6cHc=
    assert!(
        req.contains("authorization: basic ywxpy2u6chc="),
        "login sent: {req}"
    );
    assert!(req.contains("cookie: sid=abc"), "cookie sent: {req}");
}
