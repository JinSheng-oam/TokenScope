use crate::{
    models::Snapshot,
    protobuf::{
        extract_generation_metadata, make_record, proto_field, proto_varint_field,
        protobuf_timestamp_ms,
    },
    AntigravityError, Result, MAX_FILES, MAX_MODELS_PER_DB, MAX_PROTO_BYTES, MAX_TURNS_PER_DB,
};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
};
use tokenscope_core_domain::{
    session_key, stable_record_id, CanonicalRecord, Precision, TokenBreakdown, UsageScope,
};

fn assumed_per_turn(model: &str) -> (u64, u64) {
    let model = model.to_ascii_lowercase();
    if model.starts_with("gemini-2.5-flash-lite") {
        (3_000, 400)
    } else if model.starts_with("claude-opus") {
        (10_000, 900)
    } else if model.starts_with("claude-haiku") {
        (3_000, 400)
    } else if model.starts_with("claude-sonnet") {
        (8_000, 700)
    } else if model.starts_with("gemini-2.5-flash")
        || model.starts_with("gemini-3-flash")
        || model.starts_with("gemini-3.1-flash")
    {
        (4_000, 500)
    } else if model.starts_with("gemini-2.5-pro")
        || model.starts_with("gemini-3-pro")
        || model.starts_with("gemini-pro")
    {
        (8_000, 800)
    } else {
        (5_000, 600)
    }
}

// Strictly bounded protobuf field walker. Fields outside the model path are
// skipped; prompts and step content are never persisted or logged.

#[derive(Default)]
struct StepTimestamps {
    by_index: HashMap<i64, i64>,
    timeline: Vec<Option<i64>>,
}

fn read_step_timestamps(conn: &Connection) -> Result<StepTimestamps> {
    let has_steps: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='steps')",
        [],
        |row| row.get(0),
    )?;
    let mut timestamps = StepTimestamps::default();
    if !has_steps {
        return Ok(timestamps);
    }
    // `gen_metadata` rows are ordered like the model steps themselves; keep the
    // step order as well so rows without an explicit index still get a time.
    let mut stmt =
        conn.prepare("SELECT idx,metadata FROM steps WHERE step_type=15 ORDER BY idx")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let idx: Option<i64> = row.get(0)?;
        let metadata: Option<Vec<u8>> = row.get(1)?;
        let time = metadata
            .as_deref()
            .and_then(|m| proto_field(m, 1).ok().flatten())
            .and_then(protobuf_timestamp_ms);
        if let (Some(idx), Some(time)) = (idx, time) {
            timestamps.by_index.insert(idx, time);
        }
        timestamps.timeline.push(time);
        if timestamps.timeline.len() > MAX_TURNS_PER_DB as usize {
            return Err(AntigravityError::Invalid("step timestamp limit exceeded"));
        }
    }
    Ok(timestamps)
}

