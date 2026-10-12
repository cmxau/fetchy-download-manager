//! Segmented download engine.
//!
//! The speed trick IDM is known for is not magic: it opens several TCP
//! connections to the same file (each fetching a different byte range) and, when
//! one finishes, *splits the largest remaining range* so no connection ever
//! sits idle waiting on a slow tail. That is what `claim` does below.

use crate::limiter::Limiter;
use crate::types::{AddOptions, Error, Segment};
use crate::util::{filename_from_disposition, filename_from_url, write_all_at};
use bytes::Bytes;
use futures_util::StreamExt;
use reqwest::{Client, RequestBuilder, StatusCode, header};
use std::fs::File;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// Never split a range smaller than 2x this; a new connection has setup cost.
const MIN_SPLIT: u64 = 1 << 20;
const FLUSH_BYTES: usize = 512 * 1024;
const FLUSH_EVERY: Duration = Duration::from_millis(200);
const MAX_FAILS: u32 = 6;

pub fn build_client(ua: &str, proxy: &crate::types::ProxySettings) -> reqwest::Result<Client> {
    // HTTP/1.1 only (the `http2` feature is off): every range gets its own TCP
    // connection, instead of being multiplexed onto one that a single TCP
    // window would then throttle.
    let mut b = Client::builder();
    if proxy.kind != "none" && !proxy.host.trim().is_empty() {
        let scheme = if proxy.kind == "socks5" {
            "socks5h"
        } else {
            "http"
        };
        let mut p =
            reqwest::Proxy::all(format!("{scheme}://{}:{}", proxy.host.trim(), proxy.port))?;
        if !proxy.username.is_empty() {
            p = p.basic_auth(&proxy.username, &proxy.password);
        }
        b = b.proxy(p);
    }
    b.user_agent(ua)
        .tcp_nodelay(true)
        .pool_max_idle_per_host(64)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
}

fn decorate(mut req: RequestBuilder, opts: &AddOptions) -> RequestBuilder {
    if let Some(r) = &opts.referer {
        req = req.header(header::REFERER, r);
    }
    if let Some(c) = &opts.cookie {
        req = req.header(header::COOKIE, c);
    }
    if let Some(ua) = &opts.user_agent {
        req = req.header(header::USER_AGENT, ua);
    }
    for (k, v) in &opts.headers {
        req = req.header(k.as_str(), v.as_str());
    }
    if let Some(u) = &opts.username {
        req = req.basic_auth(u, opts.password.as_deref());
    }
    req
}

#[derive(Debug, Clone)]
pub struct Probe {
    pub url: String,
    pub size: Option<u64>,
    pub resumable: bool,
    pub filename: String,
    pub etag: Option<String>,
    pub content_type: String,
    /// Server marked the response as a download (Content-Disposition: attachment).
    pub attachment: bool,
}

fn status_err(s: StatusCode) -> Error {
    let msg = format!("HTTP {}", s.as_u16());
    if s == StatusCode::REQUEST_TIMEOUT || s == StatusCode::TOO_MANY_REQUESTS || s.is_server_error()
    {
        Error::Retry(msg)
    } else {
        Error::Fatal(msg)
    }
}

