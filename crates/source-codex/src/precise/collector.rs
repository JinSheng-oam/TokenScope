use super::usage::codex_project_metadata;
use super::{
    model::{PreciseReport, PreciseScan, V2Cursor},
    scanner::{
        scan_precise, scan_precise_body, scan_precise_continuation, scan_precise_continuation_body,
    },
    MAX_SESSION_INDEX_BYTES, MAX_SESSION_INDEX_LINE, MAX_SESSION_INDEX_ROWS, VERSION,
};
use crate::{
    decompress_rollout, discover_codex_files, is_zstd_rollout, mtime_ns, now_ms,
    read_session_header_metadata, CodexError, Result, MAX_FILES,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;
use tokenscope_core_domain::session_key;
use tokenscope_local_store::{LocalStore, SourceCheckpoint};

pub fn collect_codex_precise(
    home: &Path,
    store: &mut LocalStore,
    device_id: &str,
) -> Result<PreciseReport> {
    collect_codex_precise_with_policy(home, store, device_id, false)
}

fn scan_file_for(
    file: &Path,
    device_id: &str,
    prior: Option<&SourceCheckpoint>,
    observed_ms: i64,
    continuation: Option<&str>,
    body: Option<&[u8]>,
) -> Result<PreciseScan> {
    match (continuation, body) {
        (Some(session_id), Some(body)) => {
            scan_precise_continuation_body(file, body, device_id, prior, observed_ms, session_id)
        }
        (Some(session_id), None) => {
            scan_precise_continuation(file, device_id, prior, observed_ms, session_id)
        }
        (None, Some(body)) => scan_precise_body(file, body, device_id, prior, observed_ms),
        (None, None) => scan_precise(file, device_id, prior, observed_ms),
    }
}

fn import_codex_session_names(
    home: &Path,
    store: &mut LocalStore,
    source_keys: &HashMap<String, Vec<String>>,
) -> Result<usize> {
    let path = home.join("session_index.jsonl");
    let before = match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata,
        Ok(_) => return Ok(0),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    if before.len() > MAX_SESSION_INDEX_BYTES {
        return Err(CodexError::Limit("Codex session index size"));
    }

    let mut reader = BufReader::new(File::open(&path)?);
    let mut line = Vec::new();
    let mut names = HashMap::<String, Option<String>>::new();
    let mut rows = 0usize;
    loop {
        line.clear();
        let bytes = reader.read_until(b'\n', &mut line)?;
        if bytes == 0 {
            break;
        }
        rows = rows.saturating_add(1);
        if rows > MAX_SESSION_INDEX_ROWS || line.len() > MAX_SESSION_INDEX_LINE {
            return Err(CodexError::Limit("Codex session index rows"));
        }
        let has_newline = line.last() == Some(&b'\n');
        let value: Value = match serde_json::from_slice(&line) {
            Ok(value) => value,
            Err(_) if !has_newline => break,
            Err(error) => return Err(error.into()),
        };
        let Some(id) = value.get("id").and_then(Value::as_str) else {
            continue;
        };
        if id.len() < 8
            || id.len() > 128
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            continue;
        }
        let Some(raw_name) = value.get("thread_name").and_then(Value::as_str) else {
            continue;
        };
        let normalized = raw_name.split_whitespace().collect::<Vec<_>>().join(" ");
        let name = if normalized.is_empty() {
            None
        } else if normalized.chars().any(char::is_control) {
            continue;
        } else {
            Some(normalized.chars().take(200).collect::<String>())
        };
        // The index is append-only; the last row for an id carries its latest title.
        names.insert(id.to_owned(), name);
    }

    let after = fs::symlink_metadata(&path)?;
    if !after.is_file()
        || after.file_type().is_symlink()
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Ok(0);
    }

    let updated_at_ms = now_ms()?;
    let mut updated = 0usize;
    let mut batch = Vec::new();
    for (id, name) in names {
        let fallback_key;
        let keys = match source_keys.get(&id) {
            Some(keys) => keys.as_slice(),
            None => {
                fallback_key = session_key("codex", id.as_bytes())?;
                std::slice::from_ref(&fallback_key)
            }
        };
        for key in keys {
            if let Some(name) = name.as_deref() {
                batch.push((key.clone(), Some(name.to_owned())));
                updated += 1;
            } else {
                batch.push((key.clone(), None));
            }
        }
    }
    store.upsert_session_names("codex", &batch, updated_at_ms)?;
    Ok(updated)
}

