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
use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

pub const MODULE_ID: &str = "source-antigravity";
pub const PARSER_VERSION: u32 = 6;
pub(crate) const MAX_FILES: usize = 20_000;
pub(crate) const MAX_TURNS_PER_DB: u64 = 50_000;
pub(crate) const MAX_MODELS_PER_DB: usize = 512;
// Antigravity writes the full generation metadata (including model and token
// counters) into `gen_metadata`. Long conversations legitimately exceed the
// old 256 KiB guard, and skipping one row used to discard the whole
// conversation. Keep a bounded cap, but leave enough room for real metadata.
pub(crate) const MAX_PROTO_BYTES: usize = 8 * 1024 * 1024;

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
pub(crate) type Result<T> = std::result::Result<T, AntigravityError>;

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

mod collector;
mod metadata;
mod models;
mod protobuf;
mod snapshot;

pub use collector::{collect_antigravity, collect_antigravity_tolerant};
pub use metadata::import_titles;
pub use protobuf::extract_model_name;
pub use snapshot::discover_databases;

pub(crate) fn now_ms() -> Result<i64> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AntigravityError::Invalid("system time before epoch"))?
        .as_millis();
    i64::try_from(ms).map_err(|_| AntigravityError::Invalid("time out of range"))
}
