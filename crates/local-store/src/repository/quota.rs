use super::models::{AntigravityQuota, CodexQuota};
use super::LocalStore;
use crate::error::{Result, StoreError};
use rusqlite::{params, TransactionBehavior};
use std::collections::HashSet;
impl LocalStore {
    /// Atomic import of a bounded list of manually observed quota snapshots.
    /// This never touches Token usage, revision/outbox, or OAuth credentials.
    pub fn upsert_antigravity_quotas(
        &mut self,
        batch: &[AntigravityQuota],
        received_at_ms: i64,
    ) -> Result<usize> {
        if batch.is_empty() || batch.len() > 200 || received_at_ms < 0 {
            return Err(StoreError::InvalidInput("quota batch size/time"));
        }
        let mut seen = HashSet::with_capacity(batch.len());
        for row in batch {
            row.validate()?;
            if row.observed_at_ms > received_at_ms.saturating_add(60_000) {
                return Err(StoreError::InvalidInput("quota future observation"));
            }
            if !seen.insert((&row.source_id, &row.pool_id, &row.window_id)) {
                return Err(StoreError::InvalidInput("duplicate quota identity"));
            }
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
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
        if limit == 0 || limit > 200 {
            return Err(StoreError::InvalidInput("quota read limit"));
        }
        let mut stmt = self.conn.prepare(
            "SELECT source_id,pool_id,window_id,remaining_basis_points,reset_at_ms,observed_at_ms \
             FROM antigravity_quota_snapshots ORDER BY source_id,pool_id,window_id LIMIT ?1",
        )?;
        let iter = stmt.query_map(
            [i64::try_from(limit).map_err(|_| StoreError::IntegerOutOfRange)?],
            |r| {
                let fraction: Option<i64> = r.get(3)?;
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    fraction,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            },
        )?;
        iter.map(|entry| {
            let (source_id, pool_id, window_id, remaining, reset_at_ms, observed_at_ms) = entry?;
            let remaining_basis_points = remaining
                .map(|n| u16::try_from(n).map_err(|_| StoreError::IntegerOutOfRange))
                .transpose()?;
            Ok(AntigravityQuota {
                source_id,
                pool_id,
                window_id,
                remaining_basis_points,
                reset_at_ms,
                observed_at_ms,
            })
        })
        .collect::<Result<Vec<_>>>()
    }

    /// Atomically persist a complete Codex quota observation. Older snapshots
    /// cannot replace newer values, and no usage/outbox rows are touched.
    pub fn upsert_codex_quotas(
        &mut self,
        batch: &[CodexQuota],
        received_at_ms: i64,
    ) -> Result<usize> {
        if batch.is_empty() || batch.len() > 32 || received_at_ms < 0 {
            return Err(StoreError::InvalidInput("Codex quota batch size/time"));
        }
        let mut seen = HashSet::with_capacity(batch.len());
        for row in batch {
            row.validate()?;
            if row.observed_at_ms > received_at_ms.saturating_add(60_000)
                || !seen.insert((&row.source_id, &row.pool_id, row.window_duration_minutes))
            {
                return Err(StoreError::InvalidInput("Codex quota observation/identity"));
            }
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut changed = 0;
        for row in batch {
            changed += tx.execute(
                "INSERT INTO codex_quota_snapshots(source_id,pool_id,window_duration_minutes,remaining_basis_points,reset_at_ms,observed_at_ms,received_at_ms) \
                 VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(source_id,pool_id,window_duration_minutes) DO UPDATE SET \
                 remaining_basis_points=excluded.remaining_basis_points,reset_at_ms=excluded.reset_at_ms, \
                 observed_at_ms=excluded.observed_at_ms,received_at_ms=excluded.received_at_ms \
                 WHERE excluded.observed_at_ms > codex_quota_snapshots.observed_at_ms",
                params![row.source_id,row.pool_id,i64::from(row.window_duration_minutes),
                    row.remaining_basis_points.map(i64::from),row.reset_at_ms,row.observed_at_ms,received_at_ms],
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }

    pub fn codex_quota_snapshots(&self, limit: usize) -> Result<Vec<CodexQuota>> {
        if limit == 0 || limit > 32 {
            return Err(StoreError::InvalidInput("Codex quota read limit"));
        }
        let mut stmt = self.conn.prepare(
            "SELECT source_id,pool_id,window_duration_minutes,remaining_basis_points,reset_at_ms,observed_at_ms \
             FROM codex_quota_snapshots ORDER BY pool_id,window_duration_minutes LIMIT ?1")?;
        let rows = stmt.query_map(
            [i64::try_from(limit).map_err(|_| StoreError::IntegerOutOfRange)?],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            },
        )?;
        rows.map(|row| {
            let (source_id, pool_id, duration, remaining, reset_at_ms, observed_at_ms) = row?;
            Ok(CodexQuota {
                source_id,
                pool_id,
                window_duration_minutes: u32::try_from(duration)
                    .map_err(|_| StoreError::IntegerOutOfRange)?,
                remaining_basis_points: remaining
                    .map(|n| u16::try_from(n).map_err(|_| StoreError::IntegerOutOfRange))
                    .transpose()?,
                reset_at_ms,
                observed_at_ms,
            })
        })
        .collect::<Result<Vec<_>>>()
    }
}