/// Ask for byte 0 only: tells us the size, range support, final URL (after
/// redirects) and filename in one round-trip, without downloading anything.
pub async fn probe(client: &Client, url: &str, opts: &AddOptions) -> Result<Probe, Error> {
    if crate::ftp::is_ftp(url) {
        let p = crate::ftp::probe(url).await?;
        return Ok(Probe {
            url: url.to_string(),
            size: p.size,
            resumable: p.resumable,
            filename: p.filename,
            etag: None,
            content_type: String::new(),
            attachment: true,
        });
    }
    let req = decorate(client.get(url).header(header::RANGE, "bytes=0-0"), opts);
    let resp = req.send().await.map_err(|e| Error::Retry(e.to_string()))?;
    let status = resp.status();
    let h = resp.headers();
    let (size, resumable) = match status {
        StatusCode::PARTIAL_CONTENT => {
            let total = h
                .get(header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.rsplit('/').next())
                .and_then(|v| v.trim().parse::<u64>().ok());
            (total, total.is_some())
        }
        StatusCode::OK => (resp.content_length(), false),
        StatusCode::RANGE_NOT_SATISFIABLE => (Some(0), false),
        s => return Err(status_err(s)),
    };
    let filename = h
        .get(header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(filename_from_disposition)
        .unwrap_or_else(|| filename_from_url(resp.url()));
    let etag = h
        .get(header::ETAG)
        .or_else(|| h.get(header::LAST_MODIFIED))
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let attachment = h
        .get(header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.trim_start()
                .to_ascii_lowercase()
                .starts_with("attachment")
        });
    let content_type = h
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let url = resp.url().to_string();
    Ok(Probe {
        url,
        size,
        resumable,
        filename,
        etag,
        content_type,
        attachment,
    })
}

/// State shared between the workers of one download.
pub struct Shared {
    pub segs: Mutex<Vec<Segment>>,
    pub downloaded: AtomicU64,
    pub active: AtomicU32,
    pub file: Arc<File>,
}

impl Shared {
    pub fn new(file: File, segs: Vec<Segment>) -> Self {
        let done = segs.iter().map(|s| s.pos - s.start).sum();
        Shared {
            segs: Mutex::new(segs),
            downloaded: AtomicU64::new(done),
            active: AtomicU32::new(0),
            file: Arc::new(file),
        }
    }

    pub fn snapshot(&self) -> Vec<Segment> {
        self.segs.lock().unwrap().clone()
    }

    pub fn complete(&self) -> bool {
        self.segs.lock().unwrap().iter().all(|s| s.pos >= s.end)
    }
}

pub struct Job {
    pub client: Client,
    pub limiter: Arc<Limiter>,
    pub url: String,
    pub opts: AddOptions,
    pub resumable: bool,
    pub connections: u32,
    /// Attempts per segment before the download fails (0 = default).
    pub retries: u32,
    pub cancel: CancellationToken,
    pub shared: Arc<Shared>,
}

/// Give a worker something to do: an idle unfinished range first, otherwise
/// carve the upper half off the biggest range someone else is still working on.
fn claim(shared: &Shared, can_split: bool) -> Option<usize> {
    let mut segs = shared.segs.lock().unwrap();
    if let Some(i) = segs.iter().position(|s| !s.active && s.pos < s.end) {
        segs[i].active = true;
        return Some(i);
    }
    if !can_split {
        return None;
    }
    let (i, rem) = segs
        .iter()
        .enumerate()
        .filter(|(_, s)| s.active && s.end != u64::MAX)
        .map(|(i, s)| (i, s.end.saturating_sub(s.pos)))
        .max_by_key(|&(_, r)| r)?;
    if rem < 2 * MIN_SPLIT {
        return None;
    }
    let mid = segs[i].pos + rem / 2;
    let end = segs[i].end;
    segs[i].end = mid;
    segs.push(Segment {
        start: mid,
        pos: mid,
        end,
        active: true,
    });
    Some(segs.len() - 1)
}

fn release(shared: &Shared, idx: usize) {
    shared.segs.lock().unwrap()[idx].active = false;
}

