//! The download loops: one task per active download, plus the once-a-tick housekeeping.

use super::*;

impl Manager {
    pub(super) async fn drive(&self, id: &str, cancel: CancellationToken) {
        let Some((rec, settings)) = ({
            let st = self.inner.state.lock().unwrap();
            st.items
                .iter()
                .find(|i| i.rec.info.id == id)
                .map(|i| (i.rec.clone(), st.settings.clone()))
        }) else {
            return;
        };

        if hls::is_hls_url(&rec.info.url) || crate::dash::is_dash_url(&rec.info.url) {
            return self.drive_hls(id, rec, settings, cancel).await;
        }

        // 1. Probe (refreshes the final URL and detects a changed remote file).
        let mut probe = None;
        let mut last = String::new();
        for attempt in 0..3u32 {
            if cancel.is_cancelled() {
                return;
            }
            match engine::probe(&self.client(), &rec.info.url, &rec.opts).await {
                Ok(p) => {
                    probe = Some(p);
                    break;
                }
                Err(Error::Fatal(e)) => {
                    self.fail(id, e);
                    return;
                }
                Err(e) => {
                    last = e.to_string();
                    tokio::select! {
                        _ = cancel.cancelled() => return,
                        _ = tokio::time::sleep(Duration::from_millis(600 << attempt)) => {}
                    }
                }
            }
        }
        let Some(probe) = probe else {
            self.fail(id, last);
            return;
        };

        if hls::is_hls_content_type(&probe.content_type) {
            return self.drive_hls(id, rec, settings, cancel).await;
        }

        // 2. Decide whether stored progress is still valid.
        let stale = !rec.segments.is_empty()
            && (rec.info.size != probe.size
                || !probe.resumable
                || (rec.etag.is_some() && rec.etag != probe.etag));
        let mut segments = if stale { vec![] } else { rec.segments.clone() };
        let fresh = segments.is_empty();

        let (dir, filename) = if fresh && rec.segments.is_empty() && rec.info.downloaded == 0 {
            let name = rec
                .opts
                .filename
                .as_deref()
                .map(sanitize_filename)
                .unwrap_or(probe.filename.clone());
            let dir = self.target_dir(&rec, &name, &settings);
            let taken = |p: &Path| {
                let st = self.inner.state.lock().unwrap();
                st.items.iter().any(|i| {
                    i.rec.info.id != id
                        && Path::new(&i.rec.info.dir).join(&i.rec.info.filename) == p
                })
            };
            if let Err(e) = std::fs::create_dir_all(&dir) {
                self.fail(id, format!("Cannot create folder: {e}"));
                return;
            }
            let unique = if rec.opts.overwrite {
                let _ = std::fs::remove_file(dir.join(&name));
                name.clone()
            } else {
                unique_path(&dir, &name, taken).1
            };
            (dir, unique)
        } else {
            (PathBuf::from(&rec.info.dir), rec.info.filename.clone())
        };
        let final_path = dir.join(&filename);
        let part = part_path(&final_path);

        if fresh {
            segments = match probe.size {
                Some(0) => vec![],
                Some(n) => vec![Segment {
                    start: 0,
                    pos: 0,
                    end: n,
                    active: false,
                }],
                None => vec![Segment {
                    start: 0,
                    pos: 0,
                    end: u64::MAX,
                    active: false,
                }],
            };
        }

        // 3. Open + preallocate. Preallocating avoids fragmentation and fails
        // early when the disk is too small.
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(fresh)
            .open(&part)
        {
            Ok(f) => f,
            Err(e) => {
                self.fail(id, format!("Cannot open {}: {e}", part.display()));
                return;
            }
        };
        if let Some(n) = probe.size
            && let Err(e) = file.set_len(n)
        {
            self.fail(id, format!("Not enough disk space or cannot allocate: {e}"));
            return;
        }
        let shared = Arc::new(Shared::new(file, segments));
        let connections = rec
            .info
            .connections
            .min(settings.connections.max(rec.info.connections));
        self.mutate(id, |i| {
            i.rec.info.filename = filename.clone();
            i.rec.info.dir = dir.to_string_lossy().into_owned();
            i.rec.info.category = categorize(&filename);
            i.rec.info.size = probe.size;
            i.rec.info.resumable = probe.resumable;
            i.rec.info.downloaded = shared.downloaded.load(Ordering::Relaxed);
            i.rec.etag = probe.etag.clone();
            if let Some(r) = i.run.as_mut() {
                r.last_bytes = i.rec.info.downloaded;
                r.last_t = Instant::now();
                r.shared = Some(shared.clone());
            }
        });
        self.save();

        // 4. Go.
        let job = Arc::new(Job {
            client: self.client(),
            limiter: self.limiter_for(&rec.info.queue),
            url: probe.url.clone(),
            opts: rec.opts.clone(),
            resumable: probe.resumable,
            connections,
            retries: self
                .queues()
                .iter()
                .find(|q| q.id == rec.info.queue)
                .map_or(0, |q| q.max_retries),
            cancel: cancel.clone(),
            shared: shared.clone(),
        });
        let result = engine::run(job).await;
        let final_segs = shared.snapshot();
        let total = shared.downloaded.load(Ordering::Relaxed);
        // Close the file handle before renaming (required on Windows).
        let shared_for_drop = shared;
        let file_arc = shared_for_drop.file.clone();
        self.mutate(id, |i| {
            if let Some(r) = i.run.as_mut() {
                r.shared = None;
            }
        });
        drop(shared_for_drop);

        match result {
            Ok(()) => {
                if probe.size.is_none() {
                    // Length was unknown: truncate to what we actually received.
                    let _ = file_arc.set_len(total);
                }
                drop(file_arc);
                let renamed = {
                    let final_path = final_path.clone();
                    tokio::task::spawn_blocking(move || std::fs::rename(&part, &final_path)).await
                };
                match renamed {
                    Ok(Ok(())) => {
                        self.mutate(id, |i| {
                            i.rec.info.status = Status::Complete;
                            i.rec.info.downloaded = total;
                            i.rec.info.size = Some(total);
                            i.rec.info.speed = 0;
                            i.rec.info.eta_secs = None;
                            i.rec.info.active_connections = 0;
                            i.rec.info.completed_at = Some(now_secs());
                            i.rec.segments.clear();
                        });
                        self.on_complete(id);
                    }
                    Ok(Err(e)) => self.fail(id, format!("Could not finalize file: {e}")),
                    Err(e) => self.fail(id, e.to_string()),
                }
            }
            Err(Error::Cancelled) => {
                self.mutate(id, |i| {
                    i.rec.segments = final_segs;
                    i.rec.info.downloaded = total;
                    i.rec.info.speed = 0;
                    i.rec.info.eta_secs = None;
                    i.rec.info.active_connections = 0;
                });
            }
            Err(e) => {
                self.mutate(id, |i| {
                    i.rec.segments = final_segs;
                    i.rec.info.downloaded = total;
                });
                self.fail_or_retry(id, e);
            }
        }
    }

