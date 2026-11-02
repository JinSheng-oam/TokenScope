//! M0-04: bounded, resumable Codex JSONL session-total collector.
//!
//! - token_count.info.total_token_usage is CUMULATIVE, never summed across lines.
//! - last_token_usage is deliberately ignored (may repeat on rate-limit updates).
//! - one revisable record per logical session; file moves do not create duplicates.
//! - a complete malformed line or counter regression FAILS CLOSED.
//! - truncated trailing lines are left unread for the next scan.
//! - no prompt, response, source code, raw path, token or cookie is persisted.
//! - this milestone does NOT claim accurate per-day/per-model attribution.

use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokenscope_core_domain::{
    session_key, stable_record_id, CanonicalRecord, Precision, TokenBreakdown, UsageScope,
};
use tokenscope_local_store::{ApplyReport, LocalStore, SourceCheckpoint};

pub const MODULE_ID: &str = "source-codex";
pub const PARSER_VERSION: u32 = 1;
const MAX_LINE: usize = 4 * 1024 * 1024;
const MAX_FILES: usize = 20_000;
const TAIL_WINDOW: usize = 256;

#[derive(Debug)]
pub enum CodexError {
    Io(io::Error),
    Json(serde_json::Error),
    Store(tokenscope_local_store::StoreError),
    Domain(tokenscope_core_domain::DomainError),
    Invalid(&'static str),
    CounterRegression,
    PendingHeader,
    Limit(&'static str),
}
impl std::fmt::Display for CodexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // No file paths, JSON payloads or user content in errors.
        match self {
            Self::Io(e) => write!(f, "I/O error ({:?})", e.kind()),
            Self::Json(_) => write!(f, "invalid JSONL data"),
            Self::Store(e) => write!(f, "local store: {e}"),
            Self::Domain(e) => write!(f, "domain: {e}"),
            Self::Invalid(s) => write!(f, "invalid Codex record ({s})"),
            Self::CounterRegression => {
                write!(f, "cumulative token counter regressed; scan not committed")
            }
            Self::PendingHeader => write!(f, "session_meta is still being written"),
            Self::Limit(s) => write!(f, "resource limit ({s})"),
        }
    }
}
impl std::error::Error for CodexError {}
impl From<io::Error> for CodexError {
    fn from(v: io::Error) -> Self {
        Self::Io(v)
    }
}
impl From<serde_json::Error> for CodexError {
    fn from(v: serde_json::Error) -> Self {
        Self::Json(v)
    }
}
impl From<tokenscope_local_store::StoreError> for CodexError {
    fn from(v: tokenscope_local_store::StoreError) -> Self {
        Self::Store(v)
    }
}
impl From<tokenscope_core_domain::DomainError> for CodexError {
    fn from(v: tokenscope_core_domain::DomainError) -> Self {
        Self::Domain(v)
    }
}
type Result<T> = std::result::Result<T, CodexError>;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct Counters {
    input: u64,
    cached: u64,
    output: u64,
    reasoning: Option<u64>,
    total: u64,
}
impl Counters {
    fn from_info(info: &Value) -> Result<Option<Self>> {
        let Some(raw) = info.get("total_token_usage") else {
            return Ok(None);
        };
        if raw.is_null() {
            return Ok(None);
        }
        if !raw.is_object() {
            return Err(CodexError::Invalid("total_token_usage must be an object"));
        }
        let get = |key| -> Result<u64> {
            raw.get(key)
                .and_then(Value::as_u64)
                .ok_or(CodexError::Invalid(
                    "token counter missing or not unsigned integer",
                ))
        };
        let input = get("input_tokens")?;
        let cached = raw
            .get("cached_input_tokens")
            .map(|_| get("cached_input_tokens"))
            .transpose()?
            .unwrap_or(0);
        let output = get("output_tokens")?;
        let reasoning = raw
            .get("reasoning_output_tokens")
            .map(|_| get("reasoning_output_tokens"))
            .transpose()?;
        let sum = input
            .checked_add(output)
            .ok_or(CodexError::Invalid("counter overflow"))?;
        let total = raw
            .get("total_tokens")
            .map(|_| get("total_tokens"))
            .transpose()?
            .unwrap_or(sum);
        if cached > input || total < sum || reasoning.is_some_and(|x| x > output) {
            return Err(CodexError::Invalid("inconsistent token buckets"));
        }
        if total > i64::MAX as u64 {
            return Err(CodexError::Invalid("SQLite counter limit"));
        }
        Ok(Some(Self {
            input,
            cached,
            output,
            reasoning,
            total,
        }))
    }
    fn monotonic_from(&self, prev: &Self) -> bool {
        self.input >= prev.input
            && self.cached >= prev.cached
            && self.output >= prev.output
            && self.total >= prev.total
            && (match (self.reasoning, prev.reasoning) {
                (Some(a), Some(b)) => a >= b,
                _ => true,
            })
    }
    fn breakdown(self) -> TokenBreakdown {
        TokenBreakdown {
            input_uncached_tokens: self.input - self.cached,
            cache_read_tokens: self.cached,
            cache_write_tokens: 0,
            output_tokens: self.output,
            unclassified_tokens: self.total - (self.input + self.output),
            reasoning_tokens: self.reasoning,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CursorState {
    path_hash: String,
    tail_hash: String,
    mtime_ns: u128,
    counters: Option<Counters>,
    event_at_ms: Option<i64>,
    // Preserve whether the last cumulative packet came with an unknown reasoning figure.
}

#[derive(Debug)]
pub struct ScanOutcome {
    pub next: SourceCheckpoint,
    pub record: Option<CanonicalRecord>,
    pub parsed_lines: usize,
    pub reused_cursor: bool,
    pub pending_trailing_line: bool,
}

fn hex_digest(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}
fn now_ms() -> Result<i64> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CodexError::Invalid("clock"))?
        .as_millis();
    i64::try_from(ms).map_err(|_| CodexError::Invalid("clock overflow"))
}
fn mtime_ns(metadata: &fs::Metadata) -> Result<u128> {
    Ok(metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CodexError::Invalid("mtime"))?
        .as_nanos())
}
fn event_ms(v: &Value) -> Result<Option<i64>> {
    let Some(s) = v.get("timestamp").and_then(Value::as_str) else {
        return Ok(None);
    };
    Ok(Some(
        DateTime::parse_from_rfc3339(s)
            .map_err(|_| CodexError::Invalid("timestamp"))?
            .timestamp_millis(),
    ))
}

