//! Local Antigravity conversation activity and generation usage.
//!
//! `gen_metadata` exposes model activity and per-generation token counters in
//! a reverse-engineered protobuf layout. Counters are ingested as observed
//! source facts; uncertain fixed-prompt tokens remain unclassified. If a row
//! has no native counters, only the explicit estimate option may add estimates.
//!
//! This adapter does not call Antigravity RPC, read credentials, or send data.
//! CLI and IDE DBs with the same conversation identity are arbitrated before
//! producing records, so synced duplicate sources do not double count.
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokenscope_core_domain::{
    project_key, session_key, stable_record_id, CanonicalRecord, Precision, TokenBreakdown,
    UsageScope,
};
use tokenscope_local_store::{AntigravityActivity, LocalStore, SourceCheckpoint};

pub const MODULE_ID: &str = "source-antigravity";
pub const PARSER_VERSION: u32 = 5;
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
impl From<io::Error> for AntigravityError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<rusqlite::Error> for AntigravityError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}
impl From<tokenscope_local_store::StoreError> for AntigravityError {
    fn from(e: tokenscope_local_store::StoreError) -> Self {
        Self::Store(e)
    }
}
impl From<tokenscope_core_domain::DomainError> for AntigravityError {
    fn from(e: tokenscope_core_domain::DomainError) -> Self {
        Self::Domain(e)
    }
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
    pub measured_records: usize,
    pub inserted: usize,
    pub revised: usize,
    pub tombstoned: usize,
    pub unchanged: usize,
    pub skipped_sources: usize,
    pub skipped_rows: u64,
}

