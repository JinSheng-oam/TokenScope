use super::endpoint::discover_endpoints;
use super::error::RpcError;
use super::quota::{local_source_id, parse_grouped_quota, parse_legacy_quota};
use super::transport::{build_client, call_rpc};
use serde_json::json;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokenscope_local_store::{AntigravityQuota, LocalStore};

fn fetch_quota() -> Result<Vec<AntigravityQuota>, RpcError> {
    let endpoints = discover_endpoints()?;
    let client = build_client()?;
    let observed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| RpcError::InvalidResponse)?
        .as_millis();
    let observed_at_ms = i64::try_from(observed).map_err(|_| RpcError::InvalidResponse)?;
    let started = std::time::Instant::now();
    let mut last = RpcError::NoQuota;
    for endpoint in endpoints.into_iter().take(6) {
        if started.elapsed() > Duration::from_secs(12) {
            break;
        }
        for scheme in ["https", "http"] {
            if started.elapsed() > Duration::from_secs(12) {
                break;
            }
            match call_rpc(
                &client,
                &endpoint,
                scheme,
                "RetrieveUserQuotaSummary",
                &json!({"forceRefresh": true}),
            ) {
                Ok(payload) => {
                    let rows = parse_grouped_quota(&payload, local_source_id(), observed_at_ms);
                    if !rows.is_empty() {
                        return Ok(rows);
                    }
                    last = RpcError::NoQuota;
                }
                Err(e @ RpcError::Unauthorized) => last = e,
                Err(e) => last = e,
            }
            // Older language-server builds may expose only user-status model
            // configs. Keep the fallback on the same local endpoint and never
            // infer a five-hour/weekly window when upstream does not say so.
            if started.elapsed() <= Duration::from_secs(12) {
                if let Ok(status) = call_rpc(
                    &client,
                    &endpoint,
                    scheme,
                    "GetUserStatus",
                    &json!({"metadata": {
                        "ideName": "antigravity", "extensionName": "antigravity", "ideVersion": "unknown", "locale": "en"
                    }}),
                ) {
                    let rows = parse_legacy_quota(&status, local_source_id(), observed_at_ms);
                    if !rows.is_empty() {
                        return Ok(rows);
                    }
                }
                if let Ok(configs) = call_rpc(
                    &client,
                    &endpoint,
                    scheme,
                    "GetCommandModelConfigs",
                    &json!({"metadata": {
                        "ideName": "antigravity", "extensionName": "antigravity", "ideVersion": "unknown", "locale": "en"
                    }}),
                ) {
                    let rows = parse_legacy_quota(&configs, local_source_id(), observed_at_ms);
                    if !rows.is_empty() {
                        return Ok(rows);
                    }
                }
            }
        }
    }
    Err(last)
}

pub(super) fn refresh_and_store(store: &mut LocalStore) -> Result<usize, RpcError> {
    let rows = fetch_quota()?;
    let received = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| RpcError::InvalidResponse)?
        .as_millis();
    let received = i64::try_from(received).map_err(|_| RpcError::InvalidResponse)?;
    store
        .upsert_antigravity_quotas(&rows, received)
        .map_err(RpcError::Store)
}
