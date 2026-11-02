use std::time::{SystemTime, UNIX_EPOCH};

use tokenscope_local_store::LocalStore;
use crate::{
    client::{HubClient, HubClientError},
    pairing::DeviceToken,
    sync::{UsageAckStatus, MAX_SYNC_BATCH},
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SyncReport {
    pub uploaded: usize,
    pub unchanged: usize,
    pub superseded: usize,
    pub acknowledged: usize,
    pub blocked: usize,
    pub remaining_due: usize,
}

#[derive(Debug)]
pub enum SyncWorkerError {
    Hub(HubClientError),
    Store(String),
}
impl From<HubClientError> for SyncWorkerError {
    fn from(v: HubClientError) -> Self { Self::Hub(v) }
}

pub fn now_ms() -> Result<i64, SyncWorkerError> {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH)
        .map_err(|_| SyncWorkerError::Store("system_clock_before_epoch".into()))?;
    i64::try_from(duration.as_millis()).map_err(|_| SyncWorkerError::Store("system_clock_out_of_range".into()))
}

fn retryable(error: &HubClientError) -> bool {
    matches!(error,
        HubClientError::Transport |
        HubClientError::RateLimited |
        HubClientError::ServerUnavailable |
        HubClientError::RequestTooLarge |
        HubClientError::ServerRejected(500..=599)
    )
}

fn safe_error_code(error: &HubClientError) -> &'static str {
    match error {
        HubClientError::Transport => "transport",
        HubClientError::RateLimited => "rate_limited",
        HubClientError::ServerUnavailable => "server_unavailable",
        HubClientError::RequestTooLarge => "request_too_large",
        HubClientError::ServerRejected(_) => "server_error",
        _ => "non_retryable",
    }
}

/// Upload one due Outbox page. Retry times are persisted in SQLite and survive
/// process restarts. Superseded rows are quarantined instead of hot-looping.
pub fn sync_outbox_once(client: &HubClient, store: &mut LocalStore, token: &DeviceToken) -> Result<SyncReport, SyncWorkerError> {
    let now = now_ms()?;
    let pending = store.pending_outbox_due(MAX_SYNC_BATCH, now).map_err(|e| SyncWorkerError::Store(e.to_string()))?;
    if pending.is_empty() {
        // Do not rewrite last_attempt_ms when no network request was made.
        return Ok(SyncReport::default());
    }
    let mut records: Vec<_> = pending.iter().map(|entry| entry.envelope.clone()).collect();
    let response = loop {
        // A reduced body is a distinct logical batch and receives a fresh ID.
        let batch_id = uuid::Uuid::new_v4().simple().to_string();
        match client.push_usage_batch(token, &batch_id, &records) {
            Ok(response) => break response,
            Err(HubClientError::RequestTooLarge) if records.len() > 1 => { records.pop(); },
            Err(error) => {
                if retryable(&error) {
                    let retry_at = now_ms()?;
                    let code = safe_error_code(&error);
                    for entry in pending.iter().take(records.len()) {
                        let _ = store.schedule_outbox_retry(&entry.envelope.record.record_id, entry.envelope.record_revision, retry_at, code)
                            .map_err(|e| SyncWorkerError::Store(e.to_string()))?;
                    }
                    store.update_sync_state(retry_at, "deferred", Some(code)).map_err(|e| SyncWorkerError::Store(e.to_string()))?;
                    return Err(SyncWorkerError::Hub(error));
                }
                let status = match error { HubClientError::Unauthorized | HubClientError::DeviceRevoked => "auth_required", _ => "error" };
                let at = now_ms()?;
                store.update_sync_state(at, status, Some(safe_error_code(&error))).map_err(|e| SyncWorkerError::Store(e.to_string()))?;
                return Err(SyncWorkerError::Hub(error));
            }
        }
    };
    let mut report = SyncReport::default();
    for ack in response.results {
        match ack.status {
            UsageAckStatus::Accepted => {
                report.uploaded += 1;
                if store.ack_outbox_if_revision(&ack.record_id, ack.record_revision).map_err(|e| SyncWorkerError::Store(e.to_string()))? { report.acknowledged += 1; }
            }
            UsageAckStatus::Unchanged => {
                report.unchanged += 1;
                if store.ack_outbox_if_revision(&ack.record_id, ack.record_revision).map_err(|e| SyncWorkerError::Store(e.to_string()))? { report.acknowledged += 1; }
            }
            UsageAckStatus::Superseded => {
                report.superseded += 1;
                if store.block_outbox_revision(&ack.record_id, ack.record_revision, now, "hub_superseded")
                    .map_err(|e| SyncWorkerError::Store(e.to_string()))? { report.blocked += 1; }
            }
        }
    }
    let finished = now_ms()?;
    let status = if report.blocked > 0 { "blocked" } else { "success" };
    store.update_sync_state(finished, status, if report.blocked > 0 { Some("hub_superseded") } else { None })
        .map_err(|e| SyncWorkerError::Store(e.to_string()))?;
    let status_snapshot = store.sync_outbox_status(finished).map_err(|e| SyncWorkerError::Store(e.to_string()))?;
    report.remaining_due = usize::try_from(status_snapshot.pending).unwrap_or(usize::MAX);
    Ok(report)
}

/// Pull one bounded remote change-feed page. Cache rows and cursor are committed
/// in one SQLite transaction so a crash cannot advance the cursor past unapplied data.
pub fn pull_usage_changes_once(client:&HubClient,store:&mut LocalStore,token:&DeviceToken,limit:usize)->Result<usize,SyncWorkerError>{
    let cursor=store.pull_cursor().map_err(|e|SyncWorkerError::Store(e.to_string()))?;
    let page=client.pull_usage_changes(token,cursor,limit)?;
    let changes=page.changes.into_iter().map(|c|tokenscope_local_store::RemoteUsageChange{change_seq:c.change_seq,owner_device_id:c.owner_device_id,envelope:c.envelope}).collect::<Vec<_>>();
    store.apply_remote_changes(&changes,page.next_cursor).map_err(|e|SyncWorkerError::Store(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transient_errors_are_retryable_but_auth_errors_are_not() {
        assert!(retryable(&HubClientError::Transport));
        assert!(retryable(&HubClientError::RateLimited));
        assert!(retryable(&HubClientError::ServerUnavailable));
        assert!(retryable(&HubClientError::ServerRejected(503)));
        assert!(!retryable(&HubClientError::Unauthorized));
        assert!(!retryable(&HubClientError::DeviceRevoked));
        assert!(!retryable(&HubClientError::ProtocolMismatch));
    }
}
