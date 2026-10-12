//! HLS (`.m3u8`) downloader.
//!
//! Resolves a master playlist to its best variant, downloads the media
//! segments in parallel (AES-128 decrypted when needed) and appends them to
//! one file in playback order. MPEG-TS segments concatenate into a valid
//! `.ts`; fMP4 streams (init segment + fragments) into a valid fragmented `.mp4`.
//!
//! Not supported (fails with a clear message instead of producing a broken
//! file): live streams, SAMPLE-AES / other DRM, and separate audio renditions.

use crate::limiter::Limiter;
use crate::types::{AddOptions, Error};
use crate::util::write_all_at;
use aes::Aes128;
use cbc::cipher::{BlockModeDecrypt, KeyIvInit, block_padding::Pkcs7};
use futures_util::StreamExt;
use reqwest::{Client, header};
use std::collections::BTreeMap;
use std::fs::File;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use url::Url;

/// Segments fetched ahead of the write cursor (bounds memory).
const WINDOW: usize = 24;

pub fn is_hls_url(u: &str) -> bool {
    Url::parse(u)
        .map(|u| u.path().to_ascii_lowercase().ends_with(".m3u8"))
        .unwrap_or(false)
}

pub fn is_hls_content_type(ct: &str) -> bool {
    let ct = ct.to_ascii_lowercase();
    ct.contains("mpegurl") // application/vnd.apple.mpegurl, audio/x-mpegurl, ...
}

#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    pub uri: Url,
    pub iv: Option<[u8; 16]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HSeg {
    pub url: Url,
    pub duration: f64,
    pub seq: u64,
    pub key: Option<Key>,
    /// (length, offset)
    pub range: Option<(u64, u64)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Media {
    pub segments: Vec<HSeg>,
    pub init: Option<(Url, Option<(u64, u64)>)>,
    pub fmp4: bool,
    pub duration: f64,
    /// No #EXT-X-ENDLIST: the stream is still being produced.
    pub live: bool,
    /// Target segment duration in seconds (how often a live playlist changes).
    pub target: f64,
}

#[derive(Debug, PartialEq)]
pub enum Parsed {
    Master(Url),
    Media(Media),
}

fn attrs(s: &str) -> Vec<(String, String)> {
    // KEY=VALUE,KEY="VALUE, with comma"
    let mut out = vec![];
    let (mut key, mut val, mut in_key, mut quoted) = (String::new(), String::new(), true, false);
    for c in s.chars().chain(std::iter::once(',')) {
        match c {
            '"' => quoted = !quoted,
            '=' if in_key && !quoted => in_key = false,
            ',' if !quoted => {
                if !key.is_empty() {
                    out.push((key.trim().to_ascii_uppercase(), val.clone()));
                }
                key.clear();
                val.clear();
                in_key = true;
            }
            c if in_key => key.push(c),
            c => val.push(c),
        }
    }
    out
}

fn get<'a>(a: &'a [(String, String)], k: &str) -> Option<&'a str> {
    a.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

