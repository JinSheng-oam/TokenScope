use crate::error::{db_int, Result, StoreError};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tokenscope_core_domain::RevisionedRecord;

pub(super) fn tool_display_name(tool_id: &str) -> String {
    match tool_id {
        "codex" => "Codex".into(),
        "claude" => "Claude".into(),
        "antigravity" => "Antigravity".into(),
        "pi" => "Pi Agent".into(),
        "deepseek-harness" => "DeepSeek Harness".into(),
        _ => tool_id.to_owned(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalIdentity {
    pub device_id: String,
    pub display_name: String,
    pub time_zone: String,
}

/// One exact provider/model price row from an immutable public catalog version.
/// Rates are USD per million tokens, scaled by 1e9 to avoid floating-point math.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelPriceEntry {
    pub provider_id: String,
    pub model_id: String,
    pub input_rate_nanos: Option<i64>,
    pub output_rate_nanos: Option<i64>,
    pub cache_read_rate_nanos: Option<i64>,
    pub cache_write_rate_nanos: Option<i64>,
    pub reasoning_rate_nanos: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelPriceCatalogState {
    pub active_version: Option<String>,
    pub etag: Option<String>,
    pub last_attempt_ms: Option<i64>,
    pub last_success_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelPriceInstallReport {
    pub price_version: String,
    pub model_count: usize,
    pub backfilled_records: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceCheckpoint {
    pub tool_id: String,
    pub source_key: String,
    pub source_fingerprint: String,
    pub read_offset: u64,
    pub parser_version: u32,
    pub last_scan_ms: i64,
    pub last_complete_scan_ms: Option<i64>,
    /// "complete", "partial", or "degraded". A degraded scan cannot publish records.
    pub last_status: String,
}

impl SourceCheckpoint {
    pub(super) fn validate(&self) -> Result<()> {
        if self.tool_id.is_empty()
            || self.tool_id.len() > 64
            || !self
                .tool_id
                .bytes()
                .next()
                .is_some_and(|c| c.is_ascii_lowercase())
            || !self
                .tool_id
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        {
            return Err(StoreError::InvalidInput("checkpoint tool_id"));
        }
        if self.source_key.is_empty()
            || self.source_key.len() > 1024
            || self.source_key.chars().any(char::is_control)
        {
            return Err(StoreError::InvalidInput("source_key"));
        }
        if self.source_fingerprint.is_empty()
            || self.source_fingerprint.len() > 1024
            || self.source_fingerprint.chars().any(char::is_control)
        {
            return Err(StoreError::InvalidInput("source_fingerprint"));
        }
        if self.parser_version == 0 {
            return Err(StoreError::InvalidInput("parser_version"));
        }
        if !["complete", "partial", "degraded"].contains(&self.last_status.as_str()) {
            return Err(StoreError::InvalidInput("last_status"));
        }
        db_int(self.read_offset)?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ApplyReport {
    pub inserted: usize,
    pub revised: usize,
    pub unchanged: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    pub applied: ApplyReport,
    pub tombstoned: usize,
    pub scanned: usize,
}

#[derive(Clone, Debug)]
pub struct RemoteUsageChange {
    pub change_seq: u64,
    pub owner_device_id: String,
    pub envelope: RevisionedRecord,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RemoteUsageTotals {
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub unclassified_tokens: u64,
    pub reasoning_tokens: u64,
    pub reasoning_known_records: u64,
    pub record_count: u64,
    pub priced_records: u64,
    pub unpriced_records: u64,
    pub exact_records: u64,
    pub aggregate_records: u64,
    pub estimated_records: u64,
    pub unknown_precision_records: u64,
    pub estimated_cost_microusd: Option<i64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RemoteDeviceUsage {
    pub owner_device_id: String,
    pub total_tokens: u64,
    pub record_count: u64,
    pub estimated_cost_microusd: Option<i64>,
    pub usage: RemoteUsageTotals,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RemoteUsageBreakdown {
    pub owner_device_id: String,
    pub key: String,
    pub label: String,
    pub total_tokens: u64,
    pub record_count: u64,
    pub estimated_cost_microusd: Option<i64>,
    pub exact_records: u64,
    pub aggregate_records: u64,
    pub estimated_records: u64,
    pub unknown_precision_records: u64,
    pub untimed_records: u64,
}

/// A project display name shared between devices independently of usage records.
/// The project ID stays opaque and the source directory path is never included.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectName {
    pub project_id: String,
    pub name: String,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct OutboxEntry {
    pub envelope: RevisionedRecord,
    pub enqueued_at_ms: i64,
    pub attempts: u64,
}

/// Model-response activity, *not* billed Token usage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntigravityActivity {
    pub known_turns: u64,
    pub unknown_turns: u64,
    pub models: Vec<(String, u64)>,
    /// Model, response time, and count for identified responses without usage.
    pub dated_unmeasured: Vec<(String, i64, u64)>,
}
impl AntigravityActivity {
    pub(super) fn validate(&self) -> Result<()> {
        if self.models.len() > 512 {
            return Err(StoreError::InvalidInput("activity model cardinality"));
        }
        let mut seen = HashSet::new();
        let mut sum = 0u64;
        for (model, count) in &self.models {
            if model.is_empty()
                || model.len() > 128
                || !model
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
            {
                return Err(StoreError::InvalidInput("activity model id"));
            }
            if !seen.insert(model) || *count == 0 {
                return Err(StoreError::InvalidInput("activity model duplicate/zero"));
            }
            db_int(*count)?;
            sum = sum
                .checked_add(*count)
                .ok_or(StoreError::IntegerOutOfRange)?;
        }
        if sum != self.known_turns {
            return Err(StoreError::InvalidInput("activity count mismatch"));
        }
        if self.dated_unmeasured.len() > 50_000 {
            return Err(StoreError::InvalidInput("dated activity cardinality"));
        }
        let mut timestamps = HashSet::new();
        let mut remaining: std::collections::HashMap<_, _> = self
            .models
            .iter()
            .map(|(model, count)| (model.as_str(), *count))
            .collect();
        for (model, time, count) in &self.dated_unmeasured {
            if *time < 0 || *count == 0 || !timestamps.insert((model, time)) {
                return Err(StoreError::InvalidInput("dated activity time/count"));
            }
            let available = remaining
                .get_mut(model.as_str())
                .ok_or(StoreError::InvalidInput("dated activity model"))?;
            *available = available
                .checked_sub(*count)
                .ok_or(StoreError::InvalidInput("dated activity count mismatch"))?;
        }
        db_int(self.known_turns)?;
        db_int(self.unknown_turns)?;
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AntigravityActivitySummary {
    pub known_turns: u64,
    pub unknown_turns: u64,
    pub conversations: u64,
    pub by_model: Vec<(String, u64)>,
}

/// Account identity is a user-supplied opaque reference, not an email address
/// or Google credential. Quota snapshots are independent from Token usage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AntigravityQuota {
    pub source_id: String,
    pub pool_id: String,
    pub window_id: String,
    pub remaining_basis_points: Option<u16>,
    pub reset_at_ms: Option<i64>,
    pub observed_at_ms: i64,
}
impl AntigravityQuota {
    pub(super) fn validate(&self) -> Result<()> {
        fn safe(v: &str, limit: usize) -> bool {
            !v.is_empty()
                && v.len() <= limit
                && v.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        }
        if !safe(&self.source_id, 128)
            || !safe(&self.pool_id, 128)
            || !["five_hour", "weekly", "daily", "other"].contains(&self.window_id.as_str())
            || self.remaining_basis_points.is_some_and(|v| v > 10000)
            || self.observed_at_ms < 0
            || self.reset_at_ms.is_some_and(|v| v < 0)
        {
            return Err(StoreError::InvalidInput("quota snapshot"));
        }
        Ok(())
    }
}

/// Codex rate-limit window observed through the local Codex app-server.
/// This is an account quota snapshot, not Token consumption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexQuota {
    pub source_id: String,
    pub pool_id: String,
    pub window_duration_minutes: u32,
    pub remaining_basis_points: Option<u16>,
    pub reset_at_ms: Option<i64>,
    pub observed_at_ms: i64,
}
impl CodexQuota {
    pub(super) fn validate(&self) -> Result<()> {
        let safe = |v: &str, limit: usize| {
            !v.is_empty()
                && v.len() <= limit
                && v.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        };
        if !safe(&self.source_id, 128)
            || !safe(&self.pool_id, 128)
            || self.window_duration_minutes == 0
            || self.remaining_basis_points.is_some_and(|v| v > 10000)
            || self.reset_at_ms.is_some_and(|v| v < 0)
            || self.observed_at_ms < 0
        {
            return Err(StoreError::InvalidInput("Codex quota snapshot"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncOutboxStatus {
    pub pending: u64,
    pub deferred: u64,
    pub blocked: u64,
    pub oldest_pending_at_ms: Option<i64>,
    pub next_attempt_at_ms: Option<i64>,
    pub last_attempt_ms: Option<i64>,
    pub last_success_ms: Option<i64>,
    pub last_pull_seq: u64,
    pub last_pull_ms: Option<i64>,
    pub last_status: String,
    pub last_error: Option<String>,
}
impl Default for SyncOutboxStatus {
    fn default() -> Self {
        Self {
            pending: 0,
            deferred: 0,
            blocked: 0,
            oldest_pending_at_ms: None,
            next_attempt_at_ms: None,
            last_attempt_ms: None,
            last_success_ms: None,
            last_pull_seq: 0,
            last_pull_ms: None,
            last_status: "never".into(),
            last_error: None,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UsageTotals {
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub unclassified_tokens: u64,
    pub reasoning_tokens: u64,
    pub reasoning_known_records: u64,
    pub record_count: u64,
    pub estimated_cost_microusd: Option<i64>,
    pub priced_records: u64,
    pub unpriced_records: u64,
    pub exact_records: u64,
    pub aggregate_records: u64,
    pub estimated_records: u64,
    pub unknown_precision_records: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UsageBreakdown {
    pub key: String,
    pub label: String,
    pub total_tokens: u64,
    pub record_count: u64,
    pub estimated_cost_microusd: Option<i64>,
    pub exact_records: u64,
    pub aggregate_records: u64,
    pub estimated_records: u64,
    pub unknown_precision_records: u64,
    pub untimed_records: u64,
}

/// Lightweight local usage projection used by the diagnostics panel. It avoids
/// building a full dashboard snapshot when the user only needs a health check.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UsageOverview {
    pub total_tokens: u64,
    pub record_count: u64,
    pub last_event_at_ms: Option<i64>,
    pub tools: Vec<ToolUsageOverview>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolUsageOverview {
    pub tool_id: String,
    pub total_tokens: u64,
    pub record_count: u64,
    pub last_event_at_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelActivityBreakdown {
    pub tool_id: String,
    pub model_id: String,
    pub response_count: u64,
    /// `selected_range` for timestamped Codex events; `all_time` for Antigravity metadata.
    pub time_scope: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionUsage {
    /// Stable opaque session key; never a title, file path, or native account ID.
    pub session_id: String,
    pub tool_id: String,
    pub total_tokens: u64,
    pub input_uncached_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub usage_record_count: u64,
    pub exact_records: u64,
    pub aggregate_records: u64,
    pub estimated_records: u64,
    pub unknown_precision_records: u64,
    pub untimed_records: u64,
    pub model_count: u64,
    pub known_response_count: Option<u64>,
    pub unknown_response_count: Option<u64>,
    pub last_event_at_ms: Option<i64>,
    pub activity_observed_at_ms: Option<i64>,
    /// Display-only label captured from the source (never synced).
    pub display_name: Option<String>,
    /// Opaque project identifiers represented by records in this session.
    pub project_ids: Vec<String>,
    /// Per-model token split inside this session.
    pub models: Vec<SessionModelUsage>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionModelUsage {
    pub model_id: Option<String>,
    pub total_tokens: u64,
    pub record_count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionUsagePage {
    pub sessions: Vec<SessionUsage>,
    pub total: u64,
    pub offset: u64,
    pub limit: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionFilterOption {
    pub key: String,
    pub label: String,
    pub session_count: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SessionFilterOptions {
    pub tools: Vec<SessionFilterOption>,
    pub models: Vec<SessionFilterOption>,
    pub projects: Vec<SessionFilterOption>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionAnalysisPage {
    pub page: SessionUsagePage,
    pub filter_options: SessionFilterOptions,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DailyUsage {
    pub date: String,
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteDailyUsage {
    pub owner_device_id: String,
    #[serde(flatten)]
    pub usage: DailyUsage,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceHealth {
    pub tool_id: String,
    pub last_scan_ms: Option<i64>,
    pub last_complete_scan_ms: Option<i64>,
    pub status: String,
    pub source_count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DashboardObservations {
    pub observed_at_ms: i64,
    pub source_health: Vec<SourceHealth>,
    pub codex_quota: Vec<CodexQuota>,
    pub antigravity_quota: Vec<AntigravityQuota>,
    pub sync_status: SyncOutboxStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DashboardSnapshot {
    pub data_revision: u64,
    pub schema_version: i64,
    pub generated_at_ms: i64,
    pub range: String,
    pub range_start_ms: Option<i64>,
    pub range_end_ms: Option<i64>,
    pub time_zone: String,
    pub time_zone_valid: bool,
    pub identity: Option<LocalIdentity>,
    pub local: UsageTotals,
    pub remote: RemoteUsageTotals,
    pub remote_by_device: Vec<RemoteDeviceUsage>,
    pub remote_by_tool: Vec<RemoteUsageBreakdown>,
    pub remote_by_model: Vec<RemoteUsageBreakdown>,
    pub remote_by_project: Vec<RemoteUsageBreakdown>,
    pub unassigned: UsageTotals,
    pub account_global: UsageTotals,
    pub by_tool: Vec<UsageBreakdown>,
    pub by_model: Vec<UsageBreakdown>,
    pub model_activity: Vec<ModelActivityBreakdown>,
    pub by_project: Vec<UsageBreakdown>,
    pub session_usage: SessionUsagePage,
    pub daily: Vec<DailyUsage>,
    pub remote_daily: Vec<RemoteDailyUsage>,
    pub source_health: Vec<SourceHealth>,
    pub antigravity_quota: Vec<AntigravityQuota>,
    pub codex_quota: Vec<CodexQuota>,
    pub antigravity_activity: AntigravityActivitySummary,
    pub sync_status: SyncOutboxStatus,
}

/// Inputs for a read-only dashboard projection.
pub struct DashboardQuery<'a> {
    pub range: &'a str,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    pub daily_ranges: &'a [(String, i64, i64)],
    pub generated_at_ms: i64,
    pub time_zone: String,
    pub time_zone_valid: bool,
    pub session_offset: u64,
}
