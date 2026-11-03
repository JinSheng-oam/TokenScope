//! Read-only DeepSeek Harness session-log collector.
//!
//! Session JSONL is the source of truth. Compressed logs are decoded as a
//! sequence of Zstandard frames; only settled model calls are counted, with
//! retry replacement and compaction calls handled explicitly. Prompts and raw
//! paths are never stored in usage records.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File, Metadata};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokenscope_core_domain::{
    project_key, session_key, stable_record_id, CanonicalRecord, Precision, TokenBreakdown,
    UsageScope,
};
use tokenscope_local_store::{LocalStore, SourceCheckpoint};
use zstd::stream::read::Decoder;

pub const MODULE_ID: &str = "source-deepseek-harness";
pub const TOOL_ID: &str = "deepseek-harness";
pub const PARSER_VERSION: u32 = 1;

const MAX_SESSION_DIRECTORIES: usize = 100_000;
const MAX_EVENT_ROWS_PER_SESSION: usize = 2_000_000;
const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
const MAX_RECORDS_PER_SESSION: usize = 100_000;
const TAIL_BYTES: u64 = 256;

#[derive(Debug)]
pub enum DeepSeekHarnessError {
    Io(io::Error),
    Json(serde_json::Error),
    Store(tokenscope_local_store::StoreError),
    Domain(tokenscope_core_domain::DomainError),
    Invalid(&'static str),
    Limit(&'static str),
    Pending,
}

impl std::fmt::Display for DeepSeekHarnessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error ({:?})", error.kind()),
            Self::Json(_) => write!(f, "invalid session JSONL"),
            Self::Store(error) => write!(f, "local store: {error}"),
            Self::Domain(error) => write!(f, "record validation: {error}"),
            Self::Invalid(reason) => write!(f, "invalid session source ({reason})"),
            Self::Limit(reason) => write!(f, "session resource limit ({reason})"),
            Self::Pending => write!(f, "session changed during scan"),
        }
    }
}

impl std::error::Error for DeepSeekHarnessError {}

impl From<io::Error> for DeepSeekHarnessError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for DeepSeekHarnessError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<tokenscope_local_store::StoreError> for DeepSeekHarnessError {
    fn from(error: tokenscope_local_store::StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<tokenscope_core_domain::DomainError> for DeepSeekHarnessError {
    fn from(error: tokenscope_core_domain::DomainError) -> Self {
        Self::Domain(error)
    }
}

type Result<T> = std::result::Result<T, DeepSeekHarnessError>;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DeepSeekHarnessReport {
    pub source_files: usize,
    pub sessions: usize,
    pub unchanged: usize,
    pub pending: usize,
    pub skipped: usize,
    pub model_calls: usize,
    pub inserted: usize,
    pub revised: usize,
    pub deleted: usize,
}

#[derive(Clone, Debug)]
struct LogCandidate {
    path: PathBuf,
    generation: u32,
    compressed: bool,
}

#[derive(Debug, Default)]
struct Inventory {
    available: bool,
    complete: bool,
    paths: Vec<LogCandidate>,
    skipped: usize,
}

#[derive(Clone, Debug)]
struct SessionHeader {
    version: u64,
    id: String,
    cwd: Option<String>,
    created_at_ms: Option<i64>,
    is_seeded: bool,
    seed_length: Option<u64>,
}

#[derive(Clone, Debug)]
struct ModelRoute {
    provider: String,
    model: String,
}

impl ModelRoute {
    fn model_id(&self) -> Option<String> {
        let combined = format!("{}/{}", self.provider, self.model);
        (combined.len() <= 256
            && !combined.trim().is_empty()
            && !combined.chars().any(char::is_control))
        .then_some(combined)
    }
}

#[derive(Clone)]
struct IndexedRecord {
    source_seq: u64,
    record: CanonicalRecord,
}

#[derive(Clone, Copy)]
struct ReplacementSlot {
    turn: u64,
    step: u64,
    record_index: usize,
    source_seq: u64,
    legacy_chunk: bool,
}

#[derive(Default)]
struct UsageFold {
    records: Vec<IndexedRecord>,
    last_slot: Option<ReplacementSlot>,
}

struct ParsedSession {
    title: Option<String>,
    records: Vec<CanonicalRecord>,
    model_calls: usize,
}

fn parse_generation(name: &str) -> Option<(u32, bool)> {
    let (stem, compressed) = if let Some(stem) = name.strip_suffix(".jsonl.zstd") {
        (stem, true)
    } else if let Some(stem) = name.strip_suffix(".jsonl") {
        (stem, false)
    } else {
        return None;
    };
    if stem == "session" {
        return Some((0, compressed));
    }
    let digits = stem.strip_prefix("session.v")?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let generation = digits.parse::<u32>().ok()?;
    (generation > 0).then_some((generation, compressed))
}

fn discover_sessions(home: &Path) -> Result<Inventory> {
    let sessions_root = home.join("sessions");
    let root_metadata = match fs::symlink_metadata(&sessions_root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Inventory::default()),
        Err(error) => return Err(error.into()),
    };
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Ok(Inventory::default());
    }