fn parse_iv(s: &str) -> Option<[u8; 16]> {
    let h = s.trim_start_matches("0x").trim_start_matches("0X");
    if h.len() > 32 || h.is_empty() {
        return None;
    }
    let h = format!("{h:0>32}");
    let mut iv = [0u8; 16];
    for i in 0..16 {
        iv[i] = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(iv)
}

fn parse_range(s: &str, prev_end: u64) -> (u64, u64) {
    match s.split_once('@') {
        Some((l, o)) => (l.parse().unwrap_or(0), o.parse().unwrap_or(0)),
        None => (s.parse().unwrap_or(0), prev_end),
    }
}

/// Parse a playlist. A master playlist resolves to the highest-bandwidth variant.
pub fn parse(text: &str, base: &Url) -> Result<Parsed, Error> {
    let text = text.trim_start_matches('\u{feff}');
    if !text.trim_start().starts_with("#EXTM3U") {
        return Err(Error::Fatal("Not an HLS playlist".into()));
    }
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let bad = |m: &str| Error::Fatal(m.to_string());
    let join = |s: &str| {
        base.join(s)
            .map_err(|e| Error::Fatal(format!("bad playlist URL: {e}")))
    };

    if lines.iter().any(|l| l.starts_with("#EXT-X-STREAM-INF")) {
        let mut best: Option<(u64, Url)> = None;
        let mut i = 0;
        while i < lines.len() {
            if let Some(rest) = lines[i].strip_prefix("#EXT-X-STREAM-INF:") {
                let a = attrs(rest);
                let bw = get(&a, "BANDWIDTH")
                    .and_then(|b| b.parse().ok())
                    .unwrap_or(0);
                if let Some(uri) = lines[i + 1..].iter().find(|l| !l.starts_with('#')) {
                    let u = join(uri)?;
                    if best.as_ref().is_none_or(|(b, _)| bw > *b) {
                        best = Some((bw, u));
                    }
                }
            }
            i += 1;
        }
        return best
            .map(|(_, u)| Parsed::Master(u))
            .ok_or_else(|| bad("Playlist has no variants"));
    }

    let (mut seq, mut dur, mut key, mut init, mut fmp4) = (0u64, 0.0f64, None::<Key>, None, false);
    let mut target = 6.0f64;
    let (mut pending_dur, mut pending_range, mut prev_end) = (0.0, None::<(u64, u64)>, 0u64);
    let (mut segments, mut endlist) = (Vec::new(), false);
    for l in lines {
        if let Some(v) = l.strip_prefix("#EXT-X-TARGETDURATION:") {
            target = v.trim().parse().unwrap_or(6.0);
        } else if let Some(v) = l.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            seq = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = l.strip_prefix("#EXTINF:") {
            pending_dur = v
                .split(',')
                .next()
                .and_then(|d| d.trim().parse().ok())
                .unwrap_or(0.0);
        } else if let Some(v) = l.strip_prefix("#EXT-X-BYTERANGE:") {
            let r = parse_range(v.trim(), prev_end);
            prev_end = r.1 + r.0;
            pending_range = Some(r);
        } else if let Some(v) = l.strip_prefix("#EXT-X-KEY:") {
            let a = attrs(v);
            match get(&a, "METHOD").unwrap_or("NONE") {
                "NONE" => key = None,
                "AES-128" => {
                    let uri = get(&a, "URI").ok_or_else(|| bad("AES-128 key without URI"))?;
                    key = Some(Key {
                        uri: join(uri)?,
                        iv: get(&a, "IV").and_then(parse_iv),
                    });
                }
                m => return Err(bad(&format!("Encrypted stream ({m}) is not supported"))),
            }
        } else if let Some(v) = l.strip_prefix("#EXT-X-MAP:") {
            let a = attrs(v);
            let uri = get(&a, "URI").ok_or_else(|| bad("EXT-X-MAP without URI"))?;
            init = Some((join(uri)?, get(&a, "BYTERANGE").map(|r| parse_range(r, 0))));
            fmp4 = true;
        } else if l == "#EXT-X-ENDLIST" {
            endlist = true;
        } else if !l.starts_with('#') {
            segments.push(HSeg {
                url: join(l)?,
                duration: pending_dur,
                seq,
                key: key.clone(),
                range: pending_range.take(),
            });
            dur += pending_dur;
            seq += 1;
            pending_dur = 0.0;
        }
    }
    if segments.is_empty() && endlist {
        return Err(bad("Playlist has no segments"));
    }
    Ok(Parsed::Media(Media {
        segments,
        init,
        fmp4,
        duration: dur,
        live: !endlist,
        target,
    }))
}

fn decorate(mut req: reqwest::RequestBuilder, opts: &AddOptions) -> reqwest::RequestBuilder {
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
    req
}

fn status_err(s: reqwest::StatusCode) -> Error {
    let m = format!("HTTP {}", s.as_u16());
    if s.is_server_error() || s == 429 || s == 408 {
        Error::Retry(m)
    } else {
        Error::Fatal(m)
    }
}

async fn get_bytes(
    client: &Client,
    url: &Url,
    range: Option<(u64, u64)>,
    opts: &AddOptions,
    limiter: &Limiter,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, Error> {
    let mut req = decorate(client.get(url.clone()), opts);
    if let Some((len, off)) = range {
        req = req.header(
            header::RANGE,
            format!("bytes={}-{}", off, off + len.saturating_sub(1)),
        );
    }
    let resp = tokio::select! {
        _ = cancel.cancelled() => return Err(Error::Cancelled),
        r = req.send() => r.map_err(|e| Error::Retry(e.to_string()))?,
    };
    if !resp.status().is_success() {
        return Err(status_err(resp.status()));
    }
    let mut out = Vec::new();
    let mut s = resp.bytes_stream();
    loop {
        let next = tokio::select! {
            _ = cancel.cancelled() => return Err(Error::Cancelled),
            n = s.next() => n,
        };
        match next {
            Some(Ok(c)) => {
                limiter.acquire(c.len()).await;
                out.extend_from_slice(&c);
            }
            Some(Err(e)) => return Err(Error::Retry(e.to_string())),
            None => return Ok(out),
        }
    }
}

/// Fetch the playlist (following a master to its best variant). Returns the
/// media playlist and its URL.
pub async fn load(client: &Client, url: &str, opts: &AddOptions) -> Result<(Media, Url), Error> {
    let mut u = Url::parse(url).map_err(|e| Error::Fatal(e.to_string()))?;
    let limiter = Limiter::new(0);
    let cancel = CancellationToken::new();
    for _ in 0..3 {
        let mut last = None;
        let mut body = None;
        for n in 0..3u32 {
            match get_bytes(client, &u, None, opts, &limiter, &cancel).await {
                Ok(b) => {
                    body = Some(b);
                    break;
                }
                Err(Error::Retry(e)) => {
                    last = Some(e);
                    tokio::time::sleep(Duration::from_millis(500 << n)).await;
                }
                Err(e) => return Err(e),
            }
        }
        let body = body.ok_or_else(|| Error::Retry(last.unwrap_or_default()))?;
        match parse(&String::from_utf8_lossy(&body), &u)? {
            Parsed::Master(next) => u = next,
            Parsed::Media(m) => return Ok((m, u)),
        }
    }
    Err(Error::Fatal("Playlist nesting too deep".into()))
}

#[derive(Default)]
pub struct HlsShared {
    /// Recording a live stream: no total size, no time left.
    pub live: std::sync::atomic::AtomicBool,
    pub next: AtomicUsize,
    pub bytes: AtomicU64,
    pub total: AtomicUsize,
    pub active: AtomicU32,
}

pub struct HlsJob {
    pub client: Client,
    pub limiter: Arc<Limiter>,
    pub opts: AddOptions,
    pub media: Media,
    pub file: Arc<File>,
    pub connections: u32,
    pub cancel: CancellationToken,
    pub shared: Arc<HlsShared>,
}

fn decrypt(data: Vec<u8>, key: &[u8], iv: [u8; 16]) -> Result<Vec<u8>, Error> {
    if key.len() != 16 {
        return Err(Error::Fatal("Invalid AES-128 key length".into()));
    }
    let dec = cbc::Decryptor::<Aes128>::new_from_slices(key, &iv)
        .map_err(|e| Error::Fatal(e.to_string()))?;
    let mut data = data;
    let n = dec
        .decrypt_padded::<Pkcs7>(&mut data)
        .map_err(|_| Error::Fatal("Segment decryption failed (bad key or data)".into()))?
        .len();
    data.truncate(n);
    Ok(data)
}

/// Download everything from `shared.next` onward. Resumes where it left off.
pub async fn run(job: Arc<HlsJob>) -> Result<(), Error> {
    let total = job.media.segments.len();
    job.shared.total.store(total, Ordering::Relaxed);
    let start = job.shared.next.load(Ordering::Relaxed);

    // Init segment (fMP4) goes first, once.
    if start == 0
        && job.shared.bytes.load(Ordering::Relaxed) == 0
        && let Some((u, r)) = &job.media.init
    {
        let data = get_bytes(&job.client, u, *r, &job.opts, &job.limiter, &job.cancel).await?;
        let len = data.len() as u64;
        write_at(&job, data, 0).await?;
        // The media segments follow the init segment; without this they would overwrite it.
        job.shared.bytes.store(len, Ordering::Relaxed);
    }

    let fetch_next = Arc::new(AtomicUsize::new(start));
    let ready: Arc<Mutex<BTreeMap<usize, Vec<u8>>>> = Arc::default();
    let notify = Arc::new(Notify::new());
    let stop = job.cancel.child_token();
    let keys: Arc<tokio::sync::Mutex<std::collections::HashMap<String, Arc<Vec<u8>>>>> =
        Arc::default();

    let mut set = tokio::task::JoinSet::new();
    for _ in 0..job.connections.clamp(1, 32) {
        let (job, fetch_next, ready, notify, stop, keys) = (
            job.clone(),
            fetch_next.clone(),
            ready.clone(),
            notify.clone(),
            stop.clone(),
            keys.clone(),
        );
        set.spawn(async move {
            loop {
                let i = fetch_next.fetch_add(1, Ordering::SeqCst);
                if i >= total {
                    return Ok(());
                }
                // Don't run too far ahead of the writer.
                loop {
                    if stop.is_cancelled() {
                        return Err(Error::Cancelled);
                    }
                    if i < job.shared.next.load(Ordering::Relaxed) + WINDOW {
                        break;
                    }
                    tokio::select! {
                        _ = notify.notified() => {}
                        _ = tokio::time::sleep(Duration::from_millis(200)) => {}
                        _ = stop.cancelled() => return Err(Error::Cancelled),
                    }
                }
                job.shared.active.fetch_add(1, Ordering::Relaxed);
                let r = fetch_segment(&job, i, &keys, &stop).await;
                job.shared.active.fetch_sub(1, Ordering::Relaxed);
                ready.lock().unwrap().insert(i, r?);
                notify.notify_waiters();
            }
        });
    }

    // Writer: append segments strictly in order.
    let writer = {
        let (job, ready, notify, stop) = (job.clone(), ready.clone(), notify.clone(), stop.clone());
        async move {
            loop {
                let next = job.shared.next.load(Ordering::Relaxed);
                if next >= total {
                    return Ok(());
                }
                let data = ready.lock().unwrap().remove(&next);
                match data {
                    Some(d) => {
                        let off = job.shared.bytes.load(Ordering::Relaxed);
                        let len = d.len() as u64;
                        write_at(&job, d, off).await?;
                        job.shared.bytes.store(off + len, Ordering::Relaxed);
                        job.shared.next.store(next + 1, Ordering::Relaxed);
                        notify.notify_waiters();
                    }
                    None => {
                        tokio::select! {
                            _ = notify.notified() => {}
                            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                            _ = stop.cancelled() => return Err(Error::Cancelled),
                        }
                    }
                }
            }
        }
    };

    let mut result = Ok(());
    let wr = tokio::spawn(writer);
    while let Some(r) = set.join_next().await {
        match r {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                if result.is_ok() || matches!(result, Err(Error::Cancelled)) {
                    result = Err(e);
                }
                stop.cancel();
            }
            Err(e) => {
                result = Err(Error::Fatal(e.to_string()));
                stop.cancel();
            }
        }
    }
    notify.notify_waiters();
    match wr.await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            if result.is_ok() {
                result = Err(e);
            }
        }
        Err(e) => result = Err(Error::Fatal(e.to_string())),
    }
    if job.cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    result?;
    if job.shared.next.load(Ordering::Relaxed) < total {
        return Err(Error::Retry("download incomplete".into()));
    }
    Ok(())
}

