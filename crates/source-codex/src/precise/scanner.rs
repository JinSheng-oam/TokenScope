use super::{
    model::{BodyReader, PreciseScan, V2Cursor},
    usage::{clean_title, codex_project_metadata, legacy_difference, record, usage},
    MAX_BATCH, VERSION,
};
use crate::{
    bounded_line, event_ms, hash_path, mtime_ns, read_session_header_metadata,
    session_header_from_bytes, sha_tail, sha_tail_bytes, CodexError, Counters, Result,
};
use serde_json::Value;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufReader, Cursor, Seek, SeekFrom};
use std::path::Path;
use tokenscope_core_domain::session_key;
use tokenscope_local_store::SourceCheckpoint;

fn explicit_response_model(payload: &Value) -> Option<String> {
    let usage = payload.get("usage");
    [
        payload.get("model"),
        payload.get("model_id"),
        usage.and_then(|value| value.get("model")),
        usage.and_then(|value| value.get("model_id")),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .find(|model| {
        !model.trim().is_empty() && model.len() <= 256 && !model.chars().any(char::is_control)
    })
    .map(str::to_owned)
}

pub fn scan_precise(
    path: &Path,
    device_id: &str,
    prior: Option<&SourceCheckpoint>,
    observed_ms: i64,
) -> Result<PreciseScan> {
    scan_precise_entry(path, None, device_id, prior, observed_ms, None)
}

/// Scan an already decompressed `.jsonl.zst` rollout. Zstd frames are not
/// seekable, so the caller passes the decoded JSONL body.
pub fn scan_precise_body(
    path: &Path,
    body: &[u8],
    device_id: &str,
    prior: Option<&SourceCheckpoint>,
    observed_ms: i64,
) -> Result<PreciseScan> {
    scan_precise_entry(path, Some(body), device_id, prior, observed_ms, None)
}

/// Compaction writes a continuation rollout that keeps the original logical
/// session id but carries its own turns. Scan it under a derived source id so
/// checkpoints and record identities remain unique while records still belong
/// to the original logical session.
pub fn scan_precise_continuation(
    path: &Path,
    device_id: &str,
    prior: Option<&SourceCheckpoint>,
    observed_ms: i64,
    source_session_id: &str,
) -> Result<PreciseScan> {
    scan_precise_entry(
        path,
        None,
        device_id,
        prior,
        observed_ms,
        Some(source_session_id),
    )
}

pub fn scan_precise_continuation_body(
    path: &Path,
    body: &[u8],
    device_id: &str,
    prior: Option<&SourceCheckpoint>,
    observed_ms: i64,
    source_session_id: &str,
) -> Result<PreciseScan> {
    scan_precise_entry(
        path,
        Some(body),
        device_id,
        prior,
        observed_ms,
        Some(source_session_id),
    )
}

fn scan_precise_entry(
    path: &Path,
    body: Option<&[u8]>,
    device_id: &str,
    prior: Option<&SourceCheckpoint>,
    observed_ms: i64,
    source_session_override: Option<&str>,
) -> Result<PreciseScan> {
    // A rollout written across a schema switch cannot mix both projections,
    // but the cumulative counters span the whole file, so fall back to the
    // legacy delta projection and count every turn exactly once.
    match scan_precise_mode(
        path,
        body,
        device_id,
        prior,
        observed_ms,
        false,
        source_session_override,
    ) {
        Err(CodexError::Invalid("mixed legacy/modern coverage")) => scan_precise_mode(
            path,
            body,
            device_id,
            prior,
            observed_ms,
            true,
            source_session_override,
        ),
        other => other,
    }
}

fn scan_precise_mode(
    path: &Path,
    body: Option<&[u8]>,
    device_id: &str,
    prior: Option<&SourceCheckpoint>,
    observed_ms: i64,
    force_legacy: bool,
    source_session_override: Option<&str>,
) -> Result<PreciseScan> {
    let header = match body {
        Some(bytes) => session_header_from_bytes(bytes)?,
        None => read_session_header_metadata(path)?,
    };
    let header_session = header.id;
    let project_id = header
        .cwd
        .as_deref()
        .and_then(codex_project_metadata)
        .map(|(project_id, _)| project_id);
    let source_session = source_session_override
        .unwrap_or(&header_session)
        .to_owned();
    let key = session_key("codex", source_session.as_bytes())?;
    if let Some(cp) = prior {
        if cp.tool_id != "codex" || cp.source_key != key {
            return Err(CodexError::Invalid("checkpoint mismatch"));
        }
    }
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(CodexError::Invalid("not a regular file"));
    }
    let physical_size = meta.len();
    let size = body.map_or(physical_size, |bytes| bytes.len() as u64);
    let modified = mtime_ns(&meta)?;
    let path_identity = hash_path(path)?;
    let old = prior
        .filter(|x| x.parser_version == VERSION)
        .and_then(|x| serde_json::from_str::<V2Cursor>(&x.source_fingerprint).ok());
    let reuse = body.is_none()
        && match (prior, old.as_ref()) {
            (Some(cp), Some(saved)) => {
                cp.read_offset <= size
                    && saved.path_hash == path_identity
                    && (cp.read_offset != size || saved.mtime_ns == modified)
                    && saved.tail_hash == sha_tail(path, cp.read_offset)?
            }
            _ => false,
        };
    let mut offset = if reuse {
        prior.map_or(0, |x| x.read_offset)
    } else {
        0
    };
    let title_from_start = offset == 0;
    let mut old_mode = if reuse {
        old.as_ref().map(|x| x.mode.clone()).unwrap_or_default()
    } else {
        String::new()
    };
    if force_legacy {
        old_mode = "legacy".into();
    }
    let mut model = if reuse {
        old.as_ref().and_then(|x| x.model.clone())
    } else {
        None
    };
    let mut last = if reuse {
        old.as_ref().and_then(|x| x.cumulative)
    } else {
        None
    };
    let mut legacy_seq = if reuse {
        old.as_ref().map_or(0, |x| x.legacy_seq)
    } else {
        0
    };
    let mut response_seq = if reuse {
        old.as_ref().map_or(0, |x| x.response_seq)
    } else {
        0
    };
    let reader = match body {
        Some(bytes) => BodyReader::Memory(Cursor::new(bytes)),
        None => BodyReader::File(File::open(path)?),
    };
    let mut rd = BufReader::new(reader);
    rd.seek(SeekFrom::Start(offset))?;
    let mut legacy = Vec::new();
    let mut modern = Vec::new();
    let mut ids = HashSet::new();
    let mut first_legacy_before_modern = false;
    let mut saw_positive_legacy = false;
    let mut legacy_error = None;
    let mut last_count_ms: Option<i64> = None;
    let mut title: Option<String> = None;
    let mut parsed = 0;
    let mut pending = false;
    loop {
        let start = rd.stream_position()?;
        let line = bounded_line(&mut rd)?;
        if line.bytes.is_empty() && !line.oversized {
            break;
        }
        if !line.complete {
            pending = true;
            offset = start;
            break;
        }
        parsed += 1;
        if line.oversized {
            offset = rd.stream_position()?;
            continue;
        }
        let v: Value = serde_json::from_slice(&line.bytes)?;
        let typ = v.get("type").and_then(Value::as_str);
        let payload = v.get("payload");
        if typ == Some("turn_context") {
            if let Some(s) = payload.and_then(|p| p.get("model")).and_then(Value::as_str) {
                if s.len() > 256 || s.trim().is_empty() || s.chars().any(char::is_control) {
                    return Err(CodexError::Invalid("model metadata"));
                }
                model = Some(s.to_owned());
            }
        }
        if title_from_start
            && title.is_none()
            && typ == Some("response_item")
            && payload.and_then(|p| p.get("role")).and_then(Value::as_str) == Some("user")
        {
            let meta = payload.and_then(|p| p.get("internal_chat_message_metadata_passthrough"));
            let from_user = meta
                .and_then(|m| m.get("content_item_metadata"))
                .and_then(Value::as_array)
                .is_none_or(|items| {
                    items.iter().any(|item| {
                        item.get("provenance")
                            .and_then(|p| p.get("type"))
                            .and_then(Value::as_str)
                            == Some("user")
                    })
                });
            if let Some(text) = from_user
                .then(|| {
                    payload
                        .and_then(|p| p.get("content"))
                        .and_then(Value::as_array)
                        .and_then(|items| {
                            items
                                .iter()
                                .find_map(|i| i.get("text").and_then(Value::as_str))
                        })
                })
                .flatten()
            {
                title = clean_title(text);
            }
        }
        if typ == Some("token_usage_record") {
            let p = payload.ok_or(CodexError::Invalid("response payload"))?;
            let usage_value = p
                .get("usage")
                .ok_or(CodexError::Invalid("response usage"))?;
            let token = usage(usage_value)?;
            let ts = event_ms(&v)?.ok_or(CodexError::Invalid("missing response timestamp"))?;
            response_seq = response_seq
                .checked_add(1)
                .ok_or(CodexError::Invalid("response overflow"))?;
            let native = match p.get("response_id").and_then(Value::as_str) {
                Some(id) if !id.is_empty() && id.len() <= 512 => format!("response:{id}"),
                _ => format!("response-offset:{start}"),
            };
            let response_model = explicit_response_model(p).or_else(|| model.clone());
            let r = record(
                &source_session,
                &header_session,
                device_id,
                native.as_bytes(),
                response_model,
                project_id.clone(),
                ts,
                token,
            )?;
            if !ids.insert(r.record_id.clone()) {
                return Err(CodexError::Invalid("duplicate response identity"));
            }
            if modern.len() >= MAX_BATCH {
                return Err(CodexError::Limit("response batch > 50000"));
            }
            modern.push(r);
        }
        if typ == Some("event_msg")
            && payload.and_then(|p| p.get("type")).and_then(Value::as_str) == Some("token_count")
            && (force_legacy || (old_mode != "modern" && modern.is_empty()))
        {
            last_count_ms = event_ms(&v)?;
            if let Some(info) = payload.and_then(|p| p.get("info")) {
                if let Some(next) = Counters::from_info(info)? {
                    if last != Some(next) {
                        // A closed zstd archive is immutable: a decreasing
                        // cumulative counter is an internal context reset (the
                        // thread restarts its baseline), not a torn append.
                        let baseline = match last {
                            Some(prev) if body.is_some() && !next.monotonic_from(&prev) => None,
                            other => other,
                        };
                        match legacy_difference(next, baseline) {
                            Ok(delta) => {
                                if delta.total()? > 0 {
                                    if modern.is_empty() {
                                        saw_positive_legacy = true;
                                    }
                                    legacy_seq = legacy_seq
                                        .checked_add(1)
                                        .ok_or(CodexError::Invalid("legacy ordinal overflow"))?;
                                    let ts = event_ms(&v)?
                                        .ok_or(CodexError::Invalid("missing legacy timestamp"))?;
                                    let r = record(
                                        &source_session,
                                        &header_session,
                                        device_id,
                                        format!("legacy:{legacy_seq}").as_bytes(),
                                        model.clone(),
                                        project_id.clone(),
                                        ts,
                                        delta,
                                    )?;
                                    if legacy.len() >= MAX_BATCH {
                                        return Err(CodexError::Limit("legacy batch > 50000"));
                                    }
                                    legacy.push(r);
                                }
                                last = Some(next);
                            }
                            Err(e) => {
                                legacy_error = Some(e);
                            }
                        }
                    }
                }
            }
        }
        if !modern.is_empty() && saw_positive_legacy {
            first_legacy_before_modern = true;
        }
        offset = rd.stream_position()?;
    }
    let after = fs::metadata(path)?;
    if after.len() != physical_size || mtime_ns(&after)? != modified || offset > size {
        return Err(CodexError::Invalid("file changed during scan"));
    }
    // An incomplete, rewritten source is not authoritative for deletion.
    // Preserve the prior snapshot and retry once the writer completes the line.
    if pending && !reuse && prior.is_some() {
        return Err(CodexError::PendingHeader);
    }
    // New modern records in a previously legacy-tracked file require complete
    // replacement of the old delta projection, not a mixture of both schemas.
    if reuse && old_mode == "legacy" && !modern.is_empty() {
        let rebuilt = scan_precise_entry(
            path,
            body,
            device_id,
            None,
            observed_ms,
            source_session_override,
        )?;
        if rebuilt.pending_line {
            return Err(CodexError::PendingHeader);
        }
        return Ok(rebuilt);
    }
    let mode = if force_legacy {
        "legacy"
    } else if old_mode == "modern" || !modern.is_empty() {
        "modern"
    } else {
        "legacy"
    };
    if mode == "legacy" {
        if let Some(err) = legacy_error {
            return Err(err);
        }
    } else if first_legacy_before_modern {
        // A mixed-format rollout with nonzero usage before the first response
        // cannot be proven complete by simply ignoring cumulative snapshots.
        return Err(CodexError::Invalid("mixed legacy/modern coverage"));
    }
    let records = if force_legacy {
        // Cumulative deltas cover every turn up to the final snapshot; any
        // response recorded after that snapshot has no delta yet and is added
        // explicitly so the tail is not lost.
        let cutoff = last_count_ms;
        let mut merged = legacy;
        merged.extend(
            modern
                .into_iter()
                .filter(|r| match (cutoff, r.event_at_ms) {
                    (Some(cutoff), Some(at)) => at > cutoff,
                    _ => true,
                }),
        );
        merged
    } else if mode == "modern" {
        modern
    } else {
        legacy
    };
    // A newly replaced header-only file is not proof that previously measured
    // responses were deleted. Keep last-known-good until usage is observable.
    if !reuse && prior.is_some() && records.is_empty() {
        return Err(CodexError::PendingHeader);
    }
    if records.len() > MAX_BATCH {
        return Err(CodexError::Limit("batch too large"));
    }
    old_mode = mode.to_owned();
    let next_time = prior.map_or(observed_ms, |p| {
        observed_ms.max(p.last_scan_ms.saturating_add(1))
    });
    let cursor = V2Cursor {
        path_hash: path_identity,
        tail_hash: match body {
            Some(bytes) => {
                let end =
                    usize::try_from(offset).map_err(|_| CodexError::Invalid("scan offset"))?;
                if end > bytes.len() {
                    return Err(CodexError::Invalid("scan offset beyond body"));
                }
                sha_tail_bytes(&bytes[..end])
            }
            None => sha_tail(path, offset)?,
        },
        mtime_ns: modified,
        mode: old_mode.clone(),
        model,
        cumulative: if mode == "modern" { None } else { last },
        legacy_seq,
        response_seq,
        compressed: body.is_some(),
        physical_size,
        scan_size: size,
    };
    let fingerprint = serde_json::to_string(&cursor)?;
    if fingerprint.len() > 1024 {
        return Err(CodexError::Limit("checkpoint fingerprint"));
    }
    let next = SourceCheckpoint {
        tool_id: "codex".into(),
        source_key: key,
        source_fingerprint: fingerprint,
        read_offset: offset,
        parser_version: VERSION,
        last_scan_ms: next_time,
        last_complete_scan_ms: if pending {
            prior.and_then(|x| x.last_complete_scan_ms)
        } else {
            Some(next_time)
        },
        last_status: if pending { "partial" } else { "complete" }.into(),
    };
    Ok(PreciseScan {
        next,
        records,
        full_reconcile: !reuse,
        parsed_lines: parsed,
        pending_line: pending,
        mode: old_mode,
        title,
    })
}