    let mut inventory = Inventory {
        available: true,
        complete: true,
        ..Inventory::default()
    };
    let mut directory_count = 0usize;
    for project_entry in fs::read_dir(&sessions_root)? {
        let project_entry = project_entry?;
        let project_type = project_entry.file_type()?;
        if project_type.is_symlink() {
            inventory.complete = false;
            inventory.skipped = inventory.skipped.saturating_add(1);
            continue;
        }
        if !project_type.is_dir() {
            continue;
        }
        for session_entry in fs::read_dir(project_entry.path())? {
            let session_entry = session_entry?;
            let session_type = session_entry.file_type()?;
            if session_type.is_symlink() {
                inventory.complete = false;
                inventory.skipped = inventory.skipped.saturating_add(1);
                continue;
            }
            if !session_type.is_dir() {
                continue;
            }
            directory_count = directory_count.saturating_add(1);
            if directory_count > MAX_SESSION_DIRECTORIES {
                return Err(DeepSeekHarnessError::Limit("too many session directories"));
            }

            let mut candidates = Vec::new();
            for file_entry in fs::read_dir(session_entry.path())? {
                let file_entry = file_entry?;
                let file_type = file_entry.file_type()?;
                if file_type.is_symlink() {
                    inventory.complete = false;
                    inventory.skipped = inventory.skipped.saturating_add(1);
                    continue;
                }
                if !file_type.is_file() {
                    continue;
                }
                let Some(name) = file_entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                let Some((generation, compressed)) = parse_generation(&name) else {
                    continue;
                };
                candidates.push(LogCandidate {
                    path: file_entry.path(),
                    generation,
                    compressed,
                });
            }
            if candidates.is_empty() {
                continue;
            }
            let latest_generation = candidates
                .iter()
                .map(|candidate| candidate.generation)
                .max()
                .unwrap_or_default();
            let mut latest = candidates
                .into_iter()
                .filter(|candidate| candidate.generation == latest_generation);
            let Some(candidate) = latest.next() else {
                continue;
            };
            if latest.next().is_some() {
                // A root must have one physical encoding per generation.
                inventory.complete = false;
                inventory.skipped = inventory.skipped.saturating_add(1);
                continue;
            }
            inventory.paths.push(candidate);
        }
    }
    if inventory.paths.len() > MAX_SESSION_DIRECTORIES {
        return Err(DeepSeekHarnessError::Limit("too many session logs"));
    }
    inventory
        .paths
        .sort_by(|left, right| left.path.cmp(&right.path));
    Ok(inventory)
}

fn open_log_reader(path: &Path, compressed: bool) -> Result<Box<dyn Read>> {
    let file = File::open(path)?;
    if compressed {
        // The default streaming Decoder consumes concatenated frames, which is
        // how Harness appends each durable flush to a compressed session log.
        Ok(Box::new(Decoder::new(file)?))
    } else {
        Ok(Box::new(file))
    }
}

