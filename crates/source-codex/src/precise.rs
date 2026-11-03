//! M0-05 precision collector: per-response native usage is authoritative.
//! Legacy sessions use monotonic cumulative deltas. A schema switch triggers
//! a full, source-scoped reconciliation with explicit tombstones.
use super::{
    bounded_line, decompress_rollout, discover_codex_files, event_ms, hash_path,
    is_zstd_rollout, mtime_ns, now_ms, read_session_header_metadata,
    session_header_from_bytes, sha_tail, sha_tail_bytes, CodexError, Counters, Result, MAX_FILES,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use tokenscope_core_domain::{
    project_key, session_key, stable_record_id, CanonicalRecord, Precision, TokenBreakdown,
    UsageScope,
};
use tokenscope_local_store::{LocalStore, SourceCheckpoint};

const VERSION: u32 = 6;
const MAX_BATCH: usize = 50_000; // explicit upper bound pending true streaming decoder
const MAX_SESSION_INDEX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SESSION_INDEX_LINE: usize = 1024 * 1024;
const MAX_SESSION_INDEX_ROWS: usize = 100_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct V2Cursor {
    path_hash: String,
    tail_hash: String,
    mtime_ns: u128,
    mode: String, // "legacy" or "modern"
    model: Option<String>,
    cumulative: Option<Counters>,
    legacy_seq: u64,
    response_seq: u64,
    /// The rollout arrived as a zstd frame; its offsets are not seekable.
    #[serde(default)]
    compressed: bool,
    #[serde(default)]
    physical_size: u64,
    #[serde(default)]
    scan_size: u64,
}

enum BodyReader<'a> {
    File(File),
    Memory(Cursor<&'a [u8]>),
}

impl Read for BodyReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::File(file) => file.read(buf),
            Self::Memory(cursor) => cursor.read(buf),
        }
    }
}

impl Seek for BodyReader<'_> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        match self {
            Self::File(file) => file.seek(pos),
            Self::Memory(cursor) => cursor.seek(pos),
        }
    }
}

#[derive(Debug)]
pub struct PreciseScan {
    pub next: SourceCheckpoint,
    pub records: Vec<CanonicalRecord>,
    pub full_reconcile: bool,
    pub parsed_lines: usize,
    pub pending_line: bool,
    pub mode: String,
    /// First human prompt, used only as a local display label.
    pub title: Option<String>,
}

