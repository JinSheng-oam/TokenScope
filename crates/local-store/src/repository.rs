use std::collections::HashSet;
use std::path::Path;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use tokenscope_core_domain::{
    payload_hash, CanonicalRecord, Precision, RevisionedRecord, TokenBreakdown, UsageScope,
};
use crate::error::{db_int, pos_u64, Result, StoreError};
use crate::schema;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalIdentity {
    pub device_id: String,
    pub display_name: String,
    pub time_zone: String,
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
    fn validate(&self) -> Result<()> {
        if self.tool_id.is_empty() || self.tool_id.len() > 64 ||
            !self.tool_id.bytes().next().is_some_and(|c| c.is_ascii_lowercase()) ||
            !self.tool_id.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-') {
            return Err(StoreError::InvalidInput("checkpoint tool_id"));
        }
        if self.source_key.is_empty() || self.source_key.len() > 1024 ||
            self.source_key.chars().any(char::is_control) {
            return Err(StoreError::InvalidInput("source_key"));
        }
        if self.source_fingerprint.is_empty() || self.source_fingerprint.len() > 1024 ||
            self.source_fingerprint.chars().any(char::is_control) {
            return Err(StoreError::InvalidInput("source_fingerprint"));
        }
        if self.parser_version == 0 { return Err(StoreError::InvalidInput("parser_version")); }
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
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RemoteUsageBreakdown {
    pub owner_device_id: String,
    pub key: String,
    pub label: String,
    pub total_tokens: u64,
    pub record_count: u64,
    pub estimated_cost_microusd: Option<i64>,
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
}
impl AntigravityActivity {
    fn validate(&self) -> Result<()> {
        if self.models.len() > 512 { return Err(StoreError::InvalidInput("activity model cardinality")); }
        let mut seen = HashSet::new();
        let mut sum = 0u64;
        for (model, count) in &self.models {
            if model.is_empty() || model.len() > 128 || !model.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b)) {
                return Err(StoreError::InvalidInput("activity model id"));
            }
            if !seen.insert(model) || *count == 0 { return Err(StoreError::InvalidInput("activity model duplicate/zero")); }
            db_int(*count)?;
            sum = sum.checked_add(*count).ok_or(StoreError::IntegerOutOfRange)?;
        }
        if sum != self.known_turns { return Err(StoreError::InvalidInput("activity count mismatch")); }
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
    fn validate(&self) -> Result<()> {
        fn safe(v: &str, limit: usize) -> bool {
            !v.is_empty() && v.len() <= limit && v.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        }
        if !safe(&self.source_id, 128) || !safe(&self.pool_id, 128) ||
            !["five_hour", "weekly", "daily", "other"].contains(&self.window_id.as_str()) ||
            self.remaining_basis_points.is_some_and(|v| v > 10000) || self.observed_at_ms < 0 ||
            self.reset_at_ms.is_some_and(|v| v < 0) {
            return Err(StoreError::InvalidInput("quota snapshot"));
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
        Self { pending: 0, deferred: 0, blocked: 0, oldest_pending_at_ms: None,
            next_attempt_at_ms: None, last_attempt_ms: None, last_success_ms: None, last_pull_seq: 0, last_pull_ms: None,
            last_status: "never".into(), last_error: None }
    }
}

pub struct LocalStore {
    conn: Connection,
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DailyUsage {
    pub date: String,
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
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
pub struct DashboardSnapshot {
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
    pub by_project: Vec<UsageBreakdown>,
    pub daily: Vec<DailyUsage>,
    pub source_health: Vec<SourceHealth>,
    pub antigravity_quota: Vec<AntigravityQuota>,
    pub antigravity_activity: AntigravityActivitySummary,
    pub sync_status: SyncOutboxStatus,
}

impl LocalStore {
    /// Opens one local SQLite authority. The caller (Agent) must enforce a
    /// single-process writer lock; SQLite IMMEDIATE transactions serialize writes.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::configure(&conn, true)?;
        Self::migrate(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::configure(&conn, false)?;
        Self::migrate(conn)
    }

    /// Opens an existing database without creating files, running migrations, or
    /// enabling writes. Used by short-lived dashboard queries while the Agent is
    /// the sole writer. The caller must not use mutating methods on this handle.
    pub fn open_readonly(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000; PRAGMA query_only=ON;")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != schema::VERSION {
            return Err(StoreError::UnsupportedSchema(version));
        }
        let migration_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version IN (1,2,3,4)", [], |r| r.get(0)
        )?;
        let highest: i64 = conn.query_row(
            "SELECT COALESCE(MAX(version),0) FROM schema_migrations", [], |r| r.get(0)
        )?;
        if migration_count != 4 || highest != schema::VERSION {
            return Err(StoreError::DatabaseInconsistent("schema_migrations disagrees with user_version"));
        }
        Ok(Self { conn })
    }

    fn configure(conn: &Connection, persistent: bool) -> Result<()> {
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
        if persistent {
            // Each commit is durable when acknowledged, including a power loss.
            conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
        }
        Ok(())
    }

    fn migrate(mut conn: Connection) -> Result<Self> {
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if !(0..=schema::VERSION).contains(&version) {
            return Err(StoreError::UnsupportedSchema(version));
        }
        if version == 0 {
            // Never silently adopt a DB whose schema is unknown / unversioned.
            let legacy: i64 = conn.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('usage_records','source_checkpoints','schema_migrations')",
                [], |r| r.get(0)
            )?;
            if legacy != 0 { return Err(StoreError::UnsupportedSchema(0)); }
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(schema::MIGRATION_001)?;
            tx.execute("INSERT INTO schema_migrations(version,applied_at_ms) VALUES (1,CAST(strftime('%s','now') AS INTEGER)*1000)", [])?;
            tx.execute_batch("PRAGMA user_version=1;")?;
            tx.commit()?;
        }
        if version <= 1 {
            // Fail closed on mismatched legacy metadata before any v2 DDL.
            let v1: i64 = conn.query_row("SELECT COUNT(*) FROM schema_migrations WHERE version=1", [], |r| r.get(0))?;
            let highest: i64 = conn.query_row("SELECT COALESCE(MAX(version),0) FROM schema_migrations", [], |r| r.get(0))?;
            if v1 != 1 || highest != 1 {
                return Err(StoreError::DatabaseInconsistent("legacy migration metadata invalid"));
            }
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(schema::MIGRATION_002)?;
            tx.execute("INSERT INTO schema_migrations(version,applied_at_ms) VALUES (2,CAST(strftime('%s','now') AS INTEGER)*1000)", [])?;
            tx.execute_batch("PRAGMA user_version=2;")?;
            tx.commit()?;
        }
        if version <= 2 {
            let prior: i64 = conn.query_row("SELECT COUNT(*) FROM schema_migrations WHERE version IN (1,2)", [], |r| r.get(0))?;
            let highest: i64 = conn.query_row("SELECT COALESCE(MAX(version),0) FROM schema_migrations", [], |r| r.get(0))?;
            if prior != 2 || highest != 2 {
                return Err(StoreError::DatabaseInconsistent("legacy migration metadata invalid before v3"));
            }
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(schema::MIGRATION_003)?;
            tx.execute("INSERT INTO schema_migrations(version,applied_at_ms) VALUES (3,CAST(strftime('%s','now') AS INTEGER)*1000)", [])?;
            tx.execute_batch("PRAGMA user_version=3;")?;
            tx.commit()?;
        }
        if version <= 3 {
            let prior: i64 = conn.query_row("SELECT COUNT(*) FROM schema_migrations WHERE version IN (1,2,3)", [], |r| r.get(0))?;
            let highest: i64 = conn.query_row("SELECT COALESCE(MAX(version),0) FROM schema_migrations", [], |r| r.get(0))?;
            if prior != 3 || highest != 3 { return Err(StoreError::DatabaseInconsistent("legacy migration metadata invalid before v4")); }
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(schema::MIGRATION_004)?;
            tx.execute("INSERT INTO schema_migrations(version,applied_at_ms) VALUES (4,CAST(strftime('%s','now') AS INTEGER)*1000)", [])?;
            tx.execute_batch("PRAGMA user_version=4;")?;
            tx.commit()?;
        }
        let count: i64 = conn.query_row(
            "SELECT count(*) FROM schema_migrations WHERE version IN (1,2,3,4)", [], |r| r.get(0)
        )?;
        let future: i64 = conn.query_row("SELECT COALESCE(MAX(version),0) FROM schema_migrations", [], |r| r.get(0))?;
        if count != 4 || future != schema::VERSION {
            return Err(StoreError::DatabaseInconsistent("schema_migrations disagrees with user_version"));
        }
        let foreign_key_violations: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r.get(0)
        )?;
        if foreign_key_violations != 0 { return Err(StoreError::DatabaseInconsistent("foreign key violation")); }
        Ok(Self { conn })
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    /// Transactionally consistent online snapshot. The destination must not
    /// already exist. Never copy a live main DB file without its WAL state.
    pub fn backup_to(&self, destination: &Path) -> Result<()> {
        let path = destination.to_str().ok_or(StoreError::InvalidInput("backup path encoding"))?;
        if destination.exists() { return Err(StoreError::InvalidInput("backup destination exists")); }
        self.conn.execute("VACUUM INTO ?1", [path])?;
        Ok(())
    }

    pub fn quick_integrity_check(&self) -> Result<()> {
        let status: String = self.conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if status != "ok" { return Err(StoreError::DatabaseInconsistent("SQLite quick_check failed")); }
        Ok(())
    }

    /// Bootstrap once, before scanning. Identity cannot be rotated implicitly.
    pub fn init_identity(&mut self, identity: &LocalIdentity) -> Result<()> {
        fn valid_text(v: &str, max: usize) -> bool {
            !v.trim().is_empty() && v.len() <= max && !v.chars().any(char::is_control)
        }
        if !valid_text(&identity.device_id, 128) ||
            !identity.device_id.is_ascii() || identity.device_id.chars().any(char::is_whitespace) ||
            !valid_text(&identity.display_name, 128) || !valid_text(&identity.time_zone, 128) {
            return Err(StoreError::InvalidInput("local identity"));
        }
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<String> = tx.query_row(
            "SELECT device_id FROM local_identity WHERE singleton=1", [], |r| r.get(0)
        ).optional()?;
        match existing {
            None => { tx.execute(
                "INSERT INTO local_identity(singleton,device_id,display_name,time_zone) VALUES(1,?1,?2,?3)",
                params![identity.device_id, identity.display_name, identity.time_zone]
            )?; }
            Some(id) if id == identity.device_id => {
                tx.execute("UPDATE local_identity SET display_name=?1, time_zone=?2 WHERE singleton=1",
                    params![identity.display_name, identity.time_zone])?;
            }
            _ => return Err(StoreError::IdentityMismatch),
        }
        tx.commit()?;
        Ok(())
    }

    pub fn identity(&self) -> Result<Option<LocalIdentity>> {
        Ok(self.conn.query_row(
            "SELECT device_id,display_name,time_zone FROM local_identity WHERE singleton=1",
            [], |r| Ok(LocalIdentity {
                device_id: r.get(0)?, display_name: r.get(1)?, time_zone: r.get(2)?
            })
        ).optional()?)
    }

    pub fn checkpoint(&self, tool_id: &str, source_key: &str) -> Result<Option<SourceCheckpoint>> {
        load_checkpoint(&self.conn, tool_id, source_key)
    }

    /// Entire scan result is one BEGIN IMMEDIATE transaction: current records,
    /// record_sources, outbox CAS, and checkpoint. No implicit source deletions.
    /// `expected` is the exact checkpoint that the Adapter read before parsing.
    pub fn apply_scan(
        &mut self,
        expected: Option<&SourceCheckpoint>,
        next: &SourceCheckpoint,
        proposed: &[CanonicalRecord],
        observed_at_ms: i64,
        allow_explicit_tombstones: bool,
    ) -> Result<ApplyReport> {
        next.validate()?;
        // Incremental-only API is bounded; a *complete* source containing more
        // records must use reconcile_complete_source for atomic reconciliation.
        if proposed.len() > 2000 { return Err(StoreError::InvalidInput("scan batch limit (2000)")); }
        if let Some(old) = expected {
            old.validate()?;
            if old.tool_id != next.tool_id || old.source_key != next.source_key {
                return Err(StoreError::InvalidInput("checkpoint identity"));
            }
            if next.last_scan_ms < old.last_scan_ms {
                return Err(StoreError::InvalidInput("scan time must not regress"));
            }
        }
        if next.last_status == "degraded" {
            if !proposed.is_empty() {
                return Err(StoreError::InvalidInput("degraded scan cannot publish records"));
            }
            // A failed read may update health, but it must not advance the
            // last-known-good byte cursor, parser or source fingerprint.
            if let Some(old) = expected {
                if next.read_offset != old.read_offset ||
                    next.source_fingerprint != old.source_fingerprint ||
                    next.parser_version != old.parser_version ||
                    next.last_complete_scan_ms != old.last_complete_scan_ms {
                    return Err(StoreError::InvalidInput("degraded checkpoint advances cursor"));
                }
            } else if next.read_offset != 0 || next.last_complete_scan_ms.is_some() {
                return Err(StoreError::InvalidInput("degraded initial checkpoint"));
            }
        }
        if next.last_status == "complete" && next.last_complete_scan_ms != Some(next.last_scan_ms) {
            return Err(StoreError::InvalidInput("complete scan time"));
        }
        // Validate every record BEFORE starting the transaction. Reject duplicate
        // identities rather than depend on adapter ordering/last-write-wins.
        let mut seen = HashSet::with_capacity(proposed.len());
        let mut canonical = Vec::with_capacity(proposed.len());
        for rec in proposed {
            let rec = rec.clone().normalized()?;
            if rec.tool_id != next.tool_id { return Err(StoreError::InvalidInput("tool mismatch")); }
            if rec.is_deleted && !allow_explicit_tombstones {
                return Err(StoreError::InvalidInput("tombstones require explicit authorization"));
            }
            if !seen.insert(rec.record_id.clone()) { return Err(StoreError::DuplicateBatchRecord); }
            // SQLite stores signed 64-bit counters even though the domain accepts u64.
            for n in [rec.usage.input_uncached_tokens, rec.usage.cache_read_tokens,
                      rec.usage.cache_write_tokens, rec.usage.output_tokens,
                      rec.usage.unclassified_tokens] { db_int(n)?; }
            if let Some(n) = rec.usage.reasoning_tokens { db_int(n)?; }
            canonical.push((payload_hash(&rec)?, rec));
        }
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_checkpoint(&tx, &next.tool_id, &next.source_key)?;
        if current.as_ref() != expected { return Err(StoreError::CheckpointConflict); }
        let owner: Option<String> = tx.query_row(
            "SELECT device_id FROM local_identity WHERE singleton=1", [], |r| r.get(0)
        ).optional()?;
        let owner = owner.ok_or(StoreError::NotInitialized)?;
        let mut report = ApplyReport::default();
        for (hash, rec) in canonical {
            if rec.owner_device_id != owner { return Err(StoreError::IdentityMismatch); }
            match upsert_source_record(&tx, &rec, &hash, &next.source_key, &owner, observed_at_ms)? {
                RecordChange::Inserted => report.inserted += 1,
                RecordChange::Revised => report.revised += 1,
                RecordChange::Unchanged => report.unchanged += 1,
            }
        }
        tx.execute(
            "INSERT INTO source_checkpoints(tool_id,source_key,source_fingerprint,read_offset,parser_version,last_scan_ms,last_complete_scan_ms,last_status) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8) \
             ON CONFLICT(tool_id,source_key) DO UPDATE SET source_fingerprint=excluded.source_fingerprint, \
                read_offset=excluded.read_offset,parser_version=excluded.parser_version, \
                last_scan_ms=excluded.last_scan_ms,last_complete_scan_ms=excluded.last_complete_scan_ms,last_status=excluded.last_status",
            params![next.tool_id,next.source_key,next.source_fingerprint,db_int(next.read_offset)?,
                    i64::from(next.parser_version),next.last_scan_ms,next.last_complete_scan_ms,next.last_status]
        )?;
        tx.commit()?;
        Ok(report)
    }

    /// Atomic complete source reconciliation. Iteration accepts any number of records,
    /// processing one normalized event at a time. The in-memory staging set is
    /// replaced by a TEMP SQLite primary-key index, and the checkpoint/outbox
    /// commit together. Any error (including a duplicate) rolls everything back.
    ///
    /// Only call after the adapter has verified that the source is complete and
    /// unchanged during parsing. Incomplete JSONL or incremental-only scans must
    /// use `apply_scan` instead and must NEVER tombstone unseen old rows.
    pub fn reconcile_complete_source<I>(
        &mut self,
        expected: Option<&SourceCheckpoint>,
        next: &SourceCheckpoint,
        records: I,
        observed_at_ms: i64,
    ) -> Result<ReconcileReport>
    where
        I: IntoIterator<Item = CanonicalRecord>,
    {
        self.reconcile_source_with_activity(expected, next, records, observed_at_ms, None)
    }

    /// One transaction for authoritative usage rows, activity counts, checkpoint
    /// and Outbox. `None` leaves activity untouched; `Some` is a complete,
    /// verified Antigravity source snapshot (including a 0-turn source).
    pub fn reconcile_source_with_activity<I>(
        &mut self,
        expected: Option<&SourceCheckpoint>,
        next: &SourceCheckpoint,
        records: I,
        observed_at_ms: i64,
        activity: Option<&AntigravityActivity>,
    ) -> Result<ReconcileReport>
    where
        I: IntoIterator<Item = CanonicalRecord>,
    {
        next.validate()?;
        if let Some(a) = activity { a.validate()?;
            if next.tool_id != "antigravity" { return Err(StoreError::InvalidInput("activity tool")); }
        }
        if next.last_status != "complete" || next.last_complete_scan_ms != Some(next.last_scan_ms) {
            return Err(StoreError::InvalidInput("complete reconciliation requires full scan"));
        }
        if let Some(old) = expected {
            old.validate()?;
            if old.tool_id != next.tool_id || old.source_key != next.source_key ||
                next.last_scan_ms < old.last_scan_ms {
                return Err(StoreError::InvalidInput("checkpoint identity/time"));
            }
        }
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let actual = load_checkpoint(&tx, &next.tool_id, &next.source_key)?;
        if actual.as_ref() != expected { return Err(StoreError::CheckpointConflict); }
        let owner: String = tx.query_row(
            "SELECT device_id FROM local_identity WHERE singleton=1", [], |r| r.get(0)
        ).optional()?.ok_or(StoreError::NotInitialized)?;
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS tokenscope_seen (record_id TEXT PRIMARY KEY) WITHOUT ROWID; \
             DELETE FROM temp.tokenscope_seen;"
        )?;
        let mut report = ReconcileReport::default();
        for record in records {
            let rec = record.normalized()?;
            if rec.tool_id != next.tool_id || rec.is_deleted {
                return Err(StoreError::InvalidInput("source reconciliation record"));
            }
            if rec.owner_device_id != owner { return Err(StoreError::IdentityMismatch); }
            for n in [rec.usage.input_uncached_tokens, rec.usage.cache_read_tokens,
                rec.usage.cache_write_tokens, rec.usage.output_tokens,
                rec.usage.unclassified_tokens] { db_int(n)?; }
            if let Some(n) = rec.usage.reasoning_tokens { db_int(n)?; }
            let changed = tx.execute(
                "INSERT OR IGNORE INTO temp.tokenscope_seen(record_id) VALUES (?1)",
                [&rec.record_id],
            )?;
            if changed != 1 { return Err(StoreError::DuplicateBatchRecord); }
            let hash = payload_hash(&rec)?;
            match upsert_source_record(&tx, &rec, &hash, &next.source_key, &owner, observed_at_ms)? {
                RecordChange::Inserted => report.applied.inserted += 1,
                RecordChange::Revised => report.applied.revised += 1,
                RecordChange::Unchanged => report.applied.unchanged += 1,
            }
            report.scanned += 1;
        }
        // Page old records by record_id instead of fetching every old source
        // record into application memory. Never remove a row merely because
        // it was not in a *partial* scan.
        let mut last_rowid = i64::MIN;
        loop {
            // The existing ix_record_sources_source index naturally orders its
            // suffix by rowid, so seek via rowid instead of re-sorting large
            // per-source sets by hashed record_id on every iteration.
            let missing: Option<(i64, String)> = tx.query_row(
                "SELECT rs.rowid, rs.record_id FROM record_sources rs \
                 JOIN usage_records u ON u.record_id=rs.record_id \
                 WHERE rs.tool_id=?1 AND rs.source_key=?2 \
                 AND rs.rowid>?3 AND u.is_deleted=0 \
                 AND NOT EXISTS (SELECT 1 FROM temp.tokenscope_seen s WHERE s.record_id=rs.record_id) \
                 ORDER BY rs.rowid LIMIT 1",
                params![next.tool_id, next.source_key, last_rowid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ).optional()?;
            let Some((rowid, record_id)) = missing else { break; };
            last_rowid = rowid;
            let source_count: i64 = tx.query_row(
                "SELECT COUNT(*) FROM record_sources WHERE record_id=?1",
                [&record_id], |r| r.get(0),
            )?;
            if source_count != 1 { return Err(StoreError::SourceArbitrationConflict); }
            let mut record = read_record(&tx, &record_id)?
                .ok_or(StoreError::DatabaseInconsistent("source record missing"))?.record;
            record.is_deleted = true;
            record.usage = TokenBreakdown::default();
            record.estimated_cost_microusd = None;
            record.price_version = None;
            let hash = payload_hash(&record)?;
            match upsert_source_record(&tx, &record, &hash, &next.source_key, &owner, observed_at_ms)? {
                RecordChange::Revised => report.tombstoned += 1,
                _ => return Err(StoreError::DatabaseInconsistent("unexpected tombstone result")),
            }
        }
        tx.execute(
            "INSERT INTO source_checkpoints(tool_id,source_key,source_fingerprint,read_offset,parser_version,last_scan_ms,last_complete_scan_ms,last_status) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8) \
             ON CONFLICT(tool_id,source_key) DO UPDATE SET source_fingerprint=excluded.source_fingerprint, \
             read_offset=excluded.read_offset,parser_version=excluded.parser_version, \
             last_scan_ms=excluded.last_scan_ms,last_complete_scan_ms=excluded.last_complete_scan_ms,last_status=excluded.last_status",
            params![next.tool_id,next.source_key,next.source_fingerprint,db_int(next.read_offset)?,
                i64::from(next.parser_version),next.last_scan_ms,next.last_complete_scan_ms,next.last_status],
        )?;
        if let Some(a) = activity {
            tx.execute("INSERT INTO antigravity_activity_sources(source_key,known_turns,unknown_turns,observed_at_ms) \
                VALUES (?1,?2,?3,?4) ON CONFLICT(source_key) DO UPDATE SET \
                known_turns=excluded.known_turns,unknown_turns=excluded.unknown_turns,observed_at_ms=excluded.observed_at_ms",
                params![next.source_key, db_int(a.known_turns)?, db_int(a.unknown_turns)?, observed_at_ms])?;
            tx.execute("DELETE FROM antigravity_model_activity WHERE source_key=?1", [&next.source_key])?;
            for (model,count) in &a.models {
                tx.execute("INSERT INTO antigravity_model_activity(source_key,model_id,response_count) VALUES(?1,?2,?3)",
                    params![next.source_key,model,db_int(*count)?])?;
            }
        }
        // Temp seen keys are erased inside the same transaction. In-memory
        // capacity remains bounded by a record and a page cursor.
        tx.execute("DELETE FROM temp.tokenscope_seen", [])?;
        tx.commit()?;
        Ok(report)
    }

    fn usage_totals(&self, owner_device_id: &str, scope: &str, from_ms: Option<i64>, to_ms: Option<i64>) -> Result<UsageTotals> {
        let row = self.conn.query_row(
            "SELECT \
             COALESCE(SUM(input_uncached_tokens + cache_read_tokens + cache_write_tokens + output_tokens + unclassified_tokens),0), \
             COALESCE(SUM(input_uncached_tokens),0), COALESCE(SUM(cache_read_tokens),0), \
             COALESCE(SUM(cache_write_tokens),0), COALESCE(SUM(output_tokens),0), \
             COALESCE(SUM(unclassified_tokens),0), COALESCE(SUM(COALESCE(reasoning_tokens,0)),0), \
             COALESCE(SUM(CASE WHEN reasoning_tokens IS NOT NULL THEN 1 ELSE 0 END),0), \
             COUNT(*), COALESCE(SUM(CASE WHEN estimated_cost_microusd IS NOT NULL THEN estimated_cost_microusd ELSE 0 END),0), \
             COALESCE(SUM(CASE WHEN estimated_cost_microusd IS NOT NULL THEN 1 ELSE 0 END),0), \
             COALESCE(SUM(CASE WHEN estimated_cost_microusd IS NULL THEN 1 ELSE 0 END),0), \
             COALESCE(SUM(CASE WHEN precision='exact' THEN 1 ELSE 0 END),0), \
             COALESCE(SUM(CASE WHEN precision='aggregate' THEN 1 ELSE 0 END),0), \
             COALESCE(SUM(CASE WHEN precision='estimated' THEN 1 ELSE 0 END),0), \
             COALESCE(SUM(CASE WHEN precision='unknown' THEN 1 ELSE 0 END),0) \
             FROM usage_records WHERE owner_device_id=?1 AND scope=?2 AND is_deleted=0 \
             AND (?3 IS NULL OR event_at_ms >= ?3) AND (?4 IS NULL OR event_at_ms < ?4)",
            params![owner_device_id, scope, from_ms, to_ms],
            |r| Ok((
                r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?,
                r.get::<_,i64>(4)?,r.get::<_,i64>(5)?,r.get::<_,i64>(6)?,r.get::<_,i64>(7)?,
                r.get::<_,i64>(8)?,r.get::<_,i64>(9)?,r.get::<_,i64>(10)?,r.get::<_,i64>(11)?,
                r.get::<_,i64>(12)?,r.get::<_,i64>(13)?,r.get::<_,i64>(14)?,r.get::<_,i64>(15)?,
            )),
        )?;
        let (total,input,cache_read,cache_write,output,unclassified,reasoning,reasoning_known,count,cost,priced,unpriced,exact,aggregate,estimated,unknown) = row;
        Ok(UsageTotals {
            total_tokens: pos_u64(total)?, input_tokens: pos_u64(input)?,
            cache_read_tokens: pos_u64(cache_read)?, cache_write_tokens: pos_u64(cache_write)?,
            output_tokens: pos_u64(output)?, unclassified_tokens: pos_u64(unclassified)?,
            reasoning_tokens: pos_u64(reasoning)?, reasoning_known_records: pos_u64(reasoning_known)?,
            record_count: pos_u64(count)?, estimated_cost_microusd: if priced > 0 { Some(cost) } else { None },
            priced_records: pos_u64(priced)?, unpriced_records: pos_u64(unpriced)?,
            exact_records: pos_u64(exact)?, aggregate_records: pos_u64(aggregate)?,
            estimated_records: pos_u64(estimated)?, unknown_precision_records: pos_u64(unknown)?,
        })
    }

    fn usage_breakdown(&self, column: &'static str, owner_device_id: &str, scope: &str, from_ms: Option<i64>, to_ms: Option<i64>) -> Result<Vec<UsageBreakdown>> {
        // `column` is a compile-time allowlist supplied only by dashboard_snapshot.
        if !["tool_id", "model_id", "project_id"].contains(&column) {
            return Err(StoreError::InvalidInput("dashboard breakdown column"));
        }
        let sql = format!(
            "SELECT {column}, COUNT(*), \
             COALESCE(SUM(input_uncached_tokens + cache_read_tokens + cache_write_tokens + output_tokens + unclassified_tokens),0), \
             COALESCE(SUM(CASE WHEN estimated_cost_microusd IS NOT NULL THEN estimated_cost_microusd ELSE 0 END),0), \
             COALESCE(SUM(CASE WHEN estimated_cost_microusd IS NOT NULL THEN 1 ELSE 0 END),0) \
             FROM usage_records WHERE owner_device_id=?1 AND scope=?2 AND is_deleted=0 \
             AND (?3 IS NULL OR event_at_ms >= ?3) AND (?4 IS NULL OR event_at_ms < ?4) \
             GROUP BY {column} ORDER BY 3 DESC LIMIT 20"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![owner_device_id, scope, from_ms, to_ms], |r| Ok((
            r.get::<_,Option<String>>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,
            r.get::<_,i64>(3)?,r.get::<_,i64>(4)?,
        )))?;
        let mut result = Vec::new();
        for row in rows {
            let (raw_key,count,total,cost,priced) = row?;
            let (key,label) = match (column, raw_key) {
                ("tool_id", Some(value)) => (value.clone(), value),
                ("model_id", Some(value)) => (value.clone(), value),
                ("project_id", Some(value)) => {
                    let short = value.strip_prefix("prj_").unwrap_or(&value);
                    let short = short.chars().take(8).collect::<String>();
                    (value, format!("项目 {short}"))
                }
                ("tool_id", None) => ("unknown-tool".into(), "未知工具".into()),
                ("model_id", None) => ("unknown-model".into(), "未知模型".into()),
                ("project_id", None) => ("no-project".into(), "未关联项目".into()),
                _ => return Err(StoreError::InvalidInput("dashboard breakdown column")),
            };
            result.push(UsageBreakdown {
                key, label, total_tokens: pos_u64(total)?, record_count: pos_u64(count)?,
                estimated_cost_microusd: if priced > 0 { Some(cost) } else { None },
            });
        }
        Ok(result)
    }

    /// Read-only dashboard aggregate. Daily ranges are calculated by the Agent
    /// with the device IANA timezone, so DST boundaries are not assumed to be 24h.
    pub fn dashboard_snapshot(
        &self, range: &str, from_ms: Option<i64>, to_ms: Option<i64>,
        daily_ranges: &[(String, i64, i64)], generated_at_ms: i64,
        time_zone: String, time_zone_valid: bool,
    ) -> Result<DashboardSnapshot> {
        if !["today", "7d", "30d", "all"].contains(&range) || generated_at_ms < 0 ||
            to_ms.is_some_and(|v| v < 0) || from_ms.is_some_and(|v| v < 0) ||
            from_ms.zip(to_ms).is_some_and(|(a,b)| a >= b) || daily_ranges.len() > 31 {
            return Err(StoreError::InvalidInput("dashboard query"));
        }
        let identity = self.identity()?;
        let owner_device_id = identity.as_ref().map(|v| v.device_id.as_str()).unwrap_or("");
        let mut daily = Vec::with_capacity(daily_ranges.len());
        for (date,start,end) in daily_ranges {
            if date.len() != 10 || *start < 0 || *end <= *start {
                return Err(StoreError::InvalidInput("dashboard daily range"));
            }
            let totals = self.usage_totals(owner_device_id, "device_local", Some(*start), Some(*end))?;
            daily.push(DailyUsage {
                date: date.clone(), total_tokens: totals.total_tokens,
                input_tokens: totals.input_tokens, output_tokens: totals.output_tokens,
            });
        }
        let mut health = std::collections::BTreeMap::<String, SourceHealth>::new();
        let mut stmt = self.conn.prepare(
            "SELECT tool_id,last_scan_ms,last_complete_scan_ms,last_status FROM source_checkpoints ORDER BY tool_id,last_scan_ms"
        )?;
        let rows = stmt.query_map([], |r| Ok((
            r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,Option<i64>>(2)?,r.get::<_,String>(3)?,
        )))?;
        for row in rows {
            let (tool,last_scan,last_complete,status) = row?;
            let entry = health.entry(tool.clone()).or_insert(SourceHealth {
                tool_id: tool, last_scan_ms: None, last_complete_scan_ms: None,
                status: "unknown".into(), source_count: 0,
            });
            entry.source_count += 1;
            if entry.last_scan_ms.is_none_or(|old| last_scan >= old) {
                entry.last_scan_ms = Some(last_scan);
                entry.status = status;
            }
            if let Some(value) = last_complete {
                if entry.last_complete_scan_ms.is_none_or(|old| value > old) {
                    entry.last_complete_scan_ms = Some(value);
                }
            }
        }
        Ok(DashboardSnapshot {
            schema_version: schema::VERSION, generated_at_ms, range: range.into(),
            range_start_ms: from_ms, range_end_ms: to_ms, time_zone, time_zone_valid,
            identity: identity.clone(),
            local: self.usage_totals(owner_device_id, "device_local", from_ms, to_ms)?,
            remote: self.remote_usage_totals(None, from_ms, to_ms)?,
            remote_by_device: self.remote_usage_by_device(from_ms, to_ms)?,
            remote_by_tool: self.remote_usage_breakdown("tool_id", from_ms, to_ms)?,
            remote_by_model: self.remote_usage_breakdown("model_id", from_ms, to_ms)?,
            remote_by_project: self.remote_usage_breakdown("project_id", from_ms, to_ms)?,
            unassigned: self.usage_totals(owner_device_id, "unknown", from_ms, to_ms)?,
            account_global: self.usage_totals(owner_device_id, "account_global", from_ms, to_ms)?,
            by_tool: self.usage_breakdown("tool_id", owner_device_id, "device_local", from_ms, to_ms)?,
            by_model: self.usage_breakdown("model_id", owner_device_id, "device_local", from_ms, to_ms)?,
            by_project: self.usage_breakdown("project_id", owner_device_id, "device_local", from_ms, to_ms)?,
            daily, source_health: health.into_values().collect(),
            antigravity_quota: self.antigravity_quota_snapshots(200)?,
            antigravity_activity: self.antigravity_activity_summary()?,
            sync_status: self.sync_outbox_status(generated_at_ms)?,
        })
    }

    /// Read only known Antigravity activity, independent of usage/outbox.
    pub fn antigravity_activity_summary(&self) -> Result<AntigravityActivitySummary> {
        let (known, unknown, conversations): (i64,i64,i64) = self.conn.query_row(
            "SELECT COALESCE(SUM(known_turns),0),COALESCE(SUM(unknown_turns),0),COUNT(*) FROM antigravity_activity_sources",
            [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        let mut stmt = self.conn.prepare("SELECT model_id,SUM(response_count) FROM antigravity_model_activity GROUP BY model_id ORDER BY model_id LIMIT 513")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_,String>(0)?, r.get::<_,i64>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if rows.len() > 512 { return Err(StoreError::InvalidInput("activity summary too many models")); }
        Ok(AntigravityActivitySummary {
            known_turns: pos_u64(known)?, unknown_turns: pos_u64(unknown)?,
            conversations: pos_u64(conversations)?,
            by_model: rows.into_iter().map(|(m,n)| Ok((m,pos_u64(n)?))).collect::<Result<Vec<_>>>()?,
        })
    }

    /// Atomic import of a bounded list of manually observed quota snapshots.
    /// This never touches Token usage, revision/outbox, or OAuth credentials.
    pub fn upsert_antigravity_quotas(&mut self, batch: &[AntigravityQuota], received_at_ms: i64) -> Result<usize> {
        if batch.is_empty() || batch.len() > 200 || received_at_ms < 0 {
            return Err(StoreError::InvalidInput("quota batch size/time"));
        }
        let mut seen = HashSet::with_capacity(batch.len());
        for row in batch {
            row.validate()?;
            if row.observed_at_ms > received_at_ms.saturating_add(60_000) {
                return Err(StoreError::InvalidInput("quota future observation"));
            }
            if !seen.insert((&row.source_id,&row.pool_id,&row.window_id)) {
                return Err(StoreError::InvalidInput("duplicate quota identity"));
            }
        }
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut changed = 0;
        for snapshot in batch {
            changed += tx.execute(
                "INSERT INTO antigravity_quota_snapshots(source_id,pool_id,window_id,remaining_basis_points,reset_at_ms,observed_at_ms,received_at_ms) \
                 VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(source_id,pool_id,window_id) DO UPDATE SET \
                 remaining_basis_points=excluded.remaining_basis_points,reset_at_ms=excluded.reset_at_ms, \
                 observed_at_ms=excluded.observed_at_ms,received_at_ms=excluded.received_at_ms \
                 WHERE excluded.observed_at_ms > antigravity_quota_snapshots.observed_at_ms",
                params![snapshot.source_id,snapshot.pool_id,snapshot.window_id,
                    snapshot.remaining_basis_points.map(i64::from),snapshot.reset_at_ms,
                    snapshot.observed_at_ms,received_at_ms]
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }

    pub fn antigravity_quota_snapshots(&self, limit: usize) -> Result<Vec<AntigravityQuota>> {
        if limit == 0 || limit > 200 { return Err(StoreError::InvalidInput("quota read limit")); }
        let mut stmt = self.conn.prepare(
            "SELECT source_id,pool_id,window_id,remaining_basis_points,reset_at_ms,observed_at_ms \
             FROM antigravity_quota_snapshots ORDER BY source_id,pool_id,window_id LIMIT ?1")?;
        let iter = stmt.query_map([i64::try_from(limit).map_err(|_| StoreError::IntegerOutOfRange)?], |r| {
            let fraction: Option<i64> = r.get(3)?;
            Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,fraction,
                r.get::<_,Option<i64>>(4)?,r.get::<_,i64>(5)?))
        })?;
        iter.map(|entry| {
            let (source_id,pool_id,window_id,remaining,reset_at_ms,observed_at_ms) = entry?;
            let remaining_basis_points = remaining.map(|n| u16::try_from(n).map_err(|_| StoreError::IntegerOutOfRange)).transpose()?;
            Ok(AntigravityQuota { source_id,pool_id,window_id,remaining_basis_points,reset_at_ms,observed_at_ms })
        }).collect::<Result<Vec<_>>>()
    }

    /// Apply a bounded Hub change-feed page and advance its cursor atomically.
    /// Older revisions are ignored; same-revision hash conflicts abort the entire page.
    pub fn pull_cursor(&self) -> Result<u64> {
        let value:i64=self.conn.query_row("SELECT last_pull_seq FROM sync_state WHERE singleton=1", [], |r| r.get(0))?;
        pos_u64(value)
    }

    pub fn apply_remote_changes(&mut self, changes: &[RemoteUsageChange], next_cursor: u64) -> Result<usize> {
        if changes.len() > 500 { return Err(StoreError::InvalidInput("remote page limit")); }
        let local_device_id = self.identity()?.map(|i| i.device_id).unwrap_or_default();
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current_cursor: i64 = tx.query_row("SELECT last_pull_seq FROM sync_state WHERE singleton=1", [], |r| r.get(0))?;
        let mut previous = u64::try_from(current_cursor).map_err(|_| StoreError::IntegerOutOfRange)?;
        let mut applied = 0usize;
        for change in changes {
            if change.change_seq <= previous || change.change_seq > next_cursor || change.owner_device_id != change.envelope.record.owner_device_id {
                return Err(StoreError::InvalidInput("remote change sequence/owner"));
            }
            change.envelope.validate().map_err(|_| StoreError::InvalidInput("remote envelope"))?;
            if change.owner_device_id == local_device_id {
                previous = change.change_seq;
                continue;
            }
            let raw = serde_json::to_string(&change.envelope.record).map_err(|_| StoreError::InvalidInput("remote serialization"))?;
            let rev = db_int(change.envelope.record_revision)?;
            let mut old: Option<(i64,String)> = tx.query_row(
                "SELECT record_revision,payload_hash FROM remote_usage_cache WHERE owner_device_id=?1 AND record_id=?2",
                params![change.owner_device_id,change.envelope.record.record_id], |r| Ok((r.get(0)?,r.get(1)?))
            ).optional()?;
            if let Some((old_rev,old_hash)) = old.take() {
                if rev < old_rev { previous = change.change_seq; continue; }
                if rev == old_rev && !old_hash.is_empty() {
                    if old_hash != change.envelope.payload_hash { return Err(StoreError::DatabaseInconsistent("remote same-revision payload conflict")); }
                    previous = change.change_seq; continue;
                }
            }
            tx.execute(
                "INSERT INTO remote_usage_cache(owner_device_id,record_id,record_revision,payload_json,is_deleted,server_change_seq,payload_hash) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(owner_device_id,record_id) DO UPDATE SET record_revision=excluded.record_revision,payload_json=excluded.payload_json,is_deleted=excluded.is_deleted,server_change_seq=excluded.server_change_seq,payload_hash=excluded.payload_hash WHERE excluded.record_revision>remote_usage_cache.record_revision OR (excluded.record_revision=remote_usage_cache.record_revision AND remote_usage_cache.payload_hash='')",
                params![change.owner_device_id,change.envelope.record.record_id,rev,raw,change.envelope.record.is_deleted as i64,db_int(change.change_seq)?,change.envelope.payload_hash]
            )?;
            applied += 1;
            previous = change.change_seq;
        }
        if next_cursor != previous {
            return Err(StoreError::InvalidInput("remote cursor must match last applied sequence"));
        }
        tx.execute("UPDATE sync_state SET last_pull_seq=?1,last_pull_ms=CAST(strftime('%s','now') AS INTEGER)*1000 WHERE singleton=1", [db_int(next_cursor)?])?;
        tx.commit()?;
        Ok(applied)
    }

    pub fn remote_usage_totals(&self, owner_device_id: Option<&str>, start_ms: Option<i64>, end_ms: Option<i64>) -> Result<RemoteUsageTotals> {
        let (total,input,cache_read,cache_write,output,unclassified,count,priced,exact,aggregate,estimated,unknown,cost):(i64,i64,i64,i64,i64,i64,i64,i64,i64,i64,i64,i64,Option<i64>) = self.conn.query_row(
            "SELECT COALESCE(SUM(CAST(json_extract(payload_json,'$.usage.input_uncached_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.cache_read_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.cache_write_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.output_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.unclassified_tokens') AS INTEGER)),0),COALESCE(SUM(CAST(json_extract(payload_json,'$.usage.input_uncached_tokens') AS INTEGER)),0),COALESCE(SUM(CAST(json_extract(payload_json,'$.usage.cache_read_tokens') AS INTEGER)),0),COALESCE(SUM(CAST(json_extract(payload_json,'$.usage.cache_write_tokens') AS INTEGER)),0),COALESCE(SUM(CAST(json_extract(payload_json,'$.usage.output_tokens') AS INTEGER)),0),COALESCE(SUM(CAST(json_extract(payload_json,'$.usage.unclassified_tokens') AS INTEGER)),0),COUNT(*),COALESCE(SUM(CASE WHEN json_extract(payload_json,'$.estimated_cost_microusd') IS NOT NULL THEN 1 ELSE 0 END),0),COALESCE(SUM(CASE WHEN json_extract(payload_json,'$.precision')='exact' THEN 1 ELSE 0 END),0),COALESCE(SUM(CASE WHEN json_extract(payload_json,'$.precision')='aggregate' THEN 1 ELSE 0 END),0),COALESCE(SUM(CASE WHEN json_extract(payload_json,'$.precision')='estimated' THEN 1 ELSE 0 END),0),COALESCE(SUM(CASE WHEN json_extract(payload_json,'$.precision')='unknown' THEN 1 ELSE 0 END),0),SUM(json_extract(payload_json,'$.estimated_cost_microusd')) FROM remote_usage_cache WHERE is_deleted=0 AND json_extract(payload_json,'$.scope')='device_local' AND (?1 IS NULL OR owner_device_id=?1) AND (?2 IS NULL OR json_extract(payload_json,'$.event_at_ms')>=?2) AND (?3 IS NULL OR json_extract(payload_json,'$.event_at_ms')<?3)",
            params![owner_device_id,start_ms,end_ms], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?,r.get(11)?,r.get(12)?))
        )?;
        let record_count=pos_u64(count)?; let priced_records=pos_u64(priced)?;
        Ok(RemoteUsageTotals { total_tokens:pos_u64(total)?,input_tokens:pos_u64(input)?,cache_read_tokens:pos_u64(cache_read)?,cache_write_tokens:pos_u64(cache_write)?,output_tokens:pos_u64(output)?,unclassified_tokens:pos_u64(unclassified)?,record_count,priced_records,unpriced_records:record_count.saturating_sub(priced_records),exact_records:pos_u64(exact)?,aggregate_records:pos_u64(aggregate)?,estimated_records:pos_u64(estimated)?,unknown_precision_records:pos_u64(unknown)?,estimated_cost_microusd:cost })
    }

    pub fn remote_usage_by_device(&self, start_ms: Option<i64>, end_ms: Option<i64>) -> Result<Vec<RemoteDeviceUsage>> {
        let mut stmt=self.conn.prepare("SELECT owner_device_id,COALESCE(SUM(CAST(json_extract(payload_json,'$.usage.input_uncached_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.cache_read_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.cache_write_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.output_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.unclassified_tokens') AS INTEGER)),0),COUNT(*),SUM(json_extract(payload_json,'$.estimated_cost_microusd')) FROM remote_usage_cache WHERE is_deleted=0 AND json_extract(payload_json,'$.scope')='device_local' AND (?1 IS NULL OR json_extract(payload_json,'$.event_at_ms')>=?1) AND (?2 IS NULL OR json_extract(payload_json,'$.event_at_ms')<?2) GROUP BY owner_device_id ORDER BY owner_device_id")?;
        let rows=stmt.query_map(params![start_ms,end_ms],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,Option<i64>>(3)?)))?;
        let mut out=Vec::new();
        for row in rows { let (id,total,count,cost)=row?; out.push(RemoteDeviceUsage{owner_device_id:id,total_tokens:pos_u64(total)?,record_count:pos_u64(count)?,estimated_cost_microusd:cost}); }
        Ok(out)
    }

    /// Remote breakdowns preserve device ownership. Only device-local usage is
    /// included; account-global and unknown-scope rows are never apportioned.
    pub fn remote_usage_breakdown(&self, dimension: &'static str, start_ms: Option<i64>, end_ms: Option<i64>) -> Result<Vec<RemoteUsageBreakdown>> {
        let json_path = match dimension {
            "tool_id" => "$.tool_id",
            "model_id" => "$.model_id",
            "project_id" => "$.project_id",
            _ => return Err(StoreError::InvalidInput("remote breakdown dimension")),
        };
        let sql = format!(
            "SELECT owner_device_id, json_extract(payload_json,'{json_path}'), COUNT(*), \
             COALESCE(SUM(CAST(json_extract(payload_json,'$.usage.input_uncached_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.cache_read_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.cache_write_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.output_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.unclassified_tokens') AS INTEGER)),0), \
             SUM(json_extract(payload_json,'$.estimated_cost_microusd')) \
             FROM remote_usage_cache WHERE is_deleted=0 AND json_extract(payload_json,'$.scope')='device_local' \
             AND (?1 IS NULL OR json_extract(payload_json,'$.event_at_ms')>=?1) \
             AND (?2 IS NULL OR json_extract(payload_json,'$.event_at_ms')<?2) \
             GROUP BY owner_device_id, json_extract(payload_json,'{json_path}') ORDER BY 4 DESC LIMIT 1001"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![start_ms, end_ms], |r| Ok((
            r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?, r.get::<_, Option<i64>>(4)?,
        )))?.collect::<rusqlite::Result<Vec<_>>>()?;
        if rows.len() > 1000 { return Err(StoreError::InvalidInput("remote breakdown cardinality exceeds 1000")); }
        let mut out = Vec::with_capacity(rows.len());
        for (owner, raw_key, count, total, cost) in rows {
            let key = raw_key.unwrap_or_else(|| match dimension {
                "tool_id" => "unknown-tool".into(),
                "model_id" => "unknown-model".into(),
                _ => "no-project".into(),
            });
            let label = if dimension == "project_id" {
                let short = key.strip_prefix("prj_").unwrap_or(&key).chars().take(8).collect::<String>();
                format!("项目 {short}")
            } else { key.clone() };
            out.push(RemoteUsageBreakdown {
                owner_device_id: owner, key, label, total_tokens: pos_u64(total)?,
                record_count: pos_u64(count)?, estimated_cost_microusd: cost,
            });
        }
        Ok(out)
    }

    pub fn get_record(&self, record_id: &str) -> Result<Option<RevisionedRecord>> {
        read_record(&self.conn, record_id)
    }

    /// Current records associated with one authoritative source. Used only for
    /// explicit full reconciliation and parser-version upgrades. Bounded so a
    /// corrupted/adversarial source cannot exhaust memory.
    pub fn records_for_source(&self, tool: &str, source_key: &str, limit: usize) -> Result<Vec<RevisionedRecord>> {
        if limit == 0 || limit > 2000 { return Err(StoreError::InvalidInput("source read limit")); }
        let mut stmt = self.conn.prepare(
            "SELECT record_id FROM record_sources WHERE tool_id=?1 AND source_key=?2 ORDER BY record_id LIMIT ?3"
        )?;
        let ids = stmt.query_map(params![tool, source_key, i64::try_from(limit + 1).map_err(|_| StoreError::IntegerOutOfRange)?],
            |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if ids.len() > limit { return Err(StoreError::InvalidInput("source exceeds reconciliation limit")); }
        let mut result = Vec::with_capacity(ids.len());
        for id in ids {
            result.push(self.get_record(&id)?.ok_or(StoreError::DatabaseInconsistent("source record missing"))?);
        }
        Ok(result)
    }

    /// Returns only due, non-quarantined outbox entries. Retry scheduling survives restarts.
    pub fn pending_outbox_due(&self, limit: usize, now_ms: i64) -> Result<Vec<OutboxEntry>> {
        if limit == 0 || limit > 200 || now_ms < 0 { return Err(StoreError::InvalidInput("outbox due query")); }
        let mut stmt = self.conn.prepare(
            "SELECT record_id,record_revision,enqueued_at_ms,attempts FROM sync_outbox \
             WHERE blocked_at_ms IS NULL AND next_attempt_at_ms <= ?1 \
             ORDER BY enqueued_at_ms,record_id LIMIT ?2"
        )?;
        let raw = stmt.query_map(params![now_ms, i64::try_from(limit).map_err(|_| StoreError::IntegerOutOfRange)?],
            |r| Ok((r.get::<_, String>(0)?,r.get::<_, i64>(1)?,r.get::<_, i64>(2)?,r.get::<_, i64>(3)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut entries = Vec::with_capacity(raw.len());
        for (record_id, rev, enqueued_at_ms, attempts) in raw {
            let envelope = self.get_record(&record_id)?.ok_or(StoreError::DatabaseInconsistent("outbox record missing"))?;
            if envelope.record_revision != pos_u64(rev)? { return Err(StoreError::DatabaseInconsistent("outbox revision does not match record")); }
            entries.push(OutboxEntry { envelope, enqueued_at_ms, attempts: pos_u64(attempts)? });
        }
        Ok(entries)
    }

    /// Persist exponential backoff: 5s, 10s, ... capped at one hour.
    pub fn schedule_outbox_retry(&mut self, record_id: &str, revision: u64, now_ms: i64, error_code: &str) -> Result<Option<i64>> {
        if revision == 0 || now_ms < 0 || error_code.is_empty() || error_code.len() > 128 || error_code.chars().any(char::is_control) {
            return Err(StoreError::InvalidInput("outbox retry"));
        }
        let attempts: Option<i64> = self.conn.query_row(
            "SELECT attempts FROM sync_outbox WHERE record_id=?1 AND record_revision=?2 AND blocked_at_ms IS NULL",
            params![record_id, db_int(revision)?], |r| r.get(0)
        ).optional()?;
        let Some(attempts) = attempts else { return Ok(None); };
        let exponent = attempts.clamp(0, 10) as u32;
        let delay_ms = 5_000_i64.saturating_mul(1_i64 << exponent).min(3_600_000);
        let next_at = now_ms.saturating_add(delay_ms);
        let changed = self.conn.execute(
            "UPDATE sync_outbox SET attempts=attempts+1,last_error=?3,next_attempt_at_ms=?4 \
             WHERE record_id=?1 AND record_revision=?2 AND blocked_at_ms IS NULL",
            params![record_id, db_int(revision)?, error_code, next_at]
        )?;
        Ok((changed == 1).then_some(next_at))
    }

    /// A superseded revision is quarantined until explicit remote reconciliation; never retry forever.
    pub fn block_outbox_revision(&mut self, record_id: &str, revision: u64, now_ms: i64, reason: &str) -> Result<bool> {
        if revision == 0 || now_ms < 0 || reason.is_empty() || reason.len() > 128 || reason.chars().any(char::is_control) {
            return Err(StoreError::InvalidInput("outbox block"));
        }
        let changed = self.conn.execute(
            "UPDATE sync_outbox SET blocked_at_ms=?3,last_error=?4 WHERE record_id=?1 AND record_revision=?2",
            params![record_id, db_int(revision)?, now_ms, reason]
        )?;
        Ok(changed == 1)
    }

    pub fn sync_outbox_status(&self, now_ms: i64) -> Result<SyncOutboxStatus> {
        let (pending,deferred,blocked,oldest,next): (i64,i64,i64,Option<i64>,Option<i64>) = self.conn.query_row(
            "SELECT \
             COALESCE(SUM(CASE WHEN blocked_at_ms IS NULL AND next_attempt_at_ms<=?1 THEN 1 ELSE 0 END),0),\
             COALESCE(SUM(CASE WHEN blocked_at_ms IS NULL AND next_attempt_at_ms>?1 THEN 1 ELSE 0 END),0),\
             COALESCE(SUM(CASE WHEN blocked_at_ms IS NOT NULL THEN 1 ELSE 0 END),0),\
             MIN(CASE WHEN blocked_at_ms IS NULL THEN enqueued_at_ms END),\
             MIN(CASE WHEN blocked_at_ms IS NULL AND next_attempt_at_ms>?1 THEN next_attempt_at_ms END) \
             FROM sync_outbox", [now_ms], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))
        )?;
        let (last_attempt,last_success,last_pull_seq,last_pull_ms,last_status,last_error): (Option<i64>,Option<i64>,i64,Option<i64>,String,Option<String>) = self.conn.query_row(
            "SELECT last_attempt_ms,last_success_ms,last_pull_seq,last_pull_ms,last_status,last_error FROM sync_state WHERE singleton=1", [],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))
        )?;
        Ok(SyncOutboxStatus { pending: pending.max(0) as u64, deferred: deferred.max(0) as u64, blocked: blocked.max(0) as u64,
            oldest_pending_at_ms: oldest, next_attempt_at_ms: next, last_attempt_ms: last_attempt, last_success_ms: last_success,
            last_pull_seq: pos_u64(last_pull_seq)?, last_pull_ms, last_status, last_error })
    }

    pub fn update_sync_state(&mut self, now_ms: i64, status: &str, error: Option<&str>) -> Result<()> {
        if now_ms < 0 || !["idle","success","deferred","blocked","auth_required","error"].contains(&status) {
            return Err(StoreError::InvalidInput("sync state"));
        }
        if error.is_some_and(|v| v.len() > 128 || v.chars().any(char::is_control)) { return Err(StoreError::InvalidInput("sync error")); }
        self.conn.execute(
            "UPDATE sync_state SET last_attempt_ms=?1,last_status=?2,last_error=?3, \
             last_success_ms=CASE WHEN ?2='success' THEN ?1 ELSE last_success_ms END WHERE singleton=1",
            params![now_ms,status,error]
        )?;
        Ok(())
    }

    /// Bounded reads: never copy the whole archive into memory for a push.
    pub fn pending_outbox(&self, limit: usize) -> Result<Vec<OutboxEntry>> {
        if limit == 0 || limit > 200 { return Err(StoreError::InvalidInput("outbox batch limit (1..200)")); }
        let mut stmt = self.conn.prepare(
            "SELECT record_id,record_revision,enqueued_at_ms,attempts FROM sync_outbox ORDER BY enqueued_at_ms,record_id LIMIT ?1"
        )?;
        let raw = stmt.query_map([i64::try_from(limit).map_err(|_| StoreError::IntegerOutOfRange)?],
            |r| Ok((r.get::<_, String>(0)?,r.get::<_, i64>(1)?,r.get::<_, i64>(2)?,r.get::<_, i64>(3)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut entries = Vec::with_capacity(raw.len());
        for (record_id, rev, enqueued_at_ms, attempts) in raw {
            let envelope = self.get_record(&record_id)?
                .ok_or(StoreError::DatabaseInconsistent("outbox record missing"))?;
            if envelope.record_revision != pos_u64(rev)? {
                return Err(StoreError::DatabaseInconsistent("outbox revision does not match record"));
            }
            entries.push(OutboxEntry { envelope, enqueued_at_ms, attempts: pos_u64(attempts)? });
        }
        Ok(entries)
    }

    /// Delete only on an authoritative Hub accepted/unchanged response. The
    /// network layer must NOT interpret `superseded` as a safe acknowledgement.
    pub fn ack_outbox_if_revision(&mut self, record_id: &str, revision: u64) -> Result<bool> {
        if revision == 0 { return Err(StoreError::InvalidInput("ack revision")); }
        let changed = self.conn.execute(
            "DELETE FROM sync_outbox WHERE record_id=?1 AND record_revision=?2",
            params![record_id, db_int(revision)?]
        )?;
        Ok(changed == 1)
    }

    pub fn note_outbox_failure(&mut self, record_id: &str, revision: u64, error_code: &str) -> Result<bool> {
        if error_code.is_empty() || error_code.len() > 128 || error_code.chars().any(char::is_control) {
            return Err(StoreError::InvalidInput("safe error code"));
        }
        let changed = self.conn.execute(
            "UPDATE sync_outbox SET attempts=attempts+1,last_error=?3 \
             WHERE record_id=?1 AND record_revision=?2 AND attempts < 9223372036854775807",
            params![record_id, db_int(revision)?, error_code]
        )?;
        Ok(changed == 1)
    }
}

#[derive(Clone, Copy, Debug)]
enum RecordChange { Inserted, Revised, Unchanged }

fn upsert_source_record(
    tx: &rusqlite::Transaction<'_>,
    rec: &CanonicalRecord,
    hash: &str,
    source_key: &str,
    owner: &str,
    observed_at_ms: i64,
) -> Result<RecordChange> {
    if rec.owner_device_id != owner { return Err(StoreError::IdentityMismatch); }
    let previous: Option<(String, String, String, i64)> = tx.query_row(
                "SELECT owner_device_id,tool_id,payload_hash,record_revision FROM usage_records WHERE record_id=?1",
                [&rec.record_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))
            ).optional()?;
    let change;
    match previous {
                Some((old_owner, old_tool, old_hash, revision)) => {
                    if old_owner != owner || old_tool != rec.tool_id {
                        return Err(StoreError::RecordOwnershipConflict);
                    }
                    let mut sources = tx.prepare("SELECT source_key FROM record_sources WHERE record_id=?1 AND tool_id=?2")?;
                    let found = sources.query_map(params![rec.record_id, rec.tool_id], |r| r.get::<_, String>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    if found.is_empty() { return Err(StoreError::DatabaseInconsistent("record lacks source provenance")); }
                    if old_hash != hash {
                        // Once multiple observers claim a logical record, require
                        // upstream arbitration. Never oscillate between values.
                        if found.len() != 1 || found[0] != source_key {
                            return Err(StoreError::SourceArbitrationConflict);
                        }
                        let newer = revision.checked_add(1).ok_or(StoreError::RevisionOverflow)?;
                        write_record(tx, rec, newer, hash, observed_at_ms)?;
                        enqueue(tx, &rec.record_id, newer, observed_at_ms)?;
                        change = RecordChange::Revised;
                    } else {
                        change = RecordChange::Unchanged;
                    }
                }
                None => {
                    write_record(tx, rec, 1, hash, observed_at_ms)?;
                    enqueue(tx, &rec.record_id, 1, observed_at_ms)?;
                    change = RecordChange::Inserted;
                }
            }
            tx.execute(
                "INSERT OR IGNORE INTO record_sources(record_id,tool_id,source_key) VALUES(?1,?2,?3)",
                params![rec.record_id, rec.tool_id, source_key]
            )?;
    Ok(change)
}

fn read_record(conn: &Connection, record_id: &str) -> Result<Option<RevisionedRecord>> {
        let raw = conn.query_row(
            "SELECT record_id,owner_device_id,tool_id,scope,account_id,model_id,project_id,session_id,event_at_ms, \
                input_uncached_tokens,cache_read_tokens,cache_write_tokens,output_tokens,unclassified_tokens, \
                reasoning_tokens,estimated_cost_microusd,price_version,precision,record_revision,payload_hash,is_deleted \
             FROM usage_records WHERE record_id=?1",
            [record_id], RawRecord::read
        ).optional()?;
        raw.map(RawRecord::into_envelope).transpose()
}

fn load_checkpoint(conn: &Connection, tool: &str, key: &str) -> Result<Option<SourceCheckpoint>> {
    let stored = conn.query_row(
        "SELECT source_fingerprint,read_offset,parser_version,last_scan_ms,last_complete_scan_ms,last_status \
         FROM source_checkpoints WHERE tool_id=?1 AND source_key=?2", params![tool,key],
        |r| Ok((r.get::<_, String>(0)?,r.get::<_, i64>(1)?,r.get::<_, i64>(2)?,
                 r.get::<_, i64>(3)?,r.get::<_, Option<i64>>(4)?,r.get::<_, String>(5)?))
    ).optional()?;
    match stored {
        Some((source_fingerprint, offset, parser, last_scan_ms, last_complete_scan_ms, last_status)) => {
            let parser_version = u32::try_from(parser).map_err(|_| StoreError::DatabaseInconsistent("invalid parser version"))?;
            Ok(Some(SourceCheckpoint {tool_id: tool.to_string(),source_key:key.to_string(),source_fingerprint,
                read_offset:pos_u64(offset)?,parser_version,last_scan_ms,last_complete_scan_ms,last_status}))
        }
        None => Ok(None)
    }
}

fn enqueue(tx: &rusqlite::Transaction<'_>, id: &str, rev: i64, now: i64) -> Result<()> {
    tx.execute("INSERT INTO sync_outbox(record_id,record_revision,enqueued_at_ms) VALUES(?1,?2,?3) \
                ON CONFLICT(record_id) DO UPDATE SET record_revision=excluded.record_revision, \
                enqueued_at_ms=excluded.enqueued_at_ms,attempts=0,last_error=NULL,next_attempt_at_ms=0,blocked_at_ms=NULL",
        params![id,rev,now])?;
    Ok(())
}

fn write_record(tx: &rusqlite::Transaction<'_>, rec: &CanonicalRecord, rev: i64, hash: &str, now: i64) -> Result<()> {
    let scope = match rec.scope { UsageScope::DeviceLocal => "device_local", UsageScope::AccountGlobal => "account_global", UsageScope::Unknown => "unknown" };
    let precision = match rec.precision { Precision::Exact => "exact", Precision::Aggregate => "aggregate", Precision::Estimated => "estimated", Precision::Unknown => "unknown" };
    tx.execute(
        "INSERT INTO usage_records( \
            record_id,owner_device_id,tool_id,scope,account_id,model_id,project_id,session_id,event_at_ms, \
            input_uncached_tokens,cache_read_tokens,cache_write_tokens,output_tokens,unclassified_tokens,reasoning_tokens, \
            estimated_cost_microusd,price_version,precision,record_revision,payload_hash,is_deleted,observed_at_ms,updated_at_ms \
        ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23) \
        ON CONFLICT(record_id) DO UPDATE SET \
            owner_device_id=excluded.owner_device_id,tool_id=excluded.tool_id,scope=excluded.scope, \
            account_id=excluded.account_id,model_id=excluded.model_id,project_id=excluded.project_id,session_id=excluded.session_id, \
            event_at_ms=excluded.event_at_ms,input_uncached_tokens=excluded.input_uncached_tokens, \
            cache_read_tokens=excluded.cache_read_tokens,cache_write_tokens=excluded.cache_write_tokens, \
            output_tokens=excluded.output_tokens,unclassified_tokens=excluded.unclassified_tokens, \
            reasoning_tokens=excluded.reasoning_tokens,estimated_cost_microusd=excluded.estimated_cost_microusd, \
            price_version=excluded.price_version,precision=excluded.precision,record_revision=excluded.record_revision, \
            payload_hash=excluded.payload_hash,is_deleted=excluded.is_deleted,updated_at_ms=excluded.updated_at_ms",
        params![rec.record_id,rec.owner_device_id,rec.tool_id,scope,rec.account_id,rec.model_id,rec.project_id,
                rec.session_id,rec.event_at_ms,db_int(rec.usage.input_uncached_tokens)?,db_int(rec.usage.cache_read_tokens)?,
                db_int(rec.usage.cache_write_tokens)?,db_int(rec.usage.output_tokens)?,db_int(rec.usage.unclassified_tokens)?,
                rec.usage.reasoning_tokens.map(db_int).transpose()?,rec.estimated_cost_microusd,rec.price_version,
                precision,rev,hash,rec.is_deleted,now,now]
    )?;
    Ok(())
}

struct RawRecord {
    record_id: String, owner_device_id: String, tool_id: String, scope: String,
    account_id: Option<String>, model_id: Option<String>, project_id: Option<String>, session_id: Option<String>,
    event_at_ms: Option<i64>, input: i64, cache_read: i64, cache_write: i64, output: i64,
    unclassified: i64, reasoning: Option<i64>, cost: Option<i64>, price: Option<String>,
    precision: String, revision: i64, hash: String, deleted: i64,
}
impl RawRecord {
    fn read(r: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            record_id:r.get(0)?,owner_device_id:r.get(1)?,tool_id:r.get(2)?,scope:r.get(3)?,
            account_id:r.get(4)?,model_id:r.get(5)?,project_id:r.get(6)?,session_id:r.get(7)?,
            event_at_ms:r.get(8)?,input:r.get(9)?,cache_read:r.get(10)?,cache_write:r.get(11)?,
            output:r.get(12)?,unclassified:r.get(13)?,reasoning:r.get(14)?,cost:r.get(15)?,
            price:r.get(16)?,precision:r.get(17)?,revision:r.get(18)?,hash:r.get(19)?,deleted:r.get(20)?
        })
    }
    fn into_envelope(self) -> Result<RevisionedRecord> {
        let scope = match self.scope.as_str() {
            "device_local" => UsageScope::DeviceLocal, "account_global" => UsageScope::AccountGlobal,
            "unknown" => UsageScope::Unknown, _ => return Err(StoreError::DatabaseInconsistent("scope")),
        };
        let precision = match self.precision.as_str() {
            "exact" => Precision::Exact, "aggregate" => Precision::Aggregate,
            "estimated" => Precision::Estimated, "unknown" => Precision::Unknown,
            _ => return Err(StoreError::DatabaseInconsistent("precision")),
        };
        let rec = CanonicalRecord {
            record_id:self.record_id,owner_device_id:self.owner_device_id,tool_id:self.tool_id,
            scope,precision,account_id:self.account_id,model_id:self.model_id,
            project_id:self.project_id,session_id:self.session_id,event_at_ms:self.event_at_ms,
            usage:TokenBreakdown {
                input_uncached_tokens:pos_u64(self.input)?,cache_read_tokens:pos_u64(self.cache_read)?,
                cache_write_tokens:pos_u64(self.cache_write)?,output_tokens:pos_u64(self.output)?,
                unclassified_tokens:pos_u64(self.unclassified)?,reasoning_tokens:self.reasoning.map(pos_u64).transpose()?
            },
            estimated_cost_microusd:self.cost,price_version:self.price,is_deleted:match self.deleted {
                0 => false,1 => true,_ => return Err(StoreError::DatabaseInconsistent("is_deleted")),
            }
        };
        let envelope = RevisionedRecord {record:rec,record_revision:pos_u64(self.revision)?,payload_hash:self.hash};
        envelope.validate()?;
        Ok(envelope)
    }
}