fn bounded_line<R: BufRead>(reader: &mut R) -> Result<Option<(Vec<u8>, bool)>> {
    let mut output = Vec::new();
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return if output.is_empty() {
                Ok(None)
            } else {
                Ok(Some((output, false)))
            };
        }
        let until = chunk
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(chunk.len(), |index| index + 1);
        if output
            .len()
            .checked_add(until)
            .is_none_or(|length| length > MAX_LINE_BYTES)
        {
            return Err(DeepSeekHarnessError::Limit("JSONL row exceeds 16 MiB"));
        }
        let complete = chunk[until - 1] == b'\n';
        output.extend_from_slice(&chunk[..until]);
        reader.consume(until);
        if complete {
            return Ok(Some((output, true)));
        }
    }
}

fn json_line(mut line: Vec<u8>) -> Result<Value> {
    while matches!(line.last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    Ok(serde_json::from_slice(&line)?)
}

fn json_time(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
        .filter(|time| *time >= 0)
}

fn parse_header(value: &Value) -> Result<SessionHeader> {
    if value.get("type").and_then(Value::as_str) != Some("session") {
        return Err(DeepSeekHarnessError::Invalid("missing session header"));
    }
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 200 && !id.chars().any(char::is_control))
        .ok_or(DeepSeekHarnessError::Invalid("session identity"))?
        .to_owned();
    let cwd = value
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|cwd| cwd.len() <= 4096 && !cwd.chars().any(char::is_control))
        .map(str::to_owned);
    let seed_length = value.get("seedLength").and_then(Value::as_u64);
    Ok(SessionHeader {
        version: value.get("version").and_then(Value::as_u64).unwrap_or(0),
        id,
        cwd,
        created_at_ms: json_time(value.get("createdAt")),
        is_seeded: value
            .get("isSeeded")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        seed_length,
    })
}

fn normalized_path_identity(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
}

fn project_metadata(cwd: &str) -> Option<(String, String)> {
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
    let name = root.file_name()?.to_str()?.trim();
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
    let project_id = project_key(TOOL_ID, normalized_path_identity(root).as_bytes()).ok()?;
    Some((project_id, name.to_owned()))
}

fn clean_title(value: &str) -> Option<String> {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() || collapsed.chars().any(char::is_control) {
        return None;
    }
    Some(collapsed.chars().take(80).collect())
}

fn user_message_title(data: &Value) -> Option<String> {
    let message = data.get("message").unwrap_or(data);
    if message
        .get("source")
        .and_then(|source| source.get("kind"))
        .and_then(Value::as_str)
        != Some("user")
    {
        return None;
    }
    let content = message.get("content")?;
    if let Some(text) = content.as_str() {
        return clean_title(text);
    }
    content
        .as_array()?
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .find_map(|block| {
            block
                .get("text")
                .and_then(Value::as_str)
                .and_then(clean_title)
        })
}

fn route_from_config(value: &Value) -> Option<ModelRoute> {
    let provider = value
        .get("provider")
        .and_then(Value::as_str)
        .filter(|provider| {
            !provider.trim().is_empty()
                && provider.len() <= 128
                && !provider.chars().any(char::is_control)
        })?;
    let model = value.get("model").and_then(Value::as_str).filter(|model| {
        !model.trim().is_empty() && model.len() <= 256 && !model.chars().any(char::is_control)
    })?;
    Some(ModelRoute {
        provider: provider.to_owned(),
        model: model.to_owned(),
    })
}

fn route_from_message(data: &Value) -> Option<ModelRoute> {
    let message = data.get("message")?;
    let source = message.get("source")?;
    route_from_config(source)
}

fn route_from_event(event_type: &str, data: &Value) -> Option<ModelRoute> {
    match event_type {
        "request/context" => route_from_config(data),
        "request/header" => data
            .get("header")
            .and_then(|header| header.get("config"))
            .and_then(route_from_config),
        _ => None,
    }
}