/// Turns the first user prompt into a short display label by dropping injected
/// XML-ish blocks (for example `<external_codex_apps_open_page>…`).
fn clean_title(raw: &str) -> Option<String> {
    let mut out = String::new();
    let mut depth = 0usize;
    for ch in raw.chars() {
        match ch {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim();
    // Injected context (page state, agent instructions, environment blobs)
    // must never become a session label.
    if trimmed.is_empty()
        || trimmed.starts_with('{')
        || trimmed.starts_with('#')
        || trimmed.starts_with('[')
        || trimmed.contains("AGENTS.md instructions")
        || trimmed.starts_with("The following is the Codex agent history")
        // Legacy environment blobs: "D:\\path powershell 2026-04-02 Asia/Shanghai".
        || (trimmed.chars().nth(1) == Some(':') && trimmed.contains("Asia/"))
    {
        return None;
    }
    let text: String = trimmed.chars().take(80).collect();
    if text.chars().any(char::is_control) {
        return None;
    }
    Some(text)
}

fn codex_project_metadata(cwd: &str) -> Option<(String, String)> {
    let path = Path::new(cwd);
    if cwd.len() > 4096 || !path.is_absolute() || cwd.chars().any(char::is_control) {
        return None;
    }
    let mut root = path;
    for ancestor in path.ancestors().take(64) {
        if ancestor.join(".git").exists() {
            root = ancestor;
            break;
        }
    }
    let identity = root.to_string_lossy().trim().replace('\\', "/");
    let identity = if cfg!(windows) {
        identity.to_lowercase()
    } else {
        identity
    };
    let project_id = project_key("codex", identity.as_bytes()).ok()?;
    let name = root.file_name().and_then(|value| value.to_str())?.trim();
    if name.is_empty()
        || name.chars().count() > 120
        || name.chars().any(char::is_control)
        || name.contains('/')
        || name.contains('\\')
    {
        return None;
    }
    Some((project_id, name.to_owned()))
}

fn usage(v: &Value) -> Result<TokenBreakdown> {
    let number = |name: &str, required: bool| -> Result<u64> {
        match v.get(name) {
            None if !required => Ok(0),
            Some(x) => x
                .as_u64()
                .ok_or(CodexError::Invalid("invalid usage counter")),
            None => Err(CodexError::Invalid("missing usage counter")),
        }
    };
    let input = number("input_tokens", true)?;
    let cache_read = number("cached_input_tokens", false)?;
    let cache_write = number("cache_write_input_tokens", false)?;
    let output = number("output_tokens", true)?;
    let sum = input
        .checked_add(output)
        .ok_or(CodexError::Invalid("usage overflow"))?;
    let total = match v.get("total_tokens") {
        Some(x) => x
            .as_u64()
            .ok_or(CodexError::Invalid("invalid total_tokens"))?,
        None => sum,
    };
    if total < sum
        || total > i64::MAX as u64
        || cache_read
            .checked_add(cache_write)
            .is_none_or(|c| c > input)
    {
        return Err(CodexError::Invalid("usage buckets inconsistent"));
    }
    let reasoning = v
        .get("reasoning_output_tokens")
        .map(|x| x.as_u64().ok_or(CodexError::Invalid("invalid reasoning")))
        .transpose()?;
    if reasoning.is_some_and(|r| r > output) {
        return Err(CodexError::Invalid("reasoning exceeds output"));
    }
    Ok(TokenBreakdown {
        input_uncached_tokens: input - cache_read - cache_write,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        output_tokens: output,
        unclassified_tokens: total - sum,
        reasoning_tokens: reasoning,
    })
}

fn legacy_difference(current: Counters, previous: Option<Counters>) -> Result<TokenBreakdown> {
    let p = previous.unwrap_or_default();
    if !current.monotonic_from(&p) {
        return Err(CodexError::CounterRegression);
    }
    let input = current.input - p.input;
    let cached = current.cached - p.cached;
    let output = current.output - p.output;
    let total = current.total - p.total;
    if cached > input
        || total
            < input
                .checked_add(output)
                .ok_or(CodexError::Invalid("delta overflow"))?
    {
        return Err(CodexError::Invalid("cumulative delta inconsistent"));
    }
    let reasoning = match (current.reasoning, previous.and_then(|x| x.reasoning)) {
        (Some(n), Some(old)) => Some(n.checked_sub(old).ok_or(CodexError::CounterRegression)?),
        (Some(n), None) if previous.is_none() => Some(n),
        _ => None,
    };
    if reasoning.is_some_and(|r| r > output) {
        return Err(CodexError::Invalid("delta reasoning"));
    }
    Ok(TokenBreakdown {
        input_uncached_tokens: input - cached,
        cache_read_tokens: cached,
        cache_write_tokens: 0,
        output_tokens: output,
        unclassified_tokens: total - input - output,
        reasoning_tokens: reasoning,
    })
}

fn record(
    session: &str,
    device: &str,
    native_key: &[u8],
    model: Option<String>,
    project_id: Option<String>,
    at: i64,
    tokens: TokenBreakdown,
) -> Result<CanonicalRecord> {
    let rec = CanonicalRecord {
        record_id: stable_record_id("codex", session.as_bytes(), native_key)?,
        owner_device_id: device.to_owned(),
        tool_id: "codex".into(),
        scope: UsageScope::DeviceLocal,
        precision: if tokens.unclassified_tokens == 0 {
            Precision::Exact
        } else {
            Precision::Aggregate
        },
        account_id: None,
        model_id: model,
        project_id,
        session_id: Some(session_key("codex", session.as_bytes())?),
        event_at_ms: Some(at),
        usage: tokens,
        estimated_cost_microusd: None,
        price_version: None,
        is_deleted: false,
    };
    Ok(rec.normalized()?)
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

/// Compaction writes a continuation rollout that keeps the original session id
/// but carries its own turns. Scan it under a derived session id so both files
/// count and legacy ordinals cannot collide across the pair.
pub fn scan_precise_continuation(
    path: &Path,
    device_id: &str,
    prior: Option<&SourceCheckpoint>,
    observed_ms: i64,
    session_id: &str,
) -> Result<PreciseScan> {
    scan_precise_entry(path, None, device_id, prior, observed_ms, Some(session_id))
}

pub fn scan_precise_continuation_body(
    path: &Path,
    body: &[u8],
    device_id: &str,
    prior: Option<&SourceCheckpoint>,
    observed_ms: i64,
    session_id: &str,
) -> Result<PreciseScan> {
    scan_precise_entry(
        path,
        Some(body),
        device_id,
        prior,
        observed_ms,
        Some(session_id),
    )
}

fn scan_precise_entry(
    path: &Path,
    body: Option<&[u8]>,
    device_id: &str,
    prior: Option<&SourceCheckpoint>,
    observed_ms: i64,
    session_override: Option<&str>,
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
        session_override,
    ) {
        Err(CodexError::Invalid("mixed legacy/modern coverage")) => scan_precise_mode(
            path,
            body,
            device_id,
            prior,
            observed_ms,
            true,
            session_override,
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
    session_override: Option<&str>,
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
    let session = session_override.unwrap_or(&header_session).to_owned();
    let key = session_key("codex", session.as_bytes())?;
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
            let r = record(
                &session,
                device_id,
                native.as_bytes(),
                model.clone(),
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
                                        &session,
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
        let rebuilt =
            scan_precise_entry(path, body, device_id, None, observed_ms, session_override)?;
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
        merged.extend(modern.into_iter().filter(|r| match (cutoff, r.event_at_ms) {
            (Some(cutoff), Some(at)) => at > cutoff,
            _ => true,
        }));
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
                let end = usize::try_from(offset)
                    .map_err(|_| CodexError::Invalid("scan offset"))?;
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

#[derive(Debug, Default)]
pub struct PreciseReport {
    pub files: usize,
    pub inserted: usize,
    pub revised: usize,
    pub unchanged: usize,
    pub lines: usize,
    pub pending: usize,
    pub reconciled: usize,
    pub unsupported: usize,
    pub missing_source_tombstones: usize,
}

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
                store.upsert_session_name(key, "codex", name, updated_at_ms)?;
                updated += 1;
            } else {
                store.clear_session_name(key, "codex")?;
            }
        }
    }
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
                eprintln!(
                    "TokenScope: codex skipped {}: {e}",
                    file.display()
                );
                report.unsupported += 1;
                continue;
            }
            Err(e) => return Err(e),
        };
        let session = header.id;
        if let Some((project_id, project_name)) = header
            .cwd
            .as_deref()
            .and_then(codex_project_metadata)
        {
            store.upsert_project_name(&project_id, &project_name, now_ms()?)?;
        }
        let mut continuation: Option<String> = None;
        let base_key = session_key("codex", session.as_bytes())?;
        let key = if sessions.insert(base_key.clone()) {
            base_key
        } else if tolerant {
            // A compaction continuation keeps the original session id. Give it
            // a derived id so both rollouts are counted instead of dropping the
            // newer file's turns.
            let digest = hash_path(&file)?;
            let derived = format!("{session}#{}", &digest[..8.min(digest.len())]);
            let derived_key = session_key("codex", derived.as_bytes())?;
            if !sessions.insert(derived_key.clone()) {
                report.unsupported += 1;
                continue;
            }
            eprintln!(
                "TokenScope: codex continuation {} tracked as {derived}",
                file.display()
            );
            continuation = Some(derived);
            derived_key
        } else {
            return Err(CodexError::Invalid("duplicate session sources"));
        };
        session_name_keys
            .entry(session.clone())
            .or_default()
            .push(key.clone());
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
                eprintln!(
                    "TokenScope: codex skipped {}: {e}",
                    file.display()
                );
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
                    eprintln!(
                        "TokenScope: codex skipped {}: {e}",
                        file.display()
                    );
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
            store.upsert_session_name(&scan.next.source_key, "codex", name, now_ms()?)?;
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
    let inventory_roots_ready = home.join("sessions").is_dir()
        || home.join("archived_sessions").is_dir();
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