async fn write_at(job: &HlsJob, data: Vec<u8>, off: u64) -> Result<(), Error> {
    let f = job.file.clone();
    tokio::task::spawn_blocking(move || write_all_at(&f, &data, off))
        .await
        .map_err(|e| Error::Fatal(e.to_string()))?
        .map_err(|e| Error::Fatal(format!("disk write failed: {e}")))
}

async fn fetch_segment(
    job: &HlsJob,
    i: usize,
    keys: &tokio::sync::Mutex<std::collections::HashMap<String, Arc<Vec<u8>>>>,
    stop: &CancellationToken,
) -> Result<Vec<u8>, Error> {
    let seg = &job.media.segments[i];
    let mut fails = 0u32;
    loop {
        let r = async {
            let mut data = get_bytes(
                &job.client,
                &seg.url,
                seg.range,
                &job.opts,
                &job.limiter,
                stop,
            )
            .await?;
            if let Some(k) = &seg.key {
                let key = {
                    let mut cache = keys.lock().await;
                    match cache.get(k.uri.as_str()) {
                        Some(v) => v.clone(),
                        None => {
                            let v = Arc::new(
                                get_bytes(&job.client, &k.uri, None, &job.opts, &job.limiter, stop)
                                    .await?,
                            );
                            cache.insert(k.uri.to_string(), v.clone());
                            v
                        }
                    }
                };
                let iv = k.iv.unwrap_or_else(|| (seg.seq as u128).to_be_bytes());
                data = decrypt(data, &key, iv)?;
            }
            Ok::<_, Error>(data)
        }
        .await;
        match r {
            Ok(d) => return Ok(d),
            Err(Error::Retry(e)) => {
                fails += 1;
                if fails >= 5 {
                    return Err(Error::Retry(format!("segment {i}: {e}")));
                }
                tokio::select! {
                    _ = stop.cancelled() => return Err(Error::Cancelled),
                    _ = tokio::time::sleep(Duration::from_millis(400 << fails)) => {}
                }
            }
            Err(e) => return Err(e),
        }
    }
}