fn parse_counter(usage: &Value, name: &str, required: bool) -> Result<u64> {
    match usage.get(name) {
        Some(value) if value.is_null() && !required => Ok(0),
        Some(value) => value
            .as_u64()
            .ok_or(DeepSeekHarnessError::Invalid("invalid token counter")),
        None if required => Err(DeepSeekHarnessError::Invalid("missing token counter")),
        None => Ok(0),
    }
}

fn parse_usage(usage: &Value) -> Result<TokenBreakdown> {
    let result = TokenBreakdown {
        input_uncached_tokens: parse_counter(usage, "inputTokens", true)?,
        cache_read_tokens: parse_counter(usage, "cacheReadTokens", false)?,
        cache_write_tokens: parse_counter(usage, "cacheWriteTokens", false)?,
        output_tokens: parse_counter(usage, "outputTokens", true)?,
        unclassified_tokens: 0,
        reasoning_tokens: usage
            .get("reasoningTokens")
            .filter(|value| !value.is_null())
            .map(|value| {
                value
                    .as_u64()
                    .ok_or(DeepSeekHarnessError::Invalid("invalid reasoning counter"))
            })
            .transpose()?,
    };
    result.validate()?;
    if result.total()? > i64::MAX as u64 {
        return Err(DeepSeekHarnessError::Limit(
            "tokens exceed SQLite integer range",
        ));
    }
    Ok(result)
}

fn last_stream_usage(data: &Value) -> Result<Option<(TokenBreakdown, Option<i64>)>> {
    let Some(stream) = data.get("stream").and_then(Value::as_array) else {
        return Ok(None);
    };
    for record in stream.iter().rev() {
        let chunk = if record.get("type").and_then(Value::as_str) == Some("chunk") {
            record.get("chunk")
        } else if record.get("type").and_then(Value::as_str) == Some("usage") {
            Some(record)
        } else {
            None
        };
        let Some(chunk) =
            chunk.filter(|chunk| chunk.get("type").and_then(Value::as_str) == Some("usage"))
        else {
            continue;
        };
        let usage = chunk
            .get("usage")
            .ok_or(DeepSeekHarnessError::Invalid("usage chunk lacks counters"))?;
        return Ok(Some((parse_usage(usage)?, json_time(record.get("time")))));
    }
    Ok(None)
}

fn settlement_usage(
    event_type: &str,
    data: &Value,
) -> Result<Option<(TokenBreakdown, Option<i64>)>> {
    if event_type == "assistant/message" {
        if let Some(usage) = data.get("usage").filter(|usage| !usage.is_null()) {
            return Ok(Some((parse_usage(usage)?, None)));
        }
    }
    last_stream_usage(data)
}

fn event_sequence(value: &Value, fallback: u64) -> u64 {
    value
        .get("seq")
        .or_else(|| value.get("seq0"))
        .and_then(Value::as_u64)
        .unwrap_or(fallback)
}

fn make_record(
    device_id: &str,
    session_key_value: &str,
    project_id: Option<&str>,
    model_id: Option<String>,
    event_at_ms: Option<i64>,
    usage: TokenBreakdown,
    native_identity: &[u8],
) -> Result<CanonicalRecord> {
    Ok(CanonicalRecord {
        record_id: stable_record_id(TOOL_ID, session_key_value.as_bytes(), native_identity)?,
        owner_device_id: device_id.to_owned(),
        tool_id: TOOL_ID.to_owned(),
        scope: UsageScope::DeviceLocal,
        precision: Precision::Exact,
        account_id: None,
        model_id,
        project_id: project_id.map(str::to_owned),
        session_id: Some(session_key_value.to_owned()),
        event_at_ms,
        usage,
        estimated_cost_microusd: None,
        price_version: None,
        is_deleted: false,
    }
    .normalized()?)
}

