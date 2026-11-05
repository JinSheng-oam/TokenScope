//! RPC workers fetch observations only. SQLite writes stay on the daemon thread.
use crate::{antigravity_rpc, codex_rpc};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tokenscope_local_store::{AntigravityQuota, CodexQuota, LocalStore};

const REFRESH_INTERVAL: Duration = Duration::from_secs(60);
type Pending<T> = Arc<Mutex<Option<Result<Vec<T>, String>>>>;

pub(super) struct QuotaWorkers {
    codex: Pending<CodexQuota>,
    antigravity: Pending<AntigravityQuota>,
    stop: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
}

fn spawn<T: Send + 'static>(
    stop: Arc<AtomicBool>,
    pending: Pending<T>,
    delay: Duration,
    fetch: impl Fn() -> Result<Vec<T>, String> + Send + 'static,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut next = Instant::now() + delay;
        while !stop.load(Ordering::Acquire) {
            let wait = next.saturating_duration_since(Instant::now());
            if !wait.is_zero() {
                thread::park_timeout(wait);
                continue;
            }
            let result = fetch();
            // Keep at most one latest response per tool, even during a long scan.
            if let Ok(mut slot) = pending.lock() {
                *slot = Some(result);
            }
            next = Instant::now() + REFRESH_INTERVAL;
        }
    })
}

fn initial_delay(newest: Option<i64>) -> Duration {
    let age = newest.map(|at| {
        chrono::Utc::now()
            .timestamp_millis()
            .saturating_sub(at)
            .max(0) as u64
    });
    age.map(|age| REFRESH_INTERVAL.saturating_sub(Duration::from_millis(age)))
        .unwrap_or_default()
}

impl QuotaWorkers {
    pub(super) fn start(
        store: &LocalStore,
        codex_enabled: bool,
        antigravity_enabled: bool,
    ) -> Self {
        let mut workers = Self {
            codex: Arc::new(Mutex::new(None)),
            antigravity: Arc::new(Mutex::new(None)),
            stop: Arc::new(AtomicBool::new(false)),
            handles: Vec::new(),
        };
        if codex_enabled {
            let newest = store
                .codex_quota_snapshots(32)
                .ok()
                .and_then(|rows| rows.iter().map(|r| r.observed_at_ms).max());
            workers.handles.push(spawn(
                workers.stop.clone(),
                workers.codex.clone(),
                initial_delay(newest),
                || codex_rpc::fetch_rate_limits().map_err(str::to_owned),
            ));
        }
        if antigravity_enabled {
            let newest = store
                .antigravity_quota_snapshots(200)
                .ok()
                .and_then(|rows| rows.iter().map(|r| r.observed_at_ms).max());
            workers.handles.push(spawn(
                workers.stop.clone(),
                workers.antigravity.clone(),
                initial_delay(newest),
                || antigravity_rpc::fetch_quota().map_err(|e| e.to_string()),
            ));
        }
        workers
    }

    pub(super) fn persist_pending(&self, store: &mut LocalStore) {
        let codex = self.codex.lock().ok().and_then(|mut slot| slot.take());
        let antigravity = self
            .antigravity
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        let now = chrono::Utc::now().timestamp_millis();
        if let Some(result) = codex {
            match result.and_then(|rows| {
                store
                    .upsert_codex_quotas(&rows, now)
                    .map_err(|e| e.to_string())
            }) {
                Ok(updated) => println!("codex quota snapshots updated={updated}"),
                Err(error) => eprintln!("Codex quota refresh failed: {error}"),
            }
        }
        if let Some(result) = antigravity {
            match result.and_then(|rows| {
                store
                    .upsert_antigravity_quotas(&rows, now)
                    .map_err(|e| e.to_string())
            }) {
                Ok(updated) => println!("antigravity quota snapshots updated={updated}"),
                Err(error) => eprintln!("Antigravity quota refresh failed: {error}"),
            }
        }
    }
}

impl Drop for QuotaWorkers {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        for handle in &self.handles {
            handle.thread().unpark();
        }
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
    }
}