    /// HLS (.m3u8) streams: resolve the playlist, then fetch segments in parallel.
    pub(super) async fn drive_hls(
        &self,
        id: &str,
        rec: Record,
        settings: Settings,
        cancel: CancellationToken,
    ) {
        let loaded = if crate::dash::is_dash_url(&rec.info.url) {
            crate::dash::load(&self.client(), &rec.info.url, &rec.opts).await
        } else {
            hls::load(&self.client(), &rec.info.url, &rec.opts).await
        };
        let (media, playlist_url) = match loaded {
            Ok(x) => x,
            Err(Error::Cancelled) => return,
            Err(e) => return self.fail(id, e.to_string()),
        };
        let live = media.live;
        let total = media.segments.len();
        let resume = rec
            .hls
            .clone()
            .filter(|h| !live && h.total == total && h.next <= total && h.bytes > 0);
        let ext = if media.fmp4 { "mp4" } else { "ts" };

        let (dir, filename) = if resume.is_none() {
            let name = match rec.opts.filename.as_deref().map(sanitize_filename) {
                // A caller-supplied name (e.g. the page title) gets the right container extension.
                Some(n) if has_video_ext(&n) => n,
                Some(n) => format!("{n}.{ext}"),
                None => format!("{}.{ext}", hls_stem(&rec.info.url)),
            };
            let dir = self.target_dir(&rec, &name, &settings);
            if let Err(e) = std::fs::create_dir_all(&dir) {
                return self.fail(id, format!("Cannot create folder: {e}"));
            }
            let taken = |p: &Path| {
                let st = self.inner.state.lock().unwrap();
                st.items.iter().any(|i| {
                    i.rec.info.id != id
                        && Path::new(&i.rec.info.dir).join(&i.rec.info.filename) == p
                })
            };
            (dir.clone(), unique_path(&dir, &name, taken).1)
        } else {
            (PathBuf::from(&rec.info.dir), rec.info.filename.clone())
        };
        let final_path = dir.join(&filename);
        let part = part_path(&final_path);
        let (next, bytes) = resume.as_ref().map(|h| (h.next, h.bytes)).unwrap_or((0, 0));

        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&part)
        {
            Ok(f) => f,
            Err(e) => return self.fail(id, format!("Cannot open {}: {e}", part.display())),
        };
        // Drop anything written after the last fully committed segment.
        if let Err(e) = file.set_len(bytes) {
            return self.fail(id, format!("Cannot prepare file: {e}"));
        }
        let shared = Arc::new(HlsShared::default());
        shared.next.store(next, Ordering::Relaxed);
        shared.bytes.store(bytes, Ordering::Relaxed);
        shared.total.store(total, Ordering::Relaxed);
        self.mutate(id, |i| {
            i.rec.info.filename = filename.clone();
            i.rec.info.dir = dir.to_string_lossy().into_owned();
            i.rec.info.category = categorize(&filename);
            i.rec.info.resumable = true;
            i.rec.info.downloaded = bytes;
            i.rec.info.size = None;
            if let Some(r) = i.run.as_mut() {
                r.last_bytes = bytes;
                r.last_t = Instant::now();
                r.hls = Some(shared.clone());
            }
        });
        self.save();