#[cfg(test)]
mod dashboard_tests {
    use super::*;
    use tokenscope_core_domain::{CanonicalRecord, Precision, TokenBreakdown, UsageScope};

    fn record(id: char, scope: UsageScope, event_at_ms: Option<i64>) -> CanonicalRecord {
        CanonicalRecord {
            record_id: id.to_string().repeat(64),
            owner_device_id: "device-a".into(),
            tool_id: "codex".into(),
            scope,
            precision: Precision::Aggregate,
            account_id: if scope == UsageScope::AccountGlobal { Some(format!("acct_{}", "a".repeat(64))) } else { None },
            model_id: Some("gpt-5".into()),
            project_id: Some(format!("prj_{}", "b".repeat(64))),
            session_id: None,
            event_at_ms,
            usage: TokenBreakdown {
                input_uncached_tokens: 100,
                cache_read_tokens: 10,
                cache_write_tokens: 5,
                output_tokens: 50,
                unclassified_tokens: 0,
                reasoning_tokens: Some(20),
            },
            estimated_cost_microusd: Some(1250),
            price_version: Some("test-price-v1".into()),
            is_deleted: false,
        }
    }

    #[test]
    fn dashboard_keeps_scope_and_reasoning_semantics_separate() {
        let mut store = LocalStore::open_in_memory().unwrap();
        store.init_identity(&LocalIdentity {
            device_id: "device-a".into(), display_name: "Office PC".into(), time_zone: "Asia/Shanghai".into(),
        }).unwrap();
        let checkpoint = SourceCheckpoint {
            tool_id: "codex".into(), source_key: "test-source".into(),
            source_fingerprint: "fingerprint-v1".into(), read_offset: 100,
            parser_version: 1, last_scan_ms: 2_000, last_complete_scan_ms: Some(2_000),
            last_status: "complete".into(),
        };
        store.apply_scan(None, &checkpoint, &[
            record('a', UsageScope::DeviceLocal, Some(1_500)),
            record('b', UsageScope::Unknown, Some(1_500)),
            record('c', UsageScope::AccountGlobal, Some(1_500)),
        ], 2_000, false).unwrap();

        let snapshot = store.dashboard_snapshot(
            "7d", Some(1_000), Some(3_000), &[("2026-11-02".into(), 1_000, 3_000)],
            2_000, "Asia/Shanghai".into(), true,
        ).unwrap();
        assert_eq!(snapshot.local.total_tokens, 165);
        assert_eq!(snapshot.local.reasoning_tokens, 20);
        assert_eq!(snapshot.local.record_count, 1);
        assert_eq!(snapshot.local.estimated_cost_microusd, Some(1250));
        assert_eq!(snapshot.unassigned.total_tokens, 165);
        assert_eq!(snapshot.account_global.total_tokens, 165);
        assert_eq!(snapshot.by_tool.len(), 1);
        assert_eq!(snapshot.by_model[0].label, "gpt-5");
        assert_eq!(snapshot.daily[0].total_tokens, 165);
    }