// The assumptions below are deliberately *heuristic* per response, not
// official Google counters. NEVER label the derived values as measured usage.
// Defaults are aligned with the published antigravity-usage estimator's
// broad model families, but are not billing-grade.
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
fn varint(b: &[u8], pos: &mut usize) -> Result<u64> {
    let mut value = 0u64;
    for i in 0..10 {
        let byte = *b
            .get(*pos)
            .ok_or(AntigravityError::Invalid("truncated protobuf"))?;
        *pos += 1;
        if i == 9 && byte > 1 {
            return Err(AntigravityError::Invalid("protobuf integer overflow"));
        }
        value |= u64::from(byte & 0x7f) << (i * 7);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(AntigravityError::Invalid("protobuf varint too long"))
}
fn proto_field(b: &[u8], requested: u64) -> Result<Option<&[u8]>> {
    let mut pos = 0;
    while pos < b.len() {
        let tag = varint(b, &mut pos)?;
        let field = tag >> 3;
        if field == 0 {
            return Err(AntigravityError::Invalid("protobuf field zero"));
        }
        match tag & 7 {
            0 => {
                varint(b, &mut pos)?;
            }
            1 => {
                pos = pos
                    .checked_add(8)
                    .ok_or(AntigravityError::Invalid("protobuf bounds"))?;
            }
            2 => {
                let n = usize::try_from(varint(b, &mut pos)?)
                    .map_err(|_| AntigravityError::Invalid("protobuf length"))?;
                let end = pos
                    .checked_add(n)
                    .ok_or(AntigravityError::Invalid("protobuf bounds"))?;
                let slice = b
                    .get(pos..end)
                    .ok_or(AntigravityError::Invalid("protobuf truncated field"))?;
                if field == requested {
                    return Ok(Some(slice));
                }
                pos = end;
            }
            5 => {
                pos = pos
                    .checked_add(4)
                    .ok_or(AntigravityError::Invalid("protobuf bounds"))?;
            }
            _ => return Err(AntigravityError::Invalid("unsupported protobuf wire type")),
        }
        if pos > b.len() {
            return Err(AntigravityError::Invalid("protobuf bounds"));
        }
    }
    Ok(None)
}

/// `gen_metadata.field_1.field_19` holds a model identifier in known
/// Antigravity versions. Unknown layouts are reported, not interpreted as 0.
pub fn extract_model_name(data: &[u8]) -> Result<Option<String>> {
    if data.len() > MAX_PROTO_BYTES {
        return Err(AntigravityError::Invalid("metadata BLOB too large"));
    }
    let Some(nested) = proto_field(data, 1)? else {
        return Ok(None);
    };
    let Some(bytes) = proto_field(nested, 19)? else {
        return Ok(None);
    };
    let name =
        std::str::from_utf8(bytes).map_err(|_| AntigravityError::Invalid("model encoding"))?;
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b':' | b'/'))
    {
        return Err(AntigravityError::Invalid("model name is unsafe"));
    }
    Ok(Some(name.to_string()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NativeUsage {
    fixed_prompt_tokens: u64,
    input_tokens: u64,
    cache_read_tokens: u64,
    output_tokens: u64,
    reasoning_tokens: u64,
}

fn proto_varint_field(data: &[u8], requested: u64) -> Result<Option<u64>> {
    let mut pos = 0;
    while pos < data.len() {
        let tag = varint(data, &mut pos)?;
        let field = tag >> 3;
        if field == 0 {
            return Err(AntigravityError::Invalid("protobuf field zero"));
        }
        match tag & 7 {
            0 => {
                let value = varint(data, &mut pos)?;
                if field == requested {
                    return Ok(Some(value));
                }
            }
            1 => {
                pos = pos
                    .checked_add(8)
                    .ok_or(AntigravityError::Invalid("protobuf bounds"))?
            }
            2 => {
                let len = usize::try_from(varint(data, &mut pos)?)
                    .map_err(|_| AntigravityError::Invalid("protobuf length"))?;
                pos = pos
                    .checked_add(len)
                    .ok_or(AntigravityError::Invalid("protobuf bounds"))?;
            }
            5 => {
                pos = pos
                    .checked_add(4)
                    .ok_or(AntigravityError::Invalid("protobuf bounds"))?
            }
            _ => return Err(AntigravityError::Invalid("unsupported protobuf wire type")),
        }
        if pos > data.len() {
            return Err(AntigravityError::Invalid("protobuf bounds"));
        }
    }
    Ok(None)
}

fn extract_usage(data: &[u8]) -> Result<Option<NativeUsage>> {
    let Some(chat_model) = proto_field(data, 1)? else {
        return Ok(None);
    };
    let Some(usage) = proto_field(chat_model, 4)? else {
        return Ok(None);
    };
    let field = |number| proto_varint_field(usage, number).map(|value| value.unwrap_or(0));
    let parsed = NativeUsage {
        fixed_prompt_tokens: field(1)?,
        input_tokens: field(2)?,
        cache_read_tokens: field(5)?,
        output_tokens: field(9)?,
        reasoning_tokens: field(10)?,
    };
    let output_total = parsed
        .output_tokens
        .checked_add(parsed.reasoning_tokens)
        .ok_or(AntigravityError::Invalid("output token count overflow"))?;
    if proto_varint_field(usage, 3)?.is_some_and(|stored| stored != output_total) {
        return Err(AntigravityError::Invalid(
            "inconsistent output token counters",
        ));
    }
    if [
        parsed.fixed_prompt_tokens,
        parsed.input_tokens,
        parsed.cache_read_tokens,
        parsed.output_tokens,
        parsed.reasoning_tokens,
    ]
    .iter()
    .all(|count| *count == 0)
    {
        return Ok(None);
    }
    for count in [
        parsed.fixed_prompt_tokens,
        parsed.input_tokens,
        parsed.cache_read_tokens,
        output_total,
    ] {
        if count > i64::MAX as u64 {
            return Err(AntigravityError::Invalid(
                "token count outside storage range",
            ));
        }
    }
    Ok(Some(parsed))
}

fn extract_response_id(data: &[u8]) -> Result<Option<String>> {
    let Some(chat_model) = proto_field(data, 1)? else {
        return Ok(None);
    };
    let Some(usage) = proto_field(chat_model, 4)? else {
        return Ok(None);
    };
    let Some(bytes) = proto_field(usage, 11)? else {
        return Ok(None);
    };
    let value = std::str::from_utf8(bytes)
        .map_err(|_| AntigravityError::Invalid("response ID encoding"))?;
    if value.trim().is_empty() {
        return Ok(None);
    }
    if value.len() > 512 || value.chars().any(char::is_control) {
        return Err(AntigravityError::Invalid("response ID bounds"));
    }
    Ok(Some(value.to_string()))
}

fn make_record(
    stem: &str,
    device: &str,
    native_id: &str,
    model: Option<&str>,
    project_id: Option<&str>,
    usage: NativeUsage,
    event_at_ms: Option<i64>,
) -> Result<CanonicalRecord> {
    let source = stem.as_bytes();
    let session = session_key("antigravity", source)?;
    let mut record = CanonicalRecord {
        record_id: stable_record_id("antigravity", source, native_id.as_bytes())?,
        owner_device_id: device.to_string(),
        tool_id: "antigravity".into(),
        scope: UsageScope::DeviceLocal,
        precision: if usage.fixed_prompt_tokens == 0 {
            Precision::Exact
        } else {
            Precision::Aggregate
        },
        account_id: None,
        model_id: model.map(str::to_string),
        project_id: project_id.map(str::to_owned),
        session_id: Some(session),
        event_at_ms,
        usage: TokenBreakdown {
            input_uncached_tokens: usage.input_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            output_tokens: usage
                .output_tokens
                .checked_add(usage.reasoning_tokens)
                .ok_or(AntigravityError::Invalid("output token count overflow"))?,
            unclassified_tokens: usage.fixed_prompt_tokens,
            reasoning_tokens: Some(usage.reasoning_tokens),
            ..TokenBreakdown::default()
        },
        estimated_cost_microusd: None,
        price_version: None,
        is_deleted: false,
    };
    record = record.normalized()?;
    Ok(record)
}

fn protobuf_timestamp_ms(data: &[u8]) -> Option<i64> {
    let seconds = i64::try_from(proto_varint_field(data, 1).ok()??).ok()?;
    let nanos = proto_varint_field(data, 2).ok()?.unwrap_or(0);
    if !(0..=253_402_300_799).contains(&seconds) || nanos >= 1_000_000_000 {
        return None;
    }
    seconds
        .checked_mul(1000)?
        .checked_add(i64::try_from(nanos / 1_000_000).ok()?)
}

fn read_step_timestamps(
    conn: &Connection,
) -> Result<(std::collections::HashMap<i64, i64>, Vec<Option<i64>>)> {
    let has_steps: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='steps')",
        [],
        |row| row.get(0),
    )?;
    let mut timestamps = std::collections::HashMap::new();
    let mut timeline = Vec::new();
    if !has_steps {
        return Ok((timestamps, timeline));
    }
    // `gen_metadata` rows are ordered like the model steps themselves; keep the
    // step order as well so rows without an explicit index still get a time.
    let mut stmt = conn.prepare("SELECT idx,metadata FROM steps WHERE step_type=15 ORDER BY idx")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let idx: Option<i64> = row.get(0)?;
        let metadata: Option<Vec<u8>> = row.get(1)?;
        let time = metadata
            .as_deref()
            .and_then(|m| proto_field(m, 1).ok().flatten())
            .and_then(protobuf_timestamp_ms);
        if let (Some(idx), Some(time)) = (idx, time) {
            timestamps.insert(idx, time);
        }
        timeline.push(time);
        if timestamps.len() > MAX_TURNS_PER_DB as usize {
            return Err(AntigravityError::Invalid("step timestamp limit exceeded"));
        }
    }
    Ok((timestamps, timeline))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    stem: String,
    project_id: Option<String>,
    counts: BTreeMap<String, u64>,
    known_turns: u64,
    unknown_turns: u64,
    skipped_rows: u64,
    records: Vec<CanonicalRecord>,
    missing_usage: BTreeMap<String, u64>,
}
fn read_db(path: &Path, stem: String, device: &str, project_id: Option<String>) -> Result<Snapshot> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    // A single SQLite read transaction sees consistent DB+WAL state.
    conn.execute_batch("BEGIN")?;
    let (step_timestamps, step_timeline) = read_step_timestamps(&conn)?;
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='gen_metadata')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Err(AntigravityError::Invalid("gen_metadata table missing"));
    }
    let mut stmt = conn.prepare(
        "SELECT rowid,CASE WHEN length(data)<=262144 THEN data ELSE NULL END FROM gen_metadata ORDER BY rowid",
    )?;
    let mut rows = stmt.query([])?;
    let mut counts = BTreeMap::new();
    let mut known_turns = 0u64;
    let mut unknown_turns = 0u64;
    let mut skipped_rows = 0u64;
    let mut records = Vec::new();
    let mut missing_usage = BTreeMap::new();
    let mut seen_responses = std::collections::HashMap::<String, CanonicalRecord>::new();
    let mut timeline_pos = 0usize;
    while let Some(row) = rows.next()? {
        let position = timeline_pos;
        timeline_pos += 1;
        if known_turns + unknown_turns >= MAX_TURNS_PER_DB {
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
        let model = extract_model_name(&blob)?;
        let usage = extract_usage(&blob)?;
        let response_id = extract_response_id(&blob)?;
        let native_record = if let Some(usage) = usage {
            let native_id = response_id
                .clone()
                .unwrap_or_else(|| format!("row:{}", row_idx.unwrap_or(known_turns as i64)));
            let step_idx = proto_varint_field(&blob, 2)?.and_then(|idx| i64::try_from(idx).ok());
            let event_at_ms = step_idx
                .and_then(|idx| step_timestamps.get(&idx).copied())
                .or_else(|| step_timeline.get(position).copied().flatten());
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
                if previous != record {
                    return Err(AntigravityError::Invalid(
                        "conflicting duplicate response id",
                    ));
                }
                continue;
            }
            seen_responses.insert(response_id.clone(), record.clone());
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
            *counts.entry(model.clone()).or_insert(0) += 1;
            *missing_usage.entry(model).or_insert(0) += 1;
            if counts.len() > MAX_MODELS_PER_DB {
                return Err(AntigravityError::Invalid("model cardinality limit"));
            }
            known_turns += 1;
        } else {
            unknown_turns += 1;
        }
        if counts.len() > MAX_MODELS_PER_DB {
            return Err(AntigravityError::Invalid("model cardinality limit"));
        }
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
    })
}