pub(super) fn read_db(
    path: &Path,
    stem: String,
    device: &str,
    project_id: Option<String>,
) -> Result<Snapshot> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    // A single SQLite read transaction sees consistent DB+WAL state.
    conn.execute_batch("BEGIN")?;
    let step_timestamps = read_step_timestamps(&conn)?;
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='gen_metadata')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Err(AntigravityError::Invalid("gen_metadata table missing"));
    }
    let mut stmt = conn.prepare(
        "SELECT rowid,CASE WHEN length(data)<=?1 THEN data ELSE NULL END FROM gen_metadata ORDER BY rowid",
    )?;
    let mut rows = stmt.query([MAX_PROTO_BYTES as i64])?;
    let mut counts = BTreeMap::new();
    let mut known_turns = 0u64;
    let mut unknown_turns = 0u64;
    let mut skipped_rows = 0u64;
    let mut records = Vec::new();
    let mut missing_usage = BTreeMap::new();
    let mut seen_responses = std::collections::HashMap::<String, usize>::new();
    let mut dated_unmeasured = BTreeMap::new();
    let mut unmeasured_responses = HashMap::<String, (String, Option<i64>)>::new();
    let mut timeline_pos = 0usize;
    while let Some(row) = rows.next()? {
        let position = timeline_pos;
        timeline_pos += 1;
        if known_turns + unknown_turns + unmeasured_responses.len() as u64 >= MAX_TURNS_PER_DB {
            return Err(AntigravityError::Invalid(
                "conversation turn limit exceeded",
            ));
        }
        let row_idx: Option<i64> = row.get(0)?;
        let blob: Option<Vec<u8>> = row.get(1)?;
        // Oversized artifacts (long notebooks, images) are stored as metadata
        // blobs as well; skipping one row must not discard the conversation.
        let Some(blob) = blob else {
            skipped_rows += 1;
            continue;
        };
        let (model, usage, response_id) = extract_generation_metadata(&blob)?;
        let explicit_time = if usage.is_some() || model.is_some() {
            proto_varint_field(&blob, 2)?
                .and_then(|idx| i64::try_from(idx).ok())
                .and_then(|idx| step_timestamps.by_index.get(&idx).copied())
        } else {
            None
        };
        let native_record = if let Some(usage) = usage {
            let native_id = response_id
                .clone()
                .unwrap_or_else(|| format!("row:{}", row_idx.unwrap_or(known_turns as i64)));
            let event_at_ms =
                explicit_time.or_else(|| step_timestamps.timeline.get(position).copied().flatten());
            Some(make_record(
                &stem,
                device,
                &native_id,
                model.as_deref(),
                project_id.as_deref(),
                usage,
                event_at_ms,
            )?)
        } else {
            None
        };

        // A repeated responseId is the same generation; conflicting snapshots
        // are ambiguous and must not advance the source checkpoint.
        if let (Some(response_id), Some(record)) = (response_id.as_ref(), native_record.as_ref()) {
            if let Some(previous) = seen_responses.get(response_id) {
                if records.get(*previous) != Some(record) {
                    return Err(AntigravityError::Invalid(
                        "conflicting duplicate response id",
                    ));
                }
                continue;
            }
            seen_responses.insert(response_id.clone(), records.len());
        }
        if let Some(record) = native_record {
            records.push(record);
            if let Some(model) = model.as_ref() {
                *counts.entry(model.clone()).or_insert(0) += 1;
                known_turns += 1;
            } else {
                unknown_turns += 1;
            }
        } else if let Some(model) = model {
            if let Some(id) = response_id {
                if let Some((old_model, old_time)) = unmeasured_responses.get_mut(&id) {
                    if *old_model != model
                        || old_time.zip(explicit_time).is_some_and(|(a, b)| a != b)
                    {
                        return Err(AntigravityError::Invalid(
                            "conflicting duplicate response id",
                        ));
                    }
                    *old_time = old_time.or(explicit_time);
                } else {
                    unmeasured_responses.insert(id, (model, explicit_time));
                }
            } else {
                *counts.entry(model.clone()).or_insert(0) += 1;
                *missing_usage.entry(model.clone()).or_insert(0) += 1;
                if let Some(time) = explicit_time {
                    *dated_unmeasured.entry((model, time)).or_insert(0) += 1;
                }
                known_turns += 1;
            }
        } else {
            unknown_turns += 1;
        }
        if counts.len() > MAX_MODELS_PER_DB {
            return Err(AntigravityError::Invalid("model cardinality limit"));
        }
    }
    for (id, (model, time)) in unmeasured_responses {
        if seen_responses.contains_key(&id) {
            continue;
        }
        *counts.entry(model.clone()).or_insert(0) += 1;
        *missing_usage.entry(model.clone()).or_insert(0) += 1;
        if let Some(time) = time {
            *dated_unmeasured.entry((model, time)).or_insert(0) += 1;
        }
        known_turns += 1;
    }
    if counts.len() > MAX_MODELS_PER_DB {
        return Err(AntigravityError::Invalid("model cardinality limit"));
    }
    Ok(Snapshot {
        stem,
        project_id,
        counts,
        known_turns,
        unknown_turns,
        skipped_rows,
        records,
        missing_usage,
        dated_unmeasured,
    })
}

pub fn discover_databases(gemini_home: &Path) -> Result<Vec<PathBuf>> {
    let mut files = BTreeSet::new();
    for prefix in [
        "antigravity",
        "antigravity-ide",
        "antigravity-backup",
        "antigravity-cli",
    ] {
        let dir = gemini_home.join(prefix).join("conversations");
        if !dir.is_dir() {
            continue;
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let typ = entry.file_type()?;
            // Do not follow links out of the configured Antigravity roots.
            if !typ.is_file() || path.extension().and_then(|s| s.to_str()) != Some("db") {
                continue;
            }
            files.insert(path);
            if files.len() > MAX_FILES {
                return Err(AntigravityError::Invalid("too many databases"));
            }
        }
    }
    Ok(files.into_iter().collect())
}