impl UsageFold {
    fn push_settlement(
        &mut self,
        session_key_value: &str,
        device_id: &str,
        project_id: Option<&str>,
        model_id: Option<String>,
        event_at_ms: Option<i64>,
        usage: TokenBreakdown,
        turn: u64,
        step: u64,
        sequence: u64,
        legacy_chunk: bool,
    ) -> Result<()> {
        let (record_index, source_seq, native_identity) = match self.last_slot {
            Some(slot) if slot.turn == turn && slot.step == step => (
                slot.record_index,
                slot.source_seq,
                format!("assistant-settlement:{}:{}", turn, slot.source_seq).into_bytes(),
            ),
            _ => (
                self.records.len(),
                sequence,
                format!("assistant-settlement:{turn}:{sequence}").into_bytes(),
            ),
        };
        let record = make_record(
            device_id,
            session_key_value,
            project_id,
            model_id,
            event_at_ms,
            usage,
            &native_identity,
        )?;
        if record_index == self.records.len() {
            if self.records.len() >= MAX_RECORDS_PER_SESSION {
                return Err(DeepSeekHarnessError::Limit("usage rows per session"));
            }
            self.records.push(IndexedRecord { source_seq, record });
        } else {
            self.records[record_index].record = record;
        }
        self.last_slot = Some(ReplacementSlot {
            turn,
            step,
            record_index,
            source_seq,
            legacy_chunk,
        });
        Ok(())
    }

    fn update_last_metadata(
        &mut self,
        turn: u64,
        step: u64,
        model_id: Option<String>,
        event_at_ms: Option<i64>,
    ) {
        let Some(slot) = self
            .last_slot
            .filter(|slot| slot.turn == turn && slot.step == step)
        else {
            return;
        };
        let record = &mut self.records[slot.record_index].record;
        if model_id.is_some() {
            record.model_id = model_id;
        }
        if event_at_ms.is_some() {
            record.event_at_ms = event_at_ms;
        }
    }

    fn push_independent(
        &mut self,
        session_key_value: &str,
        device_id: &str,
        project_id: Option<&str>,
        model_id: Option<String>,
        event_at_ms: Option<i64>,
        usage: TokenBreakdown,
        sequence: u64,
    ) -> Result<()> {
        if self.records.len() >= MAX_RECORDS_PER_SESSION {
            return Err(DeepSeekHarnessError::Limit("usage rows per session"));
        }
        let native_identity = format!("independent-call:{sequence}");
        let record = make_record(
            device_id,
            session_key_value,
            project_id,
            model_id,
            event_at_ms,
            usage,
            native_identity.as_bytes(),
        )?;
        self.records.push(IndexedRecord {
            source_seq: sequence,
            record,
        });
        Ok(())
    }
}

