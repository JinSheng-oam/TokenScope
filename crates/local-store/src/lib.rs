//! Local SQLite authority for one device. This crate has NO network access.
//! The Agent owns the sole writer; UI uses IPC rather than opening this file.
//! Writes are atomic: records, revisions, provenance, checkpoints and outbox.

mod error;
mod repository;
pub mod schema;

pub use error::{Result, StoreError};
pub use repository::{ReconcileReport, ApplyReport, LocalIdentity, LocalStore, OutboxEntry, SourceCheckpoint, AntigravityActivity, AntigravityActivitySummary, AntigravityQuota, SyncOutboxStatus, RemoteUsageChange, RemoteUsageTotals, RemoteDeviceUsage, RemoteUsageBreakdown, UsageTotals, UsageBreakdown, DailyUsage, SourceHealth, DashboardSnapshot};
pub const MODULE_ID: &str = "local-store";
