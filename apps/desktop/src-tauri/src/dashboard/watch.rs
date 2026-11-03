use super::query::{dashboard_key, query_agent_fresh};
use crate::{paths::database_path, state::DashboardWatch};
use std::{
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager};
fn dashboard_signature(db: &Path) -> Option<(u64, u64)> {
    let wal = PathBuf::from(format!("{}-wal", db.display()));
    let meta = fs::metadata(&wal).or_else(|_| fs::metadata(db)).ok()?;
    let modified = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    Some((meta.len(), modified))
}

/// One long-lived reader: after the first snapshot, watch the Agent's WAL and
/// push a fresh projection only when new usage actually landed. This replaces
/// the previous timer that spawned a full query every 30 seconds.
pub(crate) fn spawn_dashboard_watch(app: AppHandle) {
    thread::spawn(move || {
        let mut last = None;
        loop {
            thread::sleep(Duration::from_millis(1_200));
            let Ok(db) = database_path(&app) else {
                continue;
            };
            if !db.is_file() {
                continue;
            }
            let signature = dashboard_signature(&db);
            if signature.is_none() || signature == last {
                continue;
            }
            last = signature;
            // Scans commit in bursts; let the burst settle before reading.
            thread::sleep(Duration::from_millis(800));
            let request = app
                .state::<DashboardWatch>()
                .request
                .lock()
                .map(|value| value.clone())
                .unwrap_or_default();
            if request.range.is_empty() {
                continue;
            }
            match query_agent_fresh(
                &app,
                &request.range,
                0,
                request.start_date.as_deref(),
                request.end_date.as_deref(),
            ) {
                Ok(response) => {
                    let key = dashboard_key(
                        &request.range,
                        request.start_date.as_deref(),
                        request.end_date.as_deref(),
                        0,
                    );
                    if let Ok(mut cache) = app.state::<DashboardWatch>().cache.lock() {
                        cache.insert(key, (response.clone(), Instant::now()));
                    }
                    if let Err(error) = app.emit("dashboard-updated", response) {
                        eprintln!("TokenScope dashboard push warning: {error}");
                    }
                }
                Err(error) => eprintln!("TokenScope dashboard push warning: {error}"),
            }
        }
    });
}

/// Warms the standard ranges once in the background so the first manual switch
/// hits the cache instead of paying for another cold aggregate.
pub(crate) fn spawn_dashboard_warmup(app: AppHandle) {
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(6));
        for range in ["today", "7d", "30d", "all"] {
            let key = dashboard_key(range, None, None, 0);
            let cached = app
                .state::<DashboardWatch>()
                .cache
                .lock()
                .map(|cache| cache.contains_key(&key))
                .unwrap_or(true);
            if cached {
                continue;
            }
            if let Ok(response) = query_agent_fresh(&app, range, 0, None, None) {
                if let Ok(mut cache) = app.state::<DashboardWatch>().cache.lock() {
                    cache.insert(key, (response, Instant::now()));
                }
            }
        }
    });
}