fn parse_session(
    path: &Path,
    compressed: bool,
    expected_id: &str,
    session_key_value: &str,
    device_id: &str,
    project_id: Option<&str>,
) -> Result<ParsedSession> {
    let mut reader = BufReader::new(open_log_reader(path, compressed)?);
    let mut line_number = 0usize;
    let mut event_rows = 0usize;
    let mut fallback_sequence = 0u64;
    let mut header = None;
    let mut title = None;
    let mut route: Option<ModelRoute> = None;
    let mut fold = UsageFold::default();
    let mut own_start_sequence = 0u64;
    let mut seed_boundary_seen = false;
    let mut format_version = 0u64;

    while let Some((line, complete)) = bounded_line(&mut reader)? {
        if !complete {
            return Err(DeepSeekHarnessError::Pending);
        }
        let value = json_line(line)?;
        if line_number == 0 {
            let parsed_header = parse_header(&value)?;
            if parsed_header.id != expected_id {
                return Err(DeepSeekHarnessError::Invalid("session identity changed"));
            }
            format_version = parsed_header.version;
            if let Some(seed_length) = parsed_header.seed_length {
                own_start_sequence = seed_length;
                seed_boundary_seen = true;
            }
            if !parsed_header.is_seeded {
                seed_boundary_seen = true;
            }
            header = Some(parsed_header);
            line_number += 1;
            continue;
        }
        line_number = line_number.saturating_add(1);
        event_rows = event_rows.saturating_add(1);
        if event_rows > MAX_EVENT_ROWS_PER_SESSION {
            return Err(DeepSeekHarnessError::Limit("event rows per session"));
        }
        let sequence = event_sequence(&value, fallback_sequence);
        fallback_sequence = sequence.saturating_add(1);
        let event_type = value.get("type").and_then(Value::as_str).unwrap_or("");
        let data = value.get("data").unwrap_or(&Value::Null);
        let event_time = json_time(value.get("time"))
            .or_else(|| header.as_ref().and_then(|header| header.created_at_ms));

        if let Some(next_route) = route_from_event(event_type, data) {
            route = Some(next_route);
        }
        if event_type == "session/end-seed"
            && data.get("inherited").and_then(Value::as_bool) == Some(true)
        {
            own_start_sequence = sequence;
            seed_boundary_seen = true;
            fold.last_slot = None;
            continue;
        }
        if title.is_none() && event_type == "user/message" {
            title = user_message_title(data);
        }
        if event_type == "llm/retry-started" || event_type == "llm/retry" {
            let turn = data.get("turn").and_then(Value::as_u64);
            let step = data.get("step").and_then(Value::as_u64);
            if fold
                .last_slot
                .is_some_and(|slot| Some(slot.turn) == turn && Some(slot.step) == step)
            {
                fold.last_slot = None;
            }
            continue;
        }

        if sequence < own_start_sequence {
            continue;
        }
        match event_type {
            "assistant/chunk" if format_version < 2 => {
                let Some(chunk) = data
                    .get("chunk")
                    .filter(|chunk| chunk.get("type").and_then(Value::as_str) == Some("usage"))
                else {
                    continue;
                };
                let Some(usage_value) = chunk.get("usage") else {
                    return Err(DeepSeekHarnessError::Invalid("usage chunk lacks counters"));
                };
                let turn = data
                    .get("turn")
                    .and_then(Value::as_u64)
                    .ok_or(DeepSeekHarnessError::Invalid("assistant chunk turn"))?;
                let step = data
                    .get("step")
                    .and_then(Value::as_u64)
                    .ok_or(DeepSeekHarnessError::Invalid("assistant chunk step"))?;
                let model_id = route.clone().and_then(|route| route.model_id());
                fold.push_settlement(
                    session_key_value,
                    device_id,
                    project_id,
                    model_id,
                    event_time,
                    parse_usage(usage_value)?,
                    turn,
                    step,
                    sequence,
                    true,
                )?;
            }
            "assistant/message" | "assistant/attempt" => {
                let turn = data
                    .get("turn")
                    .and_then(Value::as_u64)
                    .ok_or(DeepSeekHarnessError::Invalid("assistant settlement turn"))?;
                let step = data
                    .get("step")
                    .and_then(Value::as_u64)
                    .ok_or(DeepSeekHarnessError::Invalid("assistant settlement step"))?;
                let message_route = (event_type == "assistant/message")
                    .then(|| route_from_message(data))
                    .flatten();
                let model_id = message_route
                    .or_else(|| route.clone())
                    .and_then(|route| route.model_id());
                if event_type == "assistant/message"
                    && format_version < 2
                    && fold.last_slot.is_some_and(|slot| {
                        slot.turn == turn && slot.step == step && slot.legacy_chunk
                    })
                {
                    // Legacy logs store raw usage chunks; message.usage is a
                    // fallback for steps that produced no usage chunk.
                    fold.update_last_metadata(turn, step, model_id, event_time);
                    continue;
                }
                let Some((usage, stream_time)) = settlement_usage(event_type, data)? else {
                    continue;
                };
                fold.push_settlement(
                    session_key_value,
                    device_id,
                    project_id,
                    model_id,
                    stream_time.or(event_time),
                    usage,
                    turn,
                    step,
                    sequence,
                    false,
                )?;
            }
            "compaction/summary" => {
                let Some(usage_value) = data.get("usage").filter(|value| !value.is_null()) else {
                    continue;
                };
                let usage = parse_usage(usage_value)?;
                let model_id = route_from_config(data).and_then(|route| route.model_id());
                fold.push_independent(
                    session_key_value,
                    device_id,
                    project_id,
                    model_id,
                    event_time,
                    usage,
                    sequence,
                )?;
                // A compaction summary is an independent provider call. The
                // following assistant settlement must not replace a sample
                // from an earlier request in the same turn and step.
                fold.last_slot = None;
            }
            _ => {}
        }
    }

    let header = header.ok_or(DeepSeekHarnessError::Invalid("empty session log"))?;
    if header.is_seeded && !seed_boundary_seen {
        return Err(DeepSeekHarnessError::Invalid("fork seed boundary missing"));
    }
    let mut records: Vec<_> = fold
        .records
        .into_iter()
        .filter(|record| record.source_seq >= own_start_sequence)
        .map(|record| record.record)
        .collect();
    records.sort_by(|left, right| left.record_id.cmp(&right.record_id));
    let model_calls = records.len();
    Ok(ParsedSession {
        title,
        records,
        model_calls,
    })
}

