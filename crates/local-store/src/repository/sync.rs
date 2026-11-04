use super::models::{
    tool_display_name, OutboxEntry, RemoteDeviceUsage, RemoteUsageBreakdown, RemoteUsageChange,
    RemoteUsageTotals, SyncOutboxStatus,
};
use super::LocalStore;
use crate::error::{db_int, pos_u64, Result, StoreError};
use rusqlite::{params, OptionalExtension, TransactionBehavior};

struct RemoteUsageTotalsRow {
    total: i64,
    input: i64,
    cache_read: i64,
    cache_write: i64,
    output: i64,
    unclassified: i64,
    count: i64,
    priced: i64,
    exact: i64,
    aggregate: i64,
    estimated: i64,
    unknown: i64,
    cost: Option<i64>,
}
impl LocalStore {
    /// Apply a bounded Hub change-feed page and advance its cursor atomically.
    /// Older revisions are ignored; same-revision hash conflicts abort the entire page.
    pub fn pull_cursor(&self) -> Result<u64> {
        let value: i64 = self.conn.query_row(
            "SELECT last_pull_seq FROM sync_state WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        pos_u64(value)
    }

    pub fn apply_remote_changes(
        &mut self,
        changes: &[RemoteUsageChange],
        next_cursor: u64,
    ) -> Result<usize> {
        if changes.len() > 500 {
            return Err(StoreError::InvalidInput("remote page limit"));
        }
        let local_device_id = self.identity()?.map(|i| i.device_id).unwrap_or_default();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current_cursor: i64 = tx.query_row(
            "SELECT last_pull_seq FROM sync_state WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        let mut previous =
            u64::try_from(current_cursor).map_err(|_| StoreError::IntegerOutOfRange)?;
        let mut applied = 0usize;
        for change in changes {
            if change.change_seq <= previous
                || change.change_seq > next_cursor
                || change.owner_device_id != change.envelope.record.owner_device_id
            {
                return Err(StoreError::InvalidInput("remote change sequence/owner"));
            }
            change
                .envelope
                .validate()
                .map_err(|_| StoreError::InvalidInput("remote envelope"))?;
            if change.owner_device_id == local_device_id {
                previous = change.change_seq;
                continue;
            }
            let raw = serde_json::to_string(&change.envelope.record)
                .map_err(|_| StoreError::InvalidInput("remote serialization"))?;
            let rev = db_int(change.envelope.record_revision)?;
            let mut old: Option<(i64,String)> = tx.query_row(
                "SELECT record_revision,payload_hash FROM remote_usage_cache WHERE owner_device_id=?1 AND record_id=?2",
                params![change.owner_device_id,change.envelope.record.record_id], |r| Ok((r.get(0)?,r.get(1)?))
            ).optional()?;
            if let Some((old_rev, old_hash)) = old.take() {
                if rev < old_rev {
                    previous = change.change_seq;
                    continue;
                }
                if rev == old_rev && !old_hash.is_empty() {
                    if old_hash != change.envelope.payload_hash {
                        return Err(StoreError::DatabaseInconsistent(
                            "remote same-revision payload conflict",
                        ));
                    }
                    previous = change.change_seq;
                    continue;
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
            return Err(StoreError::InvalidInput(
                "remote cursor must match last applied sequence",
            ));
        }
        tx.execute("UPDATE sync_state SET last_pull_seq=?1,last_pull_ms=CAST(strftime('%s','now') AS INTEGER)*1000 WHERE singleton=1", [db_int(next_cursor)?])?;
        tx.commit()?;
        Ok(applied)
    }

    pub fn remote_usage_totals(
        &self,
        owner_device_id: Option<&str>,
        start_ms: Option<i64>,
        end_ms: Option<i64>,
    ) -> Result<RemoteUsageTotals> {
        let row = self.conn.query_row(
            "SELECT COALESCE(SUM(CAST(json_extract(payload_json,'$.usage.input_uncached_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.cache_read_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.cache_write_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.output_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.unclassified_tokens') AS INTEGER)),0),COALESCE(SUM(CAST(json_extract(payload_json,'$.usage.input_uncached_tokens') AS INTEGER)),0),COALESCE(SUM(CAST(json_extract(payload_json,'$.usage.cache_read_tokens') AS INTEGER)),0),COALESCE(SUM(CAST(json_extract(payload_json,'$.usage.cache_write_tokens') AS INTEGER)),0),COALESCE(SUM(CAST(json_extract(payload_json,'$.usage.output_tokens') AS INTEGER)),0),COALESCE(SUM(CAST(json_extract(payload_json,'$.usage.unclassified_tokens') AS INTEGER)),0),COUNT(*),COALESCE(SUM(CASE WHEN json_extract(payload_json,'$.estimated_cost_microusd') IS NOT NULL THEN 1 ELSE 0 END),0),COALESCE(SUM(CASE WHEN json_extract(payload_json,'$.precision')='exact' THEN 1 ELSE 0 END),0),COALESCE(SUM(CASE WHEN json_extract(payload_json,'$.precision')='aggregate' THEN 1 ELSE 0 END),0),COALESCE(SUM(CASE WHEN json_extract(payload_json,'$.precision')='estimated' THEN 1 ELSE 0 END),0),COALESCE(SUM(CASE WHEN json_extract(payload_json,'$.precision')='unknown' THEN 1 ELSE 0 END),0),SUM(json_extract(payload_json,'$.estimated_cost_microusd')) FROM remote_usage_cache WHERE is_deleted=0 AND json_extract(payload_json,'$.scope')='device_local' AND (?1 IS NULL OR owner_device_id=?1) AND (?2 IS NULL OR json_extract(payload_json,'$.event_at_ms')>=?2) AND (?3 IS NULL OR json_extract(payload_json,'$.event_at_ms')<?3)",
            params![owner_device_id,start_ms,end_ms], |r| Ok(RemoteUsageTotalsRow {
                total: r.get(0)?, input: r.get(1)?, cache_read: r.get(2)?, cache_write: r.get(3)?,
                output: r.get(4)?, unclassified: r.get(5)?, count: r.get(6)?, priced: r.get(7)?,
                exact: r.get(8)?, aggregate: r.get(9)?, estimated: r.get(10)?, unknown: r.get(11)?,
                cost: r.get(12)?,
            })
        )?;
        let RemoteUsageTotalsRow {
            total,
            input,
            cache_read,
            cache_write,
            output,
            unclassified,
            count,
            priced,
            exact,
            aggregate,
            estimated,
            unknown,
            cost,
        } = row;
        let record_count = pos_u64(count)?;
        let priced_records = pos_u64(priced)?;
        Ok(RemoteUsageTotals {
            total_tokens: pos_u64(total)?,
            input_tokens: pos_u64(input)?,
            cache_read_tokens: pos_u64(cache_read)?,
            cache_write_tokens: pos_u64(cache_write)?,
            output_tokens: pos_u64(output)?,
            unclassified_tokens: pos_u64(unclassified)?,
            record_count,
            priced_records,
            unpriced_records: record_count.saturating_sub(priced_records),
            exact_records: pos_u64(exact)?,
            aggregate_records: pos_u64(aggregate)?,
            estimated_records: pos_u64(estimated)?,
            unknown_precision_records: pos_u64(unknown)?,
            estimated_cost_microusd: cost,
        })
    }

    pub fn remote_usage_by_device(
        &self,
        start_ms: Option<i64>,
        end_ms: Option<i64>,
    ) -> Result<Vec<RemoteDeviceUsage>> {
        let mut stmt=self.conn.prepare("SELECT owner_device_id,COALESCE(SUM(CAST(json_extract(payload_json,'$.usage.input_uncached_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.cache_read_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.cache_write_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.output_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.unclassified_tokens') AS INTEGER)),0),COUNT(*),SUM(json_extract(payload_json,'$.estimated_cost_microusd')) FROM remote_usage_cache WHERE is_deleted=0 AND json_extract(payload_json,'$.scope')='device_local' AND (?1 IS NULL OR json_extract(payload_json,'$.event_at_ms')>=?1) AND (?2 IS NULL OR json_extract(payload_json,'$.event_at_ms')<?2) GROUP BY owner_device_id ORDER BY owner_device_id")?;
        let rows = stmt.query_map(params![start_ms, end_ms], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, Option<i64>>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, total, count, cost) = row?;
            out.push(RemoteDeviceUsage {
                owner_device_id: id,
                total_tokens: pos_u64(total)?,
                record_count: pos_u64(count)?,
                estimated_cost_microusd: cost,
            });
        }
        Ok(out)
    }

    /// Remote breakdowns preserve device ownership. Only device-local usage is
    /// included; account-global and unknown-scope rows are never apportioned.
    pub fn remote_usage_breakdown(
        &self,
        dimension: &'static str,
        start_ms: Option<i64>,
        end_ms: Option<i64>,
    ) -> Result<Vec<RemoteUsageBreakdown>> {
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
        let rows = stmt
            .query_map(params![start_ms, end_ms], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if rows.len() > 1000 {
            return Err(StoreError::InvalidInput(
                "remote breakdown cardinality exceeds 1000",
            ));
        }
        let mut out = Vec::with_capacity(rows.len());
        for (owner, raw_key, count, total, cost) in rows {
            let key = raw_key.unwrap_or_else(|| match dimension {
                "tool_id" => "unknown-tool".into(),
                "model_id" => "unknown-model".into(),
                _ => "no-project".into(),
            });
            let label = match dimension {
                "project_id" => self
                    .project_display_name(&key)?
                    .unwrap_or_else(|| "项目名称未提供".into()),
                "tool_id" => tool_display_name(&key),
                _ => key.clone(),
            };
            out.push(RemoteUsageBreakdown {
                owner_device_id: owner,
                key,
                label,
                total_tokens: pos_u64(total)?,
                record_count: pos_u64(count)?,
                estimated_cost_microusd: cost,
            });
        }
        Ok(out)
    }

    /// Returns only due, non-quarantined outbox entries. Retry scheduling survives restarts.
    pub fn pending_outbox_due(&self, limit: usize, now_ms: i64) -> Result<Vec<OutboxEntry>> {
        if limit == 0 || limit > 200 || now_ms < 0 {
            return Err(StoreError::InvalidInput("outbox due query"));
        }
        let mut stmt = self.conn.prepare(
            "SELECT record_id,record_revision,enqueued_at_ms,attempts FROM sync_outbox \
             WHERE blocked_at_ms IS NULL AND next_attempt_at_ms <= ?1 \
             ORDER BY enqueued_at_ms,record_id LIMIT ?2",
        )?;
        let raw = stmt
            .query_map(
                params![
                    now_ms,
                    i64::try_from(limit).map_err(|_| StoreError::IntegerOutOfRange)?
                ],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut entries = Vec::with_capacity(raw.len());
        for (record_id, rev, enqueued_at_ms, attempts) in raw {
            let envelope = self
                .get_record(&record_id)?
                .ok_or(StoreError::DatabaseInconsistent("outbox record missing"))?;
            if envelope.record_revision != pos_u64(rev)? {
                return Err(StoreError::DatabaseInconsistent(
                    "outbox revision does not match record",
                ));
            }
            entries.push(OutboxEntry {
                envelope,
                enqueued_at_ms,
                attempts: pos_u64(attempts)?,
            });
        }
        Ok(entries)
    }

    /// Persist exponential backoff: 5s, 10s, ... capped at one hour.
    pub fn schedule_outbox_retry(
        &mut self,
        record_id: &str,
        revision: u64,
        now_ms: i64,
        error_code: &str,
    ) -> Result<Option<i64>> {
        if revision == 0
            || now_ms < 0
            || error_code.is_empty()
            || error_code.len() > 128
            || error_code.chars().any(char::is_control)
        {
            return Err(StoreError::InvalidInput("outbox retry"));
        }
        let attempts: Option<i64> = self.conn.query_row(
            "SELECT attempts FROM sync_outbox WHERE record_id=?1 AND record_revision=?2 AND blocked_at_ms IS NULL",
            params![record_id, db_int(revision)?], |r| r.get(0)
        ).optional()?;
        let Some(attempts) = attempts else {
            return Ok(None);
        };
        let exponent = attempts.clamp(0, 10) as u32;
        let delay_ms = 5_000_i64.saturating_mul(1_i64 << exponent).min(3_600_000);
        let next_at = now_ms.saturating_add(delay_ms);
        let changed = self.conn.execute(
            "UPDATE sync_outbox SET attempts=attempts+1,last_error=?3,next_attempt_at_ms=?4 \
             WHERE record_id=?1 AND record_revision=?2 AND blocked_at_ms IS NULL",
            params![record_id, db_int(revision)?, error_code, next_at],
        )?;
        Ok((changed == 1).then_some(next_at))
    }

    /// A superseded revision is quarantined until explicit remote reconciliation; never retry forever.
    pub fn block_outbox_revision(
        &mut self,
        record_id: &str,
        revision: u64,
        now_ms: i64,
        reason: &str,
    ) -> Result<bool> {
        if revision == 0
            || now_ms < 0
            || reason.is_empty()
            || reason.len() > 128
            || reason.chars().any(char::is_control)
        {
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
        Ok(SyncOutboxStatus {
            pending: pending.max(0) as u64,
            deferred: deferred.max(0) as u64,
            blocked: blocked.max(0) as u64,
            oldest_pending_at_ms: oldest,
            next_attempt_at_ms: next,
            last_attempt_ms: last_attempt,
            last_success_ms: last_success,
            last_pull_seq: pos_u64(last_pull_seq)?,
            last_pull_ms,
            last_status,
            last_error,
        })
    }

    pub fn update_sync_state(
        &mut self,
        now_ms: i64,
        status: &str,
        error: Option<&str>,
    ) -> Result<()> {
        if now_ms < 0
            || ![
                "idle",
                "success",
                "deferred",
                "blocked",
                "auth_required",
                "error",
            ]
            .contains(&status)
        {
            return Err(StoreError::InvalidInput("sync state"));
        }
        if error.is_some_and(|v| v.len() > 128 || v.chars().any(char::is_control)) {
            return Err(StoreError::InvalidInput("sync error"));
        }
        self.conn.execute(
            "UPDATE sync_state SET last_attempt_ms=?1,last_status=?2,last_error=?3, \
             last_success_ms=CASE WHEN ?2='success' THEN ?1 ELSE last_success_ms END WHERE singleton=1",
            params![now_ms,status,error]
        )?;
        Ok(())
    }

    /// Bounded reads: never copy the whole archive into memory for a push.
    pub fn pending_outbox(&self, limit: usize) -> Result<Vec<OutboxEntry>> {
        if limit == 0 || limit > 200 {
            return Err(StoreError::InvalidInput("outbox batch limit (1..200)"));
        }
        let mut stmt = self.conn.prepare(
            "SELECT record_id,record_revision,enqueued_at_ms,attempts FROM sync_outbox ORDER BY enqueued_at_ms,record_id LIMIT ?1"
        )?;
        let raw = stmt
            .query_map(
                [i64::try_from(limit).map_err(|_| StoreError::IntegerOutOfRange)?],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut entries = Vec::with_capacity(raw.len());
        for (record_id, rev, enqueued_at_ms, attempts) in raw {
            let envelope = self
                .get_record(&record_id)?
                .ok_or(StoreError::DatabaseInconsistent("outbox record missing"))?;
            if envelope.record_revision != pos_u64(rev)? {
                return Err(StoreError::DatabaseInconsistent(
                    "outbox revision does not match record",
                ));
            }
            entries.push(OutboxEntry {
                envelope,
                enqueued_at_ms,
                attempts: pos_u64(attempts)?,
            });
        }
        Ok(entries)
    }

    /// Delete only on an authoritative Hub accepted/unchanged response. The
    /// network layer must NOT interpret `superseded` as a safe acknowledgement.
    pub fn ack_outbox_if_revision(&mut self, record_id: &str, revision: u64) -> Result<bool> {
        if revision == 0 {
            return Err(StoreError::InvalidInput("ack revision"));
        }
        let changed = self.conn.execute(
            "DELETE FROM sync_outbox WHERE record_id=?1 AND record_revision=?2",
            params![record_id, db_int(revision)?],
        )?;
        Ok(changed == 1)
    }

    pub fn note_outbox_failure(
        &mut self,
        record_id: &str,
        revision: u64,
        error_code: &str,
    ) -> Result<bool> {
        if error_code.is_empty()
            || error_code.len() > 128
            || error_code.chars().any(char::is_control)
        {
            return Err(StoreError::InvalidInput("safe error code"));
        }
        let changed = self.conn.execute(
            "UPDATE sync_outbox SET attempts=attempts+1,last_error=?3 \
             WHERE record_id=?1 AND record_revision=?2 AND attempts < 9223372036854775807",
            params![record_id, db_int(revision)?, error_code],
        )?;
        Ok(changed == 1)
    }

    pub fn webdav_cursor(&self, remote_device_id: &str) -> Result<u64> {
        let val: Option<i64> = self
            .conn
            .query_row(
                "SELECT last_batch_seq FROM webdav_sync_cursors WHERE remote_device_id = ?1",
                [remote_device_id],
                |r| r.get(0),
            )
            .optional()?;
        match val {
            Some(v) => pos_u64(v),
            None => Ok(0),
        }
    }

    pub fn set_webdav_cursor(
        &mut self,
        remote_device_id: &str,
        last_batch_seq: u64,
        updated_at_ms: i64,
    ) -> Result<()> {
        if remote_device_id.is_empty() || updated_at_ms < 0 {
            return Err(StoreError::InvalidInput("webdav cursor"));
        }
        self.conn.execute(
            "INSERT INTO webdav_sync_cursors(remote_device_id, last_batch_seq, updated_at_ms)              VALUES (?1, ?2, ?3)              ON CONFLICT(remote_device_id) DO UPDATE SET              last_batch_seq = excluded.last_batch_seq, updated_at_ms = excluded.updated_at_ms",
            params![remote_device_id, db_int(last_batch_seq)?, updated_at_ms],
        )?;
        Ok(())
    }

    pub fn all_webdav_cursors(&self) -> Result<Vec<(String, u64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT remote_device_id, last_batch_seq FROM webdav_sync_cursors ORDER BY remote_device_id"
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        let mut out = Vec::new();
        for r in rows {
            let (dev, seq) = r?;
            out.push((dev, pos_u64(seq)?));
        }
        Ok(out)
    }

    pub fn apply_remote_device_records(
        &mut self,
        owner_device_id: &str,
        batch_seq: u64,
        records: &[tokenscope_core_domain::RevisionedRecord],
        now_ms: i64,
    ) -> Result<usize> {
        if owner_device_id.is_empty() || batch_seq == 0 {
            return Err(StoreError::InvalidInput("webdav batch parameters"));
        }
        let local_device_id = self.identity()?.map(|i| i.device_id).unwrap_or_default();
        if owner_device_id == local_device_id {
            return Ok(0);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current_cursor: Option<i64> = tx
            .query_row(
                "SELECT last_batch_seq FROM webdav_sync_cursors WHERE remote_device_id = ?1",
                [owner_device_id],
                |r| r.get(0),
            )
            .optional()?;
        let current_seq = match current_cursor {
            Some(v) => pos_u64(v)?,
            None => 0,
        };
        if batch_seq <= current_seq {
            return Ok(0);
        }
        let mut applied = 0usize;
        for record in records {
            if record.record.owner_device_id != owner_device_id {
                return Err(StoreError::InvalidInput(
                    "record owner does not match batch device",
                ));
            }
            record
                .validate()
                .map_err(|_| StoreError::InvalidInput("invalid record envelope"))?;
            let raw = serde_json::to_string(&record.record)
                .map_err(|_| StoreError::InvalidInput("remote record serialization"))?;
            let rev = db_int(record.record_revision)?;
            let old: Option<(i64, String)> = tx
                .query_row(
                    "SELECT record_revision, payload_hash FROM remote_usage_cache WHERE owner_device_id = ?1 AND record_id = ?2",
                    params![owner_device_id, record.record.record_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((old_rev, old_hash)) = old {
                if rev < old_rev {
                    continue;
                }
                if rev == old_rev && !old_hash.is_empty() {
                    if old_hash != record.payload_hash {
                        return Err(StoreError::DatabaseInconsistent(
                            "remote same-revision payload conflict",
                        ));
                    }
                    continue;
                }
            }
            tx.execute(
                "INSERT INTO remote_usage_cache(owner_device_id, record_id, record_revision, payload_json, is_deleted, server_change_seq, payload_hash)                  VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)                  ON CONFLICT(owner_device_id, record_id) DO UPDATE SET                  record_revision=excluded.record_revision, payload_json=excluded.payload_json, is_deleted=excluded.is_deleted,                  server_change_seq=excluded.server_change_seq, payload_hash=excluded.payload_hash                  WHERE excluded.record_revision > remote_usage_cache.record_revision                  OR (excluded.record_revision = remote_usage_cache.record_revision AND remote_usage_cache.payload_hash = '')",
                params![
                    owner_device_id,
                    record.record.record_id,
                    rev,
                    raw,
                    record.record.is_deleted as i64,
                    db_int(batch_seq)?,
                    record.payload_hash,
                ],
            )?;
            applied += 1;
        }
        tx.execute(
            "INSERT INTO webdav_sync_cursors(remote_device_id, last_batch_seq, updated_at_ms)              VALUES (?1, ?2, ?3)              ON CONFLICT(remote_device_id) DO UPDATE SET              last_batch_seq = excluded.last_batch_seq, updated_at_ms = excluded.updated_at_ms",
            params![owner_device_id, db_int(batch_seq)?, now_ms],
        )?;
        tx.commit()?;
        Ok(applied)
    }
}