/// Run until every segment is complete. Returns `Cancelled` on pause/cancel.
pub async fn run(job: Arc<Job>) -> Result<(), Error> {
    let workers = if job.resumable {
        job.connections.max(1)
    } else {
        1
    };
    let mut last_err = None;
    for _round in 0..3 {
        let before = job.shared.downloaded.load(Ordering::Relaxed);
        let stop = job.cancel.child_token();
        let mut set = tokio::task::JoinSet::new();
        for n in 0..workers {
            let job = job.clone();
            let stop = stop.clone();
            set.spawn(async move {
                // Stagger a little so the first split sees real positions and
                // we don't hammer a server with N simultaneous handshakes.
                if n > 0 {
                    tokio::time::sleep(Duration::from_millis(15 * n.min(10) as u64)).await;
                }
                worker(&job, &stop).await
            });
        }
        while let Some(res) = set.join_next().await {
            match res {
                Ok(Ok(())) => {}
                Ok(Err(Error::Cancelled)) => {}
                Ok(Err(e @ Error::Fatal(_))) => {
                    stop.cancel();
                    last_err = Some(e);
                }
                Ok(Err(e)) => last_err = Some(e),
                Err(e) => last_err = Some(Error::Fatal(format!("worker panicked: {e}"))),
            }
        }
        if job.cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if job.shared.complete() {
            return Ok(());
        }
        if matches!(last_err, Some(Error::Fatal(_))) {
            break;
        }
        if job.shared.downloaded.load(Ordering::Relaxed) == before {
            break; // a whole round made no progress; stop hammering
        }
    }
    Err(last_err.unwrap_or_else(|| Error::Retry("download incomplete".into())))
}

async fn worker(job: &Job, stop: &CancellationToken) -> Result<(), Error> {
    let can_split = job.resumable;
    loop {
        if stop.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let Some(idx) = claim(&job.shared, can_split) else {
            return Ok(());
        };
        job.shared.active.fetch_add(1, Ordering::Relaxed);
        let r = fetch_segment(job, idx, stop).await;
        job.shared.active.fetch_sub(1, Ordering::Relaxed);
        release(&job.shared, idx);
        r?;
    }
}

fn seg_pos(job: &Job, idx: usize) -> u64 {
    job.shared.segs.lock().unwrap()[idx].pos
}

async fn fetch_segment(job: &Job, idx: usize, stop: &CancellationToken) -> Result<(), Error> {
    let mut fails = 0u32;
    loop {
        let before = seg_pos(job, idx);
        match attempt(job, idx, stop).await {
            Ok(()) => return Ok(()),
            Err(Error::Retry(e)) => {
                if seg_pos(job, idx) > before {
                    fails = 0;
                } else {
                    fails += 1;
                }
                if fails
                    >= if job.retries == 0 {
                        MAX_FAILS
                    } else {
                        job.retries
                    }
                {
                    return Err(Error::Retry(e));
                }
                let backoff = Duration::from_millis((400u64 << fails.min(5)).min(8000));
                tokio::select! {
                    _ = stop.cancelled() => return Err(Error::Cancelled),
                    _ = tokio::time::sleep(backoff) => {}
                }
            }
            Err(e) => return Err(e),
        }
    }
}