fn modified_ns(metadata: &Metadata) -> Result<u128> {
    metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|_| DeepSeekHarnessError::Invalid("file time before epoch"))
}

fn fingerprint(path: &Path, metadata: &Metadata, compressed: bool) -> Result<String> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(metadata.len().saturating_sub(TAIL_BYTES)))?;
    let mut tail = Vec::with_capacity(TAIL_BYTES as usize);
    file.take(TAIL_BYTES).read_to_end(&mut tail)?;
    Ok(format!(
        "v1:{}:{}:{}:{:x}",
        u8::from(compressed),
        metadata.len(),
        modified_ns(metadata)?,
        Sha256::digest(tail)
    ))
}

fn current_metadata(
    path: &Path,
    before: &Metadata,
    fingerprint_before: &str,
    compressed: bool,
) -> Result<bool> {
    let after = fs::symlink_metadata(path)?;
    if !after.file_type().is_file() || after.file_type().is_symlink() {
        return Ok(false);
    }
    if after.len() != before.len() || modified_ns(&after)? != modified_ns(before)? {
        return Ok(false);
    }
    Ok(fingerprint(path, &after, compressed)? == fingerprint_before)
}

fn now_ms() -> Result<i64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| DeepSeekHarnessError::Invalid("system clock before epoch"))?
        .as_millis();
    i64::try_from(millis).map_err(|_| DeepSeekHarnessError::Limit("timestamp range"))
}

