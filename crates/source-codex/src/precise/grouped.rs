//! Reconcile all rollouts of a logical thread as one authoritative source.
use super::{scanner::scan_precise_body, PreciseReport, VERSION};
use crate::{decompress_rollout, is_zstd_rollout, mtime_ns, now_ms, CodexError, Result};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader, Cursor},
    path::PathBuf,
};
use tokenscope_core_domain::session_key;
use tokenscope_local_store::LocalStore;

pub(super) fn collect(
    files: &[PathBuf],
    session: &str,
    store: &mut LocalStore,
    device: &str,
) -> Result<PreciseReport> {
    let key = session_key("codex", session.as_bytes())?;
    let prior = store.checkpoint("codex", &key)?;
    let mut paths = files.to_vec();
    paths.sort_by_key(|p| p.file_name().map(|s| s.to_owned()));
    let signature = group_signature(&paths)?;
    if prior.as_ref().is_some_and(|p| {
        p.parser_version == VERSION
            && serde_json::from_str::<serde_json::Value>(&p.source_fingerprint)
                .ok()
                .and_then(|v| {
                    v.get("group_signature")
                        .and_then(|x| x.as_str())
                        .map(str::to_owned)
                })
                .as_deref()
                == Some(&signature)
    }) {
        return Ok(PreciseReport {
            files: files.len(),
            unchanged: files.len(),
            ..Default::default()
        });
    }
    let mut events = Vec::new();
    let mut header = None;
    let mut responses = HashMap::new();
    let mut seen = HashSet::new();
    let mut size = 0usize;
    for path in &paths {
        let before = fs::metadata(path)?;
        let mut reader: Box<dyn BufRead> = if is_zstd_rollout(path) {
            Box::new(BufReader::new(Cursor::new(decompress_rollout(path)?)))
        } else {
            Box::new(BufReader::new(fs::File::open(path)?))
        };
        loop {
            let line = crate::bounded_line(&mut reader)?;
            if line.bytes.is_empty() && !line.oversized {
                break;
            }
            if !line.complete {
                return Err(CodexError::PendingHeader);
            }
            if line.oversized {
                continue;
            }
            let v: serde_json::Value = serde_json::from_slice(&line.bytes)?;
            let typ = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
            if !(matches!(typ, "session_meta" | "turn_context" | "token_usage_record")
                || (typ == "event_msg"
                    && v.pointer("/payload/type").and_then(|x| x.as_str()) == Some("token_count"))
                || (typ == "response_item"
                    && v.pointer("/payload/role").and_then(|x| x.as_str()) == Some("user")))
            {
                continue;
            }
            if v.get("type").and_then(|x| x.as_str()) == Some("session_meta") && header.is_none() {
                header = Some(v.clone());
            }
            if v.get("type").and_then(|x| x.as_str()) == Some("token_usage_record") {
                if let Some(id) = v.pointer("/payload/response_id").and_then(|x| x.as_str()) {
                    let payload = v.get("payload").cloned().unwrap_or_default();
                    if let Some(old) = responses.insert(id.to_owned(), payload.clone()) {
                        if old != payload {
                            return Err(CodexError::Invalid("conflicting rollout response"));
                        }
                        continue;
                    }
                }
            }
            let encoded = serde_json::to_string(&v)?;
            if seen.insert(encoded.clone()) {
                size = size.saturating_add(encoded.len());
                if size > 128 * 1024 * 1024 {
                    return Err(CodexError::Limit("group usage metadata size"));
                }
                events.push((crate::event_ms(&v)?.unwrap_or(0), encoded));
            }
        }
        let after = fs::metadata(path)?;
        if before.len() != after.len() || mtime_ns(&before)? != mtime_ns(&after)? {
            return Err(CodexError::PendingHeader);
        }
    }
    if group_signature(&paths)? != signature {
        return Err(CodexError::PendingHeader);
    }
    events.sort_by(|a, b| a.0.cmp(&b.0));
    let mut body = serde_json::to_vec(&header.ok_or(CodexError::PendingHeader)?)?;
    body.push(b'\n');
    for (_, line) in events {
        body.extend_from_slice(line.as_bytes());
        body.push(b'\n');
    }
    let mut scan = scan_precise_body(&paths[0], &body, device, prior.as_ref(), now_ms()?)?;
    let mut cursor: serde_json::Value = serde_json::from_str(&scan.next.source_fingerprint)?;
    cursor["group_signature"] = signature.into();
    scan.next.source_fingerprint = serde_json::to_string(&cursor)?;
    let result = store.reconcile_complete_session_source(
        prior.as_ref(),
        &scan.next,
        scan.records,
        scan.next.last_scan_ms,
    )?;
    if let Some(title) = scan.title {
        store.upsert_session_name(&key, "codex", &title, now_ms()?)?;
    }
    Ok(PreciseReport {
        files: files.len(),
        inserted: result.applied.inserted,
        revised: result.applied.revised + result.tombstoned,
        unchanged: result.applied.unchanged,
        lines: scan.parsed_lines,
        reconciled: 1,
        ..Default::default()
    })
}

fn group_signature(paths: &[PathBuf]) -> Result<String> {
    let signature = paths
        .iter()
        .map(|p| {
            let m = fs::metadata(p)?;
            Ok(format!(
                "{}:{}:{}",
                p.file_name().unwrap_or_default().to_string_lossy(),
                m.len(),
                mtime_ns(&m)?
            ))
        })
        .collect::<Result<Vec<_>>>()?
        .join("|");
    use sha2::{Digest, Sha256};
    Ok(format!("{:x}", Sha256::digest(signature.as_bytes())))
}
