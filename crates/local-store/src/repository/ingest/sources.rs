use super::super::models::{AntigravityActivity, ReconcileReport, SourceCheckpoint};
use super::super::records::{
    load_checkpoint, tombstone_source_records, validate_source_key, validate_tool_id,
};
use super::super::LocalStore;
use crate::error::{Result, StoreError};
use rusqlite::{params, TransactionBehavior};
use std::collections::HashSet;
impl LocalStore {
    pub fn reconcile_missing_sources(
        &mut self,
        tool_id: &str,
        present_source_keys: &HashSet<String>,
        observed_at_ms: i64,
    ) -> Result<usize> {
        validate_tool_id(tool_id)?;
        if observed_at_ms < 0 {
            return Err(StoreError::InvalidInput("source inventory time"));
        }
        for source_key in present_source_keys {
            validate_source_key(source_key)?;
        }
        // Antigravity conversation databases are local snapshots with no
        // deletion contract. Losing a file must not erase stored activity or
        // previously observed token usage.
        if tool_id == "antigravity" {
            return Ok(0);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS tokenscope_present_sources (source_key TEXT PRIMARY KEY) WITHOUT ROWID; \
             DELETE FROM temp.tokenscope_present_sources;",
        )?;
        {
            let mut insert = tx.prepare(
                "INSERT OR IGNORE INTO temp.tokenscope_present_sources(source_key) VALUES(?1)",
            )?;
            for source_key in present_source_keys {
                insert.execute([source_key])?;
            }
        }
        tx.execute(
            "DELETE FROM missing_source_candidates WHERE tool_id=?1 \
             AND source_key IN (SELECT source_key FROM temp.tokenscope_present_sources)",
            [tool_id],
        )?;
        let absent = {
            let mut stmt = tx.prepare(
                "SELECT cp.source_key FROM source_checkpoints cp \
                 WHERE cp.tool_id=?1 AND cp.last_status='complete' \
                   AND cp.last_complete_scan_ms=cp.last_scan_ms \
                   AND NOT EXISTS (SELECT 1 FROM temp.tokenscope_present_sources p WHERE p.source_key=cp.source_key) \
                 ORDER BY cp.source_key",
            )?;
            let rows = stmt.query_map([tool_id], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut tombstoned_records = 0usize;
        for source_key in absent {
            tx.execute(
                "INSERT INTO missing_source_candidates(tool_id,source_key,first_missing_at_ms,last_missing_at_ms,scan_count,tombstoned_at_ms) \
                 VALUES(?1,?2,?3,?3,1,NULL) \
                 ON CONFLICT(tool_id,source_key) DO UPDATE SET \
                   last_missing_at_ms=MAX(missing_source_candidates.last_missing_at_ms,excluded.last_missing_at_ms), \
                   scan_count=missing_source_candidates.scan_count+1 \
                 WHERE missing_source_candidates.tombstoned_at_ms IS NULL",
                params![tool_id, source_key, observed_at_ms],
            )?;
            let (first_missing, scan_count, tombstoned_at): (i64, i64, Option<i64>) = tx
                .query_row(
                    "SELECT first_missing_at_ms,scan_count,tombstoned_at_ms \
                 FROM missing_source_candidates WHERE tool_id=?1 AND source_key=?2",
                    params![tool_id, source_key],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?;
            if tombstoned_at.is_some()
                || scan_count < 2
                || observed_at_ms.saturating_sub(first_missing) < 30_000
            {
                continue;
            }
            tombstoned_records +=
                tombstone_source_records(&tx, tool_id, &source_key, observed_at_ms)?;
            tx.execute(
                "DELETE FROM source_checkpoints WHERE tool_id=?1 AND source_key=?2",
                params![tool_id, source_key],
            )?;
            tx.execute(
                "UPDATE missing_source_candidates SET tombstoned_at_ms=?3 \
                 WHERE tool_id=?1 AND source_key=?2 AND tombstoned_at_ms IS NULL",
                params![tool_id, source_key, observed_at_ms],
            )?;
        }
        tx.execute("DELETE FROM temp.tokenscope_present_sources", [])?;
        tx.commit()?;
        Ok(tombstoned_records)
    }

    /// Requires two complete empty Antigravity snapshots at least 30 seconds apart
    /// before removing a conversation's previously counted usage.
    pub fn reconcile_confirmed_empty_source(
        &mut self,
        expected: &SourceCheckpoint,
        next: &SourceCheckpoint,
        activity: &AntigravityActivity,
        observed_at_ms: i64,
    ) -> Result<Option<ReconcileReport>> {
        expected.validate()?;
        next.validate()?;
        activity.validate()?;
        if expected.tool_id != "antigravity"
            || next.tool_id != "antigravity"
            || expected.source_key != next.source_key
            || next.last_status != "complete"
            || next.last_complete_scan_ms != Some(next.last_scan_ms)
            || observed_at_ms < 0
        {
            return Err(StoreError::InvalidInput("empty source reconciliation"));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if load_checkpoint(&tx, &expected.tool_id, &expected.source_key)?.as_ref() != Some(expected)
        {
            return Err(StoreError::CheckpointConflict);
        }
        tx.execute(
            "INSERT INTO empty_source_candidates(tool_id,source_key,first_empty_at_ms,last_empty_at_ms,scan_count) \
             VALUES(?1,?2,?3,?3,1) \
             ON CONFLICT(tool_id,source_key) DO UPDATE SET \
               last_empty_at_ms=MAX(empty_source_candidates.last_empty_at_ms,excluded.last_empty_at_ms), \
               scan_count=empty_source_candidates.scan_count+1",
            params![next.tool_id, next.source_key, observed_at_ms],
        )?;
        let (first_empty, scan_count): (i64, i64) = tx.query_row(
            "SELECT first_empty_at_ms,scan_count FROM empty_source_candidates \
             WHERE tool_id=?1 AND source_key=?2",
            params![next.tool_id, next.source_key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let confirmed = scan_count >= 2 && observed_at_ms.saturating_sub(first_empty) >= 30_000;
        tx.commit()?;
        if !confirmed {
            return Ok(None);
        }
        self.reconcile_source_with_activity(
            Some(expected),
            next,
            std::iter::empty(),
            observed_at_ms,
            Some(activity),
        )
        .map(Some)
    }

    pub fn clear_empty_source_candidate(&mut self, tool_id: &str, source_key: &str) -> Result<()> {
        validate_tool_id(tool_id)?;
        validate_source_key(source_key)?;
        self.conn.execute(
            "DELETE FROM empty_source_candidates WHERE tool_id=?1 AND source_key=?2",
            params![tool_id, source_key],
        )?;
        Ok(())
    }
}
