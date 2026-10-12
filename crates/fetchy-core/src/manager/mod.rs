mod driver;
mod grabber;
mod queues;
mod streams;

use crate::engine::{self, Job, Shared};
use crate::hls::{self, HlsJob, HlsShared};
use crate::limiter::Limiter;
use crate::types::*;
use crate::util::*;
use reqwest::Client;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// After a crash the last persisted positions can run slightly ahead of what
/// reached the disk; re-fetch this much to be safe.
const CRASH_REWIND: u64 = 1 << 20;

struct Running {
    cancel: CancellationToken,
    shared: Option<Arc<Shared>>,
    hls: Option<Arc<HlsShared>>,
    handle: Option<JoinHandle<()>>,
    last_bytes: u64,
    last_t: Instant,
    ema: f64,
}

struct Item {
    rec: Record,
    run: Option<Running>,
}

struct State {
    settings: Settings,
    items: Vec<Item>,
}

struct Inner {
    state: Mutex<State>,
    events: broadcast::Sender<Event>,
    client: std::sync::RwLock<Client>,
    stats: Mutex<Vec<crate::extras::DayStat>>,
    limiter: Arc<Limiter>,
    /// Per-queue limiters (speed + hourly quota), each chained to the global one.
    qlimiters: Mutex<std::collections::HashMap<String, Arc<Limiter>>>,
    synced: Mutex<std::collections::HashMap<String, Instant>>,
    /// Queues the user started by hand (or at startup): they run regardless of their schedule until idle.
    forced: Mutex<std::collections::HashSet<String>>,
    /// Mux groups being merged right now.
    muxing: Mutex<std::collections::HashSet<String>>,
    /// Result of the most recent site crawl, so "download these files" does not crawl twice.
    last_crawl: Mutex<Option<(String, Arc<crate::grab::CrawlResult>)>>,
    started: Mutex<std::collections::HashMap<String, Instant>>,
    projects: Mutex<Vec<GrabProject>>,
    running_projects: Mutex<std::collections::HashSet<String>>,
    dir: PathBuf,
    /// Captured at creation: UI frameworks call us from threads that have no runtime.
    rt: tokio::runtime::Handle,
}

/// (url, filename, options, size, validator) of a synchronized file to re-check.
type SyncDue = (String, String, AddOptions, Option<u64>, Option<String>);

#[derive(Clone)]
pub struct Manager {
    inner: Arc<Inner>,
}

