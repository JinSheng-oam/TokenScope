//! M0-06: privacy-preserving Claude Code JSONL collector.
//!
//! Uses one authoritative usage snapshot per message.id, NOT per
//! assistant JSONL line. Full reconciliation on change handles streaming
//! partial rows, rewrites and revisions without accumulating duplicate counts.
//! Reads only local usage metadata; never persists prompts or raw file paths.
use chrono::DateTime;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, Metadata};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokenscope_core_domain::{
    project_key, session_key, stable_record_id, CanonicalRecord, Precision, TokenBreakdown, UsageScope,
};
use tokenscope_local_store::{LocalStore, SourceCheckpoint};

pub const MODULE_ID: &str = "source-claude";
pub const PARSER_VERSION: u32 = 1;
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
const MAX_FILES: usize = 20_000;
const MAX_EVENTS: usize = 50_000; // bounded fallback; streaming parse is a later optimization
const TAIL_BYTES: u64 = 256;

#[derive(Debug)]
pub enum ClaudeError {
    Io(io::Error),
    Json(serde_json::Error),
    Store(tokenscope_local_store::StoreError),
    Domain(tokenscope_core_domain::DomainError),
    Invalid(&'static str),
    Limit(&'static str),
    Pending,
}
impl std::fmt::Display for ClaudeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error ({:?})", e.kind()),
            Self::Json(_) => write!(f, "malformed JSONL"),
            Self::Store(e) => write!(f, "local store: {e}"),
            Self::Domain(e) => write!(f, "domain: {e}"),
            Self::Invalid(s) => write!(f, "invalid Claude Code usage ({s})"),
            Self::Limit(s) => write!(f, "Claude Code resource limit ({s})"),
            Self::Pending => write!(f, "source is partially written; checkpoint retained"),
        }
    }
}
impl std::error::Error for ClaudeError {}
impl From<io::Error> for ClaudeError { fn from(e: io::Error) -> Self { Self::Io(e) } }
impl From<serde_json::Error> for ClaudeError { fn from(e: serde_json::Error) -> Self { Self::Json(e) } }
impl From<tokenscope_local_store::StoreError> for ClaudeError { fn from(e: tokenscope_local_store::StoreError) -> Self { Self::Store(e) } }
impl From<tokenscope_core_domain::DomainError> for ClaudeError { fn from(e: tokenscope_core_domain::DomainError) -> Self { Self::Domain(e) } }
type Result<T> = std::result::Result<T, ClaudeError>;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ClaudeReport {
    pub files: usize,
    pub unchanged_files: usize,
    pub pending_files: usize,
    pub parsed_lines: usize,
    pub inserted: usize,
    pub revised: usize,
    pub unchanged_records: usize,
    pub deleted: usize,
}

#[derive(Debug)]
pub struct ClaudeScan {
    pub next: SourceCheckpoint,
    pub records: Vec<CanonicalRecord>,
    pub parsed_lines: usize,
}

fn now_ms() -> Result<i64> {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH)
        .map_err(|_| ClaudeError::Invalid("system clock"))?.as_millis();
    i64::try_from(millis).map_err(|_| ClaudeError::Limit("timestamp"))
}
fn modified_ns(meta: &Metadata) -> Result<u128> {
    Ok(meta.modified()?.duration_since(UNIX_EPOCH)
        .map_err(|_| ClaudeError::Invalid("mtime"))?.as_nanos())
}
fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut result = String::with_capacity(64);
    for b in digest { result.push_str(&format!("{b:02x}")); }
    result
}
fn tail_digest(path: &Path, size: u64) -> Result<String> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(size.saturating_sub(TAIL_BYTES)))?;
    let mut buf = Vec::with_capacity(TAIL_BYTES as usize);
    file.take(TAIL_BYTES).read_to_end(&mut buf)?;
    // Only a 256-byte tail is needed for the *fast unchanged hint*, not for
    // content identity. Changed sources are always fully reconciled.
    Ok(hex_digest(&buf))
}
fn fingerprint(path: &Path, meta: &Metadata) -> Result<String> {
    Ok(format!("v1:{}:{}:{}", meta.len(), modified_ns(meta)?, tail_digest(path, meta.len())?))
}

