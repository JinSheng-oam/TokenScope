//! Local SQLite authority for one device. This crate has NO network access.
//! The Agent owns the sole writer; UI uses IPC rather than opening this file.
//! Writes are atomic: records, revisions, provenance, checkpoints and outbox.

mod error;
mod repository;
pub mod schema;

pub use error::{Result, StoreError};
pub use repository::{
    AntigravityActivity, AntigravityActivitySummary, AntigravityQuota, ApplyReport, CodexQuota,
    DailyUsage, DashboardQuery, DashboardSnapshot, LocalIdentity, LocalStore,
    ModelActivityBreakdown, OutboxEntry, ReconcileReport, RemoteDeviceUsage, RemoteUsageBreakdown,
    RemoteUsageChange, RemoteUsageTotals, SessionUsage, SessionUsagePage, SourceCheckpoint,
    SourceHealth, SyncOutboxStatus, UsageBreakdown, UsageTotals,
};
pub const MODULE_ID: &str = "local-store";