// Reads at most MAX_LINE bytes even if JSONL contains a multi-gigabyte prompt line.
// Returns (bytes including newline, ended_with_newline).
fn bounded_line<R: BufRead>(reader: &mut R) -> Result<(Vec<u8>, bool)> {
    let mut result = Vec::new();
    loop {
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            return Ok((result, false));
        }
        let size = buf
            .iter()
            .position(|&x| x == b'\n')
            .map_or(buf.len(), |i| i + 1);
        if result.len().checked_add(size).is_none_or(|n| n > MAX_LINE) {
            return Err(CodexError::Limit("JSONL line exceeds 4 MiB"));
        }
        let ended = buf[..size].last() == Some(&b'\n');
        result.extend_from_slice(&buf[..size]);
        reader.consume(size);
        if ended {
            return Ok((result, true));
        }
    }
}
fn read_session_header(path: &Path) -> Result<String> {
    let mut rdr = BufReader::new(File::open(path)?);
    let (line, complete) = bounded_line(&mut rdr)?;
    if !complete {
        return Err(CodexError::PendingHeader);
    }
    let v: Value = serde_json::from_slice(&line)?;
    if v.get("type").and_then(Value::as_str) != Some("session_meta") {
        return Err(CodexError::Invalid("not Codex rollout session_meta"));
    }
    let payload = v
        .get("payload")
        .ok_or(CodexError::Invalid("session payload"))?;
    // Codex Desktop and CLI names vary. Reject explicit foreign originators.
    if let Some(origin) = payload.get("originator").and_then(Value::as_str) {
        if !origin.to_ascii_lowercase().contains("codex") {
            return Err(CodexError::Invalid("non-Codex originator"));
        }
    }
    let id = payload
        .get("id")
        .or_else(|| payload.get("session_id"))
        .and_then(Value::as_str)
        .ok_or(CodexError::Invalid("missing session id"))?;
    if id.len() < 8
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(CodexError::Invalid("bad session id"));
    }
    Ok(id.to_owned())
}
fn sha_tail(path: &Path, offset: u64) -> Result<String> {
    let mut f = File::open(path)?;
    let begin = offset.saturating_sub(TAIL_WINDOW as u64);
    f.seek(SeekFrom::Start(begin))?;
    let count = usize::try_from(offset - begin).map_err(|_| CodexError::Invalid("offset"))?;
    let mut buf = vec![0u8; count];
    f.read_exact(&mut buf)?;
    Ok(hex_digest(&buf))
}
fn hash_path(path: &Path) -> Result<String> {
    // Path stays local; store only a digest for change detection.
    Ok(hex_digest(
        path.canonicalize()?.to_string_lossy().as_bytes(),
    ))
}

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
        let (line, complete) = bounded_line(&mut reader)?;
        if line.is_empty() {
            break;
        }
        if !complete {
            incomplete = true;
            offset = start;
            break;
        }
        parsed += 1;
        let v: Value = serde_json::from_slice(&line)?;
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