/// Allocates at most MAX_LINE_BYTES per JSONL line; unlike read_until, it cannot
/// consume unbounded memory from a corrupted transcript.
fn bounded_line<R: BufRead>(reader: &mut R) -> Result<Option<(Vec<u8>, bool)>> {
    let mut output = Vec::new();
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() { return if output.is_empty() { Ok(None) } else { Ok(Some((output, false))) }; }
        let until = chunk.iter().position(|&b| b == b'\n').map_or(chunk.len(), |n| n + 1);
        if output.len().checked_add(until).is_none_or(|n| n > MAX_LINE_BYTES) {
            return Err(ClaudeError::Limit("JSONL line exceeds 4 MiB"));
        }
        let done = chunk[until - 1] == b'\n';
        output.extend_from_slice(&chunk[..until]);
        reader.consume(until);
        if done { return Ok(Some((output, true))); }
    }
}

/// Traverse only configured `projects` tree. Never traverse symlinks, even if
/// they point outside the tree, to avoid accidental disclosure or cycles.
pub fn discover_claude_files(config_dir: &Path) -> Result<Vec<PathBuf>> {
    let root = config_dir.join("projects");
    if !root.exists() { return Ok(Vec::new()); }
    if !fs::symlink_metadata(&root)?.file_type().is_dir() {
        return Err(ClaudeError::Invalid("projects root is not a real directory"));
    }
    let mut result = Vec::new();
    let mut stack = vec![root];
    let mut visited = 0_usize;
    while let Some(dir) = stack.pop() {
        for child in fs::read_dir(dir)? {
            let entry = child?;
            visited = visited.checked_add(1).ok_or(ClaudeError::Limit("entries"))?;
            if visited > 100_000 { return Err(ClaudeError::Limit("directory entries > 100000")); }
            let path = entry.path();
            let ty = entry.file_type()?;
            if ty.is_symlink() { continue; }
            if ty.is_dir() { stack.push(path); }
            else if ty.is_file() && path.extension().is_some_and(|s| s == "jsonl") {
                result.push(path);
                if result.len() > MAX_FILES { return Err(ClaudeError::Limit("transcript files > 20000")); }
            }
        }
    }
    result.sort();
    Ok(result)
}

fn file_identity(path: &Path, config_dir: &Path) -> Result<(String, String, String)> {
    let rel = path.strip_prefix(config_dir.join("projects"))
        .map_err(|_| ClaudeError::Invalid("transcript outside configured projects"))?;
    let rel_str = rel.to_str().ok_or(ClaudeError::Invalid("unrepresentable path"))?;
    let project_name = rel.components().next().ok_or(ClaudeError::Invalid("project name"))?;
    // Hash the project and the relative transcript path: never store raw paths.
    let project = project_key("claude", project_name.as_os_str().to_string_lossy().as_bytes())?;
    let session = session_key("claude", rel_str.as_bytes())?;
    Ok((session.clone(), session, project)) // source_key, session_id, project_id
}

#[derive(Clone)]
struct Sample {
    record: CanonicalRecord,
    // Higher observed totals and output counts represent a more complete
    // streaming snapshot; never add two snapshots of the same response.
    score: (u64, u64),
}

fn number(usage: &Value, key: &'static str, required: bool) -> Result<u64> {
    match usage.get(key) {
        Some(v) if v.is_null() && !required => Ok(0),
        None if !required => Ok(0),
        Some(v) => v.as_u64().ok_or(ClaudeError::Invalid("usage field not a nonnegative integer")),
        None => Err(ClaudeError::Invalid(key)),
    }
}