    #[test]
    fn dashboard_excludes_deleted_and_untimed_rows_from_calendar_buckets() {
        let mut store = LocalStore::open_in_memory().unwrap();
        store.init_identity(&LocalIdentity {
            device_id: "device-a".into(), display_name: "Office PC".into(), time_zone: "UTC".into(),
        }).unwrap();
        let checkpoint = SourceCheckpoint {
            tool_id: "codex".into(), source_key: "test-source".into(),
            source_fingerprint: "fingerprint-v1".into(), read_offset: 100,
            parser_version: 1, last_scan_ms: 2_000, last_complete_scan_ms: Some(2_000),
            last_status: "complete".into(),
        };
        store.apply_scan(None, &checkpoint, &[
            record('a', UsageScope::DeviceLocal, Some(1_500)),
            record('b', UsageScope::DeviceLocal, None),
        ], 2_000, false).unwrap();
        let snapshot = store.dashboard_snapshot(
            "all", None, None, &[("1970-01-01".into(), 1_000, 3_000)],
            2_000, "UTC".into(), true,
        ).unwrap();
        assert_eq!(snapshot.local.total_tokens, 330);
        assert_eq!(snapshot.daily[0].total_tokens, 165);
    }
}

#[cfg(test)]
mod readonly_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn readonly_open_does_not_migrate_and_rejects_mutations() {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("tokenscope-readonly-{}-{nonce}.db", std::process::id()));
        {
            let mut writer = LocalStore::open(&path).unwrap();
            writer.init_identity(&LocalIdentity {
                device_id: "readonly-test".into(), display_name: "Test".into(), time_zone: "UTC".into(),
            }).unwrap();
        }
        {
            let mut reader = LocalStore::open_readonly(&path).unwrap();
            assert_eq!(reader.schema_version().unwrap(), schema::VERSION);
            let quota = AntigravityQuota {
                source_id: "local".into(), pool_id: "gemini".into(), window_id: "weekly".into(),
                remaining_basis_points: Some(5000), reset_at_ms: None, observed_at_ms: 100,
            };
            assert!(reader.upsert_antigravity_quotas(&[quota], 100).is_err());
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }
}