/// Scans one configured `DSH_HOME`. Missing or unreadable individual sessions
/// retain their last complete projection; only a stable full inventory may
/// tombstone sessions removed by the source application.
pub fn collect_deepseek_harness(
    home: &Path,
    store: &mut LocalStore,
    device_id: &str,
) -> Result<DeepSeekHarnessReport> {
    let inventory = discover_sessions(home)?;
    let mut report = DeepSeekHarnessReport {
        source_files: inventory.paths.len(),
        skipped: inventory.skipped,
        ..DeepSeekHarnessReport::default()
    };
    if !inventory.available {
        return Ok(report);
    }

    let mut present_sources = HashSet::new();
    let mut inventory_complete = inventory.complete;
    for candidate in inventory.paths {
        let before = match fs::symlink_metadata(&candidate.path) {
            Ok(metadata)
                if metadata.file_type().is_file() && !metadata.file_type().is_symlink() =>
            {
                metadata
            }
            Ok(_) => {
                report.skipped += 1;
                inventory_complete = false;
                continue;
            }
            Err(_) => {
                report.pending += 1;
                inventory_complete = false;
                continue;
            }
        };
        let fingerprint = match fingerprint(&candidate.path, &before, candidate.compressed) {
            Ok(fingerprint) => fingerprint,
            Err(_) => {
                report.pending += 1;
                inventory_complete = false;
                continue;
            }
        };
        let header = match read_session_header(&candidate.path, candidate.compressed) {
            Ok(header) => header,
            Err(DeepSeekHarnessError::Pending) => {
                report.pending += 1;
                inventory_complete = false;
                continue;
            }
            Err(_) => {
                report.skipped += 1;
                inventory_complete = false;
                continue;
            }
        };
        let source_key = session_key(TOOL_ID, header.id.as_bytes())?;
        present_sources.insert(source_key.clone());
        if store.is_session_suppressed(TOOL_ID, &source_key)? {
            continue;
        }

        let project_id = if let Some(cwd) = header.cwd.as_deref() {
            if let Some((project_id, name)) = project_metadata(cwd) {
                store.upsert_project_name(&project_id, &name, now_ms()?)?;
                Some(project_id)
            } else {
                None
            }
        } else {
            None
        };
        let previous = store.checkpoint(TOOL_ID, &source_key)?;
        if previous.as_ref().is_some_and(|checkpoint| {
            checkpoint.parser_version == PARSER_VERSION
                && checkpoint.source_fingerprint == fingerprint
                && checkpoint.last_status == "complete"
        }) {
            if current_metadata(&candidate.path, &before, &fingerprint, candidate.compressed)? {
                report.sessions += 1;
                report.unchanged += 1;
                continue;
            }
            report.pending += 1;
            inventory_complete = false;
            continue;
        }

        let parsed = match parse_session(
            &candidate.path,
            candidate.compressed,
            &header.id,
            &source_key,
            device_id,
            project_id.as_deref(),
        ) {
            Ok(parsed) => parsed,
            Err(DeepSeekHarnessError::Pending) => {
                report.pending += 1;
                inventory_complete = false;
                continue;
            }
            Err(_) => {
                report.skipped += 1;
                inventory_complete = false;
                continue;
            }
        };
        if !current_metadata(&candidate.path, &before, &fingerprint, candidate.compressed)? {
            report.pending += 1;
            inventory_complete = false;
            continue;
        }

        if let Some(title) = parsed.title.as_deref() {
            store.upsert_session_name(&source_key, TOOL_ID, title, now_ms()?)?;
        }
        let observed_at_ms = now_ms()?;
        let last_scan_ms = previous.as_ref().map_or(observed_at_ms, |checkpoint| {
            observed_at_ms.max(checkpoint.last_scan_ms.saturating_add(1))
        });
        let checkpoint = SourceCheckpoint {
            tool_id: TOOL_ID.to_owned(),
            source_key: source_key.clone(),
            source_fingerprint: fingerprint,
            read_offset: before.len(),
            parser_version: PARSER_VERSION,
            last_scan_ms,
            last_complete_scan_ms: Some(last_scan_ms),
            last_status: "complete".into(),
        };
        let reconciled = store.reconcile_complete_source(
            previous.as_ref(),
            &checkpoint,
            parsed.records,
            last_scan_ms,
        )?;
        report.sessions += 1;
        report.model_calls = report.model_calls.saturating_add(parsed.model_calls);
        report.inserted += reconciled.applied.inserted;
        report.revised += reconciled.applied.revised;
    }

    if inventory_complete && report.pending == 0 && report.skipped == 0 {
        report.deleted = store.reconcile_missing_sources(TOOL_ID, &present_sources, now_ms()?)?;
    }
    Ok(report)
}

fn read_session_header(path: &Path, compressed: bool) -> Result<SessionHeader> {
    let mut reader = BufReader::new(open_log_reader(path, compressed)?);
    let (line, complete) =
        bounded_line(&mut reader)?.ok_or(DeepSeekHarnessError::Invalid("empty session log"))?;
    if !complete {
        return Err(DeepSeekHarnessError::Pending);
    }
    parse_header(&json_line(line)?)
}