/// A top-level zero cache-write with nonzero nested writes can be a placeholder
/// row. Ignore it only if another valid row with the same response identity is
/// present; otherwise FAIL CLOSED instead of guessing the total.
fn parse_usage(usage: &Value) -> Result<Option<TokenBreakdown>> {
    let input = number(usage, "input_tokens", true)?;
    let output = number(usage, "output_tokens", true)?;
    let read = number(usage, "cache_read_input_tokens", false)?;
    let write = number(usage, "cache_creation_input_tokens", false)?;
    if let Some(nested) = usage.get("cache_creation").filter(|v| !v.is_null()) {
        let a = number(nested, "ephemeral_5m_input_tokens", false)?;
        let b = number(nested, "ephemeral_1h_input_tokens", false)?;
        let sum = a.checked_add(b).ok_or(ClaudeError::Limit("cache counters overflow"))?;
        if sum != write { return Ok(None); }
    }
    let reasoning = usage.get("output_tokens_details")
        .and_then(|v| v.get("thinking_tokens"))
        .filter(|v| !v.is_null())
        .map(|v| v.as_u64().ok_or(ClaudeError::Invalid("reasoning counter")))
        .transpose()?;
    let result = TokenBreakdown {
        input_uncached_tokens: input, cache_read_tokens: read,
        cache_write_tokens: write, output_tokens: output,
        unclassified_tokens: 0, reasoning_tokens: reasoning,
    };
    result.validate()?;
    if result.total()? > i64::MAX as u64 { return Err(ClaudeError::Limit("SQLite token integer")); }
    Ok(Some(result))
}
fn parse_time(v: &Value) -> Result<i64> {
    let ts = v.get("timestamp").and_then(Value::as_str)
        .ok_or(ClaudeError::Invalid("missing timestamp"))?;
    DateTime::parse_from_rfc3339(ts).map(|t| t.timestamp_millis())
        .map_err(|_| ClaudeError::Invalid("invalid timestamp"))
}

/// A fresh full scan is intentionally used when file metadata changes. This
/// avoids needing to persist a large streaming-response ID state. It is
/// bounded to 50,000 distinct responses/file, with O(file size) read cost.
pub fn scan_claude_file(
    path: &Path, config_dir: &Path, device_id: &str,
    previous: Option<&SourceCheckpoint>, observed_at_ms: i64,
) -> Result<Option<ClaudeScan>> {
    let (source_key, session_id, project_id) = file_identity(path, config_dir)?;
    if let Some(cp) = previous {
        if cp.source_key != source_key || cp.tool_id != "claude" { return Err(ClaudeError::Invalid("checkpoint source mismatch")); }
    }
    let before = fs::symlink_metadata(path)?;
    if !before.file_type().is_file() { return Err(ClaudeError::Invalid("transcript must be a regular file")); }
    let fp = fingerprint(path, &before)?;
    if previous.is_some_and(|p| p.parser_version == PARSER_VERSION &&
        p.source_fingerprint == fp && p.last_status == "complete") { return Ok(None); }

    let mut stream = BufReader::new(File::open(path)?);
    let mut samples: HashMap<String, Sample> = HashMap::new();
    let mut inconsistent: HashSet<String> = HashSet::new();
    let mut parsed = 0_usize;
    while let Some((line, complete)) = bounded_line(&mut stream)? {
        if !complete { return Err(ClaudeError::Pending); }
        parsed += 1;
        let v: Value = serde_json::from_slice(&line)?;
        if v.get("type").and_then(Value::as_str) != Some("assistant") { continue; }
        let Some(message) = v.get("message") else { continue; };
        let Some(u) = message.get("usage").filter(|u| !u.is_null()) else { continue; };
        let msg_id = message.get("id").and_then(Value::as_str)
            .ok_or(ClaudeError::Invalid("assistant with usage lacks message.id"))?;
        if msg_id.is_empty() || msg_id.len() > 512 ||
            msg_id.chars().any(char::is_control) {
            return Err(ClaudeError::Invalid("response identity"));
        }
        // Anthropic message.id is the logical API response identity. requestId
        // can be absent on some rows of that same response.
        let native = msg_id.to_owned();
        let at = parse_time(&v)?;
        let usage = match parse_usage(u)? {
            Some(value) => value,
            None => {
                if !samples.contains_key(&native) { inconsistent.insert(native); }
                continue;
            }
        };
        inconsistent.remove(&native);
        let score = (usage.total()?, usage.output_tokens);
        let model = message.get("model").and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty() && s.len() <= 256 && !s.chars().any(char::is_control))
            .map(str::to_owned);
        let id = stable_record_id("claude", source_key.as_bytes(), native.as_bytes())?;
        let record = CanonicalRecord {
            record_id: id, owner_device_id: device_id.to_owned(), tool_id: "claude".into(),
            scope: UsageScope::DeviceLocal, precision: Precision::Exact,
            account_id: None, model_id: model, project_id: Some(project_id.clone()),
            session_id: Some(session_id.clone()), event_at_ms: Some(at),
            usage, estimated_cost_microusd: None, price_version: None, is_deleted: false,
        }.normalized()?;
        match samples.get_mut(&native) {
            Some(existing) => {
                let ambiguous = existing.record.precision == Precision::Estimated ||
                    existing.record.usage != record.usage;
                if score > existing.score || (score == existing.score &&
                    (record.model_id.is_some() || existing.record.model_id.is_none())) {
                    let first_time = existing.record.event_at_ms;
                    let prior_model = existing.record.model_id.clone();
                    existing.record = record;
                    if ambiguous { existing.record.precision = Precision::Estimated; }
                    if existing.record.model_id.is_none() { existing.record.model_id = prior_model; }
                    existing.record.event_at_ms = first_time.map(|x| x.min(at));
                    existing.score = score;
                } else if ambiguous {
                    existing.record.precision = Precision::Estimated;
                }
            }
            None => {
                if samples.len() >= MAX_EVENTS { return Err(ClaudeError::Limit("responses per file > 50000")); }
                samples.insert(native, Sample { record, score });
            }
        }
    }
    if !inconsistent.is_empty() { return Err(ClaudeError::Invalid("unresolved cache usage mismatch")); }
    let after = fs::symlink_metadata(path)?;
    if !after.file_type().is_file() || after.len() != before.len() ||
        modified_ns(&after)? != modified_ns(&before)? || fingerprint(path, &after)? != fp {
        return Err(ClaudeError::Pending);
    }
    let next = SourceCheckpoint {
        tool_id: "claude".into(), source_key, source_fingerprint: fp,
        read_offset: after.len(), parser_version: PARSER_VERSION,
        last_scan_ms: previous.map_or(observed_at_ms, |p| observed_at_ms.max(p.last_scan_ms.saturating_add(1))),
        last_complete_scan_ms: None, last_status: "complete".into(),
    };
    let mut next = next;
    next.last_complete_scan_ms = Some(next.last_scan_ms);
    let mut records: Vec<_> = samples.into_values().map(|s| s.record).collect();
    records.sort_by(|a, b| a.record_id.cmp(&b.record_id));
    Ok(Some(ClaudeScan { next, records, parsed_lines: parsed }))
}