fn workspace_project_id(raw: Option<&str>) -> Option<String> {
    let value: Value = serde_json::from_str(raw?).ok()?;
    let mut uris = value
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|uri| !uri.is_empty() && uri.len() <= 4096 && !uri.chars().any(char::is_control))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    uris.sort();
    uris.dedup();
    if uris.is_empty() {
        return None;
    }
    let mut identity = Vec::new();
    for uri in uris {
        let bytes = uri.as_bytes();
        identity.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        identity.extend_from_slice(bytes);
    }
    project_key("antigravity", &identity).ok()
}

fn safe_project_name(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.chars().count() > 120
        || name.chars().any(char::is_control)
        || name.contains('/')
        || name.contains('\\')
    {
        return None;
    }
    Some(name.to_owned())
}

fn decode_uri_component(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = *bytes.get(index + 1)?;
            let low = *bytes.get(index + 2)?;
            let hex = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            };
            decoded.push((hex(high)? << 4) | hex(low)?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn uri_project_name(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > 4096 || raw.chars().any(char::is_control) {
        return None;
    }
    let uri = raw.split(['?', '#']).next()?;
    let path = if let Some(path) = uri.strip_prefix("file://") {
        path
    } else if let Some((_, rest)) = uri.split_once("://") {
        rest.split_once('/')?.1
    } else {
        uri
    };
    let component = path
        .rsplit(['/', '\\'])
        .find(|component| !component.is_empty())?;
    safe_project_name(&decode_uri_component(component)?)
}

fn workspace_project_name(raw: Option<&str>) -> Option<String> {
    let value: Value = serde_json::from_str(raw?).ok()?;
    let mut names = value
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .filter_map(uri_project_name)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    let joined = names.join(" + ");
    let joined: String = joined.chars().take(120).collect();
    safe_project_name(&joined)
}

#[derive(Clone)]
struct ProjectMetadata {
    project_id: String,
    display_name: Option<String>,
}

fn project_index(gemini_home: &Path) -> HashMap<(String, String), ProjectMetadata> {
    let mut projects = HashMap::new();
    for prefix in [
        "antigravity",
        "antigravity-ide",
        "antigravity-backup",
        "antigravity-cli",
    ] {
        let path = gemini_home.join(prefix).join("conversation_summaries.db");
        if !path.is_file() {
            continue;
        }
        let Ok(conn) = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
            continue;
        };
        if conn.busy_timeout(std::time::Duration::from_secs(2)).is_err() {
            continue;
        }
        let exists = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='conversation_summaries')",
            [],
            |row| row.get::<_, bool>(0),
        );
        if !matches!(exists, Ok(true)) {
            continue;
        }
        let Ok(mut stmt) = conn.prepare(
            "SELECT conversation_id,project_id,workspace_uris FROM conversation_summaries LIMIT 5000",
        ) else {
            continue;
        };
        let Ok(mut rows) = stmt.query([]) else {
            continue;
        };
        loop {
            let row = match rows.next() {
                Ok(Some(row)) => row,
                Ok(None) | Err(_) => break,
            };
            let id = match row.get::<_, String>(0) {
                Ok(id) if !id.is_empty() && id.len() <= 200 && !id.chars().any(char::is_control) => id,
                _ => continue,
            };
            let project = row.get::<_, Option<String>>(1).ok().flatten();
            let workspace_uris = row.get::<_, Option<String>>(2).ok().flatten();
            let project_id = project
                .as_deref()
                .filter(|value| !value.trim().is_empty() && value.len() <= 4096)
                .and_then(|value| project_key("antigravity", value.trim().as_bytes()).ok())
                .or_else(|| workspace_project_id(workspace_uris.as_deref()));
            if let Some(project_id) = project_id {
                projects.insert(
                    (prefix.to_owned(), id),
                    ProjectMetadata {
                        project_id,
                        display_name: workspace_project_name(workspace_uris.as_deref()),
                    },
                );
            }
        }
    }
    projects
}

