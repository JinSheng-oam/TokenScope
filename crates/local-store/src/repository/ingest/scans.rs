use super::super::models::{AntigravityActivity, ApplyReport, ReconcileReport, SourceCheckpoint};
use super::super::records::{
    load_checkpoint, load_suppressed_sessions, read_record, upsert_source_record, RecordChange,
};
use super::super::LocalStore;
use crate::error::{db_int, pos_u64, Result, StoreError};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use std::collections::HashSet;
use tokenscope_core_domain::{payload_hash, CanonicalRecord, TokenBreakdown};
impl LocalStore {
    pub fn checkpoint(&self, tool_id: &str, source_key: &str) -> Result<Option<SourceCheckpoint>> {
        load_checkpoint(&self.conn, tool_id, source_key)
    }

    pub fn source_checkpoints(&self) -> Result<Vec<SourceCheckpoint>> {
        let mut stmt = self.conn.prepare(
            "SELECT tool_id,source_key,source_fingerprint,read_offset,parser_version,last_scan_ms,last_complete_scan_ms,last_status \
             FROM source_checkpoints ORDER BY tool_id,last_scan_ms DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, String>(7)?,
            ))
        })?;
        let mut checkpoints = Vec::new();
        for row in rows {
            let (
                tool_id,
                source_key,
                source_fingerprint,
                offset,
                parser,
                last_scan_ms,
                last_complete_scan_ms,
                last_status,
            ) = row?;
            let parser_version = u32::try_from(parser)
                .map_err(|_| StoreError::DatabaseInconsistent("invalid parser version"))?;
            checkpoints.push(SourceCheckpoint {
                tool_id,
                source_key,
                source_fingerprint,
                read_offset: pos_u64(offset)?,
                parser_version,
                last_scan_ms,
                last_complete_scan_ms,
                last_status,
            });
        }
        Ok(checkpoints)
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
        if proposed.len() > 2000 {
            return Err(StoreError::InvalidInput("scan batch limit (2000)"));
        }
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
                return Err(StoreError::InvalidInput(
                    "degraded scan cannot publish records",
                ));
            }
            // A failed read may update health, but it must not advance the
            // last-known-good byte cursor, parser or source fingerprint.
            if let Some(old) = expected {
                if next.read_offset != old.read_offset
                    || next.source_fingerprint != old.source_fingerprint
                    || next.parser_version != old.parser_version
                    || next.last_complete_scan_ms != old.last_complete_scan_ms
                {
                    return Err(StoreError::InvalidInput(
                        "degraded checkpoint advances cursor",
                    ));
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
            if rec.tool_id != next.tool_id {
                return Err(StoreError::InvalidInput("tool mismatch"));
            }
            if rec.is_deleted && !allow_explicit_tombstones {
                return Err(StoreError::InvalidInput(
                    "tombstones require explicit authorization",
                ));
            }
            if !seen.insert(rec.record_id.clone()) {
                return Err(StoreError::DuplicateBatchRecord);
            }
            // SQLite stores signed 64-bit counters even though the domain accepts u64.
            for n in [
                rec.usage.input_uncached_tokens,
                rec.usage.cache_read_tokens,
                rec.usage.cache_write_tokens,
                rec.usage.output_tokens,
                rec.usage.unclassified_tokens,
            ] {
                db_int(n)?;
            }
            if let Some(n) = rec.usage.reasoning_tokens {
                db_int(n)?;
            }
            canonical.push((payload_hash(&rec)?, rec));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_checkpoint(&tx, &next.tool_id, &next.source_key)?;
        if current.as_ref() != expected {
            return Err(StoreError::CheckpointConflict);
        }
        let owner: Option<String> = tx
            .query_row(
                "SELECT device_id FROM local_identity WHERE singleton=1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let owner = owner.ok_or(StoreError::NotInitialized)?;
        let suppressed = load_suppressed_sessions(&tx, &next.tool_id)?;
        let suppress_source = suppressed.contains(&next.source_key);
        let mut report = ApplyReport::default();
        for (hash, rec) in canonical {
            if rec.owner_device_id != owner {
                return Err(StoreError::IdentityMismatch);
            }
            if suppress_source
                || rec
                    .session_id
                    .as_ref()
                    .is_some_and(|session_id| suppressed.contains(session_id))
            {
                continue;
            }
            match upsert_source_record(&tx, &rec, &hash, &next.source_key, &owner, observed_at_ms)?
            {
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
        tx.execute(
            "DELETE FROM missing_source_candidates WHERE tool_id=?1 AND source_key=?2",
            params![next.tool_id, next.source_key],
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
        self.reconcile_source_impl(expected, next, records, observed_at_ms, activity, None)
    }

    pub fn reconcile_complete_session_source<I>(
        &mut self,
        expected: Option<&SourceCheckpoint>,
        next: &SourceCheckpoint,
        records: I,
        observed_at_ms: i64,
    ) -> Result<ReconcileReport>
    where
        I: IntoIterator<Item = CanonicalRecord>,
    {
        self.reconcile_source_impl(
            expected,
            next,
            records,
            observed_at_ms,
            None,
            Some(&next.source_key),
        )
    }

    fn reconcile_source_impl<I>(
        &mut self,
        expected: Option<&SourceCheckpoint>,
        next: &SourceCheckpoint,
        records: I,
        observed_at_ms: i64,
        activity: Option<&AntigravityActivity>,
        retire_session: Option<&str>,
    ) -> Result<ReconcileReport>
    where
        I: IntoIterator<Item = CanonicalRecord>,
    {
        next.validate()?;
        if let Some(a) = activity {
            a.validate()?;
            if next.tool_id != "antigravity" {
                return Err(StoreError::InvalidInput("activity tool"));
            }
        }
        if next.last_status != "complete" || next.last_complete_scan_ms != Some(next.last_scan_ms) {
            return Err(StoreError::InvalidInput(
                "complete reconciliation requires full scan",
            ));
        }
        if let Some(old) = expected {
            old.validate()?;
            if old.tool_id != next.tool_id
                || old.source_key != next.source_key
                || next.last_scan_ms < old.last_scan_ms
            {
                return Err(StoreError::InvalidInput("checkpoint identity/time"));
            }
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let actual = load_checkpoint(&tx, &next.tool_id, &next.source_key)?;
        if actual.as_ref() != expected {
            return Err(StoreError::CheckpointConflict);
        }
        let owner: String = tx
            .query_row(
                "SELECT device_id FROM local_identity WHERE singleton=1",
                [],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(StoreError::NotInitialized)?;
        let suppressed = load_suppressed_sessions(&tx, &next.tool_id)?;
        let mut suppress_source = suppressed.contains(&next.source_key);
        if let Some(session) = retire_session {
            let legacy_deleted: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM record_sources rs JOIN usage_records u ON u.record_id=rs.record_id JOIN session_suppressions s ON s.tool_id=rs.tool_id AND s.session_id=rs.source_key WHERE rs.tool_id=?1 AND u.session_id=?2)",
                params![next.tool_id,session],|r|r.get(0))?;
            if legacy_deleted {
                suppress_source = true;
                tx.execute("INSERT OR IGNORE INTO session_suppressions(tool_id,session_id,deleted_at_ms) VALUES(?1,?2,?3)",params![next.tool_id,session,observed_at_ms])?;
            }
        }
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
            if rec.owner_device_id != owner {
                return Err(StoreError::IdentityMismatch);
            }
            for n in [
                rec.usage.input_uncached_tokens,
                rec.usage.cache_read_tokens,
                rec.usage.cache_write_tokens,
                rec.usage.output_tokens,
                rec.usage.unclassified_tokens,
            ] {
                db_int(n)?;
            }
            if let Some(n) = rec.usage.reasoning_tokens {
                db_int(n)?;
            }
            if suppress_source
                || rec
                    .session_id
                    .as_ref()
                    .is_some_and(|session_id| suppressed.contains(session_id))
            {
                continue;
            }
            let changed = tx.execute(
                "INSERT OR IGNORE INTO temp.tokenscope_seen(record_id) VALUES (?1)",
                [&rec.record_id],
            )?;
            if changed != 1 {
                return Err(StoreError::DuplicateBatchRecord);
            }
            let hash = payload_hash(&rec)?;
            match upsert_source_record(&tx, &rec, &hash, &next.source_key, &owner, observed_at_ms)?
            {
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
            let Some((rowid, record_id)) = missing else {
                break;
            };
            last_rowid = rowid;
            let source_count: i64 = tx.query_row(
                "SELECT COUNT(*) FROM record_sources WHERE record_id=?1",
                [&record_id],
                |r| r.get(0),
            )?;
            if source_count != 1 {
                return Err(StoreError::SourceArbitrationConflict);
            }
            let mut record = read_record(&tx, &record_id)?
                .ok_or(StoreError::DatabaseInconsistent("source record missing"))?
                .record;
            record.is_deleted = true;
            record.usage = TokenBreakdown::default();
            record.estimated_cost_microusd = None;
            record.price_version = None;
            let hash = payload_hash(&record)?;
            match upsert_source_record(
                &tx,
                &record,
                &hash,
                &next.source_key,
                &owner,
                observed_at_ms,
            )? {
                RecordChange::Revised => report.tombstoned += 1,
                _ => {
                    return Err(StoreError::DatabaseInconsistent(
                        "unexpected tombstone result",
                    ))
                }
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
        tx.execute(
            "DELETE FROM missing_source_candidates WHERE tool_id=?1 AND source_key=?2",
            params![next.tool_id, next.source_key],
        )?;
        tx.execute(
            "DELETE FROM empty_source_candidates WHERE tool_id=?1 AND source_key=?2",
            params![next.tool_id, next.source_key],
        )?;
        if let Some(a) = activity {
            tx.execute("INSERT INTO antigravity_activity_sources(source_key,known_turns,unknown_turns,observed_at_ms) \
                VALUES (?1,?2,?3,?4) ON CONFLICT(source_key) DO UPDATE SET \
                known_turns=excluded.known_turns,unknown_turns=excluded.unknown_turns,observed_at_ms=excluded.observed_at_ms",
                params![next.source_key, db_int(a.known_turns)?, db_int(a.unknown_turns)?, observed_at_ms])?;
            tx.execute(
                "DELETE FROM antigravity_model_activity WHERE source_key=?1",
                [&next.source_key],
            )?;
            for (model, count) in &a.models {
                tx.execute("INSERT INTO antigravity_model_activity(source_key,model_id,response_count) VALUES(?1,?2,?3)",
                    params![next.source_key,model,db_int(*count)?])?;
            }
        }
        if let Some(session) = retire_session {
            let keys = {
                let mut stmt = tx.prepare("SELECT DISTINCT rs.source_key FROM record_sources rs JOIN usage_records u ON u.record_id=rs.record_id WHERE rs.tool_id=?1 AND u.session_id=?2 AND rs.source_key<>?3")?;
                let rows = stmt
                    .query_map(params![next.tool_id, session, next.source_key], |r| {
                        r.get::<_, String>(0)
                    })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            for key in keys {
                report.tombstoned += super::super::records::tombstone_source_records(
                    &tx,
                    &next.tool_id,
                    &key,
                    observed_at_ms,
                )?;
                tx.execute(
                    "DELETE FROM source_checkpoints WHERE tool_id=?1 AND source_key=?2",
                    params![next.tool_id, key],
                )?;
            }
        }
        // Temp seen keys are erased inside the same transaction. In-memory
        // capacity remains bounded by a record and a page cursor.
        tx.execute("DELETE FROM temp.tokenscope_seen", [])?;
        tx.commit()?;
        Ok(report)
    }
}