/// One complete file -> one atomic store transaction. Failures do NOT advance
/// this source's checkpoint. Missing source files are not treated as deletions.
pub fn collect_claude(config_dir: &Path, store: &mut LocalStore, device_id: &str) -> Result<ClaudeReport> {
    let mut report = ClaudeReport::default();
    let mut sources = HashSet::new();
    for path in discover_claude_files(config_dir)? {
        let (key, _, _) = file_identity(&path, config_dir)?;
        if !sources.insert(key.clone()) { return Err(ClaudeError::Invalid("duplicate transcript identity")); }
        let previous = store.checkpoint("claude", &key)?;
        let scan = match scan_claude_file(&path, config_dir, device_id, previous.as_ref(), now_ms()?) {
            Ok(Some(s)) => s,
            Ok(None) => { report.unchanged_files += 1; continue; }
            Err(ClaudeError::Pending) => { report.pending_files += 1; continue; }
            Err(e) => return Err(e),
        };
        // Full file was validated, including stable mtime and a complete final
        // JSONL line. Reconcile inside ONE SQLite transaction, paging prior
        // records and generating tombstones only after incoming rows are seen.
        let result = store.reconcile_complete_source(
            previous.as_ref(), &scan.next, scan.records.into_iter(), scan.next.last_scan_ms,
        )?;
        report.deleted += result.tombstoned;
        report.files += 1;
        report.parsed_lines += scan.parsed_lines;
        report.inserted += result.applied.inserted;
        report.revised += result.applied.revised;
        report.unchanged_records += result.applied.unchanged;
    }
    Ok(report)
}
