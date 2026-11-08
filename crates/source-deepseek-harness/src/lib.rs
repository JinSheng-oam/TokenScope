//! Read-only DeepSeek Harness session-log collector.
//!
//! Session JSONL is the source of truth. Compressed logs are decoded as a
//! sequence of Zstandard frames; only settled model calls are counted, with
//! retry replacement and compaction calls handled explicitly. Prompts and raw
//! paths are never stored in usage records.

use std::{
    io,
    time::{SystemTime, UNIX_EPOCH},
};

pub const MODULE_ID: &str = "source-deepseek-harness";
pub const TOOL_ID: &str = "deepseek-harness";
pub const PARSER_VERSION: u32 = 2;

pub(crate) const MAX_SESSION_DIRECTORIES: usize = 100_000;
pub(crate) const MAX_EVENT_ROWS_PER_SESSION: usize = 2_000_000;
pub(crate) const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_RECORDS_PER_SESSION: usize = 100_000;
pub(crate) const TAIL_BYTES: u64 = 256;

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

pub(crate) type Result<T> = std::result::Result<T, DeepSeekHarnessError>;

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

mod collector;
mod inventory;
mod model;
mod parse;

pub use collector::collect_deepseek_harness;

pub(crate) fn now_ms() -> Result<i64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| DeepSeekHarnessError::Invalid("system clock before epoch"))?
        .as_millis();
    i64::try_from(millis).map_err(|_| DeepSeekHarnessError::Limit("timestamp range"))
}
