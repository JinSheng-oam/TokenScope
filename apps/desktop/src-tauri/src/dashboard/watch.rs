use super::query::{dashboard_key, query_agent_fresh};
use crate::{paths::database_path, state::DashboardWatch};
use std::{
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};
use tokenscope_local_store::LocalStore;

/// Poll only two revision counters. Checkpoint/quota writes push observations;
/// only usage, names or activity changes rebuild the full projection.
pub(crate) fn spawn_dashboard_watch(app: AppHandle) {
    thread::spawn(move || {
        let mut reader: Option<LocalStore> = None;
        let mut last_full = None;
        let mut last_observations = None;
        loop {
            thread::sleep(Duration::from_millis(1_200));
            if reader.is_none() {
                reader = database_path(&app)
                    .ok()
                    .and_then(|db| LocalStore::open_readonly(&db).ok());
            }
            let Some(store) = reader.as_ref() else {
                continue;
            };
            let (full, observations) = match store.dashboard_revisions() {
                Ok(revisions) => revisions,
                Err(error) => {
                    eprintln!("TokenScope revision watch warning: {error}");
                    reader = None;
                    continue;
                }
            };
            let watch = app.state::<DashboardWatch>();
            watch.revision.store(full, Ordering::Release);
            if last_full != Some(full) {
                let request = watch.request.lock().map(|r| r.clone()).unwrap_or_default();
                if request.range.is_empty() {
                    continue;
                }
                let key = dashboard_key(
                    &request.range,
                    request.start_date.as_deref(),
                    request.end_date.as_deref(),
                    0,
                );
                let claimed = watch
                    .refreshing
                    .lock()
                    .map(|mut r| r.insert(key.clone()))
                    .unwrap_or(false);
                if !claimed {
                    continue;
                }
                // Let a burst of source transactions settle before aggregating.
                thread::sleep(Duration::from_millis(800));
                let response = query_agent_fresh(
                    &app,
                    &request.range,
                    0,
                    request.start_date.as_deref(),
                    request.end_date.as_deref(),
                );
                if let Ok(mut r) = watch.refreshing.lock() {
                    r.remove(&key);
                }
                match response {
                    Ok(response) => {
                        last_full = Some(response.snapshot.data_revision);
                        if let Ok(mut cache) = watch.cache.lock() {
                            cache.insert(key, (response.clone(), Instant::now()));
                        }
                        if let Err(error) = app.emit("dashboard-updated", response) {
                            eprintln!("TokenScope dashboard push warning: {error}");
                        }
                    }
                    Err(error) => eprintln!("TokenScope dashboard push warning: {error}"),
                }
                // Failed queries keep their old revision and retry on the next tick.
            }
            if last_observations != Some(observations) {
                match store.dashboard_observations(chrono::Utc::now().timestamp_millis()) {
                    Ok(update) => {
                        if app.emit("dashboard-observations-updated", update).is_ok() {
                            last_observations = Some(observations);
                        }
                    }
                    Err(error) => eprintln!("TokenScope observation push warning: {error}"),
                }
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
