//! Local Antigravity conversation activity (M0-08).
//!
//! Antigravity's `gen_metadata` SQLite rows expose model response *activity*,
//! but do NOT reliably persist billed token counts. Default is observation
//! only (no TokenScope usage rows). Explicit opt-in writes *estimated* tokens,
//! never Exact, never a fabricated event timestamp or cost.
//!
//! This adapter does not call Antigravity RPC, read credentials, or send data.
//! CLI and IDE DBs with the same conversation identity are arbitrated before
//! producing records, so synced duplicate sources do not double count.
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokenscope_core_domain::{
    session_key, stable_record_id, CanonicalRecord, Precision, TokenBreakdown, UsageScope,
};
use tokenscope_local_store::{AntigravityActivity, LocalStore, SourceCheckpoint};

pub const MODULE_ID: &str = "source-antigravity";
pub const PARSER_VERSION: u32 = 2;
const MAX_FILES: usize = 20_000;
const MAX_TURNS_PER_DB: u64 = 50_000;
const MAX_MODELS_PER_DB: usize = 512;
const MAX_PROTO_BYTES: usize = 256 * 1024;

#[derive(Debug)]
pub enum AntigravityError {
    Io(io::Error),
    Sql(rusqlite::Error),
    Store(tokenscope_local_store::StoreError),
    Domain(tokenscope_core_domain::DomainError),
    Invalid(&'static str),
}
impl std::fmt::Display for AntigravityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "Antigravity I/O: {}", e.kind()),
            Self::Sql(_) => write!(f, "Antigravity conversation DB read failed"),
            Self::Store(e) => write!(f, "Antigravity store: {e}"),
            Self::Domain(e) => write!(f, "Antigravity domain: {e}"),
            Self::Invalid(s) => write!(f, "Antigravity invalid source: {s}"),
        }
    }
}
impl std::error::Error for AntigravityError {}
impl From<io::Error> for AntigravityError { fn from(e: io::Error) -> Self { Self::Io(e) } }
impl From<rusqlite::Error> for AntigravityError { fn from(e: rusqlite::Error) -> Self { Self::Sql(e) } }
impl From<tokenscope_local_store::StoreError> for AntigravityError {
    fn from(e: tokenscope_local_store::StoreError) -> Self { Self::Store(e) }
}
impl From<tokenscope_core_domain::DomainError> for AntigravityError {
    fn from(e: tokenscope_core_domain::DomainError) -> Self { Self::Domain(e) }
}
type Result<T> = std::result::Result<T, AntigravityError>;

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct AntigravityReport {
    pub source_files: usize,
    pub conversations: usize,
    pub model_turns: u64,
    pub unknown_model_turns: u64,
    pub ambiguous_conversations: usize,
    pub estimated_records: usize,
    pub inserted: usize,
    pub revised: usize,
    pub tombstoned: usize,
    pub unchanged: usize,
}

// The assumptions below are deliberately *heuristic* per response, not
// official Google counters. NEVER label the derived values as measured usage.
// Defaults are aligned with the published antigravity-usage estimator's
// broad model families, but are not billing-grade.
fn assumed_per_turn(model: &str) -> (u64, u64) {
    let model = model.to_ascii_lowercase();
    if model.starts_with("gemini-2.5-flash-lite") { (3_000, 400) }
    else if model.starts_with("claude-opus") { (10_000, 900) }
    else if model.starts_with("claude-haiku") { (3_000, 400) }
    else if model.starts_with("claude-sonnet") { (8_000, 700) }
    else if model.starts_with("gemini-2.5-flash") || model.starts_with("gemini-3-flash") || model.starts_with("gemini-3.1-flash") { (4_000, 500) }
    else if model.starts_with("gemini-2.5-pro") || model.starts_with("gemini-3-pro") || model.starts_with("gemini-pro") { (8_000, 800) }
    else { (5_000, 600) }
}

// Strictly bounded protobuf field walker. Fields outside the model path are
// skipped; prompts and step content are never persisted or logged.
fn varint(b: &[u8], pos: &mut usize) -> Result<u64> {
    let mut value = 0u64;
    for i in 0..10 {
        let byte = *b.get(*pos).ok_or(AntigravityError::Invalid("truncated protobuf"))?;
        *pos += 1;
        if i == 9 && byte > 1 { return Err(AntigravityError::Invalid("protobuf integer overflow")); }
        value |= u64::from(byte & 0x7f) << (i * 7);
        if byte & 0x80 == 0 { return Ok(value); }
    }
    Err(AntigravityError::Invalid("protobuf varint too long"))
}
fn proto_field<'a>(b: &'a [u8], requested: u64) -> Result<Option<&'a [u8]>> {
    let mut pos = 0;
    while pos < b.len() {
        let tag = varint(b, &mut pos)?;
        let field = tag >> 3;
        if field == 0 { return Err(AntigravityError::Invalid("protobuf field zero")); }
        match tag & 7 {
            0 => { varint(b, &mut pos)?; }
            1 => { pos = pos.checked_add(8).ok_or(AntigravityError::Invalid("protobuf bounds"))?; }
            2 => {
                let n = usize::try_from(varint(b, &mut pos)?)
                    .map_err(|_| AntigravityError::Invalid("protobuf length"))?;
                let end = pos.checked_add(n).ok_or(AntigravityError::Invalid("protobuf bounds"))?;
                let slice = b.get(pos..end).ok_or(AntigravityError::Invalid("protobuf truncated field"))?;
                if field == requested { return Ok(Some(slice)); }
                pos = end;
            }
            5 => { pos = pos.checked_add(4).ok_or(AntigravityError::Invalid("protobuf bounds"))?; }
            _ => return Err(AntigravityError::Invalid("unsupported protobuf wire type")),
        }
        if pos > b.len() { return Err(AntigravityError::Invalid("protobuf bounds")); }
    }
    Ok(None)
}

