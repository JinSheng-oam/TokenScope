//! Explicit local quota snapshot import (not OAuth, not RPC).
//! The agent never reads browser credentials or Google tokens.
use serde::Deserialize;
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use tokenscope_local_store::{AntigravityQuota, LocalStore};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    source_id: String,
    pool_id: String,
    window_id: String,
    remaining_basis_points: Option<u16>,
    reset_at_ms: Option<i64>,
    observed_at_ms: i64,
}

pub fn import_file(
    store: &mut LocalStore,
    path: &Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    let len = fs::metadata(path)?.len();
    if len == 0 || len > 65_536 {
        return Err("quota JSON must be 1..65536 bytes".into());
    }
    let data = fs::read(path)?;
    let items: Vec<Snapshot> = serde_json::from_slice(&data)?;
    if items.is_empty() || items.len() > 200 {
        return Err("quota batch must contain 1..200 rows".into());
    }
    let received_at_ms = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
    let mut batch = Vec::with_capacity(items.len());
    for item in items {
        let quota = AntigravityQuota {
            source_id: item.source_id,
            pool_id: item.pool_id,
            window_id: item.window_id,
            remaining_basis_points: item.remaining_basis_points,
            reset_at_ms: item.reset_at_ms,
            observed_at_ms: item.observed_at_ms,
        };
        // Quota must represent an actual, externally observed snapshot.
        // Do not accept timestamps far in the future.
        if quota.observed_at_ms > received_at_ms.saturating_add(60_000) {
            return Err("quota snapshot timestamp is in the future".into());
        }
        batch.push(quota);
    }
    Ok(store.upsert_antigravity_quotas(&batch, received_at_ms)?)
}

pub fn print_status(store: &LocalStore) -> Result<(), Box<dyn std::error::Error>> {
    let activity = store.antigravity_activity_summary()?;
    println!(
        "antigravity activity conversations={} known_turns={} unknown_turns={} (NOT TOKEN COUNTS)",
        activity.conversations, activity.known_turns, activity.unknown_turns
    );
    for (model, count) in activity.by_model {
        println!("  model={model} responses={count}");
    }
    let now = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
    for quota in store.antigravity_quota_snapshots(200)? {
        let state = if quota.observed_at_ms > now + 60_000 {
            "INVALID_TIME"
        } else if now.saturating_sub(quota.observed_at_ms) > 10 * 60_000 {
            "STALE"
        } else {
            "RECENT"
        };
        let remaining = quota
            .remaining_basis_points
            .map(|n| format!("{:.2}%", f64::from(n) / 100.0))
            .unwrap_or_else(|| "unknown".into());
        println!("  quota source={} pool={} window={} remaining={} reset_at_ms={:?} observed_at_ms={} {}",
            quota.source_id, quota.pool_id, quota.window_id, remaining, quota.reset_at_ms,
            quota.observed_at_ms, state);
    }
    Ok(())
}