// ---- qualities (variants) ---------------------------------------------------------------------------

/// All qualities of a master playlist, best first. A media playlist has none.
pub fn parse_variants(text: &str, base: &Url) -> Vec<crate::types::StreamVariant> {
    let lines: Vec<&str> = text
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    // audio renditions: group id -> (uri, default)
    let mut audio: Vec<(String, Option<Url>, bool)> = vec![];
    for l in &lines {
        if let Some(rest) = l.strip_prefix("#EXT-X-MEDIA:") {
            let a = attrs(rest);
            if get(&a, "TYPE") == Some("AUDIO") {
                let uri = get(&a, "URI").and_then(|u| base.join(u).ok());
                audio.push((
                    get(&a, "GROUP-ID").unwrap_or("").to_string(),
                    uri,
                    get(&a, "DEFAULT") == Some("YES"),
                ));
            }
        }
    }
    let mut out: Vec<crate::types::StreamVariant> = vec![];
    for (i, l) in lines.iter().enumerate() {
        let Some(rest) = l.strip_prefix("#EXT-X-STREAM-INF:") else {
            continue;
        };
        let a = attrs(rest);
        let Some(uri) = lines[i + 1..].iter().find(|x| !x.starts_with('#')) else {
            continue;
        };
        let Ok(url) = base.join(uri) else { continue };
        let bandwidth: u64 = get(&a, "BANDWIDTH")
            .and_then(|b| b.parse().ok())
            .unwrap_or(0);
        let height = get(&a, "RESOLUTION")
            .and_then(|r| r.split('x').nth(1))
            .and_then(|h| h.parse::<u32>().ok());
        let audio_url = get(&a, "AUDIO").and_then(|g| {
            let in_group: Vec<_> = audio
                .iter()
                .filter(|(gid, u, _)| gid == g && u.is_some())
                .collect();
            in_group
                .iter()
                .find(|(_, _, d)| *d)
                .or(in_group.first())
                .and_then(|(_, u, _)| u.clone())
        });
        let label = match height {
            Some(h) => format!("{h}p · {:.1} Mbps", bandwidth as f64 / 1e6),
            None => format!("{:.1} Mbps", bandwidth as f64 / 1e6),
        };
        if out.iter().any(|v| v.url == url.as_str()) {
            continue;
        }
        out.push(crate::types::StreamVariant {
            url: url.to_string(),
            label,
            bandwidth,
            height,
            audio_url: audio_url.map(|u| u.to_string()),
        });
    }
    out.sort_by_key(|v| std::cmp::Reverse(v.bandwidth));
    out
}