pub(super) fn dominates(a: &Snapshot, b: &Snapshot) -> bool {
    if a.unknown_turns < b.unknown_turns
        || (b.project_id.is_some() && a.project_id != b.project_id)
        || !b
            .counts
            .iter()
            .all(|(model, count)| a.counts.get(model).copied().unwrap_or_default() >= *count)
    {
        return false;
    }
    let mut by_id = HashMap::<&str, Vec<&CanonicalRecord>>::new();
    for record in &a.records {
        by_id.entry(&record.record_id).or_default().push(record);
    }
    b.records.iter().all(|record| {
        by_id
            .get(record.record_id.as_str())
            .is_some_and(|candidates| candidates.contains(&record))
    })
}
pub(super) fn source_fingerprint(snapshot: &Snapshot, estimates: bool) -> String {
    let mut h = Sha256::new();
    h.update(b"tokenscope/antigravity/generation-usage/v3\0");
    h.update([u8::from(estimates)]);
    if let Some(project_id) = &snapshot.project_id {
        h.update((project_id.len() as u32).to_be_bytes());
        h.update(project_id.as_bytes());
    } else {
        h.update(0u32.to_be_bytes());
    }
    h.update(snapshot.unknown_turns.to_be_bytes());
    for (model, count) in &snapshot.counts {
        h.update((model.len() as u32).to_be_bytes());
        h.update(model.as_bytes());
        h.update(count.to_be_bytes());
    }
    for ((model, time), count) in &snapshot.dated_unmeasured {
        h.update((model.len() as u32).to_be_bytes());
        h.update(model.as_bytes());
        h.update(time.to_be_bytes());
        h.update(count.to_be_bytes());
    }
    for record in &snapshot.records {
        h.update(record.record_id.as_bytes());
        h.update(record.usage.input_uncached_tokens.to_be_bytes());
        h.update(record.usage.cache_read_tokens.to_be_bytes());
        h.update(record.usage.output_tokens.to_be_bytes());
        h.update(record.usage.unclassified_tokens.to_be_bytes());
        h.update(
            record
                .usage
                .reasoning_tokens
                .unwrap_or_default()
                .to_be_bytes(),
        );
        h.update(record.event_at_ms.unwrap_or_default().to_be_bytes());
    }
    format!("sha256:{:x}", h.finalize())
}
pub(super) fn source_records(
    s: &Snapshot,
    device: &str,
    estimate_tokens: bool,
) -> Result<Vec<CanonicalRecord>> {
    // This is the only code path that creates estimated usage records.
    // `event_at_ms=None`: DB mtime isn't a generation timestamp.
    let source = s.stem.as_bytes();
    let session = session_key("antigravity", source)?;
    let mut records = s.records.clone();
    if !estimate_tokens || s.unknown_turns > 0 {
        return Ok(records);
    }
    records.reserve(s.missing_usage.len());
    for (model, turns) in &s.missing_usage {
        let (input, output) = assumed_per_turn(model);
        let input = turns
            .checked_mul(input)
            .ok_or(AntigravityError::Invalid("token estimate overflow"))?;
        let output = turns
            .checked_mul(output)
            .ok_or(AntigravityError::Invalid("token estimate overflow"))?;
        if input > i64::MAX as u64 || output > i64::MAX as u64 {
            return Err(AntigravityError::Invalid(
                "estimated usage outside storage range",
            ));
        }
        records.push(CanonicalRecord {
            record_id: stable_record_id("antigravity", source, model.as_bytes())?,
            owner_device_id: device.to_string(),
            tool_id: "antigravity".to_string(),
            scope: UsageScope::DeviceLocal,
            precision: Precision::Estimated,
            account_id: None,
            model_id: Some(model.clone()),
            project_id: s.project_id.clone(),
            session_id: Some(session.clone()),
            event_at_ms: None,
            // We have no reliable cache attribution: keep estimated input
            // in Unclassified rather than falsely reporting cache misses.
            usage: TokenBreakdown {
                unclassified_tokens: input,
                output_tokens: output,
                ..TokenBreakdown::default()
            },
            estimated_cost_microusd: None,
            price_version: None,
            is_deleted: false,
        });
    }
    Ok(records)
}