/// Desktop collection policy: a single unreadable or unprovable rollout must
/// not zero out every other source. Such files are counted in `unsupported`
/// and skipped; their previously committed projection stays untouched.
pub fn collect_codex_precise_tolerant(
    home: &Path,
    store: &mut LocalStore,
    device_id: &str,
) -> Result<PreciseReport> {
    collect_codex_precise_with_policy(home, store, device_id, true)
}

fn collect_codex_precise_with_policy(
    home: &Path,
    store: &mut LocalStore,
    device_id: &str,
    tolerant: bool,
) -> Result<PreciseReport> {
    let mut report = PreciseReport::default();
    let mut sessions = HashSet::new();
    let mut session_name_keys = HashMap::<String, Vec<String>>::new();
    let files = discover_codex_files(home)?;
    let mut groups = HashMap::<String, Vec<std::path::PathBuf>>::new();
    for file in &files {
        if let Ok(header) = read_session_header_metadata(file) {
            groups.entry(header.id).or_default().push(file.clone());
        }
    }
    let mut grouped = HashSet::new();
    if files.len() > MAX_FILES {
        return Err(CodexError::Limit("too many files"));
    }
    for file in files {
        let header = match read_session_header_metadata(&file) {
            Ok(v) => v,
            Err(CodexError::PendingHeader) => {
                report.pending += 1;
                continue;
            }
            Err(e) if tolerant => {
                eprintln!("TokenScope: codex skipped {}: {e}", file.display());
                report.unsupported += 1;
                continue;
            }
            Err(e) => return Err(e),
        };
        if let Some((project_id, project_name)) =
            header.cwd.as_deref().and_then(codex_project_metadata)
        {
            store.upsert_project_name(&project_id, &project_name, now_ms()?)?;
        }
        let session = header.id;
        if let Some(group) = groups.get(&session).filter(|g| g.len() > 1) {
            if !grouped.insert(session.clone()) {
                continue;
            }
            let key = session_key("codex", session.as_bytes())?;
            sessions.insert(key.clone());
            session_name_keys
                .entry(session.clone())
                .or_default()
                .push(key);
            match super::grouped::collect(group, &session, store, device_id) {
                Ok(result) => {
                    report.files += result.files;
                    report.reconciled += result.reconciled;
                    report.inserted += result.inserted;
                    report.revised += result.revised;
                    report.unchanged += result.unchanged;
                    report.lines += result.lines;
                }
                Err(CodexError::PendingHeader) => report.pending += 1,
                Err(error) if tolerant => {
                    eprintln!("TokenScope grouped rollout skipped: {error}");
                    report.unsupported += 1;
                }
                Err(error) => return Err(error),
            }
            continue;
        }
        let continuation: Option<String> = None;
        let base_key = session_key("codex", session.as_bytes())?;
        let key = base_key.clone();
        sessions.insert(key.clone());
        let title_keys = session_name_keys.entry(session.clone()).or_default();
        if !title_keys.contains(&base_key) {
            title_keys.push(base_key.clone());
        }
        let prior = store.checkpoint("codex", &key)?;
        // Compressed rollouts are immutable archives. Their fingerprint is
        // enough to prove nothing changed; decompressing 100 MiB every cycle
        // would be wasteful.
        if is_zstd_rollout(&file) {
            if let Some(cp) = prior.as_ref() {
                if cp.parser_version == VERSION {
                    if let Ok(saved) = serde_json::from_str::<V2Cursor>(&cp.source_fingerprint) {
                        let meta = fs::metadata(&file)?;
                        if saved.compressed
                            && saved.physical_size == meta.len()
                            && saved.mtime_ns == mtime_ns(&meta)?
                            && saved.scan_size == cp.read_offset
                        {
                            report.files += 1;
                            report.unchanged += 1;
                            continue;
                        }
                    }
                }
            }
        }
        let body = if is_zstd_rollout(&file) {
            match decompress_rollout(&file) {
                Ok(body) => Some(body),
                Err(e) if tolerant => {
                    eprintln!("TokenScope: codex skipped {}: {e}", file.display());
                    report.unsupported += 1;
                    continue;
                }
                Err(e) => return Err(e),
            }
        } else {
            None
        };
        let scan = match scan_file_for(
            &file,
            device_id,
            prior.as_ref(),
            now_ms()?,
            continuation.as_deref(),
            body.as_deref(),
        ) {
            Ok(v) => v,
            Err(CodexError::PendingHeader) => {
                report.pending += 1;
                continue;
            }
            Err(e) if tolerant => {
                eprintln!("TokenScope: codex skipped {}: {e}", file.display());
                report.unsupported += 1;
                continue;
            }
            Err(e) => return Err(e),
        };
        // A large append-only delta cannot be sent to the 2,000-record
        // incremental API, and it must NOT be treated as a complete source.
        // Re-read the entire file before generating any missing-record tombstones.
        let scan = if !scan.full_reconcile && scan.records.len() > 2000 {
            let mut rebuilt = match scan_file_for(
                &file,
                device_id,
                None,
                now_ms()?,
                continuation.as_deref(),
                body.as_deref(),
            ) {
                Ok(v) => v,
                Err(e) if tolerant => {
                    eprintln!("TokenScope: codex skipped {}: {e}", file.display());
                    report.unsupported += 1;
                    continue;
                }
                Err(e) => return Err(e),
            };
            if rebuilt.pending_line {
                return Err(CodexError::PendingHeader);
            }
            if let Some(old) = prior.as_ref() {
                if rebuilt.next.last_scan_ms <= old.last_scan_ms {
                    rebuilt.next.last_scan_ms = old.last_scan_ms.saturating_add(1);
                    rebuilt.next.last_complete_scan_ms = Some(rebuilt.next.last_scan_ms);
                }
            }
            rebuilt
        } else {
            scan
        };
        if let Some(name) = scan.title.as_deref() {
            store.upsert_session_name(&base_key, "codex", name, now_ms()?)?;
        }
        let (inserted, revised, unchanged) = if scan.full_reconcile {
            if scan.pending_line && prior.is_some() {
                return Err(CodexError::PendingHeader);
            }
            if scan.pending_line {
                // Partial brand-new sources are append-only; no missing-record deletion.
                if scan.records.len() > 2000 {
                    return Err(CodexError::PendingHeader);
                }
                let result = store.apply_scan(
                    prior.as_ref(),
                    &scan.next,
                    &scan.records,
                    scan.next.last_scan_ms,
                    false,
                )?;
                (result.inserted, result.revised, result.unchanged)
            } else {
                let result = store.reconcile_complete_source(
                    prior.as_ref(),
                    &scan.next,
                    scan.records.into_iter(),
                    scan.next.last_scan_ms,
                )?;
                report.reconciled += 1;
                (
                    result.applied.inserted,
                    result.applied.revised + result.tombstoned,
                    result.applied.unchanged,
                )
            }
        } else {
            let result = store.apply_scan(
                prior.as_ref(),
                &scan.next,
                &scan.records,
                scan.next.last_scan_ms,
                false,
            )?;
            (result.inserted, result.revised, result.unchanged)
        };
        report.files += 1;
        report.inserted += inserted;
        report.revised += revised;
        report.unchanged += unchanged;
        report.lines += scan.parsed_lines;
        if scan.pending_line {
            report.pending += 1;
        }
    }
    let inventory_roots_ready =
        home.join("sessions").is_dir() || home.join("archived_sessions").is_dir();
    if inventory_roots_ready && report.pending == 0 && report.unsupported == 0 {
        report.missing_source_tombstones =
            store.reconcile_missing_sources("codex", &sessions, now_ms()?)?;
    }
    if let Err(error) = import_codex_session_names(home, store, &session_name_keys) {
        if tolerant {
            eprintln!("TokenScope: codex session titles skipped: {error}");
        } else {
            return Err(error);
        }
    }
    Ok(report)
}
