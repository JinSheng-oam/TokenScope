mod analytics;
mod ingest;
mod migration;
mod models;
mod quota;
mod records;
mod sync;

use rusqlite::Connection;

pub struct LocalStore {
    conn: Connection,
}

pub(super) const PROJECT_LABEL_TOOL_ID: &str = "project-label";

pub use models::{
    AntigravityActivity, AntigravityActivitySummary, AntigravityQuota, ApplyReport, CodexQuota,
    DailyUsage, DashboardQuery, DashboardSnapshot, LocalIdentity, ModelActivityBreakdown,
    OutboxEntry, ReconcileReport, RemoteDeviceUsage, RemoteUsageBreakdown, RemoteUsageChange,
    RemoteUsageTotals, SessionAnalysisPage, SessionFilterOption, SessionFilterOptions,
    SessionUsage, SessionUsagePage, SourceCheckpoint, SourceHealth, SyncOutboxStatus,
    UsageBreakdown, UsageTotals,
};
