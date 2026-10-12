//! Queues: schedules, start/stop/reorder, synchronization checks and per-queue limiters.

use super::*;

impl Manager {
    pub fn move_to_queue(&self, id: &str, queue: &str) {
        if !self.queues().iter().any(|q| q.id == queue) {
            return;
        }
        self.mutate(id, |i| i.rec.info.queue = queue.to_string());
        self.save();
        self.pump();
    }

    /// Items whose queue is outside its schedule window go back to waiting.
    pub(super) fn enforce_schedules(&self) {
        let to_requeue: Vec<(String, CancellationToken)> = {
            let st = self.inner.state.lock().unwrap();
            let forced = self.inner.forced.lock().unwrap();
            let inactive: Vec<&str> = st
                .settings
                .queues
                .iter()
                .filter(|q| !q.is_active_now() && !forced.contains(&q.id))
                .map(|q| q.id.as_str())
                .collect();
            st.items
                .iter()
                .filter(|i| {
                    i.rec.info.status == Status::Downloading
                        && inactive.contains(&i.rec.info.queue.as_str())
                })
                .filter_map(|i| {
                    i.run
                        .as_ref()
                        .map(|r| (i.rec.info.id.clone(), r.cancel.clone()))
                })
                .collect()
        };
        for (id, cancel) in to_requeue {
            self.mutate(&id, |i| {
                i.rec.info.status = Status::Queued;
                i.rec.info.speed = 0;
                i.rec.info.eta_secs = None;
            });
            cancel.cancel();
        }
    }

    /// Start a queue now: it runs regardless of its schedule until it has nothing left to do.
    pub fn start_queue(&self, id: &str) {
        self.inner.forced.lock().unwrap().insert(id.to_string());
        self.inner
            .started
            .lock()
            .unwrap()
            .insert(id.to_string(), Instant::now());
        let ids: Vec<String> = {
            let st = self.inner.state.lock().unwrap();
            st.items
                .iter()
                .filter(|i| i.rec.info.queue == id)
                .map(|i| i.rec.info.id.clone())
                .collect()
        };
        for i in ids {
            self.resume(&i);
        }
        let is_sync = self.queues().iter().any(|q| q.id == id && q.sync);
        if is_sync {
            self.sync_check(true);
        }
        self.pump();
    }

    pub fn stop_queue(&self, id: &str) {
        self.inner.forced.lock().unwrap().remove(id);
        let ids: Vec<String> = {
            let st = self.inner.state.lock().unwrap();
            st.items
                .iter()
                .filter(|i| i.rec.info.queue == id)
                .map(|i| i.rec.info.id.clone())
                .collect()
        };
        for i in ids {
            self.pause(&i);
        }
    }

    /// Move a download one step earlier (`-1`) or later (`1`) among the downloads of its queue.
    pub fn move_in_queue(&self, id: &str, delta: i32) {
        let mut st = self.inner.state.lock().unwrap();
        let Some(pos) = st.items.iter().position(|i| i.rec.info.id == id) else {
            return;
        };
        let queue = st.items[pos].rec.info.queue.clone();
        // The engine starts from the back of the list, so "earlier in the queue" means a higher index.
        let step: isize = if delta < 0 { 1 } else { -1 };
        let mut j = pos as isize + step;
        while j >= 0 && (j as usize) < st.items.len() {
            if st.items[j as usize].rec.info.queue == queue {
                st.items.swap(pos, j as usize);
                break;
            }
            j += step;
        }
        drop(st);
        self.save();
    }

    /// Per-queue timers: "start again every" for plain queues, forced queues that ran dry.
    pub(super) fn queue_housekeeping(&self) {
        let queues = self.queues();
        {
            let st = self.inner.state.lock().unwrap();
            let mut forced = self.inner.forced.lock().unwrap();
            forced.retain(|id| {
                st.items.iter().any(|i| {
                    &i.rec.info.queue == id
                        && matches!(i.rec.info.status, Status::Queued | Status::Downloading)
                })
            });
        }
        let due: Vec<String> = {
            let mut started = self.inner.started.lock().unwrap();
            queues
                .iter()
                .filter(|q| q.repeat_minutes > 0 && !q.sync)
                .filter(|q| {
                    let t = started.entry(q.id.clone()).or_insert_with(Instant::now);
                    t.elapsed() >= Duration::from_secs(q.repeat_minutes as u64 * 60)
                })
                .map(|q| q.id.clone())
                .collect()
        };
        for id in due {
            self.start_queue(&id);
        }
    }

    /// Synchronization queues: re-download finished files whose server copy changed.
    /// `force` ignores the schedule window and the check interval.
    pub fn sync_check(&self, force: bool) {
        let due: Vec<SyncDue> =
            {
                let st = self.inner.state.lock().unwrap();
                let mut synced = self.inner.synced.lock().unwrap();
                let mut out = vec![];
                for q in st
                    .settings
                    .queues
                    .iter()
                    .filter(|q| q.sync && (force || q.is_active_now()))
                {
                    let every = Duration::from_secs(
                        60 * if q.repeat_minutes == 0 {
                            60
                        } else {
                            q.repeat_minutes as u64
                        },
                    );
                    for i in st.items.iter().filter(|i| {
                        i.rec.info.queue == q.id && i.rec.info.status == Status::Complete
                    }) {
                        let id = &i.rec.info.id;
                        if !force && synced.get(id).is_some_and(|t| t.elapsed() < every) {
                            continue;
                        }
                        if hls::is_hls_url(&i.rec.info.url)
                            || crate::dash::is_dash_url(&i.rec.info.url)
                            || i.rec.hls.is_some()
                        {
                            continue;
                        }
                        synced.insert(id.clone(), Instant::now());
                        out.push((
                            id.clone(),
                            i.rec.info.url.clone(),
                            i.rec.opts.clone(),
                            i.rec.info.size,
                            i.rec.etag.clone(),
                        ));
                    }
                }
                out
            };
        for (id, url, opts, size, etag) in due {
            let me = self.clone();
            self.inner.rt.spawn(async move {
                let Ok(p) = engine::probe(&me.client(), &url, &opts).await else {
                    return;
                };
                let changed = (size.is_some() && p.size.is_some() && p.size != size)
                    || (etag.is_some() && p.etag.is_some() && p.etag != etag);
                if changed {
                    me.redownload(&id).await;
                }
            });
        }
    }

    /// The limiter a download in `queue` pulls its bandwidth from.
    pub(super) fn limiter_for(&self, queue: &str) -> Arc<Limiter> {
        let mut ql = self.inner.qlimiters.lock().unwrap();
        ql.entry(queue.to_string())
            .or_insert_with(|| {
                let q = self
                    .inner
                    .state
                    .lock()
                    .unwrap()
                    .settings
                    .queues
                    .iter()
                    .find(|q| q.id == queue)
                    .cloned();
                let l = Limiter::with_parent(
                    q.as_ref().map_or(0, |q| q.speed_limit),
                    self.inner.limiter.clone(),
                );
                l.set_quota(q.map_or(0, |q| q.quota_per_hour));
                Arc::new(l)
            })
            .clone()
    }
}