        let job = Arc::new(HlsJob {
            client: self.client(),
            limiter: self.limiter_for(&rec.info.queue),
            opts: rec.opts.clone(),
            media,
            file: Arc::new(file),
            connections: rec
                .info
                .connections
                .min(settings.connections.max(rec.info.connections)),
            cancel: cancel.clone(),
            shared: shared.clone(),
        });
        let result = if live {
            // Stop (or pause) ends a recording; the file recorded so far is the result.
            match hls::run_live(job.clone(), playlist_url).await {
                Err(Error::Cancelled) => Ok(()),
                r => r,
            }
        } else {
            hls::run(job.clone()).await
        };
        let state = hls_state(&shared);
        drop(job); // closes the file before the rename below (needed on Windows)

        match result {
            Ok(()) => {
                let renamed = {
                    let final_path = final_path.clone();
                    tokio::task::spawn_blocking(move || std::fs::rename(&part, &final_path)).await
                };
                match renamed {
                    Ok(Ok(())) => {
                        self.mutate(id, |i| {
                            i.rec.info.status = Status::Complete;
                            i.rec.info.downloaded = state.bytes;
                            i.rec.info.size = Some(state.bytes);
                            i.rec.info.speed = 0;
                            i.rec.info.eta_secs = None;
                            i.rec.info.active_connections = 0;
                            i.rec.info.completed_at = Some(now_secs());
                            i.rec.hls = None;
                        });
                        self.on_complete(id);
                    }
                    Ok(Err(e)) => self.fail(id, format!("Could not finalize file: {e}")),
                    Err(e) => self.fail(id, e.to_string()),
                }
            }
            Err(Error::Cancelled) => {
                self.mutate(id, |i| {
                    i.rec.hls = Some(state.clone());
                    i.rec.info.downloaded = state.bytes;
                    i.rec.info.speed = 0;
                    i.rec.info.eta_secs = None;
                    i.rec.info.active_connections = 0;
                });
            }
            Err(e) => {
                self.mutate(id, |i| {
                    i.rec.hls = Some(state.clone());
                    i.rec.info.downloaded = state.bytes;
                });
                self.fail_or_retry(id, e);
            }
        }
    }

    /// Every 250ms: refresh speed/ETA of running downloads and push to the UI.
    pub(super) async fn ticker(&self) {
        let mut n = 0u32;
        let mut iv = tokio::time::interval(Duration::from_millis(250));
        loop {
            iv.tick().await;
            n += 1;
            if n.is_multiple_of(4) {
                self.enforce_schedules();
                self.pump();
            }
            if n.is_multiple_of(40) {
                self.queue_housekeeping();
            }
            if n.is_multiple_of(240) {
                self.run_due_projects();
                self.sync_check(false);
            }
            let mut updates = vec![];
            let mut moved = 0u64;
            {
                let mut st = self.inner.state.lock().unwrap();
                for item in st.items.iter_mut() {
                    let Some(run) = item.run.as_mut() else {
                        continue;
                    };
                    let (bytes, active, est_size) = if let Some(sh) = run.shared.as_ref() {
                        (
                            sh.downloaded.load(Ordering::Relaxed),
                            sh.active.load(Ordering::Relaxed),
                            None,
                        )
                    } else if let Some(h) = run.hls.as_ref() {
                        let done = h.next.load(Ordering::Relaxed) as u64;
                        let total = h.total.load(Ordering::Relaxed) as u64;
                        let b = h.bytes.load(Ordering::Relaxed);
                        // Total size is unknown for HLS; extrapolate from segments done.
                        (
                            b,
                            h.active.load(Ordering::Relaxed),
                            (done > 0 && !h.live.load(Ordering::Relaxed)).then(|| b * total / done),
                        )
                    } else {
                        continue;
                    };
                    let now = Instant::now();
                    let dt = now.duration_since(run.last_t).as_secs_f64().max(0.001);
                    let delta = bytes.saturating_sub(run.last_bytes);
                    moved += delta;
                    let inst = delta as f64 / dt;
                    run.ema = if run.ema == 0.0 {
                        inst
                    } else {
                        run.ema * 0.7 + inst * 0.3
                    };
                    run.last_bytes = bytes;
                    run.last_t = now;
                    let info = &mut item.rec.info;
                    info.downloaded = bytes;
                    info.speed = run.ema as u64;
                    info.active_connections = active;
                    if est_size.is_some() {
                        info.size = est_size;
                    }
                    info.eta_secs = match info.size {
                        Some(s) if run.ema > 1.0 => {
                            Some((s.saturating_sub(bytes) as f64 / run.ema) as u64)
                        }
                        _ => None,
                    };
                    updates.push(info.clone());
                }
            }
            if moved > 0 {
                self.add_traffic(moved);
            }
            if n.is_multiple_of(40) {
                crate::extras::save_stats(&self.inner.dir, &self.inner.stats.lock().unwrap());
            }
            let any = !updates.is_empty();
            for u in updates {
                self.emit(&u);
            }
            if any && n.is_multiple_of(8) {
                self.save();
            }
        }
    }
}