#[derive(Debug, Default)]
pub struct CollectReport {
    pub scanned_files: usize,
    pub inserted: usize,
    pub revised: usize,
    pub unchanged: usize,
    pub parsed_lines: usize,
    pub pending_lines: usize,
}

pub fn discover_codex_files(home: &Path) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    for dir in [home.join("sessions"), home.join("archived_sessions")] {
        if !dir.exists() {
            continue;
        }
        let mut stack = vec![(dir, 0usize)];
        while let Some((root, depth)) = stack.pop() {
            if depth > 8 {
                return Err(CodexError::Limit("Codex directory depth"));
            }
            let mut entries = fs::read_dir(root)?.collect::<std::result::Result<Vec<_>, _>>()?;
            entries.sort_by_key(|e| e.file_name());
            for ent in entries {
                let file_type = ent.file_type()?;
                if file_type.is_symlink() {
                    continue;
                } // Never follow outside the Codex root.
                if file_type.is_dir() {
                    stack.push((ent.path(), depth + 1));
                    continue;
                }
                if !file_type.is_file() {
                    continue;
                }
                if ent.path().extension().and_then(|x| x.to_str()) != Some("jsonl") {
                    continue;
                }
                result.push(ent.path());
                if result.len() > MAX_FILES {
                    return Err(CodexError::Limit("Codex file count"));
                }
            }
        }
    }
    result.sort();
    Ok(result)
}

pub fn collect_codex(
    home: &Path,
    store: &mut LocalStore,
    device_id: &str,
) -> Result<CollectReport> {
    let mut report = CollectReport::default();
    let mut session_sources = std::collections::HashSet::new();
    for file in discover_codex_files(home)? {
        let session_id = match read_session_header(&file) {
            Ok(id) => id,
            Err(CodexError::PendingHeader) => {
                report.pending_lines += 1;
                continue;
            }
            Err(e) => return Err(e),
        };
        let key = session_key("codex", session_id.as_bytes())?;
        if !session_sources.insert(key.clone()) {
            return Err(CodexError::Invalid(
                "duplicate Codex session; reconcile sources before collecting",
            ));
        }
        let prior = store.checkpoint("codex", &key)?;
        let scan = scan_file(&file, device_id, prior.as_ref(), now_ms()?)?;
        let records = scan.record.iter().cloned().collect::<Vec<_>>();
        let ApplyReport {
            inserted,
            revised,
            unchanged,
        } = store.apply_scan(
            prior.as_ref(),
            &scan.next,
            &records,
            scan.next.last_scan_ms,
            false,
        )?;
        report.scanned_files += 1;
        report.inserted += inserted;
        report.revised += revised;
        report.unchanged += unchanged;
        report.parsed_lines += scan.parsed_lines;
        if scan.pending_trailing_line {
            report.pending_lines += 1
        }
    }
    Ok(report)
}

pub mod precise;
pub use precise::{collect_codex_precise, scan_precise, PreciseReport, PreciseScan};
