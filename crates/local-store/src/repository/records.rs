use super::models::SourceCheckpoint;
use super::pricing::PriceBookCache;
use super::LocalStore;
use crate::error::{db_int, pos_u64, Result, StoreError};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;
use tokenscope_core_domain::{
    payload_hash, CanonicalRecord, Precision, RevisionedRecord, TokenBreakdown, UsageScope,
};
impl LocalStore {
    pub fn get_record(&self, record_id: &str) -> Result<Option<RevisionedRecord>> {
        read_record(&self.conn, record_id)
    }

    /// Current records associated with one authoritative source. Used only for
    /// explicit full reconciliation and parser-version upgrades. Bounded so a
    /// corrupted/adversarial source cannot exhaust memory.
    pub fn records_for_source(
        &self,
        tool: &str,
        source_key: &str,
        limit: usize,
    ) -> Result<Vec<RevisionedRecord>> {
        // One long Antigravity conversation can hold several thousand
        // generations; the bound only guards memory, not correctness.
        if limit == 0 || limit > 20_000 {
            return Err(StoreError::InvalidInput("source read limit"));
        }
        let mut stmt = self.conn.prepare(
            "SELECT record_id FROM record_sources WHERE tool_id=?1 AND source_key=?2 ORDER BY record_id LIMIT ?3"
        )?;
        let ids = stmt
            .query_map(
                params![
                    tool,
                    source_key,
                    i64::try_from(limit + 1).map_err(|_| StoreError::IntegerOutOfRange)?
                ],
                |r| r.get::<_, String>(0),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if ids.len() > limit {
            return Err(StoreError::InvalidInput(
                "source exceeds reconciliation limit",
            ));
        }
        let mut result = Vec::with_capacity(ids.len());
        for id in ids {
            result.push(
                self.get_record(&id)?
                    .ok_or(StoreError::DatabaseInconsistent("source record missing"))?,
            );
        }
        Ok(result)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum RecordChange {
    Inserted,
    Revised,
    Unchanged,
}

pub(super) fn validate_tool_id(tool_id: &str) -> Result<()> {
    if tool_id.is_empty()
        || tool_id.len() > 64
        || !tool_id
            .bytes()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase())
        || !tool_id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    {
        return Err(StoreError::InvalidInput("session tool_id"));
    }
    Ok(())
}

pub(super) fn validate_session_id(session_id: &str) -> Result<()> {
    if session_id.is_empty() || session_id.len() > 1024 || session_id.chars().any(char::is_control)
    {
        return Err(StoreError::InvalidInput("session_id"));
    }
    Ok(())
}

pub(super) fn validate_source_key(source_key: &str) -> Result<()> {
    if source_key.is_empty() || source_key.len() > 1024 || source_key.chars().any(char::is_control)
    {
        return Err(StoreError::InvalidInput("source_key"));
    }
    Ok(())
}

pub(super) fn load_suppressed_sessions(
    conn: &Connection,
    tool_id: &str,
) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare(
        "SELECT session_id FROM session_suppressions WHERE tool_id=?1 ORDER BY session_id",
    )?;
    let rows = stmt.query_map([tool_id], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<HashSet<_>>>()?)
}

pub(super) fn tombstone_source_records(
    tx: &rusqlite::Transaction<'_>,
    tool_id: &str,
    source_key: &str,
    observed_at_ms: i64,
) -> Result<usize> {
    let record_ids = {
        let mut stmt = tx.prepare(
            "SELECT u.record_id FROM record_sources rs \
             JOIN usage_records u ON u.record_id=rs.record_id \
             WHERE rs.tool_id=?1 AND rs.source_key=?2 AND u.is_deleted=0 \
             ORDER BY u.record_id",
        )?;
        let rows = stmt.query_map(params![tool_id, source_key], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let session_ids = {
        let mut stmt = tx.prepare(
            "SELECT DISTINCT u.session_id FROM record_sources rs \
             JOIN usage_records u ON u.record_id=rs.record_id \
             WHERE rs.tool_id=?1 AND rs.source_key=?2 AND u.session_id IS NOT NULL",
        )?;
        let rows = stmt.query_map(params![tool_id, source_key], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut tombstoned = 0usize;
    for record_id in record_ids {
        let source_count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM record_sources WHERE record_id=?1",
            [&record_id],
            |row| row.get(0),
        )?;
        if source_count != 1 {
            return Err(StoreError::SourceArbitrationConflict);
        }
        let mut envelope = read_record(tx, &record_id)?
            .ok_or(StoreError::DatabaseInconsistent("source record missing"))?;
        if envelope.record.is_deleted {
            continue;
        }
        envelope.record.is_deleted = true;
        envelope.record.usage = TokenBreakdown::default();
        envelope.record.estimated_cost_microusd = None;
        envelope.record.price_version = None;
        let hash = payload_hash(&envelope.record)?;
        let revision = i64::try_from(envelope.record_revision)
            .map_err(|_| StoreError::RevisionOverflow)?
            .checked_add(1)
            .ok_or(StoreError::RevisionOverflow)?;
        write_record(tx, &envelope.record, revision, &hash, observed_at_ms)?;
        enqueue(tx, &record_id, revision, observed_at_ms)?;
        tombstoned += 1;
    }
    for session_id in session_ids {
        tx.execute(
            "DELETE FROM session_names WHERE session_id=?1 AND tool_id=?2 \
             AND NOT EXISTS (SELECT 1 FROM usage_records u \
                 WHERE u.session_id=?1 AND u.tool_id=?2 AND u.is_deleted=0)",
            params![session_id, tool_id],
        )?;
    }
    tx.execute(
        "DELETE FROM session_names WHERE session_id=?1 AND tool_id=?2 \
         AND NOT EXISTS (SELECT 1 FROM usage_records u \
             WHERE u.session_id=?1 AND u.tool_id=?2 AND u.is_deleted=0)",
        params![source_key, tool_id],
    )?;
    if tool_id == "antigravity" {
        tx.execute(
            "DELETE FROM antigravity_activity_sources WHERE source_key=?1",
            [source_key],
        )?;
    }
    tx.execute(
        "DELETE FROM empty_source_candidates WHERE tool_id=?1 AND source_key=?2",
        params![tool_id, source_key],
    )?;
    Ok(tombstoned)
}

pub(super) fn upsert_source_record(
    tx: &rusqlite::Transaction<'_>,
    rec: &mut CanonicalRecord,
    source_key: &str,
    owner: &str,
    observed_at_ms: i64,
    price_books: &mut PriceBookCache<'_>,
) -> Result<RecordChange> {
    if rec.owner_device_id != owner {
        return Err(StoreError::IdentityMismatch);
    }
    let previous = read_record(tx, &rec.record_id)?;
    if let Some(previous) = &previous {
        if previous.record.owner_device_id != owner || previous.record.tool_id != rec.tool_id {
            return Err(StoreError::RecordOwnershipConflict);
        }
    }
    price_books
        .price_record_with_previous(rec, previous.as_ref().map(|envelope| &envelope.record))?;
    let hash = payload_hash(rec)?;
    let change;
    match previous {
        Some(previous) => {
            let mut sources = tx.prepare(
                "SELECT source_key FROM record_sources WHERE record_id=?1 AND tool_id=?2",
            )?;
            let found = sources
                .query_map(params![rec.record_id, rec.tool_id], |r| {
                    r.get::<_, String>(0)
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if found.is_empty() {
                return Err(StoreError::DatabaseInconsistent(
                    "record lacks source provenance",
                ));
            }
            if previous.payload_hash != hash {
                // Once multiple observers claim a logical record, require
                // upstream arbitration. Never oscillate between values.
                if found.len() != 1 || found[0] != source_key {
                    return Err(StoreError::SourceArbitrationConflict);
                }
                let newer = i64::try_from(previous.record_revision)
                    .map_err(|_| StoreError::RevisionOverflow)?
                    .checked_add(1)
                    .ok_or(StoreError::RevisionOverflow)?;
                write_record(tx, rec, newer, &hash, observed_at_ms)?;
                enqueue(tx, &rec.record_id, newer, observed_at_ms)?;
                change = RecordChange::Revised;
            } else {
                change = RecordChange::Unchanged;
            }
        }
        None => {
            write_record(tx, rec, 1, &hash, observed_at_ms)?;
            enqueue(tx, &rec.record_id, 1, observed_at_ms)?;
            change = RecordChange::Inserted;
        }
    }
    tx.execute(
        "INSERT OR IGNORE INTO record_sources(record_id,tool_id,source_key) VALUES(?1,?2,?3)",
        params![rec.record_id, rec.tool_id, source_key],
    )?;
    Ok(change)
}

pub(super) fn read_record(conn: &Connection, record_id: &str) -> Result<Option<RevisionedRecord>> {
    let raw = conn.query_row(
            "SELECT record_id,owner_device_id,tool_id,scope,account_id,model_id,project_id,session_id,event_at_ms, \
                input_uncached_tokens,cache_read_tokens,cache_write_tokens,output_tokens,unclassified_tokens, \
                reasoning_tokens,estimated_cost_microusd,price_version,precision,record_revision,payload_hash,is_deleted \
             FROM usage_records WHERE record_id=?1",
            [record_id], RawRecord::read
        ).optional()?;
    raw.map(RawRecord::into_envelope).transpose()
}

pub(super) fn load_checkpoint(
    conn: &Connection,
    tool: &str,
    key: &str,
) -> Result<Option<SourceCheckpoint>> {
    let stored = conn.query_row(
        "SELECT source_fingerprint,read_offset,parser_version,last_scan_ms,last_complete_scan_ms,last_status \
         FROM source_checkpoints WHERE tool_id=?1 AND source_key=?2", params![tool,key],
        |r| Ok((r.get::<_, String>(0)?,r.get::<_, i64>(1)?,r.get::<_, i64>(2)?,
                 r.get::<_, i64>(3)?,r.get::<_, Option<i64>>(4)?,r.get::<_, String>(5)?))
    ).optional()?;
    match stored {
        Some((
            source_fingerprint,
            offset,
            parser,
            last_scan_ms,
            last_complete_scan_ms,
            last_status,
        )) => {
            let parser_version = u32::try_from(parser)
                .map_err(|_| StoreError::DatabaseInconsistent("invalid parser version"))?;
            Ok(Some(SourceCheckpoint {
                tool_id: tool.to_string(),
                source_key: key.to_string(),
                source_fingerprint,
                read_offset: pos_u64(offset)?,
                parser_version,
                last_scan_ms,
                last_complete_scan_ms,
                last_status,
            }))
        }
        None => Ok(None),
    }
}

pub(super) fn enqueue(tx: &rusqlite::Transaction<'_>, id: &str, rev: i64, now: i64) -> Result<()> {
    tx.execute("INSERT INTO sync_outbox(record_id,record_revision,enqueued_at_ms) VALUES(?1,?2,?3) \
                ON CONFLICT(record_id) DO UPDATE SET record_revision=excluded.record_revision, \
                enqueued_at_ms=excluded.enqueued_at_ms,attempts=0,last_error=NULL,next_attempt_at_ms=0,blocked_at_ms=NULL",
        params![id,rev,now])?;
    Ok(())
}

pub(super) fn write_record(
    tx: &rusqlite::Transaction<'_>,
    rec: &CanonicalRecord,
    rev: i64,
    hash: &str,
    now: i64,
) -> Result<()> {
    let scope = match rec.scope {
        UsageScope::DeviceLocal => "device_local",
        UsageScope::AccountGlobal => "account_global",
        UsageScope::Unknown => "unknown",
    };
    let precision = match rec.precision {
        Precision::Exact => "exact",
        Precision::Aggregate => "aggregate",
        Precision::Estimated => "estimated",
        Precision::Unknown => "unknown",
    };
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
    record_id: String,
    owner_device_id: String,
    tool_id: String,
    scope: String,
    account_id: Option<String>,
    model_id: Option<String>,
    project_id: Option<String>,
    session_id: Option<String>,
    event_at_ms: Option<i64>,
    input: i64,
    cache_read: i64,
    cache_write: i64,
    output: i64,
    unclassified: i64,
    reasoning: Option<i64>,
    cost: Option<i64>,
    price: Option<String>,
    precision: String,
    revision: i64,
    hash: String,
    deleted: i64,
}
impl RawRecord {
    fn read(r: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            record_id: r.get(0)?,
            owner_device_id: r.get(1)?,
            tool_id: r.get(2)?,
            scope: r.get(3)?,
            account_id: r.get(4)?,
            model_id: r.get(5)?,
            project_id: r.get(6)?,
            session_id: r.get(7)?,
            event_at_ms: r.get(8)?,
            input: r.get(9)?,
            cache_read: r.get(10)?,
            cache_write: r.get(11)?,
            output: r.get(12)?,
            unclassified: r.get(13)?,
            reasoning: r.get(14)?,
            cost: r.get(15)?,
            price: r.get(16)?,
            precision: r.get(17)?,
            revision: r.get(18)?,
            hash: r.get(19)?,
            deleted: r.get(20)?,
        })
    }
    fn into_envelope(self) -> Result<RevisionedRecord> {
        let scope = match self.scope.as_str() {
            "device_local" => UsageScope::DeviceLocal,
            "account_global" => UsageScope::AccountGlobal,
            "unknown" => UsageScope::Unknown,
            _ => return Err(StoreError::DatabaseInconsistent("scope")),
        };
        let precision = match self.precision.as_str() {
            "exact" => Precision::Exact,
            "aggregate" => Precision::Aggregate,
            "estimated" => Precision::Estimated,
            "unknown" => Precision::Unknown,
            _ => return Err(StoreError::DatabaseInconsistent("precision")),
        };
        let rec = CanonicalRecord {
            record_id: self.record_id,
            owner_device_id: self.owner_device_id,
            tool_id: self.tool_id,
            scope,
            precision,
            account_id: self.account_id,
            model_id: self.model_id,
            project_id: self.project_id,
            session_id: self.session_id,
            event_at_ms: self.event_at_ms,
            usage: TokenBreakdown {
                input_uncached_tokens: pos_u64(self.input)?,
                cache_read_tokens: pos_u64(self.cache_read)?,
                cache_write_tokens: pos_u64(self.cache_write)?,
                output_tokens: pos_u64(self.output)?,
                unclassified_tokens: pos_u64(self.unclassified)?,
                reasoning_tokens: self.reasoning.map(pos_u64).transpose()?,
            },
            estimated_cost_microusd: self.cost,
            price_version: self.price,
            is_deleted: match self.deleted {
                0 => false,
                1 => true,
                _ => return Err(StoreError::DatabaseInconsistent("is_deleted")),
            },
        };
        let envelope = RevisionedRecord {
            record: rec,
            record_revision: pos_u64(self.revision)?,
            payload_hash: self.hash,
        };
        envelope.validate()?;
        Ok(envelope)
    }
}