/// `gen_metadata.field_1.field_19` holds a model identifier in known
/// Antigravity versions. Unknown layouts are reported, not interpreted as 0.
pub fn extract_model_name(data: &[u8]) -> Result<Option<String>> {
    if data.len() > MAX_PROTO_BYTES { return Err(AntigravityError::Invalid("metadata BLOB too large")); }
    let Some(nested) = proto_field(data, 1)? else { return Ok(None); };
    let Some(bytes) = proto_field(nested, 19)? else { return Ok(None); };
    let name = std::str::from_utf8(bytes).map_err(|_| AntigravityError::Invalid("model encoding"))?;
    if name.is_empty() || name.len() > 128 || !name.bytes().all(|c|
        c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b':' | b'/')) {
        return Err(AntigravityError::Invalid("model name is unsafe"));
    }
    Ok(Some(name.to_string()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    stem: String,
    counts: BTreeMap<String, u64>,
    known_turns: u64,
    unknown_turns: u64,
}
fn read_db(path: &Path, stem: String) -> Result<Snapshot> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    // A single SQLite read transaction sees consistent DB+WAL state.
    conn.execute_batch("BEGIN")?;
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='gen_metadata')",
        [], |r| r.get(0),
    )?;
    if !exists { return Err(AntigravityError::Invalid("gen_metadata table missing")); }
    let mut stmt = conn.prepare(
        "SELECT CASE WHEN length(data)<=262144 THEN data ELSE NULL END FROM gen_metadata ORDER BY rowid",
    )?;
    let mut rows = stmt.query([])?;
    let mut counts = BTreeMap::new();
    let mut known_turns = 0u64;
    let mut unknown_turns = 0u64;
    while let Some(row) = rows.next()? {
        if known_turns + unknown_turns >= MAX_TURNS_PER_DB {
            return Err(AntigravityError::Invalid("conversation turn limit exceeded"));
        }
        let blob: Option<Vec<u8>> = row.get(0)?;
        let blob = blob.ok_or(AntigravityError::Invalid("missing/oversized metadata"))?;
        match extract_model_name(&blob)? {
            Some(model) => {
                *counts.entry(model).or_insert(0) += 1;
                if counts.len() > MAX_MODELS_PER_DB { return Err(AntigravityError::Invalid("model cardinality limit")); }
                known_turns += 1;
            }
            None => unknown_turns += 1,
        }
    }
    Ok(Snapshot { stem, counts, known_turns, unknown_turns })
}

pub fn discover_databases(gemini_home: &Path) -> Result<Vec<PathBuf>> {
    let mut files = BTreeSet::new();
    for prefix in ["antigravity", "antigravity-ide", "antigravity-backup", "antigravity-cli"] {
        let dir = gemini_home.join(prefix).join("conversations");
        if !dir.is_dir() { continue; }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let typ = entry.file_type()?;
            // Do not follow links out of the configured Antigravity roots.
            if !typ.is_file() || path.extension().and_then(|s| s.to_str()) != Some("db") { continue; }
            files.insert(path);
            if files.len() > MAX_FILES { return Err(AntigravityError::Invalid("too many databases")); }
        }
    }
    Ok(files.into_iter().collect())
}
fn now_ms() -> Result<i64> {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH)
        .map_err(|_| AntigravityError::Invalid("system time before epoch"))?.as_millis();
    i64::try_from(ms).map_err(|_| AntigravityError::Invalid("time out of range"))
}
fn dominates(a: &Snapshot, b: &Snapshot) -> bool {
    a.unknown_turns >= b.unknown_turns &&
        b.counts.iter().all(|(model, count)| a.counts.get(model).copied().unwrap_or_default() >= *count)
}
fn source_fingerprint(snapshot: &Snapshot, estimates: bool) -> String {
    let mut h = Sha256::new();
    h.update(b"tokenscope/antigravity/model-turns/v1\0");
    h.update([u8::from(estimates)]);
    h.update(snapshot.unknown_turns.to_be_bytes());
    for (model, count) in &snapshot.counts {
        h.update((model.len() as u32).to_be_bytes());
        h.update(model.as_bytes());
        h.update(count.to_be_bytes());
    }
    format!("sha256:{:x}", h.finalize())
}
fn source_records(s: &Snapshot, device: &str) -> Result<Vec<CanonicalRecord>> {
    // This is the only code path that creates estimated usage records.
    // `event_at_ms=None`: DB mtime isn't a generation timestamp.
    let source = s.stem.as_bytes();
    let session = session_key("antigravity", source)?;
    let mut records = Vec::with_capacity(s.counts.len());
    for (model, turns) in &s.counts {
        let (input, output) = assumed_per_turn(model);
        let input = turns.checked_mul(input).ok_or(AntigravityError::Invalid("token estimate overflow"))?;
        let output = turns.checked_mul(output).ok_or(AntigravityError::Invalid("token estimate overflow"))?;
        if input > i64::MAX as u64 || output > i64::MAX as u64  {
            return Err(AntigravityError::Invalid("estimated usage outside storage range"));
        }
        records.push(CanonicalRecord {
            record_id: stable_record_id("antigravity", source, model.as_bytes())?,
            owner_device_id: device.to_string(), tool_id: "antigravity".to_string(),
            scope: UsageScope::DeviceLocal, precision: Precision::Estimated,
            account_id: None, model_id: Some(model.clone()), project_id: None,
            session_id: Some(session.clone()), event_at_ms: None,
            // We have no reliable cache attribution: keep estimated input
            // in Unclassified rather than falsely reporting cache misses.
            usage: TokenBreakdown { unclassified_tokens: input, output_tokens: output, ..TokenBreakdown::default() },
            estimated_cost_microusd: None, price_version: None, is_deleted: false,
        });
    }
    Ok(records)
}

