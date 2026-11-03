use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io;
use tokenscope_core_domain::{CanonicalRecord, TokenBreakdown};
use tokenscope_local_store::SourceCheckpoint;

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
pub(crate) type Result<T> = std::result::Result<T, CodexError>;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Counters {
    pub(crate) input: u64,
    pub(crate) cached: u64,
    pub(crate) output: u64,
    pub(crate) reasoning: Option<u64>,
    pub(crate) total: u64,
}
impl Counters {
    pub(crate) fn from_info(info: &Value) -> Result<Option<Self>> {
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
    pub(crate) fn monotonic_from(&self, prev: &Self) -> bool {
        self.input >= prev.input
            && self.cached >= prev.cached
            && self.output >= prev.output
            && self.total >= prev.total
            && (match (self.reasoning, prev.reasoning) {
                (Some(a), Some(b)) => a >= b,
                _ => true,
            })
    }
    pub(crate) fn breakdown(self) -> TokenBreakdown {
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
pub(crate) struct CursorState {
    pub(crate) path_hash: String,
    pub(crate) tail_hash: String,
    pub(crate) mtime_ns: u128,
    pub(crate) counters: Option<Counters>,
    pub(crate) event_at_ms: Option<i64>,
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

#[derive(Debug, Default)]
pub struct CollectReport {
    pub scanned_files: usize,
    pub inserted: usize,
    pub revised: usize,
    pub unchanged: usize,
    pub parsed_lines: usize,
    pub pending_lines: usize,
}