impl Manager {
    /// Must be called from inside a tokio runtime (it spawns the progress ticker).
    pub fn new(data_dir: impl Into<PathBuf>) -> anyhow::Result<Manager> {
        let dir: PathBuf = data_dir.into();
        std::fs::create_dir_all(&dir)?;
        // The download history feature was removed; drop its leftover file.
        let _ = std::fs::remove_file(dir.join("history.json"));
        let mut settings: Settings = std::fs::read(dir.join("settings.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        ensure_builtin_queues(&mut settings.queues);
        let mut records: Vec<Record> = std::fs::read(dir.join("downloads.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        for r in &mut records {
            if matches!(r.info.status, Status::Downloading | Status::Queued) {
                r.info.status = Status::Paused;
                for s in &mut r.segments {
                    s.pos = s.pos.saturating_sub(CRASH_REWIND).max(s.start);
                }
            }
            r.info.speed = 0;
            r.info.eta_secs = None;
            r.info.active_connections = 0;
            if r.info.status != Status::Complete {
                r.info.downloaded = r.segments.iter().map(|s| s.pos - s.start).sum();
            }
        }
        let (tx, _) = broadcast::channel(1024);
        let limiter = Arc::new(Limiter::new(settings.speed_limit));
        let client = engine::build_client(&settings.user_agent, &settings.proxy)?;
        let inner = Arc::new(Inner {
            state: Mutex::new(State {
                settings,
                items: records
                    .into_iter()
                    .map(|rec| Item { rec, run: None })
                    .collect(),
            }),
            events: tx,
            client: std::sync::RwLock::new(client),
            stats: Mutex::new(crate::extras::load_stats(&dir)),
            limiter,
            qlimiters: Mutex::new(Default::default()),
            synced: Mutex::new(Default::default()),
            projects: Mutex::new(
                std::fs::read(dir.join("projects.json"))
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
                    .unwrap_or_default(),
            ),
            running_projects: Mutex::new(Default::default()),
            forced: Mutex::new(Default::default()),
            muxing: Mutex::new(Default::default()),
            last_crawl: Mutex::new(None),
            started: Mutex::new(Default::default()),
            dir,
            rt: tokio::runtime::Handle::current(),
        });
        let m = Manager { inner };
        let t = m.clone();
        m.inner.rt.spawn(async move { t.ticker().await });
        let startup: Vec<String> = m
            .queues()
            .into_iter()
            .filter(|q| q.start_on_startup)
            .map(|q| q.id)
            .collect();
        for q in startup {
            m.start_queue(&q);
        }
        Ok(m)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }

    pub fn list(&self) -> Vec<DownloadInfo> {
        self.inner
            .state
            .lock()
            .unwrap()
            .items
            .iter()
            .map(|i| i.rec.info.clone())
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<DownloadInfo> {
        self.with(id, |i| i.rec.info.clone())
    }

    pub fn settings(&self) -> Settings {
        self.inner.state.lock().unwrap().settings.clone()
    }

    pub fn queues(&self) -> Vec<Queue> {
        self.inner.state.lock().unwrap().settings.queues.clone()
    }

    pub fn set_settings(&self, mut s: Settings) {
        ensure_builtin_queues(&mut s.queues);
        {
            let old = self.inner.state.lock().unwrap().settings.clone();
            let p = |x: &ProxySettings| {
                format!(
                    "{}|{}|{}|{}|{}",
                    x.kind, x.host, x.port, x.username, x.password
                )
            };
            if p(&old.proxy) != p(&s.proxy) || old.user_agent != s.user_agent {
                match engine::build_client(&s.user_agent, &s.proxy) {
                    Ok(c) => *self.inner.client.write().unwrap() = c,
                    Err(_) => s.proxy = old.proxy.clone(), // an unusable proxy address keeps the old connection settings
                }
            }
        }
        self.inner.limiter.set(s.speed_limit);
        {
            let mut ql = self.inner.qlimiters.lock().unwrap();
            ql.retain(|id, _| s.queues.iter().any(|q| &q.id == id));
            for q in &s.queues {
                if let Some(l) = ql.get(&q.id) {
                    l.set(q.speed_limit);
                    l.set_quota(q.quota_per_hour);
                }
            }
        }
        {
            let mut st = self.inner.state.lock().unwrap();
            // Downloads whose queue was deleted fall back to the main queue.
            for item in st.items.iter_mut() {
                if !s.queues.iter().any(|q| q.id == item.rec.info.queue) {
                    item.rec.info.queue = main_queue();
                }
            }
            st.settings = s.clone();
        }
        if let Ok(b) = serde_json::to_vec_pretty(&s) {
            let _ = std::fs::write(self.inner.dir.join("settings.json"), b);
        }
        self.pump();
    }

    /// Where a fresh download should be saved: an explicit folder wins, otherwise
    /// the default folder, split into per-category subfolders when organising.
    fn target_dir(&self, rec: &Record, filename: &str, settings: &Settings) -> PathBuf {
        if let Some(d) = &rec.opts.dir {
            return PathBuf::from(d);
        }
        let base = PathBuf::from(&settings.default_dir);
        if !settings.organize {
            return base;
        }
        let ext = filename
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let host = url::Url::parse(&rec.info.url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_default();
        let hit = settings.categories.iter().find(|r| {
            r.exts
                .iter()
                .any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(&ext))
                || r.sites
                    .iter()
                    .any(|s| crate::extras::host_matches(s, &host))
                || r.names
                    .iter()
                    .any(|n| crate::extras::glob_match(n, filename))
        });
        if let Some(r) = hit {
            return base.join(crate::util::sanitize_filename(&r.folder));
        }
        base.join(category_folder(categorize(filename)))
    }

    /// Per-connection view for the details panel.
    pub fn details(&self, id: &str) -> Option<Details> {
        self.with(id, |i| {
            let segs = i
                .run
                .as_ref()
                .and_then(|r| r.shared.as_ref())
                .map(|s| s.snapshot())
                .unwrap_or_else(|| i.rec.segments.clone());
            let hls = i
                .run
                .as_ref()
                .and_then(|r| r.hls.as_ref())
                .map(|h| hls_state(h))
                .or_else(|| i.rec.hls.clone());
            Details {
                segments: segs
                    .iter()
                    .map(|s| SegView {
                        start: s.start,
                        pos: s.pos,
                        end: if s.end == u64::MAX { s.pos } else { s.end },
                        active: s.active,
                    })
                    .collect(),
                hls,
            }
        })
    }

    /// Quick update: is there a newer GitHub release?
    pub async fn check_update(&self) -> Result<Option<crate::types::UpdateInfo>, Error> {
        crate::update::check(&self.client(), crate::update::REPO).await
    }

    pub async fn inspect(&self, url: &str) -> Option<LinkInfo> {
        let parsed = url::Url::parse(url.trim()).ok()?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return None;
        }
        if self.list().iter().any(|i| i.url == parsed.as_str()) {
            return None;
        }
        if hls::is_hls_url(parsed.as_str()) || crate::dash::is_dash_url(parsed.as_str()) {
            let name = format!(
                "{}.{}",
                hls_stem(parsed.as_str()),
                if crate::dash::is_dash_url(parsed.as_str()) {
                    "mp4"
                } else {
                    "ts"
                }
            );
            return Some(LinkInfo {
                url: parsed.to_string(),
                category: categorize(&name),
                filename: name,
                size: None,
                resumable: true,
                stream: true,
            });
        }
        let p = tokio::time::timeout(
            Duration::from_secs(6),
            engine::probe(&self.client(), parsed.as_str(), &AddOptions::default()),
        )
        .await
        .ok()?
        .ok()?;
        let ct = p.content_type.to_ascii_lowercase();
        let page_like = ct.starts_with("text/")
            || ["html", "json", "xml", "javascript", "x-www-form"]
                .iter()
                .any(|k| ct.contains(k));
        let known_ext = categorize(&p.filename) != Category::Other;
        if hls::is_hls_content_type(&ct) {
            let name = format!("{}.ts", hls_stem(parsed.as_str()));
            return Some(LinkInfo {
                url: parsed.to_string(),
                category: Category::Video,
                filename: name,
                size: None,
                resumable: true,
                stream: true,
            });
        }
        if page_like && !p.attachment && !known_ext {
            return None;
        }
        Some(LinkInfo {
            url: parsed.to_string(),
            category: categorize(&p.filename),
            filename: p.filename,
            size: p.size,
            resumable: p.resumable,
            stream: false,
        })
    }

    pub fn full_path(&self, id: &str) -> Option<PathBuf> {
        self.get(id).map(|i| Path::new(&i.dir).join(&i.filename))
    }

    pub fn add(&self, url: &str, opts: AddOptions) -> Result<DownloadInfo, String> {
        let parsed = url::Url::parse(url.trim()).map_err(|e| format!("Invalid URL: {e}"))?;
        if !matches!(parsed.scheme(), "http" | "https" | "ftp") {
            return Err("Only http(s) and ftp links are supported".into());
        }
        let mut opts = opts;
        let (settings_dir, conns) = {
            let st = self.inner.state.lock().unwrap();
            if let Some(h) = parsed.host_str()
                && let Some(p) = st
                    .settings
                    .site_profiles
                    .iter()
                    .find(|p| crate::extras::host_matches(&p.host, h))
            {
                if opts.username.is_none() && !p.username.is_empty() {
                    opts.username = Some(p.username.clone());
                    opts.password = Some(p.password.clone());
                }
                if opts.cookie.is_none() && !p.cookie.is_empty() {
                    opts.cookie = Some(p.cookie.clone());
                }
                if opts.referer.is_none() && !p.referer.is_empty() {
                    opts.referer = Some(p.referer.clone());
                }
            }
            (st.settings.default_dir.clone(), st.settings.connections)
        };
        let dir = opts.dir.clone().unwrap_or(settings_dir);
        let filename = opts
            .filename
            .clone()
            .map(|f| sanitize_filename(&f))
            .unwrap_or_else(|| filename_from_url(&parsed));
        let start = opts.start.unwrap_or(true);
        let queue_id = {
            let st = self.inner.state.lock().unwrap();
            opts.queue
                .clone()
                .filter(|q| st.settings.queues.iter().any(|x| &x.id == q))
                .unwrap_or_else(main_queue)
        };
        let info = DownloadInfo {
            id: uuid::Uuid::new_v4().to_string(),
            url: parsed.to_string(),
            category: categorize(&filename),
            filename,
            dir,
            size: None,
            downloaded: 0,
            status: if start {
                Status::Queued
            } else {
                Status::Paused
            },
            speed: 0,
            eta_secs: None,
            error: None,
            added_at: now_secs(),
            completed_at: None,
            resumable: false,
            connections: opts.connections.unwrap_or(conns).clamp(1, 64),
            active_connections: 0,
            queue: queue_id,
            last_try_at: None,
            description: opts.description.clone().unwrap_or_default(),
            referer: opts.referer.clone(),
            parent: opts.parent.clone().or_else(|| opts.referer.clone()),
            project: opts.project.clone(),
            scan_status: None,
            verified: None,
        };
        let rec = Record {
            info: info.clone(),
            opts,
            segments: vec![],
            etag: None,
            hls: None,
            retries_used: 0,
        };
        self.inner
            .state
            .lock()
            .unwrap()
            .items
            .insert(0, Item { rec, run: None });
        self.emit(&info);
        self.save();
        self.pump();
        Ok(info)
    }

    pub fn set_description(&self, id: &str, text: &str) {
        let text = text.to_string();
        self.mutate(id, |i| i.rec.info.description = text);
        self.save();
    }

    pub fn pause(&self, id: &str) {
        let cancel = self.mutate(id, |i| {
            if matches!(i.rec.info.status, Status::Downloading | Status::Queued) {
                i.rec.info.status = Status::Paused;
                i.rec.info.speed = 0;
                i.rec.info.eta_secs = None;
                i.run.as_ref().map(|r| r.cancel.clone())
            } else {
                None
            }
        });
        if let Some(Some(c)) = cancel {
            c.cancel();
        }
        self.save();
        self.pump();
    }

    pub fn resume(&self, id: &str) {
        self.mutate(id, |i| {
            i.rec.retries_used = 0;
            if matches!(i.rec.info.status, Status::Paused | Status::Error) {
                i.rec.info.status = Status::Queued;
                i.rec.info.error = None;
            }
        });
        self.pump();
    }

    pub fn pause_all(&self) {
        for i in self.list() {
            self.pause(&i.id);
        }
    }

    pub fn resume_all(&self) {
        for i in self.list() {
            self.resume(&i.id);
        }
    }

    /// Throw away progress and fetch again from byte zero.
    pub async fn redownload(&self, id: &str) {
        self.stop_and_wait(id).await;
        let paths = self.mutate(id, |i| {
            let p = Path::new(&i.rec.info.dir).join(&i.rec.info.filename);
            i.rec.segments.clear();
            i.rec.hls = None;
            i.rec.info.downloaded = 0;
            i.rec.info.size = None;
            i.rec.info.status = Status::Queued;
            i.rec.info.error = None;
            i.rec.info.completed_at = None;
            p
        });
        if let Some(p) = paths {
            let _ = std::fs::remove_file(part_path(&p));
            let _ = std::fs::remove_file(&p);
        }
        self.pump();
    }

    pub async fn remove(&self, id: &str, delete_file: bool) {
        self.stop_and_wait(id).await;
        let item = {
            let mut st = self.inner.state.lock().unwrap();
            st.items
                .iter()
                .position(|i| i.rec.info.id == id)
                .map(|p| st.items.remove(p))
        };
        if let Some(item) = item {
            let p = Path::new(&item.rec.info.dir).join(&item.rec.info.filename);
            // A partial file is useless once the entry is gone.
            if item.rec.info.status != Status::Complete {
                let _ = std::fs::remove_file(part_path(&p));
            } else if delete_file {
                trash_file(&p);
            }
            let _ = self.inner.events.send(Event::Removed(id.to_string()));
            self.save();
            self.pump();
        }
    }

    async fn stop_and_wait(&self, id: &str) {
        let h = self.mutate(id, |i| {
            // Only active items change state; a finished download must stay "complete"
            // (remove() relies on that to know it should delete the real file).
            if matches!(i.rec.info.status, Status::Downloading | Status::Queued) {
                i.rec.info.status = Status::Paused;
            }
            i.run.as_mut().map(|r| {
                r.cancel.cancel();
                r.handle.take()
            })
        });
        if let Some(Some(Some(h))) = h {
            let _ = h.await;
        }
    }

    // ---- internals -------------------------------------------------------

    fn with<R>(&self, id: &str, f: impl FnOnce(&Item) -> R) -> Option<R> {
        let st = self.inner.state.lock().unwrap();
        st.items.iter().find(|i| i.rec.info.id == id).map(f)
    }

    /// Mutate an item and broadcast the new row state.
    fn mutate<R>(&self, id: &str, f: impl FnOnce(&mut Item) -> R) -> Option<R> {
        let (r, info) = {
            let mut st = self.inner.state.lock().unwrap();
            let item = st.items.iter_mut().find(|i| i.rec.info.id == id)?;
            let r = f(item);
            (r, item.rec.info.clone())
        };
        self.emit(&info);
        Some(r)
    }

    fn emit(&self, info: &DownloadInfo) {
        let _ = self.inner.events.send(Event::Updated(info.clone()));
    }

    fn save(&self) {
        let st = self.inner.state.lock().unwrap();
        let recs: Vec<Record> = st
            .items
            .iter()
            .map(|i| {
                let mut r = i.rec.clone();
                if let Some(sh) = i.run.as_ref().and_then(|r| r.shared.as_ref()) {
                    r.segments = sh.snapshot();
                }
                if let Some(h) = i.run.as_ref().and_then(|r| r.hls.as_ref()) {
                    r.hls = Some(hls_state(h));
                }
                r
            })
            .collect();
        drop(st);
        if let Ok(b) = serde_json::to_vec(&recs) {
            let tmp = self.inner.dir.join("downloads.json.tmp");
            if std::fs::write(&tmp, b).is_ok() {
                let _ = std::fs::rename(tmp, self.inner.dir.join("downloads.json"));
            }
        }
    }

    /// Start queued downloads until the concurrency limit is reached.
    fn pump(&self) {
        let mut to_start = vec![];
        {
            let mut st = self.inner.state.lock().unwrap();
            let max = st.settings.max_concurrent.max(1) as usize;
            let queues = st.settings.queues.clone();
            let forced = self.inner.forced.lock().unwrap().clone();
            let active: std::collections::HashMap<String, (bool, usize)> = queues
                .iter()
                .map(|q| {
                    (
                        q.id.clone(),
                        (
                            q.is_active_now() || forced.contains(&q.id),
                            q.max_concurrent.max(1) as usize,
                        ),
                    )
                })
                .collect();
            let mut per_queue: std::collections::HashMap<String, usize> = Default::default();
            let mut running = 0usize;
            for i in st
                .items
                .iter()
                .filter(|i| i.run.is_some() && i.rec.info.status == Status::Downloading)
            {
                *per_queue.entry(i.rec.info.queue.clone()).or_default() += 1;
                running += 1;
            }
            // Oldest first, so the queue behaves like a queue.
            for item in st.items.iter_mut().rev() {
                if running >= max {
                    break;
                }
                let (q_active, q_max) = active
                    .get(&item.rec.info.queue)
                    .copied()
                    .unwrap_or((true, max));
                let q_running = per_queue.entry(item.rec.info.queue.clone()).or_default();
                if !q_active || *q_running >= q_max {
                    continue;
                }
                if item.rec.info.status == Status::Queued && item.run.is_none() {
                    *q_running += 1;
                    item.rec.info.status = Status::Downloading;
                    item.rec.info.last_try_at = Some(now_secs());
                    let cancel = CancellationToken::new();
                    item.run = Some(Running {
                        cancel: cancel.clone(),
                        shared: None,
                        hls: None,
                        handle: None,
                        last_bytes: item.rec.info.downloaded,
                        last_t: Instant::now(),
                        ema: 0.0,
                    });
                    to_start.push((item.rec.info.id.clone(), cancel));
                    running += 1;
                }
            }
        }
        for (id, cancel) in to_start {
            let m = self.clone();
            let id2 = id.clone();
            let h = self.inner.rt.spawn(async move {
                m.drive(&id2, cancel).await;
                m.mutate(&id2, |i| i.run = None);
                m.save();
                m.pump();
            });
            self.mutate(&id, |i| {
                if let Some(r) = i.run.as_mut() {
                    r.handle = Some(h);
                }
            });
        }
    }

    pub fn client(&self) -> Client {
        self.inner.client.read().unwrap().clone()
    }

    /// A network error: try again later (growing delay) up to the configured number of times, then give up.
    fn fail_or_retry(&self, id: &str, e: Error) {
        let max = self.settings().auto_retry;
        let used = self.with(id, |i| i.rec.retries_used).unwrap_or(0);
        if matches!(e, Error::Retry(_)) && used < max {
            let delay = 5u64 << used.min(5);
            let msg = format!("{e} - retrying in {delay}s ({}/{max})", used + 1);
            self.fail(id, msg);
            self.mutate(id, |i| i.rec.retries_used += 1);
            let me = self.clone();
            let id = id.to_string();
            self.inner.rt.spawn(async move {
                tokio::time::sleep(Duration::from_secs(delay)).await;
                me.mutate(&id, |i| {
                    // untouched since the failure (the user may have paused or removed it meanwhile)
                    if i.rec.info.status == Status::Error && i.rec.retries_used == used + 1 {
                        i.rec.info.status = Status::Queued;
                        i.rec.info.error = None;
                    }
                });
                me.pump();
            });
        } else {
            self.fail(id, e.to_string());
        }
    }

    /// Everything that happens once a file is on disk: checksum, virus scan, user command.
    fn on_complete(&self, id: &str) {
        let Some((info, opts)) = self.with(id, |i| (i.rec.info.clone(), i.rec.opts.clone())) else {
            return;
        };
        let settings = self.settings();
        if let Some(g) = opts.mux_group.clone() {
            self.try_mux(&g);
        }
        let expected = opts.checksum.clone().filter(|c| !c.trim().is_empty());
        if expected.is_none() && !settings.scan_defender && settings.post_command.trim().is_empty()
        {
            return;
        }
        let me = self.clone();
        let id = id.to_string();
        let path = Path::new(&info.dir).join(&info.filename);
        tokio::task::spawn_blocking(move || {
            if let Some(exp) = expected
                && let Ok(r) = crate::extras::verify(&path, &exp)
            {
                me.mutate(&id, |i| i.rec.info.verified = Some(r.ok));
            }
            if settings.scan_defender {
                let r = crate::extras::defender_scan(&path);
                me.mutate(&id, |i| i.rec.info.scan_status = Some(r.to_string()));
            }
            if !settings.post_command.trim().is_empty() {
                let _ = crate::extras::run_command(&settings.post_command, &path, &info.url);
            }
            me.save();
        });
    }

    /// Bytes moved per day, oldest first (the last 120 days that had traffic).
    pub fn stats(&self) -> Vec<crate::extras::DayStat> {
        self.inner.stats.lock().unwrap().clone()
    }

    fn add_traffic(&self, bytes: u64) {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let mut s = self.inner.stats.lock().unwrap();
        crate::extras::add_bytes(&mut s, &today, bytes);
    }

    /// Hash a finished file and compare it with a checksum the user pasted.
    pub async fn verify_checksum(
        &self,
        id: &str,
        expected: &str,
    ) -> Result<crate::extras::ChecksumResult, Error> {
        let path = self
            .full_path(id)
            .ok_or_else(|| Error::Fatal("Unknown download".into()))?;
        let expected = expected.to_string();
        let r = tokio::task::spawn_blocking(move || crate::extras::verify(&path, &expected))
            .await
            .map_err(|e| Error::Fatal(e.to_string()))??;
        self.mutate(id, |i| i.rec.info.verified = Some(r.ok));
        Ok(r)
    }

    fn fail(&self, id: &str, msg: String) {
        self.mutate(id, |i| {
            if i.rec.info.status == Status::Downloading {
                i.rec.info.status = Status::Error;
                i.rec.info.error = Some(msg);
                i.rec.info.speed = 0;
                i.rec.info.eta_secs = None;
                i.rec.info.active_connections = 0;
            }
        });
    }
}

fn hls_state(h: &HlsShared) -> HlsState {
    HlsState {
        next: h.next.load(Ordering::Relaxed),
        bytes: h.bytes.load(Ordering::Relaxed),
        total: h.total.load(Ordering::Relaxed),
    }
}

/// A readable file stem for a stream URL ("…/my-video/index.m3u8" becomes "my-video").
fn hls_stem(url: &str) -> String {
    let Ok(u) = url::Url::parse(url) else {
        return "stream".into();
    };
    let segs: Vec<&str> = u
        .path_segments()
        .map(|s| s.filter(|x| !x.is_empty()).collect())
        .unwrap_or_default();
    let last = segs.last().copied().unwrap_or("stream");
    let stem = last.rsplit_once('.').map(|(a, _)| a).unwrap_or(last);
    let generic = [
        "index",
        "playlist",
        "master",
        "main",
        "chunklist",
        "stream",
        "video",
        "manifest",
        "hls",
    ];
    let pick = if generic.contains(&stem.to_ascii_lowercase().as_str()) && segs.len() >= 2 {
        segs[segs.len() - 2]
    } else {
        stem
    };
    sanitize_filename(&percent_encoding::percent_decode_str(pick).decode_utf8_lossy())
}

fn has_video_ext(name: &str) -> bool {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    matches!(ext.as_str(), "ts" | "mp4" | "m4v" | "mkv" | "mov" | "webm")
}

/// Move to the Recycle Bin / Trash when the OS supports it, else delete outright.
fn trash_file(p: &Path) {
    if p.exists() && trash::delete(p).is_err() {
        let _ = std::fs::remove_file(p);
    }
}