async fn attempt(job: &Job, idx: usize, stop: &CancellationToken) -> Result<(), Error> {
    let (start, pos, end) = {
        let s = &job.shared.segs.lock().unwrap()[idx];
        (s.start, s.pos, s.end)
    };
    if pos >= end {
        return Ok(());
    }
    let mut stream: crate::ftp::ByteStream = if crate::ftp::is_ftp(&job.url) {
        tokio::select! {
            _ = stop.cancelled() => return Err(Error::Cancelled),
            r = crate::ftp::open_range(&job.url, pos, end) => r?,
        }
    } else {
        let mut req = job.client.get(&job.url);
        if job.resumable {
            let range = if end == u64::MAX {
                format!("bytes={pos}-")
            } else {
                format!("bytes={}-{}", pos, end - 1)
            };
            req = req.header(header::RANGE, range);
        }
        req = decorate(req, &job.opts);
        let resp = tokio::select! {
            _ = stop.cancelled() => return Err(Error::Cancelled),
            r = req.send() => r.map_err(|e| Error::Retry(e.to_string()))?,
        };
        match resp.status() {
            StatusCode::PARTIAL_CONTENT if job.resumable => {
                let ok = resp
                    .headers()
                    .get(header::CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("bytes "))
                    .and_then(|v| v.split('-').next())
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .is_none_or(|s| s == pos);
                if !ok {
                    return Err(Error::Fatal("server returned a different range".into()));
                }
            }
            // A server that ignores Range is only usable for a single from-zero stream.
            StatusCode::OK if !job.resumable || (start == 0 && pos == 0 && end == u64::MAX) => {
                if pos != 0 {
                    // The server cannot resume: start this single stream over from the beginning.
                    let mut segs = job.shared.segs.lock().unwrap();
                    let lost = segs[idx].pos - segs[idx].start;
                    segs[idx].pos = segs[idx].start;
                    job.shared
                        .downloaded
                        .fetch_sub(lost, std::sync::atomic::Ordering::Relaxed);
                }
            }
            s => return Err(status_err(s)),
        }

        Box::pin(resp.bytes_stream().map(|r| r.map_err(|e| e.to_string())))
    };
    let mut buf: Vec<Bytes> = Vec::new();
    let mut buf_len = 0usize;
    let mut last_flush = Instant::now();
    loop {
        let next = tokio::select! {
            _ = stop.cancelled() => {
                flush(job, idx, &mut buf, &mut buf_len).await?;
                return Err(Error::Cancelled);
            }
            n = stream.next() => n,
        };
        match next {
            Some(Ok(chunk)) => {
                job.limiter.acquire(chunk.len()).await;
                buf_len += chunk.len();
                buf.push(chunk);
                if buf_len >= FLUSH_BYTES || last_flush.elapsed() >= FLUSH_EVERY {
                    last_flush = Instant::now();
                    if flush(job, idx, &mut buf, &mut buf_len).await? {
                        return Ok(());
                    }
                }
            }
            Some(Err(e)) => {
                flush(job, idx, &mut buf, &mut buf_len).await?;
                return Err(Error::Retry(e.to_string()));
            }
            None => break,
        }
    }
    let done = flush(job, idx, &mut buf, &mut buf_len).await?;
    let mut segs = job.shared.segs.lock().unwrap();
    let s = &mut segs[idx];
    if s.end == u64::MAX {
        s.end = s.pos; // unknown length: EOF defines the end
        Ok(())
    } else if done || s.pos >= s.end {
        Ok(())
    } else {
        Err(Error::Retry("connection closed early".into()))
    }
}

/// Commit buffered bytes to disk. Returns true once the segment is complete.
/// Bytes past the segment end (it may have been split under us) are dropped.
async fn flush(
    job: &Job,
    idx: usize,
    buf: &mut Vec<Bytes>,
    buf_len: &mut usize,
) -> Result<bool, Error> {
    if buf.is_empty() {
        let segs = job.shared.segs.lock().unwrap();
        return Ok(segs[idx].pos >= segs[idx].end);
    }
    let (off, take, done) = {
        let mut segs = job.shared.segs.lock().unwrap();
        let s = &mut segs[idx];
        let take = (*buf_len as u64).min(s.end - s.pos);
        let off = s.pos;
        s.pos += take;
        (off, take, s.pos >= s.end)
    };
    let chunks = std::mem::take(buf);
    *buf_len = 0;
    if take > 0 {
        let file = job.shared.file.clone();
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            let mut off = off;
            let mut left = take as usize;
            for c in chunks {
                if left == 0 {
                    break;
                }
                let n = c.len().min(left);
                write_all_at(&file, &c[..n], off)?;
                off += n as u64;
                left -= n;
            }
            Ok(())
        })
        .await
        .map_err(|e| Error::Fatal(e.to_string()))?
        .map_err(|e| Error::Fatal(format!("disk write failed: {e}")))?;
        job.shared.downloaded.fetch_add(take, Ordering::Relaxed);
    }
    Ok(done)
}
