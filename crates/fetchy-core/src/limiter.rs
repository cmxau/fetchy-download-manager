use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Shared pacing limiter across every connection. `0` means unlimited.
pub struct Limiter {
    bps: AtomicU64,
    next_free: Mutex<Instant>,
    /// Bytes allowed per rolling hour window (0 = no quota).
    quota: AtomicU64,
    window: Mutex<(Instant, u64)>,
    /// Every byte also passes through this (the global limiter).
    parent: Option<std::sync::Arc<Limiter>>,
}

impl Limiter {
    pub fn new(bps: u64) -> Self {
        Limiter {
            bps: AtomicU64::new(bps),
            next_free: Mutex::new(Instant::now()),
            quota: AtomicU64::new(0),
            window: Mutex::new((Instant::now(), 0)),
            parent: None,
        }
    }

    pub fn with_parent(bps: u64, parent: std::sync::Arc<Limiter>) -> Self {
        Limiter {
            parent: Some(parent),
            ..Limiter::new(bps)
        }
    }

    pub fn set(&self, bps: u64) {
        self.bps.store(bps, Ordering::Relaxed);
    }

    pub fn set_quota(&self, bytes_per_hour: u64) {
        self.quota.store(bytes_per_hour, Ordering::Relaxed);
    }

    /// Blocks while the hourly quota is used up; the window resets after an hour.
    async fn wait_quota(&self, n: usize) {
        loop {
            let q = self.quota.load(Ordering::Relaxed);
            if q == 0 {
                return;
            }
            let wait = {
                let mut w = self.window.lock().unwrap();
                if w.0.elapsed() >= Duration::from_secs(3600) {
                    *w = (Instant::now(), 0);
                }
                if w.1 < q {
                    w.1 += n as u64;
                    return;
                }
                Duration::from_secs(3600).saturating_sub(w.0.elapsed())
            };
            tokio::time::sleep(wait.clamp(Duration::from_millis(200), Duration::from_secs(5)))
                .await;
        }
    }

    pub async fn acquire(&self, n: usize) {
        self.own(n).await;
        if let Some(p) = &self.parent {
            p.own(n).await;
        }
    }

    async fn own(&self, n: usize) {
        self.wait_quota(n).await;
        let bps = self.bps.load(Ordering::Relaxed);
        if bps == 0 {
            return;
        }
        let wait = {
            let mut nf = self.next_free.lock().unwrap();
            let now = Instant::now();
            let start = (*nf).max(now);
            *nf = start + Duration::from_secs_f64(n as f64 / bps as f64);
            // ~100ms of burst so small chunks don't each pay a timer.
            start
                .saturating_duration_since(now)
                .saturating_sub(Duration::from_millis(100))
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }
}
