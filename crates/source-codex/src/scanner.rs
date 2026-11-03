use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufReader, Seek, SeekFrom};
use std::path::Path;
use tokenscope_core_domain::{
    session_key, stable_record_id, CanonicalRecord, Precision, UsageScope,
};
use tokenscope_local_store::SourceCheckpoint;

use super::model::{CodexError, Counters, CursorState, Result, ScanOutcome};
use super::rollout::{bounded_line, event_ms, hash_path, mtime_ns, read_session_header, sha_tail};
use super::PARSER_VERSION;

pub fn scan_file(
    path: &Path,
    owner_device_id: &str,
    prior: Option<&SourceCheckpoint>,
    observed_ms: i64,
) -> Result<ScanOutcome> {
    let session_id = read_session_header(path)?;
    let source_key = session_key("codex", session_id.as_bytes())?;
    if let Some(cp) = prior {
        if cp.tool_id != "codex" || cp.source_key != source_key {
            return Err(CodexError::Invalid("checkpoint identity mismatch"));
        }
    }
    let before = fs::metadata(path)?;
    if !before.is_file() {
        return Err(CodexError::Invalid("not a regular file"));
    }
    let file_len = before.len();
    let current_mtime = mtime_ns(&before)?;
    let path_hash = hash_path(path)?;
    let old_state = prior
        .filter(|cp| cp.parser_version == PARSER_VERSION)
        .and_then(|cp| serde_json::from_str::<CursorState>(&cp.source_fingerprint).ok());
    let mut reused = false;
    let mut offset = 0;
    let mut counters = None;
    let mut last_event = None;
    if let (Some(cp), Some(saved)) = (prior, old_state.as_ref()) {
        // If an old file is rewritten with equal size/mtime, the tail hash still
        // catches changes in the processed edge; periodic full reconciliation
        // remains necessary for changes far earlier in the file.
        if cp.read_offset <= file_len
            && saved.path_hash == path_hash
            && (file_len != cp.read_offset || saved.mtime_ns == current_mtime)
            && saved.tail_hash == sha_tail(path, cp.read_offset)?
        {
            offset = cp.read_offset;
            counters = saved.counters;
            last_event = saved.event_at_ms;
            reused = true;
        }
    }
    let mut reader = BufReader::new(File::open(path)?);
    reader.seek(SeekFrom::Start(offset))?;
    let mut parsed = 0usize;
    let mut incomplete = false;
    loop {
        let start = reader.stream_position()?;
        let line = bounded_line(&mut reader)?;
        if line.bytes.is_empty() && !line.oversized {
            break;
        }
        if !line.complete {
            incomplete = true;
            offset = start;
            break;
        }
        parsed += 1;
        if line.oversized {
            offset = reader.stream_position()?;
            continue;
        }
        let v: Value = serde_json::from_slice(&line.bytes)?;
        let payload = v.get("payload");
        if v.get("type").and_then(Value::as_str) == Some("event_msg")
            && payload.and_then(|p| p.get("type")).and_then(Value::as_str) == Some("token_count")
        {
            if let Some(info) = payload.and_then(|p| p.get("info")) {
                if let Some(next) = Counters::from_info(info)? {
                    if counters.is_some_and(|prev| !next.monotonic_from(&prev)) {
                        return Err(CodexError::CounterRegression);
                    }
                    if counters != Some(next) {
                        last_event = event_ms(&v)?;
                        counters = Some(next);
                    }
                }
            }
        }
        offset = reader.stream_position()?;
    }
    // A concurrent writer may append, which will be discovered next scan.
    // Truncation/rewrite during our read must never publish an unreliable total.
    let after = fs::metadata(path)?;
    if after.len() < offset || mtime_ns(&after)? != current_mtime || after.len() != file_len {
        return Err(CodexError::Invalid("file changed during scan"));
    }
    let next_time = prior.map_or(observed_ms, |p| {
        observed_ms.max(p.last_scan_ms.saturating_add(1))
    });
    let status = if incomplete { "partial" } else { "complete" };
    let state = CursorState {
        path_hash,
        tail_hash: sha_tail(path, offset)?,
        mtime_ns: current_mtime,
        counters,
        event_at_ms: last_event,
    };
    let fingerprint = serde_json::to_string(&state)?;
    if fingerprint.len() > 1024 {
        return Err(CodexError::Limit("checkpoint fingerprint"));
    }
    let next = SourceCheckpoint {
        tool_id: "codex".to_owned(),
        source_key,
        source_fingerprint: fingerprint,
        read_offset: offset,
        parser_version: PARSER_VERSION,
        last_scan_ms: next_time,
        last_complete_scan_ms: if incomplete {
            prior.and_then(|p| p.last_complete_scan_ms)
        } else {
            Some(next_time)
        },
        last_status: status.to_owned(),
    };
    let record = counters
        .filter(|c| c.total > 0)
        .map(|c| -> Result<CanonicalRecord> {
            let record = CanonicalRecord {
                record_id: stable_record_id("codex", session_id.as_bytes(), b"session-total-v1")?,
                owner_device_id: owner_device_id.to_owned(),
                tool_id: "codex".into(),
                scope: UsageScope::DeviceLocal,
                precision: Precision::Aggregate,
                account_id: None,
                model_id: None,
                project_id: None,
                session_id: Some(session_key("codex", session_id.as_bytes())?),
                event_at_ms: last_event,
                usage: c.breakdown(),
                estimated_cost_microusd: None,
                price_version: None,
                is_deleted: false,
            };
            Ok(record.normalized()?)
        })
        .transpose()?;
    Ok(ScanOutcome {
        next,
        record,
        parsed_lines: parsed,
        reused_cursor: reused,
        pending_trailing_line: incomplete,
    })
}
