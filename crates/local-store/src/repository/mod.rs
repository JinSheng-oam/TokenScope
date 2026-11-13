mod analytics;
mod ingest;
mod migration;
mod models;
mod pricing;
pub use analytics::projection::{
    read_dashboard_snapshot, read_session_analysis, SessionAnalysisOptions,
};
pub use pricing::PricingPreferences;
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
    DailyUsage, DashboardObservations, DashboardQuery, DashboardSnapshot, LocalIdentity,
    ModelActivityBreakdown, ModelPriceCatalogState, ModelPriceEntry, ModelPriceInstallReport,
    OutboxEntry, ProjectName, ReconcileReport, RemoteDailyUsage, RemoteDeviceUsage,
    RemoteUsageBreakdown, RemoteUsageChange, RemoteUsageTotals, SessionAnalysisPage,
    SessionFilterOption, SessionFilterOptions, SessionUsage, SessionUsagePage, SourceCheckpoint,
    SourceHealth, SyncOutboxStatus, ToolUsageOverview, UsageBreakdown, UsageOverview, UsageTotals,
};