/// `estimate_tokens=false` observes only session/model activity, without
/// adding bogus Token counts to the shared device totals. Explicit opt-in
/// emits `Precision::Estimated` records. Turning it off tombstones prior
/// estimates for *present* sources, retaining checkpoint history.
pub fn collect_antigravity(
    gemini_home: &Path,
    store: &mut LocalStore,
    device_id: &str,
    estimate_tokens: bool,
) -> Result<AntigravityReport> {
    let paths = discover_databases(gemini_home)?;
    let mut report = AntigravityReport { source_files: paths.len(), ..AntigravityReport::default() };
    let mut snapshots: BTreeMap<String, Snapshot> = BTreeMap::new();
    let mut ambiguous: BTreeSet<String> = BTreeSet::new();
    for path in paths {
        let stem = path.file_stem().and_then(|s| s.to_str())
            .ok_or(AntigravityError::Invalid("database filename"))?.to_string();
        if stem.is_empty() || stem.len() > 200 || stem.chars().any(char::is_control) {
            return Err(AntigravityError::Invalid("invalid conversation filename"));
        }
        let snap = read_db(&path, stem.clone())?;
        if let Some(previous) = snapshots.get(&stem) {
            if dominates(&snap, previous) {
                snapshots.insert(stem, snap);
            } else if !dominates(previous, &snap) {
                ambiguous.insert(stem);
            }
        } else {
            snapshots.insert(stem, snap);
        }
    }
    report.conversations = snapshots.len();
    report.ambiguous_conversations = ambiguous.len();
    for (stem, snapshot) in snapshots {
        if ambiguous.contains(&stem) { continue; }
        report.model_turns = report.model_turns.saturating_add(snapshot.known_turns);
        report.unknown_model_turns = report.unknown_model_turns.saturating_add(snapshot.unknown_turns);
        // Do not persist partial model coverage as fully accounted usage.
        if estimate_tokens && snapshot.unknown_turns > 0 { continue; }
        // A suddenly empty live DB might be transient. Never tombstone its history.
        if estimate_tokens && snapshot.known_turns == 0 { continue; }
        let source_key = stable_record_id("antigravity", b"conversation", stem.as_bytes())?;
        let previous = store.checkpoint("antigravity", &source_key)?;
        let fp = source_fingerprint(&snapshot, estimate_tokens);
        if previous.as_ref().is_some_and(|p|
            p.source_fingerprint == fp && p.parser_version == PARSER_VERSION && p.last_status == "complete") {
            report.unchanged += 1;
            continue;
        }
        // Activity-only collection MUST persist a separate model-turn snapshot.
        // A parse failure above never triggers any update.
        let observed_at = now_ms()?.max(previous.as_ref().map_or(0, |p| p.last_scan_ms));
        let next = SourceCheckpoint {
            tool_id: "antigravity".into(), source_key,
            source_fingerprint: fp, read_offset: 0,
            parser_version: PARSER_VERSION,
            last_scan_ms: observed_at, last_complete_scan_ms: Some(observed_at),
            last_status: "complete".into(),
        };
        let records = if estimate_tokens { source_records(&snapshot, device_id)? } else { Vec::new() };
        report.estimated_records += records.len();
        let activity = AntigravityActivity {
            known_turns: snapshot.known_turns,
            unknown_turns: snapshot.unknown_turns,
            models: snapshot.counts.iter().map(|(m,n)| (m.clone(), *n)).collect(),
        };
        let result = store.reconcile_source_with_activity(previous.as_ref(), &next, records, observed_at, Some(&activity))?;
        report.inserted += result.applied.inserted;
        report.revised += result.applied.revised;
        report.tombstoned += result.tombstoned;
    }
    Ok(report)
}