/// Antigravity keeps conversation titles in a separate summaries database.
/// They are imported as display-only labels keyed by the same conversation
/// identity the usage rows use.
pub fn import_titles(gemini_home: &Path, store: &mut LocalStore) -> Result<usize> {
    let mut imported = 0usize;
    for prefix in [
        "antigravity",
        "antigravity-ide",
        "antigravity-backup",
        "antigravity-cli",
    ] {
        let path = gemini_home
            .join(prefix)
            .join("conversation_summaries.db");
        if !path.is_file() {
            continue;
        }
        let conn = match Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
            Ok(conn) => conn,
            Err(_) => continue,
        };
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='conversation_summaries')",
            [],
            |r| r.get(0),
        )?;
        if !exists {
            continue;
        }
        let mut stmt =
            conn.prepare("SELECT conversation_id,title FROM conversation_summaries LIMIT 5000")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let title: String = row.get(1)?;
            if id.is_empty() || id.len() > 200 {
                continue;
            }
            let key = stable_record_id("antigravity", b"conversation", id.as_bytes())?;
            if title.trim().is_empty() {
                store.clear_session_name(&key, "antigravity")?;
                continue;
            }
            let name = title.split_whitespace().collect::<Vec<_>>().join(" ");
            if name.is_empty() || name.chars().count() > 200 {
                store.clear_session_name(&key, "antigravity")?;
                continue;
            }
            let name: String = name.chars().take(80).collect();
            if store
                .upsert_session_name(&key, "antigravity", &name, now_ms()?)
                .is_ok()
            {
                imported += 1;
            }
        }
    }
    Ok(imported)
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
fn now_ms() -> Result<i64> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AntigravityError::Invalid("system time before epoch"))?
        .as_millis();
    i64::try_from(ms).map_err(|_| AntigravityError::Invalid("time out of range"))
}
fn dominates(a: &Snapshot, b: &Snapshot) -> bool {
    a.unknown_turns >= b.unknown_turns
        && (b.project_id.is_none() || a.project_id == b.project_id)
        && b.counts
            .iter()
            .all(|(model, count)| a.counts.get(model).copied().unwrap_or_default() >= *count)
        && b.records
            .iter()
            .all(|record| a.records.iter().any(|candidate| candidate == record))
}
fn source_fingerprint(snapshot: &Snapshot, estimates: bool) -> String {
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
fn source_records(
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

/// Native token counters are always retained. Only source rows lacking token
/// counters use estimates, and only after the explicit CLI opt-in. Disabling
/// estimates tombstones previous estimate-only records during full reconcile.
pub fn collect_antigravity(
    gemini_home: &Path,
    store: &mut LocalStore,
    device_id: &str,
    estimate_tokens: bool,
) -> Result<AntigravityReport> {
    collect_antigravity_with_policy(gemini_home, store, device_id, estimate_tokens, false)
}

/// Desktop collection policy: one conversation database with unreadable or
/// oversized metadata is counted in `skipped_sources` and skipped instead of
/// discarding every other conversation.
pub fn collect_antigravity_tolerant(
    gemini_home: &Path,
    store: &mut LocalStore,
    device_id: &str,
    estimate_tokens: bool,
) -> Result<AntigravityReport> {
    collect_antigravity_with_policy(gemini_home, store, device_id, estimate_tokens, true)
}

fn collect_antigravity_with_policy(
    gemini_home: &Path,
    store: &mut LocalStore,
    device_id: &str,
    estimate_tokens: bool,
    skip_broken: bool,
) -> Result<AntigravityReport> {
    let paths = discover_databases(gemini_home)?;
    let projects = project_index(gemini_home);
    let mut report = AntigravityReport {
        source_files: paths.len(),
        ..AntigravityReport::default()
    };
    let mut snapshots: BTreeMap<String, Snapshot> = BTreeMap::new();
    let mut ambiguous: BTreeSet<String> = BTreeSet::new();
    let mut present_source_keys = HashSet::new();
    for path in paths {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or(AntigravityError::Invalid("database filename"))?
            .to_string();
        if stem.is_empty() || stem.len() > 200 || stem.chars().any(char::is_control) {
            return Err(AntigravityError::Invalid("invalid conversation filename"));
        }
        let source_key = stable_record_id("antigravity", b"conversation", stem.as_bytes())?;
        let prefix = path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .and_then(|name| name.to_str());
        let project = prefix
            .and_then(|prefix| projects.get(&(prefix.to_owned(), stem.clone())))
            .cloned();
        if let Some(project) = project.as_ref() {
            if let Some(name) = project.display_name.as_deref() {
                store.upsert_project_name(&project.project_id, name, now_ms()?)?;
            }
        }
        let project_id = project.map(|project| project.project_id);
        present_source_keys.insert(source_key.clone());
        if store.is_session_suppressed("antigravity", &source_key)? {
            continue;
        }
        let snap = match read_db(&path, stem.clone(), device_id, project_id) {
            Ok(snap) => snap,
            Err(_) if skip_broken => {
                report.skipped_sources += 1;
                continue;
            }
            Err(e) => return Err(e),
        };
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
        if ambiguous.contains(&stem) {
            continue;
        }
        report.model_turns = report.model_turns.saturating_add(snapshot.known_turns);
        report.unknown_model_turns = report
            .unknown_model_turns
            .saturating_add(snapshot.unknown_turns);
        report.skipped_rows = report.skipped_rows.saturating_add(snapshot.skipped_rows);
        let source_key = stable_record_id("antigravity", b"conversation", stem.as_bytes())?;
        // Keep the last complete estimate when this explicit-estimate scan
        // encounters unknown models; partial metadata cannot safely revise it.
        if estimate_tokens && snapshot.unknown_turns > 0 {
            store.clear_empty_source_candidate("antigravity", &source_key)?;
            continue;
        }
        let previous = store.checkpoint("antigravity", &source_key)?;
        let fp = source_fingerprint(&snapshot, estimate_tokens);
        let complete_empty = snapshot.records.is_empty()
            && snapshot.known_turns == 0
            && snapshot.unknown_turns == 0
            && snapshot.missing_usage.is_empty();
        if !complete_empty {
            store.clear_empty_source_candidate("antigravity", &source_key)?;
        }
        if previous.as_ref().is_some_and(|p| {
            p.source_fingerprint == fp
                && p.parser_version == PARSER_VERSION
                && p.last_status == "complete"
        }) {
            report.unchanged += 1;
            continue;
        }
        let observed_at = now_ms()?.max(previous.as_ref().map_or(0, |p| p.last_scan_ms));
        let next = SourceCheckpoint {
            tool_id: "antigravity".into(),
            source_key,
            source_fingerprint: fp,
            read_offset: 0,
            parser_version: PARSER_VERSION,
            last_scan_ms: observed_at,
            last_complete_scan_ms: Some(observed_at),
            last_status: "complete".into(),
        };
        let activity = AntigravityActivity {
            known_turns: snapshot.known_turns,
            unknown_turns: snapshot.unknown_turns,
            models: snapshot
                .counts
                .iter()
                .map(|(m, n)| (m.clone(), *n))
                .collect(),
        };
        if let Some(expected) = previous.as_ref() {
            let prior_records = store.records_for_source("antigravity", &next.source_key, 20_000)?;
            let active_prior = prior_records
                .iter()
                .map(|row| &row.record)
                .filter(|record| !record.is_deleted)
                .collect::<Vec<_>>();
            let missing_prior_native = active_prior.iter().any(|record| {
                record.precision != Precision::Estimated
                    && !snapshot
                        .records
                        .iter()
                        .any(|current| current.record_id == record.record_id)
            });
            let empty_but_unproven = snapshot.records.is_empty() && !complete_empty;
            if !active_prior.is_empty() && complete_empty {
                if let Some(result) = store.reconcile_confirmed_empty_source(
                    expected,
                    &next,
                    &activity,
                    observed_at,
                )? {
                    report.tombstoned += result.tombstoned;
                }
                continue;
            }
            if (!active_prior.is_empty() && empty_but_unproven)
                || (!snapshot.missing_usage.is_empty() && missing_prior_native)
            {
                // A tokenless/partial snapshot cannot prove that previously
                // measured generations were deleted. Keep the last complete
                // projection so missing counters cannot create tombstones.
                continue;
            }
        }
        let records = source_records(&snapshot, device_id, estimate_tokens)?;
        report.measured_records += snapshot.records.len();
        report.estimated_records += records.len().saturating_sub(snapshot.records.len());
        let result = store.reconcile_source_with_activity(
            previous.as_ref(),
            &next,
            records,
            observed_at,
            Some(&activity),
        )?;
        report.inserted += result.applied.inserted;
        report.revised += result.applied.revised;
        report.tombstoned += result.tombstoned;
    }
    // Titles live in a separate summaries DB; failing to read them must not
    // fail usage collection.
    let _ = import_titles(gemini_home, store);
    if gemini_home.is_dir() && report.skipped_sources == 0 && ambiguous.is_empty() {
        report.tombstoned += store.reconcile_missing_sources(
            "antigravity",
            &present_source_keys,
            now_ms()?,
        )?;
    }
    Ok(report)
}