/// Fetch a playlist and list its qualities (empty for a plain media playlist).
pub async fn variants(
    client: &Client,
    url: &str,
    opts: &AddOptions,
) -> Result<Vec<crate::types::StreamVariant>, Error> {
    let u = Url::parse(url).map_err(|e| Error::Fatal(e.to_string()))?;
    let body = get_bytes(
        client,
        &u,
        None,
        opts,
        &Limiter::new(0),
        &CancellationToken::new(),
    )
    .await?;
    Ok(parse_variants(&String::from_utf8_lossy(&body), &u))
}

// ---- live recording ---------------------------------------------------------------------------------

/// Record a live stream: keep reloading the playlist and append every new segment
/// until the stream ends (#EXT-X-ENDLIST) or the download is cancelled.
pub async fn run_live(job: Arc<HlsJob>, playlist: Url) -> Result<(), Error> {
    job.shared.live.store(true, Ordering::Relaxed);
    let mut media = job.media.clone();
    let mut last_seq: Option<u64> = None;
    let mut failures = 0u32;
    loop {
        if job.cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let fresh: Vec<HSeg> = media
            .segments
            .iter()
            .filter(|s| last_seq.is_none_or(|l| s.seq > l))
            .cloned()
            .collect();
        if !fresh.is_empty() {
            last_seq = fresh.last().map(|s| s.seq);
            job.shared.next.store(0, Ordering::Relaxed);
            let pass = Arc::new(HlsJob {
                client: job.client.clone(),
                limiter: job.limiter.clone(),
                opts: job.opts.clone(),
                media: Media {
                    segments: fresh,
                    init: media.init.clone(),
                    fmp4: media.fmp4,
                    duration: 0.0,
                    live: false,
                    target: media.target,
                },
                file: job.file.clone(),
                connections: job.connections,
                cancel: job.cancel.clone(),
                shared: job.shared.clone(),
            });
            run(pass).await?;
        }
        if !media.live {
            return Ok(());
        }
        let wait = Duration::from_secs_f64((media.target / 2.0).clamp(1.0, 6.0));
        tokio::select! {
            _ = job.cancel.cancelled() => return Err(Error::Cancelled),
            _ = tokio::time::sleep(wait) => {}
        }
        match load(&job.client, playlist.as_str(), &job.opts).await {
            Ok((m, _)) => {
                failures = 0;
                media = m;
            }
            Err(Error::Retry(e)) => {
                failures += 1;
                if failures > 20 {
                    return Err(Error::Retry(e));
                }
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://cdn.example.com/v/index.m3u8").unwrap()
    }

    #[test]
    fn master_picks_highest_bandwidth() {
        let t = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\nlow/p.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=5000000,CODECS=\"avc1.64001f,mp4a.40.2\"\nhi/p.m3u8\n";
        assert_eq!(
            parse(t, &base()).unwrap(),
            Parsed::Master(Url::parse("https://cdn.example.com/v/hi/p.m3u8").unwrap())
        );
    }

    #[test]
    fn media_playlist_with_key_and_relative_urls() {
        let t = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:7\n#EXT-X-KEY:METHOD=AES-128,URI=\"k.bin\",IV=0x01\n#EXTINF:4.0,\na.ts\n#EXTINF:2.5,\n/b.ts\n#EXT-X-ENDLIST\n";
        let Parsed::Media(m) = parse(t, &base()).unwrap() else {
            panic!()
        };
        assert_eq!(m.segments.len(), 2);
        assert_eq!(m.segments[0].seq, 7);
        assert_eq!(m.segments[1].url.as_str(), "https://cdn.example.com/b.ts");
        assert_eq!(m.segments[0].key.as_ref().unwrap().iv.unwrap()[15], 1);
        assert!((m.duration - 6.5).abs() < 1e-9 && !m.fmp4);
    }

    #[test]
    fn live_playlists_are_flagged_and_drm_is_rejected() {
        let Parsed::Media(m) = parse(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:7\n#EXTINF:4,\na.ts\n",
            &base(),
        )
        .unwrap() else {
            panic!()
        };
        assert!(m.live);
        assert_eq!(m.target, 4.0);
        assert_eq!(m.segments[0].seq, 7);
        let drm =
            "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"x\"\n#EXTINF:4,\na.ts\n#EXT-X-ENDLIST\n";
        assert!(matches!(parse(drm, &base()), Err(Error::Fatal(m)) if m.contains("SAMPLE-AES")));
        assert!(parse("<html>", &base()).is_err());
    }

    #[test]
    fn fmp4_map_and_byteranges() {
        let t = "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4,\n#EXT-X-BYTERANGE:1000@0\nf.mp4\n#EXTINF:4,\n#EXT-X-BYTERANGE:500\nf.mp4\n#EXT-X-ENDLIST\n";
        let Parsed::Media(m) = parse(t, &base()).unwrap() else {
            panic!()
        };
        assert!(m.fmp4);
        assert_eq!(m.segments[0].range, Some((1000, 0)));
        assert_eq!(m.segments[1].range, Some((500, 1000)));
    }
}
